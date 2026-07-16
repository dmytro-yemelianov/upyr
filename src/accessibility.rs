use std::{
    env,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Context, Result};
use macos_accessibility_client::accessibility::{
    application_is_trusted, application_is_trusted_with_prompt,
};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication};
use objc2_foundation::NSString;

const POLL_INTERVAL: Duration = Duration::from_millis(500);
static PERMISSION_REQUEST: PermissionRequestGate = PermissionRequestGate::new();

struct PermissionRequestGate {
    requested: AtomicBool,
}

impl PermissionRequestGate {
    const fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
        }
    }

    fn begin_if_needed(&self, trusted: bool) -> bool {
        !trusted
            && self
                .requested
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }
}

pub struct AccessibilityWatcher {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl AccessibilityWatcher {
    pub fn start(on_granted: impl FnOnce() + Send + 'static) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("upyr-accessibility-watcher".to_owned())
            .spawn(move || {
                while !thread_stop.load(Ordering::Relaxed) {
                    if application_is_trusted() {
                        on_granted();
                        return;
                    }
                    thread::sleep(POLL_INTERVAL);
                }
            })
            .context("failed to start the Accessibility permission watcher")?;

        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }
}

impl Drop for AccessibilityWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn is_trusted() -> bool {
    application_is_trusted()
}

/// Requests Accessibility at most once during this process. The trust check
/// always happens first, so an existing grant never opens a macOS prompt.
pub fn request_once_if_needed() -> bool {
    let trusted = is_trusted();
    if trusted {
        return true;
    }
    if PERMISSION_REQUEST.begin_if_needed(false) {
        application_is_trusted_with_prompt()
    } else {
        false
    }
}

/// Shows the restart choice on AppKit's main thread.
pub fn prompt_for_restart() -> bool {
    let Some(main_thread) = MainThreadMarker::new() else {
        return false;
    };
    let application = NSApplication::sharedApplication(main_thread);
    application.activate();

    let alert = NSAlert::new(main_thread);
    alert.setAlertStyle(NSAlertStyle::Informational);
    alert.setMessageText(&NSString::from_str("Accessibility access granted"));
    alert.setInformativeText(&NSString::from_str(
        "Restart Upyr now to activate automatic correction and keyboard monitoring.",
    ));
    alert.addButtonWithTitle(&NSString::from_str("Restart Upyr"));
    alert.addButtonWithTitle(&NSString::from_str("Later"));

    alert.runModal() == NSAlertFirstButtonReturn
}

/// Starts a detached helper that waits for this process to release the
/// single-instance lock, then reopens the same app bundle (or executable when
/// running an unpackaged development build).
pub fn schedule_relaunch() -> Result<()> {
    let executable = env::current_exe().context("could not locate the Upyr executable")?;
    let pid = std::process::id().to_string();
    let (script, target) = match app_bundle(&executable) {
        Some(bundle) => (
            "while kill -0 \"$1\" 2>/dev/null; do sleep 0.1; done; exec /usr/bin/open -n \"$2\"",
            bundle,
        ),
        None => (
            "while kill -0 \"$1\" 2>/dev/null; do sleep 0.1; done; exec \"$2\"",
            executable,
        ),
    };

    Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg("upyr-relaunch")
        .arg(pid)
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to schedule relaunch of {}", target.display()))?;
    Ok(())
}

fn app_bundle(executable: &std::path::Path) -> Option<PathBuf> {
    let bundle = executable.parent()?.parent()?.parent()?;
    (bundle.extension().and_then(|value| value.to_str()) == Some("app"))
        .then(|| bundle.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn finds_packaged_app_bundle() {
        let executable = Path::new("/Applications/Upyr.app/Contents/MacOS/upyr-background");

        assert_eq!(
            app_bundle(executable),
            Some(PathBuf::from("/Applications/Upyr.app"))
        );
    }

    #[test]
    fn rejects_unpacked_executable() {
        assert_eq!(app_bundle(Path::new("/tmp/upyr-background")), None);
    }

    #[test]
    fn permission_request_gate_skips_existing_grants_and_repeats() {
        let gate = PermissionRequestGate::new();

        assert!(!gate.begin_if_needed(true));
        assert!(gate.begin_if_needed(false));
        assert!(!gate.begin_if_needed(false));
    }
}
