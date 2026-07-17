use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use device_query::Keycode;
#[cfg(target_os = "macos")]
use device_query::{DeviceQuery, DeviceState};
use rdev::{EventType, Key};
use tracing::error;

use crate::auto_correct::{AutoKeyEvent, AutoWordTracker};

const START_TIMEOUT: Duration = Duration::from_millis(250);
const START_POLL_INTERVAL: Duration = Duration::from_millis(2);
const LISTENER_IDLE: u8 = 0;
const LISTENER_STARTING: u8 = 1;
const LISTENER_RUNNING: u8 = 2;
const LISTENER_FAILED: u8 = 3;
const MISSING_LEFT_CONTROL: u8 = 1 << 0;
const MISSING_RIGHT_CONTROL: u8 = 1 << 1;
const MISSING_LEFT_OPTION: u8 = 1 << 2;
const MISSING_RIGHT_OPTION: u8 = 1 << 3;
const MISSING_LEFT_META: u8 = 1 << 4;
const MISSING_RIGHT_META: u8 = 1 << 5;
const MAX_DEFERRED_TEXT_EVENTS: usize = 128;

type KeyCallback = Box<dyn FnMut(AutoKeyEvent) + Send + 'static>;

struct Subscription {
    id: u64,
    suspended: bool,
    deferred_text_events: VecDeque<AutoKeyEvent>,
    deferred_overflowed: bool,
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct CaptureState {
    left_shift: bool,
    right_shift: bool,
    shift_release_pending: bool,
    snapshot_missing_chords: u8,
    left_control: bool,
    right_control: bool,
    left_option: bool,
    right_option: bool,
    left_meta: bool,
    right_meta: bool,
    caps_lock: bool,
}

impl CaptureState {
    #[cfg(any(target_os = "macos", test))]
    fn from_pressed_keys(keys: &[Keycode]) -> Self {
        Self {
            left_shift: keys.contains(&Keycode::LShift),
            right_shift: keys.contains(&Keycode::RShift),
            shift_release_pending: false,
            snapshot_missing_chords: 0,
            left_control: keys.contains(&Keycode::LControl),
            right_control: keys.contains(&Keycode::RControl),
            left_option: keys.contains(&Keycode::LAlt) || keys.contains(&Keycode::LOption),
            right_option: keys.contains(&Keycode::RAlt) || keys.contains(&Keycode::ROption),
            left_meta: keys.contains(&Keycode::Command) || keys.contains(&Keycode::LMeta),
            right_meta: keys.contains(&Keycode::RCommand) || keys.contains(&Keycode::RMeta),
            caps_lock: keys.contains(&Keycode::CapsLock),
        }
    }

    #[cfg(any(target_os = "macos", test))]
    fn synchronize_momentary_modifiers(&mut self, keys: &[Keycode], name: Option<&str>) {
        let left_shift = keys.contains(&Keycode::LShift);
        let right_shift = keys.contains(&Keycode::RShift);
        if left_shift || right_shift {
            self.left_shift = left_shift;
            self.right_shift = right_shift;
            self.shift_release_pending = false;
        } else if rendered_character_is_lowercase(name) {
            // A lowercase rendered letter proves that a previously reported
            // Shift press is stale. Uppercase keeps the event-tap state: the
            // device snapshot can lag a physically held Shift key.
            self.left_shift = false;
            self.right_shift = false;
            self.shift_release_pending = false;
        }

        self.snapshot_missing_chords = 0;
        reconcile_chord_modifier(
            &mut self.left_control,
            keys.contains(&Keycode::LControl),
            &mut self.snapshot_missing_chords,
            MISSING_LEFT_CONTROL,
        );
        reconcile_chord_modifier(
            &mut self.right_control,
            keys.contains(&Keycode::RControl),
            &mut self.snapshot_missing_chords,
            MISSING_RIGHT_CONTROL,
        );
        reconcile_chord_modifier(
            &mut self.left_option,
            keys.contains(&Keycode::LAlt) || keys.contains(&Keycode::LOption),
            &mut self.snapshot_missing_chords,
            MISSING_LEFT_OPTION,
        );
        reconcile_chord_modifier(
            &mut self.right_option,
            keys.contains(&Keycode::RAlt) || keys.contains(&Keycode::ROption),
            &mut self.snapshot_missing_chords,
            MISSING_RIGHT_OPTION,
        );
        reconcile_chord_modifier(
            &mut self.left_meta,
            keys.contains(&Keycode::Command) || keys.contains(&Keycode::LMeta),
            &mut self.snapshot_missing_chords,
            MISSING_LEFT_META,
        );
        reconcile_chord_modifier(
            &mut self.right_meta,
            keys.contains(&Keycode::RCommand) || keys.contains(&Keycode::RMeta),
            &mut self.snapshot_missing_chords,
            MISSING_RIGHT_META,
        );
    }

