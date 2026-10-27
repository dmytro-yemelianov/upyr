use super::{OutputKind, OutputRoute};
use block2::{Block, RcBlock};
use std::{
    ffi::c_void,
    mem::size_of,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

type Ref = *const c_void;
const GLOBAL: u32 = u32::from_be_bytes(*b"glob");
const OUTPUT: u32 = u32::from_be_bytes(*b"outp");
const SYSTEM: u32 = 1;
const SYSTEM_OUTPUT: u32 = u32::from_be_bytes(*b"sOut");
const DEFAULT_OUTPUT: u32 = u32::from_be_bytes(*b"dOut");
const STREAMS: u32 = u32::from_be_bytes(*b"stm#");
const DATA_SOURCE: u32 = u32::from_be_bytes(*b"ssrc");
const JACK: u32 = u32::from_be_bytes(*b"jack");

#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}
type Callback = unsafe extern "C" fn(u32, u32, *const Address, *mut c_void) -> i32;
#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectHasProperty(object: u32, address: *const Address) -> u8;
    fn AudioObjectGetPropertyDataSize(
        object: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: Ref,
        size: *mut u32,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        object: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: Ref,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn AudioObjectAddPropertyListener(
        object: u32,
        address: *const Address,
        callback: Callback,
        context: *mut c_void,
    ) -> i32;
    fn AudioObjectRemovePropertyListener(
        object: u32,
        address: *const Address,
        callback: Callback,
        context: *mut c_void,
    ) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: Ref);
    fn CFStringGetLength(value: Ref) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFStringGetCString(
        value: Ref,
        buffer: *mut std::ffi::c_char,
        size: isize,
        encoding: u32,
    ) -> u8;
    fn CFRunLoopGetMain() -> Ref;
    static kCFRunLoopCommonModes: Ref;
    fn CFRunLoopPerformBlock(run_loop: Ref, mode: Ref, block: Ref);
    fn CFRunLoopWakeUp(run_loop: Ref);
}
fn address(selector: u32, scope: u32) -> Address {
    Address {
        selector,
        scope,
        element: 0,
    }
}
fn read<T: Copy + Default>(object: u32, selector: u32, scope: u32) -> Option<T> {
    let mut value = T::default();
    let mut size = size_of::<T>() as u32;
    (unsafe {
        AudioObjectGetPropertyData(
            object,
            &address(selector, scope),
            0,
            std::ptr::null(),
            &mut size,
            (&mut value as *mut T).cast(),
        )
    } == 0
        && size as usize == size_of::<T>())
    .then_some(value)
}
fn text(object: u32, selector: u32) -> Option<String> {
    let mut value: Ref = std::ptr::null();
    let mut size = size_of::<Ref>() as u32;
    if unsafe {
        AudioObjectGetPropertyData(
            object,
            &address(selector, GLOBAL),
            0,
            std::ptr::null(),
            &mut size,
            (&mut value as *mut Ref).cast(),
        )
    } != 0
        || value.is_null()
    {
        return None;
    }
    let capacity =
        unsafe { CFStringGetMaximumSizeForEncoding(CFStringGetLength(value), 0x0800_0100) }
            .checked_add(1);
    let result = capacity
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n <= 65_536)
        .and_then(|n| {
            let mut buffer = vec![0_u8; n];
            if unsafe {
                CFStringGetCString(value, buffer.as_mut_ptr().cast(), n as isize, 0x0800_0100)
            } == 0
            {
                return None;
            }
            let end = buffer.iter().position(|b| *b == 0)?;
            String::from_utf8(buffer[..end].to_vec()).ok()
        });
    unsafe { CFRelease(value) };
    result
}
pub(super) fn current() -> OutputRoute {
    let device = read::<u32>(SYSTEM, SYSTEM_OUTPUT, GLOBAL)
        .filter(|id| *id != 0)
        .or_else(|| read::<u32>(SYSTEM, DEFAULT_OUTPUT, GLOBAL))
        .unwrap_or(0);
    let name = text(device, u32::from_be_bytes(*b"lnam"))
        .unwrap_or_else(|| "Unavailable output".to_owned());
    let mut size = 0_u32;
    let mut terminals = Vec::new();
    if unsafe {
        AudioObjectGetPropertyDataSize(
            device,
            &address(STREAMS, OUTPUT),
            0,
            std::ptr::null(),
            &mut size,
        )
    } == 0
        && size <= 4096
        && size.is_multiple_of(4)
    {
        let mut streams = vec![0_u32; size as usize / 4];
        if unsafe {
            AudioObjectGetPropertyData(
                device,
                &address(STREAMS, OUTPUT),
                0,
                std::ptr::null(),
                &mut size,
                streams.as_mut_ptr().cast(),
            )
        } == 0
        {
            terminals = streams
                .into_iter()
                .filter_map(|stream| read::<u32>(stream, u32::from_be_bytes(*b"term"), GLOBAL))
                .collect();
        }
    }
    OutputRoute {
        kind: if device == 0 {
            OutputKind::Unknown
        } else {
            super::classify(&terminals, read(device, DATA_SOURCE, OUTPUT), &name)
        },
        name,
        uid: text(device, u32::from_be_bytes(*b"uid ")),
        device,
    }
}

