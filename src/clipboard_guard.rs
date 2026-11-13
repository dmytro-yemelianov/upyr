/// Returns a native clipboard revision when the platform exposes one.
///
/// A revision lets the selection workflow detect whether Copy changed the
/// clipboard without first writing a sentinel string that clipboard managers
/// could record.
pub fn revision() -> Option<i64> {
    platform::revision()
}

/// Identifies Upyr's temporary clipboard value. Native revision counters also
/// distinguish a later user copy of exactly the same text.
pub(crate) struct ClipboardOwnership {
    revision: Option<i64>,
    text: String,
}

impl ClipboardOwnership {
    pub fn after_write(temporary: &str) -> Self {
        Self {
            revision: revision(),
            text: temporary.to_owned(),
        }
    }

    pub fn is_current(&self, clipboard: &mut arboard::Clipboard) -> bool {
        // Check content as well as the revision: another writer can intervene
        // between our write and the initial revision read.
        let text = clipboard.get_text().ok();
        self.matches(revision(), text.as_deref())
    }

    fn matches(&self, current_revision: Option<i64>, current_text: Option<&str>) -> bool {
        self.revision
            .is_none_or(|expected| current_revision == Some(expected))
            && current_text == Some(self.text.as_str())
    }
}

/// Writes text with the platform's clipboard-history exclusion convention.
pub fn set_temporary_text(
    clipboard: &mut arboard::Clipboard,
    text: &str,
) -> Result<(), arboard::Error> {
    platform::set_temporary_text(clipboard, text)
}

#[cfg(test)]
mod ownership_tests {
    use super::ClipboardOwnership;

    #[test]
    fn later_copies_cannot_be_overwritten_by_a_saved_clipboard() {
        let owned = ClipboardOwnership {
            revision: Some(41),
            text: "temporary text".to_owned(),
        };
        assert!(owned.matches(Some(41), Some("temporary text")));
        assert!(!owned.matches(Some(42), Some("temporary text")));
        assert!(!owned.matches(None, Some("temporary text")));
        assert!(!owned.matches(Some(41), Some("a racing user copy")));
    }

