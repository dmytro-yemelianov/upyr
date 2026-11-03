#[cfg(target_os = "macos")]
use std::fs;

use anyhow::{Context, Result, bail};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use single_instance::SingleInstance;
use tracing::{debug, error, info, warn};
#[cfg(not(target_os = "linux"))]
use tray_icon::menu::MenuEvent;
use upyr_audio::KeyCue;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::WindowId,
};

#[cfg(target_os = "macos")]
use crate::accessibility::{self, AccessibilityWatcher};
#[cfg(target_os = "macos")]
use crate::automation::{
    ConversionTask, FocusTarget, Progress as ConversionProgress, Request as ConversionRequest,
};
#[cfg(not(target_os = "macos"))]
use crate::automation::{
    convert_previous_input_if_matches, convert_previous_word, convert_selection,
};
use crate::{
    auto_correct::{AutoCorrection, AutoDecision, AutoKeyEvent, AutoWordTracker},
    auto_correct_monitor::AutoCorrectMonitor,
    automation::SelectionOutcome,
    autostart,
    config::{Config, GestureAction, SoundEvent, config_path},
    config_watcher::ConfigWatcher,
    modifier_gesture::ModifierGestureMonitor,
    settings, system_layout,
    tray::{TRAY_FLIP_FRAME_COUNT, TRAY_FLIP_FRAME_DELAY_MS, Tray, TrayAction},
};

#[derive(Debug)]
enum AppEvent {
    HotKey(GlobalHotKeyEvent),
    #[cfg(target_os = "macos")]
    UserActivity(crate::auto_correct_monitor::ActivityEvent),
    #[cfg(not(target_os = "linux"))]
    Menu(MenuEvent),
    #[cfg(target_os = "linux")]
    Tray(TrayAction),
    ModifierGesture(GestureAction),
    AutoKey(AutoKeyEvent),
    KeySound(KeyCue),
    ApplyAutoCorrection {
        generation: u64,
        correction: AutoCorrection,
    },
    ReloadConfiguration,
    HideLayoutIndicator(u64),
    TrayFlipFrame {
        generation: u64,
        frame: u8,
    },
    #[cfg(target_os = "macos")]
    AccessibilityGranted,
    /// The target's tap holds an Enter behind the pending correction.
    #[cfg(target_os = "macos")]
    LineBreakHeld(crate::automation::HeldLineBreak),
    #[cfg(target_os = "macos")]
    GlobeSwitched(Option<String>, system_layout::ActiveInputSource),
}

/// A correction armed for the word being typed, applied if Enter ends it.
#[cfg(target_os = "macos")]
#[derive(Default)]
struct LineBreak {
    next_id: u64,
    pending: Option<PendingLineBreak>,
}

#[cfg(target_os = "macos")]
struct PendingLineBreak {
    id: u64,
    correction: AutoCorrection,
    /// Enter was seen; the tap's report for it may still be on its way.
    entered_at: Option<std::time::Instant>,
}

struct App {
    #[cfg(target_os = "macos")]
    conversion: Option<PendingConversion>,
    #[cfg(target_os = "macos")]
    line_break: LineBreak,
    /// Bundle identifier of the app last typed in, named by the tray item.
    #[cfg(target_os = "macos")]
    typing_app: Option<String>,
    #[cfg(target_os = "macos")]
    queued_conversion: Option<PendingConversion>,
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
    last_word_hotkey: HotKey,
    /// Optional transliterate / change-case shortcuts.
    transform_hotkeys: Vec<(upyr_core::TextTransform, HotKey)>,
    /// `upyr run --hotkey`, reapplied on top of every reloaded configuration.
    hotkey_override: Option<String>,
    #[cfg(target_os = "macos")]
    undo_hotkey: HotKey,
    #[cfg(target_os = "macos")]
    undo_record: Option<UndoRecord>,
    #[cfg(target_os = "macos")]
    pending_undo: Option<PendingUndo>,
    /// Counts the user's own fixes until one repeats (see `learning`).
    #[cfg(target_os = "macos")]
    learner: crate::learning::Learner,
    config: Config,
    event_proxy: EventLoopProxy<AppEvent>,
    gesture: Option<ModifierGestureMonitor>,
    auto_monitor: Option<AutoCorrectMonitor>,
    auto_tracker: AutoWordTracker,
    #[cfg(target_os = "macos")]
    auto_target: Option<FocusTarget>,
    #[cfg(target_os = "macos")]
    slash_tracker: crate::slash_commands::SlashTracker,
    #[cfg(target_os = "macos")]
    slash_target: Option<FocusTarget>,
    _config_watcher: ConfigWatcher,
    tray: Option<Tray>,
    processing: bool,
    paused: bool,
    auto_generation: u64,
    feedback_generation: u64,
    tray_flip_generation: u64,
    #[cfg(target_os = "macos")]
    accessibility_watcher: Option<AccessibilityWatcher>,
    #[cfg(target_os = "macos")]
    accessibility_blocked: bool,
    #[cfg(target_os = "macos")]
    restart_prompt_scheduled: bool,
    #[cfg(target_os = "macos")]
    restart_prompt_shown: bool,
    #[cfg(target_os = "macos")]
    globe_monitor: Option<crate::globe_switch::GlobeMonitor>,
    #[cfg(target_os = "macos")]
    globe_generation: u64,
}

#[cfg(target_os = "macos")]
struct PendingConversion {
    task: ConversionTask,
    automatic_generation: Option<u64>,
    undo_candidate: Option<UndoRecord>,
    /// For an undo: the text as typed that it restores.
    reverted: Option<String>,
}

#[cfg(target_os = "macos")]
struct UndoRecord {
    original: String,
    corrected: String,
    source_id: String,
    direction: crate::layout::Direction,
    target: FocusTarget,
    expires: std::time::Instant,
}

#[cfg(target_os = "macos")]
impl UndoRecord {
    /// The same undo after a Backspace removed the correction's last
    /// character (the space or punctuation that ended the word): the word is
    /// restored without it. None when nothing of the word would be left.
    fn after_backspace(mut self) -> Option<Self> {
        (self.corrected, self.original) = after_backspace(&self.corrected, &self.original)?;
        Some(self)
    }
}

/// `corrected` and `original` without their last character, unless nothing
/// of the word would be left.
#[cfg(target_os = "macos")]
fn after_backspace(corrected: &str, original: &str) -> Option<(String, String)> {
    let drop_last = |text: &str| {
        let mut text = text.to_owned();
        text.pop()?;
        (!text.trim().is_empty()).then_some(text)
    };
    Some((drop_last(corrected)?, drop_last(original)?))
}

#[cfg(target_os = "macos")]
struct PendingUndo {
    record: UndoRecord,
    ready_at: std::time::Instant,
    expires: std::time::Instant,
}

