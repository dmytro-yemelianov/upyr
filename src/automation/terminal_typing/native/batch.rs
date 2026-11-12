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

/// One keyboard event: its type, timestamp and key code. Copies of the same
/// event share all three; distinct events never do.
type EventId = (u32, u64, i64);

/// Keyboard events this tap has already let through, held or posted.
///
/// Some apps (Gemini, Claude) receive every key-down at their process tap
/// twice, with the same timestamp. Without an active tap the system delivers
/// it once; with one in place, both copies reach the app and every letter is
/// typed twice until the tap moves to another app. The tap therefore passes
/// each event once and drops any later arrival of it.
#[derive(Default)]
struct SeenEvents {
    recent: std::collections::VecDeque<EventId>,
}

impl SeenEvents {
    /// Copies arrive within milliseconds; this covers a burst of typing.
    const CAPACITY: usize = 64;

    /// Records `id`; `false` means it was already seen and must be dropped.
    fn first_sight(&mut self, id: EventId) -> bool {
        if self.recent.contains(&id) {
            return false;
        }
        self.remember(id);
        true
    }

    /// Marks an event posted past this tap, so a copy coming back is dropped.
    fn remember(&mut self, id: EventId) {
        if self.recent.contains(&id) {
            return;
        }
        if self.recent.len() == Self::CAPACITY {
            self.recent.pop_front();
        }
        self.recent.push_back(id);
    }

    /// Lets an event pass once more: a held key re-posted to the process
    /// enters the stream ahead of this tap.
    fn forget(&mut self, id: EventId) {
        self.recent.retain(|seen| *seen != id);
    }
}

/// The identity of a key-down, key-up or modifier change; `None` otherwise.
fn keyboard_event_id(kind: u32, event: Event) -> Option<EventId> {
    matches!(kind, 10..=12).then(|| unsafe {
        (
            kind,
            CGEventGetTimestamp(event),
            CGEventGetIntegerValueField(event, 9),
        )
    })
}

fn event_kind(event: Event) -> u32 {
    unsafe { CGEventGetType(event) }
}

fn terminal_anchor_matches(expected: Option<u64>, delivered: Option<u64>) -> bool {
    expected.is_none_or(|stamp| delivered == Some(stamp))
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
    fn CGEventGetTimestamp(event: Event) -> u64;
    fn CGEventGetType(event: Event) -> u32;
    fn CGEventKeyboardGetUnicodeString(
        event: Event,
        capacity: usize,
        length: *mut usize,
        text: *mut u16,
    );
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
    after_key: Option<u64>,
    reply: mpsc::Sender<Reply>,
}

enum Reply {
    Ready(mpsc::Sender<Commit>),
    Complete(bool),
}

#[derive(Default)]
struct Commit {
    accepted: bool,
    translation: Option<Translation>,
}

struct Translation {
    source_id: String,
    text: Arc<crate::system_layout::KeyTextMap>,
    previous: Arc<crate::system_layout::KeyTextMap>,
    through: u64,
    active: bool,
}

/// Quartz timestamps use nanoseconds of mach uptime. An unposted synthetic
/// CGEvent has timestamp zero and cannot mark the layout-change boundary.
fn event_time_now() -> u64 {
    #[repr(C)]
    #[derive(Default)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    unsafe extern "C" {
        fn mach_absolute_time() -> u64;
        fn mach_timebase_info(info: *mut Timebase) -> i32;
    }
    static SCALE: std::sync::OnceLock<Timebase> = std::sync::OnceLock::new();
    let scale = SCALE.get_or_init(|| {
        let mut scale = Timebase::default();
        assert_eq!(unsafe { mach_timebase_info(&mut scale) }, 0);
        scale
    });
    (u128::from(unsafe { mach_absolute_time() }) * u128::from(scale.numer)
        / u128::from(scale.denom)) as u64
}

