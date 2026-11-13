#![allow(unsafe_code)]
use crate::settings::i18n::tr;

use std::{
    cell::{Cell, OnceCell, RefCell},
    process::Command,
};

use anyhow::{Context, Result, anyhow};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSColor,
    NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSFont,
    NSPopUpButton, NSScrollView, NSSearchField, NSSlider, NSTabView, NSTabViewItem, NSTextField,
    NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use upyr_audio::KeyCue;

use super::{
    ACTIVATION_POLL_INTERVAL, APP_VERSION, ActivationInbox, IMPLEMENTATION_SUMMARY,
    PRIVACY_SUMMARY, PROJECT_WEBSITE_URL, REPOSITORY_URL, SEARCH_PARAMETERS, SettingsTab,
    autostart_attention,
    flag_choices::FlagChoices,
    navigation::{
        CONTEXT_HELP, CORRECTION_DESCRIPTION, CORRECTION_FOOTNOTE, CORRECTION_TITLE,
        CORRECTION_TOGGLE, MIDWORD_HELP,
    },
    normalize_exceptions, parse_exceptions, pretty_hotkey, sensitivity_label, sync_launch_at_login,
};
use crate::{
    autostart,
    config::{
        AutoCorrectSensitivity, Config, GestureAction, IndicatorStyle, ModifierGesture, SoundEvent,
        SoundPack,
    },
    layout::Direction,
};

const WINDOW_WIDTH: f64 = 720.0;
const WINDOW_HEIGHT: f64 = 680.0;
const PAGE_WIDTH: f64 = 672.0;
const PAGE_HEIGHT: f64 = 456.0;

const EXCEPTION_LIST_WIDTH: f64 = 600.0;
const EXCEPTION_LIST_VISIBLE_HEIGHT: f64 = 250.0;
const EXCEPTION_ROW_HEIGHT: f64 = 26.0;
const EXCEPTION_ROW_STEP: f64 = 32.0;
const EXCEPTION_ROW_TOP_PADDING: f64 = 8.0;
const EXCEPTION_FIELD_WIDTH: f64 = 470.0;
const EXCEPTION_REMOVE_WIDTH: f64 = 90.0;

mod flags_gallery;

struct ControllerIvars {
    updates: RefCell<super::updates::UpdateCheck>,
    config: RefCell<Config>,
    autostart_status: RefCell<autostart::AutostartStatus>,
    activation: ActivationInbox,
    controls: OnceCell<Controls>,
    flags_gallery: OnceCell<flags_gallery::FlagsGallery>,
    audio_poll: Cell<std::time::Instant>,
    flag_choices: RefCell<FlagChoices>,
}

struct Controls {
    updates: UpdateControls,
    window: Retained<NSWindow>,
    tabs: Retained<NSTabView>,
    search: Retained<NSSearchField>,
    status: Retained<NSTextField>,
    ui_language: Retained<NSPopUpButton>,
    direction: Retained<NSPopUpButton>,
    switch_layout: Retained<NSButton>,
    globe_switch: Retained<NSButton>,
    restore_clipboard: Retained<NSButton>,
    launch_at_login: Retained<NSButton>,
    repair_autostart: Retained<NSButton>,
    remove_autostart: Retained<NSButton>,
    auto_correct: Retained<NSButton>,
    terminal_correction: Retained<NSButton>,
    slash_commands: Retained<NSButton>,
    sensitivity: Retained<NSPopUpButton>,
    minimum_word_length: Retained<NSTextField>,
    auto_delay: Retained<NSTextField>,
    exceptions_container: Retained<NSView>,
    exceptions_rows: RefCell<Vec<ExceptionRow>>,
    exceptions_new_word: Retained<NSTextField>,
    selection_shortcut: Retained<ShortcutRecorder>,
    previous_word_shortcut: Retained<ShortcutRecorder>,
    undo_shortcut: Retained<ShortcutRecorder>,
    modifier_gesture: Retained<NSPopUpButton>,
    gesture_action: Retained<NSPopUpButton>,
    gesture_timeout: Retained<NSTextField>,
    show_indicator: Retained<NSButton>,
    indicator_duration: Retained<NSTextField>,
    indicator_style: Retained<NSPopUpButton>,
    flags_preview: Retained<NSView>,
    preview_layout: Retained<NSPopUpButton>,
    preview_sources: Vec<crate::system_layout::ActiveInputSource>,
    sounds_enabled: Retained<NSButton>,
    sound_pack: Retained<NSPopUpButton>,
    key_clicks: Retained<NSButton>,
    sound_volume: Retained<NSSlider>,
    sound_volume_label: Retained<NSTextField>,
    headphone_volume: Retained<NSSlider>,
    headphone_volume_label: Retained<NSTextField>,
    audio_output_status: Retained<NSTextField>,
    sound_events: SoundEventControls,
    copy_delay: Retained<NSTextField>,
    paste_delay: Retained<NSTextField>,
    restore_delay: Retained<NSTextField>,
}

struct ExceptionRow {
    field: Retained<NSTextField>,
    remove: Retained<NSButton>,
}

struct UpdateControls {
    check: Retained<NSButton>,
    open: Retained<NSButton>,
    status: Retained<NSTextField>,
}

struct ExceptionsControls {
    page: Retained<NSView>,
    container: Retained<NSView>,
    new_word: Retained<NSTextField>,
}

struct SoundEventControls {
    auto_correct: Retained<NSButton>,
    manual_conversion: Retained<NSButton>,
    layout_switch: Retained<NSButton>,
    pause: Retained<NSButton>,
    resume: Retained<NSButton>,
    error: Retained<NSButton>,
}

impl SoundEventControls {
    fn button(&self, event: SoundEvent) -> &NSButton {
        match event {
            SoundEvent::AutoCorrect => &self.auto_correct,
            SoundEvent::ManualConversion => &self.manual_conversion,
            SoundEvent::LayoutSwitch => &self.layout_switch,
            SoundEvent::Pause => &self.pause,
            SoundEvent::Resume => &self.resume,
            SoundEvent::Error => &self.error,
        }
    }
}

