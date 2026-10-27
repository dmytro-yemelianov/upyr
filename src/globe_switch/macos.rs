#![allow(unsafe_code)]

use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, c_char, c_void},
    marker::PhantomData,
    process::Command,
    rc::Rc,
};

use anyhow::{Context, Result, bail};
use objc2::MainThreadMarker;
use tracing::{error, warn};

use super::{GlobeSettings, TapDetector};
use crate::system_layout::{self, ActiveInputSource};

type Ref = *const c_void;
type Event = *mut c_void;
type Callback = unsafe extern "C" fn(Ref, u32, Event, *mut c_void) -> Event;

const FN_KEY: u32 = 63;
const FN_FLAG: u64 = 1 << 23;
const OTHER_MODIFIERS: u64 = (1 << 16) | (1 << 17) | (1 << 18) | (1 << 19) | (1 << 20);
const FN_USAGE: &str = "AppleFnUsageType";
const SYSTEM_DOMAIN: &str = "com.apple.HIToolbox";
const OWN_DOMAIN: &str = "dev.Upyr.Upyr.GlobeSwitch";
const PREVIOUS: &str = "PreviousFnUsageType";

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        location: u32,
        placement: u32,
        options: u32,
        mask: u64,
        callback: Callback,
        context: *mut c_void,
    ) -> Ref;
    fn CGEventTapEnable(tap: Ref, enable: bool);
    fn CGEventGetIntegerValueField(event: Event, field: u32) -> i64;
    fn CGEventGetFlags(event: Event) -> u64;
    fn CGEventGetTimestamp(event: Event) -> u64;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    static kTISNotifySelectedKeyboardInputSourceChanged: Ref;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: Ref;
    fn CFRelease(value: Ref);
    fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
    fn CFMachPortInvalidate(port: Ref);
    fn CFRunLoopGetMain() -> Ref;
    fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRemoveSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFNotificationCenterGetDistributedCenter() -> Ref;
    fn CFNotificationCenterAddObserver(
        center: Ref,
        observer: Ref,
        callback: unsafe extern "C" fn(Ref, *mut c_void, Ref, Ref, Ref),
        name: Ref,
        object: Ref,
        suspension: isize,
    );
    fn CFNotificationCenterRemoveObserver(center: Ref, observer: Ref, name: Ref, object: Ref);
    fn CFStringCreateWithCString(allocator: Ref, string: *const c_char, encoding: u32) -> Ref;
    fn CFStringGetCString(string: Ref, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberCreate(allocator: Ref, kind: isize, value: *const i32) -> Ref;
    fn CFNumberGetValue(number: Ref, kind: isize, value: *mut c_void) -> u8;
    fn CFPreferencesCopyAppValue(key: Ref, domain: Ref) -> Ref;
    fn CFPreferencesSetAppValue(key: Ref, value: Ref, domain: Ref);
    fn CFPreferencesAppSynchronize(domain: Ref) -> u8;
}

struct Owned(Ref);

impl Owned {
    fn new(raw: Ref) -> Result<Self> {
        if raw.is_null() {
            bail!("macOS returned an unavailable object");
        }
        Ok(Self(raw))
    }

