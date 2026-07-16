use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const SAVE_DEBOUNCE: Duration = Duration::from_millis(150);

pub struct ConfigWatcher {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ConfigWatcher {
    pub fn start(path: PathBuf, mut on_change: impl FnMut() + Send + 'static) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("upyr-config-watcher".to_owned())
            .spawn(move || {
                let mut previous = signature(&path);
                while !thread_stop.load(Ordering::Relaxed) {
                    thread::sleep(POLL_INTERVAL);
                    let current = signature(&path);
                    if current != previous {
                        previous = current;
                        thread::sleep(SAVE_DEBOUNCE);
                        on_change();
                    }
                }
            })
            .context("failed to start the configuration watcher")?;

        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileSignature {
    modified: Option<SystemTime>,
    length: u64,
}

fn signature(path: &Path) -> Option<FileSignature> {
    let metadata = fs::metadata(path).ok()?;
    Some(FileSignature {
        modified: metadata.modified().ok(),
        length: metadata.len(),
    })
}
