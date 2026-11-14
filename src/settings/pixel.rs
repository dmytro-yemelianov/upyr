//! Optional retro form renderer. Semantic egui widgets retain Unicode editing.
use super::{
    ACTIVATION_POLL_INTERVAL, ActivationInbox, SEARCH_PARAMETERS, SettingsApp, SettingsTab,
    draft::{SettingsAction, SettingsDraft},
    navigation::{CONTEXT_HELP, CORRECTION_FOOTNOTE, MIDWORD_HELP},
    persistence::{self, LocalStore, SettingsStore},
    sensitivity_label,
};
use crate::config::{AutoCorrectSensitivity, Config};
use crate::settings::i18n::tr;
use anyhow::{Result, anyhow};
use eframe::egui::{self, Color32, RichText};
use std::{cell::Cell, rc::Rc, time::Instant};

#[cfg(target_os = "macos")]
pub(super) mod flags;
#[derive(Clone, Copy)]
enum Discard {
    Close,
    Reload,
    FullSettings,
}

fn apply_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
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
    ctx.set_style(style);
}

pub(super) fn run(activation: ActivationInbox) -> Result<bool> {
    let config = Config::load()?;
    super::i18n::set_language(config.ui_language);
    let forms = SettingsApp::new(
        config.clone(),
        crate::autostart::status()?,
        SettingsTab::General,
        activation.clone(),
    );
    #[cfg(target_os = "macos")]
    let forms = {
        let mut forms = forms;
        forms.flags.load();
        forms
    };
    let open_full_settings = Rc::new(Cell::new(false));
    let next_editor = Rc::clone(&open_full_settings);
    eframe::run_native(
        tr("Upyr · Pixel Settings"),
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([780.0, 680.0])
                .with_min_inner_size([480.0, 400.0]),
            ..super::native_options()
        },
        Box::new(move |cc| {
            apply_style(&cc.egui_ctx);
            cc.egui_ctx.enable_accesskit();
            Ok(Box::new(PixelSettings {
                draft: SettingsDraft::new(config),
                section: SettingsTab::General,
                forms,
                search: String::new(),
                new_exception: String::new(),
                status: String::new(),
                confirm: None,
                allow_close: false,
                open_full_settings: next_editor,
                activation: Some(activation),
                last_activation_poll: Instant::now(),
            }))
        }),
    )
    .map_err(|error| anyhow!("Could not open pixel settings: {error}"))?;
    Ok(open_full_settings.get())
}