impl Translation {
    fn apply(&mut self, kind: u32, event: Event) {
        let stamp = unsafe { CGEventGetTimestamp(event) };
        let flags = unsafe { CGEventGetFlags(event) };
        if matches!(kind, 1 | 3 | 6 | 7 | 22 | 25 | 27)
            || flags & ((1 << 18) | (1 << 19) | (1 << 20) | (1 << 23)) != 0
        {
            self.active = false;
        }
        if !matches!(kind, 10 | 11) || (stamp > self.through && !self.active) {
            return;
        }
        let code = unsafe { CGEventGetIntegerValueField(event, 9) };
        if let Some(character) = self.text.character(code, flags) {
            let mut length = 0;
            let mut actual = [0_u16; 2];
            unsafe { CGEventKeyboardGetUnicodeString(event, 2, &mut length, actual.as_mut_ptr()) };
            let mut decoded = char::decode_utf16(actual[..length.min(2)].iter().copied());
            let Some(Ok(actual)) = decoded.next() else {
                return;
            };
            let previous = self.previous.character(code, flags);
            if decoded.next().is_some() || (Some(actual) != previous && actual != character) {
                return;
            }
            // A fresh Quartz character does not establish that the target's
            // own layout cache has caught up (notably Chromium). Supply the
            // chosen layout explicitly until a source change or navigation.
            let mut units = [0_u16; 2];
            let units = character.encode_utf16(&mut units);
            unsafe { CGEventKeyboardSetUnicodeString(event, units.len(), units.as_ptr()) };
        }
    }
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

