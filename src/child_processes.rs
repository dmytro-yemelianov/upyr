//! Short-lived helper processes (a sound player, `open`, the Settings app).
//!
//! A long-running app that never waits for its children leaves each finished
//! one behind as a zombie until it exits; with key sounds on Linux that was one
//! per keystroke. Helpers are tracked here and reaped on the next launch.

use std::process::Child;
use std::sync::Mutex;

/// Finished children are reaped whenever another is tracked or counted.
#[derive(Default)]
pub(crate) struct Reaper {
    running: Vec<Child>,
}

impl Reaper {
    /// Reaps every finished child and returns how many are still running.
    pub fn reap(&mut self) -> usize {
        self.running
            .retain_mut(|child| matches!(child.try_wait(), Ok(None)));
        self.running.len()
    }

    pub fn track(&mut self, child: Child) {
        self.reap();
        self.running.push(child);
    }
}

static HELPERS: Mutex<Reaper> = Mutex::new(Reaper {
    running: Vec::new(),
});

fn helpers() -> std::sync::MutexGuard<'static, Reaper> {
    HELPERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Hands a spawned helper over so it is reaped once it exits.
pub(crate) fn track(child: Child) {
    helpers().track(child);
}

/// Helpers still running, after reaping the finished ones.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn running() -> usize {
    helpers().reap()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{Duration, Instant};

    #[test]
    fn finished_helpers_are_reaped_and_running_ones_are_kept() {
        let mut reaper = Reaper::default();
        for _ in 0..3 {
            reaper.track(Command::new("true").spawn().unwrap());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while reaper.reap() > 0 {
            assert!(Instant::now() < deadline, "helpers were never reaped");
            std::thread::sleep(Duration::from_millis(10));
        }

        let mut long = Command::new("sleep").arg("5").spawn().unwrap();
        let id = long.id();
        long.kill().unwrap();
        // A killed child that is not waited for would remain a zombie.
        reaper.track(long);
        let deadline = Instant::now() + Duration::from_secs(5);
        while reaper.reap() > 0 {
            assert!(
                Instant::now() < deadline,
                "killed helper {id} was never reaped"
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        let running = Command::new("sleep").arg("5").spawn().unwrap();
        reaper.track(running);
        assert_eq!(reaper.reap(), 1, "a running helper must be kept");
        for child in &mut reaper.running {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