struct FeedbackControls {
    page: Retained<NSView>,
    show_indicator: Retained<NSButton>,
    indicator_duration: Retained<NSTextField>,
    indicator_style: Retained<NSPopUpButton>,
    flags_preview: Retained<NSView>,
    preview_layout: Retained<NSPopUpButton>,
    preview_sources: Vec<crate::system_layout::ActiveInputSource>,
    sounds_enabled: Retained<NSButton>,
    sound_pack: Retained<NSPopUpButton>,
    key_clicks: Retained<NSButton>,
    sound_volume: Retained<NSSlider>,
    sound_volume_label: Retained<NSTextField>,
    headphone_volume: Retained<NSSlider>,
    headphone_volume_label: Retained<NSTextField>,
    audio_output_status: Retained<NSTextField>,
    sound_events: SoundEventControls,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ControllerIvars]
    struct NativeController;

    unsafe impl NSObjectProtocol for NativeController {}

    unsafe impl NSWindowDelegate for NativeController {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }

    impl NativeController {
        #[unsafe(method(saveSettings:))]
        fn save_settings(&self, _sender: &AnyObject) {
            let edited = match self.collect_config() {
                Ok(config) => config,
                Err(error) => {
                    self.set_status(false, &crate::settings::i18n::localized_format!("Could not save settings: {error:#}", "Не вдалося зберегти налаштування: {error:#}"));
                    return;
                }
            };
            let config = match super::persistence::save_edits(&self.ivars().config.borrow(), edited) {
                Ok(config) => config,
                Err(error) => {
                    self.set_status(false, &crate::settings::i18n::localized_format!("Could not save settings: {error:#}", "Не вдалося зберегти налаштування: {error:#}"));
                    return;
                }
            };
            self.ivars().flag_choices.borrow_mut().accept(config.globe_switch.layout_flags.clone());
            // Show the saved list: an edited "two words" row became two rows.
            self.rebuild_exception_rows(&config.auto_correct_exceptions);
            let language_changed = config.ui_language != super::i18n::language();
            self.ivars().config.replace(config);
            self.refresh_flag_choices();

            let desired = is_checked(&self.controls().launch_at_login);
            match sync_launch_at_login(desired) {
                Ok(status) => {
                    self.set_autostart_status(status);
                    if !self.show_autostart_attention(tr("Settings saved. ")) {
                        self.set_status(true, if language_changed {
                            tr("Saved. Reopen settings to apply the language.")
                        } else {
                            tr("Saved. The running Upyr process will reload these settings.")
                        });
                    }
                }
                Err(error) => {
                    if let Ok(status) = autostart::status() {
                        self.set_autostart_status(status);
                    }
                    self.set_status(
                        false,
                        &crate::settings::i18n::localized_format!("Settings saved, but launch at login was not changed: {error:#}", "Налаштування збережено, але автозапуск не змінився: {error:#}"
                        ),
                    );
                }
            }
        }

        #[unsafe(method(repairAutostart:))]
        fn repair_autostart(&self, _sender: &AnyObject) {
            match autostart::enable() {
                Ok(status) => {
                    self.set_autostart_status(status);
                    self.set_status(true, tr("Launch-at-login entry repaired."));
                }
                Err(error) => {
                    self.set_status(false, &crate::settings::i18n::localized_format!("Could not repair entry: {error:#}", "Не вдалося виправити запис: {error:#}"));
                }
            }
        }

        #[unsafe(method(removeAutostart:))]
        fn remove_autostart(&self, _sender: &AnyObject) {
            match autostart::disable() {
                Ok(status) => {
                    self.set_autostart_status(status);
                    self.set_status(true, tr("Launch-at-login entry removed."));
                }
                Err(error) => {
                    self.set_status(false, &crate::settings::i18n::localized_format!("Could not remove entry: {error:#}", "Не вдалося видалити запис: {error:#}"));
                }
            }
        }

        #[unsafe(method(resetSettings:))]
        fn reset_settings(&self, _sender: &AnyObject) {
            self.apply_config(&Config::default());
            set_checked(&self.controls().launch_at_login, false);
            self.set_status(true, tr("Defaults restored in the form. Choose Save to apply them."));
        }

        #[unsafe(method(closeSettings:))]
        fn close_settings(&self, _sender: &AnyObject) {
            self.controls().window.performClose(None);
        }

        #[unsafe(method(openProjectWebsite:))]
        fn open_project_website(&self, _sender: &AnyObject) {
            self.open_external_url(PROJECT_WEBSITE_URL);
        }

        #[unsafe(method(openSourceRepository:))]
        fn open_source_repository(&self, _sender: &AnyObject) {
            self.open_external_url(REPOSITORY_URL);
        }

        #[unsafe(method(reportSecurityIssue:))]
        fn report_security_issue(&self, _sender: &AnyObject) {
            self.open_external_url(&format!("{REPOSITORY_URL}/security/advisories/new"));
        }

        #[unsafe(method(searchSettings:))]
        fn search_settings(&self, _sender: &AnyObject) {
            let query = self.controls().search.stringValue().to_string().trim().to_lowercase();
            if query.is_empty() {
                self.set_status(true, "");
                return;
            }
            let result = SEARCH_PARAMETERS.iter().find(|parameter| parameter.matches(&query));
            if let Some(parameter) = result {
                self.controls()
                    .tabs
                    .selectTabViewItemAtIndex(parameter.tab.index());
                self.set_status(
                    true,
                    &format!("{}  ›  {}", parameter.tab.label(), tr(parameter.label)),
                );
            } else {
                self.set_status(false, tr("No settings match this search."));
            }
        }

        #[unsafe(method(resetSelectionShortcut:))]
        fn reset_selection_shortcut(&self, _sender: &AnyObject) {
            self.controls()
                .selection_shortcut
                .set_value(&Config::default().hotkey);
            self.validate_shortcuts();
        }

        #[unsafe(method(resetPreviousShortcut:))]
        fn reset_previous_shortcut(&self, _sender: &AnyObject) {
            self.controls()
                .previous_word_shortcut
                .set_value(&Config::default().last_word_hotkey);
            self.validate_shortcuts();
        }

        #[unsafe(method(previewFeedback:))]
        fn preview_feedback(&self, _sender: &AnyObject) {
            let controls = self.controls();
            let preview = Config {
                show_layout_indicator: true,
                layout_indicator_duration_ms: parse_number(
                    &controls.indicator_duration, "indicator duration", 250, 3_000,
                ).unwrap_or(900),
                indicator_style: self.selected_indicator_style(),
                ..self.flag_preview_config()
            };
            self.refresh_flag_preview();
            let indicator_shown = self.selected_preview_source().is_some_and(|source| {
                crate::feedback::globe_switched(None, source, &preview)
            });
            if indicator_shown {
                unsafe {
                    let _: () = msg_send![
                        self,
                        performSelector: sel!(hideFeedback:),
                        withObject: None::<&AnyObject>,
                        afterDelay: preview.layout_indicator_duration_ms as f64 / 1000.0
                    ];
                }
            }
            self.set_status(true, tr("Previewed the language flag."));
        }

        #[unsafe(method(flagPreviewChanged:))]
        fn flag_preview_changed(&self, _sender: &AnyObject) {
            if let Some(gallery) = self.ivars().flags_gallery.get() {
                gallery.select_layout(self.selected_preview_source().map(|source| source.source_id.as_str()));
            }
            self.refresh_flag_choices();
        }

        #[unsafe(method(openCountryFlags:))]
        fn open_country_flags(&self, _sender: &AnyObject) {
            let gallery = self.ivars().flags_gallery.get_or_init(|| {
                flags_gallery::FlagsGallery::new(self, self.mtm())
            });
            gallery.select_layout(self.selected_preview_source().map(|source| source.source_id.as_str()));
            self.refresh_flag_choices();
            gallery.show();
        }

        #[unsafe(method(searchCountryFlags:))]
        fn search_country_flags(&self, _sender: &AnyObject) {
            if let Some(gallery) = self.ivars().flags_gallery.get() {
                gallery.refresh();
            }
        }

        #[unsafe(method(flagLayoutChanged:))]
        fn flag_layout_changed(&self, _sender: &AnyObject) {
            self.sync_flag_layout();
            self.refresh_flag_choices();
        }

        #[unsafe(method(chooseCountryFlag:))]
        fn choose_country_flag(&self, sender: &NSButton) {
            let Some(gallery) = self.ivars().flags_gallery.get() else { return };
            let Some(source) = gallery.selected_layout_id() else { return };
            let Some(code) = usize::try_from(sender.tag()).ok().and_then(|index| gallery.code(index)) else { return };
            self.ivars().flag_choices.borrow_mut().draft.insert(source.to_owned(), code.to_owned());
            self.sync_flag_layout();
            self.refresh_flag_choices();
            self.set_status(true, tr("Flag preview updated. Choose Save Flags to apply your choices."));
        }

        #[unsafe(method(useAutomaticFlag:))]
        fn use_automatic_flag(&self, _sender: &AnyObject) {
            let Some(gallery) = self.ivars().flags_gallery.get() else { return };
            let Some(source) = gallery.selected_layout_id() else { return };
            self.ivars().flag_choices.borrow_mut().draft.remove(source);
            self.sync_flag_layout();
            self.refresh_flag_choices();
        }

        #[unsafe(method(revertCountryFlags:))]
        fn revert_country_flags(&self, _sender: &AnyObject) {
            self.ivars().flag_choices.borrow_mut().revert();
            self.refresh_flag_choices();
            self.set_status(true, tr("Unsaved flag changes reverted."));
        }

        #[unsafe(method(saveCountryFlags:))]
        fn save_country_flags(&self, _sender: &AnyObject) {
            let result = self.persist_flag_choices();
            if let Some(gallery) = self.ivars().flags_gallery.get() {
                match result {
                    Ok(()) => {
                        gallery.set_status(true, tr("Flags saved."));
                        self.set_status(true, tr("Flags saved. The running Upyr process will reload them."));
                    }
                    Err(error) => gallery.set_status(false, &crate::settings::i18n::localized_format!("Could not save flags: {error:#}", "Не вдалося зберегти прапорці: {error:#}")),
                }
            }
        }

        #[unsafe(method(previewAutoCorrectSound:))]
        fn preview_auto_correct_sound(&self, _sender: &AnyObject) {
            self.preview_sound(SoundEvent::AutoCorrect);
        }

        #[unsafe(method(previewManualConversionSound:))]
        fn preview_manual_conversion_sound(&self, _sender: &AnyObject) {
            self.preview_sound(SoundEvent::ManualConversion);
        }

        #[unsafe(method(previewLayoutSwitchSound:))]
        fn preview_layout_switch_sound(&self, _sender: &AnyObject) {
            self.preview_sound(SoundEvent::LayoutSwitch);
        }

        #[unsafe(method(previewPauseSound:))]
        fn preview_pause_sound(&self, _sender: &AnyObject) {
            self.preview_sound(SoundEvent::Pause);
        }

        #[unsafe(method(previewResumeSound:))]
        fn preview_resume_sound(&self, _sender: &AnyObject) {
            self.preview_sound(SoundEvent::Resume);
        }

        #[unsafe(method(previewErrorSound:))]
        fn preview_error_sound(&self, _sender: &AnyObject) {
            self.preview_sound(SoundEvent::Error);
        }

        #[unsafe(method(previewKeyboardClick:))]
        fn preview_keyboard_click(&self, _sender: &AnyObject) {
            self.preview_key_sound(KeyCue::Character, "typing click");
        }

        #[unsafe(method(previewControlReaction:))]
        fn preview_control_reaction(&self, _sender: &AnyObject) {
            self.preview_key_sound(KeyCue::Space, "control-key reaction");
        }

        #[unsafe(method(soundVolumeChanged:))]
        fn sound_volume_changed(&self, _sender: &AnyObject) {
            self.refresh_sound_volume_label();
            self.refresh_audio_output_status();
        }
        #[unsafe(method(headphoneVolumeChanged:))]
        fn headphone_volume_changed(&self, _sender: &AnyObject) {
            crate::feedback::stop_rerouted_audio(u64::MAX);
            self.refresh_headphone_volume_label();
        }
        #[unsafe(method(resetUndoShortcut:))]
        fn reset_undo_shortcut(&self, _sender: &AnyObject) {
            self.controls().undo_shortcut.set_value(&Config::default().undo_hotkey);
            self.validate_shortcuts();
        }

        #[unsafe(method(hideFeedback:))]
        fn hide_feedback(&self, _sender: Option<&AnyObject>) {
            crate::feedback::hide_layout_indicator();
        }

        #[unsafe(method(addException:))]
        fn add_exception(&self, _sender: &AnyObject) {
            let input = self.controls().exceptions_new_word.stringValue().to_string();
            let mut existing: std::collections::HashSet<String> = self
                .exception_words()
                .into_iter()
                .map(|word| word.to_lowercase())
                .collect();
            for word in parse_exceptions(&input) {
                if existing.insert(word.to_lowercase()) {
                    self.push_exception_row(&word);
                }
            }
            self.controls()
                .exceptions_new_word
                .setStringValue(&NSString::from_str(""));
        }

        #[unsafe(method(removeException:))]
        fn remove_exception(&self, sender: &AnyObject) {
            let tag: isize = unsafe { msg_send![sender, tag] };
            self.remove_exception_row(tag);
        }

        #[unsafe(method(checkSettingsActivation:))]
        fn check_settings_activation(&self, _sender: Option<&AnyObject>) {
            if self.ivars().updates.borrow_mut().poll() {
                self.refresh_updates();
            }
            if self.ivars().audio_poll.get().elapsed() >= std::time::Duration::from_secs(1) {
                self.ivars().audio_poll.set(std::time::Instant::now());
                self.refresh_audio_output_status();
            }
            match self.ivars().activation.drain() {
                Ok(Some(tab)) => self.activate_tab(tab),
                Ok(None) => {}
                Err(error) => self.set_status(
                    false,
                    &crate::settings::i18n::localized_format!("Could not activate the settings window: {error:#}", "Не вдалося активувати вікно налаштувань: {error:#}"),
                ),
            }
            self.schedule_activation_poll();
        }

        #[unsafe(method(checkForUpdates:))]
        fn check_for_updates(&self, _sender: &AnyObject) {
            self.ivars().updates.borrow_mut().start();
            self.refresh_updates();
        }

        #[unsafe(method(openUpdateRelease:))]
        fn open_update_release(&self, _sender: &AnyObject) {
            if let Some(url) = self.ivars().updates.borrow().state.release_url() {
                self.open_external_url(url);
            }
        }

    }
);

impl NativeController {
    fn refresh_updates(&self) {
        let updates = self.ivars().updates.borrow();
        self.controls()
            .updates
            .check
            .setEnabled(!updates.is_checking());
        self.controls()
            .updates
            .open
            .setHidden(updates.state.release_url().is_none());
        set_text(&self.controls().updates.status, updates.state.message());
    }

    fn open_external_url(&self, url: &str) {
        match Command::new("open").arg(url).spawn() {
            Ok(child) => {
                crate::child_processes::track(child);
                self.set_status(true, tr("Opened in your default browser."));
            }
            Err(error) => self.set_status(
                false,
                &crate::settings::i18n::localized_format!(
                    "Could not open the link: {error}",
                    "Не вдалося відкрити посилання: {error}"
                ),
            ),
        }
    }

