use crate::settings::i18n::tr;
use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{collections::HashSet, time::Instant};

#[cfg(not(target_os = "macos"))]
use anyhow::anyhow;
use anyhow::{Context, Result, bail};
use eframe::egui;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use single_instance::SingleInstance;
use upyr_audio::KeyCue;

use crate::{
    autostart,
    config::{AutoCorrectSensitivity, Config, config_path},
};
use crate::{
    config::{GestureAction, IndicatorStyle, ModifierGesture, SoundEvent, SoundPack},
    layout::Direction,
};

mod draft;
#[cfg(any(target_os = "macos", test))]
mod flag_choices;
mod i18n;
mod navigation;
mod persistence;
mod pixel;
mod tui;
mod updates;
#[cfg(any(target_os = "macos", test))]
use draft::normalize_exceptions;
use draft::parse_exceptions;
use navigation::{
    CONTEXT_HELP, CORRECTION_DESCRIPTION, CORRECTION_FOOTNOTE, CORRECTION_TITLE, CORRECTION_TOGGLE,
    MIDWORD_HELP,
};
use navigation::{SEARCH_PARAMETERS, SettingsTab};
#[cfg(target_os = "macos")]
mod macos;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROJECT_WEBSITE_URL: &str = "https://upyr.org";
const REPOSITORY_URL: &str = "https://github.com/dmytro-yemelianov/upyr";
const PRIVACY_SUMMARY: &str = "Local-only by design: no accounts, analytics, telemetry, ads, or text uploads. Typed text and clipboard contents stay on this device.";
const IMPLEMENTATION_SUMMARY: &str = "Upyr is written in Rust. It maps physical English and Ukrainian keys, then uses a bundled compact character n-gram model to score language candidates locally; no cloud inference is involved.";
const ACTIVATION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const ACTIVATION_TTL: Duration = Duration::from_secs(30);

static ACTIVATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A bounded, per-user command queue used to focus the already-running
/// settings process without opening a network socket or sending user text.
#[derive(Debug, Clone)]
struct ActivationInbox {
    directory: PathBuf,
}

impl ActivationInbox {
    fn open() -> Result<Self> {
        let directory = config_path()?.with_file_name("upyr-settings-activation");
        Self::at(directory)
    }

    fn at(directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory).with_context(|| {
            format!(
                "failed to create settings activation directory {}",
                directory.display()
            )
        })?;
        #[cfg(unix)]
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).with_context(|| {
            format!(
                "failed to secure settings activation directory {}",
                directory.display()
            )
        })?;
        Ok(Self { directory })
    }

    fn send(&self, tab: SettingsTab) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let sequence = ACTIVATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let stem = format!("{timestamp:020}-{:010}-{sequence:020}", std::process::id());
        let temporary = self.directory.join(format!(".{stem}.tmp"));
        let command = self.directory.join(format!("{stem}.cmd"));

        let result = (|| -> Result<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&temporary).with_context(|| {
                format!(
                    "failed to create settings activation command {}",
                    temporary.display()
                )
            })?;
            file.write_all(tab.command().as_bytes())
                .context("failed to write settings activation command")?;
            file.sync_all()
                .context("failed to persist settings activation command")?;
            drop(file);
            fs::rename(&temporary, &command).with_context(|| {
                format!(
                    "failed to publish settings activation command {}",
                    command.display()
                )
            })?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn drain(&self) -> Result<Option<SettingsTab>> {
        let mut commands = Vec::new();
        for entry in fs::read_dir(&self.directory).with_context(|| {
            format!(
                "failed to read settings activation directory {}",
                self.directory.display()
            )
        })? {
            let entry = entry.context("failed to inspect a settings activation command")?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("cmd")
                || !entry
                    .file_type()
                    .context("failed to inspect a settings activation command type")?
                    .is_file()
            {
                continue;
            }
            commands.push(entry.path());
        }
        commands.sort_unstable();

        let now = SystemTime::now();
        let mut requested_tab = None;
        for command in commands {
            let metadata = fs::metadata(&command).ok();
            let is_fresh = metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .is_some_and(|modified| match now.duration_since(modified) {
                    Ok(age) => age <= ACTIVATION_TTL,
                    Err(_) => true,
                });
            let value = if is_fresh && metadata.is_some_and(|metadata| metadata.len() <= 32) {
                fs::read_to_string(&command).ok()
            } else {
                None
            };
            let _ = fs::remove_file(&command);
            if let Some(tab) = value
                .as_deref()
                .map(str::trim)
                .and_then(SettingsTab::from_command)
            {
                requested_tab = Some(tab);
            }
        }
        Ok(requested_tab)
    }
}

/// Optional editors use the same configuration and settings-instance guard.
#[derive(Debug, Clone, Default, clap::Args)]
pub struct SettingsOptions {
    /// Open the complete pixel-style settings editor.
    #[arg(long, conflicts_with_all = ["tui", "about"])]
    pub pixel: bool,
    /// Open a local line-oriented terminal settings editor.
    #[arg(long, conflicts_with = "about")]
    pub tui: bool,
    #[arg(long, hide = true)]
    pub about: bool,
}

pub fn run() -> Result<()> {
    run_with(SettingsOptions {
        about: requested_tab() == SettingsTab::About,
        ..Default::default()
    })
}

pub fn run_with(options: SettingsOptions) -> Result<()> {
    anyhow::ensure!(
        !(options.pixel && options.tui) && !(options.about && (options.pixel || options.tui)),
        "Choose one settings interface"
    );
    let requested_tab = if options.about {
        SettingsTab::About
    } else {
        SettingsTab::General
    };
    let frontend = if options.pixel {
        "pixel"
    } else if options.tui {
        "tui"
    } else {
        "native"
    };
    let instance_key = settings_instance_key()?;
    let instance = SingleInstance::new(&instance_key)
        .context("failed to create the settings single-instance guard")?;
    let activation = ActivationInbox::open()?;
    let mode_path = activation.directory.join("frontend");
    if !instance.is_single() {
        let current = fs::read_to_string(&mode_path).unwrap_or_else(|_| "native".into());
        anyhow::ensure!(
            frontend == current && frontend != "tui",
            "Settings is already open in {current} mode. Close that editor before choosing another interface."
        );
        return activation.send(requested_tab);
    }
    fs::write(&mode_path, frontend).context("could not record settings interface")?;
    struct FrontendLease(PathBuf);
    impl Drop for FrontendLease {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let _frontend_lease = FrontendLease(mode_path);
    if options.tui {
        return tui::run();
    }
    if options.pixel {
        let open_full_settings = pixel::run(activation)?;
        // The next editor must acquire its own guard after this window closes.
        drop(_frontend_lease);
        drop(instance);
        return if open_full_settings { spawn() } else { Ok(()) };
    }

    // Keep the existing window available to inspect defaults. Saving still
    // requires a readable current file so concurrent edits are not lost.
    let (config, problem) = Config::load_or_default();
    i18n::set_language(config.ui_language);
    let problem = problem.map(|problem| {
        format!("Could not read the settings file ({problem}). Showing defaults; repair the file before saving.")
    });
    let autostart_status = autostart::status()?;
    let initial_tab = activation.drain()?.unwrap_or(requested_tab);
    #[cfg(target_os = "macos")]
    return macos::run(config, autostart_status, initial_tab, activation, problem);

    #[cfg(not(target_os = "macos"))]
    {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title(tr("Upyr Settings"))
                .with_inner_size([680.0, 800.0])
                .with_min_inner_size([520.0, 560.0]),
            ..native_options()
        };
        eframe::run_native(
            tr("Upyr Settings"),
            options,
            Box::new(move |_context| {
                let mut app = SettingsApp::new(config, autostart_status, initial_tab, activation);
                app.status = problem.map(|problem| (false, problem));
                Ok(Box::new(app))
            }),
        )
        .map_err(|error| anyhow!(error.to_string()))
    }
}

