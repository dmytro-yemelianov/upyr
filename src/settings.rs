use std::{collections::HashSet, env, path::PathBuf, process::Command};

#[cfg(target_os = "macos")]
use std::fs;

#[cfg(not(target_os = "macos"))]
use anyhow::anyhow;
use anyhow::{Context, Result, bail};
#[cfg(not(target_os = "macos"))]
use eframe::egui;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use single_instance::SingleInstance;

use crate::{
    autostart,
    config::{AutoCorrectSensitivity, Config},
};
#[cfg(not(target_os = "macos"))]
use crate::{
    config::{GestureAction, ModifierGesture, SoundEvent},
    layout::Direction,
};

#[cfg(target_os = "macos")]
use crate::config::config_path;

#[cfg(target_os = "macos")]
mod macos;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROJECT_WEBSITE_URL: &str = "https://dmytro-yemelianov.github.io/upyr/";
const REPOSITORY_URL: &str = "https://github.com/dmytro-yemelianov/upyr";
const PRIVACY_SUMMARY: &str = "Local-only by design: no accounts, analytics, telemetry, ads, or text uploads. Typed text and clipboard contents stay on this device.";
const IMPLEMENTATION_SUMMARY: &str = "Upyr is written in Rust. It maps physical English and Ukrainian keys, then uses a bundled compact character n-gram model to score language candidates locally; no cloud inference is involved.";

pub fn run() -> Result<()> {
    let instance_key = settings_instance_key()?;
    let instance = SingleInstance::new(&instance_key)
        .context("failed to create the settings single-instance guard")?;
    if !instance.is_single() {
        bail!("Upyr Settings is already open");
    }

    let config = Config::load()?;
    let autostart_status = autostart::status()?;
    let initial_tab = if env::args_os().any(|argument| argument == "--about") {
        SettingsTab::About
    } else {
        SettingsTab::General
    };
    #[cfg(target_os = "macos")]
    return macos::run(config, autostart_status, initial_tab);

    #[cfg(not(target_os = "macos"))]
    {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Upyr Settings")
                .with_inner_size([680.0, 800.0])
                .with_min_inner_size([520.0, 560.0]),
            ..Default::default()
        };
        eframe::run_native(
            "Upyr Settings",
            options,
            Box::new(move |_context| {
                Ok(Box::new(SettingsApp::new(
                    config,
                    autostart_status,
                    initial_tab,
                )))
            }),
        )
        .map_err(|error| anyhow!(error.to_string()))
    }
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
        autostart::AutostartState::Stale => {
            "Launch at login points to another Upyr installation. Choose Repair Entry to update it, or Remove Entry to disable it."
        }
        autostart::AutostartState::Broken => {
            "The launch-at-login entry needs attention and Upyr will not overwrite it. Choose Remove Entry explicitly before enabling it again."
        }
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
    spawn_tab(None)
}

pub fn spawn_about() -> Result<()> {
    spawn_tab(Some("--about"))
}

fn spawn_tab(argument: Option<&str>) -> Result<()> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Automatic,
    Shortcuts,
    Feedback,
    Advanced,
    About,
}