    /// Posts held keys past this tap, in order.
    fn flush(&mut self, proxy: Ref, seen: &mut SeenEvents, last_keydown: &mut Option<u64>) {
        for key in self.queued.drain(..) {
            if event_kind(key.0.cast_mut()) == 10 {
                *last_keydown = Some(unsafe { CGEventGetTimestamp(key.0.cast_mut()) });
            }
            post_past_tap(proxy, &key, seen);
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
    seen: SeenEvents,
    translation: Option<Translation>,
    last_keydown: Option<u64>,
}

impl State {
    fn source_changed(&mut self, source: Option<&str>) {
        if self
            .translation
            .as_ref()
            .is_some_and(|translation| Some(translation.source_id.as_str()) != source)
        {
            self.translation = None;
        }
    }
}

/// Posts `event` immediately after this tap. A copy that returns to the tap
/// is then dropped instead of reaching the app a second time.
fn post_past_tap(proxy: Ref, event: &Owned, seen: &mut SeenEvents) {
    let event = event.0.cast_mut();
    if let Some(id) = keyboard_event_id(event_kind(event), event) {
        seen.remember(id);
    }
    unsafe { CGEventTapPostEvent(proxy, event) };
}

/// Re-posts a held key to the target process, ahead of this tap, where it
/// must be let through once more.
fn post_to_target(target_pid: i32, event: &Owned, seen: &mut SeenEvents) {
    let event = event.0.cast_mut();
    if let Some(id) = keyboard_event_id(event_kind(event), event) {
        seen.forget(id);
    }
    unsafe { CGEventPostToPid(target_pid, event) };
}

unsafe extern "C" fn deliver(proxy: Ref, kind: u32, event: Event, context: *mut c_void) -> Event {
    // The run loop and state live on the same worker for the entire tap lifetime.
    let state = unsafe { &mut *context.cast::<State>() };
    if kind == u32::MAX || kind == u32::MAX - 1 {
        state.disabled = true;
        if let Some(job) = state.job.as_mut() {
            job.finished = true;
            job.flush(proxy, &mut state.seen, &mut state.last_keydown);
            job.end();
        }
        return event;
    }
    if let Some(id) = keyboard_event_id(kind, event) {
        if !state.seen.first_sight(id) {
            // A second copy of an event this tap already passed, held or
            // posted; see `SeenEvents`.
            return std::ptr::null_mut();
        }
    }
    let own = unsafe { CGEventGetIntegerValueField(event, 41) } == i64::from(std::process::id());
    if !own {
        if let Some(translation) = &mut state.translation {
            translation.apply(kind, event);
        }
    }
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
        if kind == 10 && !own {
            state.last_keydown = Some(unsafe { CGEventGetTimestamp(event) });
        }
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
            let anchor_matches = terminal_anchor_matches(request.after_key, state.last_keydown);
            let window_matches = crate::automation::input_guard::is_target_window(
                state.target_pid,
                request.target_window,
            );
            tracing::debug!(
                pid = state.target_pid,
                window = request.target_window,
                cancelled = job.cancelled,
                counts_match,
                anchor_matches,
                window_matches,
                held = job.queued.len(),
                "validated edit target"
            );
            // Pointer input or a focus change has already moved the caret:
            // nothing Upyr prepared may be inserted at the new position.
            if !job.cancelled && window_matches {
                let mut committed = false;
                if Instant::now() < job.deadline && counts_match && anchor_matches {
                    // The main thread changes TIS only after the target has
                    // accepted this edit. Until it acknowledges, this callback
                    // holds the stream.
                    let (commit, acknowledgement) = mpsc::channel();
                    if request.reply.send(Reply::Ready(commit)).is_ok() {
                        let decision = acknowledgement
                            .recv_timeout(Duration::from_millis(100))
                            .unwrap_or_default();
                        committed = decision.accepted;
                        if let Some(mut translation) = decision.translation {
                            // Keys translated before the switch retain old Unicode
                            // even when delivered after paste. Fix both held keys
                            // and older keys still travelling toward this tap.
                            for key in &job.queued {
                                translation.apply(event_kind(key.0.cast_mut()), key.0.cast_mut());
                            }
                            state.translation = Some(translation);
                        }
                    }
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
                    post_past_tap(proxy, key, &mut state.seen);
                }
                job.applied = committed;
            }
        }
        job.flush(proxy, &mut state.seen, &mut state.last_keydown);
        job.end();
        // The empty key-up is only a trigger, never input to the application.
        return std::ptr::null_mut();
    }
    if job.finished {
        if kind == 10 && !own {
            state.last_keydown = Some(unsafe { CGEventGetTimestamp(event) });
        }
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
        job.flush(proxy, &mut state.seen, &mut state.last_keydown);
    }
    if kind == 10 && !own {
        state.last_keydown = Some(unsafe { CGEventGetTimestamp(event) });
    }
    event
}

