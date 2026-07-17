use std::{
    borrow::Cow,
    path::PathBuf,
    process, thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use arboard::{Clipboard, ImageData};
use enigo::{Direction as KeyDirection, Enigo, Key, Keyboard, Settings};

use crate::{
    clipboard_guard,
    config::Config,
    layout::{Direction, convert, convert_with_mapping},
    system_layout::{SwitchOutcome, SystemLayout},
};
use tracing::{debug, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionOutcome {
    Converted {
        direction: Direction,
        characters: usize,
        layout_switched: Option<SystemLayout>,
    },
    NoSelection,
    NoConvertibleText,
    /// Automatic correction selected something other than the word observed
    /// by the in-memory tracker, so no text was changed.
    TextMismatch,
}

enum SavedClipboard {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    Native(clipboard_guard::NativeSnapshot),
    Files(Vec<PathBuf>),
    Html {
        html: String,
        alternative: Option<String>,
    },
    Text(String),
    Image(ImageData<'static>),
    Empty,
}

enum CopyProbe {
    Revision(i64),
    Marker(String),
}

/// Copies the current selection, converts it, and pastes it back into the active app.
pub fn convert_selection(config: &Config) -> Result<SelectionOutcome> {
    convert_selection_if_matches(config, None, None)
}

fn convert_selection_if_matches(
    config: &Config,
    expected_source: Option<&str>,
    on_text_committed: Option<&mut dyn FnMut()>,
) -> Result<SelectionOutcome> {
    let mut enigo = Enigo::new(&Settings::default()).context(
        "could not connect to desktop input; check Accessibility permissions or DISPLAY",
    )?;
    let mut clipboard = Clipboard::new().context("could not access the system clipboard")?;
    let saved = snapshot(&mut clipboard);
    let probe = arm_copy_probe(&mut clipboard)?;
    if let Err(error) = send_shortcut(&mut enigo, Shortcut::Copy) {
        let _ = restore(&mut clipboard, saved);
        return Err(error).context("could not send the copy shortcut");
    }
    thread::sleep(Duration::from_millis(config.copy_delay_ms));

    let selected = match read_copied_text(&mut clipboard, &probe) {
        Some(text) => text,
        None => {
            restore(&mut clipboard, saved)?;
            return Ok(SelectionOutcome::NoSelection);
        }
    };

    if expected_source
        .is_some_and(|expected| selected != expected && selected.trim() != expected.trim())
    {
        let _ = enigo.key(Key::RightArrow, KeyDirection::Click);
        restore(&mut clipboard, saved)?;
        return Ok(SelectionOutcome::TextMismatch);
    }

    let conversion = convert_with_installed_mapping(&selected, config.direction);
    if !conversion.changed {
        restore(&mut clipboard, saved)?;
        return Ok(SelectionOutcome::NoConvertibleText);
    }

    if let Err(error) = clipboard_guard::set_temporary_text(&mut clipboard, &conversion.text) {
        let _ = restore(&mut clipboard, saved);
        return Err(error).context("could not place converted text on the clipboard");
    }
    thread::sleep(Duration::from_millis(config.paste_delay_ms));

    if let Err(error) = send_shortcut(&mut enigo, Shortcut::Paste) {
        let _ = restore(&mut clipboard, saved);
        return Err(error).context("could not send the paste shortcut");
    }

    let layout_switched = config
        .switch_layout
        .then(|| follow_converted_layout(conversion.direction))
        .flatten();
    let outcome = SelectionOutcome::Converted {
        direction: conversion.direction,
        characters: selected.chars().count(),
        layout_switched,
    };

    finish_after_text_commit(on_text_committed, || {
        if config.restore_clipboard {
            thread::sleep(Duration::from_millis(config.restore_delay_ms));
            restore(&mut clipboard, saved)?;
        }
        Ok(())
    })?;

    Ok(outcome)
}

fn finish_after_text_commit(
    on_text_committed: Option<&mut dyn FnMut()>,
    post_commit: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if let Some(on_text_committed) = on_text_committed {
        on_text_committed();
    }
    post_commit()
}

fn convert_with_installed_mapping(text: &str, direction: Direction) -> crate::layout::Conversion {
    match crate::system_layout::installed_mapping() {
        Ok(Some(mapping)) => convert_with_mapping(text, direction, &mapping),
        Ok(None) => convert(text, direction),
        Err(error) => {
            warn!(%error, "could not derive the installed layout mapping; using the built-in map");
            convert(text, direction)
        }
    }
}

fn arm_copy_probe(clipboard: &mut Clipboard) -> Result<CopyProbe> {
    if let Some(revision) = clipboard_guard::revision() {
        return Ok(CopyProbe::Revision(revision));
    }

    let marker = clipboard_marker();
    clipboard_guard::set_temporary_text(clipboard, &marker)
        .context("could not prepare the clipboard")?;
    Ok(CopyProbe::Marker(marker))
}

fn read_copied_text(clipboard: &mut Clipboard, probe: &CopyProbe) -> Option<String> {
    match probe {
        CopyProbe::Revision(before) if clipboard_guard::revision() == Some(*before) => None,
        CopyProbe::Revision(_) => clipboard.get_text().ok(),
        CopyProbe::Marker(marker) => clipboard.get_text().ok().filter(|text| text != marker),
    }
}

fn follow_converted_layout(direction: Direction) -> Option<SystemLayout> {
    let target = match direction {
        Direction::EnglishToUkrainian => SystemLayout::Ukrainian,
        Direction::UkrainianToEnglish => SystemLayout::English,
        Direction::Smart => return None,
    };

    match crate::system_layout::switch_to(target) {
        Ok(SwitchOutcome::Switched { source_id }) => {
            debug!(
                ?target,
                source_id, "followed converted text with its OS layout"
            );
            Some(target)
        }
        Ok(SwitchOutcome::AlreadyActive { source_id }) => {
            debug!(?target, source_id, "target OS layout was already active");
            None
        }
        Ok(SwitchOutcome::TargetUnavailable) => {
            warn!(?target, "target OS layout is not installed");
            None
        }
        Ok(SwitchOutcome::Unsupported) => {
            debug!("active OS layout switching is not implemented on this platform yet");
            None
        }
        Err(error) => {
            warn!(%error, ?target, "could not switch the active OS layout");
            None
        }
    }
}

/// Selects and converts the word immediately before the active caret.
pub fn convert_previous_word(config: &Config) -> Result<SelectionOutcome> {
    convert_previous_word_if_matches(config, None)
}

/// Selects the previous word and converts it only if the copied text matches
/// the word observed by automatic correction. This protects against races
/// caused by caret movement or applications with unusual selection behavior.
pub fn convert_previous_word_if_matches(
    config: &Config,
    expected_source: Option<&str>,
) -> Result<SelectionOutcome> {
    let mut enigo = Enigo::new(&Settings::default()).context(
        "could not connect to desktop input; check Accessibility permissions or DISPLAY",
    )?;
    select_previous_word(&mut enigo)?;
    thread::sleep(Duration::from_millis(30));
    convert_selection_if_matches(config, expected_source, None)
}

/// Selects an exact in-memory prefix immediately before the caret and converts
/// it only when the copied text still matches. Used by contextual automatic
/// correction after the user types a word boundary.
pub fn convert_previous_input_if_matches(
    config: &Config,
    expected_source: &str,
    on_text_committed: &mut dyn FnMut(),
) -> Result<SelectionOutcome> {
    if expected_source.is_empty() {
        return Ok(SelectionOutcome::NoSelection);
    }
    let mut enigo = Enigo::new(&Settings::default()).context(
        "could not connect to desktop input; check Accessibility permissions or DISPLAY",
    )?;
    select_previous_characters(&mut enigo, expected_source.chars().count())?;
    thread::sleep(Duration::from_millis(30));
    convert_selection_if_matches(config, Some(expected_source), Some(on_text_committed))
}

fn select_previous_word(enigo: &mut Enigo) -> Result<()> {
    #[cfg(target_os = "macos")]
    let word_modifier = Key::Alt;
    #[cfg(not(target_os = "macos"))]
    let word_modifier = Key::Control;

    enigo
        .key(word_modifier, KeyDirection::Press)
        .context("failed to press word-selection modifier")?;
    if let Err(error) = enigo.key(Key::Shift, KeyDirection::Press) {
        let _ = enigo.key(word_modifier, KeyDirection::Release);
        return Err(error).context("failed to press Shift for word selection");
    }

    let select_result = enigo.key(Key::LeftArrow, KeyDirection::Click);
    let shift_release = enigo.key(Key::Shift, KeyDirection::Release);
    let modifier_release = enigo.key(word_modifier, KeyDirection::Release);

    select_result.context("failed to select the previous word")?;
    shift_release.context("failed to release Shift after word selection")?;
    modifier_release.context("failed to release word-selection modifier")?;
    Ok(())
}

fn select_previous_characters(enigo: &mut Enigo, characters: usize) -> Result<()> {
    enigo
        .key(Key::Shift, KeyDirection::Press)
        .context("failed to press Shift for prefix selection")?;
    for _ in 0..characters {
        if let Err(error) = enigo.key(Key::LeftArrow, KeyDirection::Click) {
            let _ = enigo.key(Key::Shift, KeyDirection::Release);
            return Err(error).context("failed to extend prefix selection");
        }
    }
    enigo
        .key(Key::Shift, KeyDirection::Release)
        .context("failed to release Shift after prefix selection")
}

fn snapshot(clipboard: &mut Clipboard) -> SavedClipboard {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if let Some(snapshot) = clipboard_guard::snapshot_native() {
        return SavedClipboard::Native(snapshot);
    }

    if let Ok(files) = clipboard.get().file_list() {
        if !files.is_empty() {
            return SavedClipboard::Files(files);
        }
    }
    if let Ok(html) = clipboard.get().html() {
        let alternative = clipboard.get_text().ok();
        return SavedClipboard::Html { html, alternative };
    }
    if let Ok(text) = clipboard.get_text() {
        return SavedClipboard::Text(text);
    }
    if let Ok(image) = clipboard.get_image() {
        return SavedClipboard::Image(ImageData {
            width: image.width,
            height: image.height,
            bytes: Cow::Owned(image.bytes.into_owned()),
        });
    }
    SavedClipboard::Empty
}

fn restore(clipboard: &mut Clipboard, saved: SavedClipboard) -> Result<()> {
    match saved {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        SavedClipboard::Native(snapshot) => clipboard_guard::restore_native(snapshot),
        SavedClipboard::Files(files) => clipboard
            .set()
            .file_list(&files)
            .context("could not restore clipboard files"),
        SavedClipboard::Html { html, alternative } => clipboard
            .set()
            .html(html, alternative)
            .context("could not restore clipboard HTML"),
        SavedClipboard::Text(text) => clipboard
            .set_text(text)
            .context("could not restore clipboard text"),
        SavedClipboard::Image(image) => clipboard
            .set_image(image)
            .context("could not restore clipboard image"),
        SavedClipboard::Empty => clipboard.clear().context("could not clear the clipboard"),
    }
}

fn clipboard_marker() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("__upyr_selection_{}_{}__", process::id(), nanos)
}

#[derive(Debug, Clone, Copy)]
enum Shortcut {
    Copy,
    Paste,
}

fn send_shortcut(enigo: &mut Enigo, shortcut: Shortcut) -> Result<()> {
    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    let keycode = match shortcut {
        Shortcut::Copy => physical_keycode::COPY,
        Shortcut::Paste => physical_keycode::PASTE,
    };

    enigo
        .key(modifier, KeyDirection::Press)
        .context("failed to press shortcut modifier")?;
    let key_result = enigo.raw(keycode, KeyDirection::Click);
    let release_result = enigo.key(modifier, KeyDirection::Release);

    key_result.context("failed to click shortcut key")?;
    release_result.context("failed to release shortcut modifier")?;
    Ok(())
}

#[cfg(target_os = "macos")]
mod physical_keycode {
    // Apple virtual key codes for the physical C and V keys.
    pub const COPY: u16 = 8;
    pub const PASTE: u16 = 9;
}

#[cfg(target_os = "windows")]
mod physical_keycode {
    // Set 1 scan codes for the physical C and V keys.
    pub const COPY: u16 = 0x2e;
    pub const PASTE: u16 = 0x2f;
}

#[cfg(all(unix, not(target_os = "macos")))]
mod physical_keycode {
    // X11 keycodes for the physical C and V keys (evdev code + 8).
    pub const COPY: u16 = 54;
    pub const PASTE: u16 = 55;
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn input_monitor_resumes_before_clipboard_post_commit_work() {
        let order = RefCell::new(Vec::new());
        let mut on_text_committed = || order.borrow_mut().push("monitor");

        finish_after_text_commit(Some(&mut on_text_committed), || {
            order.borrow_mut().push("clipboard");
            Ok(())
        })
        .expect("post-commit work");

        assert_eq!(*order.borrow(), ["monitor", "clipboard"]);
    }
}
