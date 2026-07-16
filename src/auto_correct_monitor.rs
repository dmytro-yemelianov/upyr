use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use device_query::Keycode;
use rdev::{EventType, Key};
use tracing::error;

use crate::auto_correct::AutoKeyEvent;

const START_TIMEOUT: Duration = Duration::from_millis(250);
const START_POLL_INTERVAL: Duration = Duration::from_millis(2);
const LISTENER_IDLE: u8 = 0;
const LISTENER_STARTING: u8 = 1;
const LISTENER_RUNNING: u8 = 2;
const LISTENER_FAILED: u8 = 3;

type KeyCallback = Box<dyn FnMut(AutoKeyEvent) + Send + 'static>;

struct Subscription {
    id: u64,
    suspended: Arc<AtomicBool>,
    on_key_down: KeyCallback,
}

struct ListenerState {
    status: AtomicU8,
    next_subscription: AtomicU64,
    subscription: Mutex<Option<Subscription>>,
}

impl Default for ListenerState {
    fn default() -> Self {
        Self {
            status: AtomicU8::new(LISTENER_IDLE),
            next_subscription: AtomicU64::new(1),
            subscription: Mutex::new(None),
        }
    }
}

static LISTENER_STATE: OnceLock<Arc<ListenerState>> = OnceLock::new();

pub struct AutoCorrectMonitor {
    state: Arc<ListenerState>,
    subscription_id: u64,
    suspended: Arc<AtomicBool>,
}

impl AutoCorrectMonitor {
    /// Subscribes to the process-wide native key-down hook. The underlying OS
    /// listener remains alive for the process lifetime so settings reloads can
    /// replace the active subscription without briefly running duplicate hooks.
    /// No typed text is logged or retained here.
    pub fn start(
        enabled: bool,
        on_key_down: impl FnMut(AutoKeyEvent) + Send + 'static,
    ) -> Result<Option<Self>> {
        if !enabled {
            return Ok(None);
        }

        let state = Arc::clone(LISTENER_STATE.get_or_init(|| Arc::new(ListenerState::default())));
        ensure_listener(&state)?;

        let subscription_id = state.next_subscription.fetch_add(1, Ordering::Relaxed);
        let suspended = Arc::new(AtomicBool::new(false));
        let subscription = Subscription {
            id: subscription_id,
            suspended: Arc::clone(&suspended),
            on_key_down: Box::new(on_key_down),
        };
        *state
            .subscription
            .lock()
            .map_err(|_| anyhow::anyhow!("automatic-correction listener state was poisoned"))? =
            Some(subscription);

        Ok(Some(Self {
            state,
            subscription_id,
            suspended,
        }))
    }

    pub fn set_suspended(&self, suspended: bool) {
        self.suspended.store(suspended, Ordering::Relaxed);
    }
}

impl Drop for AutoCorrectMonitor {
    fn drop(&mut self) {
        let Ok(mut subscription) = self.state.subscription.lock() else {
            return;
        };
        if subscription
            .as_ref()
            .is_some_and(|current| current.id == self.subscription_id)
        {
            *subscription = None;
        }
    }
}

fn ensure_listener(state: &Arc<ListenerState>) -> Result<()> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match state.status.load(Ordering::Acquire) {
            LISTENER_RUNNING => return Ok(()),
            LISTENER_STARTING if Instant::now() < deadline => {
                thread::sleep(START_POLL_INTERVAL);
            }
            LISTENER_STARTING => {
                bail!("automatic-correction native listener did not initialize");
            }
            status @ (LISTENER_IDLE | LISTENER_FAILED) => {
                if state
                    .status
                    .compare_exchange(
                        status,
                        LISTENER_STARTING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    return spawn_listener(state);
                }
            }
            status => bail!("automatic-correction listener has invalid state {status}"),
        }
    }
}