/// Both egui frontends use the same platform renderer. Windows' system OpenGL
/// implementation can be too old over RDP; DX12 can use the built-in WARP
/// software adapter when no suitable hardware adapter is available.
fn native_options() -> eframe::NativeOptions {
    #[cfg(target_os = "windows")]
    {
        let mut options = eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        };
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup
        {
            setup.instance_descriptor.backends = wgpu::Backends::DX12;
        }
        options
    }
    #[cfg(not(target_os = "windows"))]
    eframe::NativeOptions::default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutostartTransition {
    None,
    Enable,
    Disable,
    NeedsExplicitRemoval,
}

fn autostart_transition(desired: bool, state: autostart::AutostartState) -> AutostartTransition {
    use autostart::AutostartState::{Broken, Disabled, Enabled, Stale};

    match (desired, state) {
        (true, Disabled | Stale) => AutostartTransition::Enable,
        (false, Enabled) => AutostartTransition::Disable,
        (true, Broken) => AutostartTransition::NeedsExplicitRemoval,
        (true, Enabled) | (false, Disabled | Stale | Broken) => AutostartTransition::None,
    }
}

fn sync_launch_at_login(desired: bool) -> Result<autostart::AutostartStatus> {
    let status = autostart::status()?;
    match autostart_transition(desired, status.state) {
        AutostartTransition::None => Ok(status),
        AutostartTransition::Enable => autostart::enable(),
        AutostartTransition::Disable => autostart::disable(),
        AutostartTransition::NeedsExplicitRemoval => {
            bail!(
                "the existing launch-at-login entry is broken and will not be overwritten; choose Remove Entry explicitly, then enable it again"
            )
        }
    }
}

fn autostart_attention(status: &autostart::AutostartStatus) -> Option<String> {
    let action = match status.state {
        autostart::AutostartState::Stale => tr(
            "Launch at login points to another Upyr installation. Choose Repair Entry to update it, or Remove Entry to disable it.",
        ),
        autostart::AutostartState::Broken => tr(
            "The launch-at-login entry needs attention and Upyr will not overwrite it. Choose Remove Entry explicitly before enabling it again.",
        ),
        autostart::AutostartState::Disabled | autostart::AutostartState::Enabled => return None,
    };
    Some(match status.detail.as_deref() {
        Some(detail) => format!("{action} {detail}"),
        None => action.to_owned(),
    })
}

fn settings_instance_key() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let path = config_path()?.with_file_name("upyr-settings.lock");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create settings runtime directory {}",
                    parent.display()
                )
            })?;
        }
        Ok(path.to_string_lossy().into_owned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok("dev.Upyr.Upyr.Settings".to_owned())
    }
}

/// Returns true while the separate settings process owns its single-instance
/// guard. Global actions should stay quiet in that state so recording an
/// existing shortcut does not also trigger a conversion in the background app.
pub fn is_open() -> bool {
    let Ok(key) = settings_instance_key() else {
        return false;
    };
    instance_is_held(&key)
}

fn instance_is_held(key: &str) -> bool {
    SingleInstance::new(key).is_ok_and(|instance| !instance.is_single())
}

pub fn spawn() -> Result<()> {
    spawn_tab(SettingsTab::General)
}

/// The tray's primary settings action uses the pixel-style editor.
pub fn spawn_pixel() -> Result<()> {
    spawn_editor(SettingsTab::General, true)
}

pub fn spawn_about() -> Result<()> {
    spawn_tab(SettingsTab::About)
}

fn spawn_tab(tab: SettingsTab) -> Result<()> {
    spawn_editor(tab, false)
}

fn spawn_editor(tab: SettingsTab, pixel: bool) -> Result<()> {
    if is_open() {
        let inbox = ActivationInbox::open()?;
        let current = fs::read_to_string(inbox.directory.join("frontend"))
            .unwrap_or_else(|_| "native".into());
        anyhow::ensure!(
            current == if pixel { "pixel" } else { "native" },
            "Settings is already open in {current} mode. Close that editor before choosing another interface."
        );
        return inbox.send(tab);
    }

    let argument = if pixel {
        Some("--pixel")
    } else {
        tab.cli_argument()
    };
    #[cfg(target_os = "macos")]
    if let Some(bundle) = packaged_macos_bundle()? {
        let mut command = Command::new("open");
        command.arg(&bundle);
        if let Some(argument) = argument {
            command.args(["--args", argument]);
        }
        let child = command
            .spawn()
            .with_context(|| format!("failed to open Upyr Settings at {}", bundle.display()))?;
        if child.id() == 0 {
            bail!("macOS did not start Upyr Settings");
        }
        crate::child_processes::track(child);
        return Ok(());
    }

    let path = settings_executable()?;
    let mut command = Command::new(&path);
    if let Some(argument) = argument {
        command.arg(argument);
    }
    let child = command
        .spawn()
        .with_context(|| format!("failed to open Upyr Settings at {}", path.display()))?;
    if child.id() == 0 {
        bail!("the operating system did not start Upyr Settings");
    }
    crate::child_processes::track(child);
    Ok(())
}

fn settings_executable() -> Result<PathBuf> {
    let current = env::current_exe().context("could not locate the Upyr executable")?;
    #[cfg(target_os = "macos")]
    if let Some(contents) = current.parent().and_then(|macos| macos.parent()) {
        let nested = contents.join("Helpers/Upyr Settings.app/Contents/MacOS/upyr-settings");
        if nested.is_file() {
            return Ok(nested);
        }
    }
    let executable =
        current.with_file_name(format!("upyr-settings{}", std::env::consts::EXE_SUFFIX));
    if executable.is_file() {
        Ok(executable)
    } else {
        bail!(
            "Upyr Settings is missing at {}; build or reinstall the complete application",
            executable.display()
        )
    }
}

#[cfg(target_os = "macos")]
fn packaged_macos_bundle() -> Result<Option<PathBuf>> {
    let current = env::current_exe().context("could not locate the Upyr executable")?;
    let Some(contents) = current.parent().and_then(|macos| macos.parent()) else {
        return Ok(None);
    };
    let bundle = contents.join("Helpers/Upyr Settings.app");
    Ok(bundle.is_dir().then_some(bundle))
}