    fn preview_sound(&self, event: SoundEvent) {
        let mut settings = self.ivars().config.borrow().sounds;
        settings.enabled = true;
        settings.pack = self.selected_sound_pack();
        settings.set_event_selected(event, true);
        settings.volume_percent = self.sound_volume_percent();
        settings.headphone_volume_percent = self.headphone_volume_percent();
        let globe = self.ivars().config.borrow().globe_switch.clone();
        match crate::feedback::preview_sound(event, &settings, &globe) {
            Ok(()) => self.set_status(
                true,
                &crate::settings::i18n::localized_format!(
                    "Previewed {} sound.",
                    "Відтворено звук «{}».",
                    event.label()
                ),
            ),
            Err(error) => self.set_status(
                false,
                &crate::settings::i18n::localized_format!(
                    "Could not preview {} sound: {error:#}",
                    "Не вдалося відтворити звук «{}»: {error:#}",
                    event.label()
                ),
            ),
        }
    }

    fn preview_key_sound(&self, cue: KeyCue, label: &str) {
        let label = tr(label);
        let mut settings = self.ivars().config.borrow().sounds;
        settings.enabled = true;
        settings.key_clicks = true;
        settings.pack = self.selected_sound_pack();
        settings.volume_percent = self.sound_volume_percent();
        settings.headphone_volume_percent = self.headphone_volume_percent();
        match crate::feedback::preview_key_sound(cue, &settings) {
            Ok(()) => self.set_status(
                true,
                &crate::settings::i18n::localized_format!(
                    "Previewed {label}.",
                    "Відтворено: {label}."
                ),
            ),
            Err(error) => {
                self.set_status(
                    false,
                    &crate::settings::i18n::localized_format!(
                        "Could not preview {label}: {error:#}",
                        "Не вдалося відтворити {label}: {error:#}"
                    ),
                );
            }
        }
    }

    fn selected_sound_pack(&self) -> SoundPack {
        match self.controls().sound_pack.indexOfSelectedItem() {
            1 => SoundPack::Arcade,
            2 => SoundPack::Anime,
            _ => SoundPack::Original,
        }
    }

    fn selected_indicator_style(&self) -> IndicatorStyle {
        match self.controls().indicator_style.indexOfSelectedItem() {
            1 => IndicatorStyle::Flag,
            2 => IndicatorStyle::Both,
            _ => IndicatorStyle::Letters,
        }
    }

    /// Reads every exception row's current text, splitting a row edited into
    /// several words, and dropping blanks and case-insensitive duplicates a
    /// live edit may have created.
    fn exception_words(&self) -> Vec<String> {
        normalize_exceptions(
            self.controls()
                .exceptions_rows
                .borrow()
                .iter()
                .map(|row| row.field.stringValue().to_string()),
        )
    }

    /// Replaces every exception row with fresh ones built from `words`, used
    /// when a config is loaded, reset, or applied to the form.
    fn rebuild_exception_rows(&self, words: &[String]) {
        {
            let mut rows = self.controls().exceptions_rows.borrow_mut();
            for row in rows.drain(..) {
                row.field.removeFromSuperview();
                row.remove.removeFromSuperview();
            }
        }
        for word in words {
            self.push_exception_row(word);
        }
    }