impl SettingsTab {
    #[cfg(not(target_os = "macos"))]
    const ALL: [Self; 6] = [
        Self::General,
        Self::Automatic,
        Self::Shortcuts,
        Self::Feedback,
        Self::Advanced,
        Self::About,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Automatic => "Automatic",
            Self::Shortcuts => "Shortcuts",
            Self::Feedback => "Feedback",
            Self::Advanced => "Advanced",
            Self::About => "About",
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutSlot {
    Selection,
    PreviousWord,
}

struct SearchParameter {
    tab: SettingsTab,
    label: &'static str,
    terms: &'static str,
}

const SEARCH_PARAMETERS: &[SearchParameter] = &[
    SearchParameter {
        tab: SettingsTab::General,
        label: "Conversion direction",
        terms: "smart english ukrainian language mapping",
    },
    SearchParameter {
        tab: SettingsTab::General,
        label: "Switch OS layout",
        terms: "input source follow converted text",
    },
    SearchParameter {
        tab: SettingsTab::General,
        label: "Restore clipboard",
        terms: "pasteboard copy contents",
    },
    SearchParameter {
        tab: SettingsTab::General,
        label: "Launch at login",
        terms: "autostart startup boot",
    },
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "Automatic correction",
        terms: "auto fix after space detection",
    },
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "Sensitivity",
        terms: "confidence conservative balanced aggressive",
    },
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "Minimum word length",
        terms: "characters threshold",
    },
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "Correction delay",
        terms: "space milliseconds timing",
    },
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "Never correct list",
        terms: "exceptions dictionary words ignore exclude",
    },
    SearchParameter {
        tab: SettingsTab::Shortcuts,
        label: "Convert selection shortcut",
        terms: "hotkey record key combination",
    },
    SearchParameter {
        tab: SettingsTab::Shortcuts,
        label: "Fix previous word shortcut",
        terms: "hotkey record backspace key combination",
    },
    SearchParameter {
        tab: SettingsTab::Shortcuts,
        label: "Modifier gesture",
        terms: "double control shift trigger",
    },
    SearchParameter {
        tab: SettingsTab::Shortcuts,
        label: "Gesture action",
        terms: "selection previous word",
    },
    SearchParameter {
        tab: SettingsTab::Shortcuts,
        label: "Gesture timeout",
        terms: "double press milliseconds",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Language indicator",
        terms: "flag floating cursor pointer overlay english ukrainian",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Indicator duration",
        terms: "flag overlay milliseconds timing",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Sound feedback",
        terms: "audio effects enable disable",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Sound volume",
        terms: "audio loud quiet percent preview",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Event sounds",
        terms: "automatic correction manual conversion layout switch pause resume error preview",
    },
    SearchParameter {
        tab: SettingsTab::Advanced,
        label: "Copy delay",
        terms: "clipboard milliseconds timing",
    },
    SearchParameter {
        tab: SettingsTab::Advanced,
        label: "Paste delay",
        terms: "clipboard milliseconds timing",
    },
    SearchParameter {
        tab: SettingsTab::Advanced,
        label: "Clipboard restore delay",
        terms: "pasteboard milliseconds timing",
    },
    SearchParameter {
        tab: SettingsTab::About,
        label: "Version and license",
        terms: "about release semver copyright mit",
    },
    SearchParameter {
        tab: SettingsTab::About,
        label: "Privacy and implementation",
        terms: "local only no tracking telemetry analytics n-gram model rust security source",
    },
];

#[cfg(not(target_os = "macos"))]
struct SettingsApp {
    config: Config,
    exceptions: String,
    launch_at_login: bool,
    autostart_status: autostart::AutostartStatus,
    status: Option<(bool, String)>,
    style_applied: bool,
    tab: SettingsTab,
    search: String,
    recording: Option<ShortcutSlot>,
    shortcut_error: Option<String>,
}

#[cfg(not(target_os = "macos"))]
impl SettingsApp {
    fn new(
        config: Config,
        autostart_status: autostart::AutostartStatus,
        initial_tab: SettingsTab,
    ) -> Self {
        let exceptions = config.auto_correct_exceptions.join("\n");
        let launch_at_login = autostart_status.enabled;
        Self {
            config,
            exceptions,
            launch_at_login,
            autostart_status,
            status: None,
            style_applied: false,
            tab: initial_tab,
            search: String::new(),
            recording: None,
            shortcut_error: None,
        }
    }

