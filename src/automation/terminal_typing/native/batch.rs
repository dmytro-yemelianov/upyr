//! Insert the edit from one target-process event-tap callback. Every generated
//! key precedes subsequent user input in that process's event stream.
use super::*;
use std::{
    cell::RefCell,
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

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
    stamp: [u32; 6],
    reply: mpsc::Sender<Reply>,
}

enum Reply {
    Ready(mpsc::Sender<bool>),
    Complete(bool),
}

struct Job {
    request: Request,
    deadline: Instant,
    cancelled: bool,
    finished: bool,
    applied: bool,
    queued: Vec<Owned>,
    queued_keydowns: u32,
}

impl Job {
    fn flush(&mut self, proxy: Ref) {
        for key in self.queued.drain(..) {
            unsafe { CGEventTapPostEvent(proxy, key.0.cast_mut()) };
        }
    }
}

struct State {
    target_pid: i32,
    job: Option<Job>,
    disabled: bool,
}

unsafe extern "C" fn deliver(proxy: Ref, kind: u32, event: Event, context: *mut c_void) -> Event {
    // The run loop and state live on the same worker for the entire tap lifetime.
    let state = unsafe { &mut *context.cast::<State>() };
    if kind == u32::MAX || kind == u32::MAX - 1 {
        state.disabled = true;
        if let Some(job) = state.job.as_mut() {
            job.finished = true;
            job.flush(proxy);
        }
        return event;
    }
    let Some(job) = state.job.as_mut() else {
        return event;
    };
    let own_trigger = kind == 11
        && unsafe { CGEventGetIntegerValueField(event, 41) } == i64::from(std::process::id())
        && unsafe { CGEventGetIntegerValueField(event, 42) } == job.request.prepared.marker;
    if own_trigger {
        job.finished = true;
        let mut expected_stamp = job.request.stamp;
        expected_stamp[2] = expected_stamp[2].wrapping_add(job.queued_keydowns);
        let counts_match = crate::automation::input_guard::input_counts() == expected_stamp;
        let window_matches = crate::automation::input_guard::is_target_window(
            state.target_pid,
            job.request.target_window,
        );
        tracing::debug!(
            pid = state.target_pid,
            window = job.request.target_window,
            cancelled = job.cancelled,
            counts_match,
            window_matches,
            "validated terminal edit target"
        );
        if !job.cancelled && Instant::now() < job.deadline && counts_match && window_matches {
            // The main thread changes TIS only after the target has accepted
            // this edit. Until it acknowledges, this callback holds the stream.
            let (commit, acknowledgement) = mpsc::channel();
            let committed = job.request.reply.send(Reply::Ready(commit)).is_ok()
                && acknowledgement
                    .recv_timeout(Duration::from_millis(100))
                    .unwrap_or(false);
            // Post at this exact point in the stream, bypassing this tap. A
            // subsequent key cannot overtake Backspace or enter inside the edit.
            if committed {
                for key in &job.request.prepared.events {
                    unsafe { CGEventTapPostEvent(proxy, key.0.cast_mut()) };
                }
                job.applied = true;
            }
        }
        job.flush(proxy);
        // The empty key-up is only a trigger, never input to the application.
        return std::ptr::null_mut();
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
    Stop,
}

struct Ready(Owned);
// CFRunLoop references are retained and CFRunLoopStop is thread-safe.
unsafe impl Send for Ready {}

struct Worker {
    pid: i32,
    commands: mpsc::Sender<Command>,
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
        let handle = std::thread::Builder::new()
            .name("upyr-terminal-edit".to_owned())
            .spawn(move || {
                let errors = ready_sender.clone();
                if let Err(error) = run(pid, receiver, ready_sender) {
                    let _ = errors.send(Err(error.to_string()));
                }
            })
            .context("could not start the terminal edit worker")?;
        Ok(Self {
            pid,
            commands,
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
                stamp,
                reply,
            }))
            .map_err(|_| anyhow::anyhow!("terminal event tap stopped"))?;
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
) -> Result<()> {
    let mut state = Box::new(State {
        target_pid,
        job: None,
        disabled: false,
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
                    state.job = Some(Job {
                        request,
                        deadline: Instant::now() + Duration::from_millis(250),
                        cancelled: false,
                        finished: false,
                        applied: false,
                        queued: Vec::with_capacity(16),
                        queued_keydowns: 0,
                    });
                    if let Some(job) = &state.job {
                        unsafe {
                            CGEventPostToPid(target_pid, job.request.prepared.trigger.0.cast_mut())
                        };
                    }
                }
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
                        "terminal edit target did not receive its trigger"
                    );
                }
                for key in job.queued.drain(..) {
                    unsafe { CGEventPostToPid(target_pid, key.0.cast_mut()) };
                }
                let _ = job.request.reply.send(Reply::Complete(job.applied));
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
        let _ = job.request.reply.send(Reply::Complete(false));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