    fn push_exception_row(&self, word: &str) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let controls = self.controls();
        let index = controls.exceptions_rows.borrow().len();
        let field = text_field(
            word,
            rect(0.0, 0.0, EXCEPTION_FIELD_WIDTH, EXCEPTION_ROW_HEIGHT),
            mtm,
        );
        let remove = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(tr("Remove")),
                Some(self),
                Some(sel!(removeException:)),
                mtm,
            )
        };
        remove.setFrame(rect(0.0, 0.0, EXCEPTION_REMOVE_WIDTH, EXCEPTION_ROW_HEIGHT));
        remove.setTag(index as isize);
        controls.exceptions_container.addSubview(&field);
        controls.exceptions_container.addSubview(&remove);
        controls
            .exceptions_rows
            .borrow_mut()
            .push(ExceptionRow { field, remove });
        self.layout_exception_rows();
    }

    fn remove_exception_row(&self, tag: isize) {
        let Ok(index) = usize::try_from(tag) else {
            return;
        };
        let removed = {
            let mut rows = self.controls().exceptions_rows.borrow_mut();
            if index >= rows.len() {
                return;
            }
            rows.remove(index)
        };
        removed.field.removeFromSuperview();
        removed.remove.removeFromSuperview();
        for (new_index, row) in self.controls().exceptions_rows.borrow().iter().enumerate() {
            row.remove.setTag(new_index as isize);
        }
        self.layout_exception_rows();
    }

    /// Repositions every row top-to-bottom and grows the scrollable container
    /// to fit them, so the exceptions list is never silently truncated.
    fn layout_exception_rows(&self) {
        let controls = self.controls();
        let rows = controls.exceptions_rows.borrow();
        let container_height = (EXCEPTION_ROW_TOP_PADDING * 2.0
            + rows.len() as f64 * EXCEPTION_ROW_STEP)
            .max(EXCEPTION_LIST_VISIBLE_HEIGHT);
        controls
            .exceptions_container
            .setFrameSize(NSSize::new(EXCEPTION_LIST_WIDTH, container_height));
        for (index, row) in rows.iter().enumerate() {
            let top =
                container_height - EXCEPTION_ROW_TOP_PADDING - index as f64 * EXCEPTION_ROW_STEP;
            let field_y = top - EXCEPTION_ROW_HEIGHT;
            row.field.setFrame(rect(
                8.0,
                field_y,
                EXCEPTION_FIELD_WIDTH,
                EXCEPTION_ROW_HEIGHT,
            ));
            row.remove.setFrame(rect(
                8.0 + EXCEPTION_FIELD_WIDTH + 10.0,
                field_y,
                EXCEPTION_REMOVE_WIDTH,
                EXCEPTION_ROW_HEIGHT,
            ));
        }
    }

    fn headphone_volume_percent(&self) -> u8 {
        self.controls()
            .headphone_volume
            .doubleValue()
            .round()
            .clamp(0.0, 30.0) as u8
    }
    fn refresh_headphone_volume_label(&self) {
        set_text(
            &self.controls().headphone_volume_label,
            format!("{}%", self.headphone_volume_percent()),
        );
        self.refresh_audio_output_status();
    }
    fn refresh_audio_output_status(&self) {
        let route = crate::audio_output::current();
        let mut settings = self.ivars().config.borrow().sounds;
        settings.volume_percent = self.sound_volume_percent();
        settings.headphone_volume_percent = self.headphone_volume_percent();
        set_text(
            &self.controls().audio_output_status,
            format!(
                "{} · {} · Upyr {}%",
                route.name,
                tr(route.kind.label()),
                route.kind.volume(&settings)
            ),
        );
    }

    fn sound_volume_percent(&self) -> u8 {
        self.controls()
            .sound_volume
            .doubleValue()
            .round()
            .clamp(0.0, 100.0) as u8
    }

    fn refresh_sound_volume_label(&self) {
        let volume = self.sound_volume_percent();
        self.controls()
            .sound_volume
            .setDoubleValue(f64::from(volume));
        set_text(&self.controls().sound_volume_label, format!("{volume}%"));
    }

    fn new(
        config: Config,
        autostart_status: autostart::AutostartStatus,
        activation: ActivationInbox,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let flag_choices = FlagChoices::new(config.globe_switch.layout_flags.clone());
        let this = Self::alloc(mtm).set_ivars(ControllerIvars {
            updates: RefCell::new(super::updates::UpdateCheck::default()),
            config: RefCell::new(config),
            autostart_status: RefCell::new(autostart_status),
            activation,
            controls: OnceCell::new(),
            flags_gallery: OnceCell::new(),
            audio_poll: Cell::new(std::time::Instant::now()),
            flag_choices: RefCell::new(flag_choices),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn activate_tab(&self, tab: SettingsTab) {
        self.controls().tabs.selectTabViewItemAtIndex(tab.index());
        self.controls().window.deminiaturize(None);
        self.controls().window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        NSApplication::sharedApplication(self.mtm()).activateIgnoringOtherApps(true);
    }

    fn schedule_activation_poll(&self) {
        unsafe {
            let _: () = msg_send![
                self,
                performSelector: sel!(checkSettingsActivation:),
                withObject: None::<&AnyObject>,
                afterDelay: ACTIVATION_POLL_INTERVAL.as_secs_f64()
            ];
        }
    }

    fn controls(&self) -> &Controls {
        self.ivars()
            .controls
            .get()
            .expect("native settings controls must be initialized")
    }

    fn build_window(&self) -> Retained<NSWindow> {
        let mtm = self.mtm();
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, WINDOW_WIDTH, WINDOW_HEIGHT),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(tr("Upyr Settings")));
        window.center();
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        let content = window
            .contentView()
            .expect("native settings window must have a content view");

        let title = label(tr("Upyr Settings"), rect(24.0, 632.0, 280.0, 30.0), mtm);
        title.setFont(Some(&NSFont::boldSystemFontOfSize(22.0)));
        content.addSubview(&title);
        content.addSubview(&label(
            tr("English ↔ Ukrainian keyboard layout correction"),
            rect(24.0, 610.0, 330.0, 20.0),
            mtm,
        ));

        let search = NSSearchField::initWithFrame(
            NSSearchField::alloc(mtm),
            rect(420.0, 624.0, 276.0, 30.0),
        );
        search.setPlaceholderString(Some(&NSString::from_str(tr("Search settings"))));
        search.setSendsSearchStringImmediately(true);
        unsafe {
            search.setTarget(Some(self));
            search.setAction(Some(sel!(searchSettings:)));
        }
        content.addSubview(&search);

        let tabs = NSTabView::initWithFrame(NSTabView::alloc(mtm), rect(18.0, 106.0, 684.0, 492.0));
        let (
            general,
            ui_language,
            direction,
            switch_layout,
            globe_switch,
            restore_clipboard,
            login,
            repair_autostart,
            remove_autostart,
        ) = make_general_page(self, mtm);
        add_tab(&tabs, SettingsTab::General, &general, mtm);
        let AutomaticControls {
            page: automatic,
            enabled: auto_correct,
            sensitivity,
            minimum: minimum_word_length,
            delay: auto_delay,
            terminal_correction,
            slash_commands,
        } = make_automatic_page(mtm);
        add_tab(&tabs, SettingsTab::Automatic, &automatic, mtm);
        let ExceptionsControls {
            page: exceptions_page,
            container: exceptions_container,
            new_word: exceptions_new_word,
        } = make_exceptions_page(self, mtm);
        add_tab(&tabs, SettingsTab::Exceptions, &exceptions_page, mtm);
        let (
            shortcuts,
            selection_shortcut,
            previous_word_shortcut,
            undo_shortcut,
            modifier_gesture,
            gesture_action,
            gesture_timeout,
        ) = make_shortcuts_page(self, mtm);
        add_tab(&tabs, SettingsTab::Shortcuts, &shortcuts, mtm);
        let FeedbackControls {
            page: feedback,
            show_indicator,
            indicator_duration,
            indicator_style,
            flags_preview,
            preview_layout,
            preview_sources,
            sounds_enabled,
            sound_pack,
            key_clicks,
            sound_volume,
            sound_volume_label,
            headphone_volume,
            headphone_volume_label,
            audio_output_status,
            sound_events,
        } = make_feedback_page(self, mtm);
        add_tab(&tabs, SettingsTab::Feedback, &feedback, mtm);
        let (advanced, copy_delay, paste_delay, restore_delay) = make_advanced_page(mtm);
        add_tab(&tabs, SettingsTab::Advanced, &advanced, mtm);
        let (about, updates) = make_about_page(self, mtm);
        add_tab(&tabs, SettingsTab::About, &about, mtm);
        content.addSubview(&tabs);

        let status = label("", rect(24.0, 60.0, 672.0, 40.0), mtm);
        status.setMaximumNumberOfLines(2);
        status.setUsesSingleLineMode(false);
        content.addSubview(&status);
        content.addSubview(&action_button(
            tr("Save"),
            rect(504.0, 24.0, 92.0, 32.0),
            self,
            sel!(saveSettings:),
            mtm,
        ));
        content.addSubview(&action_button(
            tr("Reset All"),
            rect(400.0, 24.0, 96.0, 32.0),
            self,
            sel!(resetSettings:),
            mtm,
        ));
        content.addSubview(&action_button(
            tr("Close"),
            rect(604.0, 24.0, 92.0, 32.0),
            self,
            sel!(closeSettings:),
            mtm,
        ));

        self.ivars()
            .controls
            .set(Controls {
                window: window.clone(),
                tabs,
                search,
                status,
                ui_language,
                direction,
                switch_layout,
                globe_switch,
                restore_clipboard,
                launch_at_login: login,
                repair_autostart,
                remove_autostart,
                auto_correct,
                terminal_correction,
                slash_commands,
                sensitivity,
                minimum_word_length,
                auto_delay,
                exceptions_container,
                exceptions_rows: RefCell::new(Vec::new()),
                exceptions_new_word,
                selection_shortcut,
                previous_word_shortcut,
                undo_shortcut,
                modifier_gesture,
                gesture_action,
                gesture_timeout,
                show_indicator,
                indicator_duration,
                indicator_style,
                flags_preview,
                preview_layout,
                preview_sources,
                sounds_enabled,
                sound_pack,
                key_clicks,
                sound_volume,
                sound_volume_label,
                headphone_volume,
                headphone_volume_label,
                audio_output_status,
                sound_events,
                copy_delay,
                paste_delay,
                restore_delay,
                updates,
            })
            .ok()
            .expect("native settings controls must only be initialized once");
        let config = self.ivars().config.borrow().clone();
        self.apply_config(&config);
        let autostart_status = self.ivars().autostart_status.borrow().clone();
        self.set_autostart_status(autostart_status);
        self.show_autostart_attention("");
        window
    }

    fn collect_config(&self) -> Result<Config> {
        let controls = self.controls();
        if let Some(error) = controls.selection_shortcut.error() {
            return Err(anyhow!("convert-selection shortcut: {error}"));
        }
        if let Some(error) = controls.previous_word_shortcut.error() {
            return Err(anyhow!("previous-word shortcut: {error}"));
        }
        let mut config = self.ivars().config.borrow().clone();
        config.ui_language = if controls.ui_language.indexOfSelectedItem() == 1 {
            crate::config::UiLanguage::English
        } else {
            crate::config::UiLanguage::Ukrainian
        };
        config.hotkey = controls.selection_shortcut.value();
        config.last_word_hotkey = controls.previous_word_shortcut.value();
        config.undo_hotkey = controls.undo_shortcut.value();
        if let Some(error) = controls.undo_shortcut.error() {
            return Err(anyhow!("undo shortcut: {error}"));
        }
        config.direction = match controls.direction.indexOfSelectedItem() {
            1 => Direction::EnglishToUkrainian,
            2 => Direction::UkrainianToEnglish,
            _ => Direction::Smart,
        };
        config.switch_layout = is_checked(&controls.switch_layout);
        config.globe_switch.enabled = is_checked(&controls.globe_switch);
        config.globe_switch.layout_flags = Config::load()?.globe_switch.layout_flags;
        self.ivars()
            .flag_choices
            .borrow()
            .apply_to(&mut config.globe_switch.layout_flags);
        config.restore_clipboard = is_checked(&controls.restore_clipboard);
        config.auto_correct = is_checked(&controls.auto_correct);
        set_terminal_correction(&mut config, is_checked(&controls.terminal_correction));
        set_superset_slash_commands(&mut config, is_checked(&controls.slash_commands));
        config.auto_correct_sensitivity = match controls.sensitivity.indexOfSelectedItem() {
            1 => AutoCorrectSensitivity::Balanced,
            2 => AutoCorrectSensitivity::Aggressive,
            _ => AutoCorrectSensitivity::Conservative,
        };
        config.auto_correct_min_word_length =
            parse_number(&controls.minimum_word_length, "minimum word length", 2, 32)? as usize;
        config.auto_correct_delay_ms =
            parse_number(&controls.auto_delay, "automatic correction delay", 10, 250)?;
        config.auto_correct_exceptions = self.exception_words();
        config.modifier_gesture = match controls.modifier_gesture.indexOfSelectedItem() {
            1 => ModifierGesture::DoubleControl,
            2 => ModifierGesture::DoubleShift,
            3 => ModifierGesture::DoubleControlShift,
            _ => ModifierGesture::Disabled,
        };
        config.modifier_gesture_action = match controls.gesture_action.indexOfSelectedItem() {
            1 => GestureAction::Selection,
            _ => GestureAction::PreviousWord,
        };
        config.modifier_gesture_timeout_ms =
            parse_number(&controls.gesture_timeout, "gesture timeout", 150, 2_000)?;
        config.show_layout_indicator = is_checked(&controls.show_indicator);
        config.layout_indicator_duration_ms = parse_number(
            &controls.indicator_duration,
            "indicator duration",
            250,
            3_000,
        )?;
        config.indicator_style = self.selected_indicator_style();
        config.sounds.enabled = is_checked(&controls.sounds_enabled);
        config.sounds.volume_percent = self.sound_volume_percent();
        config.sounds.headphone_volume_percent = self.headphone_volume_percent();
        config.sounds.pack = self.selected_sound_pack();
        config.sounds.key_clicks = is_checked(&controls.key_clicks);
        for event in SoundEvent::ALL {
            config
                .sounds
                .set_event_selected(event, is_checked(controls.sound_events.button(event)));
        }
        config.copy_delay_ms = parse_number(&controls.copy_delay, "copy delay", 10, 2_000)?;
        config.paste_delay_ms = parse_number(&controls.paste_delay, "paste delay", 0, 2_000)?;
        config.restore_delay_ms =
            parse_number(&controls.restore_delay, "clipboard restore delay", 0, 5_000)?;
        config.validate()?;
        Ok(config)
    }

    fn apply_config(&self, config: &Config) {
        self.ivars()
            .flag_choices
            .borrow_mut()
            .draft
            .clone_from(&config.globe_switch.layout_flags);
        let controls = self.controls();
        controls.ui_language.selectItemAtIndex(
            if config.ui_language == crate::config::UiLanguage::English {
                1
            } else {
                0
            },
        );
        controls
            .direction
            .selectItemAtIndex(match config.direction {
                Direction::Smart => 0,
                Direction::EnglishToUkrainian => 1,
                Direction::UkrainianToEnglish => 2,
            });
        set_checked(&controls.switch_layout, config.switch_layout);
        set_checked(&controls.globe_switch, config.globe_switch.enabled);
        set_checked(&controls.restore_clipboard, config.restore_clipboard);
        set_checked(&controls.auto_correct, config.auto_correct);
        set_checked(
            &controls.terminal_correction,
            terminal_correction_enabled(config),
        );
        set_checked(
            &controls.slash_commands,
            superset_slash_commands_enabled(config),
        );
        controls
            .sensitivity
            .selectItemAtIndex(match config.auto_correct_sensitivity {
                AutoCorrectSensitivity::Conservative => 0,
                AutoCorrectSensitivity::Balanced => 1,
                AutoCorrectSensitivity::Aggressive => 2,
            });
        set_text(
            &controls.minimum_word_length,
            config.auto_correct_min_word_length,
        );
        set_text(&controls.auto_delay, config.auto_correct_delay_ms);
        self.rebuild_exception_rows(&config.auto_correct_exceptions);
        controls.selection_shortcut.set_value(&config.hotkey);
        controls
            .previous_word_shortcut
            .set_value(&config.last_word_hotkey);
        controls.undo_shortcut.set_value(&config.undo_hotkey);
        controls
            .modifier_gesture
            .selectItemAtIndex(match config.modifier_gesture {
                ModifierGesture::Disabled => 0,
                ModifierGesture::DoubleControl => 1,
                ModifierGesture::DoubleShift => 2,
                ModifierGesture::DoubleControlShift => 3,
            });
        controls
            .gesture_action
            .selectItemAtIndex(match config.modifier_gesture_action {
                GestureAction::PreviousWord => 0,
                GestureAction::Selection => 1,
            });
        set_text(
            &controls.gesture_timeout,
            config.modifier_gesture_timeout_ms,
        );
        set_checked(&controls.show_indicator, config.show_layout_indicator);
        set_text(
            &controls.indicator_duration,
            config.layout_indicator_duration_ms,
        );
        controls
            .indicator_style
            .selectItemAtIndex(match config.indicator_style {
                IndicatorStyle::Letters => 0,
                IndicatorStyle::Flag => 1,
                IndicatorStyle::Both => 2,
            });
        self.refresh_flag_choices();
        set_checked(&controls.sounds_enabled, config.sounds.enabled);
        controls
            .sound_pack
            .selectItemAtIndex(match config.sounds.pack {
                SoundPack::Original => 0,
                SoundPack::Arcade => 1,
                SoundPack::Anime => 2,
            });
        set_checked(&controls.key_clicks, config.sounds.key_clicks);
        controls
            .sound_volume
            .setDoubleValue(f64::from(config.sounds.volume_percent));
        controls
            .headphone_volume
            .setDoubleValue(f64::from(config.sounds.headphone_volume_percent));
        self.refresh_headphone_volume_label();
        self.refresh_sound_volume_label();
        for event in SoundEvent::ALL {
            set_checked(
                controls.sound_events.button(event),
                config.sounds.event_selected(event),
            );
        }
        set_text(&controls.copy_delay, config.copy_delay_ms);
        set_text(&controls.paste_delay, config.paste_delay_ms);
        set_text(&controls.restore_delay, config.restore_delay_ms);
    }

    fn selected_preview_source(&self) -> Option<&crate::system_layout::ActiveInputSource> {
        usize::try_from(self.controls().preview_layout.indexOfSelectedItem())
            .ok()
            .and_then(|index| self.controls().preview_sources.get(index))
    }

    fn flag_preview_config(&self) -> Config {
        let mut config = self.ivars().config.borrow().clone();
        config.indicator_style = self.selected_indicator_style();
        config
            .globe_switch
            .layout_flags
            .clone_from(&self.ivars().flag_choices.borrow().draft);
        config
    }

    fn sync_flag_layout(&self) {
        let source = self
            .ivars()
            .flags_gallery
            .get()
            .and_then(|gallery| gallery.selected_layout_id());
        if let Some(index) = source.and_then(|id| {
            self.controls()
                .preview_sources
                .iter()
                .position(|source| source.source_id == id)
        }) {
            self.controls()
                .preview_layout
                .selectItemAtIndex(index as isize);
        }
    }

    fn refresh_flag_choices(&self) {
        self.refresh_flag_preview();
        if let Some(gallery) = self.ivars().flags_gallery.get() {
            gallery.update(
                &self.flag_preview_config(),
                self.ivars().flag_choices.borrow().dirty(),
            );
        }
    }

    fn persist_flag_choices(&self) -> Result<()> {
        let mut config = Config::load()?;
        self.ivars()
            .flag_choices
            .borrow()
            .apply_to(&mut config.globe_switch.layout_flags);
        config.write(true)?;
        self.ivars()
            .flag_choices
            .borrow_mut()
            .accept(config.globe_switch.layout_flags.clone());
        self.ivars().config.replace(config);
        self.refresh_flag_choices();
        Ok(())
    }

    fn refresh_flag_preview(&self) {
        let container = &self.controls().flags_preview;
        for child in container.subviews().to_vec() {
            child.removeFromSuperview();
        }
        let config = self.flag_preview_config();
        if config.indicator_style == IndicatorStyle::Flag {
            let view = crate::feedback::globe_indicator::preview(
                &config,
                self.selected_preview_source().map(|s| s.source_id.as_str()),
                container.bounds(),
                self.mtm(),
            );
            container.addSubview(&view);
        } else {
            let layout = self
                .selected_preview_source()
                .and_then(|source| source.layout);
            let text = layout
                .map(|layout| crate::feedback::indicator_text(layout, config.indicator_style))
                .unwrap_or(tr("Layout preview"));
            let label = label(text, rect(260.0, 32.0, 220.0, 36.0), self.mtm());
            label.setFont(Some(&NSFont::boldSystemFontOfSize(24.0)));
            container.addSubview(&label);
        }
    }

    fn validate_shortcuts(&self) {
        let controls = self.controls();
        let recorders = [
            &controls.selection_shortcut,
            &controls.previous_word_shortcut,
            &controls.undo_shortcut,
        ];
        if recorders.iter().any(|recorder| recorder.error().is_some()) {
            self.set_status(
                false,
                tr("A shortcut is incomplete. Record a key together with a modifier."),
            );
        } else {
            let values = recorders.map(|recorder| recorder.value());
            let duplicates = (0..values.len()).any(|i| {
                values[i + 1..]
                    .iter()
                    .any(|value| shortcuts_equal(&values[i], value))
            });
            self.set_status(
                !duplicates,
                if duplicates {
                    tr("Actions must use different shortcuts.")
                } else {
                    tr("Shortcuts are valid.")
                },
            );
        }
    }

    fn set_autostart_status(&self, status: autostart::AutostartStatus) {
        let controls = self.controls();
        let stale = status.state == autostart::AutostartState::Stale;
        let exceptional = matches!(
            status.state,
            autostart::AutostartState::Stale | autostart::AutostartState::Broken
        );
        set_checked(&controls.launch_at_login, status.enabled);
        controls.repair_autostart.setEnabled(stale);
        controls.repair_autostart.setHidden(!stale);
        controls.remove_autostart.setEnabled(exceptional);
        controls.remove_autostart.setHidden(!exceptional);
        self.ivars().autostart_status.replace(status);
    }

    fn show_autostart_attention(&self, prefix: &str) -> bool {
        let message = {
            let status = self.ivars().autostart_status.borrow();
            autostart_attention(&status).map(|attention| format!("{prefix}{attention}"))
        };
        if let Some(message) = message {
            self.set_status(false, &message);
            true
        } else {
            false
        }
    }

    fn set_status(&self, success: bool, message: &str) {
        let status = &self.controls().status;
        status.setStringValue(&NSString::from_str(&crate::settings::i18n::diagnostic(
            message,
        )));
        status.setToolTip(Some(&NSString::from_str(
            &crate::settings::i18n::diagnostic(message),
        )));
        let color = if success {
            NSColor::systemGreenColor()
        } else {
            NSColor::systemRedColor()
        };
        status.setTextColor(Some(&color));
    }
}