    fn save(&mut self) {
        self.config.auto_correct_exceptions = parse_exceptions(&self.exceptions);
        self.exceptions = self.config.auto_correct_exceptions.join("\n");
        if let Err(error) = self.config.write(true) {
            self.status = Some((false, format!("Could not save settings: {error:#}")));
            return;
        }

        match sync_launch_at_login(self.launch_at_login) {
            Ok(status) => {
                self.launch_at_login = status.enabled;
                self.autostart_status = status;
                self.status = Some(match autostart_attention(&self.autostart_status) {
                    Some(attention) => (false, format!("Settings saved. {attention}")),
                    None => (
                        true,
                        "Saved. The running background app will reload these settings.".to_owned(),
                    ),
                });
            }
            Err(error) => {
                if let Ok(status) = autostart::status() {
                    self.launch_at_login = status.enabled;
                    self.autostart_status = status;
                }
                self.status = Some((
                    false,
                    format!("Settings saved, but launch at login was not changed: {error:#}"),
                ));
            }
        }
    }

    fn repair_autostart(&mut self) {
        match autostart::enable() {
            Ok(status) => {
                self.launch_at_login = status.enabled;
                self.autostart_status = status;
                self.status = Some((true, "Launch-at-login entry repaired.".to_owned()));
            }
            Err(error) => {
                self.status = Some((false, format!("Could not repair entry: {error:#}")));
            }
        }
    }

    fn remove_autostart(&mut self) {
        match autostart::disable() {
            Ok(status) => {
                self.launch_at_login = status.enabled;
                self.autostart_status = status;
                self.status = Some((true, "Launch-at-login entry removed.".to_owned()));
            }
            Err(error) => {
                self.status = Some((false, format!("Could not remove entry: {error:#}")));
            }
        }
    }

