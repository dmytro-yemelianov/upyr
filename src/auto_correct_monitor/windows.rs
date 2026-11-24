//! Windows key listener for automatic correction.
//!
//! `rdev::listen` names every key press by attaching its hook thread to the
//! foreground thread's input queue (`AttachThreadInput`) and calling
//! `ToUnicodeEx`. Doing that inside a low-level hook breaks other
//! applications' shortcuts: while it ran, Windows 11 Notepad ignored every
//! Ctrl+C, so neither automatic correction nor a manual conversion could
//! copy the word they were about to replace. This listener only observes:
//! it never attaches to another thread, never translates through the
//! system keyboard state, and passes every key on unchanged.
//!
//! The rendered name the capture state uses to detect Caps Lock is derived
//! from the tracked Shift and Caps Lock state for Latin letter keys only.
#![allow(unsafe_code)]

use std::{cell::RefCell, mem, ptr};

use rdev::{EventType, Key};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::GetKeyState,
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
            SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN,
            WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    },
};

const VK_CAPITAL: u32 = 0x14;
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;

type Callback = Box<dyn FnMut(EventType, Option<&str>)>;

struct ListenerThread {
    callback: Callback,
    left_shift: bool,
    right_shift: bool,
    caps_lock: bool,
    caps_lock_down: bool,
}

thread_local! {
    // A low-level hook runs on the thread that installed it.
    static LISTENER: RefCell<Option<ListenerThread>> = const { RefCell::new(None) };
}

/// Installs the hook on the calling thread and pumps its messages. Returns
/// only if the hook cannot be installed or the message loop ends.
pub(super) fn listen(
    callback: impl FnMut(EventType, Option<&str>) + 'static,
) -> Result<(), String> {
    // SAFETY: GetKeyState has no preconditions. A thread that has not read
    // input yet sees the current toggle state.
    let caps_lock = unsafe { GetKeyState(VK_CAPITAL as i32) } & 1 != 0;
    LISTENER.with(|slot| {
        *slot.borrow_mut() = Some(ListenerThread {
            callback: Box::new(callback),
            left_shift: false,
            right_shift: false,
            caps_lock,
            caps_lock_down: false,
        });
    });
    // SAFETY: a low-level hook needs no module handle; the procedure lives
    // in this process for its whole lifetime.
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), ptr::null_mut(), 0) };
    if hook.is_null() {
        LISTENER.with(|slot| slot.borrow_mut().take());
        return Err(format!(
            "could not install the keyboard hook: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: zeroed MSG is a valid out-parameter for GetMessageW.
    let mut message: MSG = unsafe { mem::zeroed() };
    // SAFETY: `message` is valid for writes for the whole loop.
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
        // SAFETY: `message` was filled in by GetMessageW.
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    // SAFETY: `hook` was returned by SetWindowsHookExW on this thread.
    unsafe { UnhookWindowsHookEx(hook) };
    LISTENER.with(|slot| slot.borrow_mut().take());
    Err("the keyboard hook message loop ended".to_owned())
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam != 0 {
        // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let pressed = match wparam as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(true),
            WM_KEYUP | WM_SYSKEYUP => Some(false),
            _ => None,
        };
        if let Some(pressed) = pressed {
            // A panic must not unwind across the FFI boundary, and a
            // re-entrant call (should a callback pump messages) is skipped.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                LISTENER.with(|slot| {
                    if let Ok(mut slot) = slot.try_borrow_mut() {
                        if let Some(listener) = slot.as_mut() {
                            listener.observe(info.vkCode, pressed);
                        }
                    }
                });
            }));
        }
    }
    // SAFETY: forwarding the unmodified hook arguments.
    unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
}

impl ListenerThread {
    fn observe(&mut self, vk: u32, pressed: bool) {
        match vk {
            VK_LSHIFT => self.left_shift = pressed,
            VK_RSHIFT => self.right_shift = pressed,
            VK_CAPITAL => {
                // Caps Lock toggles on the press, not on auto-repeat.
                if pressed && !self.caps_lock_down {
                    self.caps_lock = !self.caps_lock;
                }
                self.caps_lock_down = pressed;
            }
            _ => {}
        }
        let key = key_from_code(vk);
        let event = if pressed {
            EventType::KeyPress(key)
        } else {
            EventType::KeyRelease(key)
        };
        let mut buffer = [0u8; 4];
        let name = pressed.then(|| self.letter_name(vk, &mut buffer)).flatten();
        (self.callback)(event, name);
    }

    /// The case a Latin letter key renders in. Other keys have no name; the
    /// capture state then infers nothing from them.
    fn letter_name<'a>(&self, vk: u32, buffer: &'a mut [u8; 4]) -> Option<&'a str> {
        let letter = char::from_u32(vk).filter(char::is_ascii_uppercase)?;
        let upper = (self.left_shift || self.right_shift) != self.caps_lock;
        let letter = if upper {
            letter
        } else {
            letter.to_ascii_lowercase()
        };
        Some(letter.encode_utf8(buffer))
    }
}

