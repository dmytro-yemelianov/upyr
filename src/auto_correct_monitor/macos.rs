//! AppKit delivers global-monitor callbacks on the main thread. Read its
//! rendered characters directly; never call rdev's Carbon translator on a worker.
#![allow(unsafe_code)]

use super::*;
use anyhow::Context;
use block2::RcBlock;
use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
use std::{cell::RefCell, ffi::c_void, ptr::NonNull, rc::Rc};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
}

thread_local! {
    static MONITOR: RefCell<Option<Monitor>> = const { RefCell::new(None) };
}

struct Monitor(Retained<AnyObject>);
impl Drop for Monitor {
    fn drop(&mut self) {
        // The token came from this exact AppKit monitor and stays on its thread.
        unsafe { NSEvent::removeMonitor(&self.0) };
    }
}

struct Capture {
    state: Arc<ListenerState>,
    keys: CaptureState,
    held: [bool; 128],
}

pub(super) fn start(state: Arc<ListenerState>) -> Result<()> {
    MainThreadMarker::new().context("keyboard monitoring must start on the main thread")?;
    MONITOR.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            return Ok(());
        }
        let device = DeviceState::checked_new();
        let pressed = device
            .as_ref()
            .map(DeviceQuery::get_keys)
            .unwrap_or_default();
        let mut held = [false; 128];
        for (code, down) in held.iter_mut().enumerate() {
            *down = map_key(key_from_code(code as u16)).is_some_and(|key| pressed.contains(&key));
        }
        let capture = Rc::new(RefCell::new(Capture {
            state,
            keys: initial_capture_state(device.as_ref()),
            held,
        }));
        let handler = RcBlock::new(move |event: NonNull<NSEvent>| {
            let Ok(mut capture) = capture.try_borrow_mut() else {
                return;
            };
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // AppKit owns the event for the duration of this callback.
                capture.observe(unsafe { event.as_ref() });
            }))
            .is_err()
            {
                dispatch_activity(&capture.state, ActivityEvent::Pointer);
                capture.keys = CaptureState::default();
                dispatch_key(&capture.state, reset_event(Keycode::Escape));
                tracing::error!("keyboard monitor recovered from a callback panic");
            }
        });
        let token = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            NSEventMask::KeyDown
                | NSEventMask::KeyUp
                | NSEventMask::FlagsChanged
                | NSEventMask::LeftMouseDown
                | NSEventMask::RightMouseDown
                | NSEventMask::OtherMouseDown
                | NSEventMask::ScrollWheel,
            &handler,
        )
        .context("could not install the AppKit keyboard monitor; check Accessibility permission")?;
        *slot = Some(Monitor(token));
        Ok(())
    })
}

impl Capture {
    fn observe(&mut self, event: &NSEvent) {
        if MainThreadMarker::new().is_none() {
            return; // Fail closed if AppKit ever violates its callback contract.
        }
        let cg_event: *mut c_void = unsafe { objc2::msg_send![event, CGEvent] };
        if !cg_event.is_null()
            && unsafe { CGEventGetIntegerValueField(cg_event, 41) } == i64::from(std::process::id())
        {
            return; // Copy/Paste and recovery keys must not become new user input.
        }
        let kind = event.r#type();
        if matches!(
            kind,
            NSEventType::LeftMouseDown
                | NSEventType::RightMouseDown
                | NSEventType::OtherMouseDown
                | NSEventType::ScrollWheel
        ) {
            dispatch_activity(&self.state, ActivityEvent::Pointer);
            self.keys = CaptureState::default();
            dispatch_key(&self.state, reset_event(Keycode::Escape));
            return;
        }
        let flags = event.modifierFlags().bits();
        if kind == NSEventType::KeyDown {
            dispatch_activity(
                &self.state,
                ActivityEvent::Key {
                    code: event.keyCode(),
                    flags,
                },
            );
        }
        let Some(event_type) = decode(kind, event.keyCode(), flags, &mut self.held) else {
            return;
        };
        if kind == NSEventType::KeyDown {
            // NSEvent flags are the state of this event, including startup and
            // synthetic events. A hardware snapshot can lag or omit those.
            self.keys.left_shift = flags & (1 << 17) != 0;
            self.keys.right_shift = false;
            self.keys.left_control = flags & (1 << 18) != 0;
            self.keys.right_control = false;
            self.keys.left_option = flags & (1 << 19) != 0;
            self.keys.right_option = false;
            self.keys.left_meta = flags & (1 << 20) != 0;
            self.keys.right_meta = false;
            self.keys.caps_lock = flags & (1 << 16) != 0;
            self.keys.shift_release_pending = false;
            self.keys.snapshot_missing_chords = 0;
        }
        let name = (kind == NSEventType::KeyDown)
            .then(|| event.characters().map(|text| text.to_string()))
            .flatten();
        handle_event(
            &mut self.keys,
            &self.state,
            event_type,
            name.as_deref(),
            None,
        );
    }
}

fn decode(kind: NSEventType, code: u16, flags: usize, held: &mut [bool; 128]) -> Option<EventType> {
    let down = match kind {
        NSEventType::KeyDown => true,
        NSEventType::KeyUp => false,
        NSEventType::FlagsChanged => {
            let mask = match code {
                54 | 55 => 1 << 20,
                56 | 60 => 1 << 17,
                57 => 1 << 16,
                58 | 61 => 1 << 19,
                59 | 62 => 1 << 18,
                _ => return None,
            };
            flags & mask != 0 && (code == 57 || !held[usize::from(code)])
        }
        _ => return None,
    };
    if let Some(held) = held.get_mut(usize::from(code)) {
        *held = down;
    }
    let key = key_from_code(code);
    Some(if down {
        EventType::KeyPress(key)
    } else {
        EventType::KeyRelease(key)
    })
}