    fn string(value: &str) -> Result<Self> {
        let value = std::ffi::CString::new(value)?;
        Self::new(unsafe {
            CFStringCreateWithCString(std::ptr::null(), value.as_ptr(), 0x0800_0100)
        })
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

struct State {
    detector: TapDetector,
    settings: GlobeSettings,
    history: Rc<History>,
    tap: Ref,
    on_switch: Box<dyn FnMut(Option<String>, ActiveInputSource)>,
}

#[derive(Default)]
struct History {
    recent: RefCell<Vec<String>>,
    generation: Cell<u64>,
}

impl History {
    fn remember_current(&self) {
        if let Ok(Some(current)) = system_layout::current() {
            let mut recent = self.recent.borrow_mut();
            recent.retain(|id| id != &current.source_id);
            recent.insert(0, current.source_id);
            recent.truncate(32);
        }
    }
}

impl State {
    fn observe(&mut self, kind: u32, event: Event) {
        if kind == u32::MAX - 1 || kind == u32::MAX {
            self.detector.reset();
            unsafe { CGEventTapEnable(self.tap, true) };
            return;
        }
        if kind != 12 || unsafe { CGEventGetIntegerValueField(event, 9) } != i64::from(FN_KEY) {
            self.detector.other();
            return;
        }
        let flags = unsafe { CGEventGetFlags(event) };
        let at = unsafe { CGEventGetTimestamp(event) };
        if flags & FN_FLAG != 0 {
            self.history.remember_current();
            self.detector.down(at, flags & OTHER_MODIFIERS != 0);
        } else {
            if flags & OTHER_MODIFIERS != 0 {
                self.detector.other();
            }
            if !self
                .detector
                .up(at, self.settings.tap_window_ms * 1_000_000)
            {
                return;
            }
            // A changed system preference must never turn one tap into two switches.
            if fn_usage().ok().flatten() != Some(0) {
                warn!(
                    "macOS is handling the Globe key again; reload Upyr configuration to reclaim it"
                );
                return;
            }
            let recent = self.history.recent.borrow().clone();
            let previous = system_layout::current()
                .ok()
                .flatten()
                .map(|source| source.source_id);
            match system_layout::cycle(&self.settings, &recent) {
                Ok(Some(selected)) => {
                    self.history
                        .generation
                        .set(self.history.generation.get().wrapping_add(1));
                    self.history.remember_current();
                    (self.on_switch)(previous, selected);
                }
                Ok(None) => {}
                Err(error) => error!(%error, "Globe-key input-source switch failed"),
            }
        }
    }
}

unsafe extern "C" fn callback(_: Ref, kind: u32, event: Event, context: *mut c_void) -> Event {
    // The pointer is owned by GlobeMonitor and removed from the main run loop
    // before that owner is dropped. No Rust unwind may cross the C boundary.
    let cell = unsafe { &*context.cast::<RefCell<State>>() };
    let Ok(mut state) = cell.try_borrow_mut() else {
        return event;
    };
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| state.observe(kind, event)))
        .is_err()
    {
        state.detector.reset();
        error!("Globe-key observer recovered from a callback panic");
    }
    event // listen-only: always return the original event
}

unsafe extern "C" fn source_changed(_: Ref, context: *mut c_void, _: Ref, _: Ref, _: Ref) {
    // This observer has its own shared object: TIS selection may notify us
    // synchronously while the event callback still holds &mut State.
    let history = unsafe { &*context.cast::<History>() };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| history.remember_current()));
}

/// A listen-only tap in Upyr's existing main run loop, with no helper process.
/// The marker prevents moving or dropping it on a different thread.
pub struct GlobeMonitor {
    state: Rc<RefCell<State>>,
    source: Owned,
    tap: Owned,
    _main_thread: PhantomData<Rc<()>>,
    _instance: single_instance::SingleInstance,
}

