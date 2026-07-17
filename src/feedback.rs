mod sound;

use anyhow::{Result, bail};
use tracing::debug;
use upyr_audio::KeyCue;

use crate::{
    config::{Config, IndicatorStyle, SoundEvent, SoundSettings},
    system_layout::SystemLayout,
};

/// Plays a sound directly from the settings preview UI.
///
/// Preview callers opt into the event on a temporary settings value, so this
/// uses the same master switch, event switch, and volume rules as live events.
pub fn preview_sound(event: SoundEvent, settings: &SoundSettings) -> Result<()> {
    if !settings.event_enabled(event) {
        bail!("{} sound is disabled or its volume is zero", event.label());
    }
    sound::play_event(event, settings.pack, settings.volume_percent)
}

/// Previews the selected keyboard pack without requiring the live monitor to
/// be enabled. Only the physical key category crosses this boundary.
pub fn preview_key_sound(cue: KeyCue, settings: &SoundSettings) -> Result<()> {
    if !settings.key_clicks_enabled() {
        bail!("keyboard sounds are disabled or their volume is zero");
    }
    sound::play_key(cue, settings.pack, settings.volume_percent, true)
}

/// Plays the configured cue for an application event.
///
/// Returns `true` only when playback was enabled and successfully started. A
/// caller can use this to choose a single fallback cue without double-playing.
pub fn play_sound_event(event: SoundEvent, config: &Config) -> bool {
    if !config.sounds.event_enabled(event) {
        return false;
    }

    match sound::play_event(event, config.sounds.pack, config.sounds.volume_percent) {
        Ok(()) => true,
        Err(error) => {
            debug!(?event, %error, "sound feedback unavailable");
            false
        }
    }
}

pub fn play_key_sound(cue: KeyCue, config: &Config) -> bool {
    if !config.sounds.key_clicks_enabled() {
        return false;
    }
    match sound::play_key(cue, config.sounds.pack, config.sounds.volume_percent, false) {
        Ok(()) => true,
        Err(error) => {
            debug!(?cue, %error, "keyboard sound feedback unavailable");
            false
        }
    }
}

pub fn prewarm_sound_pack(settings: &SoundSettings) {
    if sound_pack_needs_prewarm(settings) {
        sound::prewarm(settings.pack);
    }
}

fn sound_pack_needs_prewarm(settings: &SoundSettings) -> bool {
    settings.key_clicks_enabled()
        || SoundEvent::ALL
            .into_iter()
            .any(|event| settings.event_enabled(event))
}

/// Shows the enabled visual feedback for a confirmed OS input-source change.
/// Sound dispatch stays separate so each action can produce at most one cue.
pub fn layout_switched(layout: SystemLayout, config: &Config) -> bool {
    config.show_layout_indicator && platform::show_indicator(layout, config.indicator_style)
}