fn requested_tab() -> SettingsTab {
    if env::args_os().any(|argument| argument == "--about") {
        SettingsTab::About
    } else {
        SettingsTab::General
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutSlot {
    Selection,
    PreviousWord,
    #[cfg(target_os = "macos")]
    Undo,
}

struct SettingsApp {
    config: Config,
    /// The configuration as loaded or last saved, the base of a save's merge.
    loaded: Config,
    new_exception: String,
    launch_at_login: bool,
    autostart_status: autostart::AutostartStatus,
    activation: ActivationInbox,
    next_activation_poll: Instant,
    status: Option<(bool, String)>,
    style_applied: bool,
    tab: SettingsTab,
    search: String,
    recording: Option<ShortcutSlot>,
    shortcut_error: Option<String>,
    updates: updates::UpdateCheck,
    #[cfg(target_os = "macos")]
    flags: pixel::flags::FlagPicker,
    #[cfg(target_os = "macos")]
    audio_status: Option<(Instant, String)>,
}

impl SettingsApp {
    fn new(
        config: Config,
        autostart_status: autostart::AutostartStatus,
        initial_tab: SettingsTab,
        activation: ActivationInbox,
    ) -> Self {
        let launch_at_login = autostart_status.enabled;
        Self {
            loaded: config.clone(),
            config,
            new_exception: String::new(),
            launch_at_login,
            autostart_status,
            activation,
            next_activation_poll: Instant::now(),
            status: None,
            style_applied: false,
            tab: initial_tab,
            search: String::new(),
            recording: None,
            shortcut_error: None,
            updates: updates::UpdateCheck::default(),
            #[cfg(target_os = "macos")]
            flags: pixel::flags::FlagPicker::default(),
            #[cfg(target_os = "macos")]
            audio_status: None,
        }
    }

    fn poll_activation(&mut self, context: &egui::Context) {
        let now = Instant::now();
        if now < self.next_activation_poll {
            return;
        }
        self.next_activation_poll = now + ACTIVATION_POLL_INTERVAL;

        match self.activation.drain() {
            Ok(Some(tab)) => {
                self.tab = tab;
                self.search.clear();
                context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                context.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                context.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            Ok(None) => {}
            Err(error) => {
                self.status = Some((
                    false,
                    crate::settings::i18n::localized_format!(
                        "Could not activate the settings window: {error:#}",
                        "Не вдалося активувати вікно налаштувань: {error:#}"
                    ),
                ));
            }
        }
    }

    fn save(&mut self) {
        let config = match persistence::save_edits(&self.loaded, self.config.clone()) {
            Ok(config) => config,
            Err(error) => {
                self.status = Some((
                    false,
                    crate::settings::i18n::localized_format!(
                        "Could not save settings: {error:#}",
                        "Не вдалося зберегти налаштування: {error:#}"
                    ),
                ));
                return;
            }
        };
        self.loaded = config.clone();
        self.config = config;
        self.save_startup();
    }

    fn startup_dirty(&self) -> bool {
        self.launch_at_login != self.autostart_status.enabled
    }

    fn save_startup(&mut self) {
        if !self.startup_dirty() {
            self.status = Some((true, tr("Saved. Upyr will reload these settings.").into()));
            return;
        }
        self.accept_startup_result(sync_launch_at_login(self.launch_at_login));
    }

    fn accept_startup_result(&mut self, result: Result<autostart::AutostartStatus>) {
        match result {
            Ok(status) => {
                self.launch_at_login = status.enabled;
                self.autostart_status = status;
                self.status = Some(match autostart_attention(&self.autostart_status) {
                    Some(attention) => (
                        false,
                        crate::settings::i18n::localized_format!(
                            "Settings saved. {attention}",
                            "Налаштування збережено. {attention}"
                        ),
                    ),
                    None => (
                        true,
                        tr("Saved. The running background app will reload these settings.")
                            .to_owned(),
                    ),
                });
            }
            Err(error) => {
                self.status = Some((
                    false,
                    crate::settings::i18n::localized_format!(
                        "Settings saved, but launch at login failed: {error:#}. Your choice is kept; Save retries it.",
                        "Налаштування збережено, але автозапуск не змінився: {error:#}. Ваш вибір залишено; «Зберегти» повторить спробу."
                    ),
                ));
            }
        }
    }

    fn repair_autostart(&mut self) {
        match autostart::enable() {
            Ok(status) => {
                self.launch_at_login = status.enabled;
                self.autostart_status = status;
                self.status = Some((true, tr("Launch-at-login entry repaired.").to_owned()));
            }
            Err(error) => {
                self.status = Some((
                    false,
                    crate::settings::i18n::localized_format!(
                        "Could not repair entry: {error:#}",
                        "Не вдалося виправити запис: {error:#}"
                    ),
                ));
            }
        }
    }

    fn remove_autostart(&mut self) {
        match autostart::disable() {
            Ok(status) => {
                self.launch_at_login = status.enabled;
                self.autostart_status = status;
                self.status = Some((true, tr("Launch-at-login entry removed.").to_owned()));
            }
            Err(error) => {
                self.status = Some((
                    false,
                    crate::settings::i18n::localized_format!(
                        "Could not remove entry: {error:#}",
                        "Не вдалося видалити запис: {error:#}"
                    ),
                ));
            }
        }
    }

    fn reset(&mut self) {
        self.config = Config::default();
        self.new_exception.clear();
        self.recording = None;
        self.shortcut_error = None;
        self.status = Some((
            true,
            tr("Defaults restored in the form. Choose Save to apply them.").to_owned(),
        ));
    }

    fn capture_shortcut(&mut self, context: &egui::Context) {
        let Some(slot) = self.recording else {
            return;
        };
        let event = context.input(|input| {
            input.events.iter().rev().find_map(|event| match event {
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                } => Some((physical_key.unwrap_or(*key), *modifiers)),
                _ => None,
            })
        });
        let Some((key, modifiers)) = event else {
            return;
        };
        if key == egui::Key::Escape {
            self.recording = None;
            self.shortcut_error = None;
            return;
        }

        let Some(code) = egui_key_to_code(key) else {
            self.shortcut_error =
                Some(tr("That key cannot be used in a global shortcut.").to_owned());
            return;
        };
        let modifiers = hotkey_modifiers(modifiers);
        if modifiers.is_empty() {
            self.shortcut_error = Some(
                tr("Include Command, Control, Option/Alt, or Shift so normal typing is never captured.")
                    .to_owned(),
            );
            return;
        }

        let value = HotKey::new(Some(modifiers), code).to_string();
        let mut candidate = self.config.clone();
        match slot {
            ShortcutSlot::Selection => candidate.hotkey = value,
            ShortcutSlot::PreviousWord => candidate.last_word_hotkey = value,
            #[cfg(target_os = "macos")]
            ShortcutSlot::Undo => candidate.undo_hotkey = value,
        }
        if let Err(error) = candidate.validate() {
            self.shortcut_error = Some(format!("{error:#}"));
            return;
        }
        self.config = candidate;
        self.recording = None;
        self.shortcut_error = None;
    }

    fn draw_general(&mut self, ui: &mut egui::Ui) {
        section_heading(
            ui,
            tr("General"),
            tr("Core conversion, clipboard, and startup behavior."),
        );
        egui::Grid::new("general-grid")
            .num_columns(2)
            .spacing([20.0, 12.0])
            .show(ui, |ui| {
                ui.label(tr("Interface language"));
                let language_picker = egui::ComboBox::from_id_salt("ui-language")
                    .selected_text(self.config.ui_language.label())
                    .show_ui(ui, |ui| {
                        for language in crate::config::UiLanguage::ALL {
                            ui.selectable_value(
                                &mut self.config.ui_language,
                                language,
                                language.label(),
                            );
                        }
                    });
                language_picker.response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::ComboBox,
                        ui.is_enabled(),
                        format!(
                            "{}: {}",
                            tr("Interface language"),
                            self.config.ui_language.label()
                        ),
                    )
                });
                ui.end_row();
                ui.label(tr("Conversion direction"));
                egui::ComboBox::from_id_salt("direction")
                    .selected_text(direction_label(self.config.direction))
                    .show_ui(ui, |ui| {
                        for value in [
                            Direction::Smart,
                            Direction::EnglishToUkrainian,
                            Direction::UkrainianToEnglish,
                        ] {
                            ui.selectable_value(
                                &mut self.config.direction,
                                value,
                                direction_label(value),
                            );
                        }
                    });
                ui.end_row();
            });
        ui.add_space(6.0);
        ui.checkbox(
            &mut self.config.switch_layout,
            tr("Switch the OS layout to match converted text"),
        );
        ui.checkbox(
            &mut self.config.restore_clipboard,
            tr("Restore clipboard contents after conversion"),
        );
        #[cfg(target_os = "macos")]
        {
            ui.checkbox(
                &mut self.config.globe_switch.enabled,
                tr("Tap Globe to switch keyboard layouts"),
            );
            ui.small(tr(
                "Keeps fn shortcuts. Restores macOS handling when paused or quit.",
            ));
        }
        ui.checkbox(&mut self.launch_at_login, tr("Launch Upyr at login"));
        if let Some(attention) = autostart_attention(&self.autostart_status) {
            ui.colored_label(egui::Color32::from_rgb(180, 105, 20), attention);
            ui.horizontal_wrapped(|ui| {
                if self.autostart_status.state == autostart::AutostartState::Stale
                    && ui.button(tr("Repair Entry")).clicked()
                {
                    self.repair_autostart();
                }
                if ui.button(tr("Remove Entry")).clicked() {
                    self.remove_autostart();
                }
            });
        }
    }

    fn draw_automatic(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, CORRECTION_TITLE, CORRECTION_DESCRIPTION);
        ui.checkbox(&mut self.config.auto_correct, tr(CORRECTION_TOGGLE));
        ui.small(
            tr("Opt-in: a short input prefix is kept only in memory, cleared at safe boundaries, and never logged."),
        );
        #[cfg(target_os = "macos")]
        ui.colored_label(
            egui::Color32::from_rgb(190, 125, 35),
            tr("macOS requires Accessibility access. Upyr checks existing access first and offers one restart after permission is granted."),
        );
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label(tr(MIDWORD_HELP));
            ui.add_space(4.0);
            ui.label(tr(CONTEXT_HELP));
        });
        ui.add_space(8.0);
        ui.add_enabled_ui(self.config.auto_correct, |ui| {
            egui::Grid::new("automatic-correction-grid")
                .num_columns(2)
                .spacing([20.0, 12.0])
                .show(ui, |ui| {
                    ui.label(tr("Sensitivity"));
                    egui::ComboBox::from_id_salt("auto-correct-sensitivity")
                        .selected_text(sensitivity_label(self.config.auto_correct_sensitivity))
                        .show_ui(ui, |ui| {
                            for value in [
                                AutoCorrectSensitivity::Conservative,
                                AutoCorrectSensitivity::Balanced,
                                AutoCorrectSensitivity::Aggressive,
                            ] {
                                ui.selectable_value(
                                    &mut self.config.auto_correct_sensitivity,
                                    value,
                                    sensitivity_label(value),
                                );
                            }
                        });
                    ui.end_row();
                    ui.label(tr("Minimum word length"));
                    ui.add(
                        egui::DragValue::new(&mut self.config.auto_correct_min_word_length)
                            .range(2..=32),
                    );
                    ui.end_row();
                    ui.label(tr("Pause after Space")).on_hover_text(tr("This delay applies after Space; a clear prefix can switch immediately during typing."));
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.config.auto_correct_delay_ms)
                                .range(10..=250),
                        );
                        ui.label(tr("ms"));
                    });
                    ui.end_row();
                });
        });
        ui.small(tr(CORRECTION_FOOTNOTE));
    }

    fn draw_exceptions(&mut self, ui: &mut egui::Ui) {
        section_heading(
            ui,
            tr("Never correct"),
            tr("Exact words that automatic correction must always leave alone."),
        );
        ui.add_space(8.0);

        let mut remove_index = None;
        egui::Grid::new("exceptions-table")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                ui.strong(tr("Word"));
                ui.end_row();
                for (index, word) in self.config.auto_correct_exceptions.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(word).desired_width(260.0));
                    if ui.small_button(tr("Remove")).clicked() {
                        remove_index = Some(index);
                    }
                    ui.end_row();
                }
            });
        if let Some(index) = remove_index {
            self.config.auto_correct_exceptions.remove(index);
        }
        if self.config.auto_correct_exceptions.is_empty() {
            ui.small(tr("No exceptions yet."));
        }

        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_exception)
                    .desired_width(260.0)
                    .hint_text(tr("words separated by spaces or commas")),
            );
            if ui.button(tr("Add")).clicked() {
                self.add_exceptions();
            }
        });
        ui.small(
            tr("Spaces and commas separate words, so \"two words\" adds two exceptions. Case-insensitive duplicates and blank entries are ignored."),
        );
    }

    fn add_exceptions(&mut self) {
        let mut seen: HashSet<String> = self
            .config
            .auto_correct_exceptions
            .iter()
            .map(|word| word.to_lowercase())
            .collect();
        for word in parse_exceptions(&self.new_exception) {
            if seen.insert(word.to_lowercase()) {
                self.config.auto_correct_exceptions.push(word);
            }
        }
        self.new_exception.clear();
    }

    fn draw_shortcuts(&mut self, ui: &mut egui::Ui) {
        section_heading(
            ui,
            tr("Shortcuts"),
            tr("Click a recorder, then press the combination you want."),
        );
        self.shortcut_row(ui, tr("Convert selection"), ShortcutSlot::Selection);
        ui.add_space(8.0);
        self.shortcut_row(ui, tr("Fix previous word"), ShortcutSlot::PreviousWord);
        #[cfg(target_os = "macos")]
        {
            ui.add_space(8.0);
            self.shortcut_row(ui, tr("Undo auto-correction"), ShortcutSlot::Undo);
            ui.small(tr(
                "Undo is available for 30 seconds, until further input or navigation.",
            ));
        }
        if let Some(error) = &self.shortcut_error {
            ui.add_space(6.0);
            ui.colored_label(
                egui::Color32::from_rgb(190, 55, 55),
                i18n::diagnostic(error),
            );
        }
        ui.add_space(18.0);
        section_heading(
            ui,
            tr("Modifier gesture"),
            tr("An optional double-tap trigger without a regular key."),
        );
        egui::Grid::new("gesture-grid")
            .num_columns(2)
            .spacing([20.0, 12.0])
            .show(ui, |ui| {
                ui.label(tr("Gesture"));
                egui::ComboBox::from_id_salt("modifier-gesture")
                    .selected_text(gesture_label(self.config.modifier_gesture))
                    .show_ui(ui, |ui| {
                        for value in [
                            ModifierGesture::Disabled,
                            ModifierGesture::DoubleControl,
                            ModifierGesture::DoubleShift,
                            ModifierGesture::DoubleControlShift,
                        ] {
                            ui.selectable_value(
                                &mut self.config.modifier_gesture,
                                value,
                                gesture_label(value),
                            );
                        }
                    });
                ui.end_row();
                ui.label(tr("Action"));
                egui::ComboBox::from_id_salt("gesture-action")
                    .selected_text(gesture_action_label(self.config.modifier_gesture_action))
                    .show_ui(ui, |ui| {
                        for value in [GestureAction::PreviousWord, GestureAction::Selection] {
                            ui.selectable_value(
                                &mut self.config.modifier_gesture_action,
                                value,
                                gesture_action_label(value),
                            );
                        }
                    });
                ui.end_row();
                ui.label(tr("Double-tap timeout"));
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.config.modifier_gesture_timeout_ms)
                            .range(150..=2_000),
                    );
                    ui.label(tr("ms"));
                });
                ui.end_row();
            });
    }

    fn shortcut_row(&mut self, ui: &mut egui::Ui, label: &str, slot: ShortcutSlot) {
        let active = self.recording == Some(slot);
        let value = match slot {
            ShortcutSlot::Selection => self.config.hotkey.clone(),
            ShortcutSlot::PreviousWord => self.config.last_word_hotkey.clone(),
            #[cfg(target_os = "macos")]
            ShortcutSlot::Undo => self.config.undo_hotkey.clone(),
        };
        ui.label(egui::RichText::new(tr(label)).strong());
        ui.horizontal_wrapped(|ui| {
            let text = if active {
                tr("Press shortcut…  (Esc to cancel)").to_owned()
            } else {
                text_hotkey(&value).unwrap_or_else(|| value.clone())
            };
            let response = ui.add_sized(
                [310.0, 38.0],
                egui::Button::new(egui::RichText::new(text).monospace())
                    .wrap()
                    .selected(active),
            );
            if response.clicked() {
                self.recording = Some(slot);
                self.shortcut_error = None;
            }
            let default = Config::default();
            let default_value = match slot {
                ShortcutSlot::Selection => default.hotkey,
                ShortcutSlot::PreviousWord => default.last_word_hotkey,
                #[cfg(target_os = "macos")]
                ShortcutSlot::Undo => default.undo_hotkey,
            };
            if ui
                .add_enabled(value != default_value, egui::Button::new(tr("Reset")))
                .clicked()
            {
                match slot {
                    ShortcutSlot::Selection => self.config.hotkey = default_value,
                    ShortcutSlot::PreviousWord => self.config.last_word_hotkey = default_value,
                    #[cfg(target_os = "macos")]
                    ShortcutSlot::Undo => self.config.undo_hotkey = default_value,
                }
                self.recording = None;
                self.shortcut_error = None;
            }
        });
        ui.small(tr(
            "The shortcut uses physical keys, so it keeps working in either keyboard layout.",
        ));
    }

    fn preview_key_sound(&mut self, cue: KeyCue, label: &str) {
        let label = tr(label);
        let mut preview_settings = self.config.sounds;
        preview_settings.enabled = true;
        preview_settings.key_clicks = true;
        self.status = Some(
            match crate::feedback::preview_key_sound(cue, &preview_settings) {
                Ok(()) => (
                    true,
                    crate::settings::i18n::localized_format!(
                        "Previewed {label}.",
                        "Відтворено: {label}."
                    ),
                ),
                Err(error) => (
                    false,
                    crate::settings::i18n::localized_format!(
                        "Could not preview {label}: {error:#}",
                        "Не вдалося відтворити {label}: {error:#}"
                    ),
                ),
            },
        );
    }

    #[cfg(target_os = "macos")]
    fn refresh_audio_status(&mut self) {
        if self
            .audio_status
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed() >= Duration::from_secs(1))
        {
            let route = crate::audio_output::current();
            self.audio_status = Some((
                Instant::now(),
                format!("{} · {}", route.name, tr(route.kind.label())),
            ));
        }
    }

    fn draw_feedback(&mut self, ui: &mut egui::Ui) {
        section_heading(
            ui,
            tr("Visual feedback"),
            tr("Optional confirmation after Upyr changes the active input source."),
        );
        ui.checkbox(
            &mut self.config.show_layout_indicator,
            tr("Show a temporary language flag next to the pointer"),
        );
        ui.add_enabled_ui(self.config.show_layout_indicator, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("Visible for"));
                ui.add(
                    egui::DragValue::new(&mut self.config.layout_indicator_duration_ms)
                        .range(250..=3_000),
                );
                ui.label(tr("ms"));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("Shows"));
                egui::ComboBox::from_id_salt("indicator-style")
                    .selected_text(tr(self.config.indicator_style.label()))
                    .show_ui(ui, |ui| {
                        for style in IndicatorStyle::ALL {
                            ui.selectable_value(
                                &mut self.config.indicator_style,
                                style,
                                tr(style.label()),
                            );
                        }
                    });
            });
        });
        ui.add_space(18.0);
        #[cfg(target_os = "macos")]
        self.flags.show(ui, &mut self.config, &mut self.status);
        section_heading(
            ui,
            tr("Sound feedback"),
            tr("Choose which Upyr actions play a subtle confirmation sound."),
        );
        ui.checkbox(&mut self.config.sounds.enabled, tr("Enable sound feedback"));
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Volume"));
            ui.add(egui::Slider::new(&mut self.config.sounds.volume_percent, 0..=100).suffix("%"));
        });
        #[cfg(target_os = "macos")]
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("Headphone limit"));
                ui.add(
                    egui::Slider::new(&mut self.config.sounds.headphone_volume_percent, 0..=30)
                        .suffix("%"),
                );
            });
            if let Some((_, text)) = &self.audio_status {
                ui.small(text);
            }
            ui.small(tr("Quiet limit for headphones and unrecognized outputs. Does not change system volume."));
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Sound pack"));
            egui::ComboBox::from_id_salt("sound-pack")
                .selected_text(self.config.sounds.pack.label())
                .show_ui(ui, |ui| {
                    for pack in SoundPack::ALL {
                        ui.selectable_value(&mut self.config.sounds.pack, pack, pack.label());
                    }
                });
        });
        ui.small(tr(self.config.sounds.pack.description()));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(
                &mut self.config.sounds.key_clicks,
                tr("Keyboard sounds (control-key reactions in Anime pack)"),
            );
            if ui.button(tr("Preview click")).clicked() {
                self.preview_key_sound(KeyCue::Character, "typing click");
            }
            if ui.button(tr("Preview control")).clicked() {
                self.preview_key_sound(KeyCue::Space, "control-key reaction");
            }
        });
        ui.small(
            tr("Keyboard sounds are opt-in and use only physical key categories; no typed text is retained."),
        );
        ui.add_space(8.0);
        egui::Grid::new("sound-feedback-grid")
            .num_columns(2)
            .spacing([20.0, 8.0])
            .show(ui, |ui| {
                for event in SoundEvent::ALL {
                    let mut selected = self.config.sounds.event_selected(event);
                    if ui.checkbox(&mut selected, tr(event.label())).changed() {
                        self.config.sounds.set_event_selected(event, selected);
                    }
                    if ui.button(tr("Preview")).clicked() {
                        let mut preview_settings = self.config.sounds;
                        preview_settings.enabled = true;
                        preview_settings.set_event_selected(event, true);
                        self.status = Some(
                            match crate::feedback::preview_sound(
                                event,
                                &preview_settings,
                                &self.config.globe_switch,
                            ) {
                                Ok(()) => (
                                    true,
                                    crate::settings::i18n::localized_format!(
                                        "Previewed {} sound.",
                                        "Відтворено звук «{}».",
                                        tr(event.label())
                                    ),
                                ),
                                Err(error) => (
                                    false,
                                    crate::settings::i18n::localized_format!(
                                        "Could not preview {} sound: {error:#}",
                                        "Не вдалося відтворити звук «{}»: {error:#}",
                                        tr(event.label())
                                    ),
                                ),
                            },
                        );
                    }
                    ui.end_row();
                }
            });
        ui.add_space(10.0);
        ui.small(tr(
            "Preview buttons work even while sound feedback or an individual event is disabled.",
        ));
    }

    fn draw_advanced(&mut self, ui: &mut egui::Ui) {
        section_heading(
            ui,
            tr("Advanced timing"),
            tr("Increase these only for applications with slow clipboard handling."),
        );
        egui::Grid::new("timing-grid")
            .num_columns(3)
            .spacing([20.0, 12.0])
            .show(ui, |ui| {
                timing_row(
                    ui,
                    tr("Copy delay"),
                    &mut self.config.copy_delay_ms,
                    10,
                    2_000,
                );
                timing_row(
                    ui,
                    tr("Paste delay"),
                    &mut self.config.paste_delay_ms,
                    0,
                    2_000,
                );
                timing_row(
                    ui,
                    tr("Clipboard restore delay"),
                    &mut self.config.restore_delay_ms,
                    0,
                    5_000,
                );
            });
    }

    fn draw_about(&mut self, ui: &mut egui::Ui) {
        self.updates.poll();
        section_heading(
            ui,
            &format!("Upyr {APP_VERSION}"),
            tr("English ↔ Ukrainian keyboard layout correction."),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.updates.is_checking(),
                    egui::Button::new(tr("Check for updates")),
                )
                .clicked()
            {
                self.updates.start();
            }
            if let Some(url) = self.updates.state.release_url() {
                ui.hyperlink_to(tr("Open release"), url);
            }
        });
        ui.label(self.updates.state.message());
        if self.updates.is_checking() {
            ui.ctx().request_repaint_after(ACTIVATION_POLL_INTERVAL);
        }
        ui.add_space(14.0);
        ui.label(egui::RichText::new(tr("Private by construction")).strong());
        ui.label(tr(PRIVACY_SUMMARY));
        ui.add_space(14.0);
        ui.label(egui::RichText::new(tr("How it works")).strong());
        ui.label(tr(IMPLEMENTATION_SUMMARY));
        ui.add_space(14.0);
        ui.label(egui::RichText::new(tr("Open source")).strong());
        ui.label(tr("MIT licensed. Security reports and implementation details are published with the source."));
        ui.horizontal_wrapped(|ui| {
            ui.hyperlink_to(tr("Project website"), PROJECT_WEBSITE_URL);
            ui.separator();
            ui.hyperlink_to(tr("Source repository"), REPOSITORY_URL);
            ui.separator();
            ui.hyperlink_to(
                tr("Report a security issue"),
                format!("{REPOSITORY_URL}/security/advisories/new"),
            );
        });
    }

    fn draw_search_results(&mut self, ui: &mut egui::Ui) {
        let query = self.search.trim().to_lowercase();
        section_heading(
            ui,
            tr("Search results"),
            tr("Choose a setting to open its tab."),
        );
        let mut found = 0;
        for parameter in SEARCH_PARAMETERS {
            if !parameter.matches(&query) {
                continue;
            }
            found += 1;
            if ui
                .add_sized(
                    [ui.available_width(), 38.0],
                    egui::Button::new(format!(
                        "{}  ›  {}",
                        parameter.tab.label(),
                        tr(parameter.label)
                    )),
                )
                .clicked()
            {
                self.tab = parameter.tab;
                self.search.clear();
            }
        }
        if found == 0 {
            ui.label(tr("No settings match this search."));
        }
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        i18n::set_language(self.config.ui_language);
        self.poll_activation(context);
        context.request_repaint_after(ACTIVATION_POLL_INTERVAL);
        if !self.style_applied {
            let mut style = (*context.style()).clone();
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(14.0, 7.0);
            style.spacing.interact_size.y = 28.0;
            context.set_style(style);
            self.style_applied = true;
        }
        self.capture_shortcut(context);

        egui::TopBottomPanel::top("settings-header").show(context, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(tr("Upyr Settings")).size(22.0).strong());
                    ui.small(tr("English ↔ Ukrainian keyboard layout correction"));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_sized(
                        [245.0, 32.0],
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text(tr("Search settings…")),
                    );
                });
            });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                for tab in SettingsTab::ALL {
                    if ui.selectable_label(self.tab == tab, tab.label()).clicked() {
                        self.tab = tab;
                        self.search.clear();
                    }
                }
            });
            ui.add_space(5.0);
        });

        egui::TopBottomPanel::bottom("settings-actions").show(context, |ui| {
            ui.add_space(6.0);
            if let Some((success, message)) = &self.status {
                ui.colored_label(
                    if *success {
                        egui::Color32::from_rgb(45, 145, 75)
                    } else {
                        egui::Color32::from_rgb(190, 55, 55)
                    },
                    i18n::diagnostic(message),
                );
            }
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(tr("Save settings")).strong())
                            .fill(egui::Color32::from_rgb(37, 105, 177)),
                    )
                    .clicked()
                {
                    self.save();
                }
                if ui.button(tr("Reset all")).clicked() {
                    self.reset();
                }
                if ui.button(tr("Close")).clicked() {
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show(context, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(12.0);
                if self.search.trim().is_empty() {
                    match self.tab {
                        SettingsTab::General => self.draw_general(ui),
                        SettingsTab::Automatic => self.draw_automatic(ui),
                        SettingsTab::Exceptions => self.draw_exceptions(ui),
                        SettingsTab::Shortcuts => self.draw_shortcuts(ui),
                        SettingsTab::Feedback => self.draw_feedback(ui),
                        SettingsTab::Advanced => self.draw_advanced(ui),
                        SettingsTab::About => self.draw_about(ui),
                    }
                } else {
                    self.draw_search_results(ui);
                }
                ui.add_space(16.0);
            });
        });
    }
}

