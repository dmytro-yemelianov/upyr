use anyhow::Result;

/// The input-source families supported by Upyr's initial mapping.
pub use upyr_core::InputLayout as SystemLayout;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveInputSource {
    pub layout: Option<SystemLayout>,
    pub source_id: String,
    pub source_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchOutcome {
    Switched { source_id: String },
    AlreadyActive { source_id: String },
    TargetUnavailable,
    Unsupported,
}

/// Returns the current supported input source, or `None` for an unrelated
/// source such as an emoji palette or a layout Upyr does not map yet.
pub fn current() -> Result<Option<ActiveInputSource>> {
    platform::current()
}

/// Selects an installed input source matching the requested layout family.
pub fn switch_to(layout: SystemLayout) -> Result<SwitchOutcome> {
    platform::switch_to(layout)
}

/// Builds a physical-key character mapping from installed OS layouts when the
/// platform exposes a reliable translation API.
pub fn installed_mapping() -> Result<Option<Vec<(char, char)>>> {
    platform::installed_mapping()
}

#[cfg(any(target_os = "linux", test))]
fn parse_xkb_layouts(query: &str) -> Vec<String> {
    query
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(key, _)| key.trim() == "layout")
        })
        .map_or_else(Vec::new, |(_, layouts)| {
            layouts
                .split(',')
                .map(str::trim)
                .filter(|layout| !layout.is_empty())
                .map(str::to_owned)
                .collect()
        })
}