// Process-lifetime subscriptions have no heap callback context to outlive.
// Core Audio callbacks only schedule work; NSSound stays on the AppKit thread.
static GENERATION: AtomicU64 = AtomicU64::new(0);
pub(crate) fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}
#[derive(Default)]
struct Watch {
    system: bool,
    device: u32,
    device_ready: bool,
    selectors: Vec<u32>,
}
static WATCH: OnceLock<Mutex<Watch>> = OnceLock::new();
unsafe extern "C" fn changed(_: u32, _: u32, _: *const Address, _: *mut c_void) -> i32 {
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let block = RcBlock::new(move || {
        crate::feedback::stop_rerouted_audio(generation);
        let _ = watch();
    });
    unsafe {
        let run_loop = CFRunLoopGetMain();
        CFRunLoopPerformBlock(
            run_loop,
            kCFRunLoopCommonModes,
            (&*block as *const Block<dyn Fn()>).cast(),
        );
        CFRunLoopWakeUp(run_loop);
    }
    0
}
pub(crate) fn watch() -> bool {
    let mut state = WATCH
        .get_or_init(|| Mutex::new(Watch::default()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if !state.system {
        // A failure uses the quiet gain even for a known speaker.
        state.system = unsafe {
            AudioObjectAddPropertyListener(
                SYSTEM,
                &address(SYSTEM_OUTPUT, GLOBAL),
                changed,
                std::ptr::null_mut(),
            )
        } == 0;
        if state.system {
            unsafe {
                AudioObjectAddPropertyListener(
                    SYSTEM,
                    &address(DEFAULT_OUTPUT, GLOBAL),
                    changed,
                    std::ptr::null_mut(),
                )
            };
        }
    }
    let device = current().device;
    if state.device != device {
        for selector in &state.selectors {
            unsafe {
                AudioObjectRemovePropertyListener(
                    state.device,
                    &address(*selector, OUTPUT),
                    changed,
                    std::ptr::null_mut(),
                )
            };
        }
        state.selectors.clear();
        state.device = device;
        state.device_ready = device != 0;
        for selector in [DATA_SOURCE, JACK, STREAMS] {
            if device != 0
                && unsafe { AudioObjectHasProperty(device, &address(selector, OUTPUT)) } != 0
            {
                if unsafe {
                    AudioObjectAddPropertyListener(
                        device,
                        &address(selector, OUTPUT),
                        changed,
                        std::ptr::null_mut(),
                    )
                } == 0
                {
                    state.selectors.push(selector);
                } else {
                    state.device_ready = false;
                }
            }
        }
    }
    state.system && state.device_ready
}
