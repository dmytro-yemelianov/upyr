//! Optional retro form renderer. Semantic egui widgets retain Unicode editing.
use super::{
    draft::{SettingsAction, SettingsDraft},
    navigation::{CONTEXT_HELP, CORRECTION_FOOTNOTE, MIDWORD_HELP},
    persistence::{self, LocalStore, SettingsStore},
    sensitivity_label,
};
use crate::config::{AutoCorrectSensitivity, Config};
use anyhow::{Result, anyhow};
use eframe::egui::{self, Color32, RichText};

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Correction,
    Exceptions,
}
#[derive(Clone, Copy)]
enum Discard {
    Close,
    Reload,
}

pub(super) fn run() -> Result<()> {
    let config = Config::load()?;
    eframe::run_native(
        "Upyr · Pixel Settings",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([780.0, 680.0])
                .with_min_inner_size([480.0, 400.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut style = (*cc.egui_ctx.style()).clone();
            style.visuals = egui::Visuals::dark();
            style.visuals.panel_fill = Color32::from_rgb(17, 24, 35);
            style.visuals.selection.bg_fill = Color32::from_rgb(35, 79, 102);
            style.visuals.selection.stroke.color = Color32::from_rgb(142, 224, 184);
            style.visuals.window_corner_radius = egui::CornerRadius::ZERO;
            for widget in [
                &mut style.visuals.widgets.inactive,
                &mut style.visuals.widgets.hovered,
                &mut style.visuals.widgets.active,
                &mut style.visuals.widgets.noninteractive,
                &mut style.visuals.widgets.open,
            ] {
                widget.corner_radius = egui::CornerRadius::ZERO;
            }
            style.override_font_id = Some(egui::FontId::monospace(15.0));
            style.spacing.item_spacing = egui::vec2(12.0, 10.0);
            style.spacing.button_padding = egui::vec2(14.0, 8.0);
            cc.egui_ctx.set_style(style);
            cc.egui_ctx.enable_accesskit();
            Ok(Box::new(PixelSettings {
                draft: SettingsDraft::new(config),
                section: Section::Correction,
                new_exception: String::new(),
                status: String::new(),
                confirm: None,
                allow_close: false,
            }))
        }),
    )
    .map_err(|error| anyhow!("Could not open pixel settings: {error}"))
}

