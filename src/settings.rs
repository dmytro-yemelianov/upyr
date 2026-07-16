use std::{collections::HashSet, env, path::PathBuf, process::Command};

#[cfg(target_os = "macos")]
use std::fs;

use anyhow::{Context, Result, anyhow, bail};
use eframe::egui;
use single_instance::SingleInstance;

use crate::{
    autostart,
    config::{AutoCorrectSensitivity, Config, GestureAction, ModifierGesture},
    layout::Direction,
};

#[cfg(target_os = "macos")]
use crate::config::config_path;

pub fn run() -> Result<()> {
    let instance_key = settings_instance_key()?;
    let instance = SingleInstance::new(&instance_key)
        .context("failed to create the settings single-instance guard")?;
    if !instance.is_single() {
        bail!("Upyr Settings is already open");
    }

    let config = Config::load()?;
    let launch_at_login = autostart::status()?.enabled;
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
        Box::new(move |_context| Ok(Box::new(SettingsApp::new(config, launch_at_login)))),
    )
    .map_err(|error| anyhow!(error.to_string()))
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

pub fn spawn() -> Result<()> {
    #[cfg(target_os = "macos")]
    if let Some(bundle) = packaged_macos_bundle()? {
        let child = Command::new("open")
            .arg(&bundle)
            .spawn()
            .with_context(|| format!("failed to open Upyr Settings at {}", bundle.display()))?;
        if child.id() == 0 {
            bail!("macOS did not start Upyr Settings");
        }
        return Ok(());
    }

    let path = settings_executable()?;
    let child = Command::new(&path)
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

struct SettingsApp {
    config: Config,
    exceptions: String,
    launch_at_login: bool,
    status: Option<(bool, String)>,
    style_applied: bool,
}

impl SettingsApp {
    fn new(config: Config, launch_at_login: bool) -> Self {
        let exceptions = config.auto_correct_exceptions.join("\n");
        Self {
            config,
            exceptions,
            launch_at_login,
            status: None,
            style_applied: false,
        }
    }

    fn save(&mut self) {
        self.config.auto_correct_exceptions = parse_exceptions(&self.exceptions);
        self.exceptions = self.config.auto_correct_exceptions.join("\n");
        let result = self.config.write(true).and_then(|_| {
            let enabled = autostart::status()?.enabled;
            if enabled != self.launch_at_login {
                if self.launch_at_login {
                    autostart::enable()?;
                } else {
                    autostart::disable()?;
                }
            }
            Ok(())
        });
        self.status = Some(match result {
            Ok(()) => (
                true,
                "Saved. The running background app will reload these settings.".to_owned(),
            ),
            Err(error) => (false, format!("Could not save: {error:#}")),
        });
    }

    fn reset(&mut self) {
        self.config = Config::default();
        self.exceptions.clear();
        self.status = Some((
            true,
            "Defaults restored in the form. Choose Save to apply them.".to_owned(),
        ));
    }
}

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

        egui::CentralPanel::default().show(context, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(37, 83, 145))
                .corner_radius(10)
                .inner_margin(16)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Upyr Settings")
                            .size(24.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Local English ↔ Ukrainian keyboard layout correction",
                        )
                        .color(egui::Color32::from_rgb(220, 232, 248)),
                    );
                });
            ui.add_space(10.0);

            egui::ScrollArea::vertical().show(ui, |ui| {
                section_heading(
                    ui,
                    "Automatic correction",
                    "Fix high-confidence wrong-layout words immediately after Space.",
                );
                ui.checkbox(
                    &mut self.config.auto_correct,
                    "Correct confidently recognized words after Space",
                );
                ui.small(
                    "Opt-in: keys are kept only in memory while a word is being typed and are never logged.",
                );
                #[cfg(target_os = "macos")]
                ui.colored_label(
                    egui::Color32::from_rgb(225, 166, 55),
                    "macOS requires Accessibility access for Upyr. After granting it, save again or restart Upyr.",
                );
                ui.add_enabled_ui(self.config.auto_correct, |ui| {
                    egui::Grid::new("automatic-correction-grid")
                        .num_columns(2)
                        .spacing([16.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Sensitivity");
                            egui::ComboBox::from_id_salt("auto-correct-sensitivity")
                                .selected_text(sensitivity_label(
                                    self.config.auto_correct_sensitivity,
                                ))
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
                                egui::DragValue::new(
                                    &mut self.config.auto_correct_min_word_length,
                                )
                                .range(2..=32),
                            );
                            ui.end_row();

                            ui.label("Delay after Space");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::DragValue::new(
                                        &mut self.config.auto_correct_delay_ms,
                                    )
                                    .range(10..=250),
                                );
                                ui.label("ms");
                            });
                            ui.end_row();
                        });
                    ui.label("Never auto-correct these words (one per line or comma-separated)");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.exceptions)
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    );
                });

                ui.add_space(12.0);
                section_heading(
                    ui,
                    "Shortcuts and gesture",
                    "Manual correction stays available whether automation is enabled or not.",
                );
                egui::Grid::new("shortcuts-grid")
                    .num_columns(2)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Convert selection");
                        ui.add_sized(
                            [300.0, 28.0],
                            egui::TextEdit::singleline(&mut self.config.hotkey),
                        );
                        ui.end_row();

                        ui.label("Fix previous word");
                        ui.add_sized(
                            [300.0, 28.0],
                            egui::TextEdit::singleline(&mut self.config.last_word_hotkey),
                        );
                        ui.end_row();

                        ui.label("Modifier gesture");
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

                        ui.label("Gesture action");
                        egui::ComboBox::from_id_salt("gesture-action")
                            .selected_text(gesture_action_label(
                                self.config.modifier_gesture_action,
                            ))
                            .show_ui(ui, |ui| {
                                for value in [GestureAction::PreviousWord, GestureAction::Selection]
                                {
                                    ui.selectable_value(
                                        &mut self.config.modifier_gesture_action,
                                        value,
                                        gesture_action_label(value),
                                    );
                                }
                            });
                        ui.end_row();

                        ui.label("Gesture timeout");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(
                                    &mut self.config.modifier_gesture_timeout_ms,
                                )
                                .range(150..=2_000),
                            );
                            ui.label("ms");
                        });
                        ui.end_row();
                    });

                ui.add_space(12.0);
                section_heading(
                    ui,
                    "Behavior",
                    "Choose how converted text, the active layout, and clipboard are handled.",
                );
                egui::Grid::new("behavior-grid")
                    .num_columns(2)
                    .spacing([16.0, 8.0])
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
                ui.checkbox(
                    &mut self.config.switch_layout,
                    "Switch the OS layout to match converted text",
                );
                ui.checkbox(
                    &mut self.config.restore_clipboard,
                    "Restore clipboard contents after conversion",
                );
                ui.checkbox(&mut self.launch_at_login, "Launch Upyr at login");

                ui.collapsing("Advanced timing", |ui| {
                    egui::Grid::new("timing-grid")
                        .num_columns(3)
                        .spacing([16.0, 8.0])
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
                });

                ui.add_space(12.0);
                if let Some((success, message)) = &self.status {
                    ui.colored_label(
                        if *success {
                            egui::Color32::from_rgb(45, 145, 75)
                        } else {
                            egui::Color32::from_rgb(190, 55, 55)
                        },
                        message,
                    );
                    ui.add_space(6.0);
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
                    if ui.button("Reset to defaults").clicked() {
                        self.reset();
                    }
                    if ui.button("Close").clicked() {
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.add_space(8.0);
            });
        });
    }
}

fn section_heading(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.label(egui::RichText::new(title).size(18.0).strong());
    ui.label(egui::RichText::new(description).weak());
    ui.add_space(2.0);
}

fn timing_row(ui: &mut egui::Ui, label: &str, value: &mut u64, min: u64, max: u64) {
    ui.label(label);
    ui.add(egui::DragValue::new(value).range(min..=max));
    ui.label("ms");
    ui.end_row();
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

fn gesture_label(value: ModifierGesture) -> &'static str {
    match value {
        ModifierGesture::Disabled => "Disabled",
        ModifierGesture::DoubleControl => "Double Control",
        ModifierGesture::DoubleShift => "Double Shift",
        ModifierGesture::DoubleControlShift => "Double Control + Shift",
    }
}

fn gesture_action_label(value: GestureAction) -> &'static str {
    match value {
        GestureAction::PreviousWord => "Fix previous word",
        GestureAction::Selection => "Convert selection",
    }
}

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
}