fn section_heading(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.label(egui::RichText::new(tr(title)).size(18.0).strong());
    ui.label(egui::RichText::new(tr(description)).weak());
    ui.add_space(2.0);
}

fn timing_row(ui: &mut egui::Ui, label: &str, value: &mut u64, min: u64, max: u64) {
    ui.label(tr(label));
    ui.add(egui::DragValue::new(value).range(min..=max));
    ui.label(tr("ms"));
    ui.end_row();
}

fn hotkey_modifiers(modifiers: egui::Modifiers) -> Modifiers {
    let mut result = Modifiers::empty();
    if modifiers.shift {
        result |= Modifiers::SHIFT;
    }
    if modifiers.ctrl {
        result |= Modifiers::CONTROL;
    }
    if modifiers.alt {
        result |= Modifiers::ALT;
    }
    if modifiers.mac_cmd {
        result |= Modifiers::SUPER;
    }
    result
}

#[allow(clippy::too_many_lines)]
fn egui_key_to_code(key: egui::Key) -> Option<Code> {
    Some(match key {
        egui::Key::ArrowDown => Code::ArrowDown,
        egui::Key::ArrowLeft => Code::ArrowLeft,
        egui::Key::ArrowRight => Code::ArrowRight,
        egui::Key::ArrowUp => Code::ArrowUp,
        egui::Key::Escape => Code::Escape,
        egui::Key::Tab => Code::Tab,
        egui::Key::Backspace => Code::Backspace,
        egui::Key::Enter => Code::Enter,
        egui::Key::Space => Code::Space,
        egui::Key::Insert => Code::Insert,
        egui::Key::Delete => Code::Delete,
        egui::Key::Home => Code::Home,
        egui::Key::End => Code::End,
        egui::Key::PageUp => Code::PageUp,
        egui::Key::PageDown => Code::PageDown,
        egui::Key::Comma => Code::Comma,
        egui::Key::Backslash | egui::Key::Pipe => Code::Backslash,
        egui::Key::Slash | egui::Key::Questionmark => Code::Slash,
        egui::Key::OpenBracket | egui::Key::OpenCurlyBracket => Code::BracketLeft,
        egui::Key::CloseBracket | egui::Key::CloseCurlyBracket => Code::BracketRight,
        egui::Key::Backtick => Code::Backquote,
        egui::Key::Minus => Code::Minus,
        egui::Key::Period => Code::Period,
        egui::Key::Plus | egui::Key::Equals => Code::Equal,
        egui::Key::Colon | egui::Key::Semicolon => Code::Semicolon,
        egui::Key::Quote => Code::Quote,
        egui::Key::Num0 => Code::Digit0,
        egui::Key::Num1 | egui::Key::Exclamationmark => Code::Digit1,
        egui::Key::Num2 => Code::Digit2,
        egui::Key::Num3 => Code::Digit3,
        egui::Key::Num4 => Code::Digit4,
        egui::Key::Num5 => Code::Digit5,
        egui::Key::Num6 => Code::Digit6,
        egui::Key::Num7 => Code::Digit7,
        egui::Key::Num8 => Code::Digit8,
        egui::Key::Num9 => Code::Digit9,
        egui::Key::A => Code::KeyA,
        egui::Key::B => Code::KeyB,
        egui::Key::C => Code::KeyC,
        egui::Key::D => Code::KeyD,
        egui::Key::E => Code::KeyE,
        egui::Key::F => Code::KeyF,
        egui::Key::G => Code::KeyG,
        egui::Key::H => Code::KeyH,
        egui::Key::I => Code::KeyI,
        egui::Key::J => Code::KeyJ,
        egui::Key::K => Code::KeyK,
        egui::Key::L => Code::KeyL,
        egui::Key::M => Code::KeyM,
        egui::Key::N => Code::KeyN,
        egui::Key::O => Code::KeyO,
        egui::Key::P => Code::KeyP,
        egui::Key::Q => Code::KeyQ,
        egui::Key::R => Code::KeyR,
        egui::Key::S => Code::KeyS,
        egui::Key::T => Code::KeyT,
        egui::Key::U => Code::KeyU,
        egui::Key::V => Code::KeyV,
        egui::Key::W => Code::KeyW,
        egui::Key::X => Code::KeyX,
        egui::Key::Y => Code::KeyY,
        egui::Key::Z => Code::KeyZ,
        egui::Key::F1 => Code::F1,
        egui::Key::F2 => Code::F2,
        egui::Key::F3 => Code::F3,
        egui::Key::F4 => Code::F4,
        egui::Key::F5 => Code::F5,
        egui::Key::F6 => Code::F6,
        egui::Key::F7 => Code::F7,
        egui::Key::F8 => Code::F8,
        egui::Key::F9 => Code::F9,
        egui::Key::F10 => Code::F10,
        egui::Key::F11 => Code::F11,
        egui::Key::F12 => Code::F12,
        egui::Key::F13 => Code::F13,
        egui::Key::F14 => Code::F14,
        egui::Key::F15 => Code::F15,
        egui::Key::F16 => Code::F16,
        egui::Key::F17 => Code::F17,
        egui::Key::F18 => Code::F18,
        egui::Key::F19 => Code::F19,
        egui::Key::F20 => Code::F20,
        egui::Key::F21 => Code::F21,
        egui::Key::F22 => Code::F22,
        egui::Key::F23 => Code::F23,
        egui::Key::F24 => Code::F24,
        _ => return None,
    })
}