/// Runs the background app. `hotkey_override` is `upyr run --hotkey`; it is
/// applied now and reapplied after every configuration reload.
pub fn run(config: Config, hotkey_override: Option<String>) -> Result<()> {
    let config = config.with_hotkey_override(hotkey_override.as_deref())?;
    config.validate()?;
    let instance = acquire_single_instance()?;
    if !instance.is_single() {
        bail!("Upyr is already running");
    }
    let (hotkey, last_word_hotkey) = parse_hotkeys(&config)?;

    #[cfg(target_os = "macos")]
    let undo_hotkey: HotKey = config
        .undo_hotkey
        .parse()
        .context("invalid undo shortcut")?;
    let transform_hotkeys = config.transform_hotkeys()?;
    #[allow(unused_mut)]
    let mut startup_keys = vec![hotkey, last_word_hotkey];
    #[cfg(target_os = "macos")]
    startup_keys.push(undo_hotkey);
    startup_keys.extend(transform_hotkeys.iter().map(|(_, key)| *key));
    let manager = GlobalHotKeyManager::new().context("failed to initialize global hotkeys")?;
    manager.register_all(&startup_keys).with_context(|| {
        format!(
            "failed to register global hotkeys {} and {}",
            config.hotkey, config.last_word_hotkey
        )
    })?;

    let mut event_loop_builder = EventLoop::<AppEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        event_loop_builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let event_loop = event_loop_builder
        .build()
        .context("failed to create the desktop event loop")?;

    let event_proxy = event_loop.create_proxy();
    let hotkey_proxy = event_proxy.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |event| {
        if let Err(error) = hotkey_proxy.send_event(AppEvent::HotKey(event)) {
            error!(%error, "failed to forward a global hotkey event");
        }
    }));
    #[cfg(not(target_os = "linux"))]
    {
        let menu_proxy = event_proxy.clone();
        MenuEvent::set_event_handler(Some(move |event| {
            if let Err(error) = menu_proxy.send_event(AppEvent::Menu(event)) {
                error!(%error, "failed to forward a tray menu event");
            }
        }));
    }
    #[cfg(target_os = "macos")]
    let accessibility_blocked = !accessibility::is_trusted();
    let gesture = create_gesture_monitor_or_log(&config, &event_proxy);
    let auto_monitor = create_auto_monitor_or_log(&config, &event_proxy);
    let watcher_proxy = event_proxy.clone();
    let config_watcher = ConfigWatcher::start(config_path()?, move || {
        if let Err(error) = watcher_proxy.send_event(AppEvent::ReloadConfiguration) {
            error!(%error, "failed to forward a configuration change");
        }
    })?;

    info!(hotkey = %config.hotkey, modifier_gesture = ?config.modifier_gesture, auto_correct = config.auto_correct, "Upyr is running; select mistyped text and press the hotkey");
    let mut app = App {
        #[cfg(target_os = "macos")]
        conversion: None,
        #[cfg(target_os = "macos")]
        queued_conversion: None,
        manager,
        hotkey,
        last_word_hotkey,
        transform_hotkeys,
        hotkey_override,
        #[cfg(target_os = "macos")]
        undo_hotkey,
        #[cfg(target_os = "macos")]
        undo_record: None,
        #[cfg(target_os = "macos")]
        line_break: LineBreak::default(),
        #[cfg(target_os = "macos")]
        typing_app: None,
        #[cfg(target_os = "macos")]
        pending_undo: None,
        #[cfg(target_os = "macos")]
        learner: crate::learning::Learner::default(),
        config,
        event_proxy,
        gesture,
        auto_monitor,
        auto_tracker: AutoWordTracker::default(),
        #[cfg(target_os = "macos")]
        auto_target: None,
        #[cfg(target_os = "macos")]
        slash_tracker: crate::slash_commands::SlashTracker::default(),
        #[cfg(target_os = "macos")]
        slash_target: None,
        _config_watcher: config_watcher,
        tray: None,
        processing: false,
        paused: false,
        auto_generation: 0,
        feedback_generation: 0,
        tray_flip_generation: 0,
        #[cfg(target_os = "macos")]
        accessibility_watcher: None,
        #[cfg(target_os = "macos")]
        accessibility_blocked,
        #[cfg(target_os = "macos")]
        restart_prompt_scheduled: false,
        #[cfg(target_os = "macos")]
        restart_prompt_shown: false,
        #[cfg(target_os = "macos")]
        globe_monitor: None,
        #[cfg(target_os = "macos")]
        globe_generation: 0,
    };
    #[cfg(target_os = "macos")]
    app.refresh_globe_monitor();
    #[cfg(target_os = "macos")]
    app.refresh_accessibility_watcher();
    event_loop
        .run_app(&mut app)
        .context("desktop event loop failed")
}

fn create_auto_monitor(
    config: &Config,
    event_proxy: &EventLoopProxy<AppEvent>,
) -> Result<Option<AutoCorrectMonitor>> {
    crate::feedback::prewarm_sound_pack(&config.sounds);
    let proxy = event_proxy.clone();
    let sound_proxy = event_proxy.clone();
    let monitor = AutoCorrectMonitor::start(
        config.auto_correct,
        config.sounds.key_clicks_enabled(),
        move |event| {
            if let Err(error) = proxy.send_event(AppEvent::AutoKey(event)) {
                error!(%error, "failed to forward an automatic-correction key event");
            }
        },
        move |cue| {
            if let Err(error) = sound_proxy.send_event(AppEvent::KeySound(cue)) {
                error!(%error, "failed to forward a keyboard sound event");
            }
        },
    )?;
    #[cfg(target_os = "macos")]
    if let Some(monitor) = &monitor {
        let proxy = event_proxy.clone();
        monitor.set_activity_handler(move |event| {
            let _ = proxy.send_event(AppEvent::UserActivity(event));
        });
    }
    Ok(monitor)
}

fn create_auto_monitor_or_log(
    config: &Config,
    event_proxy: &EventLoopProxy<AppEvent>,
) -> Option<AutoCorrectMonitor> {
    #[cfg(target_os = "macos")]
    if (config.auto_correct || config.sounds.key_clicks_enabled()) && !accessibility::is_trusted() {
        return None;
    }
    match create_auto_monitor(config, event_proxy) {
        Ok(monitor) => monitor,
        Err(error) => {
            error!(%error, "automatic correction is unavailable; grant desktop input permission and reload the configuration");
            None
        }
    }
}

fn create_gesture_monitor(
    config: &Config,
    event_proxy: &EventLoopProxy<AppEvent>,
) -> Result<Option<ModifierGestureMonitor>> {
    let proxy = event_proxy.clone();
    let action = config.modifier_gesture_action;
    ModifierGestureMonitor::start(
        config.modifier_gesture,
        std::time::Duration::from_millis(config.modifier_gesture_timeout_ms),
        move || {
            if let Err(error) = proxy.send_event(AppEvent::ModifierGesture(action)) {
                error!(%error, "failed to forward a modifier gesture");
            }
        },
    )
}

fn create_gesture_monitor_or_log(
    config: &Config,
    event_proxy: &EventLoopProxy<AppEvent>,
) -> Option<ModifierGestureMonitor> {
    #[cfg(target_os = "macos")]
    if config.modifier_gesture != crate::config::ModifierGesture::Disabled
        && !accessibility::is_trusted()
    {
        return None;
    }
    match create_gesture_monitor(config, event_proxy) {
        Ok(monitor) => monitor,
        Err(error) => {
            error!(%error, "modifier gesture is unavailable; grant desktop input permission and reload the configuration");
            None
        }
    }
}

fn acquire_single_instance() -> Result<SingleInstance> {
    #[cfg(target_os = "macos")]
    let key = {
        let path = config_path()?.with_file_name("upyr.lock");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create runtime directory {}", parent.display())
            })?;
        }
        path.to_string_lossy().into_owned()
    };
    #[cfg(not(target_os = "macos"))]
    let key = "dev.Upyr.Upyr".to_owned();

    SingleInstance::new(&key).context("failed to create the single-instance guard")
}

impl App {
    fn reset_auto_context(&mut self) {
        #[cfg(target_os = "macos")]
        {
            self.auto_target = None;
        }
        self.auto_tracker.clear();
        self.auto_generation = self.auto_generation.wrapping_add(1);
    }