impl GlobeMonitor {
    pub fn start(
        settings: &GlobeSettings,
        on_switch: impl FnMut(Option<String>, ActiveInputSource) + 'static,
    ) -> Result<Self> {
        if MainThreadMarker::new().is_none() {
            bail!("the Globe-key monitor must start on the main thread");
        }
        let instance = acquire_globe_instance()?;
        if !instance.is_single() {
            bail!("another Upyr process already handles the Globe key");
        }
        if legacy_agent_exists() {
            bail!(
                "standalone GlobeSwitch is installed; run `upyr globe migrate` first to avoid double switching"
            );
        }
        if !unsafe { CGPreflightListenEventAccess() } && !crate::accessibility::is_trusted() {
            unsafe { CGRequestListenEventAccess() };
            bail!("grant Upyr Input Monitoring or Accessibility permission, then restart Upyr");
        }
        let state = Rc::new(RefCell::new(State {
            detector: TapDetector::default(),
            settings: settings.clone(),
            history: Rc::default(),
            tap: std::ptr::null(),
            on_switch: Box::new(on_switch),
        }));
        state.borrow().history.remember_current();
        // keyDown, flagsChanged, mouse down, scroll, NX_SYSDEFINED (media keys).
        let mask = [1, 3, 10, 12, 14, 22, 25]
            .into_iter()
            .fold(0_u64, |mask, kind| mask | (1 << kind));
        let tap = Owned::new(unsafe {
            CGEventTapCreate(
                1,
                0,
                1,
                mask,
                callback,
                Rc::as_ptr(&state).cast_mut().cast(),
            )
        })
        .context("could not create the listen-only Globe-key event tap")?;
        state.borrow_mut().tap = tap.0;
        let source =
            Owned::new(unsafe { CFMachPortCreateRunLoopSource(std::ptr::null(), tap.0, 0) })?;
        take_system_setting()?;
        unsafe {
            CFNotificationCenterAddObserver(
                CFNotificationCenterGetDistributedCenter(),
                Rc::as_ptr(&state.borrow().history).cast(),
                source_changed,
                kTISNotifySelectedKeyboardInputSourceChanged,
                std::ptr::null(),
                4,
            );
            CFRunLoopAddSource(CFRunLoopGetMain(), source.0, kCFRunLoopCommonModes);
            CGEventTapEnable(tap.0, true);
        }
        Ok(Self {
            state,
            source,
            tap,
            _main_thread: PhantomData,
            _instance: instance,
        })
    }

    pub fn generation(&self) -> u64 {
        self.state.borrow().history.generation.get()
    }

    pub fn reconfigure(&mut self, settings: &GlobeSettings) -> Result<()> {
        take_system_setting()?;
        let mut state = self.state.borrow_mut();
        state.detector.reset();
        state.settings = settings.clone();
        Ok(())
    }
}

impl Drop for GlobeMonitor {
    fn drop(&mut self) {
        unsafe {
            CFNotificationCenterRemoveObserver(
                CFNotificationCenterGetDistributedCenter(),
                Rc::as_ptr(&self.state.borrow().history).cast(),
                kTISNotifySelectedKeyboardInputSourceChanged,
                std::ptr::null(),
            );
            CGEventTapEnable(self.tap.0, false);
            CFRunLoopRemoveSource(CFRunLoopGetMain(), self.source.0, kCFRunLoopCommonModes);
            CFMachPortInvalidate(self.tap.0);
        }
        self.state.borrow_mut().detector.reset();
        if let Err(error) = restore_preferences() {
            warn!(%error, "could not restore the system Globe-key setting; run `upyr globe restore`");
        }
    }
}

fn preference(key: &str, domain: &str) -> Result<Option<Owned>> {
    let key = Owned::string(key)?;
    let domain = Owned::string(domain)?;
    unsafe { CFPreferencesAppSynchronize(domain.0) };
    let value = unsafe { CFPreferencesCopyAppValue(key.0, domain.0) };
    Ok((!value.is_null()).then(|| Owned(value)))
}

fn write_preference(key: &str, value: Option<&Owned>, domain: &str) -> Result<()> {
    let key = Owned::string(key)?;
    let domain = Owned::string(domain)?;
    unsafe {
        CFPreferencesSetAppValue(
            key.0,
            value.map_or(std::ptr::null(), |value| value.0),
            domain.0,
        )
    };
    if unsafe { CFPreferencesAppSynchronize(domain.0) } == 0 {
        bail!("could not save macOS keyboard preferences");
    }
    Ok(())
}

fn fn_usage() -> Result<Option<i32>> {
    let Some(value) = preference(FN_USAGE, SYSTEM_DOMAIN)? else {
        return Ok(None);
    };
    let mut number = 0;
    if unsafe {
        CFGetTypeID(value.0) != CFNumberGetTypeID()
            || CFNumberGetValue(value.0, 3, std::ptr::from_mut(&mut number).cast()) == 0
    } {
        bail!("AppleFnUsageType is not an integer");
    }
    Ok(Some(number))
}