struct PixelSettings {
    draft: SettingsDraft,
    section: Section,
    new_exception: String,
    status: String,
    confirm: Option<Discard>,
    allow_close: bool,
}
impl PixelSettings {
    fn dirty(&self) -> bool {
        self.draft.dirty() || !self.new_exception.trim().is_empty()
    }
    fn stage_new_exceptions(&mut self) {
        let text = std::mem::take(&mut self.new_exception);
        self.apply(SettingsAction::AddExceptions(text));
    }
    fn revert(&mut self) {
        self.new_exception.clear();
        self.apply(SettingsAction::Revert);
    }
    fn apply(&mut self, action: SettingsAction) {
        self.status = match self.draft.apply(action) {
            Ok(()) => String::new(),
            Err(e) => format!("{e:#}"),
        };
    }
    fn save(&mut self, store: &impl SettingsStore) {
        self.stage_new_exceptions();
        self.status = match persistence::save(&mut self.draft, store) {
            Ok(()) => "Saved. Upyr will reload these settings.".into(),
            Err(e) => format!("Save failed: {e:#}"),
        };
    }
    fn reload(&mut self, store: &impl SettingsStore) {
        self.status = match persistence::reload(&mut self.draft, store) {
            Ok(()) => {
                self.new_exception.clear();
                "Reloaded from disk.".into()
            }
            Err(e) => format!("Reload failed: {e:#}"),
        };
    }
    fn correction(&mut self, ui: &mut egui::Ui) {
        ui.heading("CORRECTION");
        let mut automatic = self.draft.edited.auto_correct;
        if ui
            .checkbox(&mut automatic, "Correct the wrong layout automatically")
            .changed()
        {
            self.apply(SettingsAction::Automatic(automatic));
        }
        let mut follow = self.draft.edited.switch_layout;
        if ui
            .checkbox(&mut follow, "Switch OS layout to match corrected text")
            .changed()
        {
            self.apply(SettingsAction::SwitchLayout(follow));
        }
        ui.separator();
        ui.label(MIDWORD_HELP);
        ui.label(CONTEXT_HELP);
        ui.separator();
        ui.label("Sensitivity");
        let mut sensitivity = self.draft.edited.auto_correct_sensitivity;
        egui::ComboBox::from_id_salt("sensitivity")
            .selected_text(sensitivity_label(sensitivity))
            .show_ui(ui, |ui| {
                for value in [
                    AutoCorrectSensitivity::Conservative,
                    AutoCorrectSensitivity::Balanced,
                    AutoCorrectSensitivity::Aggressive,
                ] {
                    ui.selectable_value(&mut sensitivity, value, sensitivity_label(value));
                }
            });
        if sensitivity != self.draft.edited.auto_correct_sensitivity {
            self.apply(SettingsAction::Sensitivity(sensitivity));
        }
        let minimum_label = ui.label("Minimum word length (2–32)");
        let mut minimum = self.draft.minimum.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut minimum)
                    .id(egui::Id::new("pixel-minimum"))
                    .desired_width(100.0),
            )
            .labelled_by(minimum_label.id)
            .changed()
        {
            self.apply(SettingsAction::Minimum(minimum));
        }
        let delay_label = ui.label("Pause after Space (10–250 ms)");
        let mut delay = self.draft.delay.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut delay)
                    .id(egui::Id::new("pixel-delay"))
                    .desired_width(100.0),
            )
            .labelled_by(delay_label.id)
            .changed()
        {
            self.apply(SettingsAction::Delay(delay));
        }
        ui.small("Clear prefixes can switch immediately while typing.");
        ui.separator();
        ui.label(CORRECTION_FOOTNOTE);
    }
    fn exceptions(&mut self, ui: &mut egui::Ui) {
        ui.heading("EXCEPTIONS");
        ui.label("Words Upyr should leave alone. Spaces and commas separate words.");
        let add_label = ui.label("Add words");
        ui.add(
            egui::TextEdit::singleline(&mut self.new_exception)
                .id(egui::Id::new("pixel-new-exception"))
                .hint_text("Ґанок, Їжак, GitHub")
                .desired_width(f32::INFINITY),
        )
        .labelled_by(add_label.id);
        if ui.button("Add words").clicked() {
            self.stage_new_exceptions();
        }
        ui.separator();
        if self.draft.edited.auto_correct_exceptions.is_empty() {
            ui.label("No exceptions yet.");
        }
        let mut action = None;
        for (i, row) in self.draft.edited.auto_correct_exceptions.iter().enumerate() {
            let mut text = row.clone();
            ui.push_id(i, |ui| {
                ui.horizontal(|ui| {
                    let row_label = ui.label(format!("{}.", i + 1));
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut text)
                                .id(egui::Id::new(("pixel-exception", i)))
                                .desired_width((ui.available_width() - 96.0).max(100.0)),
                        )
                        .labelled_by(row_label.id)
                        .changed()
                    {
                        action = Some(SettingsAction::EditException(i, text));
                    }
                    if ui.button("Remove").clicked() {
                        action = Some(SettingsAction::RemoveException(i));
                    }
                });
            });
        }
        if let Some(action) = action {
            self.apply(action);
        }
    }
}
impl PixelSettings {
    fn draw(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty() && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Discard::Close);
        }
        let validation = self.draft.candidate().err().map(|e| format!("{e:#}"));
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("UPYR / SETTINGS")
                        .strong()
                        .color(Color32::from_rgb(142, 224, 184)),
                );
                ui.label(if self.dirty() { "[UNSAVED]" } else { "[SAVED]" });
            });
            ui.label("Correction & Exceptions · Full settings: upyr settings");
            ui.add_enabled_ui(self.confirm.is_none(), |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.section, Section::Correction, "Correction");
                    ui.selectable_value(&mut self.section, Section::Exceptions, "Exceptions");
                });
            });
        });
        egui::TopBottomPanel::bottom("actions").show(ctx, |ui| {
            if let Some(error) = &validation {
                ui.colored_label(Color32::from_rgb(255, 173, 132), error);
            }
            if !self.status.is_empty() {
                ui.label(&self.status);
            }
            ui.add_enabled_ui(self.confirm.is_none(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(validation.is_none(), egui::Button::new("Save"))
                        .clicked()
                    {
                        self.save(&LocalStore);
                    }
                    if ui
                        .add_enabled(self.dirty(), egui::Button::new("Revert"))
                        .clicked()
                    {
                        self.revert();
                    }
                    if ui.button("Reload").clicked() {
                        if self.dirty() {
                            self.confirm = Some(Discard::Reload);
                        } else {
                            self.reload(&LocalStore);
                        }
                    }
                });
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_enabled_ui(self.confirm.is_none(), |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| match self.section {
                    Section::Correction => self.correction(ui),
                    Section::Exceptions => self.exceptions(ui),
                });
            });
        });
        if let Some(discard) = self.confirm {
            egui::Modal::new(egui::Id::new("discard")).show(ctx, |ui| {
                ui.heading("Discard unsaved changes?");
                ui.label("Your edits have not been saved.");
                ui.horizontal(|ui| {
                    if ui.button("Keep editing").clicked() {
                        self.confirm = None;
                    }
                    if ui.button("Discard changes").clicked() {
                        self.confirm = None;
                        match discard {
                            Discard::Close => {
                                self.allow_close = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            Discard::Reload => self.reload(&LocalStore),
                        }
                    }
                });
            });
        }
    }
}