struct RecorderIvars {
    value: RefCell<String>,
    previous: RefCell<String>,
    recording: Cell<bool>,
    error: RefCell<Option<String>>,
    controller: Cell<*const NativeController>,
}

define_class!(
    #[unsafe(super = NSButton)]
    #[thread_kind = MainThreadOnly]
    #[ivars = RecorderIvars]
    struct ShortcutRecorder;

    unsafe impl NSObjectProtocol for ShortcutRecorder {}

    impl ShortcutRecorder {
        #[unsafe(method(beginRecording:))]
        fn begin_recording(&self, _sender: &AnyObject) {
            self.ivars().previous.replace(self.value());
            self.ivars().recording.set(true);
            self.ivars().error.replace(None);
            self.setTitle(&NSString::from_str(tr("Press shortcut…  (Esc to cancel)")));
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self));
            }
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !self.ivars().recording.get() {
                unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                }
                return;
            }
            if event.keyCode() == 53 {
                let previous = self.ivars().previous.borrow().clone();
                self.set_value(&previous);
                return;
            }
            let Some(code) = mac_key_code(event.keyCode()) else {
                self.ivars()
                    .error
                    .replace(Some(tr("That key is not supported.").to_owned()));
                self.setTitle(&NSString::from_str(tr("Unsupported key — try another")));
                return;
            };
            let modifiers = mac_modifiers(event.modifierFlags());
            if modifiers.is_empty() {
                self.ivars().error.replace(Some(
                    tr("Include Command, Control, Option, or Shift.").to_owned(),
                ));
                self.setTitle(&NSString::from_str(tr("Add a modifier key")));
                return;
            }
            self.set_value(&HotKey::new(Some(modifiers), code).to_string());
            if let Some(window) = self.window() {
                window.makeFirstResponder(None);
            }
            let controller = self.ivars().controller.get();
            if !controller.is_null() {
                // The controller owns this recorder through `Controls`, so it
                // necessarily outlives every key event delivered to the view.
                unsafe { (&*controller).validate_shortcuts() };
            }
        }
    }
);