    #[cfg(target_os = "macos")]
    fn refresh_globe_monitor(&mut self) {
        if self.paused || !self.config.globe_switch.enabled {
            self.globe_monitor = None;
            // A previous process may have crashed before releasing its lease.
            if let Err(error) = crate::globe_switch::restore_system_setting() {
                warn!(%error, "could not restore the system Globe-key setting");
            }
        } else if let Some(monitor) = &mut self.globe_monitor {
            if let Err(error) = monitor.reconfigure(&self.config.globe_switch) {
                error!(%error, "could not reload Globe-key settings");
            }
        } else {
            let proxy = self.event_proxy.clone();
            match crate::globe_switch::GlobeMonitor::start(
                &self.config.globe_switch,
                move |previous, selected| {
                    let _ = proxy.send_event(AppEvent::GlobeSwitched(previous, selected));
                },
            ) {
                Ok(monitor) => self.globe_monitor = Some(monitor),
                Err(error) => {
                    error!(%error, "Globe-key switching is unavailable; run `upyr doctor` for diagnostics")
                }
            }
        }
        self.globe_generation = self
            .globe_monitor
            .as_ref()
            .map_or(0, |monitor| monitor.generation());
        self.auto_generation = self.auto_generation.wrapping_add(1);
        self.auto_tracker.clear();
    }

    #[cfg(target_os = "macos")]
    fn handle_globe_switch(
        &mut self,
        previous: Option<String>,
        selected: system_layout::ActiveInputSource,
    ) {
        self.clear_undo();
        if self.processing {
            self.cancel_conversion();
        }
        self.queued_conversion = None;
        self.globe_generation = self
            .globe_monitor
            .as_ref()
            .map_or(0, |monitor| monitor.generation());
        self.auto_generation = self.auto_generation.wrapping_add(1);
        self.auto_tracker.clear();
        if self.paused || !self.config.globe_switch.enabled {
            return;
        }
        self.present_action_cue(selected.layout, SoundEvent::LayoutSwitch);
        if crate::feedback::globe_switched(previous.as_deref(), &selected, &self.config) {
            self.schedule_indicator_dismissal();
        }
    }

