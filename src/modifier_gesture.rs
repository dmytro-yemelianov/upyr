use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use device_query::{DeviceQuery, DeviceState, Keycode};

use crate::config::ModifierGesture;

const POLL_INTERVAL: Duration = Duration::from_millis(12);
const START_TIMEOUT: Duration = Duration::from_secs(5);
const CONTROL: u8 = 1 << 0;
const SHIFT: u8 = 1 << 1;
const ALT: u8 = 1 << 2;
const META: u8 = 1 << 3;

pub struct ModifierGestureMonitor {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ModifierGestureMonitor {
    /// Starts modifier-state polling only when the user explicitly enables a
    /// gesture. Key identities are reduced immediately to modifier flags and
    /// an `other key pressed` bit; they are never stored or logged.
    pub fn start(
        gesture: ModifierGesture,
        timeout: Duration,
        mut on_trigger: impl FnMut() + Send + 'static,
    ) -> Result<Option<Self>> {
        let Some(required) = required_modifiers(gesture) else {
            return Ok(None);
        };

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let handle = thread::Builder::new()
            .name("upyr-modifier-gesture".to_owned())
            .spawn(move || {
                let Some(device) = DeviceState::checked_new() else {
                    let _ = ready_tx.send(false);
                    return;
                };
                if ready_tx.send(true).is_err() {
                    return;
                }

                let started = Instant::now();
                let mut detector = GestureDetector::new(required, timeout);
                while !thread_stop.load(Ordering::Relaxed) {
                    let snapshot = KeySnapshot::from_keys(&device.get_keys());
                    if detector.update(snapshot, started.elapsed()) {
                        on_trigger();
                    }
                    thread::sleep(POLL_INTERVAL);
                }
            })
            .context("failed to start the modifier-gesture monitor")?;

        let ready = match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(ready) => ready,
            Err(error) => {
                stop.store(true, Ordering::Relaxed);
                let _ = handle.join();
                return Err(error).context("modifier-gesture monitor did not initialize");
            }
        };
        if !ready {
            stop.store(true, Ordering::Relaxed);
            let _ = handle.join();
            bail!(
                "modifier gesture needs Accessibility permission on macOS or an active X11 display on Linux"
            );
        }

        Ok(Some(Self {
            stop,
            handle: Some(handle),
        }))
    }
}

impl Drop for ModifierGestureMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn required_modifiers(gesture: ModifierGesture) -> Option<u8> {
    match gesture {
        ModifierGesture::Disabled => None,
        ModifierGesture::DoubleControl => Some(CONTROL),
        ModifierGesture::DoubleShift => Some(SHIFT),
        ModifierGesture::DoubleControlShift => Some(CONTROL | SHIFT),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct KeySnapshot {
    modifiers: u8,
    other_key: bool,
}

impl KeySnapshot {
    fn from_keys(keys: &[Keycode]) -> Self {
        let mut snapshot = Self::default();
        for key in keys {
            match key {
                Keycode::LControl | Keycode::RControl => snapshot.modifiers |= CONTROL,
                Keycode::LShift | Keycode::RShift => snapshot.modifiers |= SHIFT,
                Keycode::LAlt | Keycode::RAlt | Keycode::LOption | Keycode::ROption => {
                    snapshot.modifiers |= ALT;
                }
                Keycode::Command | Keycode::RCommand | Keycode::LMeta | Keycode::RMeta => {
                    snapshot.modifiers |= META
                }
                _ => snapshot.other_key = true,
            }
        }
        snapshot
    }
}

struct GestureDetector {
    required: u8,
    timeout: Duration,
    chord_was_down: bool,
    first_tap_at: Option<Duration>,
    trigger_on_release: bool,
}

impl GestureDetector {
    fn new(required: u8, timeout: Duration) -> Self {
        Self {
            required,
            timeout,
            chord_was_down: false,
            first_tap_at: None,
            trigger_on_release: false,
        }
    }

    fn update(&mut self, snapshot: KeySnapshot, now: Duration) -> bool {
        if snapshot.other_key || snapshot.modifiers & !self.required != 0 {
            self.chord_was_down = false;
            self.first_tap_at = None;
            self.trigger_on_release = false;
            return false;
        }

        if self
            .first_tap_at
            .is_some_and(|first| now.saturating_sub(first) > self.timeout)
        {
            self.first_tap_at = None;
        }

        let chord_is_down = snapshot.modifiers == self.required;
        if !chord_is_down {
            if self.chord_was_down {
                self.chord_was_down = false;
                if self.trigger_on_release {
                    self.trigger_on_release = false;
                    return true;
                }
                self.first_tap_at = Some(now);
            }
            return false;
        }
        if self.chord_was_down {
            return false;
        }
        self.chord_was_down = true;
        self.trigger_on_release = self.first_tap_at.take().is_some();
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(modifiers: u8) -> KeySnapshot {
        KeySnapshot {
            modifiers,
            other_key: false,
        }
    }

    #[test]
    fn triggers_after_second_distinct_chord_is_released() {
        let mut detector = GestureDetector::new(CONTROL | SHIFT, Duration::from_millis(500));

        assert!(!detector.update(snapshot(CONTROL | SHIFT), Duration::from_millis(10)));
        assert!(!detector.update(snapshot(0), Duration::from_millis(80)));
        assert!(!detector.update(snapshot(CONTROL | SHIFT), Duration::from_millis(180)));
        assert!(detector.update(snapshot(0), Duration::from_millis(240)));
    }

    #[test]
    fn holding_chord_counts_as_one_tap() {
        let mut detector = GestureDetector::new(SHIFT, Duration::from_millis(500));

        assert!(!detector.update(snapshot(SHIFT), Duration::from_millis(10)));
        assert!(!detector.update(snapshot(SHIFT), Duration::from_millis(200)));
        assert!(!detector.update(snapshot(0), Duration::from_millis(250)));
    }

    #[test]
    fn timeout_starts_a_new_sequence() {
        let mut detector = GestureDetector::new(CONTROL, Duration::from_millis(300));

        assert!(!detector.update(snapshot(CONTROL), Duration::from_millis(10)));
        assert!(!detector.update(snapshot(0), Duration::from_millis(40)));
        assert!(!detector.update(snapshot(CONTROL), Duration::from_millis(400)));
        assert!(!detector.update(snapshot(0), Duration::from_millis(430)));
    }

    #[test]
    fn other_keys_and_extra_modifiers_cancel_sequence() {
        let mut detector = GestureDetector::new(CONTROL | SHIFT, Duration::from_millis(500));
        assert!(!detector.update(snapshot(CONTROL | SHIFT), Duration::from_millis(10)));
        assert!(!detector.update(snapshot(0), Duration::from_millis(40)));
        assert!(!detector.update(
            KeySnapshot {
                modifiers: 0,
                other_key: true,
            },
            Duration::from_millis(80)
        ));
        assert!(!detector.update(snapshot(CONTROL | SHIFT), Duration::from_millis(100)));

        assert!(!detector.update(snapshot(0), Duration::from_millis(120)));
        assert!(!detector.update(snapshot(CONTROL | SHIFT | ALT), Duration::from_millis(150)));
        assert!(!detector.update(snapshot(CONTROL | SHIFT), Duration::from_millis(180)));
        assert!(!detector.update(snapshot(0), Duration::from_millis(200)));
    }
}