/// Plain key names remain readable in the bundled egui monospace font.
fn text_hotkey(value: &str) -> Option<String> {
    let hotkey = value.parse::<HotKey>().ok()?;
    let mut parts = Vec::new();
    for (modifier, name) in [
        (Modifiers::CONTROL, "Ctrl"),
        (
            Modifiers::ALT,
            if cfg!(target_os = "macos") {
                "Option"
            } else {
                "Alt"
            },
        ),
        (Modifiers::SHIFT, "Shift"),
        (
            Modifiers::SUPER,
            if cfg!(target_os = "macos") {
                "Cmd"
            } else {
                "Super"
            },
        ),
    ] {
        if hotkey.mods.contains(modifier) {
            parts.push(name.to_owned());
        }
    }
    let key = hotkey.key.to_string();
    let key = key
        .strip_prefix("Key")
        .or_else(|| key.strip_prefix("Digit"))
        .unwrap_or(&key);
    parts.push(key.to_owned());
    Some(parts.join(" + "))
}

pub(crate) fn pretty_hotkey(value: &str) -> Option<String> {
    let hotkey = value.parse::<HotKey>().ok()?;
    let key = pretty_key(hotkey.key);
    #[cfg(target_os = "macos")]
    {
        let mut result = String::new();
        if hotkey.mods.contains(Modifiers::CONTROL) {
            result.push('⌃');
        }
        if hotkey.mods.contains(Modifiers::ALT) {
            result.push('⌥');
        }
        if hotkey.mods.contains(Modifiers::SHIFT) {
            result.push('⇧');
        }
        if hotkey.mods.contains(Modifiers::SUPER) {
            result.push('⌘');
        }
        result.push_str(&key);
        Some(result)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut parts = Vec::new();
        if hotkey.mods.contains(Modifiers::CONTROL) {
            parts.push("Ctrl".to_owned());
        }
        if hotkey.mods.contains(Modifiers::ALT) {
            parts.push("Alt".to_owned());
        }
        if hotkey.mods.contains(Modifiers::SHIFT) {
            parts.push("Shift".to_owned());
        }
        if hotkey.mods.contains(Modifiers::SUPER) {
            parts.push("Super".to_owned());
        }
        parts.push(key);
        Some(parts.join(" + "))
    }
}