    #[cfg(any(target_os = "macos", test))]
    fn reconcile_shift_release(&mut self, keys: &[Keycode]) {
        // Enigo and the user can hold the same logical Shift at once. When
        // Enigo releases its synthetic press, Quartz still emits a release
        // transition even though the physical key remains down. Trust the
        // hardware snapshot after the release so the user's next capital is
        // not mistaken for Caps Lock.
        self.left_shift = keys.contains(&Keycode::LShift);
        self.right_shift = keys.contains(&Keycode::RShift);
        self.shift_release_pending = !self.left_shift && !self.right_shift;
    }

    fn observe(&mut self, event_type: EventType, name: Option<&str>) -> Option<AutoKeyEvent> {
        match event_type {
            EventType::KeyPress(Key::ShiftLeft) => {
                self.left_shift = true;
                self.shift_release_pending = false;
                Some(reset_event(Keycode::LShift))
            }
            EventType::KeyPress(Key::ShiftRight) => {
                self.right_shift = true;
                self.shift_release_pending = false;
                Some(reset_event(Keycode::RShift))
            }
            EventType::KeyRelease(Key::ShiftLeft) => {
                self.left_shift = false;
                None
            }
            EventType::KeyRelease(Key::ShiftRight) => {
                self.right_shift = false;
                None
            }
            EventType::KeyPress(Key::ControlLeft) => {
                self.left_control = true;
                Some(reset_event(Keycode::LControl))
            }
            EventType::KeyPress(Key::ControlRight) => {
                self.right_control = true;
                Some(reset_event(Keycode::RControl))
            }
            EventType::KeyRelease(Key::ControlLeft) => {
                self.left_control = false;
                None
            }
            EventType::KeyRelease(Key::ControlRight) => {
                self.right_control = false;
                None
            }
            EventType::KeyPress(Key::Alt) => {
                self.left_option = true;
                Some(reset_event(Keycode::LAlt))
            }
            EventType::KeyPress(Key::AltGr) => {
                self.right_option = true;
                Some(reset_event(Keycode::RAlt))
            }
            EventType::KeyRelease(Key::Alt) => {
                self.left_option = false;
                None
            }
            EventType::KeyRelease(Key::AltGr) => {
                self.right_option = false;
                None
            }
            EventType::KeyPress(Key::MetaLeft) => {
                self.left_meta = true;
                Some(reset_event(Keycode::LMeta))
            }
            EventType::KeyPress(Key::MetaRight) => {
                self.right_meta = true;
                Some(reset_event(Keycode::RMeta))
            }
            EventType::KeyRelease(Key::MetaLeft) => {
                self.left_meta = false;
                None
            }
            EventType::KeyRelease(Key::MetaRight) => {
                self.right_meta = false;
                None
            }
            EventType::KeyPress(Key::CapsLock) => {
                #[cfg(target_os = "macos")]
                {
                    // Quartz reports activation as a press and deactivation as
                    // a release because both are flags-changed events.
                    self.caps_lock = true;
                }
                #[cfg(not(target_os = "macos"))]
                {
                    // Other rdev backends report the physical press/release
                    // pair, so the logical lock state changes on each press.
                    self.caps_lock = !self.caps_lock;
                }
                Some(reset_event(Keycode::CapsLock))
            }
            EventType::KeyRelease(Key::CapsLock) => {
                #[cfg(target_os = "macos")]
                {
                    self.caps_lock = false;
                    Some(reset_event(Keycode::CapsLock))
                }
                #[cfg(not(target_os = "macos"))]
                {
                    None
                }
            }
            EventType::KeyPress(key) => self.capture_key(key, name),
            _ => None,
        }
    }