#[cfg(any(target_os = "linux", test))]
fn classify_xkb_layout(layout: &str) -> Option<SystemLayout> {
    match layout.trim().to_ascii_lowercase().as_str() {
        "us" => Some(SystemLayout::English),
        "ua" => Some(SystemLayout::Ukrainian),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod platform {
    // The narrow Carbon/CoreFoundation binding in this module was adapted
    // from issw (MIT); see THIRD_PARTY_NOTICES.md.
    use std::{
        collections::HashSet,
        ffi::{CStr, c_char, c_void},
        ptr::NonNull,
        sync::OnceLock,
    };

    use anyhow::{Result, anyhow, bail};

    use super::{ActiveInputSource, SwitchOutcome, SystemLayout};

    type Boolean = u8;
    type CfIndex = isize;
    type CfStringEncoding = u32;
    type OsStatus = i32;
    type CfArrayRef = *const c_void;
    type CfBooleanRef = *const c_void;
    type CfStringRef = *const c_void;
    type CfTypeRef = *const c_void;
    type TisInputSourceRef = *const c_void;

    const CF_STRING_ENCODING_UTF8: CfStringEncoding = 0x0800_0100;
    const KEY_ACTION_DOWN: u16 = 0;
    const NO_DEAD_KEYS: u32 = 1;
    const SHIFT_MODIFIER: u32 = 2;
    const OPTION_MODIFIER: u32 = 8;
    const MAX_TRANSLATED_UNITS: usize = 8;

    const KEY_CODES: &[u16] = &[
        12, 13, 14, 15, 17, 16, 32, 34, 31, 35, 33, 30, // Q through ]
        0, 1, 2, 3, 5, 4, 38, 40, 37, 41, 39, // A through '
        6, 7, 8, 9, 11, 45, 46, 43, 47, 44, 42, 50, // Z through ` and backslash
        18, 19, 20, 21, 23, 22, 26, 28, 25, 29, 27, 24, // 1 through =
    ];

    static INSTALLED_MAPPING: OnceLock<std::result::Result<Vec<(char, char)>, String>> =
        OnceLock::new();

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        static kTISCategoryKeyboardInputSource: CfStringRef;
        static kTISPropertyInputSourceCategory: CfStringRef;
        static kTISPropertyInputSourceID: CfStringRef;
        static kTISPropertyInputSourceIsSelectCapable: CfStringRef;
        static kTISPropertyLocalizedName: CfStringRef;
        static kTISPropertyUnicodeKeyLayoutData: CfStringRef;

        fn TISCopyCurrentKeyboardInputSource() -> TisInputSourceRef;
        fn TISCreateInputSourceList(
            properties: CfTypeRef,
            include_all_installed: Boolean,
        ) -> CfArrayRef;
        fn TISGetInputSourceProperty(
            input_source: TisInputSourceRef,
            property_key: CfStringRef,
        ) -> CfTypeRef;
        fn TISSelectInputSource(input_source: TisInputSourceRef) -> OsStatus;
        fn LMGetKbdType() -> u8;
        fn UCKeyTranslate(
            key_layout: *const c_void,
            virtual_key_code: u16,
            key_action: u16,
            modifier_key_state: u32,
            keyboard_type: u32,
            options: u32,
            dead_key_state: *mut u32,
            max_string_length: usize,
            actual_string_length: *mut usize,
            unicode_string: *mut u16,
        ) -> OsStatus;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFArrayGetCount(array: CfArrayRef) -> CfIndex;
        fn CFArrayGetValueAtIndex(array: CfArrayRef, index: CfIndex) -> *const c_void;
        fn CFBooleanGetValue(boolean: CfBooleanRef) -> Boolean;
        fn CFDataGetBytePtr(data: CfTypeRef) -> *const u8;
        fn CFEqual(left: CfTypeRef, right: CfTypeRef) -> Boolean;
        fn CFRelease(value: CfTypeRef);
        fn CFStringGetCString(
            string: CfStringRef,
            buffer: *mut c_char,
            buffer_size: CfIndex,
            encoding: CfStringEncoding,
        ) -> Boolean;
        fn CFStringGetCStringPtr(string: CfStringRef, encoding: CfStringEncoding) -> *const c_char;
        fn CFStringGetLength(string: CfStringRef) -> CfIndex;
        fn CFStringGetMaximumSizeForEncoding(
            length: CfIndex,
            encoding: CfStringEncoding,
        ) -> CfIndex;
    }

    pub fn current() -> Result<Option<ActiveInputSource>> {
        let source = unsafe {
            OwnedCf::from_create(
                TISCopyCurrentKeyboardInputSource(),
                "TISCopyCurrentKeyboardInputSource",
            )?
        };
        Ok(source_info(source.non_null()).map(SourceInfo::active_source))
    }

    pub fn switch_to(target: SystemLayout) -> Result<SwitchOutcome> {
        if let Some(active) = current()?
            && active.layout == Some(target)
        {
            return Ok(SwitchOutcome::AlreadyActive {
                source_id: active.source_id,
            });
        }

        let list = SourceList::load()?;
        let Some(source) = preferred_source(&list.sources, target) else {
            return Ok(SwitchOutcome::TargetUnavailable);
        };
        let status = unsafe { TISSelectInputSource(source.raw.as_ptr().cast_const()) };
        if status != 0 {
            bail!("TISSelectInputSource failed with OSStatus {status}");
        }

        Ok(SwitchOutcome::Switched {
            source_id: source.info.id.clone(),
        })
    }

    pub fn installed_mapping() -> Result<Option<Vec<(char, char)>>> {
        let mapping = INSTALLED_MAPPING
            .get_or_init(|| generate_installed_mapping().map_err(|error| format!("{error:#}")));
        match mapping {
            Ok(mapping) if mapping.is_empty() => Ok(None),
            Ok(mapping) => Ok(Some(mapping.clone())),
            Err(error) => Err(anyhow!(error.clone())),
        }
    }

    fn generate_installed_mapping() -> Result<Vec<(char, char)>> {
        let list = SourceList::load()?;
        let Some(english) = preferred_source(&list.sources, SystemLayout::English) else {
            return Ok(Vec::new());
        };
        let Some(ukrainian) = preferred_source(&list.sources, SystemLayout::Ukrainian) else {
            return Ok(Vec::new());
        };

        let english_layout = unicode_layout_data(english)?;
        let ukrainian_layout = unicode_layout_data(ukrainian)?;
        let keyboard_type = u32::from(unsafe { LMGetKbdType() });
        let mut mapping = Vec::new();
        let mut english_seen = HashSet::new();
        let mut ukrainian_seen = HashSet::new();

        for modifier in [
            0,
            SHIFT_MODIFIER,
            OPTION_MODIFIER,
            SHIFT_MODIFIER | OPTION_MODIFIER,
        ] {
            for key_code in KEY_CODES {
                let Some(english_character) =
                    translate_key(english_layout, *key_code, modifier, keyboard_type)
                else {
                    continue;
                };
                let Some(ukrainian_character) =
                    translate_key(ukrainian_layout, *key_code, modifier, keyboard_type)
                else {
                    continue;
                };
                if english_character == ukrainian_character
                    || english_seen.contains(&english_character)
                    || ukrainian_seen.contains(&ukrainian_character)
                {
                    continue;
                }
                english_seen.insert(english_character);
                ukrainian_seen.insert(ukrainian_character);
                mapping.push((english_character, ukrainian_character));
            }
        }

        if mapping.len() < 26 {
            bail!(
                "installed layouts yielded only {} distinct physical-key pairs",
                mapping.len()
            );
        }
        Ok(mapping)
    }

    fn unicode_layout_data(source: &InputSource) -> Result<*const u8> {
        let data = unsafe {
            TISGetInputSourceProperty(
                source.raw.as_ptr().cast_const(),
                kTISPropertyUnicodeKeyLayoutData,
            )
        };
        if data.is_null() {
            bail!(
                "input source {} has no Unicode key-layout data",
                source.info.id
            );
        }
        let bytes = unsafe { CFDataGetBytePtr(data) };
        if bytes.is_null() {
            bail!(
                "input source {} returned empty key-layout data",
                source.info.id
            );
        }
        Ok(bytes)
    }

    fn translate_key(
        layout: *const u8,
        key_code: u16,
        modifier: u32,
        keyboard_type: u32,
    ) -> Option<char> {
        let mut dead_key_state = 0;
        let mut length = 0;
        let mut units = [0_u16; MAX_TRANSLATED_UNITS];
        let status = unsafe {
            UCKeyTranslate(
                layout.cast(),
                key_code,
                KEY_ACTION_DOWN,
                modifier,
                keyboard_type,
                NO_DEAD_KEYS,
                &mut dead_key_state,
                units.len(),
                &mut length,
                units.as_mut_ptr(),
            )
        };
        if status != 0 || length == 0 || length > units.len() {
            return None;
        }
        let mut characters = char::decode_utf16(units[..length].iter().copied());
        match (characters.next(), characters.next()) {
            (Some(Ok(character)), None) if !character.is_control() => Some(character),
            _ => None,
        }
    }

    struct SourceList {
        sources: Vec<InputSource>,
        _array: OwnedCf,
    }

    impl SourceList {
        fn load() -> Result<Self> {
            let array = unsafe {
                OwnedCf::from_create(
                    TISCreateInputSourceList(std::ptr::null(), 0),
                    "TISCreateInputSourceList",
                )?
            };
            let count = unsafe { CFArrayGetCount(array.as_array()) };
            if count < 0 {
                bail!("TISCreateInputSourceList returned a negative source count");
            }

            let mut sources = Vec::with_capacity(count as usize);
            for index in 0..count {
                let raw = unsafe { CFArrayGetValueAtIndex(array.as_array(), index) };
                let Some(raw) = NonNull::new(raw.cast_mut()) else {
                    continue;
                };
                if !is_selectable_keyboard_source(raw.as_ptr().cast_const()) {
                    continue;
                }
                if let Some(info) = source_info(raw) {
                    sources.push(InputSource { raw, info });
                }
            }

            Ok(Self {
                sources,
                _array: array,
            })
        }
    }

    struct InputSource {
        raw: NonNull<c_void>,
        info: SourceInfo,
    }

    struct SourceInfo {
        id: String,
        name: String,
    }

    impl SourceInfo {
        fn active_source(self) -> ActiveInputSource {
            ActiveInputSource {
                layout: classify(&self.id, &self.name),
                source_id: self.id,
                source_name: self.name,
            }
        }
    }

    fn preferred_source(sources: &[InputSource], target: SystemLayout) -> Option<&InputSource> {
        sources
            .iter()
            .filter(|source| classify(&source.info.id, &source.info.name) == Some(target))
            .min_by_key(|source| preference(&source.info.id, target))
    }

    fn preference(id: &str, target: SystemLayout) -> u8 {
        let id = id.to_ascii_lowercase();
        match target {
            SystemLayout::English if id == "com.apple.keylayout.abc" => 0,
            SystemLayout::English if id == "com.apple.keylayout.us" => 1,
            SystemLayout::Ukrainian if id.ends_with(".ukrainian") => 0,
            SystemLayout::Ukrainian if id.ends_with(".ukrainian-pc") => 1,
            _ => 2,
        }
    }

    fn classify(id: &str, name: &str) -> Option<SystemLayout> {
        let id = id.to_lowercase();
        let name = name.to_lowercase();

        if id.contains("ukrainian") || name.contains("ukrainian") || name.contains("україн") {
            return Some(SystemLayout::Ukrainian);
        }
        if id == "com.apple.keylayout.abc"
            || id == "com.apple.keylayout.us"
            || matches!(name.as_str(), "abc" | "u.s." | "us" | "english")
        {
            return Some(SystemLayout::English);
        }
        None
    }

    fn source_info(raw: NonNull<c_void>) -> Option<SourceInfo> {
        let source = raw.as_ptr().cast_const();
        let id = string_property(source, unsafe { kTISPropertyInputSourceID })?;
        let name = string_property(source, unsafe { kTISPropertyLocalizedName })?;
        Some(SourceInfo { id, name })
    }

    fn is_selectable_keyboard_source(source: TisInputSourceRef) -> bool {
        if !bool_property(source, unsafe { kTISPropertyInputSourceIsSelectCapable }) {
            return false;
        }
        let category =
            unsafe { TISGetInputSourceProperty(source, kTISPropertyInputSourceCategory) };
        !category.is_null() && unsafe { CFEqual(category, kTISCategoryKeyboardInputSource) != 0 }
    }

    fn string_property(source: TisInputSourceRef, key: CfStringRef) -> Option<String> {
        let value = unsafe { TISGetInputSourceProperty(source, key) };
        cf_string_to_string(value.cast())
    }

    fn bool_property(source: TisInputSourceRef, key: CfStringRef) -> bool {
        let value = unsafe { TISGetInputSourceProperty(source, key) };
        !value.is_null() && unsafe { CFBooleanGetValue(value.cast()) != 0 }
    }

    fn cf_string_to_string(value: CfStringRef) -> Option<String> {
        if value.is_null() {
            return None;
        }

        let direct = unsafe { CFStringGetCStringPtr(value, CF_STRING_ENCODING_UTF8) };
        if !direct.is_null() {
            return unsafe { CStr::from_ptr(direct) }
                .to_str()
                .ok()
                .map(str::to_owned);
        }

        let length = unsafe { CFStringGetLength(value) };
        if length < 0 {
            return None;
        }
        let max_size =
            unsafe { CFStringGetMaximumSizeForEncoding(length, CF_STRING_ENCODING_UTF8) };
        let buffer_size = max_size.checked_add(1)?;
        let mut buffer = vec![0; usize::try_from(buffer_size).ok()?];
        let copied = unsafe {
            CFStringGetCString(
                value,
                buffer.as_mut_ptr(),
                buffer_size,
                CF_STRING_ENCODING_UTF8,
            )
        };
        if copied == 0 {
            return None;
        }
        unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_str()
            .ok()
            .map(str::to_owned)
    }

    struct OwnedCf {
        ptr: NonNull<c_void>,
    }

    impl OwnedCf {
        unsafe fn from_create(ptr: CfTypeRef, operation: &'static str) -> Result<Self> {
            NonNull::new(ptr.cast_mut()).map_or_else(
                || Err(anyhow!("{operation} returned null")),
                |ptr| Ok(Self { ptr }),
            )
        }

        fn as_array(&self) -> CfArrayRef {
            self.ptr.as_ptr().cast_const()
        }

        fn non_null(&self) -> NonNull<c_void> {
            self.ptr
        }
    }

    impl Drop for OwnedCf {
        fn drop(&mut self) {
            unsafe { CFRelease(self.ptr.as_ptr().cast_const()) };
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn classifies_initial_macos_sources() {
            assert_eq!(
                classify("com.apple.keylayout.US", "U.S."),
                Some(SystemLayout::English)
            );
            assert_eq!(
                classify("com.apple.keylayout.Ukrainian-PC", "Ukrainian – PC"),
                Some(SystemLayout::Ukrainian)
            );
            assert_eq!(classify("com.apple.keylayout.Dvorak", "Dvorak"), None);
        }

        #[test]
        fn prefers_standard_ukrainian_and_abc_english() {
            assert!(
                preference("com.apple.keylayout.Ukrainian", SystemLayout::Ukrainian)
                    < preference("com.apple.keylayout.Ukrainian-PC", SystemLayout::Ukrainian)
            );
            assert!(
                preference("com.apple.keylayout.ABC", SystemLayout::English)
                    < preference("com.apple.keylayout.US", SystemLayout::English)
            );
        }
    }
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod platform {
    use std::{collections::HashSet, ptr, sync::OnceLock};

    use anyhow::{Result, bail};
    use windows_sys::Win32::UI::{
        Input::KeyboardAndMouse::{
            GetKeyboardLayout, GetKeyboardLayoutList, HKL, MAPVK_VK_TO_VSC_EX, MapVirtualKeyExW,
            ToUnicodeEx,
        },
        WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, WM_INPUTLANGCHANGEREQUEST,
        },
    };

    use super::{ActiveInputSource, SwitchOutcome, SystemLayout};

    const ENGLISH_PRIMARY_LANGUAGE: u16 = 0x09;
    const UKRAINIAN_PRIMARY_LANGUAGE: u16 = 0x22;
    const US_ENGLISH_LANGUAGE_ID: u16 = 0x0409;
    const NO_KEYBOARD_STATE_CHANGE: u32 = 1 << 2;
    const KEY_PRESSED: u8 = 1 << 7;
    const VK_SHIFT: usize = 0x10;
    const VK_CONTROL: usize = 0x11;
    const VK_MENU: usize = 0x12;
    const MAX_TRANSLATED_UNITS: usize = 8;
    const KEY_CODES: &[u32] = &[
        0x51, 0x57, 0x45, 0x52, 0x54, 0x59, 0x55, 0x49, 0x4f, 0x50, 0xdb, 0xdd, // Q-]
        0x41, 0x53, 0x44, 0x46, 0x47, 0x48, 0x4a, 0x4b, 0x4c, 0xba, 0xde, // A-'
        0x5a, 0x58, 0x43, 0x56, 0x42, 0x4e, 0x4d, 0xbc, 0xbe, 0xbf, 0xdc, 0xc0, // Z-`
        0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x30, 0xbd, 0xbb, // 1-=
    ];

    static INSTALLED_MAPPING: OnceLock<std::result::Result<Vec<(char, char)>, String>> =
        OnceLock::new();

    pub fn current() -> Result<Option<ActiveInputSource>> {
        let layout = foreground_layout()?;
        Ok(Some(input_source(layout)))
    }

    pub fn switch_to(target: SystemLayout) -> Result<SwitchOutcome> {
        let active = foreground_layout()?;
        if classify(active) == Some(target) {
            return Ok(SwitchOutcome::AlreadyActive {
                source_id: source_id(active),
            });
        }

        let layouts = installed_layouts()?;
        let Some(layout) = preferred_layout(&layouts, target) else {
            return Ok(SwitchOutcome::TargetUnavailable);
        };
        let window = unsafe { GetForegroundWindow() };
        if window.is_null() {
            bail!("Windows did not report a foreground window");
        }
        let posted = unsafe { PostMessageW(window, WM_INPUTLANGCHANGEREQUEST, 0, layout as isize) };
        if posted == 0 {
            bail!("PostMessageW rejected the input-language change request");
        }
        Ok(SwitchOutcome::Switched {
            source_id: source_id(layout),
        })
    }

    pub fn installed_mapping() -> Result<Option<Vec<(char, char)>>> {
        let mapping = INSTALLED_MAPPING
            .get_or_init(|| generate_installed_mapping().map_err(|error| format!("{error:#}")));
        match mapping {
            Ok(mapping) if mapping.is_empty() => Ok(None),
            Ok(mapping) => Ok(Some(mapping.clone())),
            Err(error) => bail!(error.clone()),
        }
    }

    fn generate_installed_mapping() -> Result<Vec<(char, char)>> {
        let layouts = installed_layouts()?;
        let Some(english) = preferred_layout(&layouts, SystemLayout::English) else {
            return Ok(Vec::new());
        };
        let Some(ukrainian) = preferred_layout(&layouts, SystemLayout::Ukrainian) else {
            return Ok(Vec::new());
        };
        let mut mapping = Vec::new();
        let mut english_seen = HashSet::new();
        let mut ukrainian_seen = HashSet::new();

        for (shift, alt_gr) in [(false, false), (true, false), (false, true), (true, true)] {
            for key_code in KEY_CODES {
                let Some(english_character) = translate_key(english, *key_code, shift, alt_gr)
                else {
                    continue;
                };
                let Some(ukrainian_character) = translate_key(ukrainian, *key_code, shift, alt_gr)
                else {
                    continue;
                };
                if english_character == ukrainian_character
                    || english_seen.contains(&english_character)
                    || ukrainian_seen.contains(&ukrainian_character)
                {
                    continue;
                }
                english_seen.insert(english_character);
                ukrainian_seen.insert(ukrainian_character);
                mapping.push((english_character, ukrainian_character));
            }
        }

        if mapping.len() < 26 {
            bail!(
                "installed layouts yielded only {} distinct physical-key pairs",
                mapping.len()
            );
        }
        Ok(mapping)
    }

    fn translate_key(layout: HKL, key_code: u32, shift: bool, alt_gr: bool) -> Option<char> {
        let mut keyboard_state = [0_u8; 256];
        if shift {
            keyboard_state[VK_SHIFT] = KEY_PRESSED;
        }
        if alt_gr {
            keyboard_state[VK_CONTROL] = KEY_PRESSED;
            keyboard_state[VK_MENU] = KEY_PRESSED;
        }
        let scan_code = unsafe { MapVirtualKeyExW(key_code, MAPVK_VK_TO_VSC_EX, layout) };
        let mut units = [0_u16; MAX_TRANSLATED_UNITS];
        let length = unsafe {
            ToUnicodeEx(
                key_code,
                scan_code,
                keyboard_state.as_ptr(),
                units.as_mut_ptr(),
                units.len() as i32,
                NO_KEYBOARD_STATE_CHANGE,
                layout,
            )
        };
        if length <= 0 || length as usize > units.len() {
            return None;
        }
        let mut characters = char::decode_utf16(units[..length as usize].iter().copied());
        match (characters.next(), characters.next()) {
            (Some(Ok(character)), None) if !character.is_control() => Some(character),
            _ => None,
        }
    }

    fn foreground_layout() -> Result<HKL> {
        let window = unsafe { GetForegroundWindow() };
        let thread_id = if window.is_null() {
            0
        } else {
            unsafe { GetWindowThreadProcessId(window, ptr::null_mut()) }
        };
        let layout = unsafe { GetKeyboardLayout(thread_id) };
        if layout.is_null() {
            bail!("GetKeyboardLayout returned null");
        }
        Ok(layout)
    }

    fn installed_layouts() -> Result<Vec<HKL>> {
        let count = unsafe { GetKeyboardLayoutList(0, ptr::null_mut()) };
        if count <= 0 {
            bail!("GetKeyboardLayoutList returned no installed layouts");
        }
        let mut layouts = vec![ptr::null_mut(); count as usize];
        let written = unsafe { GetKeyboardLayoutList(count, layouts.as_mut_ptr()) };
        if written <= 0 {
            bail!("GetKeyboardLayoutList failed to return installed layouts");
        }
        layouts.truncate(written as usize);
        Ok(layouts)
    }

    fn preferred_layout(layouts: &[HKL], target: SystemLayout) -> Option<HKL> {
        layouts
            .iter()
            .copied()
            .filter(|layout| classify(*layout) == Some(target))
            .min_by_key(|layout| preference(*layout, target))
    }

    fn preference(layout: HKL, target: SystemLayout) -> u8 {
        match target {
            SystemLayout::English if language_id(layout) == US_ENGLISH_LANGUAGE_ID => 0,
            _ => 1,
        }
    }

    fn input_source(layout: HKL) -> ActiveInputSource {
        let family = classify(layout);
        ActiveInputSource {
            layout: family,
            source_id: source_id(layout),
            source_name: family.map_or("Unmapped".to_owned(), |family| format!("{family:?}")),
        }
    }

    fn classify(layout: HKL) -> Option<SystemLayout> {
        match primary_language(layout) {
            ENGLISH_PRIMARY_LANGUAGE => Some(SystemLayout::English),
            UKRAINIAN_PRIMARY_LANGUAGE => Some(SystemLayout::Ukrainian),
            _ => None,
        }
    }

    fn language_id(layout: HKL) -> u16 {
        (layout as usize & 0xffff) as u16
    }

    fn primary_language(layout: HKL) -> u16 {
        language_id(layout) & 0x03ff
    }

    fn source_id(layout: HKL) -> String {
        format!("0x{:X}", layout as usize)
    }
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
mod platform {
    use std::{collections::HashSet, mem, process::Command, ptr, sync::OnceLock};

    use anyhow::{Context, Result, anyhow, bail};
    use x11_dl::xlib::{Display, XkbStateRec, Xlib};
    use xkbcommon::xkb::{Keysym, keysym_to_utf32};

    use super::{
        ActiveInputSource, SwitchOutcome, SystemLayout, classify_xkb_layout, parse_xkb_layouts,
    };

    const XKB_USE_CORE_KEYBOARD: u32 = 0x0100;
    const X11_SUCCESS: i32 = 0;
    const MIN_X11_KEY_CODE: u8 = 8;
    const MAX_X11_KEY_CODE: u8 = 255;
    const MAX_SHIFT_LEVEL: i32 = 3;

    static INSTALLED_MAPPING: OnceLock<std::result::Result<Vec<(char, char)>, String>> =
        OnceLock::new();

    pub fn current() -> Result<Option<ActiveInputSource>> {
        let layouts = configured_layouts()?;
        let xlib = Xlib::open().map_err(|error| anyhow!("could not load Xlib: {error}"))?;
        let display = DisplayConnection::open(&xlib)?;
        let group = display.current_group()?;
        let Some(layout_name) = layouts.get(group as usize) else {
            bail!(
                "XKB active group {group} is outside the configured layout list ({})",
                layouts.len()
            );
        };
        Ok(Some(input_source(group, layout_name)))
    }

    pub fn switch_to(target: SystemLayout) -> Result<SwitchOutcome> {
        let layouts = configured_layouts()?;
        let Some((group, layout_name)) = layouts
            .iter()
            .enumerate()
            .find(|(_, layout)| classify_xkb_layout(layout) == Some(target))
        else {
            return Ok(SwitchOutcome::TargetUnavailable);
        };

        let xlib = Xlib::open().map_err(|error| anyhow!("could not load Xlib: {error}"))?;
        let display = DisplayConnection::open(&xlib)?;
        if usize::from(display.current_group()?) == group {
            return Ok(SwitchOutcome::AlreadyActive {
                source_id: source_id(group as u8, layout_name),
            });
        }
        display.lock_group(group)?;
        Ok(SwitchOutcome::Switched {
            source_id: source_id(group as u8, layout_name),
        })
    }

    pub fn installed_mapping() -> Result<Option<Vec<(char, char)>>> {
        let mapping = INSTALLED_MAPPING
            .get_or_init(|| generate_installed_mapping().map_err(|error| format!("{error:#}")));
        match mapping {
            Ok(mapping) if mapping.is_empty() => Ok(None),
            Ok(mapping) => Ok(Some(mapping.clone())),
            Err(error) => bail!(error.clone()),
        }
    }

    fn generate_installed_mapping() -> Result<Vec<(char, char)>> {
        let layouts = configured_layouts()?;
        let Some(english_group) = layouts
            .iter()
            .position(|layout| classify_xkb_layout(layout) == Some(SystemLayout::English))
        else {
            return Ok(Vec::new());
        };
        let Some(ukrainian_group) = layouts
            .iter()
            .position(|layout| classify_xkb_layout(layout) == Some(SystemLayout::Ukrainian))
        else {
            return Ok(Vec::new());
        };
        let xlib = Xlib::open().map_err(|error| anyhow!("could not load Xlib: {error}"))?;
        let display = DisplayConnection::open(&xlib)?;
        let mut mapping = Vec::new();
        let mut english_seen = HashSet::new();
        let mut ukrainian_seen = HashSet::new();

        for level in 0..=MAX_SHIFT_LEVEL {
            for key_code in MIN_X11_KEY_CODE..=MAX_X11_KEY_CODE {
                let Some(english_character) = display.translate_key(key_code, english_group, level)
                else {
                    continue;
                };
                let Some(ukrainian_character) =
                    display.translate_key(key_code, ukrainian_group, level)
                else {
                    continue;
                };
                if english_character == ukrainian_character
                    || english_seen.contains(&english_character)
                    || ukrainian_seen.contains(&ukrainian_character)
                {
                    continue;
                }
                english_seen.insert(english_character);
                ukrainian_seen.insert(ukrainian_character);
                mapping.push((english_character, ukrainian_character));
            }
        }

        if mapping.len() < 26 {
            bail!(
                "installed layouts yielded only {} distinct physical-key pairs",
                mapping.len()
            );
        }
        Ok(mapping)
    }

    fn configured_layouts() -> Result<Vec<String>> {
        let output = Command::new("setxkbmap")
            .arg("-query")
            .output()
            .context("failed to run `setxkbmap -query`; install x11-xkb-utils")?;
        if !output.status.success() {
            bail!("`setxkbmap -query` failed with {}", output.status);
        }
        let query = String::from_utf8(output.stdout)
            .context("`setxkbmap -query` returned non-UTF-8 output")?;
        let layouts = parse_xkb_layouts(&query);
        if layouts.is_empty() {
            bail!("`setxkbmap -query` did not report any layouts");
        }
        Ok(layouts)
    }

    struct DisplayConnection<'a> {
        xlib: &'a Xlib,
        display: *mut Display,
    }

    impl<'a> DisplayConnection<'a> {
        fn open(xlib: &'a Xlib) -> Result<Self> {
            let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
            if display.is_null() {
                bail!("could not open the X11 display; check DISPLAY or use an X11 session");
            }
            Ok(Self { xlib, display })
        }

        fn current_group(&self) -> Result<u8> {
            let mut state: XkbStateRec = unsafe { mem::zeroed() };
            let status =
                unsafe { (self.xlib.XkbGetState)(self.display, XKB_USE_CORE_KEYBOARD, &mut state) };
            if status != X11_SUCCESS {
                bail!("XkbGetState failed with X11 status {status}");
            }
            Ok(state.group)
        }

        fn lock_group(&self, group: usize) -> Result<()> {
            let group = u32::try_from(group).context("XKB group index does not fit in u32")?;
            let accepted =
                unsafe { (self.xlib.XkbLockGroup)(self.display, XKB_USE_CORE_KEYBOARD, group) };
            if accepted == 0 {
                bail!("XkbLockGroup rejected group {group}");
            }
            unsafe {
                (self.xlib.XFlush)(self.display);
            }
            Ok(())
        }

        fn translate_key(&self, key_code: u8, group: usize, level: i32) -> Option<char> {
            let group = i32::try_from(group).ok()?;
            let keysym =
                unsafe { (self.xlib.XkbKeycodeToKeysym)(self.display, key_code, group, level) };
            let codepoint = keysym_to_utf32(Keysym::new(keysym as u32));
            char::from_u32(codepoint).filter(|character| !character.is_control())
        }
    }

    impl Drop for DisplayConnection<'_> {
        fn drop(&mut self) {
            unsafe {
                (self.xlib.XCloseDisplay)(self.display);
            }
        }
    }

    fn input_source(group: u8, layout_name: &str) -> ActiveInputSource {
        ActiveInputSource {
            layout: classify_xkb_layout(layout_name),
            source_id: source_id(group, layout_name),
            source_name: layout_name.to_owned(),
        }
    }

    fn source_id(group: u8, layout_name: &str) -> String {
        format!("xkb-group-{group}:{layout_name}")
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use anyhow::Result;

    use super::{ActiveInputSource, SwitchOutcome, SystemLayout};

    pub fn current() -> Result<Option<ActiveInputSource>> {
        Ok(None)
    }

    pub fn switch_to(_layout: SystemLayout) -> Result<SwitchOutcome> {
        Ok(SwitchOutcome::Unsupported)
    }

    pub fn installed_mapping() -> Result<Option<Vec<(char, char)>>> {
        Ok(None)
    }
}

#[cfg(test)]
mod cross_platform_tests {
    use super::*;

    #[test]
    fn parses_xkb_layout_groups() {
        let query = "rules: evdev\nmodel: pc105\nlayout: us,ua\nvariant: ,winkeys\n";

        assert_eq!(parse_xkb_layouts(query), ["us", "ua"]);
        assert_eq!(classify_xkb_layout("us"), Some(SystemLayout::English));
        assert_eq!(classify_xkb_layout("ua"), Some(SystemLayout::Ukrainian));
        assert_eq!(classify_xkb_layout("de"), None);
    }
}