enum Command {
    Source(Option<String>),
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

pub(super) fn input_source_changed(source: Option<String>) {
    WORKER.with(|slot| {
        if let Ok(slot) = slot.try_borrow() {
            if let Some(worker) = slot.as_ref() {
                let _ = worker.commands.send(Command::Source(source));
                worker.wake();
            }
        }
    });
}

pub(super) fn post(
    prepared: Prepared,
    target_pid: i32,
    target_window: i64,
    stamp: [u32; 6],
    after_key: Option<u64>,
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
                after_key,
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
                after_key: None,
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
                    let source = || {
                        objc2::MainThreadMarker::new()
                            .and_then(|_| crate::system_layout::current().ok().flatten())
                            .map(|s| s.source_id)
                    };
                    let before = source();
                    let previous = crate::system_layout::current_key_text_map();
                    let accepted = on_ready();
                    let through = event_time_now();
                    let after = source();
                    let translation = (accepted && before.is_some() && before != after)
                        .then(|| {
                            let text = crate::system_layout::current_key_text_map()?;
                            Some(Translation {
                                source_id: after?,
                                text,
                                previous: previous?,
                                through,
                                active: true,
                            })
                        })
                        .flatten();
                    let _ = commit.send(Commit {
                        accepted,
                        translation,
                    });
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
        seen: SeenEvents::default(),
        translation: None,
        last_keydown: None,
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
                Ok(Command::Source(source)) => {
                    state.source_changed(source.as_deref());
                }
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
                    post_to_target(target_pid, &key, &mut state.seen);
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
            post_to_target(target_pid, &key, &mut state.seen);
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
    fn a_terminal_snapshot_cannot_hide_a_key_delivered_before_the_guard() {
        // The guard can stamp counters after key 5, while the event loop is
        // still recognizing the prefix ending at key 4. Equal counters alone
        // cannot authorize deleting four characters from a five-key word.
        assert!(!terminal_anchor_matches(Some(40), Some(50)));
        assert!(!terminal_anchor_matches(Some(40), Some(30)));
        assert!(!terminal_anchor_matches(Some(40), None));
        assert!(terminal_anchor_matches(Some(40), Some(40)));
        // Manual terminal undo still uses its existing focus/counter guard.
        assert!(terminal_anchor_matches(None, Some(50)));
    }

    unsafe extern "C" {
        fn CGEventSetTimestamp(event: Event, timestamp: u64);
    }

    fn translated_key(text: char, stamp: u64, flags: u64) -> Owned {
        let key =
            Owned::new(unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), 34, true) }).unwrap();
        let mut units = [0; 2];
        let units = text.encode_utf16(&mut units);
        unsafe {
            CGEventKeyboardSetUnicodeString(key.0.cast_mut(), units.len(), units.as_ptr());
            CGEventSetTimestamp(key.0.cast_mut(), stamp);
            CGEventSetFlags(key.0.cast_mut(), flags);
        }
        key
    }

    fn key_text(key: &Owned) -> String {
        let mut units = [0; 2];
        let mut length = 0;
        unsafe {
            CGEventKeyboardGetUnicodeString(key.0.cast_mut(), 2, &mut length, units.as_mut_ptr());
        }
        String::from_utf16(&units[..length]).unwrap()
    }

    fn transition() -> Translation {
        Translation {
            source_id: "test".into(),
            previous: Arc::new(crate::system_layout::KeyTextMap::test_key(
                34,
                ['ш', 'Ш', 'Ш', 'ш'],
            )),
            text: Arc::new(crate::system_layout::KeyTextMap::test_key(
                34,
                ['i', 'I', 'I', 'i'],
            )),
            through: 100,
            active: true,
        }
    }

    #[test]
    fn fresh_and_delayed_physical_keys_keep_the_committed_layout() {
        let mut change = transition();
        for stamp in [90, 101] {
            let key = translated_key('ш', stamp, 0);
            change.apply(10, key.0.cast_mut());
            assert_eq!(key_text(&key), "i");
            assert_eq!(unsafe { CGEventGetTimestamp(key.0.cast_mut()) }, stamp);
        }
        let fresh = translated_key('i', 102, 0);
        change.apply(10, fresh.0.cast_mut());
        assert!(change.active);
        let later = translated_key('ш', change.through + 1, 0);
        change.apply(10, later.0.cast_mut());
        assert_eq!(key_text(&later), "i");
        let delayed = translated_key('ш', 99, 0);
        change.apply(10, delayed.0.cast_mut());
        assert_eq!(key_text(&delayed), "i");
    }

    #[test]
    fn translation_preserves_case_shortcuts_and_unrelated_unicode() {
        for (flags, input, expected) in [
            (1 << 17, 'Ш', 'I'),
            (1 << 16, 'Ш', 'I'),
            ((1 << 17) | (1 << 16), 'ш', 'i'),
            (1 << 20, 'ш', 'ш'),
            (1 << 19, 'ш', 'ш'),
            (0, 'ї', 'ї'),
        ] {
            let key = translated_key(input, 90, flags);
            transition().apply(10, key.0.cast_mut());
            assert_eq!(key_text(&key), expected.to_string());
            assert_eq!(unsafe { CGEventGetFlags(key.0.cast_mut()) }, flags);
        }
    }

    #[test]
    fn manual_switch_gestures_end_the_source_transition() {
        for kind in [1, 3, 22, 12] {
            let mut change = transition();
            let gesture = translated_key('ш', 101, if kind == 12 { 1 << 23 } else { 0 });
            change.apply(kind, gesture.0.cast_mut());
            let key = translated_key('ш', 102, 0);
            change.apply(10, key.0.cast_mut());
            assert_eq!(key_text(&key), "ш");
        }
    }

    #[test]
    fn layout_boundary_uses_a_nonzero_monotonic_clock() {
        let before = event_time_now();
        assert!(before > 0);
        assert!(event_time_now() >= before);
    }

    #[test]
    fn external_source_changes_clear_a_transition_even_without_another_key() {
        let mut state = State {
            target_pid: 0,
            job: None,
            disabled: false,
            line_break: SharedArm::default(),
            seen: SeenEvents::default(),
            translation: Some(transition()),
            last_keydown: None,
        };
        state.source_changed(Some("test"));
        assert!(state.translation.is_some());
        state.source_changed(Some("manual"));
        assert!(state.translation.is_none());
        state.translation = Some(transition());
        state.source_changed(None);
        assert!(state.translation.is_none());
    }

    #[test]
    fn a_key_event_passes_once_and_its_second_arrival_is_dropped() {
        // Gemini and Claude deliver each key-down to their process tap twice
        // with one timestamp; letting both through doubles every letter.
        let mut seen = SeenEvents::default();
        let key_down = (10, 1_000, 4);
        assert!(seen.first_sight(key_down));
        assert!(!seen.first_sight(key_down));
        assert!(!seen.first_sight(key_down));
    }

    #[test]
    fn events_sharing_a_timestamp_are_told_apart_by_type_and_key() {
        let mut seen = SeenEvents::default();
        assert!(seen.first_sight((10, 1_000, 4)));
        assert!(seen.first_sight((11, 1_000, 4)));
        assert!(seen.first_sight((10, 1_000, 5)));
        assert!(seen.first_sight((10, 1_001, 4)));
    }

    #[test]
    fn a_held_key_reposted_to_the_process_passes_once_more() {
        // A held key re-enters the stream ahead of this tap when it is
        // released by timeout, so it must not count as its own repeat.
        let mut seen = SeenEvents::default();
        let held = (10, 2_000, 4);
        assert!(seen.first_sight(held));
        seen.forget(held);
        assert!(seen.first_sight(held));
        assert!(!seen.first_sight(held));
    }

    #[test]
    fn an_event_posted_after_the_tap_is_not_let_through_again() {
        // Upyr's own edit is posted past this tap; a copy coming back to the
        // tap would insert the paste twice.
        let mut seen = SeenEvents::default();
        seen.remember((10, 3_000, 9));
        assert!(!seen.first_sight((10, 3_000, 9)));
        seen.remember((10, 3_000, 9));
        seen.forget((10, 3_000, 9));
        assert!(seen.first_sight((10, 3_000, 9)));
    }

    #[test]
    fn only_recent_events_are_remembered() {
        let mut seen = SeenEvents::default();
        for stamp in 0..SeenEvents::CAPACITY as u64 + 1 {
            assert!(seen.first_sight((10, stamp, 0)));
        }
        // The oldest identity has been evicted; the newest is still known.
        assert!(seen.first_sight((10, 0, 0)));
        assert!(!seen.first_sight((10, SeenEvents::CAPACITY as u64, 0)));
    }

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
            assert!(acknowledgement.recv().unwrap().accepted);
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
            assert!(!acknowledgement.recv().unwrap().accepted);
            sender.send(Reply::Complete(false)).unwrap();
        });
        assert!(!await_commit(receiver, || false).unwrap());
        target.join().unwrap();
    }
}
