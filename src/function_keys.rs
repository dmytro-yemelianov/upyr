//! The macOS function-key mode: whether F1–F12 send standard function keys
//! or the printed brightness, volume and media actions. Fn reverses either.
//!
//! The HID system's `HIDFKeyMode` parameter switches the mode immediately.
//! The global `com.apple.keyboard.fnState` preference keeps it after a
//! restart and is the setting System Settings shows.
#![allow(unsafe_code)]

use anyhow::{Result, bail};
use std::ffi::{CStr, c_char, c_void};

type Ref = *const c_void;

const HID_FKEY_MODE: &CStr = c"HIDFKeyMode";
const FN_STATE: &CStr = c"com.apple.keyboard.fnState";

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> Ref;
    fn IOServiceGetMatchingService(main_port: u32, matching: Ref) -> u32;
    fn IOServiceOpen(service: u32, owning_task: u32, kind: u32, connect: *mut u32) -> i32;
    fn IOServiceClose(connect: u32) -> i32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IOHIDGetParameter(
        connect: u32,
        key: Ref,
        max_size: u32,
        bytes: *mut c_void,
        actual_size: *mut u32,
    ) -> i32;
    fn IOHIDSetParameter(connect: u32, key: Ref, bytes: *const c_void, size: u32) -> i32;
    static mach_task_self_: u32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFPreferencesAnyApplication: Ref;
    static kCFPreferencesCurrentUser: Ref;
    static kCFPreferencesAnyHost: Ref;
    static kCFBooleanTrue: Ref;
    static kCFBooleanFalse: Ref;
    fn CFStringCreateWithCString(allocator: Ref, string: *const c_char, encoding: u32) -> Ref;
    fn CFPreferencesSetValue(key: Ref, value: Ref, application: Ref, user: Ref, host: Ref);
    fn CFPreferencesSynchronize(application: Ref, user: Ref, host: Ref) -> u8;
    fn CFRelease(value: Ref);
}

/// `HIDFKeyMode` for standard F1–F12 (1) or the media actions (0).
const fn hid_mode(standard: bool) -> u32 {
    if standard { 1 } else { 0 }
}

const fn is_standard(mode: u32) -> bool {
    mode != 0
}

/// Whether F1–F12 currently act as standard function keys.
pub fn standard() -> Result<bool> {
    let hid = HidSystem::open()?;
    let key = CfString::new(HID_FKEY_MODE)?;
    let mut mode = 0_u32;
    let mut size = 0_u32;
    let status = unsafe {
        IOHIDGetParameter(
            hid.0,
            key.0,
            size_of::<u32>() as u32,
            (&raw mut mode).cast(),
            &mut size,
        )
    };
    if status != 0 || size as usize != size_of::<u32>() {
        bail!("could not read the function-key mode (IOKit error {status:#x})");
    }
    Ok(is_standard(mode))
}

/// Makes F1–F12 standard function keys or media keys now and after restarts.
pub fn set_standard(standard: bool) -> Result<()> {
    let hid = HidSystem::open()?;
    let key = CfString::new(HID_FKEY_MODE)?;
    let mode = hid_mode(standard);
    let status = unsafe {
        IOHIDSetParameter(
            hid.0,
            key.0,
            (&raw const mode).cast(),
            size_of::<u32>() as u32,
        )
    };
    if status != 0 {
        bail!("could not switch the function-key mode (IOKit error {status:#x})");
    }
    let preference = CfString::new(FN_STATE)?;
    unsafe {
        let value = if standard {
            kCFBooleanTrue
        } else {
            kCFBooleanFalse
        };
        CFPreferencesSetValue(
            preference.0,
            value,
            kCFPreferencesAnyApplication,
            kCFPreferencesCurrentUser,
            kCFPreferencesAnyHost,
        );
        if CFPreferencesSynchronize(
            kCFPreferencesAnyApplication,
            kCFPreferencesCurrentUser,
            kCFPreferencesAnyHost,
        ) == 0
        {
            bail!("switched the function keys but could not save the setting for restarts");
        }
    }
    Ok(())
}

/// A parameter connection to the HID system, closed on drop.
struct HidSystem(u32);

impl HidSystem {
    fn open() -> Result<Self> {
        const MAIN_PORT_DEFAULT: u32 = 0;
        const HID_PARAM_CONNECT_TYPE: u32 = 1;
        unsafe {
            let matching = IOServiceMatching(c"IOHIDSystem".as_ptr());
            if matching.is_null() {
                bail!("could not describe the HID system service");
            }
            // Consumes `matching`.
            let service = IOServiceGetMatchingService(MAIN_PORT_DEFAULT, matching);
            if service == 0 {
                bail!("the HID system service is unavailable");
            }
            let mut connect = 0;
            let status = IOServiceOpen(
                service,
                mach_task_self_,
                HID_PARAM_CONNECT_TYPE,
                &mut connect,
            );
            IOObjectRelease(service);
            if status != 0 {
                bail!("could not open the HID system (IOKit error {status:#x})");
            }
            Ok(Self(connect))
        }
    }
}

impl Drop for HidSystem {
    fn drop(&mut self) {
        unsafe { IOServiceClose(self.0) };
    }
}

struct CfString(Ref);

impl CfString {
    fn new(value: &CStr) -> Result<Self> {
        let string =
            unsafe { CFStringCreateWithCString(std::ptr::null(), value.as_ptr(), 0x0800_0100) };
        if string.is_null() {
            bail!("could not create a Core Foundation string");
        }
        Ok(Self(string))
    }
}

impl Drop for CfString {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_keys_use_hid_mode_one_and_media_keys_mode_zero() {
        assert_eq!(hid_mode(true), 1);
        assert_eq!(hid_mode(false), 0);
    }

    fn saved_setting() -> Option<String> {
        let output = std::process::Command::new("defaults")
            .args(["read", "-g", "com.apple.keyboard.fnState"])
            .output()
            .unwrap();
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    #[test]
    #[ignore = "switches this Mac's function-key mode and back; run by hand"]
    fn switching_changes_the_live_mode_and_the_saved_setting() {
        let original = standard().unwrap();
        let original_setting = saved_setting();

        set_standard(!original).unwrap();
        assert_eq!(standard().unwrap(), !original);
        assert_eq!(
            saved_setting().as_deref(),
            Some(if original { "0" } else { "1" })
        );

        set_standard(original).unwrap();
        assert_eq!(standard().unwrap(), original);
        if original_setting.is_none() {
            let _ = std::process::Command::new("defaults")
                .args(["delete", "-g", "com.apple.keyboard.fnState"])
                .status();
        }
        assert_eq!(saved_setting(), original_setting);
    }

    #[test]
    fn any_nonzero_hid_mode_means_standard_keys() {
        assert!(!is_standard(0));
        assert!(is_standard(1));
        assert!(is_standard(2));
    }
}