fn spawn_listener(state: &Arc<ListenerState>) -> Result<()> {
    let (start_tx, start_rx) = mpsc::sync_channel(1);
    let listener_state = Arc::clone(state);
    if let Err(error) = thread::Builder::new()
        .name("upyr-auto-correct".to_owned())
        .spawn(move || {
            let mut left_shift = false;
            let mut right_shift = false;
            let callback_state = Arc::clone(&listener_state);
            let result = rdev::listen(move |event| match event.event_type {
                EventType::KeyPress(Key::ShiftLeft) => left_shift = true,
                EventType::KeyPress(Key::ShiftRight) => right_shift = true,
                EventType::KeyRelease(Key::ShiftLeft) => left_shift = false,
                EventType::KeyRelease(Key::ShiftRight) => right_shift = false,
                EventType::KeyPress(key) => {
                    if let Some(key) = map_key(key) {
                        dispatch_key(&callback_state, key, left_shift || right_shift);
                    }
                }
                _ => {}
            });

            listener_state
                .status
                .store(LISTENER_FAILED, Ordering::Release);
            let message = match result {
                Ok(()) => "native input listener stopped unexpectedly".to_owned(),
                Err(error) => format!("native input listener failed: {error:?}"),
            };
            if start_tx.send(message.clone()).is_err() {
                error!(%message, "automatic-correction native listener stopped");
            }
        })
    {
        state.status.store(LISTENER_FAILED, Ordering::Release);
        return Err(error).context("failed to start the automatic-correction listener thread");
    }

    match start_rx.recv_timeout(START_TIMEOUT) {
        Ok(error) => bail!(error),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            if state
                .status
                .compare_exchange(
                    LISTENER_STARTING,
                    LISTENER_RUNNING,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                Ok(())
            } else {
                bail!("automatic-correction listener failed during initialization")
            }
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            state.status.store(LISTENER_FAILED, Ordering::Release);
            bail!("automatic-correction listener exited during initialization")
        }
    }
}

fn dispatch_key(state: &ListenerState, key: Keycode, shifted: bool) {
    let Ok(mut subscription) = state.subscription.lock() else {
        return;
    };
    let Some(subscription) = subscription.as_mut() else {
        return;
    };
    if !subscription.suspended.load(Ordering::Relaxed) {
        (subscription.on_key_down)(AutoKeyEvent { key, shifted });
    }
}

