#![allow(unsafe_code)]

mod focus;

use super::{ApplicationLayoutRule, FocusSession, preferred_layout};
use anyhow::{Context, Result};
use block2::RcBlock;
use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{
    NSRunningApplication, NSWorkspace, NSWorkspaceDidActivateApplicationNotification,
    NSWorkspaceDidLaunchApplicationNotification, NSWorkspaceDidTerminateApplicationNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue};
use std::{cell::RefCell, collections::HashMap, ptr::NonNull, rc::Rc};

struct State {
    rules: Vec<ApplicationLayoutRule>,
    session: FocusSession,
}

impl State {
    fn observe(&mut self, pid: Option<i32>) {
        if !self.session.changed(pid) {
            return;
        }
        let Some(app) = pid.and_then(NSRunningApplication::runningApplicationWithProcessIdentifier)
        else {
            return;
        };
        let Some(layout) = layout_for(&self.rules, &app) else {
            return;
        };
        match crate::system_layout::switch_to(layout) {
            Ok(crate::system_layout::SwitchOutcome::TargetUnavailable) => {
                tracing::warn!(?layout, "application layout is not installed");
            }
            Err(error) => tracing::error!(%error, "could not apply the application layout"),
            Ok(outcome) => tracing::debug!(?pid, ?layout, ?outcome, "applied application layout"),
        }
    }
}

type Observers = Rc<RefCell<HashMap<i32, focus::Observer>>>;

pub(crate) struct Monitor {
    state: Rc<RefCell<State>>,
    _focus: Observers,
    center: Retained<NSNotificationCenter>,
    tokens: Vec<Retained<AnyObject>>,
}

impl Monitor {
    pub(crate) fn matches(&self, rules: &[ApplicationLayoutRule]) -> bool {
        self.state.borrow().rules == rules
    }

    pub(crate) fn start(rules: Vec<ApplicationLayoutRule>) -> Result<Self> {
        MainThreadMarker::new().context("application layout rules require the main thread")?;
        let state = Rc::new(RefCell::new(State {
            rules,
            session: FocusSession::default(),
        }));
        let observers = Rc::new(RefCell::new(HashMap::new()));
        let callback_state = Rc::clone(&state);
        let callback_observers = Rc::clone(&observers);
        let block = RcBlock::new(move |_: NonNull<NSNotification>| {
            if MainThreadMarker::new().is_none() {
                return;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                refresh_observers(&callback_state, &callback_observers);
                observe_current(&callback_state, true);
            }));
            if result.is_err() {
                tracing::error!("application layout observer recovered from a panic");
            }
        });
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let mut tokens = Vec::new();
        for name in unsafe {
            [
                NSWorkspaceDidActivateApplicationNotification,
                NSWorkspaceDidLaunchApplicationNotification,
                NSWorkspaceDidTerminateApplicationNotification,
            ]
        } {
            // SAFETY: the main queue confines capture access to the main thread;
            // Drop removes every token before releasing this state.
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(
                    Some(name),
                    None,
                    Some(&NSOperationQueue::mainQueue()),
                    &block,
                )
            };
            tokens.push(unsafe { Retained::cast_unchecked(token) });
        }
        refresh_observers(&state, &observers);
        observe_current(&state, true);
        Ok(Self {
            state,
            _focus: observers,
            center,
            tokens,
        })
    }
}

fn layout_for(
    rules: &[ApplicationLayoutRule],
    app: &NSRunningApplication,
) -> Option<upyr_core::InputLayout> {
    let id = app.bundleIdentifier()?.to_string();
    let name = app
        .localizedName()
        .map(|s| s.to_string())
        .unwrap_or_default();
    preferred_layout(rules, &id, &name)
}

fn observe_current(state: &RefCell<State>, workspace_fallback: bool) {
    // AX owns the actual keyboard focus, including nonactivating panels. Never
    // treat a notification's original app as current: delivery may be queued.
    let pid = focus::focused_pid().or_else(|| {
        workspace_fallback
            .then(|| NSWorkspace::sharedWorkspace().frontmostApplication())
            .flatten()
            .map(|app| app.processIdentifier())
    });
    if let Ok(mut state) = state.try_borrow_mut() {
        state.observe(pid);
    }
}

fn refresh_observers(
    state: &Rc<RefCell<State>>,
    observers: &RefCell<HashMap<i32, focus::Observer>>,
) {
    let workspace = NSWorkspace::sharedWorkspace();
    let foreground = workspace
        .frontmostApplication()
        .map(|app| app.processIdentifier());
    let wanted: Vec<_> = workspace
        .runningApplications()
        .iter()
        .filter_map(|app| {
            let pid = app.processIdentifier();
            (Some(pid) == foreground || layout_for(&state.borrow().rules, &app).is_some())
                .then_some(pid)
        })
        .collect();
    let mut observers = observers.borrow_mut();
    observers.retain(|pid, _| wanted.contains(pid));
    if !crate::accessibility::is_trusted() {
        return;
    }
    for pid in wanted {
        if let std::collections::hash_map::Entry::Vacant(entry) = observers.entry(pid) {
            // Watching the underlying front app detects panel dismissal even
            // when NSWorkspace's foreground app never changes.
            let weak = Rc::downgrade(state);
            if let Some(observer) = focus::Observer::new(pid, move || {
                if let Some(state) = weak.upgrade() {
                    observe_current(&state, false);
                }
            }) {
                entry.insert(observer);
            }
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        for token in &self.tokens {
            unsafe {
                self.center.removeObserver(token);
            }
        }
    }
}