fn pretty_key(code: Code) -> String {
    match code {
        Code::Space => "Space".to_owned(),
        Code::Backspace => "⌫".to_owned(),
        Code::Delete => "⌦".to_owned(),
        Code::Enter => "↩".to_owned(),
        Code::Tab => "⇥".to_owned(),
        Code::Escape => "Esc".to_owned(),
        Code::ArrowUp => "↑".to_owned(),
        Code::ArrowDown => "↓".to_owned(),
        Code::ArrowLeft => "←".to_owned(),
        Code::ArrowRight => "→".to_owned(),
        _ => {
            let raw = code.to_string();
            raw.strip_prefix("Key").unwrap_or(&raw).to_owned()
        }
    }
}

fn sensitivity_label(value: AutoCorrectSensitivity) -> &'static str {
    match value {
        AutoCorrectSensitivity::Conservative => tr("Conservative"),
        AutoCorrectSensitivity::Balanced => tr("Balanced"),
        AutoCorrectSensitivity::Aggressive => tr("Aggressive"),
    }
}

fn gesture_label(value: ModifierGesture) -> &'static str {
    match value {
        ModifierGesture::Disabled => tr("Disabled"),
        ModifierGesture::DoubleControl => tr("Double Control"),
        ModifierGesture::DoubleShift => tr("Double Shift"),
        ModifierGesture::DoubleControlShift => tr("Double Control + Shift"),
    }
}