impl ShortcutRecorder {
    fn new(
        value: &str,
        frame: NSRect,
        controller: &NativeController,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RecorderIvars {
            value: RefCell::new(value.to_owned()),
            previous: RefCell::new(value.to_owned()),
            recording: Cell::new(false),
            error: RefCell::new(None),
            controller: Cell::new(controller as *const NativeController),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        unsafe {
            this.setTarget(Some(&this));
            this.setAction(Some(sel!(beginRecording:)));
        }
        this.refresh_title();
        this
    }

    fn value(&self) -> String {
        self.ivars().value.borrow().clone()
    }

    fn set_value(&self, value: &str) {
        self.ivars().value.replace(value.to_owned());
        self.ivars().previous.replace(value.to_owned());
        self.ivars().recording.set(false);
        self.ivars().error.replace(None);
        self.refresh_title();
    }

    fn error(&self) -> Option<String> {
        self.ivars().error.borrow().clone()
    }

    fn refresh_title(&self) {
        let value = self.value();
        self.setTitle(&NSString::from_str(&pretty_hotkey(&value).unwrap_or(value)));
    }
}

pub(super) fn run(
    config: Config,
    autostart_status: autostart::AutostartStatus,
    initial_tab: SettingsTab,
    activation: ActivationInbox,
    problem: Option<String>,
) -> Result<()> {
    super::i18n::set_language(config.ui_language);
    let mtm =
        MainThreadMarker::new().context("Upyr Settings must start on the macOS main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let controller = NativeController::new(config, autostart_status, activation, mtm);
    let window = controller.build_window();
    controller
        .controls()
        .tabs
        .selectTabViewItemAtIndex(initial_tab.index());
    if let Some(problem) = problem {
        controller.set_status(false, &problem);
    }
    window.makeKeyAndOrderFront(None);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    controller.schedule_activation_poll();
    app.run();
    Ok(())
}

#[allow(clippy::type_complexity)]
fn make_general_page(
    controller: &NativeController,
    mtm: MainThreadMarker,
) -> (
    Retained<NSView>,
    Retained<NSPopUpButton>,
    Retained<NSPopUpButton>,
    Retained<NSButton>,
    Retained<NSButton>,
    Retained<NSButton>,
    Retained<NSButton>,
    Retained<NSButton>,
    Retained<NSButton>,
) {
    let page = page(mtm);
    page_header(
        &page,
        tr("General"),
        tr("Core conversion, clipboard, and startup behavior."),
        mtm,
    );
    page.addSubview(&label(
        tr("Conversion direction"),
        rect(28.0, 330.0, 210.0, 24.0),
        mtm,
    ));
    let direction = popup(
        &[
            tr("Smart (detect script)"),
            tr("English → Ukrainian"),
            tr("Ukrainian → English"),
        ],
        rect(252.0, 326.0, 260.0, 28.0),
        mtm,
    );
    page.addSubview(&direction);
    let switch_layout = checkbox(
        tr("Switch the OS layout to match converted text"),
        rect(24.0, 272.0, 620.0, 26.0),
        mtm,
    );
    let restore = checkbox(
        tr("Restore clipboard contents after conversion"),
        rect(24.0, 228.0, 620.0, 26.0),
        mtm,
    );
    let login = checkbox(
        tr("Launch Upyr at login"),
        rect(24.0, 184.0, 430.0, 26.0),
        mtm,
    );
    let repair_autostart = action_button(
        tr("Repair Entry"),
        rect(24.0, 132.0, 126.0, 32.0),
        controller,
        sel!(repairAutostart:),
        mtm,
    );
    let remove_autostart = action_button(
        tr("Remove Entry"),
        rect(160.0, 132.0, 126.0, 32.0),
        controller,
        sel!(removeAutostart:),
        mtm,
    );
    repair_autostart.setEnabled(false);
    repair_autostart.setHidden(true);
    remove_autostart.setEnabled(false);
    remove_autostart.setHidden(true);
    let globe_switch = checkbox(
        tr("Tap Globe (🌐) to switch keyboard layouts instantly"),
        rect(24.0, 84.0, 590.0, 26.0),
        mtm,
    );
    page.addSubview(&globe_switch);
    page.addSubview(&label(
        tr("Keeps fn shortcuts. Restores macOS handling when paused or quit."),
        rect(28.0, 52.0, 600.0, 24.0),
        mtm,
    ));
    page.addSubview(&label(
        tr("Interface language"),
        rect(28.0, 12.0, 210.0, 24.0),
        mtm,
    ));
    let ui_language = popup(
        &["Українська", "English"],
        rect(252.0, 8.0, 260.0, 28.0),
        mtm,
    );
    ui_language.setToolTip(Some(&NSString::from_str(tr(
        "Save and reopen settings to apply the language.",
    ))));
    page.addSubview(&ui_language);
    page.addSubview(&switch_layout);
    page.addSubview(&restore);
    page.addSubview(&login);
    page.addSubview(&repair_autostart);
    page.addSubview(&remove_autostart);
    (
        page,
        ui_language,
        direction,
        switch_layout,
        globe_switch,
        restore,
        login,
        repair_autostart,
        remove_autostart,
    )
}

#[allow(clippy::type_complexity)]
fn make_automatic_page(mtm: MainThreadMarker) -> AutomaticControls {
    let page = page(mtm);
    page_header(&page, CORRECTION_TITLE, CORRECTION_DESCRIPTION, mtm);
    let enabled = checkbox(CORRECTION_TOGGLE, rect(24.0, 346.0, 610.0, 26.0), mtm);
    page.addSubview(&enabled);
    page.addSubview(&multiline_label(
        MIDWORD_HELP,
        rect(28.0, 292.0, 610.0, 42.0),
        mtm,
    ));
    page.addSubview(&multiline_label(
        CONTEXT_HELP,
        rect(28.0, 248.0, 610.0, 38.0),
        mtm,
    ));
    page.addSubview(&label(
        tr("Sensitivity"),
        rect(28.0, 210.0, 180.0, 24.0),
        mtm,
    ));
    let sensitivity = popup(
        &[
            sensitivity_label(AutoCorrectSensitivity::Conservative),
            sensitivity_label(AutoCorrectSensitivity::Balanced),
            sensitivity_label(AutoCorrectSensitivity::Aggressive),
        ],
        rect(252.0, 206.0, 220.0, 28.0),
        mtm,
    );
    page.addSubview(&sensitivity);
    let minimum = numeric_row(&page, tr("Minimum word length"), 170.0, mtm);
    let delay = numeric_row(&page, tr("Pause after Space (ms)"), 132.0, mtm);
    delay.setToolTip(Some(&NSString::from_str(tr(
        "This delay applies after Space; a clear prefix can switch immediately during typing.",
    ))));
    let terminal_correction = checkbox(
        tr("Allow correction in terminals"),
        rect(24.0, 94.0, 610.0, 26.0),
        mtm,
    );
    terminal_correction.setToolTip(Some(&NSString::from_str(
        tr("Opt in for Terminal, iTerm2, Ghostty, kitty and Warp. Upyr holds the next keys while replacing the tracked text; it never sends Enter."),
    )));
    page.addSubview(&terminal_correction);
    let slash_commands = checkbox(
        tr("Correct freshly typed /commands in Superset"),
        rect(24.0, 62.0, 610.0, 26.0),
        mtm,
    );
    page.addSubview(&slash_commands);
    page.addSubview(&multiline_label(
        CORRECTION_FOOTNOTE,
        rect(28.0, 14.0, 610.0, 38.0),
        mtm,
    ));
    AutomaticControls {
        page,
        enabled,
        sensitivity,
        minimum,
        delay,
        terminal_correction,
        slash_commands,
    }
}

struct AutomaticControls {
    page: Retained<NSView>,
    enabled: Retained<NSButton>,
    sensitivity: Retained<NSPopUpButton>,
    minimum: Retained<NSTextField>,
    delay: Retained<NSTextField>,
    terminal_correction: Retained<NSButton>,
    slash_commands: Retained<NSButton>,
}

fn make_exceptions_page(
    controller: &NativeController,
    mtm: MainThreadMarker,
) -> ExceptionsControls {
    let page = page(mtm);
    page_header(
        &page,
        tr("Exceptions"),
        tr("Exact words that automatic correction must always leave alone."),
        mtm,
    );

    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        rect(24.0, 100.0, 624.0, EXCEPTION_LIST_VISIBLE_HEIGHT),
    );
    scroll.setHasVerticalScroller(true);
    scroll.setDrawsBackground(true);
    let container = NSView::initWithFrame(
        NSView::alloc(mtm),
        rect(
            0.0,
            0.0,
            EXCEPTION_LIST_WIDTH,
            EXCEPTION_LIST_VISIBLE_HEIGHT,
        ),
    );
    scroll.setDocumentView(Some(&container));
    page.addSubview(&scroll);

    let new_word = text_field("", rect(24.0, 66.0, 470.0, 26.0), mtm);
    new_word.setPlaceholderString(Some(&NSString::from_str(tr(
        "words separated by spaces or commas",
    ))));
    page.addSubview(&new_word);
    page.addSubview(&action_button(
        tr("Add"),
        rect(504.0, 64.0, 96.0, 30.0),
        controller,
        sel!(addException:),
        mtm,
    ));
    page.addSubview(&multiline_label(
        tr("Spaces and commas separate words, so \"two words\" adds two exceptions. Case-insensitive duplicates and blank entries are ignored."),
        rect(24.0, 26.0, 624.0, 32.0),
        mtm,
    ));

    ExceptionsControls {
        page,
        container,
        new_word,
    }
}

#[allow(clippy::type_complexity)]
fn make_shortcuts_page(
    controller: &NativeController,
    mtm: MainThreadMarker,
) -> (
    Retained<NSView>,
    Retained<ShortcutRecorder>,
    Retained<ShortcutRecorder>,
    Retained<ShortcutRecorder>,
    Retained<NSPopUpButton>,
    Retained<NSPopUpButton>,
    Retained<NSTextField>,
) {
    let page = page(mtm);
    page_header(
        &page,
        tr("Shortcuts"),
        tr(
            "Click a recorder, then press a physical key combination. Upyr actions are paused here.",
        ),
        mtm,
    );
    page.addSubview(&label(
        tr("Convert selection"),
        rect(28.0, 338.0, 180.0, 24.0),
        mtm,
    ));
    let selection = ShortcutRecorder::new(
        "CmdOrCtrl+Alt+Space",
        rect(210.0, 332.0, 310.0, 32.0),
        controller,
        mtm,
    );
    page.addSubview(&selection);
    page.addSubview(&action_button(
        tr("Reset"),
        rect(536.0, 332.0, 82.0, 32.0),
        controller,
        sel!(resetSelectionShortcut:),
        mtm,
    ));
    page.addSubview(&label(
        tr("Fix previous word"),
        rect(28.0, 290.0, 180.0, 24.0),
        mtm,
    ));
    let previous = ShortcutRecorder::new(
        "CmdOrCtrl+Alt+Backspace",
        rect(210.0, 284.0, 310.0, 32.0),
        controller,
        mtm,
    );
    page.addSubview(&previous);
    page.addSubview(&action_button(
        tr("Reset"),
        rect(536.0, 284.0, 82.0, 32.0),
        controller,
        sel!(resetPreviousShortcut:),
        mtm,
    ));
    page.addSubview(&label(
        tr("Undo auto-correction"),
        rect(28.0, 242.0, 180.0, 24.0),
        mtm,
    ));
    let undo = ShortcutRecorder::new(
        "CmdOrCtrl+Alt+Z",
        rect(210.0, 236.0, 310.0, 32.0),
        controller,
        mtm,
    );
    undo.setToolTip(Some(&NSString::from_str(tr(
        "Restore the last correction before more typing, clicks or cursor movement.",
    ))));
    page.addSubview(&undo);
    page.addSubview(&action_button(
        tr("Reset"),
        rect(536.0, 236.0, 82.0, 32.0),
        controller,
        sel!(resetUndoShortcut:),
        mtm,
    ));
    page.addSubview(&label(
        tr("Modifier gesture"),
        rect(28.0, 186.0, 180.0, 24.0),
        mtm,
    ));
    let gesture = popup(
        &[
            tr("Disabled"),
            tr("Double Control"),
            tr("Double Shift"),
            tr("Double Control + Shift"),
        ],
        rect(210.0, 182.0, 258.0, 28.0),
        mtm,
    );
    page.addSubview(&gesture);
    page.addSubview(&label(
        tr("Gesture action"),
        rect(28.0, 142.0, 180.0, 24.0),
        mtm,
    ));
    let action = popup(
        &[tr("Fix previous word"), tr("Convert selection")],
        rect(210.0, 138.0, 258.0, 28.0),
        mtm,
    );
    page.addSubview(&action);
    let timeout = numeric_row(&page, tr("Double-tap timeout (ms)"), 94.0, mtm);
    page.addSubview(&label(
        tr("Undo is available for 30 seconds, until further input or navigation."),
        rect(28.0, 46.0, 610.0, 24.0),
        mtm,
    ));
    (page, selection, previous, undo, gesture, action, timeout)
}

fn make_feedback_page(controller: &NativeController, mtm: MainThreadMarker) -> FeedbackControls {
    let container = page(mtm);
    let page = NSView::initWithFrame(
        NSView::alloc(mtm),
        rect(0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT + 210.0),
    );
    page_header(
        &page,
        tr("Feedback"),
        tr("Optional visual and sound confirmation for Upyr actions."),
        mtm,
    );
    let indicator = checkbox(
        tr("Show the layout indicator"),
        rect(24.0, 342.0, 470.0, 26.0),
        mtm,
    );
    page.addSubview(&indicator);
    page.addSubview(&action_button(
        tr("Preview Flag"),
        rect(532.0, 339.0, 112.0, 30.0),
        controller,
        sel!(previewFeedback:),
        mtm,
    ));
    page.addSubview(&label(
        tr("Indicator duration (ms)"),
        rect(48.0, 308.0, 190.0, 24.0),
        mtm,
    ));
    let duration = text_field("", rect(252.0, 305.0, 116.0, 26.0), mtm);
    page.addSubview(&duration);
    page.addSubview(&label(tr("Shows"), rect(388.0, 308.0, 46.0, 24.0), mtm));
    let indicator_style = popup(
        &[
            IndicatorStyle::Letters.label(),
            tr("GlobeSwitch flags"),
            IndicatorStyle::Both.label(),
        ],
        rect(436.0, 305.0, 168.0, 28.0),
        mtm,
    );
    page.addSubview(&indicator_style);

    unsafe {
        indicator_style.setTarget(Some(controller));
        indicator_style.setAction(Some(sel!(flagPreviewChanged:)));
    }
    // Make room for an embedded preview without compressing sound controls.
    for child in page.subviews().to_vec() {
        let mut frame = child.frame();
        frame.origin.y += 140.0;
        child.setFrame(frame);
    }
    page.addSubview(&label(
        tr("Preview layout"),
        rect(48.0, 408.0, 130.0, 24.0),
        mtm,
    ));
    let preview_sources: Vec<_> = crate::system_layout::indicator_sources()
        .unwrap_or_default()
        .into_iter()
        .map(|(source, _)| source)
        .collect();
    let names: Vec<_> = preview_sources
        .iter()
        .map(|s| s.source_name.as_str())
        .collect();
    let preview_layout = popup(&names, rect(180.0, 405.0, 260.0, 28.0), mtm);
    if let Ok(Some(current)) = crate::system_layout::current() {
        if let Some(index) = preview_sources
            .iter()
            .position(|s| s.source_id == current.source_id)
        {
            preview_layout.selectItemAtIndex(index as isize);
        }
    }
    unsafe {
        preview_layout.setTarget(Some(controller));
        preview_layout.setAction(Some(sel!(flagPreviewChanged:)));
    }
    page.addSubview(&preview_layout);
    page.addSubview(&action_button(
        tr("All Country Flags…"),
        rect(456.0, 405.0, 188.0, 30.0),
        controller,
        sel!(openCountryFlags:),
        mtm,
    ));
    let flags_preview = NSView::initWithFrame(NSView::alloc(mtm), rect(24.0, 302.0, 620.0, 100.0));
    page.addSubview(&flags_preview);

    let sounds_enabled = checkbox(tr("Enable sounds"), rect(24.0, 267.0, 145.0, 26.0), mtm);
    page.addSubview(&sounds_enabled);
    page.addSubview(&label(tr("Pack"), rect(172.0, 269.0, 42.0, 24.0), mtm));
    let sound_pack = popup(
        &[
            SoundPack::Original.label(),
            SoundPack::Arcade.label(),
            SoundPack::Anime.label(),
        ],
        rect(210.0, 264.0, 154.0, 28.0),
        mtm,
    );
    sound_pack.setToolTip(Some(&NSString::from_str(
        tr("Anime Reactions synthesizes nonverbal control-key sounds locally; its six event cues are pre-rendered clips bundled with Upyr."),
    )));
    page.addSubview(&sound_pack);
    page.addSubview(&label(tr("Volume"), rect(372.0, 269.0, 58.0, 24.0), mtm));
    let sound_volume = unsafe {
        NSSlider::sliderWithValue_minValue_maxValue_target_action(
            65.0,
            0.0,
            100.0,
            Some(controller),
            Some(sel!(soundVolumeChanged:)),
            mtm,
        )
    };
    sound_volume.setFrame(rect(426.0, 264.0, 154.0, 30.0));
    sound_volume.setContinuous(true);
    sound_volume.setNumberOfTickMarks(5);
    sound_volume.setAllowsTickMarkValuesOnly(false);
    sound_volume.setAltIncrementValue(5.0);
    sound_volume.setToolTip(Some(&NSString::from_str(tr(
        "Sound volume, 0 to 100 percent",
    ))));
    page.addSubview(&sound_volume);
    let sound_volume_label = label("65%", rect(588.0, 269.0, 56.0, 24.0), mtm);
    page.addSubview(&sound_volume_label);

    let key_clicks = checkbox(
        tr("Keyboard sounds (Anime adds control-key reactions)"),
        rect(42.0, 230.0, 366.0, 26.0),
        mtm,
    );
    key_clicks.setToolTip(Some(&NSString::from_str(tr(
        "Opt-in physical-key categories only. Typed characters are never stored.",
    ))));
    page.addSubview(&key_clicks);
    page.addSubview(&action_button(
        tr("Click"),
        rect(430.0, 228.0, 96.0, 30.0),
        controller,
        sel!(previewKeyboardClick:),
        mtm,
    ));
    page.addSubview(&action_button(
        tr("Control"),
        rect(536.0, 228.0, 108.0, 30.0),
        controller,
        sel!(previewControlReaction:),
        mtm,
    ));

    let auto_correct = sound_event_row(&page, SoundEvent::AutoCorrect, 195.0, controller, mtm);
    let manual_conversion =
        sound_event_row(&page, SoundEvent::ManualConversion, 162.0, controller, mtm);
    let layout_switch = sound_event_row(&page, SoundEvent::LayoutSwitch, 129.0, controller, mtm);
    let pause = sound_event_row(&page, SoundEvent::Pause, 96.0, controller, mtm);
    let resume = sound_event_row(&page, SoundEvent::Resume, 63.0, controller, mtm);
    let error = sound_event_row(&page, SoundEvent::Error, 30.0, controller, mtm);

    for child in page.subviews().to_vec() {
        let mut frame = child.frame();
        if frame.origin.y >= 250.0 {
            frame.origin.y += 70.0;
            child.setFrame(frame);
        }
    }
    page.addSubview(&label(
        tr("Headphone limit"),
        rect(48.0, 290.0, 190.0, 24.0),
        mtm,
    ));
    let headphone_volume = unsafe {
        NSSlider::sliderWithValue_minValue_maxValue_target_action(
            12.0,
            0.0,
            30.0,
            Some(controller),
            Some(sel!(headphoneVolumeChanged:)),
            mtm,
        )
    };
    headphone_volume.setFrame(rect(252.0, 285.0, 328.0, 30.0));
    headphone_volume.setContinuous(true);
    headphone_volume.setToolTip(Some(&NSString::from_str(tr("Quiet limit for Upyr sounds on headphones and unidentified outputs. Does not change macOS volume."))));
    page.addSubview(&headphone_volume);
    let headphone_volume_label = label("12%", rect(588.0, 290.0, 56.0, 24.0), mtm);
    page.addSubview(&headphone_volume_label);
    let audio_output_status = label("", rect(48.0, 258.0, 596.0, 22.0), mtm);
    audio_output_status.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    audio_output_status.setTextColor(Some(&NSColor::secondaryLabelColor()));
    page.addSubview(&audio_output_status);
    let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), container.bounds());
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setDrawsBackground(false);
    scroll.setDocumentView(Some(&page));
    container.addSubview(&scroll);
    scroll.contentView().scrollToPoint(NSPoint::new(0.0, 210.0));
    scroll.reflectScrolledClipView(&scroll.contentView());
    FeedbackControls {
        page: container,
        show_indicator: indicator,
        indicator_duration: duration,
        indicator_style,
        flags_preview,
        preview_layout,
        preview_sources,
        sounds_enabled,
        sound_pack,
        key_clicks,
        sound_volume,
        sound_volume_label,
        headphone_volume,
        headphone_volume_label,
        audio_output_status,
        sound_events: SoundEventControls {
            auto_correct,
            manual_conversion,
            layout_switch,
            pause,
            resume,
            error,
        },
    }
}

