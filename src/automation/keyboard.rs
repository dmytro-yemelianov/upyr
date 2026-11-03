//! One keyboard connection for every conversion on the main thread.
//!
//! Dropping an `enigo::Enigo` sleeps about 20 ms for each event it posted
//! recently, so that the events are not lost with the connection. A
//! connection per conversion therefore stalled the event loop when the
//! conversion ended: about a second after selecting a 30-character word.
//! A shared connection is never dropped while Upyr runs.

use anyhow::Result;
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

/// Releases whatever keys are still pressed through this connection.
pub(crate) trait ReleaseHeld {
    fn release_held(&mut self);
}

/// A connection created on first use and lent to each task afterwards.
pub(crate) struct Shared<T>(RefCell<Option<Rc<RefCell<T>>>>);

impl<T> Default for Shared<T> {
    fn default() -> Self {
        Self(RefCell::new(None))
    }
}

impl<T: ReleaseHeld> Shared<T> {
    /// Lends the connection, creating it with `connect` the first time. A
    /// failed attempt is retried by the next task.
    pub fn lease(&self, connect: impl FnOnce() -> Result<T>) -> Result<Lease<T>> {
        let mut slot = self.0.borrow_mut();
        if let Some(keyboard) = slot.as_ref() {
            return Ok(Lease(Rc::clone(keyboard)));
        }
        let keyboard = Rc::new(RefCell::new(connect()?));
        *slot = Some(Rc::clone(&keyboard));
        Ok(Lease(keyboard))
    }
}

/// One task's use of the shared connection. Keys the task left pressed, after
/// an error halfway through a chord, are released when the lease ends, as a
/// dropped connection used to release them.
pub(crate) struct Lease<T: ReleaseHeld>(Rc<RefCell<T>>);

impl<T: ReleaseHeld> Lease<T> {
    pub fn get(&self) -> RefMut<'_, T> {
        self.0.borrow_mut()
    }
}

impl<T: ReleaseHeld> Drop for Lease<T> {
    fn drop(&mut self) {
        if let Ok(mut keyboard) = self.0.try_borrow_mut() {
            keyboard.release_held();
        }
    }
}

#[cfg(target_os = "macos")]
impl ReleaseHeld for enigo::Enigo {
    fn release_held(&mut self) {
        use enigo::{Direction, Keyboard};
        let (keys, keycodes) = self.held();
        for key in keys {
            if let Err(error) = self.key(key, Direction::Release) {
                tracing::debug!(%error, ?key, "could not release a held key");
            }
        }
        for keycode in keycodes {
            if let Err(error) = self.raw(keycode, Direction::Release) {
                tracing::debug!(%error, keycode, "could not release a held key");
            }
        }
    }
}

#[cfg(target_os = "macos")]
thread_local! {
    static KEYBOARD: Shared<enigo::Enigo> = Shared::default();
}

/// The main thread's keyboard connection.
#[cfg(target_os = "macos")]
pub(crate) fn lease() -> Result<Lease<enigo::Enigo>> {
    use anyhow::Context;
    KEYBOARD.with(|keyboard| {
        keyboard.lease(|| {
            enigo::Enigo::new(&enigo::Settings::default())
                .context("could not connect to desktop input")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[derive(Default)]
    struct Keys {
        held: usize,
        released: usize,
    }

    impl ReleaseHeld for Keys {
        fn release_held(&mut self) {
            self.released += self.held;
            self.held = 0;
        }
    }

    #[test]
    fn every_task_borrows_the_one_connection() {
        let shared = Shared::default();
        let connections = Cell::new(0);
        let connect = || {
            connections.set(connections.get() + 1);
            Ok(Keys::default())
        };
        let first = shared.lease(connect).unwrap();
        drop(first);
        let second = shared.lease(connect).unwrap();
        let queued = shared.lease(connect).unwrap();
        assert_eq!(connections.get(), 1);
        second.get().held = 1;
        assert_eq!(queued.get().held, 1, "both leases see the same keys");
    }

    #[test]
    fn a_task_releases_the_keys_it_left_pressed() {
        let shared = Shared::default();
        let task = shared.lease(|| Ok(Keys::default())).unwrap();
        task.get().held = 2; // Shift and Option, after a failed chord.
        drop(task);
        let next = shared.lease(|| unreachable!()).unwrap();
        assert_eq!(next.get().held, 0);
        assert_eq!(next.get().released, 2);
    }

    #[test]
    fn a_failed_connection_is_retried_by_the_next_task() {
        let shared: Shared<Keys> = Shared::default();
        assert!(shared.lease(|| anyhow::bail!("no permission")).is_err());
        assert!(shared.lease(|| Ok(Keys::default())).is_ok());
    }
}
