/// Returns a native clipboard revision when the platform exposes one.
///
/// A revision lets the selection workflow detect whether Copy changed the
/// clipboard without first writing a sentinel string that clipboard managers
/// could record.
pub fn revision() -> Option<i64> {
    platform::revision()
}

/// Writes text with the platform's clipboard-history exclusion convention.
pub fn set_temporary_text(
    clipboard: &mut arboard::Clipboard,
    text: &str,
) -> Result<(), arboard::Error> {
    platform::set_temporary_text(clipboard, text)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use platform::NativeSnapshot;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn snapshot_native() -> Option<NativeSnapshot> {
    platform::snapshot_native()
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn restore_native(snapshot: NativeSnapshot) -> anyhow::Result<()> {
    platform::restore_native(snapshot)
}

#[cfg(target_os = "macos")]
mod platform {
    use anyhow::{Result, bail};
    use arboard::SetExtApple;
    use objc2::{rc::Retained, runtime::ProtocolObject};
    use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSData, NSString};

    pub struct NativeSnapshot {
        items: Vec<Vec<FormatData>>,
    }

    struct FormatData {
        data_type: String,
        bytes: Vec<u8>,
    }

    pub fn revision() -> Option<i64> {
        i64::try_from(NSPasteboard::generalPasteboard().changeCount()).ok()
    }

    pub fn set_temporary_text(
        clipboard: &mut arboard::Clipboard,
        text: &str,
    ) -> Result<(), arboard::Error> {
        clipboard.set().exclude_from_history().text(text.to_owned())
    }

    pub fn snapshot_native() -> Option<NativeSnapshot> {
        let pasteboard = NSPasteboard::generalPasteboard();
        let Some(items) = pasteboard.pasteboardItems() else {
            return Some(NativeSnapshot { items: Vec::new() });
        };

        let mut snapshot_items = Vec::with_capacity(items.len());
        for item in items.to_vec() {
            let types = item.types();
            let mut formats = Vec::with_capacity(types.len());
            for data_type in types.to_vec() {
                let data = item.dataForType(&data_type)?;
                formats.push(FormatData {
                    data_type: data_type.to_string(),
                    bytes: data.to_vec(),
                });
            }
            snapshot_items.push(formats);
        }
        Some(NativeSnapshot {
            items: snapshot_items,
        })
    }

    pub fn restore_native(snapshot: NativeSnapshot) -> Result<()> {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        if snapshot.items.is_empty() {
            return Ok(());
        }

        let mut restored: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> =
            Vec::with_capacity(snapshot.items.len());
        for formats in snapshot.items {
            let item = NSPasteboardItem::new();
            for format in formats {
                let data_type = NSString::from_str(&format.data_type);
                let data = NSData::with_bytes(&format.bytes);
                if !item.setData_forType(&data, &data_type) {
                    bail!("macOS rejected clipboard format {}", format.data_type);
                }
            }
            restored.push(ProtocolObject::from_retained(item));
        }

        let restored = NSArray::from_retained_slice(&restored);
        if !pasteboard.writeObjects(&restored) {
            bail!("macOS rejected the restored clipboard items");
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod platform {
    use std::{ptr, slice, thread, time::Duration};

    use anyhow::{Context, Result, bail};
    use arboard::SetExtWindows;
    use windows_sys::Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL},
        System::{
            DataExchange::{
                CloseClipboard, CountClipboardFormats, EmptyClipboard, EnumClipboardFormats,
                GetClipboardData, GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        },
    };

    const OPEN_RETRIES: usize = 10;
    const OPEN_RETRY_DELAY: Duration = Duration::from_millis(5);
    const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

    // These clipboard formats contain GDI handles rather than HGLOBAL-backed
    // byte buffers. If any are present, snapshot_native returns None so the
    // caller can use its image/text fallback instead of restoring partial data.
    const CF_BITMAP: u32 = 2;
    const CF_METAFILEPICT: u32 = 3;
    const CF_PALETTE: u32 = 9;
    const CF_ENHMETAFILE: u32 = 14;
    const CF_OWNERDISPLAY: u32 = 128;
    const CF_DSPBITMAP: u32 = 130;
    const CF_DSPMETAFILEPICT: u32 = 131;
    const CF_DSPENHMETAFILE: u32 = 142;
    const CF_GDIOBJFIRST: u32 = 0x0300;
    const CF_GDIOBJLAST: u32 = 0x03ff;

    pub struct NativeSnapshot {
        formats: Vec<FormatData>,
    }

    struct FormatData {
        format: u32,
        bytes: Vec<u8>,
    }

    struct ClipboardLease;

    impl ClipboardLease {
        fn open() -> Option<Self> {
            for attempt in 0..OPEN_RETRIES {
                if unsafe { OpenClipboard(ptr::null_mut()) } != 0 {
                    return Some(Self);
                }
                if attempt + 1 < OPEN_RETRIES {
                    thread::sleep(OPEN_RETRY_DELAY);
                }
            }
            None
        }
    }

    impl Drop for ClipboardLease {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }

    pub fn revision() -> Option<i64> {
        Some(i64::from(unsafe { GetClipboardSequenceNumber() }))
    }

    pub fn set_temporary_text(
        clipboard: &mut arboard::Clipboard,
        text: &str,
    ) -> Result<(), arboard::Error> {
        clipboard
            .set()
            .exclude_from_monitoring()
            .text(text.to_owned())
    }

    pub fn snapshot_native() -> Option<NativeSnapshot> {
        let _clipboard = ClipboardLease::open()?;
        let expected = usize::try_from(unsafe { CountClipboardFormats() }).ok()?;
        if expected == 0 {
            return Some(NativeSnapshot {
                formats: Vec::new(),
            });
        }

        let mut formats = Vec::with_capacity(expected);
        let mut format = 0;
        let mut total_bytes = 0usize;
        loop {
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            if !is_global_memory_format(format) {
                return None;
            }

            let handle = unsafe { GetClipboardData(format) };
            if handle.is_null() {
                // A delayed-rendering format cannot be faithfully snapshotted.
                return None;
            }
            let size = unsafe { GlobalSize(handle as HGLOBAL) };
            if size == 0 || total_bytes.saturating_add(size) > MAX_SNAPSHOT_BYTES {
                return None;
            }
            let data = unsafe { GlobalLock(handle as HGLOBAL) };
            if data.is_null() {
                return None;
            }
            let bytes = unsafe { slice::from_raw_parts(data.cast::<u8>(), size) }.to_vec();
            unsafe {
                GlobalUnlock(handle as HGLOBAL);
            }
            total_bytes += bytes.len();
            formats.push(FormatData { format, bytes });
        }

        (formats.len() == expected).then_some(NativeSnapshot { formats })
    }

    pub fn restore_native(snapshot: NativeSnapshot) -> Result<()> {
        let _clipboard = ClipboardLease::open().context("Windows clipboard is busy")?;
        if unsafe { EmptyClipboard() } == 0 {
            bail!("Windows rejected clearing the clipboard");
        }

        for format in snapshot.formats {
            let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, format.bytes.len()) };
            if memory.is_null() {
                bail!("Windows could not allocate restored clipboard data");
            }
            let destination = unsafe { GlobalLock(memory) };
            if destination.is_null() {
                unsafe {
                    GlobalFree(memory);
                }
                bail!("Windows could not lock restored clipboard data");
            }
            unsafe {
                ptr::copy_nonoverlapping(
                    format.bytes.as_ptr(),
                    destination.cast::<u8>(),
                    format.bytes.len(),
                );
                GlobalUnlock(memory);
            }

            // SetClipboardData assumes ownership after success. On failure the
            // allocation is still ours and must be released.
            if unsafe { SetClipboardData(format.format, memory as HANDLE) }.is_null() {
                unsafe {
                    GlobalFree(memory);
                }
                bail!("Windows rejected clipboard format {}", format.format);
            }
        }
        Ok(())
    }

    fn is_global_memory_format(format: u32) -> bool {
        !matches!(
            format,
            CF_BITMAP
                | CF_METAFILEPICT
                | CF_PALETTE
                | CF_ENHMETAFILE
                | CF_OWNERDISPLAY
                | CF_DSPBITMAP
                | CF_DSPMETAFILEPICT
                | CF_DSPENHMETAFILE
        ) && !(CF_GDIOBJFIRST..=CF_GDIOBJLAST).contains(&format)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn excludes_handle_backed_clipboard_formats() {
            for format in [
                CF_BITMAP,
                CF_METAFILEPICT,
                CF_PALETTE,
                CF_ENHMETAFILE,
                CF_OWNERDISPLAY,
                CF_DSPBITMAP,
                CF_DSPMETAFILEPICT,
                CF_DSPENHMETAFILE,
                CF_GDIOBJFIRST,
                CF_GDIOBJLAST,
            ] {
                assert!(!is_global_memory_format(format));
            }
        }

        #[test]
        fn accepts_text_dib_and_registered_formats() {
            for format in [1, 8, 13, 15, 17, 0xc001] {
                assert!(is_global_memory_format(format));
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use arboard::SetExtLinux;

    pub fn revision() -> Option<i64> {
        None
    }

    pub fn set_temporary_text(
        clipboard: &mut arboard::Clipboard,
        text: &str,
    ) -> Result<(), arboard::Error> {
        clipboard.set().exclude_from_history().text(text.to_owned())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    pub fn revision() -> Option<i64> {
        None
    }

    pub fn set_temporary_text(
        clipboard: &mut arboard::Clipboard,
        text: &str,
    ) -> Result<(), arboard::Error> {
        clipboard.set_text(text.to_owned())
    }
}