impl eframe::App for PixelSettings {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct Store {
        config: RefCell<Config>,
        fail: Cell<bool>,
    }
    impl SettingsStore for Store {
        fn load(&self) -> Result<Config> {
            anyhow::ensure!(!self.fail.get(), "read failed");
            Ok(self.config.borrow().clone())
        }
        fn write(&self, config: &Config) -> Result<()> {
            self.config.replace(config.clone());
            Ok(())
        }
    }
    fn app() -> PixelSettings {
        PixelSettings {
            draft: SettingsDraft::new(Config::default()),
            section: Section::Correction,
            new_exception: String::new(),
            status: String::new(),
            confirm: None,
            allow_close: false,
        }
    }
    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(480.0, 400.0),
            )),
            events,
            focused: true,
            ..Default::default()
        }
    }
    fn replace_text(text: &str) -> Vec<egui::Event> {
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            },
            egui::Event::Paste(text.into()),
        ]
    }
    #[test]
    fn gui_edits_unicode_via_keyboard_and_publishes_named_fields() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = app();
        app.draft
            .apply(SettingsAction::AddExceptions("Ґанок".into()))
            .unwrap();
        app.section = Section::Exceptions;
        let _ = ctx.run(input(vec![]), |ctx| app.draw(ctx));
        ctx.memory_mut(|m| m.request_focus(egui::Id::new(("pixel-exception", 0_usize))));
        let value = "Їжак Єва Іван Ґанок ".repeat(20);
        let out = ctx.run(input(replace_text(&value)), |ctx| app.draw(ctx));
        assert_eq!(app.draft.edited.auto_correct_exceptions[0], value);
        assert_eq!(
            app.draft.candidate().unwrap().auto_correct_exceptions,
            ["Їжак", "Єва", "Іван", "Ґанок"]
        );
        let tree = out.platform_output.accesskit_update.expect("semantic tree");
        let fields: Vec<_> = tree
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == egui::accesskit::Role::TextInput)
            .collect();
        assert!(fields.len() >= 2);
        assert!(fields.iter().all(|(_, n)| !n.labelled_by().is_empty()));
        let focused = ctx.memory(|m| m.focused()).expect("edited row is focused");
        let tab = |modifiers, pressed| egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        let _ = ctx.run(input(vec![tab(egui::Modifiers::NONE, true)]), |ctx| {
            app.draw(ctx)
        });
        let _ = ctx.run(input(vec![tab(egui::Modifiers::NONE, false)]), |ctx| {
            app.draw(ctx)
        });
        assert_ne!(ctx.memory(|m| m.focused()), Some(focused));
        let mut back = input(vec![tab(egui::Modifiers::SHIFT, true)]);
        back.modifiers = egui::Modifiers::SHIFT;
        let _ = ctx.run(back, |ctx| app.draw(ctx));
        let _ = ctx.run(input(vec![tab(egui::Modifiers::SHIFT, false)]), |ctx| {
            app.draw(ctx)
        });
        assert_eq!(ctx.memory(|m| m.focused()), Some(focused));
    }
    #[test]
    fn invalid_keyboard_edit_blocks_save_and_dirty_close_requests_confirmation() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = app();
        let _ = ctx.run(input(vec![]), |ctx| app.draw(ctx));
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("pixel-minimum")));
        let _ = ctx.run(input(replace_text("bad")), |ctx| app.draw(ctx));
        assert_eq!(app.draft.minimum, "bad");
        assert!(app.draft.candidate().is_err());
        let out = ctx.run(input(vec![]), |ctx| app.draw(ctx));
        let tree = out.platform_output.accesskit_update.unwrap();
        assert!(
            tree.nodes
                .iter()
                .any(|(_, n)| n.label() == Some("Save") && n.is_disabled())
        );
        let mut close = input(vec![]);
        close
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .events
            .push(egui::ViewportEvent::Close);
        let out = ctx.run(close, |ctx| app.draw(ctx));
        assert!(matches!(app.confirm, Some(Discard::Close)));
        assert!(
            out.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::CancelClose)
        );
    }

    #[test]
    fn pending_exception_is_protected_and_saved_without_pressing_add() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.section = Section::Exceptions;
        let _ = ctx.run(input(vec![]), |ctx| app.draw(ctx));
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("pixel-new-exception")));
        let _ = ctx.run(input(replace_text("Їжак, Єва їжак")), |ctx| {
            app.draw(ctx)
        });
        assert!(app.dirty());
        assert!(!app.draft.dirty());
        let mut close = input(vec![]);
        close
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .events
            .push(egui::ViewportEvent::Close);
        let out = ctx.run(close, |ctx| app.draw(ctx));
        assert!(matches!(app.confirm, Some(Discard::Close)));
        assert!(
            out.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::CancelClose)
        );

        let store = Store {
            config: RefCell::new(Config::default()),
            fail: Cell::new(true),
        };
        app.confirm = None;
        app.reload(&store);
        assert_eq!(app.new_exception, "Їжак, Єва їжак");
        app.save(&store);
        assert!(app.dirty());
        assert_eq!(
            app.draft.candidate().unwrap().auto_correct_exceptions,
            ["Їжак", "Єва"]
        );
        store.fail.set(false);
        app.save(&store);
        assert!(!app.dirty());
        assert_eq!(
            store.config.borrow().auto_correct_exceptions,
            ["Їжак", "Єва"]
        );

        app.new_exception = "Іван".into();
        app.revert();
        assert!(app.new_exception.is_empty());
        assert!(!app.dirty());
        app.new_exception = "Ґанок".into();
        app.reload(&store);
        assert!(app.new_exception.is_empty());
        assert!(!app.dirty());
    }
}
