//! Insert the edit from one target-process event-tap callback. Every generated
//! key precedes subsequent user input in that process's event stream.
//!
//! The same per-process tap also holds a GUI selection edit: from the moment
//! Upyr starts selecting the word until its paste (or its selection recovery)
//! is in the stream, the user's keys wait here instead of replacing the
//! selection, and then follow the edit in their original order.
use super::*;
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Shared between the hold worker and the main-thread conversion guard.
#[derive(Debug, Default)]
pub(crate) struct HoldSignal {
    active: AtomicBool,
    broken: AtomicBool,
}

impl HoldSignal {
    /// User keyboard input is waiting in the target's tap and has not reached
    /// the editor, so it cannot have touched the selection.
    pub fn active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Held input was released without the edit's own trigger (deadline, tap
    /// disabled, worker stopped). The selection can no longer be trusted.
    pub fn broken(&self) -> bool {
        self.broken.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionHold {
    Pass,
    Queue,
    PassAndCancel,
}

/// What a GUI selection hold does with one event routed to the target.
fn selection_hold_action(kind: u32, own: bool) -> SelectionHold {
    if own {
        return SelectionHold::Pass;
    }
    match kind {
        // Key down, key up and modifier changes: any of them would act on the
        // active selection, so all wait behind the edit in order.
        10..=12 => SelectionHold::Queue,
        // Clicks, drags and scrolling move the caret or view themselves.
        1 | 3 | 6 | 7 | 22 | 25 | 27 => SelectionHold::PassAndCancel,
        _ => SelectionHold::Pass,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Policy {
    /// Terminal edits hold ordinary typing; anything else cancels and releases.
    Terminal,
    /// GUI selection edits hold every key until the paste or the recovery.
    Selection,
}

/// A line break held because the main thread had a correction pending for the
/// word before it. Enter waits in the target's stream behind that correction.
#[derive(Debug)]
pub(crate) struct HeldLineBreak {
    pub id: u64,
    pub pid: i32,
    pub signal: Arc<HoldSignal>,
}

/// Armed by the main thread while the word being typed would be corrected if
/// it ended now; consumed by the first Enter the target receives.
struct LineBreakArm {
    id: u64,
    duration: Duration,
    notify: Box<dyn Fn(HeldLineBreak) + Send>,
}

type SharedArm = Arc<std::sync::Mutex<Option<LineBreakArm>>>;

/// Whether the tap holds this event as the line break ending a word that has
/// a pending correction: a user's first Return or keypad Enter key-down, while
/// no edit is already holding input.
fn holds_line_break(
    kind: u32,
    own: bool,
    keycode: i64,
    autorepeat: i64,
    job_active: bool,
    armed: bool,
) -> bool {
    armed && !job_active && !own && kind == 10 && autorepeat == 0 && matches!(keycode, 36 | 76)
}

struct HoldStart {
    signal: Arc<HoldSignal>,
    duration: Duration,
    reply: mpsc::Sender<bool>,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreateForPid(
        pid: i32,
        placement: u32,
        options: u32,
        mask: u64,
        callback: unsafe extern "C" fn(Ref, u32, Event, *mut c_void) -> Event,
        context: *mut c_void,
    ) -> Ref;
    fn CGEventTapEnable(tap: Ref, enable: bool);
    fn CGEventTapPostEvent(proxy: Ref, event: Event);
    fn CGEventPostToPid(pid: i32, event: Event);
    fn CGEventGetIntegerValueField(event: Event, field: u32) -> i64;
    fn CGEventGetFlags(event: Event) -> u64;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: Ref;
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
    fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRemoveSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRunInMode(mode: Ref, seconds: f64, after_source: bool) -> i32;
    fn CFRunLoopStop(run_loop: Ref);
    fn CFRunLoopWakeUp(run_loop: Ref);
    fn CFRunLoopPerformBlock(run_loop: Ref, mode: Ref, block: Ref);
    fn CFMachPortInvalidate(port: Ref);
    fn CFRetain(value: Ref) -> Ref;
}

struct Request {
    prepared: Prepared,
    target_window: i64,
    /// Terminal edits compare Quartz input counters; a GUI release does not,
    /// because Upyr's own selection and copy keys advance them.
    stamp: Option<[u32; 6]>,
    reply: mpsc::Sender<Reply>,
}

enum Reply {
    Ready(mpsc::Sender<bool>),
    Complete(bool),
}

struct Job {
    policy: Policy,
    /// A GUI hold starts without a request and receives one at release.
    request: Option<Request>,
    signal: Option<Arc<HoldSignal>>,
    deadline: Instant,
    cancelled: bool,
    finished: bool,
    triggered: bool,
    applied: bool,
    queued: Vec<Owned>,
    queued_keydowns: u32,
}

impl Job {
    fn new(
        policy: Policy,
        request: Option<Request>,
        signal: Option<Arc<HoldSignal>>,
        deadline: Instant,
    ) -> Self {
        Self {
            policy,
            request,
            signal,
            deadline,
            cancelled: false,
            finished: false,
            triggered: false,
            applied: false,
            queued: Vec::with_capacity(16),
            queued_keydowns: 0,
        }
    }

    fn flush(&mut self, proxy: Ref) {
        for key in self.queued.drain(..) {
            unsafe { CGEventTapPostEvent(proxy, key.0.cast_mut()) };
        }
    }

    /// Stop holding. Input released other than by the edit's own trigger
    /// means the main thread must abandon the edit.
    fn end(&self) {
        if let Some(signal) = &self.signal {
            if !self.triggered {
                signal.broken.store(true, Ordering::SeqCst);
            }
            signal.active.store(false, Ordering::SeqCst);
        }
    }

    fn reply(&self) {
        if let Some(request) = &self.request {
            let _ = request.reply.send(Reply::Complete(self.applied));
        }
    }
}

struct State {
    target_pid: i32,
    job: Option<Job>,
    disabled: bool,
    line_break: SharedArm,
}

unsafe extern "C" fn deliver(proxy: Ref, kind: u32, event: Event, context: *mut c_void) -> Event {
    // The run loop and state live on the same worker for the entire tap lifetime.
    let state = unsafe { &mut *context.cast::<State>() };
    if kind == u32::MAX || kind == u32::MAX - 1 {
        state.disabled = true;
        if let Some(job) = state.job.as_mut() {
            job.finished = true;
            job.flush(proxy);
            job.end();
        }
        return event;
    }
    let own = unsafe { CGEventGetIntegerValueField(event, 41) } == i64::from(std::process::id());
    if kind == 10 && state.job.is_none() {
        let keycode = unsafe { CGEventGetIntegerValueField(event, 9) };
        let autorepeat = unsafe { CGEventGetIntegerValueField(event, 8) };
        let mut slot = state
            .line_break
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if holds_line_break(kind, own, keycode, autorepeat, false, slot.is_some()) {
            if let Some(arm) = slot.take() {
                drop(slot);
                // Enter and everything after it wait until the correction of
                // the word before it is pasted (or abandoned).
                let signal = Arc::new(HoldSignal::default());
                signal.active.store(true, Ordering::SeqCst);
                let mut job = Job::new(
                    Policy::Selection,
                    None,
                    Some(Arc::clone(&signal)),
                    Instant::now() + arm.duration,
                );
                job.queued_keydowns = 1;
                job.queued.push(Owned(unsafe { CFRetain(event.cast()) }));
                state.job = Some(job);
                (arm.notify)(HeldLineBreak {
                    id: arm.id,
                    pid: state.target_pid,
                    signal,
                });
                return std::ptr::null_mut();
            }
        }
    }
    let Some(job) = state.job.as_mut() else {
        return event;
    };
    let own_trigger = kind == 11
        && own
        && job.request.as_ref().is_some_and(|request| {
            let marker = unsafe { CGEventGetIntegerValueField(event, 42) };
            marker == request.prepared.marker
        });
    if own_trigger {
        job.finished = true;
        job.triggered = true;
        if let Some(request) = job.request.as_ref() {
            let counts_match = request.stamp.is_none_or(|mut expected_stamp| {
                expected_stamp[2] = expected_stamp[2].wrapping_add(job.queued_keydowns);
                crate::automation::input_guard::input_counts() == expected_stamp
            });
            let window_matches = crate::automation::input_guard::is_target_window(
                state.target_pid,
                request.target_window,
            );
            tracing::debug!(
                pid = state.target_pid,
                window = request.target_window,
                cancelled = job.cancelled,
                counts_match,
                window_matches,
                held = job.queued.len(),
                "validated edit target"
            );
            // Pointer input or a focus change has already moved the caret:
            // nothing Upyr prepared may be inserted at the new position.
            if !job.cancelled && window_matches {
                let mut committed = false;
                if Instant::now() < job.deadline && counts_match {
                    // The main thread changes TIS only after the target has
                    // accepted this edit. Until it acknowledges, this callback
                    // holds the stream.
                    let (commit, acknowledgement) = mpsc::channel();
                    committed = request.reply.send(Reply::Ready(commit)).is_ok()
                        && acknowledgement
                            .recv_timeout(Duration::from_millis(100))
                            .unwrap_or(false);
                }
                // Post at this exact point in the stream, bypassing this tap. A
                // subsequent key cannot overtake the edit or enter inside it. A
                // refused edit inserts its fallback, which releases a selection
                // before any held key can replace it.
                let batch = if committed {
                    &request.prepared.events
                } else {
                    &request.prepared.fallback
                };
                for key in batch {
                    unsafe { CGEventTapPostEvent(proxy, key.0.cast_mut()) };
                }
                job.applied = committed;
            }
        }
        job.flush(proxy);
        job.end();
        // The empty key-up is only a trigger, never input to the application.
        return std::ptr::null_mut();
    }
    if job.finished {
        return event;
    }
    if job.policy == Policy::Selection {
        return match selection_hold_action(kind, own) {
            SelectionHold::Queue => {
                if kind == 10 && unsafe { CGEventGetIntegerValueField(event, 8) } == 0 {
                    job.queued_keydowns = job.queued_keydowns.wrapping_add(1);
                }
                job.queued.push(Owned(unsafe { CFRetain(event.cast()) }));
                std::ptr::null_mut()
            }
            SelectionHold::PassAndCancel => {
                // The pointer reaches the editor first; held keys stay queued
                // and follow it at release instead of landing in the selection.
                job.cancelled = true;
                event
            }
            SelectionHold::Pass => event,
        };
    }
    // Ordinary typing that reaches the target before the trigger waits behind
    // the edit. It is forwarded with its original flags/source and never logged.
    let code = if matches!(kind, 10 | 11) {
        unsafe { CGEventGetIntegerValueField(event, 9) }
    } else {
        -1
    };
    let ordinary_key = matches!(code, 0..=35 | 37..=47 | 49 | 50)
        && unsafe { CGEventGetFlags(event) } & ((1 << 18) | (1 << 19) | (1 << 20) | (1 << 23)) == 0;
    if !job.cancelled && !job.finished && ordinary_key {
        if kind == 10 && unsafe { CGEventGetIntegerValueField(event, 8) } == 0 {
            job.queued_keydowns = job.queued_keydowns.wrapping_add(1);
        }
        job.queued.push(Owned(unsafe { CFRetain(event.cast()) }));
        return std::ptr::null_mut();
    }
    if matches!(kind, 1 | 3 | 6 | 7 | 10 | 12 | 22 | 25 | 27) {
        job.cancelled = true;
        job.flush(proxy);
    }
    event
}

enum Command {
    Edit(Request),
    Hold(HoldStart),
    Release(Request),
    Stop,
}

struct Ready(Owned);
// CFRunLoop references are retained and CFRunLoopStop is thread-safe.
unsafe impl Send for Ready {}

struct Worker {
    pid: i32,
    commands: mpsc::Sender<Command>,
    line_break: SharedArm,
    ready: Option<mpsc::Receiver<std::result::Result<Ready, String>>>,
    run_loop: Option<Ready>,
    handle: Option<JoinHandle<()>>,
}

thread_local! {
    static WORKER: RefCell<Option<Worker>> = const { RefCell::new(None) };
}

impl Worker {
    fn start(pid: i32) -> Result<Self> {
        let (commands, receiver) = mpsc::channel();
        let (ready_sender, ready) = mpsc::channel();
        let line_break = SharedArm::default();
        let worker_arm = Arc::clone(&line_break);
        let handle = std::thread::Builder::new()
            .name("upyr-terminal-edit".to_owned())
            .spawn(move || {
                let errors = ready_sender.clone();
                if let Err(error) = run(pid, receiver, ready_sender, worker_arm) {
                    let _ = errors.send(Err(error.to_string()));
                }
            })
            .context("could not start the terminal edit worker")?;
        Ok(Self {
            pid,
            commands,
            line_break,
            ready: Some(ready),
            run_loop: None,
            handle: Some(handle),
        })
    }

    fn ensure_ready(&mut self) -> Result<()> {
        if let Some(receiver) = self.ready.as_ref() {
            self.run_loop = Some(
                receiver
                    .recv_timeout(Duration::from_millis(250))
                    .context("terminal event tap did not become ready")?
                    .map_err(anyhow::Error::msg)?,
            );
            self.ready = None;
        }
        Ok(())
    }

    fn wake(&self) {
        if let Some(ready) = &self.run_loop {
            // Queue the stop as work: a plain Stop between two RunInMode calls
            // can be lost just before the worker enters its idle wait.
            let stop = block2::RcBlock::new(|| unsafe { CFRunLoopStop(CFRunLoopGetCurrent()) });
            unsafe {
                CFRunLoopPerformBlock(
                    ready.0.0,
                    kCFRunLoopDefaultMode,
                    std::ptr::from_ref(&*stop).cast(),
                );
                CFRunLoopWakeUp(ready.0.0);
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        let _ = self.ensure_ready();
        self.wake();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub(super) fn warm(target_pid: i32) -> Result<()> {
    WORKER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_none_or(|worker| {
            worker.pid != target_pid || worker.handle.as_ref().is_none_or(JoinHandle::is_finished)
        }) {
            *slot = Some(Worker::start(target_pid)?);
        }
        Ok(())
    })
}

pub(super) fn post(
    prepared: Prepared,
    target_pid: i32,
    target_window: i64,
    stamp: [u32; 6],
    on_ready: impl FnOnce() -> bool,
) -> Result<bool> {
    warm(target_pid)?;
    WORKER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let worker = slot.as_mut().context("terminal worker is unavailable")?;
        worker.ensure_ready()?;
        let (reply, result) = mpsc::channel();
        worker
            .commands
            .send(Command::Edit(Request {
                prepared,
                target_window,
                stamp: Some(stamp),
                reply,
            }))
            .map_err(|_| anyhow::anyhow!("terminal event tap stopped"))?;
        worker.wake();
        await_commit(result, on_ready)
    })
}

/// Arms the target's tap to hold the next Enter behind the correction of the
/// word being typed. `notify` runs on the worker thread when Enter is held.
pub(super) fn arm_line_break(
    target_pid: i32,
    id: u64,
    duration: Duration,
    notify: impl Fn(HeldLineBreak) + Send + 'static,
) -> Result<()> {
    warm(target_pid)?;
    WORKER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let worker = slot.as_mut().context("hold worker is unavailable")?;
        worker.ensure_ready()?;
        *worker
            .line_break
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(LineBreakArm {
            id,
            duration,
            notify: Box::new(notify),
        });
        Ok(())
    })
}

/// No word with a pending correction is being typed any more.
pub(super) fn disarm_line_break() {
    WORKER.with(|slot| {
        if let Some(worker) = slot.borrow().as_ref() {
            *worker
                .line_break
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    });
}

/// Releases a held line break without any edit: Enter goes through as typed.
pub(super) fn release_held(target_pid: i32) -> Result<bool> {
    // No real window id: nothing is inserted, the held keys are flushed.
    release(Prepared::empty()?, target_pid, -1, || true)
}

/// Starts holding the target's keyboard input behind a GUI selection edit.
/// Returns `false` when the hold could not be established in time; the caller
/// then keeps its listen-only guard semantics.
pub(super) fn hold(target_pid: i32, signal: Arc<HoldSignal>, duration: Duration) -> Result<bool> {
    warm(target_pid)?;
    WORKER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let worker = slot.as_mut().context("hold worker is unavailable")?;
        worker.ensure_ready()?;
        let (reply, accepted) = mpsc::channel();
        worker
            .commands
            .send(Command::Hold(HoldStart {
                signal,
                duration,
                reply,
            }))
            .map_err(|_| anyhow::anyhow!("hold event tap stopped"))?;
        worker.wake();
        Ok(accepted
            .recv_timeout(Duration::from_millis(100))
            .unwrap_or(false))
    })
}

/// Ends a GUI hold: `prepared` is inserted at the trigger, immediately ahead
/// of every held key. `on_ready` runs on this thread after validation.
pub(super) fn release(
    prepared: Prepared,
    target_pid: i32,
    target_window: i64,
    on_ready: impl FnOnce() -> bool,
) -> Result<bool> {
    WORKER.with(|slot| {
        let mut slot = slot.borrow_mut();
        // Never warm here: starting a worker for another process would drop
        // the one holding this input.
        let worker = slot
            .as_mut()
            .filter(|worker| worker.pid == target_pid)
            .context("hold worker is unavailable")?;
        worker.ensure_ready()?;
        let (reply, result) = mpsc::channel();
        worker
            .commands
            .send(Command::Release(Request {
                prepared,
                target_window,
                stamp: None,
                reply,
            }))
            .map_err(|_| anyhow::anyhow!("hold event tap stopped"))?;
        worker.wake();
        await_commit(result, on_ready)
    })
}

fn await_commit(result: mpsc::Receiver<Reply>, on_ready: impl FnOnce() -> bool) -> Result<bool> {
    let deadline = Instant::now() + Duration::from_millis(350);
    let mut on_ready = Some(on_ready);
    loop {
        match result
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .context("terminal edit did not finish")?
        {
            Reply::Ready(commit) => {
                if let Some(on_ready) = on_ready.take() {
                    let accepted = on_ready();
                    let _ = commit.send(accepted);
                }
            }
            Reply::Complete(applied) => return Ok(applied),
        }
    }
}

fn run(
    target_pid: i32,
    commands: mpsc::Receiver<Command>,
    ready: mpsc::Sender<std::result::Result<Ready, String>>,
    line_break: SharedArm,
) -> Result<()> {
    let mut state = Box::new(State {
        target_pid,
        job: None,
        disabled: false,
        line_break,
    });
    let mask = [1, 2, 3, 4, 6, 7, 10, 11, 12, 22, 25, 26, 27]
        .into_iter()
        .fold(0_u64, |mask, kind| mask | (1 << kind));
    let tap = Owned::new(unsafe {
        CGEventTapCreateForPid(
            target_pid,
            0,
            0,
            mask,
            deliver,
            std::ptr::from_mut(&mut *state).cast(),
        )
    })
    .context("could not guard the terminal process's event stream")?;
    let source = Owned::new(unsafe { CFMachPortCreateRunLoopSource(std::ptr::null(), tap.0, 0) })
        .context("could not attach the terminal edit callback")?;
    let run_loop = unsafe { CFRunLoopGetCurrent() };
    unsafe {
        CFRunLoopAddSource(run_loop, source.0, kCFRunLoopDefaultMode);
        CGEventTapEnable(tap.0, true);
    }
    let _ = ready.send(Ok(Ready(Owned(unsafe { CFRetain(run_loop) }))));
    'worker: loop {
        loop {
            match commands.try_recv() {
                Ok(Command::Stop) | Err(mpsc::TryRecvError::Disconnected) => break 'worker,
                Err(mpsc::TryRecvError::Empty) => break,
                Ok(Command::Edit(request)) => {
                    if state.job.is_some() {
                        let _ = request.reply.send(Reply::Complete(false));
                        continue;
                    }
                    let trigger = request.prepared.trigger.0.cast_mut();
                    state.job = Some(Job::new(
                        Policy::Terminal,
                        Some(request),
                        None,
                        Instant::now() + Duration::from_millis(250),
                    ));
                    unsafe { CGEventPostToPid(target_pid, trigger) };
                }
                Ok(Command::Hold(start)) => {
                    let accepted = state.job.is_none();
                    if accepted {
                        start.signal.active.store(true, Ordering::SeqCst);
                        state.job = Some(Job::new(
                            Policy::Selection,
                            None,
                            Some(start.signal),
                            Instant::now() + start.duration,
                        ));
                    }
                    let _ = start.reply.send(accepted);
                }
                Ok(Command::Release(request)) => match state.job.as_mut() {
                    Some(job)
                        if job.policy == Policy::Selection
                            && job.request.is_none()
                            && !job.finished =>
                    {
                        let trigger = request.prepared.trigger.0.cast_mut();
                        job.request = Some(request);
                        job.deadline = Instant::now() + Duration::from_millis(250);
                        unsafe { CGEventPostToPid(target_pid, trigger) };
                    }
                    _ => {
                        let _ = request.reply.send(Reply::Complete(false));
                    }
                },
            }
        }
        if state.disabled {
            break;
        }
        if state
            .job
            .as_ref()
            .is_some_and(|job| job.finished || Instant::now() >= job.deadline)
        {
            if let Some(mut job) = state.job.take() {
                if !job.finished {
                    tracing::debug!(
                        pid = target_pid,
                        held = job.queued.len(),
                        "edit target did not receive its trigger; releasing held input"
                    );
                }
                for key in job.queued.drain(..) {
                    unsafe { CGEventPostToPid(target_pid, key.0.cast_mut()) };
                }
                job.end();
                job.reply();
            }
        }
        // Idle workers sleep; CFRunLoopStop wakes them when a command arrives.
        let seconds = state.job.as_ref().map_or(3600.0, |job| {
            job.deadline
                .saturating_duration_since(Instant::now())
                .as_secs_f64()
        });
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, true) };
    }
    unsafe {
        CGEventTapEnable(tap.0, false);
        CFRunLoopRemoveSource(run_loop, source.0, kCFRunLoopDefaultMode);
        CFMachPortInvalidate(tap.0);
    }
    // A timed-out trigger never performs an edit. Return every retained user
    // event after removing the tap rather than dropping any part of their input.
    if let Some(mut job) = state.job.take() {
        for key in job.queued.drain(..) {
            unsafe { CGEventPostToPid(target_pid, key.0.cast_mut()) };
        }
        job.applied = false;
        job.end();
        job.reply();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_hold_queues_every_user_key_and_passes_upyrs_own_edit() {
        // Upyr's own Shift+Left and Cmd+C must reach the editor during the hold.
        for kind in [10, 11, 12] {
            assert_eq!(selection_hold_action(kind, true), SelectionHold::Pass);
        }
        // Any user key, including Backspace, Return, arrows and shortcuts,
        // would act on the active selection, so all of them wait.
        for kind in [10, 11, 12] {
            assert_eq!(selection_hold_action(kind, false), SelectionHold::Queue);
        }
        // Pointer activity moves the caret itself: it goes through first and
        // refuses the paste; the queued keys then follow it.
        for kind in [1, 3, 6, 7, 22, 25, 27] {
            assert_eq!(
                selection_hold_action(kind, false),
                SelectionHold::PassAndCancel
            );
        }
        for kind in [2, 4, 5, 26] {
            assert_eq!(selection_hold_action(kind, false), SelectionHold::Pass);
        }
    }

    #[test]
    fn only_a_fresh_user_enter_with_a_pending_correction_is_held() {
        // Return (36) and keypad Enter (76) key-downs from the user.
        for code in [36, 76] {
            assert!(holds_line_break(10, false, code, 0, false, true));
        }
        // No pending correction, a repeat, Upyr's own key, a key-up, another
        // key, or an edit already holding input: Enter passes untouched.
        assert!(!holds_line_break(10, false, 36, 0, false, false));
        assert!(!holds_line_break(10, false, 36, 1, false, true));
        assert!(!holds_line_break(10, true, 36, 0, false, true));
        assert!(!holds_line_break(11, false, 36, 0, false, true));
        assert!(!holds_line_break(10, false, 49, 0, false, true));
        assert!(!holds_line_break(10, false, 36, 0, true, true));
    }

    #[test]
    fn rejected_edit_never_changes_the_layout() {
        let (sender, receiver) = mpsc::channel();
        sender.send(Reply::Complete(false)).unwrap();
        assert!(!await_commit(receiver, || panic!("rejected edit changed layout")).unwrap());
    }

    #[test]
    fn accepted_edit_waits_for_layout_commit_before_typing() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let changed = Arc::new(AtomicBool::new(false));
        let checked = changed.clone();
        let (sender, receiver) = mpsc::channel();
        let target = std::thread::spawn(move || {
            let (commit, acknowledgement) = mpsc::channel();
            sender.send(Reply::Ready(commit)).unwrap();
            assert!(acknowledgement.recv().unwrap());
            assert!(
                checked.load(Ordering::SeqCst),
                "typing preceded layout commit"
            );
            sender.send(Reply::Complete(true)).unwrap();
        });
        assert!(
            await_commit(receiver, || {
                changed.store(true, Ordering::SeqCst);
                true
            })
            .unwrap()
        );
        target.join().unwrap();
    }

    #[test]
    fn changed_application_focus_rejects_the_validated_edit() {
        let (sender, receiver) = mpsc::channel();
        let target = std::thread::spawn(move || {
            let (commit, acknowledgement) = mpsc::channel();
            sender.send(Reply::Ready(commit)).unwrap();
            assert!(!acknowledgement.recv().unwrap());
            sender.send(Reply::Complete(false)).unwrap();
        });
        assert!(!await_commit(receiver, || false).unwrap());
        target.join().unwrap();
    }
}