    fn reset(&mut self) {
        self.config = Config::default();
        self.exceptions.clear();
        self.recording = None;
        self.shortcut_error = None;
        self.status = Some((
            true,
            "Defaults restored in the form. Choose Save to apply them.".to_owned(),
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
            self.shortcut_error = Some("That key cannot be used in a global shortcut.".to_owned());
            return;
        };
        let modifiers = hotkey_modifiers(modifiers);
        if modifiers.is_empty() {
            self.shortcut_error = Some(
                "Include Command, Control, Option/Alt, or Shift so normal typing is never captured."
                    .to_owned(),
            );
            return;
        }

        let value = HotKey::new(Some(modifiers), code).to_string();
        let other = match slot {
            ShortcutSlot::Selection => &self.config.last_word_hotkey,
            ShortcutSlot::PreviousWord => &self.config.hotkey,
        };
        if shortcuts_equal(&value, other) {
            self.shortcut_error =
                Some("That shortcut is already assigned to the other action.".to_owned());
            return;
        }
        match slot {
            ShortcutSlot::Selection => self.config.hotkey = value,
            ShortcutSlot::PreviousWord => self.config.last_word_hotkey = value,
        }
        self.recording = None;
        self.shortcut_error = None;
    }

    fn draw_general(&mut self, ui: &mut egui::Ui) {
        section_heading(
            ui,
            "General",
            "Core conversion, clipboard, and startup behavior.",
        );
        egui::Grid::new("general-grid")
            .num_columns(2)
            .spacing([20.0, 12.0])
            .show(ui, |ui| {
                ui.label("Conversion direction");
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
            "Switch the OS layout to match converted text",
        );
        ui.checkbox(
            &mut self.config.restore_clipboard,
            "Restore clipboard contents after conversion",
        );
        ui.checkbox(&mut self.launch_at_login, "Launch Upyr at login");
        if let Some(attention) = autostart_attention(&self.autostart_status) {
            ui.colored_label(egui::Color32::from_rgb(180, 105, 20), attention);
            ui.horizontal(|ui| {
                if self.autostart_status.state == autostart::AutostartState::Stale
                    && ui.button("Repair Entry").clicked()
                {
                    self.repair_autostart();
                }
                if ui.button("Remove Entry").clicked() {
                    self.remove_autostart();
                }
            });
        }
    }

    fn draw_automatic(&mut self, ui: &mut egui::Ui) {
        section_heading(
            ui,
            "Automatic correction",
            "Fix high-confidence wrong-layout text immediately after Space.",
        );
        ui.checkbox(
            &mut self.config.auto_correct,
            "Correct confidently recognized text after Space",
        );
        ui.small(
            "Opt-in: a short input prefix is kept only in memory, cleared at safe boundaries, and never logged.",
        );
        #[cfg(target_os = "macos")]
        ui.colored_label(
            egui::Color32::from_rgb(190, 125, 35),
            "macOS requires Accessibility access. Upyr checks existing access first and offers one restart after permission is granted.",
        );
        ui.add_space(8.0);
        ui.add_enabled_ui(self.config.auto_correct, |ui| {
            egui::Grid::new("automatic-correction-grid")
                .num_columns(2)
                .spacing([20.0, 12.0])
                .show(ui, |ui| {
                    ui.label("Sensitivity");
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
                    ui.label("Minimum word length");
                    ui.add(
                        egui::DragValue::new(&mut self.config.auto_correct_min_word_length)
                            .range(2..=32),
                    );
                    ui.end_row();
                    ui.label("Delay after Space");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.config.auto_correct_delay_ms)
                                .range(10..=250),
                        );
                        ui.label("ms");
                    });
                    ui.end_row();
                });
            ui.add_space(8.0);
            ui.label("Never auto-correct these words");
            ui.small("One word per line, or separate words with commas.");
            ui.add(
                egui::TextEdit::multiline(&mut self.exceptions)
                    .desired_rows(8)
                    .desired_width(f32::INFINITY),
            );
        });
    }

    fn draw_shortcuts(&mut self, ui: &mut egui::Ui) {
        section_heading(
            ui,
            "Shortcuts",
            "Click a recorder, then press the combination you want.",
        );
        self.shortcut_row(ui, "Convert selection", ShortcutSlot::Selection);
        ui.add_space(8.0);
        self.shortcut_row(ui, "Fix previous word", ShortcutSlot::PreviousWord);
        if let Some(error) = &self.shortcut_error {
            ui.add_space(6.0);
            ui.colored_label(egui::Color32::from_rgb(190, 55, 55), error);
        }
        ui.add_space(18.0);
        section_heading(
            ui,
            "Modifier gesture",
            "An optional double-tap trigger without a regular key.",
        );
        egui::Grid::new("gesture-grid")
            .num_columns(2)
            .spacing([20.0, 12.0])
            .show(ui, |ui| {
                ui.label("Gesture");
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
                ui.label("Action");
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
                ui.label("Double-tap timeout");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.config.modifier_gesture_timeout_ms)
                            .range(150..=2_000),
                    );
                    ui.label("ms");
                });
                ui.end_row();
            });
    }

    fn shortcut_row(&mut self, ui: &mut egui::Ui, label: &str, slot: ShortcutSlot) {
        let active = self.recording == Some(slot);
        let value = match slot {
            ShortcutSlot::Selection => self.config.hotkey.clone(),
            ShortcutSlot::PreviousWord => self.config.last_word_hotkey.clone(),
        };
        ui.label(egui::RichText::new(label).strong());
        ui.horizontal(|ui| {
            let text = if active {
                "Press shortcut…  (Esc to cancel)".to_owned()
            } else {
                pretty_hotkey(&value).unwrap_or_else(|| value.clone())
            };
            let response = ui.add_sized(
                [310.0, 38.0],
                egui::Button::new(egui::RichText::new(text).monospace()).selected(active),
            );
            if response.clicked() {
                self.recording = Some(slot);
                self.shortcut_error = None;
            }
            let default = Config::default();
            let default_value = match slot {
                ShortcutSlot::Selection => default.hotkey,
                ShortcutSlot::PreviousWord => default.last_word_hotkey,
            };
            if ui
                .add_enabled(value != default_value, egui::Button::new("Reset"))
                .clicked()
            {
                match slot {
                    ShortcutSlot::Selection => self.config.hotkey = default_value,
                    ShortcutSlot::PreviousWord => self.config.last_word_hotkey = default_value,
                }
                self.recording = None;
                self.shortcut_error = None;
            }
        });
        ui.small("The shortcut uses physical keys, so it keeps working in either keyboard layout.");
    }

    fn draw_feedback(&mut self, ui: &mut egui::Ui) {
        section_heading(
            ui,
            "Visual feedback",
            "Optional confirmation after Upyr changes the active input source.",
        );
        ui.checkbox(
            &mut self.config.show_layout_indicator,
            "Show a temporary language flag next to the pointer",
        );
        ui.add_enabled_ui(self.config.show_layout_indicator, |ui| {
            ui.horizontal(|ui| {
                ui.label("Visible for");
                ui.add(
                    egui::DragValue::new(&mut self.config.layout_indicator_duration_ms)
                        .range(250..=3_000),
                );
                ui.label("ms");
            });
        });
        ui.add_space(18.0);
        section_heading(
            ui,
            "Sound feedback",
            "Choose which Upyr actions play a subtle confirmation sound.",
        );
        ui.checkbox(&mut self.config.sounds.enabled, "Enable sound feedback");
        ui.horizontal(|ui| {
            ui.label("Volume");
            ui.add(egui::Slider::new(&mut self.config.sounds.volume_percent, 0..=100).suffix("%"));
        });
        ui.add_space(8.0);
        egui::Grid::new("sound-feedback-grid")
            .num_columns(2)
            .spacing([20.0, 8.0])
            .show(ui, |ui| {
                for event in SoundEvent::ALL {
                    let mut selected = self.config.sounds.event_selected(event);
                    if ui.checkbox(&mut selected, event.label()).changed() {
                        self.config.sounds.set_event_selected(event, selected);
                    }
                    if ui.button("Preview").clicked() {
                        let mut preview_settings = self.config.sounds;
                        preview_settings.enabled = true;
                        preview_settings.set_event_selected(event, true);
                        self.status = Some(
                            match crate::feedback::preview_sound(event, &preview_settings) {
                                Ok(()) => (true, format!("Previewed {} sound.", event.label())),
                                Err(error) => (
                                    false,
                                    format!("Could not preview {} sound: {error:#}", event.label()),
                                ),
                            },
                        );
                    }
                    ui.end_row();
                }
            });
        ui.add_space(10.0);
        ui.small(
            "Preview buttons work even while sound feedback or an individual event is disabled.",
        );
    }

    fn draw_advanced(&mut self, ui: &mut egui::Ui) {
        section_heading(
            ui,
            "Advanced timing",
            "Increase these only for applications with slow clipboard handling.",
        );
        egui::Grid::new("timing-grid")
            .num_columns(3)
            .spacing([20.0, 12.0])
            .show(ui, |ui| {
                timing_row(ui, "Copy delay", &mut self.config.copy_delay_ms, 10, 2_000);
                timing_row(ui, "Paste delay", &mut self.config.paste_delay_ms, 0, 2_000);
                timing_row(
                    ui,
                    "Clipboard restore delay",
                    &mut self.config.restore_delay_ms,
                    0,
                    5_000,
                );
            });
    }

    fn draw_about(&mut self, ui: &mut egui::Ui) {
        section_heading(
            ui,
            &format!("Upyr {APP_VERSION}"),
            "English ↔ Ukrainian keyboard layout correction.",
        );
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Private by construction").strong());
        ui.label(PRIVACY_SUMMARY);
        ui.add_space(14.0);
        ui.label(egui::RichText::new("How it works").strong());
        ui.label(IMPLEMENTATION_SUMMARY);
        ui.add_space(14.0);
        ui.label(egui::RichText::new("Open source").strong());
        ui.label("MIT licensed. Security reports and implementation details are published with the source.");
        ui.horizontal_wrapped(|ui| {
            ui.hyperlink_to("Project website", PROJECT_WEBSITE_URL);
            ui.separator();
            ui.hyperlink_to("Source repository", REPOSITORY_URL);
            ui.separator();
            ui.hyperlink_to(
                "Report a security issue",
                format!("{REPOSITORY_URL}/security/advisories/new"),
            );
        });
    }

    fn draw_search_results(&mut self, ui: &mut egui::Ui) {
        let query = self.search.trim().to_lowercase();
        section_heading(ui, "Search results", "Choose a setting to open its tab.");
        let mut found = 0;
        for parameter in SEARCH_PARAMETERS {
            let haystack = format!("{} {}", parameter.label, parameter.terms).to_lowercase();
            if !haystack.contains(&query) {
                continue;
            }
            found += 1;
            if ui
                .add_sized(
                    [ui.available_width(), 38.0],
                    egui::Button::new(format!("{}  ›  {}", parameter.tab.label(), parameter.label)),
                )
                .clicked()
            {
                self.tab = parameter.tab;
                self.search.clear();
            }
        }
        if found == 0 {
            ui.label("No settings match this search.");
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl eframe::App for SettingsApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
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
                    ui.label(egui::RichText::new("Upyr Settings").size(22.0).strong());
                    ui.small("English ↔ Ukrainian keyboard layout correction");
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_sized(
                        [245.0, 32.0],
                        egui::TextEdit::singleline(&mut self.search).hint_text("Search settings…"),
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
                    message,
                );
            }
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("Save settings").strong())
                            .fill(egui::Color32::from_rgb(37, 105, 177)),
                    )
                    .clicked()
                {
                    self.save();
                }
                if ui.button("Reset all").clicked() {
                    self.reset();
                }
                if ui.button("Close").clicked() {
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

#[cfg(not(target_os = "macos"))]
fn section_heading(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.label(egui::RichText::new(title).size(18.0).strong());
    ui.label(egui::RichText::new(description).weak());
    ui.add_space(2.0);
}

#[cfg(not(target_os = "macos"))]
fn timing_row(ui: &mut egui::Ui, label: &str, value: &mut u64, min: u64, max: u64) {
    ui.label(label);
    ui.add(egui::DragValue::new(value).range(min..=max));
    ui.label("ms");
    ui.end_row();
}

#[cfg(not(target_os = "macos"))]
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

#[cfg(not(target_os = "macos"))]
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

#[cfg(not(target_os = "macos"))]
fn shortcuts_equal(left: &str, right: &str) -> bool {
    match (left.parse::<HotKey>(), right.parse::<HotKey>()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left.eq_ignore_ascii_case(right),
    }
}

fn pretty_hotkey(value: &str) -> Option<String> {
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

fn parse_exceptions(source: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    source
        .split([',', '\n', '\r'])
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .filter(|word| seen.insert(word.to_lowercase()))
        .map(str::to_owned)
        .collect()
}

fn sensitivity_label(value: AutoCorrectSensitivity) -> &'static str {
    match value {
        AutoCorrectSensitivity::Conservative => "Conservative",
        AutoCorrectSensitivity::Balanced => "Balanced",
        AutoCorrectSensitivity::Aggressive => "Aggressive",
    }
}

#[cfg(not(target_os = "macos"))]
fn gesture_label(value: ModifierGesture) -> &'static str {
    match value {
        ModifierGesture::Disabled => "Disabled",
        ModifierGesture::DoubleControl => "Double Control",
        ModifierGesture::DoubleShift => "Double Shift",
        ModifierGesture::DoubleControlShift => "Double Control + Shift",
    }
}

#[cfg(not(target_os = "macos"))]
fn gesture_action_label(value: GestureAction) -> &'static str {
    match value {
        GestureAction::PreviousWord => "Fix previous word",
        GestureAction::Selection => "Convert selection",
    }
}

#[cfg(not(target_os = "macos"))]
fn direction_label(value: Direction) -> &'static str {
    match value {
        Direction::Smart => "Smart (detect script)",
        Direction::EnglishToUkrainian => "English → Ukrainian",
        Direction::UkrainianToEnglish => "Ukrainian → English",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exception_parser_trims_and_deduplicates_case_insensitively() {
        assert_eq!(
            parse_exceptions("GitHub, codex\ngithub\n  Upyr "),
            vec!["GitHub", "codex", "Upyr"]
        );
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
