#![allow(unsafe_code)]

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
    NSPopUpButton, NSSearchField, NSSlider, NSTabView, NSTabViewItem, NSTextField, NSView,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

use super::{
    ACTIVATION_POLL_INTERVAL, APP_VERSION, ActivationInbox, IMPLEMENTATION_SUMMARY,
    PRIVACY_SUMMARY, PROJECT_WEBSITE_URL, REPOSITORY_URL, SEARCH_PARAMETERS, SettingsTab,
    autostart_attention, parse_exceptions, pretty_hotkey, sensitivity_label, sync_launch_at_login,
};
use crate::{
    autostart,
    config::{AutoCorrectSensitivity, Config, GestureAction, ModifierGesture, SoundEvent},
    layout::Direction,
};

const WINDOW_WIDTH: f64 = 720.0;
const WINDOW_HEIGHT: f64 = 680.0;
const PAGE_WIDTH: f64 = 672.0;
const PAGE_HEIGHT: f64 = 456.0;

struct ControllerIvars {
    config: RefCell<Config>,
    autostart_status: RefCell<autostart::AutostartStatus>,
    activation: ActivationInbox,
    controls: OnceCell<Controls>,
}

struct Controls {
    window: Retained<NSWindow>,
    tabs: Retained<NSTabView>,
    search: Retained<NSSearchField>,
    status: Retained<NSTextField>,
    direction: Retained<NSPopUpButton>,
    switch_layout: Retained<NSButton>,
    restore_clipboard: Retained<NSButton>,
    launch_at_login: Retained<NSButton>,
    repair_autostart: Retained<NSButton>,
    remove_autostart: Retained<NSButton>,
    auto_correct: Retained<NSButton>,
    sensitivity: Retained<NSPopUpButton>,
    minimum_word_length: Retained<NSTextField>,
    auto_delay: Retained<NSTextField>,
    exceptions: Retained<NSTextField>,
    selection_shortcut: Retained<ShortcutRecorder>,
    previous_word_shortcut: Retained<ShortcutRecorder>,
    modifier_gesture: Retained<NSPopUpButton>,
    gesture_action: Retained<NSPopUpButton>,
    gesture_timeout: Retained<NSTextField>,
    show_indicator: Retained<NSButton>,
    indicator_duration: Retained<NSTextField>,
    sounds_enabled: Retained<NSButton>,
    sound_volume: Retained<NSSlider>,
    sound_volume_label: Retained<NSTextField>,
    sound_events: SoundEventControls,
    copy_delay: Retained<NSTextField>,
    paste_delay: Retained<NSTextField>,
    restore_delay: Retained<NSTextField>,
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
    sounds_enabled: Retained<NSButton>,
    sound_volume: Retained<NSSlider>,
    sound_volume_label: Retained<NSTextField>,
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
            let config = match self.collect_config() {
                Ok(config) => config,
                Err(error) => {
                    self.set_status(false, &format!("Could not save settings: {error:#}"));
                    return;
                }
            };
            if let Err(error) = config.write(true) {
                self.set_status(false, &format!("Could not save settings: {error:#}"));
                return;
            }
            self.ivars().config.replace(config);