pub fn hide_layout_indicator() {
    platform::hide_indicator();
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux", test))]
fn indicator_text(layout: SystemLayout, style: IndicatorStyle) -> &'static str {
    match (layout, style) {
        (SystemLayout::English, IndicatorStyle::Letters) => "EN",
        (SystemLayout::English, IndicatorStyle::Flag) => "🇬🇧",
        (SystemLayout::English, IndicatorStyle::Both) => "EN  🇬🇧",
        (SystemLayout::Ukrainian, IndicatorStyle::Letters) => "UK",
        (SystemLayout::Ukrainian, IndicatorStyle::Flag) => "🇺🇦",
        (SystemLayout::Ukrainian, IndicatorStyle::Both) => "UK  🇺🇦",
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod platform {
    use std::cell::RefCell;

    use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
    use objc2_app_kit::{
        NSBackingStoreType, NSColor, NSEvent, NSFont, NSPanel, NSStatusWindowLevel,
        NSTextAlignment, NSTextField, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    use super::{IndicatorStyle, SystemLayout, indicator_text};

    struct Indicator {
        panel: Retained<NSPanel>,
        label: Retained<NSTextField>,
    }

    thread_local! {
        static INDICATOR: RefCell<Option<Indicator>> = const { RefCell::new(None) };
    }

    pub fn show_indicator(layout: SystemLayout, style: IndicatorStyle) -> bool {
        let Some(main_thread) = MainThreadMarker::new() else {
            return false;
        };
        INDICATOR.with_borrow_mut(|indicator| {
            let indicator = indicator.get_or_insert_with(|| make_indicator(main_thread));
            indicator
                .label
                .setStringValue(&NSString::from_str(indicator_text(layout, style)));
            let pointer = NSEvent::mouseLocation();
            indicator
                .panel
                .setFrameOrigin(NSPoint::new(pointer.x + 16.0, pointer.y + 18.0));
            indicator.panel.orderFrontRegardless();
        });
        true
    }

    pub fn hide_indicator() {
        if MainThreadMarker::new().is_none() {
            return;
        }
        INDICATOR.with_borrow(|indicator| {
            if let Some(indicator) = indicator.as_ref() {
                indicator.panel.orderOut(None);
            }
        });
    }

    fn make_indicator(main_thread: MainThreadMarker) -> Indicator {
        let size = NSSize::new(94.0, 42.0);
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(main_thread),
            NSRect::new(NSPoint::new(0.0, 0.0), size),
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setFloatingPanel(true);
        panel.setLevel(NSStatusWindowLevel);
        panel.setOpaque(false);
        panel.setHasShadow(true);
        panel.setIgnoresMouseEvents(true);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        panel.setBackgroundColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
            0.09, 0.10, 0.13, 0.92,
        )));

        let label = NSTextField::labelWithString(&NSString::from_str(""), main_thread);
        label.setFrame(NSRect::new(NSPoint::new(0.0, 7.0), NSSize::new(94.0, 28.0)));
        label.setAlignment(NSTextAlignment::Center);
        label.setFont(Some(&NSFont::boldSystemFontOfSize(16.0)));
        label.setTextColor(Some(&NSColor::whiteColor()));
        let content = panel
            .contentView()
            .expect("a newly created NSPanel must have a content view");
        content.addSubview(&label);

        Indicator { panel, label }
    }
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod platform {
    use std::{cell::Cell, ptr};

    use windows_sys::Win32::{
        Foundation::{HWND, POINT},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject, UpdateWindow},
        System::SystemServices::{SS_CENTER, SS_CENTERIMAGE},
        UI::WindowsAndMessaging::{
            CreateWindowExW, GetCursorPos, HWND_TOPMOST, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW,
            SendMessageW, SetWindowPos, SetWindowTextW, ShowWindow, WM_SETFONT, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        },
    };

    use super::{IndicatorStyle, SystemLayout, indicator_text};

    thread_local! {
        static INDICATOR: Cell<HWND> = const { Cell::new(ptr::null_mut()) };
    }

    pub fn show_indicator(layout: SystemLayout, style: IndicatorStyle) -> bool {
        let text = wide(indicator_text(layout, style));
        let class = wide("STATIC");
        let mut pointer = POINT { x: 0, y: 0 };
        unsafe {
            GetCursorPos(&mut pointer);
        }
        INDICATOR.with(|stored| {
            let mut window = stored.get();
            if window.is_null() {
                window = unsafe {
                    CreateWindowExW(
                        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                        class.as_ptr(),
                        text.as_ptr(),
                        WS_POPUP | SS_CENTER | SS_CENTERIMAGE,
                        pointer.x + 16,
                        pointer.y + 18,
                        104,
                        42,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null(),
                    )
                };
                if window.is_null() {
                    return false;
                }
                stored.set(window);
                let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
                unsafe {
                    SendMessageW(window, WM_SETFONT, font as usize, 1);
                }
            } else {
                unsafe {
                    SetWindowTextW(window, text.as_ptr());
                }
            }
            unsafe {
                SetWindowPos(
                    window,
                    HWND_TOPMOST,
                    pointer.x + 16,
                    pointer.y + 18,
                    104,
                    42,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                UpdateWindow(window);
            }
            true
        })
    }

    pub fn hide_indicator() {
        INDICATOR.with(|stored| {
            let window = stored.get();
            if !window.is_null() {
                unsafe {
                    ShowWindow(window, SW_HIDE);
                }
            }
        });
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::cell::RefCell;

    use device_query::{DeviceQuery, DeviceState};
    use gtk::prelude::*;

    use super::{IndicatorStyle, SystemLayout, indicator_text};

    struct Indicator {
        window: gtk::Window,
        label: gtk::Label,
    }

    thread_local! {
        static INDICATOR: RefCell<Option<Indicator>> = const { RefCell::new(None) };
    }

    pub fn show_indicator(layout: SystemLayout, style: IndicatorStyle) -> bool {
        if !gtk::is_initialized() {
            return false;
        }
        INDICATOR.with_borrow_mut(|indicator| {
            let indicator = indicator.get_or_insert_with(|| {
                let window = gtk::Window::new(gtk::WindowType::Popup);
                window.set_decorated(false);
                window.set_keep_above(true);
                window.set_skip_taskbar_hint(true);
                window.set_skip_pager_hint(true);
                window.set_accept_focus(false);
                window.set_resizable(false);
                window.set_default_size(104, 42);
                let label = gtk::Label::new(None);
                label.set_margin_start(14);
                label.set_margin_end(14);
                label.set_margin_top(10);
                label.set_margin_bottom(10);
                window.add(&label);
                Indicator { window, label }
            });
            indicator.label.set_markup(&format!(
                "<b><span size=\"large\">{}</span></b>",
                indicator_text(layout, style)
            ));
            let pointer = DeviceState::new().get_mouse().coords;
            indicator.window.move_(pointer.0 + 16, pointer.1 + 18);
            indicator.window.show_all();
        });
        true
    }

    pub fn hide_indicator() {
        INDICATOR.with_borrow(|indicator| {
            if let Some(indicator) = indicator.as_ref() {
                indicator.window.hide();
            }
        });
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform {
    use super::{IndicatorStyle, SystemLayout};

    pub fn show_indicator(_layout: SystemLayout, _style: IndicatorStyle) -> bool {
        false
    }
    pub fn hide_indicator() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_style_selects_letters_flag_or_both() {
        assert_eq!(
            indicator_text(SystemLayout::English, IndicatorStyle::Letters),
            "EN"
        );
        assert_eq!(
            indicator_text(SystemLayout::English, IndicatorStyle::Flag),
            "🇬🇧"
        );
        assert_eq!(
            indicator_text(SystemLayout::English, IndicatorStyle::Both),
            "EN  🇬🇧"
        );
        assert_eq!(
            indicator_text(SystemLayout::Ukrainian, IndicatorStyle::Letters),
            "UK"
        );
        assert_eq!(
            indicator_text(SystemLayout::Ukrainian, IndicatorStyle::Flag),
            "🇺🇦"
        );
        assert_eq!(
            indicator_text(SystemLayout::Ukrainian, IndicatorStyle::Both),
            "UK  🇺🇦"
        );
    }

    #[test]
    fn prewarms_for_events_or_keyboard_feedback_only_when_audible() {
        let mut settings = SoundSettings::default();
        assert!(!sound_pack_needs_prewarm(&settings));

        settings.enabled = true;
        assert!(sound_pack_needs_prewarm(&settings));

        for event in SoundEvent::ALL {
            settings.set_event_selected(event, false);
        }
        assert!(!sound_pack_needs_prewarm(&settings));

        settings.set_event_selected(SoundEvent::Error, true);
        assert!(sound_pack_needs_prewarm(&settings));
        settings.set_event_selected(SoundEvent::Error, false);

        settings.key_clicks = true;
        assert!(sound_pack_needs_prewarm(&settings));

        settings.volume_percent = 0;
        assert!(!sound_pack_needs_prewarm(&settings));
    }
}