    fn capture_key(&mut self, key: Key, name: Option<&str>) -> Option<AutoKeyEvent> {
        if self.chord_modifier_active() {
            self.shift_release_pending = false;
            self.clear_snapshot_missing_chords();
            return None;
        }

        let shifted = self.left_shift
            || self.right_shift
            || (self.shift_release_pending && rendered_character_is_uppercase(name));
        self.shift_release_pending = false;
        if let Some(caps_lock) = infer_caps_lock(name, shifted) {
            let newly_detected = caps_lock && !self.caps_lock;
            self.caps_lock = caps_lock;
            if newly_detected {
                // The OS-rendered character exposes a Caps Lock state that may
                // predate listener startup and therefore has no press event.
                return Some(reset_event(Keycode::CapsLock));
            }
        }
        if self.caps_lock {
            return None;
        }

        map_key(key).map(|key| AutoKeyEvent { key, shifted })
    }

    fn chord_modifier_active(&self) -> bool {
        self.left_control
            || self.right_control
            || self.left_option
            || self.right_option
            || self.left_meta
            || self.right_meta
    }

    fn clear_snapshot_missing_chords(&mut self) {
        if self.snapshot_missing_chords & MISSING_LEFT_CONTROL != 0 {
            self.left_control = false;
        }
        if self.snapshot_missing_chords & MISSING_RIGHT_CONTROL != 0 {
            self.right_control = false;
        }
        if self.snapshot_missing_chords & MISSING_LEFT_OPTION != 0 {
            self.left_option = false;
        }
        if self.snapshot_missing_chords & MISSING_RIGHT_OPTION != 0 {
            self.right_option = false;
        }
        if self.snapshot_missing_chords & MISSING_LEFT_META != 0 {
            self.left_meta = false;
        }
        if self.snapshot_missing_chords & MISSING_RIGHT_META != 0 {
            self.right_meta = false;
        }
        self.snapshot_missing_chords = 0;
    }