    fn set_auto_suspended(&self, suspended: bool) {
        if let Some(monitor) = &self.auto_monitor {
            monitor.set_suspended(suspended);
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn perform_conversion(&mut self, previous_word: bool) {
        #[cfg(target_os = "macos")]
        self.clear_undo();
        if self.processing {
            debug!("ignoring conversion request while one is already running");
            return;
        }

        self.processing = true;
        self.set_auto_suspended(true);
        self.reset_auto_context();
        let result = if previous_word {
            convert_previous_word(&self.config)
        } else {
            convert_selection(&self.config)
        };
        match result {
            Ok(SelectionOutcome::Converted {
                direction,
                characters,
                layout_switched,
            }) => {
                info!(?direction, characters, "converted selection");
                self.present_action_feedback(layout_switched, SoundEvent::ManualConversion);
            }
            Ok(SelectionOutcome::NoSelection) => {
                debug!("copy produced no selection; clipboard was restored");
                self.play_error_sound();
            }
            Ok(SelectionOutcome::NoConvertibleText) => {
                debug!("selection did not contain characters from the active layouts");
                self.play_error_sound();
            }
            Ok(SelectionOutcome::TextMismatch) => {
                debug!("selected text no longer matched the observed word");
                self.play_error_sound();
            }
            Ok(SelectionOutcome::ClipboardChanged) => {
                debug!("clipboard changed before conversion could paste; no replacement was sent");
                self.play_error_sound();
            }
            Ok(SelectionOutcome::Cancelled) => debug!("selection conversion was cancelled"),
            Err(error) => {
                error!(%error, "selection conversion failed");
                self.play_error_sound();
            }
        }
        self.processing = false;
        self.set_auto_suspended(false);
    }

    /// Transliterates or re-cases the selection; the layout never changes.
    #[cfg(target_os = "macos")]
    fn perform_transform(&mut self, transform: upyr_core::TextTransform) {
        self.clear_undo();
        let mut config = self.config.clone();
        config.switch_layout = false;
        self.start_conversion(ConversionRequest::Transform(transform), config, None, None);
    }

    #[cfg(not(target_os = "macos"))]
    fn perform_transform(&mut self, transform: upyr_core::TextTransform) {
        if self.processing {
            return;
        }
        self.processing = true;
        self.set_auto_suspended(true);
        self.reset_auto_context();
        match crate::automation::transform_selection(&self.config, transform) {
            Ok(SelectionOutcome::Converted { .. }) => info!(?transform, "transformed selection"),
            Ok(outcome) => {
                debug!(?transform, ?outcome, "selection was not transformed");
                self.play_error_sound();
            }
            Err(error) => {
                error!(%error, "selection transform failed");
                self.play_error_sound();
            }
        }
        self.processing = false;
        self.set_auto_suspended(false);
    }

    #[cfg(target_os = "macos")]
    fn perform_conversion(&mut self, previous_word: bool) {
        #[cfg(target_os = "macos")]
        self.clear_undo();
        let request = if previous_word {
            ConversionRequest::PreviousWord
        } else {
            ConversionRequest::Selection
        };
        self.start_conversion(request, self.config.clone(), None, None);
    }

    #[cfg(target_os = "macos")]
    fn start_conversion(
        &mut self,
        request: ConversionRequest,
        config: Config,
        generation: Option<u64>,
        held: Option<(i32, std::sync::Arc<crate::automation::HoldSignal>)>,
    ) {
        if self.processing {
            if let Some((pid, _)) = held {
                let _ = crate::automation::release_held(pid);
            }
            return;
        }
        let undo_text = match &request {
            ConversionRequest::Automatic {
                expected,
                replacement,
            } => Some((expected.clone(), replacement.clone())),
            _ => None,
        };
        let reverted = match &request {
            ConversionRequest::Undo { replacement, .. } => Some(replacement.clone()),
            _ => None,
        };
        let direction = config.direction;
        // The guard stamps Quartz input counters and starts holding input
        // first. A key that arrives during the slower identity queries below
        // must count as new input, not be folded into the stamp.
        let task = match ConversionTask::desktop_with_hold(config, request, held) {
            Ok(task) => task,
            Err(error) => {
                error!(%error, "could not begin conversion");
                self.play_error_sound();
                return;
            }
        };
        let undo_candidate = undo_text.and_then(|(original, corrected)| {
            // Trackers retain window identity across input boundaries.
            // The actual editor may have been replaced inside that window;
            // undo must bind to the editor this conversion is about to use.
            let target = FocusTarget::capture();
            let source = system_layout::current().ok().flatten();
            target.zip(source).map(|(target, source)| UndoRecord {
                original,
                corrected,
                source_id: source.source_id,
                direction,
                target,
                expires: std::time::Instant::now() + std::time::Duration::from_secs(30),
            })
        });
        let pending = PendingConversion {
            task,
            automatic_generation: generation,
            undo_candidate,
            reverted,
        };
        if self.conversion.is_some() {
            // The previous paste has committed and only clipboard cleanup is
            // pending. Keep one guarded request; further input invalidates it.
            self.queued_conversion = Some(pending);
            return;
        }
        self.processing = true;
        self.set_auto_suspended(true);
        self.reset_auto_context();
        self.conversion = Some(pending);
        self.advance_conversion(std::time::Instant::now());
    }

    #[cfg(target_os = "macos")]
    fn finish_editing(&mut self) {
        self.processing = false;
        self.reset_auto_context();
        self.set_auto_suspended(false);
    }

    #[cfg(target_os = "macos")]
    fn cancel_conversion(&mut self) {
        self.queued_conversion = None;
        if let Some(mut pending) = self.conversion.take() {
            if pending.task.committed() {
                // Preserve the configured paste-consumption interval. Pause
                // and reload cancel edits, but cleanup still owns its deadline.
                self.conversion = Some(pending);
            } else if let Err(error) = pending.task.cancel() {
                warn!(%error, "conversion cleanup failed");
            }
        }
        if self.processing {
            self.finish_editing();
        }
    }

    #[cfg(target_os = "macos")]
    fn advance_conversion(&mut self, now: std::time::Instant) {
        let Some(mut pending) = self.conversion.take() else {
            return;
        };
        let automatic = pending.task.automatic();
        let committed = pending.task.committed();
        match pending.task.poll(now) {
            Ok(ConversionProgress::Waiting) => self.conversion = Some(pending),
            Ok(ConversionProgress::Committed(outcome)) => {
                self.finish_editing();
                if matches!(outcome, SelectionOutcome::Converted { .. }) {
                    self.undo_record = pending.undo_candidate.take();
                    self.learn_from(&pending);
                }
                self.report_conversion(outcome, automatic);
                self.conversion = Some(pending);
            }
            Ok(ConversionProgress::Finished(outcome)) => {
                if !committed {
                    self.finish_editing();
                    if let Some(outcome) = outcome {
                        if matches!(outcome, SelectionOutcome::Converted { .. }) {
                            self.undo_record = pending.undo_candidate.take();
                            self.learn_from(&pending);
                        }
                        self.report_conversion(outcome, automatic);
                    }
                }
            }
            Err(error) => {
                if !committed {
                    self.finish_editing();
                    self.play_error_sound();
                }
                error!(%error, "conversion phase failed");
            }
        }
        if self.conversion.is_none() {
            if let Some(pending) = self.queued_conversion.take() {
                let valid_generation = pending.automatic_generation.is_none_or(|generation| {
                    generation == self.auto_generation && self.config.auto_correct && !self.paused
                });
                if valid_generation {
                    self.processing = true;
                    self.set_auto_suspended(true);
                    self.reset_auto_context();
                    self.conversion = Some(pending);
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn report_conversion(&mut self, outcome: SelectionOutcome, automatic: bool) {
        match outcome {
            SelectionOutcome::Converted {
                direction,
                characters,
                layout_switched,
            } => {
                if automatic {
                    self.slash_tracker.committed();
                }
                info!(?direction, characters, automatic, "converted text");
                let feedback_layout = layout_switched.or_else(|| {
                    (automatic && self.config.switch_layout)
                        .then(|| {
                            system_layout::current()
                                .ok()
                                .flatten()
                                .and_then(|source| source.layout)
                        })
                        .flatten()
                });
                self.present_action_feedback(
                    feedback_layout,
                    if automatic {
                        SoundEvent::AutoCorrect
                    } else {
                        SoundEvent::ManualConversion
                    },
                );
            }
            SelectionOutcome::Cancelled => {
                debug!("conversion cancelled after input or focus changed")
            }
            SelectionOutcome::NoSelection => debug!("conversion had no verified selection"),
            SelectionOutcome::TextMismatch => {
                debug!("conversion skipped because the selected content changed")
            }
            SelectionOutcome::ClipboardChanged => {
                debug!("conversion skipped because the clipboard changed")
            }
            SelectionOutcome::NoConvertibleText => {
                debug!("selection contained no convertible characters")
            }
        }
        if !automatic
            && !matches!(
                outcome,
                SelectionOutcome::Converted { .. } | SelectionOutcome::Cancelled
            )
        {
            self.play_error_sound();
        }
    }

    fn handle_hotkey(&mut self, event: GlobalHotKeyEvent) {
        if self.paused || event.state != HotKeyState::Released {
            return;
        }
        if settings::is_open() {
            debug!("ignoring global hotkey while Upyr Settings is open");
            return;
        }
        #[cfg(target_os = "macos")]
        if event.id == self.undo_hotkey.id() {
            debug!(
                available = self.undo_record.is_some(),
                "received correction undo shortcut"
            );
            if let Some(record) = self.undo_record.take() {
                let now = std::time::Instant::now();
                if record.expires > now && record.target.is_current() {
                    self.pending_undo = Some(PendingUndo {
                        record,
                        ready_at: now,
                        expires: now + std::time::Duration::from_secs(2),
                    });
                }
            }
            return;
        }
        if let Some(&(transform, _)) = self
            .transform_hotkeys
            .iter()
            .find(|(_, key)| key.id() == event.id)
        {
            self.perform_transform(transform);
            return;
        }
        if event.id == self.hotkey.id() {
            self.perform_conversion(false);
        } else if event.id == self.last_word_hotkey.id() {
            self.perform_conversion(true);
        }
    }

    fn handle_modifier_gesture(&mut self, action: GestureAction) {
        // Same rule as handle_hotkey: Settings is recording keys, so a
        // double-tap there must not also convert text in the background app.
        if settings::is_open() {
            debug!("ignoring modifier gesture while Upyr Settings is open");
            return;
        }
        self.perform_conversion(action == GestureAction::PreviousWord);
    }

    fn active_hotkeys(&self) -> Vec<HotKey> {
        #[allow(unused_mut)]
        let mut keys = vec![self.hotkey, self.last_word_hotkey];
        #[cfg(target_os = "macos")]
        keys.push(self.undo_hotkey);
        keys.extend(self.transform_hotkeys.iter().map(|(_, key)| *key));
        keys
    }

    #[cfg(target_os = "macos")]
    fn clear_undo(&mut self) {
        if self.undo_record.is_some() || self.pending_undo.is_some() {
            debug!("invalidated correction undo state");
        }
        self.undo_record = None;
        self.pending_undo = None;
        // Input can arrive after the native edit commits but before its
        // completion reaches the event loop. Never install stale history.
        if let Some(pending) = self.conversion.as_mut() {
            pending.undo_candidate = None;
        }
        if let Some(pending) = self.queued_conversion.as_mut() {
            pending.undo_candidate = None;
        }
    }

    #[cfg(target_os = "macos")]
    fn handle_activity(&mut self, activity: crate::auto_correct_monitor::ActivityEvent) {
        /// kVK_Delete, the Backspace key.
        const BACKSPACE: u16 = 51;
        /// Shift, Control, Option and Command.
        const MODIFIERS: usize = 0b1111 << 17;
        let (undo_chord, backspace) = match activity {
            crate::auto_correct_monitor::ActivityEvent::Key { code, flags } => (
                crate::hotkey_bindings::matches_mac(self.undo_hotkey, code, flags),
                code == BACKSPACE && flags & MODIFIERS == 0,
            ),
            crate::auto_correct_monitor::ActivityEvent::Pointer => (false, false),
        };
        if backspace && self.revert_on_backspace() {
            return;
        }
        if !undo_chord {
            self.clear_undo();
        }
    }

    /// Backspace as the first key after an automatic correction reverts it.
    /// The Backspace itself goes through and deletes the character that ended
    /// the word, so the word comes back as typed with the caret after it. A
    /// second key before the revert starts (holding Backspace, say) cancels
    /// it as any other key does.
    #[cfg(target_os = "macos")]
    fn revert_on_backspace(&mut self) -> bool {
        if !self.config.backspace_undo
            || self.pending_undo.is_some()
            || self.processing
            || self.paused
        {
            return false;
        }
        let Some(record) = self.undo_record.take() else {
            return false;
        };
        let now = std::time::Instant::now();
        if record.expires <= now || !record.target.is_current() {
            return false;
        }
        let Some(record) = record.after_backspace() else {
            return false;
        };
        debug!("Backspace after an automatic correction: reverting it");
        self.pending_undo = Some(PendingUndo {
            record,
            // Let the editor apply the Backspace before the word is selected.
            ready_at: now + std::time::Duration::from_millis(40),
            expires: now + std::time::Duration::from_secs(2),
        });
        true
    }

    /// Counts a successful undo or manual conversion toward a lesson, and
    /// saves the lesson once the same fix has repeated.
    #[cfg(target_os = "macos")]
    fn learn_from(&mut self, pending: &PendingConversion) {
        if !self.config.auto_correct || !self.config.auto_correct_learning {
            return;
        }
        let lesson = match (&pending.reverted, pending.task.manual_source()) {
            (Some(original), _) => self.learner.reverted(original),
            (None, Some(source)) => self.learner.converted(source),
            (None, None) => None,
        };
        if let Some(lesson) = lesson {
            if let Err(error) = self.remember(&lesson) {
                warn!(%error, "could not save what was learned from a fix");
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn remember(&mut self, lesson: &crate::learning::Lesson) -> Result<()> {
        let mut config = Config::load()?;
        if !config.learn(lesson) {
            return Ok(());
        }
        config.write(true)?;
        info!(?lesson, "learned from a repeated fix");
        self.config.auto_correct_exceptions = config.auto_correct_exceptions;
        self.config.auto_correct_rules = config.auto_correct_rules;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn advance_undo(&mut self, now: std::time::Instant) {
        if self
            .undo_record
            .as_ref()
            .is_some_and(|record| now >= record.expires)
        {
            self.undo_record = None;
        }
        if self
            .pending_undo
            .as_ref()
            .is_none_or(|pending| now < pending.ready_at)
        {
            return;
        }
        let Some(mut pending) = self.pending_undo.take() else {
            return;
        };
        if self.processing
            || self.paused
            || now >= pending.expires
            || !pending.record.target.is_current()
        {
            debug!("undo cancelled before applying to its original window");
            return;
        }
        if !crate::automation::modifiers_released() {
            pending.ready_at = now + std::time::Duration::from_millis(10);
            self.pending_undo = Some(pending);
            return;
        }
        if !pending.record.target.is_current_for_undo() {
            debug!("undo cancelled because its focused element changed");
            return;
        }
        let mut config = self.config.clone();
        config.direction = match pending.record.direction {
            crate::layout::Direction::EnglishToUkrainian => {
                crate::layout::Direction::UkrainianToEnglish
            }
            _ => crate::layout::Direction::EnglishToUkrainian,
        };
        self.start_conversion(
            ConversionRequest::Undo {
                expected: pending.record.corrected,
                replacement: pending.record.original,
                source_id: pending.record.source_id,
            },
            config,
            None,
            None,
        );
    }

    fn handle_auto_key(&mut self, event: AutoKeyEvent) {
        self.auto_generation = self.auto_generation.wrapping_add(1);
        if self.paused || self.processing || !self.config.auto_correct {
            #[cfg(target_os = "macos")]
            self.slash_tracker.invalidate();
            return;
        }
        #[cfg(target_os = "macos")]
        if self.auto_correct_paused_here() {
            self.auto_tracker.clear();
            crate::automation::disarm_line_break();
            return;
        }
        #[cfg(target_os = "macos")]
        if self.handle_slash_key(event) {
            self.auto_tracker.clear();
            return;
        }
        #[cfg(target_os = "macos")]
        {
            if self
                .auto_target
                .as_ref()
                .is_some_and(|target| !target.is_current())
            {
                self.reset_auto_context();
            }
            if crate::input_context::terminal_typing_enabled(&self.config)
                && self.auto_target.is_none()
                && AutoWordTracker::can_begin(event.key)
            {
                self.auto_target = FocusTarget::capture();
                if self.auto_target.is_none() {
                    self.auto_tracker.clear();
                    return;
                }
            }
            if !AutoWordTracker::can_begin(event.key)
                && !matches!(
                    event.key,
                    device_query::Keycode::Space
                        | device_query::Keycode::Backspace
                        | device_query::Keycode::LShift
                        | device_query::Keycode::RShift
                )
            {
                self.auto_target = None;
            }
        }

        // Re-sample on every text-producing key. A user can switch layouts
        // while an ambiguous word is still buffered, or immediately after a
        // visibly wrong first character; retaining the old layout would merge
        // physical keys from two layouts into one correction candidate.
        if AutoWordTracker::can_begin(event.key) {
            let layout = match system_layout::current() {
                Ok(Some(source)) => source.layout,
                Ok(None) => None,
                Err(error) => {
                    debug!(%error, "could not read the active layout for automatic correction");
                    None
                }
            };
            self.auto_tracker.set_source_layout(layout);
        }

        let Some(sample) = self.auto_tracker.observe(event) else {
            #[cfg(target_os = "macos")]
            self.refresh_line_break(event.key);
            return;
        };
        #[cfg(target_os = "macos")]
        self.refresh_line_break(event.key);
        #[cfg(target_os = "macos")]
        let decision = crate::auto_correct::evaluate_with_caps_lock(
            &sample,
            &self.config,
            crate::automation::caps_lock_on(),
            crate::input_context::terminal_typing_enabled(&self.config),
        );
        #[cfg(not(target_os = "macos"))]
        let decision = crate::auto_correct::evaluate(&sample, &self.config);
        match decision {
            AutoDecision::Correct(correction) => {
                debug!(direction = ?correction.direction, "automatic word accepted for correction");
                self.schedule_auto_correction(correction);
            }
            AutoDecision::Continue => debug!("automatic word needs more confidence"),
            AutoDecision::Reset => {
                self.auto_tracker.clear();
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn handle_slash_key(&mut self, event: AutoKeyEvent) -> bool {
        if !crate::input_context::slash_commands_enabled(&self.config) {
            self.slash_tracker.invalidate();
            self.slash_target = None;
            return false;
        }
        if self
            .slash_target
            .as_ref()
            .is_some_and(|target| !target.is_current())
        {
            self.slash_target = None;
        }
        if self.slash_target.is_none() {
            self.slash_target = FocusTarget::capture();
            self.slash_tracker = crate::slash_commands::SlashTracker::default();
        }
        if self.slash_target.is_none() {
            self.slash_tracker.invalidate();
            return false;
        }
        let layout = system_layout::current()
            .ok()
            .flatten()
            .and_then(|source| source.layout);
        let mapping = system_layout::installed_mapping()
            .ok()
            .flatten()
            .unwrap_or_else(upyr_core::default_physical_mapping);
        let owns_word = self.slash_tracker.observe(event, layout, &mapping);
        if let Some(correction) = self
            .slash_tracker
            .correction(&self.config.auto_correct_exceptions)
        {
            self.auto_target.clone_from(&self.slash_target);
            // Queue immediately: waiting for a word boundary would leave a
            // command menu closed until after the command had been submitted.
            let _ = self.event_proxy.send_event(AppEvent::ApplyAutoCorrection {
                generation: self.auto_generation,
                correction,
            });
        }
        owns_word
    }

    fn handle_key_sound(&self, cue: KeyCue) {
        if self.paused || self.processing {
            return;
        }
        crate::feedback::play_key_sound(cue, &self.config);
    }

    fn schedule_auto_correction(&self, correction: AutoCorrection) {
        let generation = self.auto_generation;
        let delay = self.config.auto_correct_delay_ms;
        let proxy = self.event_proxy.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(delay));
            if let Err(error) = proxy.send_event(AppEvent::ApplyAutoCorrection {
                generation,
                correction,
            }) {
                debug!(%error, "failed to schedule automatic correction");
            }
        });
    }

    #[cfg(not(target_os = "macos"))]
    fn apply_auto_correction(&mut self, generation: u64, correction: AutoCorrection) {
        if generation != self.auto_generation
            || self.paused
            || self.processing
            || !self.config.auto_correct
        {
            return;
        }
        self.processing = true;
        self.set_auto_suspended(true);
        let mut conversion_config = self.config.clone();
        conversion_config.direction = correction.direction;
        let result = {
            let auto_monitor = self.auto_monitor.as_ref();
            let mut on_text_committed = || {
                if let Some(monitor) = auto_monitor {
                    // Paste and layout switching are complete. Resume before
                    // the trailing clipboard-restore delay so the first key of
                    // the next word cannot disappear from the tracker.
                    monitor.set_suspended(false);
                }
            };
            convert_previous_input_if_matches(
                &conversion_config,
                &correction.expected_source,
                &correction.replacement,
                &mut on_text_committed,
            )
        };
        match result {
            Ok(SelectionOutcome::Converted {
                direction,
                characters,
                layout_switched,
            }) => {
                info!(
                    ?direction,
                    characters, "automatically corrected a recognized word"
                );
                self.present_action_feedback(layout_switched, SoundEvent::AutoCorrect);
            }
            Ok(SelectionOutcome::TextMismatch) => {
                debug!("automatic correction was skipped because the caret or text changed")
            }
            Ok(SelectionOutcome::ClipboardChanged) => {
                debug!("automatic correction was skipped because the clipboard changed")
            }
            Ok(SelectionOutcome::Cancelled) => debug!("automatic correction was cancelled"),
            Ok(SelectionOutcome::NoSelection) => {
                debug!("automatic correction could not select the previous word")
            }
            Ok(SelectionOutcome::NoConvertibleText) => {
                debug!("automatic correction found no convertible text")
            }
            Err(error) => {
                error!(%error, "automatic correction failed");
                self.play_error_sound();
            }
        }
        self.auto_tracker.clear();
        self.auto_generation = self.auto_generation.wrapping_add(1);
        self.processing = false;
        self.set_auto_suspended(false);
    }

    #[cfg(target_os = "macos")]
    fn apply_auto_correction(&mut self, generation: u64, correction: AutoCorrection) {
        if generation != self.auto_generation
            || self.paused
            || self.processing
            || !self.config.auto_correct
            || self
                .globe_monitor
                .as_ref()
                .is_some_and(|monitor| monitor.generation() != self.globe_generation)
        {
            return;
        }
        if crate::input_context::terminal_typing_enabled(&self.config)
            && !self
                .auto_target
                .as_ref()
                .is_some_and(FocusTarget::is_current)
        {
            self.reset_auto_context();
            return;
        }
        let config = self.correction_config(&correction);
        self.start_conversion(
            ConversionRequest::Automatic {
                expected: correction.expected_source,
                replacement: correction.replacement,
            },
            config,
            Some(generation),
            None,
        );
    }

    /// The configuration one automatic correction runs with. A case-only
    /// accidental-Caps-Lock fix (`Direction::Smart`) keeps the layout, and
    /// any accidental-Caps-Lock fix releases Caps Lock for the next words.
    #[cfg(target_os = "macos")]
    fn correction_config(&self, correction: &AutoCorrection) -> Config {
        let mut config = self.config.clone();
        config.direction = correction.direction;
        if correction.direction == upyr_core::Direction::Smart {
            config.switch_layout = false;
        }
        let typed_word = correction
            .expected_source
            .trim_end()
            .rsplit(' ')
            .next()
            .unwrap_or("");
        if crate::automation::caps_lock_on()
            && crate::auto_correct::accidental_caps_fix(typed_word).is_some()
        {
            if let Err(error) = crate::automation::turn_off_caps_lock() {
                debug!(%error, "could not turn Caps Lock off after fixing its word");
            }
        }
        config
    }

    /// Arms the target's tap to hold Enter while the word being typed would be
    /// corrected if it ended now, so a chat message is sent corrected rather
    /// than corrected after it has gone. Any other state disarms it.
    #[cfg(target_os = "macos")]
    fn refresh_line_break(&mut self, key: device_query::Keycode) {
        if matches!(
            key,
            device_query::Keycode::Enter | device_query::Keycode::NumpadEnter
        ) {
            // The tap may already hold this Enter and report it shortly: keep
            // the candidate for that report, but never arm the next Enter.
            crate::automation::disarm_line_break();
            if let Some(pending) = &mut self.line_break.pending {
                pending
                    .entered_at
                    .get_or_insert_with(std::time::Instant::now);
            }
            return;
        }
        if self.line_break.pending.as_ref().is_some_and(|pending| {
            pending
                .entered_at
                .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(300))
        }) {
            return;
        }
        let correction = (!crate::input_context::terminal_typing_enabled(&self.config))
            .then(|| self.auto_tracker.line_break_sample())
            .flatten()
            .and_then(|sample| {
                match crate::auto_correct::evaluate_line_break(
                    &sample,
                    &self.config,
                    crate::automation::caps_lock_on(),
                ) {
                    AutoDecision::Correct(correction) => Some(correction),
                    _ => None,
                }
            });
        let armed = correction.and_then(|correction| {
            let pid = crate::automation::frontmost_pid()?;
            self.line_break.next_id = self.line_break.next_id.wrapping_add(1);
            let id = self.line_break.next_id;
            let proxy = self.event_proxy.clone();
            // Selection, copy and paste with the same margin as a held edit.
            let budget = 30_u64
                .saturating_add(self.config.copy_delay_ms)
                .saturating_add(self.config.paste_delay_ms)
                .saturating_add(750);
            match crate::automation::arm_line_break(
                pid,
                id,
                std::time::Duration::from_millis(budget),
                move |held| {
                    let _ = proxy.send_event(AppEvent::LineBreakHeld(held));
                },
            ) {
                Ok(()) => Some(PendingLineBreak {
                    id,
                    correction,
                    entered_at: None,
                }),
                Err(error) => {
                    debug!(%error, "could not arm Enter for automatic correction");
                    None
                }
            }
        });
        if armed.is_none() {
            crate::automation::disarm_line_break();
        }
        self.line_break.pending = armed;
    }

    #[cfg(target_os = "macos")]
    fn handle_line_break_held(&mut self, held: crate::automation::HeldLineBreak) {
        let pending = self.line_break.pending.take();
        let correction = pending
            .filter(|pending| pending.id == held.id)
            .map(|pending| pending.correction)
            .filter(|_| !self.paused && self.config.auto_correct);
        let Some(correction) = correction else {
            let _ = crate::automation::release_held(held.pid);
            return;
        };
        debug!(direction = ?correction.direction, "correcting the word before Enter");
        let config = self.correction_config(&correction);
        self.start_conversion(
            ConversionRequest::Automatic {
                expected: correction.expected_source,
                replacement: correction.replacement,
            },
            config,
            None,
            Some((held.pid, held.signal)),
        );
    }

    fn present_action_feedback(
        &mut self,
        layout: Option<system_layout::SystemLayout>,
        event: SoundEvent,
    ) {
        self.present_action_cue(layout, event);
        let Some(layout) = layout else { return };
        if crate::feedback::layout_switched(layout, &self.config) {
            self.schedule_indicator_dismissal();
        }
    }

    fn present_action_cue(
        &mut self,
        layout: Option<system_layout::SystemLayout>,
        event: SoundEvent,
    ) {
        self.start_tray_flip();

        // One action produces at most one cue. The layout sound is a fallback
        // for v4 users whose migration intentionally enables only that event.
        if self.config.sounds.event_enabled(event) {
            crate::feedback::play_sound_event(event, &self.config);
        } else if layout.is_some() {
            crate::feedback::play_sound_event(SoundEvent::LayoutSwitch, &self.config);
        }
    }

    fn schedule_indicator_dismissal(&mut self) {
        self.feedback_generation = self.feedback_generation.wrapping_add(1);
        let generation = self.feedback_generation;
        let delay = std::time::Duration::from_millis(self.config.layout_indicator_duration_ms);
        let proxy = self.event_proxy.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            if let Err(error) = proxy.send_event(AppEvent::HideLayoutIndicator(generation)) {
                debug!(%error, "could not schedule layout indicator dismissal");
            }
        });
    }

    fn play_error_sound(&self) {
        crate::feedback::play_sound_event(SoundEvent::Error, &self.config);
    }

    fn start_tray_flip(&mut self) {
        let Some(tray) = &self.tray else {
            return;
        };

        self.tray_flip_generation = self.tray_flip_generation.wrapping_add(1);
        let generation = self.tray_flip_generation;
        if let Err(error) = tray.set_flip_frame(1) {
            debug!(%error, "could not start tray icon flip");
            return;
        }

        let proxy = self.event_proxy.clone();
        std::thread::spawn(move || {
            for frame in 2..=TRAY_FLIP_FRAME_COUNT {
                std::thread::sleep(std::time::Duration::from_millis(TRAY_FLIP_FRAME_DELAY_MS));
                if let Err(error) = proxy.send_event(AppEvent::TrayFlipFrame { generation, frame })
                {
                    debug!(%error, "could not schedule tray icon flip frame");
                    break;
                }
            }
        });
    }

    fn toggle_paused(&mut self) -> Result<()> {
        #[cfg(target_os = "macos")]
        self.clear_undo();
        #[cfg(target_os = "macos")]
        self.cancel_conversion();
        if self.paused {
            #[cfg(target_os = "macos")]
            {
                self.accessibility_blocked = !accessibility::is_trusted();
            }
            crate::hotkey_bindings::replace(&self.manager, &[], &self.active_hotkeys())
                .context("failed to resume the global hotkeys")?;
            self.gesture = create_gesture_monitor_or_log(&self.config, &self.event_proxy);
            self.auto_monitor = create_auto_monitor_or_log(&self.config, &self.event_proxy);
        } else {
            #[cfg(target_os = "macos")]
            {
                self.accessibility_blocked = false;
            }
            crate::hotkey_bindings::replace(&self.manager, &self.active_hotkeys(), &[])
                .context("failed to pause the global hotkeys")?;
            self.gesture = None;
            self.auto_monitor = None;
        }
        self.paused = !self.paused;
        self.reset_auto_context();
        #[cfg(target_os = "macos")]
        self.refresh_globe_monitor();
        #[cfg(target_os = "macos")]
        self.refresh_accessibility_watcher();
        self.update_tray()?;
        crate::feedback::play_sound_event(
            if self.paused {
                SoundEvent::Pause
            } else {
                SoundEvent::Resume
            },
            &self.config,
        );
        info!(paused = self.paused, "updated pause state");
        Ok(())
    }

    fn reload_configuration(&mut self) -> Result<()> {
        let new_config = Config::load()?.with_hotkey_override(self.hotkey_override.as_deref())?;
        let (new_hotkey, new_last_word_hotkey) = parse_hotkeys(&new_config)?;
        #[cfg(target_os = "macos")]
        let new_undo: HotKey = new_config
            .undo_hotkey
            .parse()
            .context("invalid undo shortcut")?;
        let new_transforms = new_config.transform_hotkeys()?;
        #[allow(unused_mut)]
        let mut new_keys = vec![new_hotkey, new_last_word_hotkey];
        #[cfg(target_os = "macos")]
        new_keys.push(new_undo);
        new_keys.extend(new_transforms.iter().map(|(_, key)| *key));
        #[cfg(target_os = "macos")]
        let new_accessibility_blocked = !self.paused && !accessibility::is_trusted();
        if !self.paused && new_keys != self.active_hotkeys() {
            crate::hotkey_bindings::replace(&self.manager, &self.active_hotkeys(), &new_keys)
                .context("failed to register the reloaded hotkeys")?;
        }

        #[cfg(target_os = "macos")]
        self.cancel_conversion();
        // Monitor creation replaces the shared native subscription. Do this
        // only after shortcut registration has committed successfully.
        let new_gesture = if self.paused {
            None
        } else {
            create_gesture_monitor_or_log(&new_config, &self.event_proxy)
        };
        let new_auto_monitor = if self.paused {
            None
        } else {
            create_auto_monitor_or_log(&new_config, &self.event_proxy)
        };

        #[cfg(target_os = "macos")]
        {
            self.undo_hotkey = new_undo;
            self.clear_undo();
        }
        #[cfg(target_os = "macos")]
        crate::feedback::stop_rerouted_audio(u64::MAX);
        self.config = new_config;
        self.hotkey = new_hotkey;
        self.last_word_hotkey = new_last_word_hotkey;
        self.transform_hotkeys = new_transforms;
        self.gesture = new_gesture;
        self.auto_monitor = new_auto_monitor;
        self.reset_auto_context();
        #[cfg(target_os = "macos")]
        self.refresh_globe_monitor();
        if !self.config.show_layout_indicator {
            self.feedback_generation = self.feedback_generation.wrapping_add(1);
            crate::feedback::hide_layout_indicator();
        }
        #[cfg(target_os = "macos")]
        {
            self.accessibility_blocked = new_accessibility_blocked;
        }
        #[cfg(target_os = "macos")]
        self.refresh_accessibility_watcher();
        self.update_tray()?;
        info!(hotkey = %self.config.hotkey, auto_correct = self.config.auto_correct, "reloaded configuration");
        Ok(())
    }

    fn update_tray(&self) -> Result<()> {
        if let Some(tray) = &self.tray {
            tray.update(&self.config, self.paused)?;
        }
        Ok(())
    }

    fn handle_tray_action(&mut self, event_loop: &ActiveEventLoop, action: Option<TrayAction>) {
        let result = match action {
            Some(TrayAction::ConvertPreviousWord) => {
                self.perform_conversion(true);
                Ok(())
            }
            Some(TrayAction::ConvertSelection) => {
                self.perform_conversion(false);
                Ok(())
            }
            Some(TrayAction::TogglePaused) => self.toggle_paused(),
            Some(TrayAction::ToggleCurrentApp) => self.toggle_current_app(),
            Some(TrayAction::OpenSettings) => settings::spawn(),
            Some(TrayAction::OpenAbout) => settings::spawn_about(),
            Some(TrayAction::ReloadConfiguration) => self.reload_configuration(),
            Some(TrayAction::ToggleAutostart) => self.toggle_autostart(),
            Some(TrayAction::Quit) => {
                #[cfg(target_os = "macos")]
                {
                    self.cancel_conversion();
                    self.conversion = None;
                }
                event_loop.exit();
                Ok(())
            }
            None => Ok(()),
        };

        if let Err(error) = result {
            error!(%error, "tray action failed");
            self.play_error_sound();
            let _ = self.update_tray();
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn handle_menu(&mut self, event_loop: &ActiveEventLoop, event: &MenuEvent) {
        let action = self.tray.as_ref().and_then(|tray| tray.action(event));
        self.handle_tray_action(event_loop, action);
    }

    /// Keeps the tray's per-app item naming the app being typed in, and says
    /// whether automatic correction is switched off there.
    #[cfg(target_os = "macos")]
    fn auto_correct_paused_here(&mut self) -> bool {
        let app = crate::input_context::frontmost_app();
        if app.as_ref().map(|(id, _)| id) != self.typing_app.as_ref() {
            self.typing_app = app.as_ref().map(|(id, _)| id.clone());
            if let Some(tray) = &self.tray {
                tray.set_current_app(
                    app.as_ref().map(|(_, name)| name.as_str()),
                    crate::input_context::auto_correct_disabled(&self.config),
                );
            }
        }
        crate::input_context::auto_correct_disabled(&self.config)
    }

    /// Switches automatic correction off or back on for the frontmost app.
    /// A menu-bar click leaves that app frontmost on macOS.
    fn toggle_current_app(&mut self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let Some((id, name)) = crate::input_context::frontmost_app() else {
                return Ok(());
            };
            let mut config = Config::load()?;
            let disabled = config.toggle_auto_correct_app(&id);
            config.write(true)?;
            info!(app = %id, disabled, "toggled automatic correction for an app");
            self.config.auto_correct_disabled_apps = config.auto_correct_disabled_apps;
            self.typing_app = Some(id);
            if let Some(tray) = &self.tray {
                tray.set_current_app(Some(&name), disabled);
            }
            if disabled {
                self.reset_auto_context();
                crate::automation::disarm_line_break();
            }
        }
        Ok(())
    }

    fn toggle_autostart(&self) -> Result<()> {
        let status = autostart::status()?;
        let status = match status.state {
            autostart::AutostartState::Enabled => autostart::disable()?,
            autostart::AutostartState::Disabled | autostart::AutostartState::Stale => {
                autostart::enable()?
            }
            autostart::AutostartState::Broken => {
                warn!(
                    detail = status.detail.as_deref().unwrap_or("no detail available"),
                    "launch-at-login entry needs attention; opening settings"
                );
                settings::spawn()?;
                return Ok(());
            }
        };
        self.update_tray()?;
        info!(
            enabled = status.enabled,
            location = status.location,
            "updated launch-at-login state"
        );
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn refresh_accessibility_watcher(&mut self) {
        if self.paused || !self.accessibility_blocked {
            self.accessibility_watcher = None;
            return;
        }
        if accessibility::request_once_if_needed() || accessibility::is_trusted() {
            self.accessibility_watcher = None;
            self.schedule_restart_prompt();
            return;
        }
        if self.accessibility_watcher.is_some() {
            return;
        }

        let proxy = self.event_proxy.clone();
        match AccessibilityWatcher::start(move || {
            if let Err(error) = proxy.send_event(AppEvent::AccessibilityGranted) {
                error!(%error, "failed to report granted Accessibility permission");
            }
        }) {
            Ok(watcher) => {
                self.accessibility_watcher = Some(watcher);
                info!("waiting for Accessibility permission");
            }
            Err(error) => error!(%error, "could not watch Accessibility permission"),
        }
    }

    #[cfg(target_os = "macos")]
    fn schedule_restart_prompt(&mut self) {
        if self.restart_prompt_scheduled || self.restart_prompt_shown {
            return;
        }
        self.restart_prompt_scheduled = true;
        if let Err(error) = self.event_proxy.send_event(AppEvent::AccessibilityGranted) {
            self.restart_prompt_scheduled = false;
            error!(%error, "failed to schedule the Accessibility restart prompt");
        }
    }

    #[cfg(target_os = "macos")]
    fn handle_accessibility_granted(&mut self, event_loop: &ActiveEventLoop) {
        self.accessibility_watcher = None;
        self.restart_prompt_scheduled = true;
        if self.restart_prompt_shown || !accessibility::is_trusted() {
            return;
        }
        if !self.accessibility_blocked {
            info!("Accessibility permission is active and monitors are initialized");
            return;
        }

        self.restart_prompt_shown = true;
        info!("Accessibility permission was granted; offering to restart Upyr");
        if accessibility::prompt_for_restart() {
            match accessibility::schedule_relaunch() {
                Ok(()) => event_loop.exit(),
                Err(error) => error!(%error, "could not restart Upyr"),
            }
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    #[cfg(target_os = "macos")]
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        use winit::event_loop::ControlFlow;
        let now = std::time::Instant::now();
        self.advance_undo(now);
        self.advance_conversion(now);
        let deadline = self
            .conversion
            .as_ref()
            .map(|pending| pending.task.deadline())
            .into_iter()
            .chain(self.pending_undo.as_ref().map(|pending| pending.ready_at))
            .chain(self.undo_record.as_ref().map(|record| record.expires))
            .min();
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        if self.tray.is_none() {
            #[cfg(target_os = "linux")]
            let tray_result = {
                let tray_proxy = self.event_proxy.clone();
                Tray::new(&self.config, move |action| {
                    if let Err(error) = tray_proxy.send_event(AppEvent::Tray(action)) {
                        error!(%error, "failed to forward a tray menu event");
                    }
                })
            };
            #[cfg(not(target_os = "linux"))]
            let tray_result = Tray::new(&self.config);

            match tray_result {
                Ok(tray) => {
                    self.tray = Some(tray);
                    info!("system tray control is ready");
                }
                Err(error) => {
                    error!(%error, "failed to initialize system tray; hotkey remains active")
                }
            }
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::HotKey(event) => self.handle_hotkey(event),
            #[cfg(target_os = "macos")]
            AppEvent::UserActivity(activity) => self.handle_activity(activity),
            #[cfg(not(target_os = "linux"))]
            AppEvent::Menu(event) => self.handle_menu(event_loop, &event),
            #[cfg(target_os = "linux")]
            AppEvent::Tray(action) => self.handle_tray_action(event_loop, Some(action)),
            AppEvent::ModifierGesture(action) if !self.paused => {
                self.handle_modifier_gesture(action)
            }
            AppEvent::AutoKey(event) => self.handle_auto_key(event),
            #[cfg(target_os = "macos")]
            AppEvent::LineBreakHeld(held) => self.handle_line_break_held(held),
            AppEvent::KeySound(cue) => self.handle_key_sound(cue),
            AppEvent::ApplyAutoCorrection {
                generation,
                correction,
            } => self.apply_auto_correction(generation, correction),
            AppEvent::ReloadConfiguration => {
                if let Err(error) = self.reload_configuration() {
                    error!(%error, "could not reload the changed configuration");
                }
            }
            AppEvent::HideLayoutIndicator(generation) if generation == self.feedback_generation => {
                crate::feedback::hide_layout_indicator();
            }
            AppEvent::TrayFlipFrame { generation, frame }
                if generation == self.tray_flip_generation =>
            {
                if let Some(tray) = &self.tray {
                    if let Err(error) = tray.set_flip_frame(frame) {
                        debug!(%error, "could not advance tray icon flip");
                    }
                }
            }
            #[cfg(target_os = "macos")]
            AppEvent::AccessibilityGranted => self.handle_accessibility_granted(event_loop),
            #[cfg(target_os = "macos")]
            AppEvent::GlobeSwitched(previous, selected) => {
                self.handle_globe_switch(previous, selected)
            }
            AppEvent::ModifierGesture(_) => {}
            AppEvent::HideLayoutIndicator(_) => {}
            AppEvent::TrayFlipFrame { .. } => {}
        }
    }
}

fn parse_hotkeys(config: &Config) -> Result<(HotKey, HotKey)> {
    let hotkey: HotKey = config
        .hotkey
        .parse()
        .with_context(|| format!("invalid hotkey {:?}", config.hotkey))?;
    let last_word_hotkey: HotKey = config
        .last_word_hotkey
        .parse()
        .with_context(|| format!("invalid last-word hotkey {:?}", config.last_word_hotkey))?;
    if hotkey == last_word_hotkey {
        bail!("hotkey and last_word_hotkey resolve to the same shortcut");
    }
    Ok((hotkey, last_word_hotkey))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn backspace_reverts_the_word_without_the_character_it_deleted() {
        assert_eq!(
            after_backspace("привіт ", "ghbdsn "),
            Some(("привіт".to_owned(), "ghbdsn".to_owned()))
        );
        assert_eq!(
            after_backspace("ок,", "jr,"),
            Some(("ок".to_owned(), "jr".to_owned()))
        );
        // Several rewritten words keep the earlier ones.
        assert_eq!(
            after_backspace("ти дивись ", "nb lbdbcm "),
            Some(("ти дивись".to_owned(), "nb lbdbcm".to_owned()))
        );
        // Nothing of the word would be left to restore.
        assert_eq!(after_backspace("я", "z"), None);
        assert_eq!(after_backspace(" ", " "), None);
        assert_eq!(after_backspace("", ""), None);
    }
}
