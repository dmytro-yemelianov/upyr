use crate::{config::Config, system_layout::SystemLayout};

/// Presents the enabled local feedback channels for a confirmed OS input-source
/// change. Returns whether a temporary visual indicator was shown.
pub fn layout_switched(layout: SystemLayout, config: &Config) -> bool {
    if config.play_switch_sound {
        platform::play_sound();
    }
    config.show_layout_indicator && platform::show_indicator(layout)
}

pub fn hide_layout_indicator() {
    platform::hide_indicator();
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux", test))]
fn indicator_text(layout: SystemLayout) -> &'static str {
    match layout {
        SystemLayout::English => "EN  🇬🇧",
        SystemLayout::Ukrainian => "UK  🇺🇦",
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod platform {
    use std::cell::RefCell;

    use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
    use objc2_app_kit::{
        NSBackingStoreType, NSColor, NSEvent, NSFont, NSPanel, NSSound, NSStatusWindowLevel,
        NSTextAlignment, NSTextField, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    use super::{SystemLayout, indicator_text};

    struct Indicator {
        panel: Retained<NSPanel>,
        label: Retained<NSTextField>,
    }

    thread_local! {
        static INDICATOR: RefCell<Option<Indicator>> = const { RefCell::new(None) };
    }

    pub fn play_sound() {
        let sound_name = NSString::from_str("Tink");
        if let Some(sound) = NSSound::soundNamed(&sound_name) {
            sound.setVolume(0.35);
            sound.play();
        }
    }

    pub fn show_indicator(layout: SystemLayout) -> bool {
        let Some(main_thread) = MainThreadMarker::new() else {
            return false;
        };
        INDICATOR.with_borrow_mut(|indicator| {
            let indicator = indicator.get_or_insert_with(|| make_indicator(main_thread));
            indicator
                .label
                .setStringValue(&NSString::from_str(indicator_text(layout)));
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

    use super::{SystemLayout, indicator_text};

    thread_local! {
        static INDICATOR: Cell<HWND> = const { Cell::new(ptr::null_mut()) };
    }

    pub fn play_sound() {
        unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(0);
        }
    }

    pub fn show_indicator(layout: SystemLayout) -> bool {
        let text = wide(indicator_text(layout));
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
    use std::{cell::RefCell, process::Command, process::Stdio};

    use device_query::{DeviceQuery, DeviceState};
    use gtk::prelude::*;

    use super::{SystemLayout, indicator_text};

    struct Indicator {
        window: gtk::Window,
        label: gtk::Label,
    }

    thread_local! {
        static INDICATOR: RefCell<Option<Indicator>> = const { RefCell::new(None) };
    }

    pub fn play_sound() {
        let mut command = Command::new("canberra-gtk-play");
        command
            .args([
                "--id",
                "audio-volume-change",
                "--description",
                "Upyr layout switch",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _ = command.spawn();
    }

    pub fn show_indicator(layout: SystemLayout) -> bool {
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
                indicator_text(layout)
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
    use super::SystemLayout;

    pub fn play_sound() {}
    pub fn show_indicator(_layout: SystemLayout) -> bool {
        false
    }
    pub fn hide_indicator() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_uses_short_language_labels_and_flags() {
        assert_eq!(indicator_text(SystemLayout::English), "EN  🇬🇧");
        assert_eq!(indicator_text(SystemLayout::Ukrainian), "UK  🇺🇦");
    }
}
