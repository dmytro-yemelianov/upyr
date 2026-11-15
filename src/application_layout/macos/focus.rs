//! Read focus ownership, never the focused field's value or typed text.
use std::{ffi::c_void, ptr};

type Ref = *const c_void;
type Callback = unsafe extern "C" fn(Ref, Ref, Ref, *mut c_void);

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> Ref;
    fn AXUIElementCreateSystemWide() -> Ref;
    fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
    fn AXUIElementGetPid(element: Ref, pid: *mut i32) -> i32;
    fn AXUIElementSetMessagingTimeout(element: Ref, seconds: f32) -> i32;
    fn AXObserverCreate(pid: i32, callback: Callback, observer: *mut Ref) -> i32;
    fn AXObserverAddNotification(
        observer: Ref,
        element: Ref,
        name: Ref,
        context: *mut c_void,
    ) -> i32;
    fn AXObserverGetRunLoopSource(observer: Ref) -> Ref;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: Ref;
    fn CFRunLoopGetMain() -> Ref;
    fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRemoveSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFRelease(value: Ref);
    fn CFStringCreateWithCString(allocator: Ref, string: *const i8, encoding: u32) -> Ref;
}

struct Owned(Ref);
impl Owned {
    fn new(value: Ref) -> Option<Self> {
        (!value.is_null()).then(|| Self(value))
    }

    fn string(value: &std::ffi::CStr) -> Option<Self> {
        Self::new(unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr(), 0x0800_0100) })
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

pub(super) fn focused_pid() -> Option<i32> {
    let system = Owned::new(unsafe { AXUIElementCreateSystemWide() })?;
    unsafe { AXUIElementSetMessagingTimeout(system.0, 0.05) };
    for name in [c"AXFocusedUIElement", c"AXFocusedApplication"] {
        let attribute = Owned::string(name)?;
        let mut element = ptr::null();
        if unsafe { AXUIElementCopyAttributeValue(system.0, attribute.0, &mut element) } != 0 {
            continue;
        }
        let Some(element) = Owned::new(element) else {
            continue;
        };
        let mut pid = 0;
        if unsafe { AXUIElementGetPid(element.0, &mut pid) } == 0 && pid > 0 {
            return Some(pid);
        }
    }
    None
}

struct Handler(Box<dyn Fn()>);

pub(super) struct Observer {
    observer: Owned,
    _element: Owned,
    // Stable address until the run-loop source has been removed.
    _handler: Box<Handler>,
}

unsafe extern "C" fn changed(_: Ref, _: Ref, _: Ref, context: *mut c_void) {
    if objc2::MainThreadMarker::new().is_none() || context.is_null() {
        return;
    }
    let handler = unsafe { &*context.cast::<Handler>() };
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (handler.0)())).is_err() {
        tracing::error!("application focus observer recovered from a panic");
    }
}

impl Observer {
    pub(super) fn new(pid: i32, handler: impl Fn() + 'static) -> Option<Self> {
        let element = Owned::new(unsafe { AXUIElementCreateApplication(pid) })?;
        unsafe { AXUIElementSetMessagingTimeout(element.0, 0.05) };
        let mut observer = ptr::null();
        if unsafe { AXObserverCreate(pid, changed, &mut observer) } != 0 {
            return None;
        }
        let observer = Owned::new(observer)?;
        let mut handler = Box::new(Handler(Box::new(handler)));
        let context = (&raw mut *handler).cast();
        let mut subscribed = false;
        for name in [
            c"AXFocusedUIElementChanged",
            c"AXFocusedWindowChanged",
            c"AXApplicationActivated",
            c"AXApplicationDeactivated",
            c"AXUIElementDestroyed",
        ] {
            let name = Owned::string(name)?;
            let status =
                unsafe { AXObserverAddNotification(observer.0, element.0, name.0, context) };
            subscribed |= status == 0;
        }
        if !subscribed {
            return None;
        }
        unsafe {
            CFRunLoopAddSource(
                CFRunLoopGetMain(),
                AXObserverGetRunLoopSource(observer.0),
                kCFRunLoopCommonModes,
            );
        }
        Some(Self {
            observer,
            _element: element,
            _handler: handler,
        })
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        // All construction, delivery and removal happen on the main thread.
        // Remove delivery before dropping the context or releasing the observer.
        unsafe {
            CFRunLoopRemoveSource(
                CFRunLoopGetMain(),
                AXObserverGetRunLoopSource(self.observer.0),
                kCFRunLoopCommonModes,
            );
        }
    }
}