struct PixelSettings {
    draft: SettingsDraft,
    section: SettingsTab,
    forms: SettingsApp,
    search: String,
    new_exception: String,
    status: String,
    confirm: Option<Discard>,
    allow_close: bool,
    open_full_settings: Rc<Cell<bool>>,
    activation: Option<ActivationInbox>,
    last_activation_poll: Instant,
}
impl PixelSettings {
    fn poll_activation(&mut self, ctx: &egui::Context) {
        let Some(activation) = &self.activation else {
            return;
        };
        ctx.request_repaint_after(ACTIVATION_POLL_INTERVAL);
        if self.last_activation_poll.elapsed() < ACTIVATION_POLL_INTERVAL {
            return;
        }
        self.last_activation_poll = Instant::now();
        match activation.drain() {
            Ok(Some(_)) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            Ok(None) => {}
            Err(error) => {
                self.status = crate::settings::i18n::localized_format!(
                    "Could not activate settings: {error:#}",
                    "Не вдалося активувати налаштування: {error:#}"
                )
            }
        }
    }
    fn request_full_settings(&mut self, ctx: &egui::Context) {
        if self.dirty() {
            self.confirm = Some(Discard::FullSettings);
        } else {
            self.open_full_settings(ctx);
        }
    }
    fn open_full_settings(&mut self, ctx: &egui::Context) {
        self.open_full_settings.set(true);
        self.allow_close = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    fn dirty(&self) -> bool {
        self.draft.dirty() || !self.new_exception.trim().is_empty() || self.forms.startup_dirty()
    }
    fn stage_new_exceptions(&mut self) {
        let text = std::mem::take(&mut self.new_exception);
        self.apply(SettingsAction::AddExceptions(text));
    }
    fn revert(&mut self) {
        self.new_exception.clear();
        self.apply(SettingsAction::Revert);
        self.forms.launch_at_login = self.forms.autostart_status.enabled;
        self.forms.recording = None;
        self.forms.shortcut_error = None;
    }
    fn apply(&mut self, action: SettingsAction) {
        self.status = match self.draft.apply(action) {
            Ok(()) => String::new(),
            Err(e) => format!("{e:#}"),
        };
    }
    fn save(&mut self, store: &impl SettingsStore) -> bool {
        self.stage_new_exceptions();
        let result = persistence::save(&mut self.draft, store);
        let saved = result.is_ok();
        self.status = match result {
            Ok(()) => tr("Saved. Upyr will reload these settings.").into(),
            Err(e) => crate::settings::i18n::localized_format!(
                "Save failed: {e:#}",
                "Не вдалося зберегти: {e:#}"
            ),
        };
        saved
    }
    fn reload(&mut self, store: &impl SettingsStore) -> bool {
        let result = persistence::reload(&mut self.draft, store);
        let loaded = result.is_ok();
        self.status = match result {
            Ok(()) => {
                self.new_exception.clear();
                self.forms.launch_at_login = self.forms.autostart_status.enabled;
                self.forms.recording = None;
                self.forms.shortcut_error = None;
                tr("Reloaded from disk.").into()
            }
            Err(e) => crate::settings::i18n::localized_format!(
                "Reload failed: {e:#}",
                "Не вдалося перечитати: {e:#}"
            ),
        };
        loaded
    }
    fn correction(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("CORRECTION"));
        let mut automatic = self.draft.edited.auto_correct;
        if ui
            .checkbox(&mut automatic, tr("Correct the wrong layout automatically"))
            .changed()
        {
            self.apply(SettingsAction::Automatic(automatic));
        }
        let mut follow = self.draft.edited.switch_layout;
        if ui
            .checkbox(&mut follow, tr("Switch OS layout to match corrected text"))
            .changed()
        {
            self.apply(SettingsAction::SwitchLayout(follow));
        }
        ui.separator();
        ui.label(tr(MIDWORD_HELP));
        ui.label(tr(CONTEXT_HELP));
        ui.separator();
        ui.label(tr("Sensitivity"));
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
        let minimum_label = ui.label(tr("Minimum word length (2–32)"));
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
        let delay_label = ui.label(tr("Pause after Space (10–250 ms)"));
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
        ui.small(tr("Clear prefixes can switch immediately while typing."));
        #[cfg(target_os = "macos")]
        {
            let mut terminal = super::macos::terminal_correction_enabled(&self.draft.edited);
            if ui
                .checkbox(&mut terminal, tr("Allow correction in terminals"))
                .changed()
            {
                super::macos::set_terminal_correction(&mut self.draft.edited, terminal);
            }
            let mut slash = super::macos::superset_slash_commands_enabled(&self.draft.edited);
            if ui
                .checkbox(
                    &mut slash,
                    tr("Correct freshly typed /commands in Superset"),
                )
                .changed()
            {
                super::macos::set_superset_slash_commands(&mut self.draft.edited, slash);
            }
        }
        ui.separator();
        ui.label(tr(CORRECTION_FOOTNOTE));
    }
    fn exceptions(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("EXCEPTIONS"));
        ui.label(tr(
            "Words Upyr should leave alone. Spaces and commas separate words.",
        ));
        let add_label = ui.label(tr("Add words"));
        ui.add(
            egui::TextEdit::singleline(&mut self.new_exception)
                .id(egui::Id::new("pixel-new-exception"))
                .hint_text("Ґанок, Їжак, GitHub")
                .desired_width(f32::INFINITY),
        )
        .labelled_by(add_label.id);
        if ui.button(tr("Add words")).clicked() {
            self.stage_new_exceptions();
        }
        ui.separator();
        if self.draft.edited.auto_correct_exceptions.is_empty() {
            ui.label(tr("No exceptions yet."));
        }
        let mut action = None;
        for (i, row) in self.draft.edited.auto_correct_exceptions.iter().enumerate() {
            let mut text = row.clone();
            ui.push_id(i, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let row_label = ui.label(format!("{}.", i + 1));
                    let remove_width = ui
                        .painter()
                        .layout_no_wrap(
                            tr("Remove").to_owned(),
                            egui::TextStyle::Button.resolve(ui.style()),
                            ui.visuals().text_color(),
                        )
                        .size()
                        .x
                        + 2.0 * ui.spacing().button_padding.x
                        + ui.spacing().item_spacing.x
                        + 16.0;
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut text)
                                .id(egui::Id::new(("pixel-exception", i)))
                                .desired_width((ui.available_width() - remove_width).max(100.0)),
                        )
                        .labelled_by(row_label.id)
                        .changed()
                    {
                        action = Some(SettingsAction::EditException(i, text));
                    }
                    if ui.button(tr("Remove")).clicked() {
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
    fn capture_shortcut(&mut self, ctx: &egui::Context) {
        if self.confirm.is_none() && self.forms.recording.is_some() {
            self.forms.config.clone_from(&self.draft.edited);
            self.forms.capture_shortcut(ctx);
            self.draft.edited.clone_from(&self.forms.config);
            // A recorded Space/Enter must not also press a form button.
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(event, egui::Event::Key { .. } | egui::Event::Text(_))
                })
            });
        }
    }

    fn other_page(&mut self, ui: &mut egui::Ui) {
        self.forms.config.clone_from(&self.draft.edited);
        match self.section {
            SettingsTab::General => self.forms.draw_general(ui),
            SettingsTab::Shortcuts => self.forms.draw_shortcuts(ui),
            SettingsTab::Feedback => self.forms.draw_feedback(ui),
            SettingsTab::Advanced => self.forms.draw_advanced(ui),
            SettingsTab::About => self.forms.draw_about(ui),
            SettingsTab::Automatic | SettingsTab::Exceptions => unreachable!(),
        }
        self.draft.edited.clone_from(&self.forms.config);
        if let Some((_, message)) = self.forms.status.take() {
            self.status = message;
        }
    }

    fn reload_desktop(
        &mut self,
        store: &impl SettingsStore,
        startup: Result<crate::autostart::AutostartStatus>,
    ) {
        // Read both sources before accepting either, so a failed OS status
        // read cannot discard a pending launch-at-login choice.
        match startup {
            Ok(status) => {
                if self.reload(store) {
                    self.forms.launch_at_login = status.enabled;
                    self.forms.autostart_status = status;
                }
            }
            Err(error) => {
                self.status = crate::settings::i18n::localized_format!(
                    "Reload failed: could not read launch-at-login status: {error:#}",
                    "Не вдалося перечитати стан автозапуску: {error:#}"
                )
            }
        }
    }

    fn draw(&mut self, ctx: &egui::Context) {
        super::i18n::set_language(self.draft.edited.ui_language);
        self.poll_activation(ctx);
        self.capture_shortcut(ctx);
        #[cfg(target_os = "macos")]
        self.forms.flags.poll_preview(ctx);
        #[cfg(target_os = "macos")]
        if self.section == SettingsTab::Feedback && self.activation.is_some() {
            self.forms.refresh_audio_status();
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty() && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Discard::Close);
        }
        let validation = self.draft.candidate().err().map(|e| format!("{e:#}"));
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(tr("UPYR / SETTINGS"))
                        .strong()
                        .color(Color32::from_rgb(142, 224, 184)),
                );
                ui.label(if self.dirty() {
                    tr("[UNSAVED]")
                } else {
                    tr("[SAVED]")
                });
            });
            ui.add_enabled_ui(self.confirm.is_none(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr("All settings"));
                    if ui.button(tr("Native interface…")).clicked() {
                        self.request_full_settings(ctx);
                    }
                });
                let label = ui.label(tr("Search settings"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .id(egui::Id::new("pixel-search"))
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id);
                ui.horizontal_wrapped(|ui| {
                    for tab in SettingsTab::ALL {
                        if ui
                            .selectable_label(self.section == tab, tab.label())
                            .clicked()
                        {
                            self.section = tab;
                            self.search.clear();
                            self.forms.recording = None;
                        }
                    }
                });
            });
        });
        egui::TopBottomPanel::bottom("actions").show(ctx, |ui| {
            if let Some(error) = &validation {
                ui.colored_label(
                    Color32::from_rgb(255, 173, 132),
                    super::i18n::diagnostic(error),
                );
            }
            if !self.status.is_empty() {
                ui.label(super::i18n::diagnostic(&self.status));
            }
            ui.add_enabled_ui(self.confirm.is_none(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(validation.is_none(), egui::Button::new(tr("Save")))
                        .clicked()
                        && self.save(&LocalStore)
                    {
                        self.forms.save_startup();
                        if let Some((_, message)) = self.forms.status.take() {
                            self.status = message;
                        }
                    }
                    if ui
                        .add_enabled(self.dirty(), egui::Button::new(tr("Revert")))
                        .clicked()
                    {
                        self.revert();
                    }
                    if ui.button(tr("Reload")).clicked() {
                        if self.dirty() {
                            self.confirm = Some(Discard::Reload);
                        } else {
                            self.reload_desktop(&LocalStore, crate::autostart::status());
                        }
                    }
                    if ui.button(tr("Defaults")).clicked() {
                        self.apply(SettingsAction::Defaults);
                        self.new_exception.clear();
                        self.forms.recording = None;
                        self.forms.shortcut_error = None;
                        self.status = tr("Defaults staged. Choose Save to apply them.").into();
                    }
                });
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_enabled_ui(self.confirm.is_none(), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(self.section.command())
                    .show(ui, |ui| {
                        if !self.search.trim().is_empty() {
                            let mut count = 0;
                            let query = self.search.clone();
                            for item in SEARCH_PARAMETERS.iter().filter(|item| item.matches(&query))
                            {
                                count += 1;
                                if ui
                                    .button(format!("{} › {}", item.tab.label(), tr(item.label)))
                                    .clicked()
                                {
                                    self.section = item.tab;
                                    self.search.clear();
                                    break;
                                }
                            }
                            if count == 0 {
                                ui.label(tr("No settings match this search."));
                            }
                        } else {
                            match self.section {
                                SettingsTab::Automatic => self.correction(ui),
                                SettingsTab::Exceptions => self.exceptions(ui),
                                _ => self.other_page(ui),
                            }
                        }
                    });
            });
        });
        if let Some(discard) = self.confirm {
            egui::Modal::new(egui::Id::new("discard")).show(ctx, |ui| {
                ui.heading(tr("Discard unsaved changes?"));
                ui.label(tr("Your edits have not been saved."));
                ui.horizontal(|ui| {
                    if ui.button(tr("Keep editing")).clicked() {
                        self.confirm = None;
                    }
                    if ui.button(tr("Discard changes")).clicked() {
                        self.confirm = None;
                        match discard {
                            Discard::Close => {
                                self.allow_close = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            Discard::Reload => {
                                self.reload_desktop(&LocalStore, crate::autostart::status());
                            }
                            Discard::FullSettings => self.open_full_settings(ctx),
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
        let config = Config {
            ui_language: crate::config::UiLanguage::English,
            ..Config::default()
        };
        let forms = SettingsApp::new(
            config.clone(),
            crate::autostart::AutostartStatus {
                enabled: false,
                state: crate::autostart::AutostartState::Disabled,
                location: String::new(),
                detail: None,
            },
            SettingsTab::General,
            ActivationInbox {
                directory: std::env::temp_dir().join("unused-pixel-test-inbox"),
            },
        );
        PixelSettings {
            draft: SettingsDraft::new(config),
            forms,
            search: String::new(),
            section: SettingsTab::Automatic,
            new_exception: String::new(),
            status: String::new(),
            confirm: None,
            allow_close: false,
            open_full_settings: Rc::new(Cell::new(false)),
            activation: None,
            last_activation_poll: Instant::now(),
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
    fn ukrainian_pages_language_picker_save_and_revert_preserve_user_values() {
        use crate::config::UiLanguage;
        let ctx = egui::Context::default();
        apply_style(&ctx);
        ctx.enable_accesskit();
        let mut app = app();
        let config = Config {
            auto_correct_exceptions: vec!["General".into(), "Українська".into()],
            ..Config::default()
        };
        app.draft.accept(config.clone());
        let store = Store {
            config: RefCell::new(config.clone()),
            fail: Cell::new(false),
        };
        let frame = |events| {
            let mut raw = input(events);
            raw.screen_rect = Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(780.0, 1800.0),
            ));
            raw
        };
        for (tab, label) in [
            (SettingsTab::General, "Мова інтерфейсу"),
            (SettingsTab::Automatic, "ВИПРАВЛЕННЯ"),
            (SettingsTab::Exceptions, "ВИНЯТКИ"),
            (SettingsTab::Shortcuts, "Перетворити виділене"),
            (SettingsTab::Feedback, "Візуальні індикатори"),
            (SettingsTab::Advanced, "Затримка копіювання"),
            (SettingsTab::About, "Приватність за задумом"),
        ] {
            app.section = tab;
            let _ = ctx.run(frame(vec![]), |ctx| app.draw(ctx));
            let out = ctx.run(frame(vec![]), |ctx| app.draw(ctx));
            let tree = out.platform_output.accesskit_update.unwrap();
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some(label) || n.value() == Some(label)),
                "missing {label}"
            );
        }
        for tab in SettingsTab::ALL {
            app.section = tab;
            let _ = ctx.run(input(vec![]), |ctx| app.draw(ctx));
            let _ = ctx.run(input(vec![]), |ctx| app.draw(ctx));
            assert!(
                ctx.used_size().x <= 481.0,
                "Ukrainian {} overflows: {:?}",
                tab.label(),
                ctx.used_size()
            );
        }
        app.section = SettingsTab::General;
        let click = |app: &mut PixelSettings, label: &str| {
            let _ = ctx.run(frame(vec![]), |ctx| app.draw(ctx));
            let out = ctx.run(frame(vec![]), |ctx| app.draw(ctx));
            let tree = out.platform_output.accesskit_update.unwrap();
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, n)| n.label() == Some(label) || n.value() == Some(label))
                .unwrap_or_else(|| {
                    panic!(
                        "missing {label}: {:?}",
                        tree.nodes
                            .iter()
                            .map(|(_, n)| (n.role(), n.label(), n.value()))
                            .collect::<Vec<_>>()
                    )
                })
                .1
                .bounds()
                .unwrap();
            let pos = egui::pos2(
                ((bounds.x0 + bounds.x1) / 2.0) as f32,
                ((bounds.y0 + bounds.y1) / 2.0) as f32,
            );
            for pressed in [true, false] {
                let _ = ctx.run(
                    frame(vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]),
                    |ctx| app.draw(ctx),
                );
            }
        };
        click(&mut app, "Мова інтерфейсу: Українська");
        click(&mut app, "English");
        let mut expected = config.clone();
        expected.ui_language = UiLanguage::English;
        assert_eq!(app.draft.edited, expected);
        assert!(app.dirty());
        app.revert();
        assert_eq!(app.draft.edited.ui_language, UiLanguage::Ukrainian);
        assert!(!app.dirty());
        app.draft.edited.ui_language = UiLanguage::English;
        assert!(app.save(&store));
        assert_eq!(store.config.borrow().ui_language, UiLanguage::English);
        assert_eq!(
            store.config.borrow().auto_correct_exceptions,
            config.auto_correct_exceptions
        );
        assert_eq!(store.config.borrow().hotkey, config.hotkey);
        assert!(!app.dirty());
        app.draft.edited.ui_language = UiLanguage::Ukrainian;
        assert!(app.reload(&store));
        assert_eq!(app.draft.edited.ui_language, UiLanguage::English);
    }

    #[test]
    fn gui_edits_unicode_via_keyboard_and_publishes_named_fields() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = app();
        app.draft
            .apply(SettingsAction::AddExceptions("Ґанок".into()))
            .unwrap();
        app.section = SettingsTab::Exceptions;
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
        app.section = SettingsTab::Exceptions;
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

    #[test]
    fn menu_activation_restores_focus_without_replacing_unsaved_edits() {
        use super::super::{ACTIVATION_SEQUENCE, SettingsTab};
        use std::sync::atomic::Ordering;
        let unique = ACTIVATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "upyr-pixel-activation-{}-{unique}",
            std::process::id()
        ));
        let inbox = ActivationInbox::at(directory.clone()).unwrap();
        let ctx = egui::Context::default();
        let mut app = app();
        app.new_exception = "Їжак".into();
        app.section = SettingsTab::Exceptions;
        app.activation = Some(inbox.clone());
        for _ in 0..2 {
            inbox.send(SettingsTab::General).unwrap();
            app.last_activation_poll = Instant::now() - ACTIVATION_POLL_INTERVAL;
            let out = ctx.run(input(vec![]), |ctx| app.draw(ctx));
            let commands = &out.viewport_output[&egui::ViewportId::ROOT].commands;
            assert!(commands.contains(&egui::ViewportCommand::Minimized(false)));
            assert!(commands.contains(&egui::ViewportCommand::Focus));
            assert_eq!(app.new_exception, "Їжак");
            assert!(matches!(app.section, SettingsTab::Exceptions));
            assert!(app.dirty());
            assert_eq!(inbox.drain().unwrap(), None);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn full_settings_handoff_requires_explicit_discard_of_pending_input() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.new_exception = "Ґанок".into();
        let out = ctx.run(input(vec![]), |ctx| {
            app.request_full_settings(ctx);
            app.draw(ctx);
        });
        assert!(matches!(app.confirm, Some(Discard::FullSettings)));
        assert!(!app.open_full_settings.get());
        assert!(!app.allow_close);
        assert!(
            !out.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::Close)
        );
        assert_eq!(app.new_exception, "Ґанок");

        // The explicit discard handler closes this editor and records the
        // handoff; the host launches the replacement after releasing its guard.
        let out = ctx.run(input(vec![]), |ctx| app.open_full_settings(ctx));
        assert!(app.open_full_settings.get());
        assert!(app.allow_close);
        assert!(
            out.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::Close)
        );

        let mut clean = self::app();
        let _ = ctx.run(input(vec![]), |ctx| clean.request_full_settings(ctx));
        assert!(clean.confirm.is_none());
        assert!(clean.open_full_settings.get());
    }

    #[test]
    fn every_page_renders_at_small_size_and_search_preserves_the_draft() {
        let ctx = egui::Context::default();
        apply_style(&ctx);
        ctx.enable_accesskit();
        let mut app = app();
        app.draft.edited.restore_clipboard = false;
        for (tab, expected) in [
            (SettingsTab::General, "Conversion direction"),
            (SettingsTab::Automatic, "CORRECTION"),
            (SettingsTab::Exceptions, "EXCEPTIONS"),
            (SettingsTab::Shortcuts, "Convert selection"),
            (SettingsTab::Feedback, "Visual feedback"),
            (SettingsTab::Advanced, "Copy delay"),
            (SettingsTab::About, "Private by construction"),
        ] {
            app.section = tab;
            let _ = ctx.run(input(vec![]), |ctx| app.draw(ctx));
            let out = ctx.run(input(vec![]), |ctx| app.draw(ctx));
            let tree = out.platform_output.accesskit_update.unwrap();
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some(expected) || n.value() == Some(expected)),
                "missing {expected}: {:?}",
                tree.nodes
                    .iter()
                    .filter_map(|(_, n)| n.label())
                    .collect::<Vec<_>>()
            );
            assert!(
                ctx.used_size().x <= 481.0,
                "{} overflows: {:?}",
                tab.label(),
                ctx.used_size()
            );
            assert!(!app.draft.edited.restore_clipboard);
        }
        app.search = "copy delay".into();
        let out = ctx.run(input(vec![]), |ctx| app.draw(ctx));
        let tree = out.platform_output.accesskit_update.unwrap();
        let result = tree
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("Advanced › Copy delay"))
            .unwrap()
            .1
            .bounds()
            .unwrap();
        let pos = egui::pos2(
            ((result.x0 + result.x1) / 2.0) as f32,
            ((result.y0 + result.y1) / 2.0) as f32,
        );
        for pressed in [true, false] {
            let _ = ctx.run(
                input(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]),
                |ctx| app.draw(ctx),
            );
        }
        assert_eq!(app.section, SettingsTab::Advanced);
        assert!(app.search.is_empty());
        assert!(app.dirty());
        assert!(!app.draft.edited.restore_clipboard);
    }

    #[test]
    fn startup_edits_remain_dirty_on_failure_and_revert_without_os_effects() {
        let mut app = app();
        app.forms.launch_at_login = true;
        assert!(app.dirty());
        app.forms
            .accept_startup_result(Err(anyhow!("simulated failure")));
        assert!(app.forms.launch_at_login);
        assert!(app.dirty());
        app.revert();
        assert!(!app.forms.launch_at_login);
        assert!(!app.dirty());
        app.forms.launch_at_login = true;
        let mut enabled = app.forms.autostart_status.clone();
        enabled.enabled = true;
        enabled.state = crate::autostart::AutostartState::Enabled;
        app.forms.accept_startup_result(Ok(enabled));
        assert!(!app.dirty());
        let store = Store {
            config: RefCell::new(Config::default()),
            fail: Cell::new(true),
        };
        app.forms.launch_at_login = false;
        assert!(!app.save(&store));
        assert!(!app.reload(&store));
        assert!(!app.forms.launch_at_login);
        assert!(app.dirty());
        store.fail.set(false);
        app.draft.edited.restore_clipboard = false;
        app.reload_desktop(&store, Err(anyhow!("OS status unavailable")));
        assert!(!app.forms.launch_at_login);
        assert!(!app.draft.edited.restore_clipboard);
        assert!(app.dirty());
    }

    #[test]
    fn recorder_uses_physical_keys_rejects_conflicts_and_consumes_input() {
        use super::super::ShortcutSlot;
        use global_hotkey::hotkey::{Code, HotKey};
        let ctx = egui::Context::default();
        let mut app = app();
        app.section = SettingsTab::Shortcuts;
        app.forms.recording = Some(ShortcutSlot::PreviousWord);
        let modifiers = egui::Modifiers {
            alt: true,
            command: true,
            mac_cmd: cfg!(target_os = "macos"),
            ctrl: !cfg!(target_os = "macos"),
            ..Default::default()
        };
        let event = |key, physical_key| egui::Event::Key {
            key,
            physical_key: Some(physical_key),
            pressed: true,
            repeat: false,
            modifiers,
        };
        let previous = app.draft.edited.last_word_hotkey.clone();
        let _ = ctx.run(
            input(vec![event(egui::Key::Space, egui::Key::Space)]),
            |ctx| {
                app.draw(ctx);
                assert!(!ctx.input(|i| {
                    i.events
                        .iter()
                        .any(|e| matches!(e, egui::Event::Key { .. }))
                }));
            },
        );
        assert_eq!(app.draft.edited.last_word_hotkey, previous);
        assert!(app.forms.shortcut_error.is_some());
        let _ = ctx.run(
            input(vec![event(egui::Key::Escape, egui::Key::Escape)]),
            |ctx| app.draw(ctx),
        );
        assert!(app.forms.recording.is_none());
        assert_eq!(app.draft.edited.last_word_hotkey, previous);
        app.forms.recording = Some(ShortcutSlot::PreviousWord);
        let _ = ctx.run(input(vec![event(egui::Key::Y, egui::Key::K)]), |ctx| {
            app.draw(ctx)
        });
        let saved: HotKey = app.draft.edited.last_word_hotkey.parse().unwrap();
        assert_eq!(saved.key, Code::KeyK);
        assert!(app.forms.recording.is_none());
        assert!(app.dirty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn pixel_pages_include_the_native_macos_controls() {
        let ctx = egui::Context::default();
        apply_style(&ctx);
        ctx.enable_accesskit();
        let mut app = app();
        let frame = || {
            let mut input = input(vec![]);
            input.screen_rect = Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(780.0, 1800.0),
            ));
            input
        };
        for (tab, expected) in [
            (
                SettingsTab::General,
                vec![
                    "Launch Upyr at login",
                    "Tap Globe to switch keyboard layouts",
                    "Restore clipboard contents after conversion",
                ],
            ),
            (
                SettingsTab::Automatic,
                vec![
                    "Allow correction in terminals",
                    "Correct freshly typed /commands in Superset",
                ],
            ),
            (
                SettingsTab::Shortcuts,
                vec![
                    "Convert selection",
                    "Fix previous word",
                    "Undo auto-correction",
                    "Modifier gesture",
                    "Double-tap timeout",
                ],
            ),
            (
                SettingsTab::Feedback,
                vec![
                    "All Country Flags…",
                    "Preview layout",
                    "Headphone limit",
                    "Enable sound feedback",
                    "Sound pack",
                    "Preview click",
                    "Preview control",
                ],
            ),
            (
                SettingsTab::Advanced,
                vec!["Copy delay", "Paste delay", "Clipboard restore delay"],
            ),
        ] {
            app.section = tab;
            let _ = ctx.run(frame(), |ctx| app.draw(ctx));
            let out = ctx.run(frame(), |ctx| app.draw(ctx));
            let tree = out.platform_output.accesskit_update.unwrap();
            for text in expected {
                assert!(
                    tree.nodes
                        .iter()
                        .any(|(_, n)| n.label() == Some(text) || n.value() == Some(text)),
                    "{} lacks {text}",
                    tab.label()
                );
            }
        }
    }
}