// macOS virtual key positions; independent of the selected input source.
fn key_from_code(code: u16) -> Key {
    match code {
        0 => Key::KeyA,
        1 => Key::KeyS,
        2 => Key::KeyD,
        3 => Key::KeyF,
        4 => Key::KeyH,
        5 => Key::KeyG,
        6 => Key::KeyZ,
        7 => Key::KeyX,
        8 => Key::KeyC,
        9 => Key::KeyV,
        10 => Key::IntlBackslash,
        11 => Key::KeyB,
        12 => Key::KeyQ,
        13 => Key::KeyW,
        14 => Key::KeyE,
        15 => Key::KeyR,
        16 => Key::KeyY,
        17 => Key::KeyT,
        18 => Key::Num1,
        19 => Key::Num2,
        20 => Key::Num3,
        21 => Key::Num4,
        22 => Key::Num6,
        23 => Key::Num5,
        24 => Key::Equal,
        25 => Key::Num9,
        26 => Key::Num7,
        27 => Key::Minus,
        28 => Key::Num8,
        29 => Key::Num0,
        30 => Key::RightBracket,
        31 => Key::KeyO,
        32 => Key::KeyU,
        33 => Key::LeftBracket,
        34 => Key::KeyI,
        35 => Key::KeyP,
        36 => Key::Return,
        37 => Key::KeyL,
        38 => Key::KeyJ,
        39 => Key::Quote,
        40 => Key::KeyK,
        41 => Key::SemiColon,
        42 => Key::BackSlash,
        43 => Key::Comma,
        44 => Key::Slash,
        45 => Key::KeyN,
        46 => Key::KeyM,
        47 => Key::Dot,
        48 => Key::Tab,
        49 => Key::Space,
        50 => Key::BackQuote,
        51 => Key::Backspace,
        53 => Key::Escape,
        54 => Key::MetaRight,
        55 => Key::MetaLeft,
        56 => Key::ShiftLeft,
        57 => Key::CapsLock,
        58 => Key::Alt,
        59 => Key::ControlLeft,
        60 => Key::ShiftRight,
        61 => Key::AltGr,
        62 => Key::ControlRight,
        63 => Key::Function,
        65 => Key::KpDelete,
        67 => Key::KpMultiply,
        69 => Key::KpPlus,
        75 => Key::KpDivide,
        76 => Key::KpReturn,
        78 => Key::KpMinus,
        82 => Key::Kp0,
        83 => Key::Kp1,
        84 => Key::Kp2,
        85 => Key::Kp3,
        86 => Key::Kp4,
        87 => Key::Kp5,
        88 => Key::Kp6,
        89 => Key::Kp7,
        91 => Key::Kp8,
        92 => Key::Kp9,
        96 => Key::F5,
        97 => Key::F6,
        98 => Key::F7,
        99 => Key::F3,
        100 => Key::F8,
        101 => Key::F9,
        103 => Key::F11,
        109 => Key::F10,
        111 => Key::F12,
        114 => Key::Insert,
        115 => Key::Home,
        116 => Key::PageUp,
        117 => Key::Delete,
        118 => Key::F4,
        119 => Key::End,
        120 => Key::F2,
        121 => Key::PageDown,
        122 => Key::F1,
        123 => Key::LeftArrow,
        124 => Key::RightArrow,
        125 => Key::DownArrow,
        126 => Key::UpArrow,
        _ => Key::Unknown(u32::from(code)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_positions_cover_letters_and_ukrainian_punctuation() {
        assert_eq!(map_key(key_from_code(5)), Some(Keycode::G));
        assert_eq!(map_key(key_from_code(33)), Some(Keycode::LeftBracket));
        assert_eq!(map_key(key_from_code(39)), Some(Keycode::Apostrophe));
        assert_eq!(map_key(key_from_code(62)), Some(Keycode::RControl));
        assert_eq!(map_key(key_from_code(76)), Some(Keycode::NumpadEnter));
    }

    #[test]
    fn releasing_one_shift_while_the_other_is_held_is_a_release() {
        let mut held = [false; 128];
        let shift = 1 << 17;
        assert_eq!(
            decode(NSEventType::FlagsChanged, 56, shift, &mut held),
            Some(EventType::KeyPress(Key::ShiftLeft))
        );
        assert_eq!(
            decode(NSEventType::FlagsChanged, 60, shift, &mut held),
            Some(EventType::KeyPress(Key::ShiftRight))
        );
        assert_eq!(
            decode(NSEventType::FlagsChanged, 56, shift, &mut held),
            Some(EventType::KeyRelease(Key::ShiftLeft))
        );
        assert_eq!(
            decode(NSEventType::FlagsChanged, 60, 0, &mut held),
            Some(EventType::KeyRelease(Key::ShiftRight))
        );
    }

    #[test]
    fn caps_lock_uses_its_logical_flag_and_globe_is_not_a_text_key() {
        let mut held = [false; 128];
        assert_eq!(
            decode(NSEventType::FlagsChanged, 57, 1 << 16, &mut held),
            Some(EventType::KeyPress(Key::CapsLock))
        );
        assert_eq!(
            decode(NSEventType::FlagsChanged, 57, 0, &mut held),
            Some(EventType::KeyRelease(Key::CapsLock))
        );
        assert_eq!(
            decode(NSEventType::FlagsChanged, 63, 1 << 23, &mut held),
            None
        );
    }
}