/// Virtual-key codes as `rdev` names them on Windows, plus the right
/// Windows key, which `rdev` leaves unnamed.
fn key_from_code(code: u32) -> Key {
    match code {
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0D => Key::Return,
        0x13 => Key::Pause,
        0x14 => Key::CapsLock,
        0x1B => Key::Escape,
        0x20 => Key::Space,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x23 => Key::End,
        0x24 => Key::Home,
        0x25 => Key::LeftArrow,
        0x26 => Key::UpArrow,
        0x27 => Key::RightArrow,
        0x28 => Key::DownArrow,
        0x2C => Key::PrintScreen,
        0x2D => Key::Insert,
        0x2E => Key::Delete,
        0x30 => Key::Num0,
        0x31 => Key::Num1,
        0x32 => Key::Num2,
        0x33 => Key::Num3,
        0x34 => Key::Num4,
        0x35 => Key::Num5,
        0x36 => Key::Num6,
        0x37 => Key::Num7,
        0x38 => Key::Num8,
        0x39 => Key::Num9,
        0x41 => Key::KeyA,
        0x42 => Key::KeyB,
        0x43 => Key::KeyC,
        0x44 => Key::KeyD,
        0x45 => Key::KeyE,
        0x46 => Key::KeyF,
        0x47 => Key::KeyG,
        0x48 => Key::KeyH,
        0x49 => Key::KeyI,
        0x4A => Key::KeyJ,
        0x4B => Key::KeyK,
        0x4C => Key::KeyL,
        0x4D => Key::KeyM,
        0x4E => Key::KeyN,
        0x4F => Key::KeyO,
        0x50 => Key::KeyP,
        0x51 => Key::KeyQ,
        0x52 => Key::KeyR,
        0x53 => Key::KeyS,
        0x54 => Key::KeyT,
        0x55 => Key::KeyU,
        0x56 => Key::KeyV,
        0x57 => Key::KeyW,
        0x58 => Key::KeyX,
        0x59 => Key::KeyY,
        0x5A => Key::KeyZ,
        0x5B => Key::MetaLeft,
        0x5C => Key::MetaRight,
        0x60 => Key::Kp0,
        0x61 => Key::Kp1,
        0x62 => Key::Kp2,
        0x63 => Key::Kp3,
        0x64 => Key::Kp4,
        0x65 => Key::Kp5,
        0x66 => Key::Kp6,
        0x67 => Key::Kp7,
        0x68 => Key::Kp8,
        0x69 => Key::Kp9,
        0x6A => Key::KpMultiply,
        0x6B => Key::KpPlus,
        0x6D => Key::KpMinus,
        0x6E => Key::KpDelete,
        0x6F => Key::KpDivide,
        0x70 => Key::F1,
        0x71 => Key::F2,
        0x72 => Key::F3,
        0x73 => Key::F4,
        0x74 => Key::F5,
        0x75 => Key::F6,
        0x76 => Key::F7,
        0x77 => Key::F8,
        0x78 => Key::F9,
        0x79 => Key::F10,
        0x7A => Key::F11,
        0x7B => Key::F12,
        0x90 => Key::NumLock,
        0x91 => Key::ScrollLock,
        0xA0 => Key::ShiftLeft,
        0xA1 => Key::ShiftRight,
        0xA2 => Key::ControlLeft,
        0xA3 => Key::ControlRight,
        0xA4 => Key::Alt,
        0xA5 => Key::AltGr,
        0xBA => Key::SemiColon,
        0xBB => Key::Equal,
        0xBC => Key::Comma,
        0xBD => Key::Minus,
        0xBE => Key::Dot,
        0xBF => Key::Slash,
        0xC0 => Key::BackQuote,
        0xDB => Key::LeftBracket,
        0xDC => Key::BackSlash,
        0xDD => Key::RightBracket,
        0xDE => Key::Quote,
        0xE2 => Key::IntlBackslash,
        other => Key::Unknown(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener() -> ListenerThread {
        ListenerThread {
            callback: Box::new(|_, _| {}),
            left_shift: false,
            right_shift: false,
            caps_lock: false,
            caps_lock_down: false,
        }
    }

    #[test]
    fn letter_names_follow_shift_and_caps_lock() {
        let mut listener = listener();
        let mut buffer = [0u8; 4];
        assert_eq!(listener.letter_name(0x41, &mut buffer), Some("a"));
        listener.observe(VK_LSHIFT, true);
        assert_eq!(listener.letter_name(0x41, &mut buffer), Some("A"));
        listener.observe(VK_LSHIFT, false);
        listener.observe(VK_CAPITAL, true);
        listener.observe(VK_CAPITAL, true); // auto-repeat does not toggle
        listener.observe(VK_CAPITAL, false);
        assert_eq!(listener.letter_name(0x41, &mut buffer), Some("A"));
        listener.observe(VK_RSHIFT, true);
        assert_eq!(listener.letter_name(0x41, &mut buffer), Some("a"));
        assert_eq!(listener.letter_name(0xBA, &mut buffer), None);
        assert_eq!(listener.letter_name(0x31, &mut buffer), None);
    }

    #[test]
    fn virtual_keys_map_like_rdev() {
        assert_eq!(key_from_code(0x41), Key::KeyA);
        assert_eq!(key_from_code(0xDE), Key::Quote);
        assert_eq!(key_from_code(0xA5), Key::AltGr);
        assert_eq!(key_from_code(0x5C), Key::MetaRight);
        assert_eq!(key_from_code(0xFF), Key::Unknown(0xFF));
    }
}
