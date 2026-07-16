#[cfg(target_os = "macos")]
use std::fs;

use anyhow::{Context, Result, bail};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use single_instance::SingleInstance;
use tracing::{debug, error, info};
use tray_icon::menu::MenuEvent;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::WindowId,
};

use crate::{
    auto_correct::{AutoKeyEvent, AutoWordTracker, evaluate},
    auto_correct_monitor::AutoCorrectMonitor,
    automation::{
        SelectionOutcome, convert_previous_word, convert_previous_word_if_matches,
        convert_selection,
    },
    autostart,
    config::{Config, GestureAction, config_path},
    config_watcher::ConfigWatcher,
    modifier_gesture::ModifierGestureMonitor,
    settings, system_layout,
    tray::{Tray, TrayAction},
};

#[derive(Debug)]
enum AppEvent {
    HotKey(GlobalHotKeyEvent),
    Menu(MenuEvent),
    ModifierGesture(GestureAction),
    AutoKey(AutoKeyEvent),
    ReloadConfiguration,
}

struct App {
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
    last_word_hotkey: HotKey,
    config: Config,
    event_proxy: EventLoopProxy<AppEvent>,
    gesture: Option<ModifierGestureMonitor>,
    auto_monitor: Option<AutoCorrectMonitor>,
    auto_tracker: AutoWordTracker,
    _config_watcher: ConfigWatcher,
    tray: Option<Tray>,
    processing: bool,
    paused: bool,
}

