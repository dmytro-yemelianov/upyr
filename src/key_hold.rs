//! Holds the user's keystrokes while an automatic correction edits text.
//!
//! A correction selects the previous characters with Shift+Left, copies them,
//! pastes the replacement and may switch the layout. A key typed during that
//! window would replace the selection or land before the corrected word, so
//! the correction fails and the caret recovery scrambles the text. Windows
//! has no per-application event tap, so a low-level keyboard hook swallows
//! the user's keys for the duration and replays them, in order, once the
//! corrected text is in place. Upyr's own synthetic input passes through.
//! Nothing here inspects or logs which keys were held.

use std::time::Duration;

/// Longest time keys stay held. A correction normally commits in well under
/// half a second; past this the hook lets new keys through so a stalled edit
/// can never take the keyboard away.
const MAX_HOLD: Duration = Duration::from_millis(1500);

/// Holds keystrokes until released or dropped. Replays them on release.
pub(crate) struct KeyHold {
    active: bool,
}

impl KeyHold {
    /// Starts holding keys. Returns `None` where holding is unavailable, in
    /// which case the correction runs exactly as before.
    pub(crate) fn start() -> Option<Self> {
        platform::start(MAX_HOLD).then_some(Self { active: true })
    }

    /// Stops holding and replays the held keys. Idempotent.
    pub(crate) fn release(&mut self) {
        if std::mem::take(&mut self.active) {
            platform::release();
        }
    }
}

impl Drop for KeyHold {
    fn drop(&mut self) {
        self.release();
    }
}

#[allow(unsafe_code)]
mod platform {
    use std::{
        mem, ptr,
        sync::{Mutex, OnceLock, mpsc},
        thread,
        time::{Duration, Instant},
    };

    use tracing::{debug, warn};
    use windows_sys::Win32::{
        Foundation::{LPARAM, LRESULT, WPARAM},
        UI::{
            Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
                KEYEVENTF_UNICODE, SendInput, VK_PACKET,
            },
            WindowsAndMessaging::{
                CallNextHookEx, DispatchMessageW, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
                LLKHF_UP, MSG, SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL,
            },
        },
    };

    /// `dwExtraInfo` enigo stamps on Upyr's own selection, copy and paste keys.
    const ENIGO_MARKER: usize = 100;
    /// `dwExtraInfo` on replayed keys, so the hook never holds them twice.
    const REPLAY_MARKER: usize = 0x5550_5952; // "UPYR"

    #[derive(Clone, Copy)]
    struct HeldKey {
        vk: u32,
        scan: u32,
        flags: u32,
    }

    struct HoldState {
        deadline: Option<Instant>,
        keys: Vec<HeldKey>,
        /// Keys whose press was held; their release is held too. A release
        /// of a key pressed before the hold passes through, so a modifier
        /// the user lets go of is not left logically down during the edit.
        pressed: Vec<u32>,
    }

    static STATE: Mutex<HoldState> = Mutex::new(HoldState {
        deadline: None,
        keys: Vec::new(),
        pressed: Vec::new(),
    });
    static HOOK_READY: OnceLock<bool> = OnceLock::new();

    pub(super) fn start(limit: Duration) -> bool {
        if !*HOOK_READY.get_or_init(install_hook) {
            return false;
        }
        let Ok(mut state) = STATE.lock() else {
            return false;
        };
        state.deadline = Some(Instant::now() + limit);
        state.keys.clear();
        state.pressed.clear();
        true
    }

    pub(super) fn release() {
        let keys = match STATE.lock() {
            Ok(mut state) => {
                state.deadline = None;
                state.pressed.clear();
                mem::take(&mut state.keys)
            }
            Err(_) => return,
        };
        if keys.is_empty() {
            return;
        }
        let inputs: Vec<INPUT> = keys.iter().map(replay_input).collect();
        // SAFETY: `inputs` is a valid, initialized slice of keyboard INPUTs.
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                mem::size_of::<INPUT>() as i32,
            )
        };
        if sent as usize == inputs.len() {
            debug!(
                keys = inputs.len(),
                "replayed keys held during the correction"
            );
        } else {
            warn!(
                held = inputs.len(),
                sent, "could not replay every key held during the correction"
            );
        }
    }

    fn replay_input(key: &HeldKey) -> INPUT {
        let mut flags = 0;
        if key.flags & LLKHF_UP != 0 {
            flags |= KEYEVENTF_KEYUP;
        }
        let (vk, scan) = if key.vk == u32::from(VK_PACKET) {
            // Text injected by an IME or another tool: the character rides
            // in the scan code.
            flags |= KEYEVENTF_UNICODE;
            (0, key.scan as u16)
        } else {
            if key.flags & LLKHF_EXTENDED != 0 {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            (key.vk as u16, key.scan as u16)
        };
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: REPLAY_MARKER,
                },
            },
        }
    }

    fn install_hook() -> bool {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let spawned = thread::Builder::new()
            .name("upyr-key-hold".to_owned())
            .spawn(move || {
                // SAFETY: a low-level hook needs no module handle; the
                // procedure lives in this process for its whole lifetime.
                let hook = unsafe {
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), ptr::null_mut(), 0)
                };
                let installed = !hook.is_null();
                let _ = ready_tx.send(installed);
                if !installed {
                    return;
                }
                // Low-level hooks run on the installing thread's message loop.
                // SAFETY: zeroed MSG is a valid out-parameter.
                let mut message: MSG = unsafe { mem::zeroed() };
                // SAFETY: `message` is valid for writes; the loop owns it.
                while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
                    unsafe {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
            });
        if spawned.is_err() {
            warn!("could not start the key-hold thread; corrections run without holding keys");
            return false;
        }
        let installed = ready_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or(false);
        if !installed {
            warn!("could not install the key-hold hook; corrections run without holding keys");
        }
        installed
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // HC_ACTION is 0; negative codes must go straight to the next hook.
        if code == 0 && lparam != 0 {
            // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
            let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
            if hold(info) {
                return 1;
            }
        }
        // SAFETY: forwarding the unmodified hook arguments.
        unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
    }

    fn hold(info: &KBDLLHOOKSTRUCT) -> bool {
        if matches!(info.dwExtraInfo, ENIGO_MARKER | REPLAY_MARKER) {
            return false;
        }
        let Ok(mut state) = STATE.try_lock() else {
            // Never stall the system keyboard on our own lock.
            return false;
        };
        match state.deadline {
            Some(deadline) if Instant::now() < deadline => {}
            _ => return false,
        }
        let up = info.flags & LLKHF_UP != 0;
        if up {
            if !state.pressed.contains(&info.vkCode) {
                return false;
            }
        } else if !state.pressed.contains(&info.vkCode) {
            state.pressed.push(info.vkCode);
        }
        state.keys.push(HeldKey {
            vk: info.vkCode,
            scan: info.scanCode,
            flags: info.flags,
        });
        true
    }
}