    #[test]
    fn platforms_without_revisions_require_the_temporary_value_to_remain() {
        let owned = ClipboardOwnership {
            revision: None,
            text: "converted text".to_owned(),
        };
        assert!(owned.matches(None, Some("converted text")));
        assert!(!owned.matches(None, Some("a newer user copy")));
        assert!(!owned.matches(None, None));
    }
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
        snapshot_pasteboard(&NSPasteboard::generalPasteboard())
    }

    fn snapshot_pasteboard(pasteboard: &NSPasteboard) -> Option<NativeSnapshot> {
        let Some(items) = pasteboard.pasteboardItems() else {
            return Some(NativeSnapshot { items: Vec::new() });
        };

        let mut snapshot_items = Vec::with_capacity(items.len());
        for item in items.to_vec() {
            let types = item.types();
            let mut formats = Vec::with_capacity(types.len());
            for data_type in types.to_vec() {
                // A pasteboard can advertise unavailable/promised formats
                // (including iOS metadata). One unreadable format must not
                // discard the other items and their available rich data by
                // forcing the lossy text/HTML fallback.
                let Some(data) = item.dataForType(&data_type) else {
                    continue;
                };
                formats.push(FormatData {
                    data_type: data_type.to_string(),
                    bytes: data.to_vec(),
                });
            }
            if !formats.is_empty() {
                snapshot_items.push(formats);
            }
        }
        if !items.is_empty() && snapshot_items.is_empty() {
            return None;
        }
        Some(NativeSnapshot {
            items: snapshot_items,
        })
    }

    pub fn restore_native(snapshot: NativeSnapshot) -> Result<()> {
        restore_pasteboard(&NSPasteboard::generalPasteboard(), snapshot)
    }

    fn restore_pasteboard(pasteboard: &NSPasteboard, snapshot: NativeSnapshot) -> Result<()> {
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

    #[cfg(test)]
    #[allow(unsafe_code)]
    mod tests {
        use super::*;

        #[test]
        fn unreadable_format_does_not_drop_other_clipboard_formats() {
            // A private pasteboard avoids touching the user's clipboard.
            let board = NSPasteboard::pasteboardWithUniqueName();
            let text = NSString::from_str("public.utf8-plain-text");
            let rich = NSString::from_str("dev.upyr.test.rich-data");
            let unavailable = NSString::from_str("dev.upyr.test.unavailable");
            let types =
                NSArray::from_retained_slice(&[text.clone(), unavailable.clone(), rich.clone()]);
            // SAFETY: no owner object is supplied; the private board simply
            // declares a format whose provider never supplies any data.
            unsafe { board.declareTypes_owner(&types, None) };
            assert!(board.setData_forType(Some(&NSData::with_bytes(b"test text")), &text));
            assert!(board.setData_forType(Some(&NSData::with_bytes(&[0, 1, 128, 255])), &rich));
            assert!(board.dataForType(&unavailable).is_none());

            let snapshot = snapshot_pasteboard(&board).expect("preserve readable formats");
            board.clearContents();
            restore_pasteboard(&board, snapshot).unwrap();
            assert_eq!(board.dataForType(&text).unwrap().to_vec(), b"test text");
            assert_eq!(board.dataForType(&rich).unwrap().to_vec(), [0, 1, 128, 255]);
            board.clearContents();
            // SAFETY: no owner object, as above. Keep the fallback available
            // when the board has no readable data at all.
            unsafe {
                board.declareTypes_owner(&NSArray::from_retained_slice(&[unavailable]), None)
            };
            assert!(snapshot_pasteboard(&board).is_none());
            board.clearContents();
        }

        #[test]
        fn native_round_trip_preserves_multiple_items_and_empty_clipboard() {
            let board = NSPasteboard::pasteboardWithUniqueName();
            let kind = NSString::from_str("public.utf8-plain-text");
            let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> =
                [b"first".as_slice(), b"second".as_slice()]
                    .into_iter()
                    .map(|bytes| {
                        let item = NSPasteboardItem::new();
                        assert!(item.setData_forType(&NSData::with_bytes(bytes), &kind));
                        ProtocolObject::from_retained(item)
                    })
                    .collect();
            assert!(board.writeObjects(&NSArray::from_retained_slice(&items)));
            let snapshot = snapshot_pasteboard(&board).unwrap();
            restore_pasteboard(&board, snapshot).unwrap();
            let restored = board.pasteboardItems().unwrap().to_vec();
            assert_eq!(restored.len(), 2);
            assert_eq!(restored[0].dataForType(&kind).unwrap().to_vec(), b"first");
            assert_eq!(restored[1].dataForType(&kind).unwrap().to_vec(), b"second");
            board.clearContents();
            let empty = snapshot_pasteboard(&board).unwrap();
            restore_pasteboard(&board, empty).unwrap();
            assert!(board.pasteboardItems().is_none_or(|items| items.is_empty()));
        }

        #[test]
        fn rich_clipboard_formats_survive_repeated_native_round_trips() {
            let board = NSPasteboard::pasteboardWithUniqueName();
            // Exercise actual native format aliases, binary/empty payloads,
            // multiple items, and an advertised iOS format without a provider.
            let formats: &[(&str, &[u8])] = &[
                ("public.utf8-plain-text", "Виправлення — test".as_bytes()),
                ("public.html", b"<b>test</b>"),
                ("public.rtf", br"{\rtf1\ansi test}"),
                ("dev.upyr.test.binary", &[0, 1, 127, 128, 255]),
                ("dev.upyr.test.empty", &[]),
            ];
            let unavailable = NSString::from_str("iOS rich content paste pasteboard type");
            let mut kinds = formats
                .iter()
                .map(|(name, _)| NSString::from_str(name))
                .collect::<Vec<_>>();
            kinds.push(unavailable.clone());
            // SAFETY: a private pasteboard with no provider/owner.
            unsafe { board.declareTypes_owner(&NSArray::from_retained_slice(&kinds), None) };
            for (name, bytes) in formats {
                assert!(
                    board.setData_forType(
                        Some(&NSData::with_bytes(bytes)),
                        &NSString::from_str(name)
                    )
                );
            }
            assert!(board.dataForType(&unavailable).is_none());
            let all_readable = |snapshot: NativeSnapshot| {
                snapshot
                    .items
                    .into_iter()
                    .map(|formats| {
                        let mut values = formats
                            .into_iter()
                            .map(|format| (format.data_type, format.bytes))
                            .collect::<Vec<_>>();
                        values.sort();
                        values
                    })
                    .collect::<Vec<_>>()
            };
            let before = all_readable(snapshot_pasteboard(&board).unwrap());
            for _ in 0..100 {
                let saved = snapshot_pasteboard(&board).unwrap();
                restore_pasteboard(&board, saved).unwrap();
                assert_eq!(all_readable(snapshot_pasteboard(&board).unwrap()), before);
                for (name, bytes) in formats {
                    assert_eq!(
                        board
                            .dataForType(&NSString::from_str(name))
                            .unwrap()
                            .to_vec(),
                        *bytes,
                        "{name}"
                    );
                }
                assert_eq!(board.pasteboardItems().unwrap().len(), 1);
            }
            board.clearContents();
        }
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