fn map_key(key: Key) -> Option<Keycode> {
    Some(match key {
        Key::Backspace => Keycode::Backspace,
        Key::CapsLock => Keycode::CapsLock,
        Key::ControlLeft => Keycode::LControl,
        Key::ControlRight => Keycode::RControl,
        Key::Delete => Keycode::Delete,
        Key::DownArrow => Keycode::Down,
        Key::End => Keycode::End,
        Key::Escape => Keycode::Escape,
        Key::F1 => Keycode::F1,
        Key::F2 => Keycode::F2,
        Key::F3 => Keycode::F3,
        Key::F4 => Keycode::F4,
        Key::F5 => Keycode::F5,
        Key::F6 => Keycode::F6,
        Key::F7 => Keycode::F7,
        Key::F8 => Keycode::F8,
        Key::F9 => Keycode::F9,
        Key::F10 => Keycode::F10,
        Key::F11 => Keycode::F11,
        Key::F12 => Keycode::F12,
        Key::Home => Keycode::Home,
        Key::LeftArrow => Keycode::Left,
        Key::MetaLeft => Keycode::LMeta,
        Key::MetaRight => Keycode::RMeta,
        Key::PageDown => Keycode::PageDown,
        Key::PageUp => Keycode::PageUp,
        Key::Return => Keycode::Enter,
        Key::RightArrow => Keycode::Right,
        Key::ShiftLeft => Keycode::LShift,
        Key::ShiftRight => Keycode::RShift,
        Key::Space => Keycode::Space,
        Key::Tab => Keycode::Tab,
        Key::UpArrow => Keycode::Up,
        Key::Num0 => Keycode::Key0,
        Key::Num1 => Keycode::Key1,
        Key::Num2 => Keycode::Key2,
        Key::Num3 => Keycode::Key3,
        Key::Num4 => Keycode::Key4,
        Key::Num5 => Keycode::Key5,
        Key::Num6 => Keycode::Key6,
        Key::Num7 => Keycode::Key7,
        Key::Num8 => Keycode::Key8,
        Key::Num9 => Keycode::Key9,
        Key::BackQuote => Keycode::Grave,
        Key::Minus => Keycode::Minus,
        Key::Equal => Keycode::Equal,
        Key::KeyQ => Keycode::Q,
        Key::KeyW => Keycode::W,
        Key::KeyE => Keycode::E,
        Key::KeyR => Keycode::R,
        Key::KeyT => Keycode::T,
        Key::KeyY => Keycode::Y,
        Key::KeyU => Keycode::U,
        Key::KeyI => Keycode::I,
        Key::KeyO => Keycode::O,
        Key::KeyP => Keycode::P,
        Key::LeftBracket => Keycode::LeftBracket,
        Key::RightBracket => Keycode::RightBracket,
        Key::KeyA => Keycode::A,
        Key::KeyS => Keycode::S,
        Key::KeyD => Keycode::D,
        Key::KeyF => Keycode::F,
        Key::KeyG => Keycode::G,
        Key::KeyH => Keycode::H,
        Key::KeyJ => Keycode::J,
        Key::KeyK => Keycode::K,
        Key::KeyL => Keycode::L,
        Key::SemiColon => Keycode::Semicolon,
        Key::Quote => Keycode::Apostrophe,
        Key::BackSlash | Key::IntlBackslash => Keycode::BackSlash,
        Key::KeyZ => Keycode::Z,
        Key::KeyX => Keycode::X,
        Key::KeyC => Keycode::C,
        Key::KeyV => Keycode::V,
        Key::KeyB => Keycode::B,
        Key::KeyN => Keycode::N,
        Key::KeyM => Keycode::M,
        Key::Comma => Keycode::Comma,
        Key::Dot => Keycode::Dot,
        Key::Slash => Keycode::Slash,
        Key::Insert => Keycode::Insert,
        Key::KpReturn => Keycode::NumpadEnter,
        Key::KpMinus => Keycode::NumpadSubtract,
        Key::KpPlus => Keycode::NumpadAdd,
        Key::KpMultiply => Keycode::NumpadMultiply,
        Key::KpDivide => Keycode::NumpadDivide,
        Key::Kp0 => Keycode::Numpad0,
        Key::Kp1 => Keycode::Numpad1,
        Key::Kp2 => Keycode::Numpad2,
        Key::Kp3 => Keycode::Numpad3,
        Key::Kp4 => Keycode::Numpad4,
        Key::Kp5 => Keycode::Numpad5,
        Key::Kp6 => Keycode::Numpad6,
        Key::Kp7 => Keycode::Numpad7,
        Key::Kp8 => Keycode::Numpad8,
        Key::Kp9 => Keycode::Numpad9,
        Key::KpDelete => Keycode::NumpadDecimal,
        Key::Alt => Keycode::LAlt,
        Key::AltGr => Keycode::RAlt,
        Key::PrintScreen
        | Key::ScrollLock
        | Key::Pause
        | Key::NumLock
        | Key::Function
        | Key::Unknown(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_physical_letters_and_ukrainian_punctuation_positions() {
        assert_eq!(map_key(Key::KeyG), Some(Keycode::G));
        assert_eq!(map_key(Key::LeftBracket), Some(Keycode::LeftBracket));
        assert_eq!(map_key(Key::Quote), Some(Keycode::Apostrophe));
    }

    #[test]
    fn ignores_keys_that_cannot_affect_typed_text() {
        assert_eq!(map_key(Key::PrintScreen), None);
        assert_eq!(map_key(Key::Unknown(9000)), None);
    }
}
