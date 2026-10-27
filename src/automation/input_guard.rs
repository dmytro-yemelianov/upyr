//! A short-lived, listen-only guard for an explicitly requested conversion.
//! It observes activity, never characters, and ignores Upyr's own input.
#![allow(unsafe_code)]

use anyhow::{Context, Result, bail};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_foundation::NSObjectProtocol;
use std::{cell::Cell, ffi::c_void, rc::Rc};

type Ref = *const c_void;
type Event = *mut c_void;
const MODIFIERS: u64 = (1 << 16) | (1 << 17) | (1 << 18) | (1 << 19) | (1 << 20) | (1 << 23);
const INPUT_KINDS: [u32; 6] = [1, 3, 10, 12, 22, 25];

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    static kCGWindowOwnerPID: Ref;
    static kCGWindowNumber: Ref;
    static kCGWindowLayer: Ref;
    static kCGWindowBounds: Ref;
    fn CGWindowListCopyWindowInfo(options: u32, relative: u32) -> Ref;
    fn CGRectMakeWithDictionaryRepresentation(
        dictionary: Ref,
        rect: *mut objc2_foundation::NSRect,
    ) -> bool;
    fn CGEventTapCreate(
        location: u32,
        placement: u32,
        options: u32,
        mask: u64,
        callback: unsafe extern "C" fn(Ref, u32, Event, *mut c_void) -> Event,
        context: *mut c_void,
    ) -> Ref;
    fn CGEventTapEnable(tap: Ref, enable: bool);
    fn CGEventGetIntegerValueField(event: Event, field: u32) -> i64;
    fn CGEventGetFlags(event: Event) -> u64;
    fn CGEventSourceFlagsState(state: i32) -> u64;
    fn CGEventSourceCounterForEventType(state: i32, kind: u32) -> u32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: Ref;
    fn CFRunLoopGetMain() -> Ref;
    fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
    fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRemoveSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFMachPortInvalidate(port: Ref);
    fn CFRelease(value: Ref);
    fn CFEqual(left: Ref, right: Ref) -> u8;
    fn CFArrayGetCount(array: Ref) -> isize;
    fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
    fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
    fn CFNumberGetValue(number: Ref, kind: isize, value: *mut c_void) -> u8;
    fn CFStringCreateWithCString(allocator: Ref, string: *const i8, encoding: u32) -> Ref;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateSystemWide() -> Ref;
    fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
    fn AXUIElementSetMessagingTimeout(element: Ref, seconds: f32) -> i32;
}