            let desired = is_checked(&self.controls().launch_at_login);
            match sync_launch_at_login(desired) {
                Ok(status) => {
                    self.set_autostart_status(status);
                    if !self.show_autostart_attention("Settings saved. ") {
                        self.set_status(
                            true,
                            "Saved. The running Upyr process will reload these settings.",
                        );
                    }
                }
                Err(error) => {
                    if let Ok(status) = autostart::status() {
                        self.set_autostart_status(status);
                    }
                    self.set_status(
                        false,
                        &format!(
                            "Settings saved, but launch at login was not changed: {error:#}"
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
                    self.set_status(true, "Launch-at-login entry repaired.");
                }
                Err(error) => {
                    self.set_status(false, &format!("Could not repair entry: {error:#}"));
                }
            }
        }

        #[unsafe(method(removeAutostart:))]
        fn remove_autostart(&self, _sender: &AnyObject) {
            match autostart::disable() {
                Ok(status) => {
                    self.set_autostart_status(status);
                    self.set_status(true, "Launch-at-login entry removed.");
                }
                Err(error) => {
                    self.set_status(false, &format!("Could not remove entry: {error:#}"));
                }
            }
        }

        #[unsafe(method(resetSettings:))]
        fn reset_settings(&self, _sender: &AnyObject) {
            self.apply_config(&Config::default());
            set_checked(&self.controls().launch_at_login, false);
            self.set_status(true, "Defaults restored in the form. Choose Save to apply them.");
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
            let result = SEARCH_PARAMETERS.iter().find(|parameter| {
                format!("{} {}", parameter.label, parameter.terms)
                    .to_lowercase()
                    .contains(&query)
            });
            if let Some(parameter) = result {
                self.controls()
                    .tabs
                    .selectTabViewItemAtIndex(tab_index(parameter.tab));
                self.set_status(
                    true,
                    &format!("{}  ›  {}", parameter.tab.label(), parameter.label),
                );
            } else {
                self.set_status(false, "No settings match this search.");
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
                show_layout_indicator: is_checked(&controls.show_indicator),
                layout_indicator_duration_ms: parse_number(
                    &controls.indicator_duration,
                    "indicator duration",
                    250,
                    3_000,
                )
                .unwrap_or(900),
                ..Config::default()
            };
            if !preview.show_layout_indicator {
                self.set_status(false, "Enable the language flag to preview it.");
                return;
            }

            let indicator_shown = crate::feedback::layout_switched(
                crate::system_layout::SystemLayout::Ukrainian,
                &preview,
            );
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
            self.set_status(true, "Previewed the language flag.");
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

        #[unsafe(method(soundVolumeChanged:))]
        fn sound_volume_changed(&self, _sender: &AnyObject) {
            self.refresh_sound_volume_label();
        }

        #[unsafe(method(hideFeedback:))]
        fn hide_feedback(&self, _sender: Option<&AnyObject>) {
            crate::feedback::hide_layout_indicator();
        }

        #[unsafe(method(checkSettingsActivation:))]
        fn check_settings_activation(&self, _sender: Option<&AnyObject>) {
            match self.ivars().activation.drain() {
                Ok(Some(tab)) => self.activate_tab(tab),
                Ok(None) => {}
                Err(error) => self.set_status(
                    false,
                    &format!("Could not activate the settings window: {error:#}"),
                ),
            }
            self.schedule_activation_poll();
        }

    }
);

impl NativeController {
    fn open_external_url(&self, url: &str) {
        match Command::new("open").arg(url).spawn() {
            Ok(_) => self.set_status(true, "Opened in your default browser."),
            Err(error) => self.set_status(false, &format!("Could not open the link: {error}")),
        }
    }