fn sound_event_row(
    page: &NSView,
    event: SoundEvent,
    y: f64,
    controller: &NativeController,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let enabled = checkbox(event.label(), rect(42.0, y, 360.0, 26.0), mtm);
    page.addSubview(&enabled);
    let preview_action = match event {
        SoundEvent::AutoCorrect => sel!(previewAutoCorrectSound:),
        SoundEvent::ManualConversion => sel!(previewManualConversionSound:),
        SoundEvent::LayoutSwitch => sel!(previewLayoutSwitchSound:),
        SoundEvent::Pause => sel!(previewPauseSound:),
        SoundEvent::Resume => sel!(previewResumeSound:),
        SoundEvent::Error => sel!(previewErrorSound:),
    };
    page.addSubview(&action_button(
        tr("Preview"),
        rect(532.0, y - 2.0, 112.0, 30.0),
        controller,
        preview_action,
        mtm,
    ));
    enabled
}

fn make_advanced_page(
    mtm: MainThreadMarker,
) -> (
    Retained<NSView>,
    Retained<NSTextField>,
    Retained<NSTextField>,
    Retained<NSTextField>,
) {
    let page = page(mtm);
    page_header(
        &page,
        tr("Advanced timing"),
        tr("Increase these only for applications with slow clipboard handling."),
        mtm,
    );
    let copy = numeric_row(&page, tr("Copy delay (ms)"), 326.0, mtm);
    let paste = numeric_row(&page, tr("Paste delay (ms)"), 276.0, mtm);
    let restore = numeric_row(&page, tr("Clipboard restore delay (ms)"), 226.0, mtm);
    (page, copy, paste, restore)
}

fn make_about_page(
    controller: &NativeController,
    mtm: MainThreadMarker,
) -> (Retained<NSView>, UpdateControls) {
    let page = page(mtm);
    page_header(
        &page,
        &format!("Upyr {APP_VERSION}"),
        tr("English ↔ Ukrainian keyboard layout correction."),
        mtm,
    );

    let privacy_heading = label(
        tr("Private by construction"),
        rect(24.0, 330.0, 610.0, 24.0),
        mtm,
    );
    privacy_heading.setFont(Some(&NSFont::boldSystemFontOfSize(14.0)));
    page.addSubview(&privacy_heading);
    page.addSubview(&multiline_label(
        PRIVACY_SUMMARY,
        rect(24.0, 282.0, 620.0, 44.0),
        mtm,
    ));

    let implementation_heading = label(tr("How it works"), rect(24.0, 252.0, 610.0, 24.0), mtm);
    implementation_heading.setFont(Some(&NSFont::boldSystemFontOfSize(14.0)));
    page.addSubview(&implementation_heading);
    page.addSubview(&multiline_label(
        IMPLEMENTATION_SUMMARY,
        rect(24.0, 194.0, 620.0, 54.0),
        mtm,
    ));

    let license_heading = label(tr("Open source"), rect(24.0, 164.0, 610.0, 24.0), mtm);
    license_heading.setFont(Some(&NSFont::boldSystemFontOfSize(14.0)));
    page.addSubview(&license_heading);
    page.addSubview(&label(
        tr("MIT licensed. Source, security policy, and release history are public."),
        rect(24.0, 140.0, 620.0, 22.0),
        mtm,
    ));
    page.addSubview(&action_button(
        tr("Project Website"),
        rect(24.0, 100.0, 150.0, 32.0),
        controller,
        sel!(openProjectWebsite:),
        mtm,
    ));
    page.addSubview(&action_button(
        tr("Source Repository"),
        rect(184.0, 100.0, 158.0, 32.0),
        controller,
        sel!(openSourceRepository:),
        mtm,
    ));
    page.addSubview(&action_button(
        tr("Report Security Issue"),
        rect(352.0, 100.0, 178.0, 32.0),
        controller,
        sel!(reportSecurityIssue:),
        mtm,
    ));
    let check = action_button(
        tr("Check for updates"),
        rect(24.0, 56.0, 220.0, 32.0),
        controller,
        sel!(checkForUpdates:),
        mtm,
    );
    let open = action_button(
        tr("Open release"),
        rect(256.0, 56.0, 180.0, 32.0),
        controller,
        sel!(openUpdateRelease:),
        mtm,
    );
    open.setHidden(true);
    let status = multiline_label(
        &super::updates::CheckState::Idle.message(),
        rect(24.0, 6.0, 620.0, 44.0),
        mtm,
    );
    page.addSubview(&check);
    page.addSubview(&open);
    page.addSubview(&status);
    (
        page,
        UpdateControls {
            check,
            open,
            status,
        },
    )
}