fn gesture_action_label(value: GestureAction) -> &'static str {
    match value {
        GestureAction::PreviousWord => tr("Fix previous word"),
        GestureAction::Selection => tr("Convert selection"),
    }
}

fn direction_label(value: Direction) -> &'static str {
    match value {
        Direction::Smart => tr("Smart (detect script)"),
        Direction::EnglishToUkrainian => tr("English → Ukrainian"),
        Direction::UkrainianToEnglish => tr("Ukrainian → English"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egui_shortcut_labels_do_not_require_macos_symbol_glyphs() {
        let label = text_hotkey("CmdOrCtrl+Alt+Backspace").unwrap();
        assert!(label.is_ascii());
        assert!(label.contains("Backspace"));
        assert!(label.contains(if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        }));
    }

    #[test]
    fn settings_cli_flags_are_explicit_and_mutually_exclusive() {
        use clap::Parser;
        #[derive(Debug, Parser)]
        struct Cli {
            #[command(flatten)]
            options: SettingsOptions,
        }
        assert!(
            Cli::try_parse_from(["settings", "--pixel"])
                .unwrap()
                .options
                .pixel
        );
        assert!(
            Cli::try_parse_from(["settings", "--tui"])
                .unwrap()
                .options
                .tui
        );
        assert!(Cli::try_parse_from(["settings", "--pixel", "--tui"]).is_err());
        assert!(Cli::try_parse_from(["settings", "--pixel", "--about"]).is_err());
        assert_eq!(
            Cli::try_parse_from(["settings", "--help"])
                .unwrap_err()
                .kind(),
            clap::error::ErrorKind::DisplayHelp
        );
    }

    #[test]
    fn exception_parser_trims_and_deduplicates_case_insensitively() {
        assert_eq!(
            parse_exceptions("GitHub, codex\ngithub\n  Upyr "),
            vec!["GitHub", "codex", "Upyr"]
        );
    }

    #[test]
    fn exception_parser_splits_on_whitespace_so_every_entry_validates() {
        let words = parse_exceptions("two words,\tthree\u{a0}more  TWO\r\n");

        assert_eq!(words, vec!["two", "words", "three", "more"]);
        Config {
            auto_correct_exceptions: words,
            ..Config::default()
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn edited_exception_rows_are_split_and_cleaned_before_saving() {
        let words = normalize_exceptions(["GitHub", "  ", " two words ", "github", "Two"]);

        assert_eq!(words, vec!["GitHub", "two", "words"]);
        Config {
            auto_correct_exceptions: words,
            ..Config::default()
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn activation_queue_delivers_the_latest_valid_tab() {
        let unique = ACTIVATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "upyr-settings-activation-test-{}-{unique}",
            std::process::id()
        ));
        let inbox = ActivationInbox::at(directory.clone()).unwrap();

        inbox.send(SettingsTab::General).unwrap();
        inbox.send(SettingsTab::About).unwrap();
        fs::write(
            directory.join("99999999999999999999-invalid.cmd"),
            "invalid",
        )
        .unwrap();

        #[cfg(unix)]
        for entry in fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            if entry.path().extension().and_then(|value| value.to_str()) == Some("cmd")
                && entry.file_name() != "99999999999999999999-invalid.cmd"
            {
                assert_eq!(
                    entry.metadata().unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }

        assert_eq!(inbox.drain().unwrap(), Some(SettingsTab::About));
        assert_eq!(inbox.drain().unwrap(), None);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn settings_tab_activation_commands_round_trip() {
        for tab in [
            SettingsTab::General,
            SettingsTab::Automatic,
            SettingsTab::Exceptions,
            SettingsTab::Shortcuts,
            SettingsTab::Feedback,
            SettingsTab::Advanced,
            SettingsTab::About,
        ] {
            assert_eq!(SettingsTab::from_command(tab.command()), Some(tab));
        }
        assert_eq!(SettingsTab::from_command("not-a-tab"), None);
    }

    #[test]
    fn search_lists_only_controls_drawn_on_this_platform() {
        let labels: Vec<_> = SEARCH_PARAMETERS
            .iter()
            .map(|parameter| parameter.label)
            .collect();

        for label in [
            "Globe key switching",
            "Correct words in terminals",
            "Slash commands in Superset",
            "Undo auto-correction",
            "All country flags",
            "Headphone limit",
        ] {
            assert_eq!(
                labels.contains(&label),
                cfg!(target_os = "macos"),
                "search entry {label:?} must exist only where its control is drawn"
            );
        }
    }

    #[test]
    fn hotkey_preview_uses_readable_platform_notation() {
        #[cfg(target_os = "macos")]
        assert_eq!(pretty_hotkey("Cmd+Alt+Space").as_deref(), Some("⌥⌘Space"));

        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            pretty_hotkey("Ctrl+Alt+Space").as_deref(),
            Some("Ctrl + Alt + Space")
        );
    }

    #[test]
    fn exceptional_autostart_entries_require_explicit_actions() {
        use autostart::AutostartState::{Broken, Disabled, Enabled, Stale};

        assert_eq!(
            autostart_transition(true, Stale),
            AutostartTransition::Enable
        );
        assert_eq!(
            autostart_transition(false, Stale),
            AutostartTransition::None
        );
        assert_eq!(
            autostart_transition(true, Broken),
            AutostartTransition::NeedsExplicitRemoval
        );
        assert_eq!(
            autostart_transition(false, Broken),
            AutostartTransition::None
        );
        assert_eq!(
            autostart_transition(true, Disabled),
            AutostartTransition::Enable
        );
        assert_eq!(
            autostart_transition(false, Enabled),
            AutostartTransition::Disable
        );
    }

    #[test]
    fn detects_a_held_settings_instance_guard() {
        #[cfg(target_os = "windows")]
        let key = format!("dev.Upyr.Upyr.Settings.Test.{}", std::process::id());
        #[cfg(not(target_os = "windows"))]
        let key = std::env::temp_dir()
            .join(format!("upyr-settings-test-{}", std::process::id()))
            .to_string_lossy()
            .into_owned();
        let guard = SingleInstance::new(&key).unwrap();

        assert!(guard.is_single());
        assert!(instance_is_held(&key));
        drop(guard);
        assert!(!instance_is_held(&key));
    }
}
