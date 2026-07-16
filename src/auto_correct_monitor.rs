use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use device_query::{DeviceQuery, DeviceState, Keycode};

use crate::auto_correct::AutoKeyEvent;

const POLL_INTERVAL: Duration = Duration::from_millis(8);
const START_TIMEOUT: Duration = Duration::from_secs(5);

pub struct AutoCorrectMonitor {
    stop: Arc<AtomicBool>,
    suspended: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl AutoCorrectMonitor {
    /// Polls only while automatic correction is enabled. The held-key snapshot
    /// lives solely in this thread and no typed key or word is ever logged.
    pub fn start(
        enabled: bool,
        mut on_key_down: impl FnMut(AutoKeyEvent) + Send + 'static,
    ) -> Result<Option<Self>> {
        if !enabled {
            return Ok(None);
        }

        let stop = Arc::new(AtomicBool::new(false));
        let suspended = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_suspended = Arc::clone(&suspended);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let handle = thread::Builder::new()
            .name("upyr-auto-correct".to_owned())
            .spawn(move || {
                let Some(device) = DeviceState::checked_new() else {
                    let _ = ready_tx.send(false);
                    return;
                };
                let mut previous = device.get_keys();
                if ready_tx.send(true).is_err() {
                    return;
                }

                while !thread_stop.load(Ordering::Relaxed) {
                    let current = device.get_keys();
                    if !thread_suspended.load(Ordering::Relaxed) {
                        let shifted = current
                            .iter()
                            .any(|key| matches!(key, Keycode::LShift | Keycode::RShift));
                        for key in current.iter().filter(|key| !previous.contains(key)) {
                            on_key_down(AutoKeyEvent { key: *key, shifted });
                        }
                    }
                    previous = current;
                    thread::sleep(POLL_INTERVAL);
                }
            })
            .context("failed to start the automatic-correction monitor")?;

        let ready = match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(ready) => ready,
            Err(error) => {
                stop.store(true, Ordering::Relaxed);
                let _ = handle.join();
                return Err(error).context("automatic-correction monitor did not initialize");
            }
        };
        if !ready {
            stop.store(true, Ordering::Relaxed);
            let _ = handle.join();
            bail!(
                "automatic correction needs Accessibility permission on macOS or an active X11 display on Linux"
            );
        }

        Ok(Some(Self {
            stop,
            suspended,
            handle: Some(handle),
        }))
    }

    pub fn set_suspended(&self, suspended: bool) {
        self.suspended.store(suspended, Ordering::Relaxed);
    }
}

impl Drop for AutoCorrectMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