pub fn run(config: Config) -> Result<()> {
    config.validate()?;
    let instance = acquire_single_instance()?;
    if !instance.is_single() {
        bail!("Upyr is already running");
    }
    let (hotkey, last_word_hotkey) = parse_hotkeys(&config)?;

    #[cfg(target_os = "linux")]
    gtk::init().context("failed to initialize GTK for the system tray")?;

    let manager = GlobalHotKeyManager::new().context("failed to initialize global hotkeys")?;
    manager
        .register_all(&[hotkey, last_word_hotkey])
        .with_context(|| {
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
    let menu_proxy = event_proxy.clone();
    MenuEvent::set_event_handler(Some(move |event| {
        if let Err(error) = menu_proxy.send_event(AppEvent::Menu(event)) {
            error!(%error, "failed to forward a tray menu event");
        }
    }));
    let gesture = create_gesture_monitor(&config, &event_proxy)?;
    let auto_monitor = create_auto_monitor_or_log(&config, &event_proxy);
    let watcher_proxy = event_proxy.clone();
    let config_watcher = ConfigWatcher::start(config_path()?, move || {
        if let Err(error) = watcher_proxy.send_event(AppEvent::ReloadConfiguration) {
            error!(%error, "failed to forward a configuration change");
        }
    })?;

    info!(hotkey = %config.hotkey, modifier_gesture = ?config.modifier_gesture, auto_correct = config.auto_correct, "Upyr is running; select mistyped text and press the hotkey");
    let mut app = App {
        manager,
        hotkey,
        last_word_hotkey,
        config,
        event_proxy,
        gesture,
        auto_monitor,
        auto_tracker: AutoWordTracker::default(),
        _config_watcher: config_watcher,
        tray: None,
        processing: false,
        paused: false,
    };
    event_loop
        .run_app(&mut app)
        .context("desktop event loop failed")
}

fn create_auto_monitor(
    config: &Config,
    event_proxy: &EventLoopProxy<AppEvent>,
) -> Result<Option<AutoCorrectMonitor>> {
    let proxy = event_proxy.clone();
    AutoCorrectMonitor::start(config.auto_correct, move |event| {
        if let Err(error) = proxy.send_event(AppEvent::AutoKey(event)) {
            error!(%error, "failed to forward an automatic-correction key event");
        }
    })
}

fn create_auto_monitor_or_log(
    config: &Config,
    event_proxy: &EventLoopProxy<AppEvent>,
) -> Option<AutoCorrectMonitor> {
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
    fn set_auto_suspended(&self, suspended: bool) {
        if let Some(monitor) = &self.auto_monitor {
            monitor.set_suspended(suspended);
        }
    }

    fn perform_conversion(&mut self, previous_word: bool) {
        if self.processing {
            debug!("ignoring conversion request while one is already running");
            return;
        }

        self.processing = true;
        self.set_auto_suspended(true);
        self.auto_tracker.clear();
        let result = if previous_word {
            convert_previous_word(&self.config)
        } else {
            convert_selection(&self.config)
        };
        match result {
            Ok(SelectionOutcome::Converted {
                direction,
                characters,
            }) => info!(?direction, characters, "converted selection"),
            Ok(SelectionOutcome::NoSelection) => {
                debug!("copy produced no selection; clipboard was restored")
            }
            Ok(SelectionOutcome::NoConvertibleText) => {
                debug!("selection did not contain characters from the active layouts")
            }
            Ok(SelectionOutcome::TextMismatch) => {
                debug!("selected text no longer matched the observed word")
            }
            Err(error) => error!(%error, "selection conversion failed"),
        }
        self.processing = false;
        self.set_auto_suspended(false);
    }

    fn handle_auto_key(&mut self, event: AutoKeyEvent) {
        if self.paused || self.processing || !self.config.auto_correct {
            return;
        }

        if self.auto_tracker.is_empty() && AutoWordTracker::can_begin(event.key) {
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
            return;
        };
        let Some(correction) = evaluate(&sample, &self.config) else {
            return;
        };

        self.processing = true;
        self.set_auto_suspended(true);
        std::thread::sleep(std::time::Duration::from_millis(
            self.config.auto_correct_delay_ms,
        ));
        let mut conversion_config = self.config.clone();
        conversion_config.direction = correction.direction;
        let result =
            convert_previous_word_if_matches(&conversion_config, Some(&correction.expected_source));
        match result {
            Ok(SelectionOutcome::Converted {
                direction,
                characters,
            }) => info!(
                ?direction,
                characters, "automatically corrected a recognized word"
            ),
            Ok(SelectionOutcome::TextMismatch) => {
                debug!("automatic correction was skipped because the caret or text changed")
            }
            Ok(SelectionOutcome::NoSelection) => {
                debug!("automatic correction could not select the previous word")
            }
            Ok(SelectionOutcome::NoConvertibleText) => {
                debug!("automatic correction found no convertible text")
            }
            Err(error) => error!(%error, "automatic correction failed"),
        }
        self.processing = false;
        self.set_auto_suspended(false);
    }

    fn toggle_paused(&mut self) -> Result<()> {
        if self.paused {
            let gesture = create_gesture_monitor(&self.config, &self.event_proxy)?;
            let auto_monitor = create_auto_monitor_or_log(&self.config, &self.event_proxy);
            self.manager
                .register_all(&[self.hotkey, self.last_word_hotkey])
                .context("failed to resume the global hotkeys")?;
            self.gesture = gesture;
            self.auto_monitor = auto_monitor;
        } else {
            self.manager
                .unregister_all(&[self.hotkey, self.last_word_hotkey])
                .context("failed to pause the global hotkeys")?;
            self.gesture = None;
            self.auto_monitor = None;
            self.auto_tracker.clear();
        }
        self.paused = !self.paused;
        self.update_tray()?;
        info!(paused = self.paused, "updated shortcut state");
        Ok(())
    }

    fn reload_configuration(&mut self) -> Result<()> {
        let new_config = Config::load()?;
        let (new_hotkey, new_last_word_hotkey) = parse_hotkeys(&new_config)?;
        let new_gesture = if self.paused {
            None
        } else {
            create_gesture_monitor(&new_config, &self.event_proxy)?
        };
        let new_auto_monitor = if self.paused {
            None
        } else {
            create_auto_monitor_or_log(&new_config, &self.event_proxy)
        };

        if !self.paused
            && (new_hotkey != self.hotkey || new_last_word_hotkey != self.last_word_hotkey)
        {
            self.manager
                .unregister_all(&[self.hotkey, self.last_word_hotkey])
                .context("failed to unregister the previous hotkeys")?;
            if let Err(error) = self
                .manager
                .register_all(&[new_hotkey, new_last_word_hotkey])
            {
                let _ = self
                    .manager
                    .register_all(&[self.hotkey, self.last_word_hotkey]);
                return Err(error).context("failed to register the reloaded hotkeys");
            }
        }

        self.config = new_config;
        self.hotkey = new_hotkey;
        self.last_word_hotkey = new_last_word_hotkey;
        self.gesture = new_gesture;
        self.auto_monitor = new_auto_monitor;
        self.auto_tracker.clear();
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

    fn handle_menu(&mut self, event_loop: &ActiveEventLoop, event: &MenuEvent) {
        let action = self.tray.as_ref().and_then(|tray| tray.action(event));
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
            Some(TrayAction::OpenSettings) => settings::spawn(),
            Some(TrayAction::ReloadConfiguration) => self.reload_configuration(),
            Some(TrayAction::ToggleAutostart) => self.toggle_autostart(),
            Some(TrayAction::Quit) => {
                event_loop.exit();
                Ok(())
            }
            None => Ok(()),
        };

        if let Err(error) = result {
            error!(%error, "tray action failed");
            let _ = self.update_tray();
        }
    }

    fn toggle_autostart(&self) -> Result<()> {
        let status = autostart::status()?;
        let status = if status.enabled {
            autostart::disable()?
        } else {
            autostart::enable()?
        };
        self.update_tray()?;
        info!(
            enabled = status.enabled,
            location = status.location,
            "updated launch-at-login state"
        );
        Ok(())
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        if self.tray.is_none() {
            match Tray::new(&self.config) {
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
            AppEvent::HotKey(event)
                if !self.paused
                    && event.id == self.hotkey.id()
                    && event.state == HotKeyState::Released =>
            {
                self.perform_conversion(false);
            }
            AppEvent::HotKey(event)
                if !self.paused
                    && event.id == self.last_word_hotkey.id()
                    && event.state == HotKeyState::Released =>
            {
                self.perform_conversion(true);
            }
            AppEvent::Menu(event) => self.handle_menu(event_loop, &event),
            AppEvent::ModifierGesture(action) if !self.paused => {
                self.perform_conversion(action == GestureAction::PreviousWord);
            }
            AppEvent::AutoKey(event) => self.handle_auto_key(event),
            AppEvent::ReloadConfiguration => {
                if let Err(error) = self.reload_configuration() {
                    error!(%error, "could not reload the changed configuration");
                }
            }
            AppEvent::ModifierGesture(_) => {}
            AppEvent::HotKey(_) => {}
        }
    }

    #[cfg(target_os = "linux")]
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        use std::time::{Duration, Instant};
        use winit::event_loop::ControlFlow;

        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(50),
        ));
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