    #[cfg(any(target_os = "macos", test))]
    fn momentary_modifier_active(&self) -> bool {
        self.left_shift
            || self.right_shift
            || self.shift_release_pending
            || self.chord_modifier_active()
    }
}

fn reset_event(key: Keycode) -> AutoKeyEvent {
    AutoKeyEvent {
        key,
        shifted: false,
    }
}

fn infer_caps_lock(name: Option<&str>, shifted: bool) -> Option<bool> {
    let mut characters = name?.chars();
    let character = characters.next()?;
    if characters.next().is_some() {
        return None;
    }
    if character.is_uppercase() {
        Some(!shifted)
    } else if character.is_lowercase() {
        Some(shifted)
    } else {
        None
    }
}

#[cfg(any(target_os = "macos", test))]
fn rendered_character_is_lowercase(name: Option<&str>) -> bool {
    let Some(name) = name else {
        return false;
    };
    let mut characters = name.chars();
    characters.next().is_some_and(char::is_lowercase) && characters.next().is_none()
}

fn rendered_character_is_uppercase(name: Option<&str>) -> bool {
    let Some(name) = name else {
        return false;
    };
    let mut characters = name.chars();
    characters.next().is_some_and(char::is_uppercase) && characters.next().is_none()
}

#[cfg(any(target_os = "macos", test))]
fn reconcile_chord_modifier(current: &mut bool, present: bool, missing: &mut u8, bit: u8) {
    if present {
        *current = true;
    } else if *current {
        *missing |= bit;
    }
}

#[cfg(target_os = "macos")]
fn initial_capture_state(device: Option<&DeviceState>) -> CaptureState {
    device.map_or_else(CaptureState::default, |device| {
        CaptureState::from_pressed_keys(&device.get_keys())
    })
}

#[cfg(not(target_os = "macos"))]
fn initial_capture_state() -> CaptureState {
    CaptureState::default()
}

#[cfg(target_os = "macos")]
fn is_ordinary_key_press(event_type: EventType) -> bool {
    matches!(
        event_type,
        EventType::KeyPress(key)
            if !matches!(
                key,
                Key::ShiftLeft
                    | Key::ShiftRight
                    | Key::ControlLeft
                    | Key::ControlRight
                    | Key::Alt
                    | Key::AltGr
                    | Key::MetaLeft
                    | Key::MetaRight
                    | Key::CapsLock
            )
    )
}

pub struct AutoCorrectMonitor {
    state: Arc<ListenerState>,
    subscription_id: u64,
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
        let subscription = Subscription {
            id: subscription_id,
            suspended: false,
            deferred_text_events: VecDeque::new(),
            deferred_overflowed: false,
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
        }))
    }

    pub fn set_suspended(&self, suspended: bool) {
        let Ok(mut subscription) = self.state.subscription.lock() else {
            return;
        };
        let Some(subscription) = subscription
            .as_mut()
            .filter(|current| current.id == self.subscription_id)
        else {
            return;
        };

        if suspended {
            subscription.deferred_text_events.clear();
            subscription.deferred_overflowed = false;
            subscription.suspended = true;
            return;
        }

        subscription.suspended = false;
        if subscription.deferred_overflowed {
            subscription.deferred_text_events.clear();
            subscription.deferred_overflowed = false;
            (subscription.on_key_down)(reset_event(Keycode::Escape));
            return;
        }
        while let Some(event) = subscription.deferred_text_events.pop_front() {
            (subscription.on_key_down)(event);
        }
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
            #[cfg(target_os = "macos")]
            let device_state = DeviceState::checked_new();
            #[cfg(target_os = "macos")]
            let mut capture_state = initial_capture_state(device_state.as_ref());
            #[cfg(not(target_os = "macos"))]
            let mut capture_state = initial_capture_state();
            let callback_state = Arc::clone(&listener_state);
            let result = rdev::listen(move |event| {
                #[cfg(target_os = "macos")]
                if capture_state.momentary_modifier_active()
                    && is_ordinary_key_press(event.event_type)
                {
                    if let Some(device) = device_state.as_ref() {
                        // rdev's first flags-changed event can classify release as
                        // press when the key predated its event tap. Reconcile only
                        // while a modifier appears active, keeping normal typing on
                        // the zero-query fast path.
                        capture_state.synchronize_momentary_modifiers(
                            &device.get_keys(),
                            event.name.as_deref(),
                        );
                    }
                }
                let captured = capture_state.observe(event.event_type, event.name.as_deref());
                #[cfg(target_os = "macos")]
                if matches!(
                    event.event_type,
                    EventType::KeyRelease(Key::ShiftLeft | Key::ShiftRight)
                ) {
                    if let Some(device) = device_state.as_ref() {
                        capture_state.reconcile_shift_release(&device.get_keys());
                    }
                }
                if let Some(captured) = captured {
                    dispatch_key(&callback_state, captured);
                }
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

fn dispatch_key(state: &ListenerState, event: AutoKeyEvent) {
    let Ok(mut subscription) = state.subscription.lock() else {
        return;
    };
    let Some(subscription) = subscription.as_mut() else {
        return;
    };
    if subscription.suspended {
        if AutoWordTracker::can_begin(event.key)
            || matches!(event.key, Keycode::Space | Keycode::Backspace)
        {
            if subscription.deferred_text_events.len() < MAX_DEFERRED_TEXT_EVENTS {
                subscription.deferred_text_events.push_back(event);
            } else {
                subscription.deferred_text_events.clear();
                subscription.deferred_overflowed = true;
            }
        }
        return;
    }
    (subscription.on_key_down)(event);
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

    fn test_monitor() -> (AutoCorrectMonitor, Arc<Mutex<Vec<AutoKeyEvent>>>) {
        let state = Arc::new(ListenerState::default());
        let received = Arc::new(Mutex::new(Vec::new()));
        let callback_events = Arc::clone(&received);
        let subscription_id = 1;
        *state.subscription.lock().expect("test listener lock") = Some(Subscription {
            id: subscription_id,
            suspended: false,
            deferred_text_events: VecDeque::new(),
            deferred_overflowed: false,
            on_key_down: Box::new(move |event| {
                callback_events
                    .lock()
                    .expect("test callback lock")
                    .push(event);
            }),
        });
        (
            AutoCorrectMonitor {
                state,
                subscription_id,
            },
            received,
        )
    }

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

    #[test]
    fn deferred_user_text_survives_later_synthetic_resets() {
        let (monitor, received) = test_monitor();
        monitor.set_suspended(true);

        dispatch_key(&monitor.state, reset_event(Keycode::LShift));
        dispatch_key(
            &monitor.state,
            AutoKeyEvent {
                key: Keycode::H,
                shifted: true,
            },
        );
        // Paste emits a later Command reset. It must not erase the genuine H.
        dispatch_key(&monitor.state, reset_event(Keycode::LMeta));

        assert!(received.lock().expect("received lock").is_empty());
        monitor.set_suspended(false);

        assert_eq!(
            *received.lock().expect("received lock"),
            [AutoKeyEvent {
                key: Keycode::H,
                shifted: true,
            }]
        );
    }

    #[test]
    fn deferred_capital_stays_at_the_front_of_the_next_autocorrected_word() {
        use crate::{auto_correct::AutoDecision, system_layout::SystemLayout};

        for (first, suffix, expected_source, expected_replacement) in [
            (
                Keycode::G,
                &[
                    Keycode::J,
                    Keycode::L,
                    Keycode::S,
                    Keycode::Z,
                    Keycode::Space,
                ][..],
                "Gjlsz ",
                "Подія ",
            ),
            (
                Keycode::H,
                &[
                    Keycode::J,
                    Keycode::Comma,
                    Keycode::J,
                    Keycode::N,
                    Keycode::F,
                    Keycode::Space,
                ][..],
                "Hj,jnf ",
                "Робота ",
            ),
        ] {
            let (monitor, received) = test_monitor();
            monitor.set_suspended(true);
            dispatch_key(&monitor.state, reset_event(Keycode::LShift));
            dispatch_key(
                &monitor.state,
                AutoKeyEvent {
                    key: first,
                    shifted: true,
                },
            );
            assert!(received.lock().expect("received lock").is_empty());

            monitor.set_suspended(false);
            for &key in suffix {
                dispatch_key(
                    &monitor.state,
                    AutoKeyEvent {
                        key,
                        shifted: false,
                    },
                );
            }
            // Resuming an already active subscription is idempotent.
            monitor.set_suspended(false);

            let events = received.lock().expect("received lock").clone();
            assert_eq!(events.first().map(|event| event.key), Some(first));
            assert_eq!(events.iter().filter(|event| event.key == first).count(), 1);

            let mut tracker = AutoWordTracker::default();
            let mut decision = AutoDecision::Continue;
            for event in events {
                if tracker.needs_layout_check() && AutoWordTracker::can_begin(event.key) {
                    tracker.set_source_layout(Some(SystemLayout::English));
                }
                if let Some(sample) = tracker.observe(event) {
                    decision = upyr_core::evaluate(
                        &sample,
                        &upyr_core::AutoCorrectPolicy::default(),
                        None,
                    );
                }
            }

            let AutoDecision::Correct(correction) = decision else {
                panic!("expected correction after deferred capital, got {decision:?}");
            };
            assert_eq!(correction.expected_source, expected_source);
            assert_eq!(correction.replacement, expected_replacement);
        }
    }

    #[test]
    fn capital_on_either_side_of_the_resume_handoff_is_delivered_once() {
        for arrives_before_resume in [true, false] {
            let (monitor, received) = test_monitor();
            let capital = AutoKeyEvent {
                key: Keycode::H,
                shifted: true,
            };
            monitor.set_suspended(true);
            if arrives_before_resume {
                dispatch_key(&monitor.state, capital);
            }
            monitor.set_suspended(false);
            if !arrives_before_resume {
                dispatch_key(&monitor.state, capital);
            }
            monitor.set_suspended(false);

            assert_eq!(*received.lock().expect("received lock"), [capital]);
        }
    }

    #[test]
    fn deferred_overflow_replays_only_a_tracker_reset() {
        let (monitor, received) = test_monitor();
        monitor.set_suspended(true);
        for _ in 0..=MAX_DEFERRED_TEXT_EVENTS {
            dispatch_key(
                &monitor.state,
                AutoKeyEvent {
                    key: Keycode::H,
                    shifted: false,
                },
            );
        }
        monitor.set_suspended(false);

        assert_eq!(
            *received.lock().expect("received lock"),
            [reset_event(Keycode::Escape)]
        );
    }

    #[test]
    fn command_chord_resets_suppresses_and_then_recovers() {
        let mut state = CaptureState::default();

        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyA), Some("a")),
            Some(AutoKeyEvent {
                key: Keycode::A,
                shifted: false,
            })
        );
        assert_eq!(
            state.observe(EventType::KeyPress(Key::MetaLeft), None),
            Some(reset_event(Keycode::LMeta))
        );
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyC), Some("c")),
            None
        );
        assert_eq!(
            state.observe(EventType::KeyRelease(Key::MetaLeft), None),
            None
        );
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyN), Some("n")),
            Some(AutoKeyEvent {
                key: Keycode::N,
                shifted: false,
            })
        );
    }

    #[test]
    fn all_chord_modifiers_must_be_released_before_capture_resumes() {
        let mut state = CaptureState::default();

        assert_eq!(
            state.observe(EventType::KeyPress(Key::ControlLeft), None),
            Some(reset_event(Keycode::LControl))
        );
        assert_eq!(
            state.observe(EventType::KeyPress(Key::Alt), None),
            Some(reset_event(Keycode::LAlt))
        );
        state.observe(EventType::KeyRelease(Key::ControlLeft), None);
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyA), Some("a")),
            None
        );
        state.observe(EventType::KeyRelease(Key::Alt), None);
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyA), Some("a")),
            Some(AutoKeyEvent {
                key: Keycode::A,
                shifted: false,
            })
        );
    }

    #[test]
    fn live_modifier_resync_recovers_from_a_stale_release_event() {
        let mut state = CaptureState::from_pressed_keys(&[Keycode::LShift]);
        assert!(state.momentary_modifier_active());

        state.synchronize_momentary_modifiers(&[], Some("a"));

        assert!(!state.momentary_modifier_active());
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyA), Some("a")),
            Some(AutoKeyEvent {
                key: Keycode::A,
                shifted: false,
            })
        );
    }

    #[test]
    fn transient_shift_snapshot_does_not_drop_the_uppercase_key() {
        let mut state = CaptureState::default();

        assert_eq!(
            state.observe(EventType::KeyPress(Key::ShiftLeft), None),
            Some(reset_event(Keycode::LShift))
        );
        // device_query can briefly lag the event tap and omit a still-held
        // Shift key. The rendered uppercase character is the tie-breaker.
        state.synchronize_momentary_modifiers(&[], Some("H"));

        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyH), Some("H")),
            Some(AutoKeyEvent {
                key: Keycode::H,
                shifted: true,
            })
        );
        state.synchronize_momentary_modifiers(&[], Some("O"));
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyO), Some("O")),
            Some(AutoKeyEvent {
                key: Keycode::O,
                shifted: true,
            })
        );
    }

    #[test]
    fn physical_shift_survives_an_overlapping_synthetic_release() {
        let mut state = CaptureState::default();

        state.observe(EventType::KeyPress(Key::ShiftLeft), None);
        state.observe(EventType::KeyRelease(Key::ShiftLeft), None);
        state.reconcile_shift_release(&[Keycode::LShift]);

        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyG), Some("G")),
            Some(AutoKeyEvent {
                key: Keycode::G,
                shifted: true,
            })
        );
    }

    #[test]
    fn uppercase_resolves_an_empty_snapshot_after_synthetic_shift_release() {
        let mut state = CaptureState::default();

        state.observe(EventType::KeyPress(Key::ShiftLeft), None);
        state.observe(EventType::KeyRelease(Key::ShiftLeft), None);
        state.reconcile_shift_release(&[]);
        assert!(state.momentary_modifier_active());

        // A second empty snapshot can still lag the physically held key. The
        // rendered uppercase character resolves this one-key ambiguity.
        state.synchronize_momentary_modifiers(&[], Some("H"));
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyH), Some("H")),
            Some(AutoKeyEvent {
                key: Keycode::H,
                shifted: true,
            })
        );
        assert!(!state.shift_release_pending);
    }

    #[test]
    fn lowercase_clears_an_uncertain_shift_release() {
        let mut state = CaptureState::default();

        state.observe(EventType::KeyPress(Key::ShiftLeft), None);
        state.observe(EventType::KeyRelease(Key::ShiftLeft), None);
        state.reconcile_shift_release(&[]);
        state.synchronize_momentary_modifiers(&[], Some("h"));

        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyH), Some("h")),
            Some(AutoKeyEvent {
                key: Keycode::H,
                shifted: false,
            })
        );
        assert!(!state.shift_release_pending);
    }

    #[test]
    fn transient_meta_snapshot_never_turns_a_shortcut_into_typed_text() {
        let mut state = CaptureState::default();

        assert_eq!(
            state.observe(EventType::KeyPress(Key::MetaLeft), None),
            Some(reset_event(Keycode::LMeta))
        );
        state.synchronize_momentary_modifiers(&[], Some("c"));
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyC), Some("c")),
            None
        );
        assert!(!state.chord_modifier_active());
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyA), Some("a")),
            Some(AutoKeyEvent {
                key: Keycode::A,
                shifted: false,
            })
        );
    }

    #[test]
    fn already_active_caps_lock_is_suppressed_and_recovers_when_off() {
        let mut state = CaptureState::from_pressed_keys(&[Keycode::CapsLock]);

        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyA), Some("A")),
            None
        );
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyA), Some("a")),
            Some(AutoKeyEvent {
                key: Keycode::A,
                shifted: false,
            })
        );
    }

    #[test]
    fn rendered_ukrainian_case_detects_caps_lock_before_capture() {
        let mut state = CaptureState::default();

        assert_eq!(
            state.observe(EventType::KeyPress(Key::LeftBracket), Some("Х")),
            Some(reset_event(Keycode::CapsLock))
        );
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyA), Some("Ф")),
            None
        );
        assert_eq!(
            state.observe(EventType::KeyPress(Key::LeftBracket), Some("х")),
            Some(AutoKeyEvent {
                key: Keycode::LeftBracket,
                shifted: false,
            })
        );
    }

    #[test]
    fn shift_is_preserved_without_becoming_a_blocker() {
        let mut state = CaptureState::default();

        assert_eq!(
            state.observe(EventType::KeyPress(Key::ShiftLeft), None),
            Some(reset_event(Keycode::LShift))
        );
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyA), Some("A")),
            Some(AutoKeyEvent {
                key: Keycode::A,
                shifted: true,
            })
        );
        state.observe(EventType::KeyRelease(Key::ShiftLeft), None);
        assert_eq!(
            state.observe(EventType::KeyPress(Key::KeyA), Some("a")),
            Some(AutoKeyEvent {
                key: Keycode::A,
                shifted: false,
            })
        );
    }
}