struct Owned(Ref);
impl Owned {
    fn new(value: Ref) -> Option<Self> {
        (!value.is_null()).then(|| Self(value))
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

fn focused_element() -> Option<Owned> {
    let system = Owned::new(unsafe { AXUIElementCreateSystemWide() })?;
    let attribute = Owned::new(unsafe {
        CFStringCreateWithCString(
            std::ptr::null(),
            c"AXFocusedUIElement".as_ptr(),
            0x0800_0100,
        )
    })?;
    unsafe { AXUIElementSetMessagingTimeout(system.0, 0.05) };
    let mut element = std::ptr::null();
    if unsafe { AXUIElementCopyAttributeValue(system.0, attribute.0, &mut element) } != 0 {
        return None;
    }
    Owned::new(element)
}

fn front_application() -> Option<Retained<NSRunningApplication>> {
    NSWorkspace::sharedWorkspace().frontmostApplication()
}

fn front_window(application: &NSRunningApplication) -> Option<i64> {
    let pid: i32 = unsafe { objc2::msg_send![application, processIdentifier] };
    visible_window(Some(pid)).map(|(_, window)| window)
}

pub(super) fn is_target_window(pid: i32, window: i64) -> bool {
    // A nonactivating window from another process can be above the keyboard
    // target. Application focus is checked on the main thread before commit.
    visible_window(Some(pid)) == Some((pid, window))
}

pub(super) fn input_counts() -> [u32; 6] {
    INPUT_KINDS.map(|kind| unsafe { CGEventSourceCounterForEventType(0, kind) })
}

pub(crate) fn modifiers_released() -> bool {
    (unsafe { CGEventSourceFlagsState(0) }) & (MODIFIERS & !(1 << 16)) == 0
}

fn visible_window(owner: Option<i32>) -> Option<(i32, i64)> {
    let windows = Owned::new(unsafe { CGWindowListCopyWindowInfo(1, 0) })?;
    let number = |dictionary: Ref, key: Ref| -> Option<i64> {
        let value = unsafe { CFDictionaryGetValue(dictionary, key) };
        if value.is_null() {
            return None;
        }
        let mut result = 0_i64;
        (unsafe { CFNumberGetValue(value, 4, std::ptr::from_mut(&mut result).cast()) } != 0)
            .then_some(result)
    };
    // WindowServer returns front-to-back metadata. No window names or contents
    // are fetched; small title-bar companion windows are not input targets.
    for index in 0..unsafe { CFArrayGetCount(windows.0) } {
        let window = unsafe { CFArrayGetValueAtIndex(windows.0, index) };
        let Some(pid) = number(window, unsafe { kCGWindowOwnerPID }) else {
            continue;
        };
        if owner.is_some_and(|owner| pid != i64::from(owner))
            || number(window, unsafe { kCGWindowLayer }) != Some(0)
        {
            continue;
        }
        let bounds = unsafe { CFDictionaryGetValue(window, kCGWindowBounds) };
        let mut rect = objc2_foundation::NSRect::default();
        if !bounds.is_null()
            && unsafe { CGRectMakeWithDictionaryRepresentation(bounds, &mut rect) }
            && rect.size.width >= 64.0
            && rect.size.height >= 48.0
        {
            return Some((
                i32::try_from(pid).ok()?,
                number(window, unsafe { kCGWindowNumber })?,
            ));
        }
    }
    None
}

/// Identity only: no screen contents or typed text are read or retained.
#[derive(Clone)]
pub(crate) struct FocusTarget {
    element: Option<Rc<Owned>>,
    application: Retained<NSRunningApplication>,
    window: i64,
}

impl FocusTarget {
    pub fn capture() -> Option<Self> {
        MainThreadMarker::new()?;
        let application = front_application()?;
        let window = front_window(&application)?;
        let element = focused_element();
        let attribute = Owned::new(unsafe {
            CFStringCreateWithCString(std::ptr::null(), c"AXSubrole".as_ptr(), 0x0800_0100)
        })?;
        let secure = Owned::new(unsafe {
            CFStringCreateWithCString(std::ptr::null(), c"AXSecureTextField".as_ptr(), 0x0800_0100)
        })?;
        let mut subrole = std::ptr::null();
        if let Some(element) = element.as_ref() {
            if unsafe { AXUIElementCopyAttributeValue(element.0, attribute.0, &mut subrole) } == 0 {
                if let Some(subrole) = Owned::new(subrole) {
                    if unsafe { CFEqual(subrole.0, secure.0) != 0 } {
                        return None;
                    }
                }
            }
        }
        tracing::debug!(
            window,
            ax_available = element.is_some(),
            "captured terminal input target"
        );
        let pid: i32 = unsafe { objc2::msg_send![&*application, processIdentifier] };
        if let Err(error) = super::terminal_typing::warm_target(pid) {
            tracing::debug!(%error, "could not prepare the terminal input guard");
            return None;
        }
        Some(Self {
            element: element.map(Rc::new),
            application,
            window,
        })
    }

    pub fn is_current_for_undo(&self) -> bool {
        self.is_current()
            && self.element.as_ref().is_none_or(|original| {
                focused_element()
                    .is_some_and(|current| unsafe { CFEqual(original.0, current.0) != 0 })
            })
    }

    pub fn is_current(&self) -> bool {
        front_application().is_some_and(|current| current.isEqual(Some(&self.application)))
            && front_window(&self.application) == Some(self.window)
    }
}

struct Activity {
    interrupted: Cell<bool>,
    edited: Cell<bool>,
    modifiers: Cell<u64>,
}

unsafe extern "C" fn observe(_: Ref, kind: u32, event: Event, context: *mut c_void) -> Event {
    let activity = unsafe { &*context.cast::<Activity>() };
    if kind == u32::MAX || kind == u32::MAX - 1 {
        tracing::debug!("conversion activity guard was disabled by macOS");
        activity.interrupted.set(true);
        activity.edited.set(true);
        return event;
    }
    // kCGEventSourceUnixProcessID. No character or key-code field is read.
    if unsafe { CGEventGetIntegerValueField(event, 41) } == i64::from(std::process::id()) {
        return event;
    }
    if kind == 12 {
        let flags = unsafe { CGEventGetFlags(event) } & MODIFIERS;
        let previous = activity.modifiers.replace(flags);
        // Releasing the modifiers which triggered the conversion is expected.
        if flags & !previous != 0 {
            activity.interrupted.set(true);
            if flags & !previous & !(1 << 23) != 0 {
                activity.edited.set(true);
            }
        }
    } else {
        tracing::debug!(
            kind,
            source_pid = unsafe { CGEventGetIntegerValueField(event, 41) },
            "conversion interrupted by external activity"
        );
        activity.interrupted.set(true);
        activity.edited.set(true);
    }
    event
}

pub(super) struct InputGuard {
    activity: Rc<Activity>,
    source: Owned,
    tap: Owned,
    application: Retained<NSRunningApplication>,
    element: Option<Owned>,
    window: Option<i64>,
    terminal: bool,
    input_counts: [u32; 6],
}

impl InputGuard {
    pub fn target_pid(&self) -> i32 {
        unsafe { objc2::msg_send![&*self.application, processIdentifier] }
    }

    pub fn target_window(&self) -> Option<i64> {
        self.window
    }

    pub fn input_stamp(&self) -> [u32; 6] {
        self.input_counts
    }

    pub fn new(terminal: bool) -> Result<Self> {
        MainThreadMarker::new().context("conversion guards must run on the main thread")?;
        // Quartz counters catch input which reached WindowServer while the
        // main-thread observer was busy, before any destructive key is posted.
        let input_counts = input_counts();
        let application =
            front_application().context("there is no foreground application to convert text in")?;
        // Terminal guards must not block on a cross-process AX query. A next
        // letter can otherwise arrive during that query and be backspaced.
        let element = (!terminal).then(focused_element).flatten();
        let window = front_window(&application);
        let activity = Rc::new(Activity {
            interrupted: Cell::new(false),
            edited: Cell::new(false),
            modifiers: Cell::new(unsafe { CGEventSourceFlagsState(0) } & MODIFIERS),
        });
        let mask = [1, 3, 10, 12, 14, 22, 25]
            .into_iter()
            .fold(0_u64, |mask, kind| mask | (1 << kind));
        let Some(tap) = Owned::new(unsafe {
            CGEventTapCreate(
                1,
                1,
                1,
                mask,
                observe,
                Rc::as_ptr(&activity).cast_mut().cast(),
            )
        }) else {
            bail!("could not guard the conversion; check Accessibility permission");
        };
        let source =
            Owned::new(unsafe { CFMachPortCreateRunLoopSource(std::ptr::null(), tap.0, 0) })
                .context("could not attach the conversion input guard")?;
        unsafe {
            CFRunLoopAddSource(CFRunLoopGetMain(), source.0, kCFRunLoopCommonModes);
            CGEventTapEnable(tap.0, true);
        }
        Ok(Self {
            activity,
            source,
            tap,
            application,
            element,
            window,
            terminal,
            input_counts,
        })
    }

    fn same_target(&self) -> bool {
        if !front_application().is_some_and(|current| current.isEqual(Some(&self.application))) {
            tracing::debug!("conversion foreground application changed");
            return false;
        }
        if self.window.is_some() && front_window(&self.application) != self.window {
            return false;
        }
        let same_element = self.element.as_ref().is_none_or(|original| {
            focused_element().is_some_and(|current| unsafe { CFEqual(original.0, current.0) != 0 })
        });
        if !same_element {
            tracing::debug!("conversion focused element changed or became unavailable");
        }
        same_element
    }

    pub fn valid(&self) -> bool {
        if self.terminal {
            self.can_type()
        } else {
            !self.activity.interrupted.get() && self.same_target()
        }
    }

    pub fn can_type(&self) -> bool {
        !self.activity.interrupted.get()
            && self.has_target_focus()
            && unsafe { CGEventSourceFlagsState(0) } & (MODIFIERS & !(1 << 16)) == 0
            && input_counts() == self.input_counts
    }

    pub fn has_target_focus(&self) -> bool {
        front_application().is_some_and(|current| current.isEqual(Some(&self.application)))
            && self.window.is_some()
            && front_window(&self.application) == self.window
    }
    pub fn can_recover_selection(&self) -> bool {
        !self.activity.edited.get()
            && unsafe { CGEventSourceFlagsState(0) } & (MODIFIERS & !(1 << 16)) == 0
            && self.same_target()
    }

    pub fn defer_cancellation(&self) -> bool {
        // An injected recovery arrow while fn is held would become fn+arrow
        // and cancel GlobeSwitch's clean tap. Wait for release without edits.
        !self.activity.edited.get()
            && unsafe { CGEventSourceFlagsState(0) } & (1 << 23) != 0
            && self.same_target()
    }
}

impl Drop for InputGuard {
    fn drop(&mut self) {
        unsafe {
            CGEventTapEnable(self.tap.0, false);
            CFRunLoopRemoveSource(CFRunLoopGetMain(), self.source.0, kCFRunLoopCommonModes);
            CFMachPortInvalidate(self.tap.0);
        }
    }
}