fn set_fn_usage(value: Option<i32>) -> Result<()> {
    let number = value
        .map(|number| Owned::new(unsafe { CFNumberCreate(std::ptr::null(), 3, &number) }))
        .transpose()?;
    write_preference(FN_USAGE, number.as_ref(), SYSTEM_DOMAIN)
}

fn take_system_setting() -> Result<()> {
    let newly_owned = preference(PREVIOUS, OWN_DOMAIN)?.is_none();
    if newly_owned {
        let previous = fn_usage()?.map_or_else(|| "unset".to_owned(), |value| value.to_string());
        write_preference(PREVIOUS, Some(&Owned::string(&previous)?), OWN_DOMAIN)?;
    }
    let changed = fn_usage()? != Some(0);
    if changed {
        if let Err(error) = set_fn_usage(Some(0)) {
            let _ = restore_preferences();
            return Err(error);
        }
    }
    // Even an existing Do Nothing value may be cached as Change Input Source
    // by a helper which was launched before it changed.
    if changed || newly_owned {
        restart_input_agents();
    }
    Ok(())
}

/// Also works after an unclean exit: the saved value lives in preferences.
pub fn restore_system_setting() -> Result<()> {
    let instance = acquire_globe_instance()?;
    // A live monitor owns the preference lease. Disabling its configuration
    // makes it restore on drop; a separate UPYR_CONFIG must not steal its key.
    if instance.is_single() {
        restore_preferences()?;
    }
    Ok(())
}

fn acquire_globe_instance() -> Result<single_instance::SingleInstance> {
    let dirs = directories::BaseDirs::new().context("could not find the home directory")?;
    let directory = dirs.home_dir().join("Library/Application Support/Upyr");
    std::fs::create_dir_all(&directory)?;
    single_instance::SingleInstance::new(&directory.join("globe.lock").to_string_lossy())
        .context("could not acquire the Globe-key monitor lock")
}

fn restore_preferences() -> Result<()> {
    let Some(previous) = preference(PREVIOUS, OWN_DOMAIN)? else {
        return Ok(());
    };
    let mut buffer = [0_i8; 64];
    if unsafe {
        CFGetTypeID(previous.0) != CFStringGetTypeID()
            || CFStringGetCString(previous.0, buffer.as_mut_ptr(), 64, 0x0800_0100) == 0
    } {
        bail!("the saved Globe-key setting is invalid; it was left in place for recovery");
    }
    let saved = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_str()?;
    let saved = if saved == "unset" {
        None
    } else {
        Some(saved.parse::<i32>()?)
    };
    // Preserve an explicit change made in System Settings while Upyr ran.
    if fn_usage()? == Some(0) {
        set_fn_usage(saved)?;
        restart_input_agents();
    }
    write_preference(PREVIOUS, None, OWN_DOMAIN)
}

fn restart_input_agents() {
    for name in ["TextInputMenuAgent", "TextInputSwitcher"] {
        let _ = Command::new("/usr/bin/killall").arg(name).output();
    }
}

pub(super) fn legacy_agent_exists() -> bool {
    directories::BaseDirs::new().is_some_and(|dirs| {
        dirs.home_dir()
            .join("Library/LaunchAgents/dev.yemelianov.globe-switch.plist")
            .exists()
    })
}

pub fn diagnostics() -> String {
    let permission =
        unsafe { CGPreflightListenEventAccess() } || crate::accessibility::is_trusted();
    let usage = fn_usage()
        .map(|value| value.map_or_else(|| "macOS default".to_owned(), |value| value.to_string()))
        .unwrap_or_else(|error| error.to_string());
    format!(
        "input permission: {permission}; AppleFnUsageType: {usage}; standalone GlobeSwitch: {}",
        legacy_agent_exists()
    )
}