    fn preview_sound(&self, event: SoundEvent) {
        let mut settings = self.ivars().config.borrow().sounds;
        settings.enabled = true;
        settings.set_event_selected(event, true);
        settings.volume_percent = self.sound_volume_percent();
        match crate::feedback::preview_sound(event, &settings) {
            Ok(()) => self.set_status(true, &format!("Previewed {} sound.", event.label())),
            Err(error) => self.set_status(
                false,
                &format!("Could not preview {} sound: {error:#}", event.label()),
            ),
        }
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
        let this = Self::alloc(mtm).set_ivars(ControllerIvars {
            config: RefCell::new(config),
            autostart_status: RefCell::new(autostart_status),
            activation,
            controls: OnceCell::new(),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn activate_tab(&self, tab: SettingsTab) {
        self.controls()
            .tabs
            .selectTabViewItemAtIndex(tab_index(tab));
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
        window.setTitle(&NSString::from_str("Upyr Settings"));
        window.center();
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        let content = window
            .contentView()
            .expect("native settings window must have a content view");

        let title = label("Upyr Settings", rect(24.0, 632.0, 280.0, 30.0), mtm);
        title.setFont(Some(&NSFont::boldSystemFontOfSize(22.0)));
        content.addSubview(&title);
        content.addSubview(&label(
            "English ↔ Ukrainian keyboard layout correction",
            rect(24.0, 610.0, 330.0, 20.0),
            mtm,
        ));

        let search = NSSearchField::initWithFrame(
            NSSearchField::alloc(mtm),
            rect(420.0, 624.0, 276.0, 30.0),
        );
        search.setPlaceholderString(Some(&NSString::from_str("Search settings")));
        search.setSendsSearchStringImmediately(true);
        unsafe {
            search.setTarget(Some(self));
            search.setAction(Some(sel!(searchSettings:)));
        }
        content.addSubview(&search);

        let tabs = NSTabView::initWithFrame(NSTabView::alloc(mtm), rect(18.0, 106.0, 684.0, 492.0));
        let (
            general,
            direction,
            switch_layout,
            restore_clipboard,
            login,
            repair_autostart,
            remove_autostart,
        ) = make_general_page(self, mtm);
        add_tab(&tabs, "General", &general, mtm);
        let (automatic, auto_correct, sensitivity, minimum_word_length, auto_delay, exceptions) =
            make_automatic_page(mtm);
        add_tab(&tabs, "Automatic", &automatic, mtm);
        let (
            shortcuts,
            selection_shortcut,
            previous_word_shortcut,
            modifier_gesture,
            gesture_action,
            gesture_timeout,
        ) = make_shortcuts_page(self, mtm);
        add_tab(&tabs, "Shortcuts", &shortcuts, mtm);
        let FeedbackControls {
            page: feedback,
            show_indicator,
            indicator_duration,
            sounds_enabled,
            sound_volume,
            sound_volume_label,
            sound_events,
        } = make_feedback_page(self, mtm);
        add_tab(&tabs, "Feedback", &feedback, mtm);
        let (advanced, copy_delay, paste_delay, restore_delay) = make_advanced_page(mtm);
        add_tab(&tabs, "Advanced", &advanced, mtm);
        let about = make_about_page(self, mtm);
        add_tab(&tabs, "About", &about, mtm);
        content.addSubview(&tabs);

        let status = label("", rect(24.0, 60.0, 672.0, 40.0), mtm);
        status.setMaximumNumberOfLines(2);
        status.setUsesSingleLineMode(false);
        content.addSubview(&status);
        content.addSubview(&action_button(
            "Save",
            rect(504.0, 24.0, 92.0, 32.0),
            self,
            sel!(saveSettings:),
            mtm,
        ));
        content.addSubview(&action_button(
            "Reset All",
            rect(400.0, 24.0, 96.0, 32.0),
            self,
            sel!(resetSettings:),
            mtm,
        ));
        content.addSubview(&action_button(
            "Close",
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
                direction,
                switch_layout,
                restore_clipboard,
                launch_at_login: login,
                repair_autostart,
                remove_autostart,
                auto_correct,
                sensitivity,
                minimum_word_length,
                auto_delay,
                exceptions,
                selection_shortcut,
                previous_word_shortcut,
                modifier_gesture,
                gesture_action,
                gesture_timeout,
                show_indicator,
                indicator_duration,
                sounds_enabled,
                sound_volume,
                sound_volume_label,
                sound_events,
                copy_delay,
                paste_delay,
                restore_delay,
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
        config.hotkey = controls.selection_shortcut.value();
        config.last_word_hotkey = controls.previous_word_shortcut.value();
        config.direction = match controls.direction.indexOfSelectedItem() {
            1 => Direction::EnglishToUkrainian,
            2 => Direction::UkrainianToEnglish,
            _ => Direction::Smart,
        };
        config.switch_layout = is_checked(&controls.switch_layout);
        config.restore_clipboard = is_checked(&controls.restore_clipboard);
        config.auto_correct = is_checked(&controls.auto_correct);
        config.auto_correct_sensitivity = match controls.sensitivity.indexOfSelectedItem() {
            1 => AutoCorrectSensitivity::Balanced,
            2 => AutoCorrectSensitivity::Aggressive,
            _ => AutoCorrectSensitivity::Conservative,
        };
        config.auto_correct_min_word_length =
            parse_number(&controls.minimum_word_length, "minimum word length", 2, 32)? as usize;
        config.auto_correct_delay_ms =
            parse_number(&controls.auto_delay, "automatic correction delay", 10, 250)?;
        config.auto_correct_exceptions =
            parse_exceptions(&controls.exceptions.stringValue().to_string());
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
        config.sounds.enabled = is_checked(&controls.sounds_enabled);
        config.sounds.volume_percent = self.sound_volume_percent();
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
        let controls = self.controls();
        controls
            .direction
            .selectItemAtIndex(match config.direction {
                Direction::Smart => 0,
                Direction::EnglishToUkrainian => 1,
                Direction::UkrainianToEnglish => 2,
            });
        set_checked(&controls.switch_layout, config.switch_layout);
        set_checked(&controls.restore_clipboard, config.restore_clipboard);
        set_checked(&controls.auto_correct, config.auto_correct);
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
        controls.exceptions.setStringValue(&NSString::from_str(
            &config.auto_correct_exceptions.join(", "),
        ));
        controls.selection_shortcut.set_value(&config.hotkey);
        controls
            .previous_word_shortcut
            .set_value(&config.last_word_hotkey);
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
        set_checked(&controls.sounds_enabled, config.sounds.enabled);
        controls
            .sound_volume
            .setDoubleValue(f64::from(config.sounds.volume_percent));
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

    fn validate_shortcuts(&self) {
        let controls = self.controls();
        let selection = controls.selection_shortcut.value();
        let previous = controls.previous_word_shortcut.value();
        let message = if controls.selection_shortcut.error().is_some()
            || controls.previous_word_shortcut.error().is_some()
        {
            Some("A shortcut is incomplete. Record a key together with at least one modifier.")
        } else if shortcuts_equal(&selection, &previous) {
            Some("The two actions cannot use the same shortcut.")
        } else {
            None
        };
        if let Some(message) = message {
            self.set_status(false, message);
        } else {
            self.set_status(true, "Shortcuts are valid.");
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
        status.setStringValue(&NSString::from_str(message));
        status.setToolTip(Some(&NSString::from_str(message)));
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
            self.setTitle(&NSString::from_str("Press shortcut…  (Esc to cancel)"));
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
                    .replace(Some("That key is not supported.".to_owned()));
                self.setTitle(&NSString::from_str("Unsupported key — try another"));
                return;
            };
            let modifiers = mac_modifiers(event.modifierFlags());
            if modifiers.is_empty() {
                self.ivars().error.replace(Some(
                    "Include Command, Control, Option, or Shift.".to_owned(),
                ));
                self.setTitle(&NSString::from_str("Add a modifier key"));
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
) -> Result<()> {
    let mtm =
        MainThreadMarker::new().context("Upyr Settings must start on the macOS main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let controller = NativeController::new(config, autostart_status, activation, mtm);
    let window = controller.build_window();
    controller
        .controls()
        .tabs
        .selectTabViewItemAtIndex(tab_index(initial_tab));
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
    Retained<NSButton>,
    Retained<NSButton>,
    Retained<NSButton>,
    Retained<NSButton>,
    Retained<NSButton>,
) {
    let page = page(mtm);
    page_header(
        &page,
        "General",
        "Core conversion, clipboard, and startup behavior.",
        mtm,
    );
    page.addSubview(&label(
        "Conversion direction",
        rect(28.0, 330.0, 210.0, 24.0),
        mtm,
    ));
    let direction = popup(
        &[
            "Smart (detect script)",
            "English → Ukrainian",
            "Ukrainian → English",
        ],
        rect(252.0, 326.0, 260.0, 28.0),
        mtm,
    );
    page.addSubview(&direction);
    let switch_layout = checkbox(
        "Switch the OS layout to match converted text",
        rect(24.0, 272.0, 430.0, 26.0),
        mtm,
    );
    let restore = checkbox(
        "Restore clipboard contents after conversion",
        rect(24.0, 228.0, 430.0, 26.0),
        mtm,
    );
    let login = checkbox("Launch Upyr at login", rect(24.0, 184.0, 430.0, 26.0), mtm);
    let repair_autostart = action_button(
        "Repair Entry",
        rect(24.0, 132.0, 126.0, 32.0),
        controller,
        sel!(repairAutostart:),
        mtm,
    );
    let remove_autostart = action_button(
        "Remove Entry",
        rect(160.0, 132.0, 126.0, 32.0),
        controller,
        sel!(removeAutostart:),
        mtm,
    );
    repair_autostart.setEnabled(false);
    repair_autostart.setHidden(true);
    remove_autostart.setEnabled(false);
    remove_autostart.setHidden(true);
    page.addSubview(&switch_layout);
    page.addSubview(&restore);
    page.addSubview(&login);
    page.addSubview(&repair_autostart);
    page.addSubview(&remove_autostart);
    (
        page,
        direction,
        switch_layout,
        restore,
        login,
        repair_autostart,
        remove_autostart,
    )
}

#[allow(clippy::type_complexity)]
fn make_automatic_page(
    mtm: MainThreadMarker,
) -> (
    Retained<NSView>,
    Retained<NSButton>,
    Retained<NSPopUpButton>,
    Retained<NSTextField>,
    Retained<NSTextField>,
    Retained<NSTextField>,
) {
    let page = page(mtm);
    page_header(
        &page,
        "Automatic correction",
        "Opt-in, local recognition after Space. Typed prefixes are never logged.",
        mtm,
    );
    let enabled = checkbox(
        "Correct confidently recognized text after Space",
        rect(24.0, 346.0, 470.0, 26.0),
        mtm,
    );
    page.addSubview(&enabled);
    page.addSubview(&label("Sensitivity", rect(28.0, 298.0, 180.0, 24.0), mtm));
    let sensitivity = popup(
        &[
            sensitivity_label(AutoCorrectSensitivity::Conservative),
            sensitivity_label(AutoCorrectSensitivity::Balanced),
            sensitivity_label(AutoCorrectSensitivity::Aggressive),
        ],
        rect(252.0, 294.0, 220.0, 28.0),
        mtm,
    );
    page.addSubview(&sensitivity);
    let minimum = numeric_row(&page, "Minimum word length", 250.0, mtm);
    let delay = numeric_row(&page, "Delay after Space (ms)", 206.0, mtm);
    page.addSubview(&label(
        "Never correct (comma-separated)",
        rect(28.0, 158.0, 220.0, 24.0),
        mtm,
    ));
    let exceptions = text_field("", rect(252.0, 154.0, 372.0, 26.0), mtm);
    exceptions.setPlaceholderString(Some(&NSString::from_str("GitHub, Upyr, project-name")));
    page.addSubview(&exceptions);
    page.addSubview(&label(
        "macOS Accessibility access is checked before input monitoring starts.",
        rect(28.0, 96.0, 590.0, 24.0),
        mtm,
    ));
    (page, enabled, sensitivity, minimum, delay, exceptions)
}

#[allow(clippy::type_complexity)]
fn make_shortcuts_page(
    controller: &NativeController,
    mtm: MainThreadMarker,
) -> (
    Retained<NSView>,
    Retained<ShortcutRecorder>,
    Retained<ShortcutRecorder>,
    Retained<NSPopUpButton>,
    Retained<NSPopUpButton>,
    Retained<NSTextField>,
) {
    let page = page(mtm);
    page_header(
        &page,
        "Shortcuts",
        "Click a recorder, then press a physical key combination. Upyr actions are paused here.",
        mtm,
    );
    page.addSubview(&label(
        "Convert selection",
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
        "Reset",
        rect(536.0, 332.0, 82.0, 32.0),
        controller,
        sel!(resetSelectionShortcut:),
        mtm,
    ));
    page.addSubview(&label(
        "Fix previous word",
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
        "Reset",
        rect(536.0, 284.0, 82.0, 32.0),
        controller,
        sel!(resetPreviousShortcut:),
        mtm,
    ));
    page.addSubview(&label(
        "Modifier gesture",
        rect(28.0, 218.0, 180.0, 24.0),
        mtm,
    ));
    let gesture = popup(
        &[
            "Disabled",
            "Double Control",
            "Double Shift",
            "Double Control + Shift",
        ],
        rect(210.0, 214.0, 258.0, 28.0),
        mtm,
    );
    page.addSubview(&gesture);
    page.addSubview(&label(
        "Gesture action",
        rect(28.0, 174.0, 180.0, 24.0),
        mtm,
    ));
    let action = popup(
        &["Fix previous word", "Convert selection"],
        rect(210.0, 170.0, 258.0, 28.0),
        mtm,
    );
    page.addSubview(&action);
    let timeout = numeric_row(&page, "Double-tap timeout (ms)", 126.0, mtm);
    (page, selection, previous, gesture, action, timeout)
}

fn make_feedback_page(controller: &NativeController, mtm: MainThreadMarker) -> FeedbackControls {
    let page = page(mtm);
    page_header(
        &page,
        "Feedback",
        "Optional visual and sound confirmation for Upyr actions.",
        mtm,
    );
    let indicator = checkbox(
        "Show a temporary language flag next to the pointer",
        rect(24.0, 342.0, 470.0, 26.0),
        mtm,
    );
    page.addSubview(&indicator);
    page.addSubview(&action_button(
        "Preview Flag",
        rect(532.0, 339.0, 112.0, 30.0),
        controller,
        sel!(previewFeedback:),
        mtm,
    ));
    page.addSubview(&label(
        "Indicator duration (ms)",
        rect(48.0, 308.0, 190.0, 24.0),
        mtm,
    ));
    let duration = text_field("", rect(252.0, 305.0, 116.0, 26.0), mtm);
    page.addSubview(&duration);

    let sounds_enabled = checkbox("Enable event sounds", rect(24.0, 267.0, 250.0, 26.0), mtm);
    page.addSubview(&sounds_enabled);
    page.addSubview(&label("Volume", rect(280.0, 269.0, 58.0, 24.0), mtm));
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
    sound_volume.setFrame(rect(342.0, 264.0, 230.0, 30.0));
    sound_volume.setContinuous(true);
    sound_volume.setNumberOfTickMarks(5);
    sound_volume.setAllowsTickMarkValuesOnly(false);
    sound_volume.setAltIncrementValue(5.0);
    sound_volume.setToolTip(Some(&NSString::from_str("Sound volume, 0 to 100 percent")));
    page.addSubview(&sound_volume);
    let sound_volume_label = label("65%", rect(584.0, 269.0, 60.0, 24.0), mtm);
    page.addSubview(&sound_volume_label);

    let auto_correct = sound_event_row(&page, SoundEvent::AutoCorrect, 229.0, controller, mtm);
    let manual_conversion =
        sound_event_row(&page, SoundEvent::ManualConversion, 195.0, controller, mtm);
    let layout_switch = sound_event_row(&page, SoundEvent::LayoutSwitch, 161.0, controller, mtm);
    let pause = sound_event_row(&page, SoundEvent::Pause, 127.0, controller, mtm);
    let resume = sound_event_row(&page, SoundEvent::Resume, 93.0, controller, mtm);
    let error = sound_event_row(&page, SoundEvent::Error, 59.0, controller, mtm);

    FeedbackControls {
        page,
        show_indicator: indicator,
        indicator_duration: duration,
        sounds_enabled,
        sound_volume,
        sound_volume_label,
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
        "Preview",
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
        "Advanced timing",
        "Increase these only for applications with slow clipboard handling.",
        mtm,
    );
    let copy = numeric_row(&page, "Copy delay (ms)", 326.0, mtm);
    let paste = numeric_row(&page, "Paste delay (ms)", 276.0, mtm);
    let restore = numeric_row(&page, "Clipboard restore delay (ms)", 226.0, mtm);
    (page, copy, paste, restore)
}

fn make_about_page(controller: &NativeController, mtm: MainThreadMarker) -> Retained<NSView> {
    let page = page(mtm);
    page_header(
        &page,
        &format!("Upyr {APP_VERSION}"),
        "English ↔ Ukrainian keyboard layout correction.",
        mtm,
    );

    let privacy_heading = label(
        "Private by construction",
        rect(24.0, 330.0, 610.0, 24.0),
        mtm,
    );
    privacy_heading.setFont(Some(&NSFont::boldSystemFontOfSize(14.0)));
    page.addSubview(&privacy_heading);
    page.addSubview(&multiline_label(
        PRIVACY_SUMMARY,
        rect(24.0, 276.0, 620.0, 50.0),
        mtm,
    ));

    let implementation_heading = label("How it works", rect(24.0, 238.0, 610.0, 24.0), mtm);
    implementation_heading.setFont(Some(&NSFont::boldSystemFontOfSize(14.0)));
    page.addSubview(&implementation_heading);
    page.addSubview(&multiline_label(
        IMPLEMENTATION_SUMMARY,
        rect(24.0, 178.0, 620.0, 56.0),
        mtm,
    ));

    let license_heading = label("Open source", rect(24.0, 140.0, 610.0, 24.0), mtm);
    license_heading.setFont(Some(&NSFont::boldSystemFontOfSize(14.0)));
    page.addSubview(&license_heading);
    page.addSubview(&label(
        "MIT licensed. Source, security policy, and release history are public.",
        rect(24.0, 116.0, 620.0, 22.0),
        mtm,
    ));
    page.addSubview(&action_button(
        "Project Website",
        rect(24.0, 68.0, 150.0, 32.0),
        controller,
        sel!(openProjectWebsite:),
        mtm,
    ));
    page.addSubview(&action_button(
        "Source Repository",
        rect(184.0, 68.0, 158.0, 32.0),
        controller,
        sel!(openSourceRepository:),
        mtm,
    ));
    page.addSubview(&action_button(
        "Report Security Issue",
        rect(352.0, 68.0, 178.0, 32.0),
        controller,
        sel!(reportSecurityIssue:),
        mtm,
    ));
    page
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

fn add_tab(tabs: &NSTabView, title: &str, page: &NSView, _mtm: MainThreadMarker) {
    let item = unsafe { NSTabViewItem::initWithIdentifier(NSTabViewItem::alloc(), None) };
    item.setLabel(&NSString::from_str(title));
    item.setView(Some(page));
    tabs.addTabViewItem(&item);
}

fn label(value: &str, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(value), mtm);
    label.setFrame(frame);
    label
}

fn multiline_label(value: &str, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = label(value, frame, mtm);
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
        NSButton::checkboxWithTitle_target_action(&NSString::from_str(title), None, None, mtm)
    };
    button.setFrame(frame);
    button
}

fn popup(items: &[&str], frame: NSRect, mtm: MainThreadMarker) -> Retained<NSPopUpButton> {
    let popup = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), frame, false);
    for item in items {
        popup.addItemWithTitle(&NSString::from_str(item));
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
            &NSString::from_str(title),
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
        .with_context(|| format!("{label} must be a whole number"))?;
    if !(min..=max).contains(&value) {
        return Err(anyhow!("{label} must be between {min} and {max}"));
    }
    Ok(value)
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn tab_index(tab: SettingsTab) -> isize {
    match tab {
        SettingsTab::General => 0,
        SettingsTab::Automatic => 1,
        SettingsTab::Shortcuts => 2,
        SettingsTab::Feedback => 3,
        SettingsTab::Advanced => 4,
        SettingsTab::About => 5,
    }
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

fn mac_key_code(code: u16) -> Option<Code> {
    Some(match code {
        0 => Code::KeyA,
        1 => Code::KeyS,
        2 => Code::KeyD,
        3 => Code::KeyF,
        4 => Code::KeyH,
        5 => Code::KeyG,
        6 => Code::KeyZ,
        7 => Code::KeyX,
        8 => Code::KeyC,
        9 => Code::KeyV,
        11 => Code::KeyB,
        12 => Code::KeyQ,
        13 => Code::KeyW,
        14 => Code::KeyE,
        15 => Code::KeyR,
        16 => Code::KeyY,
        17 => Code::KeyT,
        18 => Code::Digit1,
        19 => Code::Digit2,
        20 => Code::Digit3,
        21 => Code::Digit4,
        22 => Code::Digit6,
        23 => Code::Digit5,
        24 => Code::Equal,
        25 => Code::Digit9,
        26 => Code::Digit7,
        27 => Code::Minus,
        28 => Code::Digit8,
        29 => Code::Digit0,
        30 => Code::BracketRight,
        31 => Code::KeyO,
        32 => Code::KeyU,
        33 => Code::BracketLeft,
        34 => Code::KeyI,
        35 => Code::KeyP,
        36 => Code::Enter,
        37 => Code::KeyL,
        38 => Code::KeyJ,
        39 => Code::Quote,
        40 => Code::KeyK,
        41 => Code::Semicolon,
        42 => Code::Backslash,
        43 => Code::Comma,
        44 => Code::Slash,
        45 => Code::KeyN,
        46 => Code::KeyM,
        47 => Code::Period,
        48 => Code::Tab,
        49 => Code::Space,
        50 => Code::Backquote,
        51 => Code::Backspace,
        53 => Code::Escape,
        64 => Code::F17,
        79 => Code::F18,
        80 => Code::F19,
        90 => Code::F20,
        96 => Code::F5,
        97 => Code::F6,
        98 => Code::F7,
        99 => Code::F3,
        100 => Code::F8,
        101 => Code::F9,
        103 => Code::F11,
        105 => Code::F13,
        106 => Code::F16,
        107 => Code::F14,
        109 => Code::F10,
        111 => Code::F12,
        113 => Code::F15,
        115 => Code::Home,
        116 => Code::PageUp,
        117 => Code::Delete,
        118 => Code::F4,
        119 => Code::End,
        120 => Code::F2,
        121 => Code::PageDown,
        122 => Code::F1,
        123 => Code::ArrowLeft,
        124 => Code::ArrowRight,
        125 => Code::ArrowDown,
        126 => Code::ArrowUp,
        _ => return None,
    })
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
}