fn page(mtm: MainThreadMarker) -> Retained<NSView> {
    NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT))
}

fn page_header(page: &NSView, title: &str, description: &str, mtm: MainThreadMarker) {
    let heading = label(title, rect(24.0, 402.0, 420.0, 28.0), mtm);
    heading.setFont(Some(&NSFont::boldSystemFontOfSize(18.0)));
    page.addSubview(&heading);
    page.addSubview(&label(description, rect(24.0, 378.0, 610.0, 22.0), mtm));
}

fn add_tab(tabs: &NSTabView, tab: SettingsTab, page: &NSView, _mtm: MainThreadMarker) {
    let item = unsafe { NSTabViewItem::initWithIdentifier(NSTabViewItem::alloc(), None) };
    item.setLabel(&NSString::from_str(tab.label()));
    item.setView(Some(page));
    tabs.addTabViewItem(&item);
}

fn label(value: &str, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(tr(value)), mtm);
    label.setFrame(frame);
    label
}

fn multiline_label(value: &str, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(tr(value)), mtm);
    label.setFrame(frame);
    label.setMaximumNumberOfLines(3);
    label.setUsesSingleLineMode(false);
    label
}

fn text_field(value: &str, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(&NSString::from_str(value), mtm);
    field.setFrame(frame);
    field
}

fn numeric_row(page: &NSView, title: &str, y: f64, mtm: MainThreadMarker) -> Retained<NSTextField> {
    page.addSubview(&label(title, rect(28.0, y, 220.0, 24.0), mtm));
    let field = text_field("", rect(252.0, y - 3.0, 116.0, 26.0), mtm);
    page.addSubview(&field);
    field
}

fn checkbox(title: &str, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::checkboxWithTitle_target_action(&NSString::from_str(tr(title)), None, None, mtm)
    };
    button.setFrame(frame);
    button
}

fn popup(items: &[&str], frame: NSRect, mtm: MainThreadMarker) -> Retained<NSPopUpButton> {
    let popup = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), frame, false);
    for item in items {
        popup.addItemWithTitle(&NSString::from_str(tr(item)));
    }
    popup
}

fn action_button(
    title: &str,
    frame: NSRect,
    target: &NativeController,
    action: Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(tr(title)),
            Some(target),
            Some(action),
            mtm,
        )
    };
    button.setFrame(frame);
    button
}

fn is_checked(button: &NSButton) -> bool {
    button.state() == NSControlStateValueOn
}

fn set_checked(button: &NSButton, checked: bool) {
    button.setState(if checked {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
}

fn set_text(field: &NSTextField, value: impl ToString) {
    field.setStringValue(&NSString::from_str(&value.to_string()));
}

fn parse_number(field: &NSTextField, label: &str, min: u64, max: u64) -> Result<u64> {
    let value = field
        .stringValue()
        .to_string()
        .trim()
        .parse::<u64>()
        .with_context(|| {
            crate::settings::i18n::localized_format!(
                "{label} must be a whole number",
                "{label}: потрібне ціле число"
            )
        })?;
    if !(min..=max).contains(&value) {
        return Err(anyhow!("{label} must be between {min} and {max}"));
    }
    Ok(value)
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn shortcuts_equal(left: &str, right: &str) -> bool {
    match (left.parse::<HotKey>(), right.parse::<HotKey>()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left.eq_ignore_ascii_case(right),
    }
}

fn mac_modifiers(flags: NSEventModifierFlags) -> Modifiers {
    let mut modifiers = Modifiers::empty();
    if flags.contains(NSEventModifierFlags::Shift) {
        modifiers |= Modifiers::SHIFT;
    }
    if flags.contains(NSEventModifierFlags::Control) {
        modifiers |= Modifiers::CONTROL;
    }
    if flags.contains(NSEventModifierFlags::Option) {
        modifiers |= Modifiers::ALT;
    }
    if flags.contains(NSEventModifierFlags::Command) {
        modifiers |= Modifiers::SUPER;
    }
    modifiers
}

const SUPERSET_APP_ID: &str = "com.superset.desktop";

/// Known terminals other than Superset, whose own checkbox manages it.
fn known_terminals() -> impl Iterator<Item = &'static str> {
    crate::input_context::KNOWN_TERMINAL_APPS
        .iter()
        .copied()
        .filter(|app| *app != SUPERSET_APP_ID)
}

/// True when the tr("Correct words in terminals") checkbox should appear checked.
pub(super) fn terminal_correction_enabled(config: &Config) -> bool {
    known_terminals().any(|terminal| {
        config
            .terminal_typing_apps
            .iter()
            .any(|app| app == terminal)
    })
}

/// Applies the tr("Correct words in terminals") checkbox: checking opts every
/// known terminal in; unchecking a previously checked box removes them again.
/// Superset and hand-written entries are left to their own controls.
pub(super) fn set_terminal_correction(config: &mut Config, enabled: bool) {
    if enabled {
        for terminal in known_terminals() {
            if !config
                .terminal_typing_apps
                .iter()
                .any(|app| app == terminal)
            {
                config.terminal_typing_apps.push(terminal.to_owned());
            }
        }
    } else if terminal_correction_enabled(config) {
        config
            .terminal_typing_apps
            .retain(|app| !known_terminals().any(|terminal| terminal == app));
    }
}

/// True when the tr("Slash commands in Superset") checkbox should appear checked.
pub(super) fn superset_slash_commands_enabled(config: &Config) -> bool {
    config
        .slash_command_apps
        .iter()
        .any(|app| app == SUPERSET_APP_ID)
}

/// Applies the tr("Slash commands in Superset") checkbox to both opt-in lists.
/// Slash commands need tracked terminal typing, so checking the box adds
/// Superset to both lists and unchecking a previously checked box removes it
/// from both. Other entries are preserved, and an unchanged unchecked box
/// leaves a hand-written terminal-only Superset opt-in alone.
pub(super) fn set_superset_slash_commands(config: &mut Config, enabled: bool) {
    if enabled {
        for apps in [
            &mut config.slash_command_apps,
            &mut config.terminal_typing_apps,
        ] {
            if !apps.iter().any(|app| app == SUPERSET_APP_ID) {
                apps.push(SUPERSET_APP_ID.to_owned());
            }
        }
    } else if superset_slash_commands_enabled(config) {
        config
            .slash_command_apps
            .retain(|app| app != SUPERSET_APP_ID);
        config
            .terminal_typing_apps
            .retain(|app| app != SUPERSET_APP_ID);
    }
}

fn mac_key_code(code: u16) -> Option<Code> {
    crate::hotkey_bindings::mac_key_code(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_macos_physical_keys_for_shortcut_recording() {
        assert_eq!(mac_key_code(49), Some(Code::Space));
        assert_eq!(mac_key_code(51), Some(Code::Backspace));
        assert_eq!(mac_key_code(40), Some(Code::KeyK));
        assert_eq!(mac_key_code(123), Some(Code::ArrowLeft));
        assert_eq!(mac_key_code(10), None);
    }

    fn app_lists(slash: &[&str], terminal: &[&str]) -> Config {
        Config {
            slash_command_apps: slash.iter().map(|app| (*app).to_owned()).collect(),
            terminal_typing_apps: terminal.iter().map(|app| (*app).to_owned()).collect(),
            ..Config::default()
        }
    }

    #[test]
    fn terminal_correction_opts_every_known_terminal_in_and_out() {
        let mut config = app_lists(&[SUPERSET_APP_ID], &[SUPERSET_APP_ID, "com.example.repl"]);
        assert!(!terminal_correction_enabled(&config));

        set_terminal_correction(&mut config, true);
        set_terminal_correction(&mut config, true);
        assert!(terminal_correction_enabled(&config));
        for terminal in [
            "com.apple.Terminal",
            "com.googlecode.iterm2",
            "com.mitchellh.ghostty",
        ] {
            assert_eq!(
                config
                    .terminal_typing_apps
                    .iter()
                    .filter(|app| *app == terminal)
                    .count(),
                1,
                "{terminal}"
            );
        }

        // Unchecking removes the known terminals only: Superset belongs to its
        // own checkbox and hand-written entries stay.
        set_terminal_correction(&mut config, false);
        assert!(!terminal_correction_enabled(&config));
        assert_eq!(
            config.terminal_typing_apps,
            [SUPERSET_APP_ID, "com.example.repl"]
        );
    }

    #[test]
    fn checking_superset_slash_commands_opts_in_both_lists_once() {
        let mut config = app_lists(&["com.example.cli"], &["com.apple.Terminal"]);

        set_superset_slash_commands(&mut config, true);
        set_superset_slash_commands(&mut config, true);

        assert!(superset_slash_commands_enabled(&config));
        assert_eq!(
            config.slash_command_apps,
            ["com.example.cli", SUPERSET_APP_ID]
        );
        assert_eq!(
            config.terminal_typing_apps,
            ["com.apple.Terminal", SUPERSET_APP_ID]
        );
    }

    #[test]
    fn unchecking_superset_slash_commands_removes_it_from_both_lists() {
        let mut config = app_lists(
            &["com.example.cli", SUPERSET_APP_ID],
            &[SUPERSET_APP_ID, "com.apple.Terminal"],
        );

        set_superset_slash_commands(&mut config, false);

        assert!(!superset_slash_commands_enabled(&config));
        assert_eq!(config.slash_command_apps, ["com.example.cli"]);
        assert_eq!(config.terminal_typing_apps, ["com.apple.Terminal"]);
    }

    #[test]
    fn an_unchanged_unchecked_box_keeps_a_terminal_only_superset_opt_in() {
        let mut config = app_lists(&[], &[SUPERSET_APP_ID, "com.apple.Terminal"]);

        set_superset_slash_commands(&mut config, false);

        assert!(config.slash_command_apps.is_empty());
        assert_eq!(
            config.terminal_typing_apps,
            [SUPERSET_APP_ID, "com.apple.Terminal"]
        );
    }
}
