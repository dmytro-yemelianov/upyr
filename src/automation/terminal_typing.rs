//! Opt-in terminal/TUI edits of freshly tracked words. No clipboard,
//! word-kill shortcuts, shell commands, or Enter events are used.

pub(super) struct Plan<'a> {
    pub backspaces: usize,
    pub replacement: &'a str,
}

pub(super) fn plan<'a>(expected: &str, replacement: &'a str) -> Option<Plan<'a>> {
    plan_with_mapping(expected, replacement, None)
}

pub(super) fn plan_with_mapping<'a>(
    expected: &str,
    replacement: &'a str,
    mapping: Option<&[(char, char)]>,
) -> Option<Plan<'a>> {
    // Contextual repair is limited to two short pending words and one anchor.
    // Every changed word must be an exact keyboard-position conversion; no
    // punctuation, new separators, or arbitrary phrase replacement is allowed.
    if expected.ends_with(' ')
        && upyr_core::contextual_edit_is_bounded(expected, replacement, mapping)
    {
        return Some(Plan {
            backspaces: expected.chars().count(),
            replacement,
        });
    }
    // A mid-word edit is an exact positional layout conversion of a bounded
    // alphabetic prefix. Never admit arbitrary unfinished replacements here.
    if !expected.ends_with(' ') && !replacement.ends_with(' ') {
        let length = expected.chars().count();
        if (4..=64).contains(&length)
            && replacement.chars().count() == length
            && expected != replacement
            && expected.chars().all(char::is_alphabetic)
            && replacement.chars().all(char::is_alphabetic)
            && [
                upyr_core::Direction::EnglishToUkrainian,
                upyr_core::Direction::UkrainianToEnglish,
            ]
            .into_iter()
            .any(|direction| match mapping {
                Some(mapping) => upyr_core::convert_with_mapping(expected, direction, mapping).text,
                None => upyr_core::convert(expected, direction).text,
            } == replacement)
        {
            return Some(Plan { backspaces: length, replacement });
        }
    }
    // Command prefixes are corrected before Space so command menus can open.
    // This path accepts only the bounded, tracked single-token namespace.
    #[cfg(any(target_os = "macos", test))]
    if crate::slash_commands::is_command(replacement)
        && expected.starts_with('.')
        && expected.chars().count() == replacement.len()
        && expected
            .chars()
            .all(|c| c.is_alphabetic() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
    {
        return Some(Plan {
            backspaces: expected.chars().count(),
            replacement,
        });
    }
    let source_word = expected.strip_suffix(' ')?;
    let target_word = replacement.strip_suffix(' ')?;
    if source_word.chars().count() > 64 || target_word.chars().count() > 64 {
        return None;
    }
    // A trailing separator must already be present, either literally or at the
    // same physical key in the other layout. Never introduce a new separator.
    let mut source_body = source_word;
    let mut target_body = target_word;
    let fallback;
    let mapping = match mapping {
        Some(mapping) => mapping,
        None => {
            fallback = upyr_core::default_physical_mapping();
            &fallback
        }
    };
    while let Some(delimiter) = target_body
        .chars()
        .last()
        .filter(|c| matches!(c, ',' | '.' | '!' | '?' | ':' | ';' | '…'))
    {
        let source = source_body.chars().last()?;
        if source != delimiter
            && !mapping.iter().any(|(a, b)| {
                (*a == source && *b == delimiter) || (*b == source && *a == delimiter)
            })
        {
            return None;
        }
        source_body = source_body.strip_suffix(source)?;
        target_body = target_body.strip_suffix(delimiter)?;
    }
    let word = |text: &str, physical_punctuation: bool| {
        !text.is_empty()
            && text.chars().count() <= 64
            && text.chars().all(|c| {
                c.is_alphabetic()
                    || matches!(c, '\'' | '’')
                    // These English punctuation keys produce Ukrainian
                    // letters (including their shifted uppercase forms).
                    || (physical_punctuation
                        && matches!(
                            c,
                            '[' | ']' | ';' | '\\' | '`' | ',' | '.' | '/' | '{' | '}' | ':'
                                | '"' | '|' | '<' | '>' | '?'
                        ))
            })
    };
    if source_word == target_word || !word(source_body, true) || !word(target_body, false) {
        return None;
    }
    Some(Plan {
        backspaces: expected.chars().count(),
        replacement,
    })
}

/// Undo only an edit which the forward terminal planner could have produced,
/// or that edit after Backspace deleted the space which ended the word.
pub(super) fn plan_undo<'a>(expected: &str, original: &'a str) -> Option<Plan<'a>> {
    plan_undo_with_mapping(expected, original, None)
}

pub(super) fn plan_undo_with_mapping<'a>(
    expected: &str,
    original: &'a str,
    mapping: Option<&[(char, char)]>,
) -> Option<Plan<'a>> {
    let forward =
        |original: &str, expected: &str| plan_with_mapping(original, expected, mapping).is_some();
    if !forward(original, expected)
        && (expected.ends_with(' ') || !forward(&format!("{original} "), &format!("{expected} ")))
    {
        return None;
    }
    Some(Plan {
        backspaces: expected.chars().count(),
        replacement: original,
    })
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod native {
    mod batch;
    use super::Plan;
    use anyhow::{Context, Result};
    use std::ffi::c_void;

    type Ref = *const c_void;
    type Event = *mut c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceCreate(state: i32) -> Ref;
        fn CGEventSourceSetLocalEventsSuppressionInterval(source: Ref, seconds: f64);
        fn CGEventCreateKeyboardEvent(source: Ref, key: u16, down: bool) -> Event;
        fn CGEventSetType(event: Event, kind: u32);
        fn CGEventSetFlags(event: Event, flags: u64);
        fn CGEventKeyboardSetUnicodeString(event: Event, length: usize, text: *const u16);
        fn CGEventSetIntegerValueField(event: Event, field: u32, value: i64);
        fn CGEventPostToPid(pid: i32, event: Event);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: Ref);
    }

    struct Owned(Ref);
    impl Owned {
        fn new(value: Ref) -> Result<Self> {
            (!value.is_null())
                .then(|| Self(value))
                .context("could not prepare terminal input")
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) };
        }
    }

    /// Allocate the entire edit before sending any deletion. The source PID
    /// identifies our events so the input monitor does not learn its own edit.
    pub(in crate::automation) struct Prepared {
        events: Vec<Owned>,
        /// Inserted instead of `events` when the target refuses the edit while
        /// its caret is still where Upyr left it (an active selection).
        fallback: Vec<Owned>,
        _source: Owned,
        trigger: Owned,
        marker: i64,
    }

    // Quartz event/source objects are owned by the batch and are accessed only
    // by the receiving worker after this transfer. No AppKit objects move.
    unsafe impl Send for Prepared {}

    impl Prepared {
        pub fn new(plan: &Plan<'_>) -> Result<Self> {
            Self::unicode(plan, false)
        }

        /// Replace an already verified, held selection without a pasteboard.
        pub fn selection(text: &str) -> Result<Self> {
            Self::unicode(
                &Plan {
                    backspaces: 0,
                    replacement: text,
                },
                true,
            )
        }

        fn unicode(plan: &Plan<'_>, selection: bool) -> Result<Self> {
            let source = Owned::new(unsafe { CGEventSourceCreate(-1) })?;
            unsafe { CGEventSourceSetLocalEventsSuppressionInterval(source.0, 0.0) };
            let mut events = Vec::new();
            for _ in 0..plan.backspaces {
                for down in [true, false] {
                    let event =
                        Owned::new(unsafe { CGEventCreateKeyboardEvent(source.0, 51, down) })?;
                    unsafe { CGEventSetFlags(event.0.cast_mut(), 0) };
                    events.push(event);
                }
            }
            // Send one scalar per key, without splitting UTF-16 pairs.
            // Chromium treats multi-character insertText as an IME commit;
            // terminal input can then overtake it despite Quartz event order.
            // Single-character keypresses keep the edit in the ordinary path.
            let characters: Vec<char> = plan.replacement.chars().collect();
            for chunk in characters.chunks(1) {
                let text: Vec<u16> = chunk.iter().collect::<String>().encode_utf16().collect();
                for down in [true, false] {
                    let event =
                        Owned::new(unsafe { CGEventCreateKeyboardEvent(source.0, 0, down) })?;
                    unsafe {
                        CGEventSetFlags(event.0.cast_mut(), 0);
                        CGEventKeyboardSetUnicodeString(
                            event.0.cast_mut(),
                            text.len(),
                            text.as_ptr(),
                        );
                    }
                    events.push(event);
                }
            }
            let mut fallback = Vec::new();
            if selection {
                for down in [true, false] {
                    let event =
                        Owned::new(unsafe { CGEventCreateKeyboardEvent(source.0, 124, down) })?;
                    unsafe { CGEventSetFlags(event.0.cast_mut(), 0) };
                    fallback.push(event);
                }
            }
            Self::with_trigger(source, events, fallback)
        }

        /// Cmd+V as one uninterrupted batch, shaped like hardware input:
        /// Command down, V down and up, Command up.
        pub fn paste() -> Result<Self> {
            let base = ENIGO_BASE_FLAGS;
            let command = base | COMMAND_FLAGS;
            // A refused paste must not leave the word selected for the held
            // keys to overwrite: collapse it to the original caret instead.
            Self::keystrokes(
                &[
                    Keystroke::modifier(COMMAND_KEY, command),
                    Keystroke::key(PASTE_KEY, true, command),
                    Keystroke::key(PASTE_KEY, false, command),
                    Keystroke::modifier(COMMAND_KEY, base),
                ],
                &right_arrow_strokes(1),
            )
        }

        /// Right arrows that collapse an abandoned selection to its end.
        /// Only the trigger: releases held input without inserting anything.
        pub fn empty() -> Result<Self> {
            Self::keystrokes(&[], &[])
        }

        /// Recovery is wanted even when late, so it is its own fallback.
        pub fn right_arrows(count: usize) -> Result<Self> {
            let strokes = right_arrow_strokes(count);
            Self::keystrokes(&strokes, &strokes)
        }

        /// Select only inside the held target process. Posting Shift through
        /// HID changes the global modifier state and can taint characters in
        /// physical keys created concurrently, even after Shift is released.
        pub fn select_characters(count: usize, target_pid: i32) -> Result<()> {
            let shift = 1 << 17;
            let mut strokes = Vec::with_capacity(count * 2 + 2);
            strokes.push(Keystroke::modifier(56, shift));
            for _ in 0..count {
                strokes.push(Keystroke::key(123, true, shift));
                strokes.push(Keystroke::key(123, false, shift));
            }
            strokes.push(Keystroke::modifier(56, 0));
            let prepared = Self::keystrokes(&strokes, &[])?;
            // The active process hold queues every foreign key. Our own
            // selection events pass, in order, before the release trigger.
            for event in &prepared.events {
                unsafe { CGEventPostToPid(target_pid, event.0.cast_mut()) };
            }
            Ok(())
        }

        fn keystrokes(strokes: &[Keystroke], fallback: &[Keystroke]) -> Result<Self> {
            let source = Owned::new(unsafe { CGEventSourceCreate(-1) })?;
            unsafe { CGEventSourceSetLocalEventsSuppressionInterval(source.0, 0.0) };
            let create = |strokes: &[Keystroke]| -> Result<Vec<Owned>> {
                let mut events = Vec::with_capacity(strokes.len());
                for stroke in strokes {
                    let event = Owned::new(unsafe {
                        CGEventCreateKeyboardEvent(source.0, stroke.code, stroke.down)
                    })?;
                    unsafe {
                        if stroke.modifier {
                            // Inserted after the system's own translation, so
                            // the modifier must already be a flags-changed event.
                            CGEventSetType(event.0.cast_mut(), FLAGS_CHANGED);
                        }
                        CGEventSetFlags(event.0.cast_mut(), stroke.flags);
                    }
                    events.push(event);
                }
                Ok(events)
            };
            let events = create(strokes)?;
            let fallback = create(fallback)?;
            Self::with_trigger(source, events, fallback)
        }

        fn with_trigger(source: Owned, events: Vec<Owned>, fallback: Vec<Owned>) -> Result<Self> {
            static NEXT_BATCH: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
            let marker = NEXT_BATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let trigger = Owned::new(unsafe { CGEventCreateKeyboardEvent(source.0, 0, false) })?;
            unsafe {
                CGEventSetFlags(trigger.0.cast_mut(), 0);
                CGEventSetIntegerValueField(trigger.0.cast_mut(), 42, marker);
            }
            Ok(Self {
                events,
                fallback,
                _source: source,
                trigger,
                marker,
            })
        }

        /// Ends a GUI hold with this batch directly ahead of the held keys.
        pub fn release(
            self,
            target_pid: i32,
            target_window: i64,
            on_ready: impl FnOnce() -> bool,
        ) -> Result<bool> {
            batch::release(self, target_pid, target_window, on_ready)
        }

        pub fn post(
            self,
            target_pid: i32,
            target_window: i64,
            stamp: [u32; 6],
            after_key: Option<u64>,
            on_ready: impl FnOnce() -> bool,
        ) -> Result<bool> {
            batch::post(self, target_pid, target_window, stamp, after_key, on_ready)
        }
    }

    pub(in crate::automation) fn warm_target(pid: i32) -> Result<()> {
        batch::warm(pid)
    }

    pub(in crate::automation) fn input_source_changed(source: Option<String>) {
        batch::input_source_changed(source);
    }

    /// Holds the target's keyboard input behind a GUI selection edit until
    /// [`Prepared::release`]. `false` means the hold is not in place.
    pub(in crate::automation) fn hold_target(
        pid: i32,
        signal: std::sync::Arc<HoldSignal>,
        duration: std::time::Duration,
    ) -> Result<bool> {
        batch::hold(pid, signal, duration)
    }

    pub(crate) use batch::{HeldLineBreak, HoldSignal};

    /// Holds the next Enter in `pid` behind the correction `id` (see batch).
    pub(crate) fn arm_line_break(
        pid: i32,
        id: u64,
        duration: std::time::Duration,
        notify: impl Fn(HeldLineBreak) + Send + 'static,
    ) -> Result<()> {
        batch::arm_line_break(pid, id, duration, notify)
    }

    pub(crate) fn disarm_line_break() {
        batch::disarm_line_break();
    }

    pub(crate) fn release_held(pid: i32) -> Result<bool> {
        batch::release_held(pid)
    }

    // Apple virtual key codes and the flags enigo's proven shortcuts carry.
    const COMMAND_KEY: u16 = 55;
    const PASTE_KEY: u16 = 9;
    const RIGHT_ARROW_KEY: u16 = 124;
    const FLAGS_CHANGED: u32 = 12;
    const ENIGO_BASE_FLAGS: u64 = 0x2000_0100;
    /// kCGEventFlagMaskCommand plus the left-Command device bit.
    const COMMAND_FLAGS: u64 = (1 << 20) | 0x08;

    fn right_arrow_strokes(count: usize) -> Vec<Keystroke> {
        (0..count)
            .flat_map(|_| {
                [true, false].map(|down| Keystroke::key(RIGHT_ARROW_KEY, down, ENIGO_BASE_FLAGS))
            })
            .collect()
    }

    #[derive(Clone, Copy)]
    struct Keystroke {
        code: u16,
        down: bool,
        flags: u64,
        modifier: bool,
    }

    impl Keystroke {
        fn key(code: u16, down: bool, flags: u64) -> Self {
            Self {
                code,
                down,
                flags,
                modifier: false,
            }
        }

        fn modifier(code: u16, flags: u64) -> Self {
            Self {
                code,
                down: flags & (1 << 20) != 0,
                flags,
                modifier: true,
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) use native::{
    HeldLineBreak, HoldSignal, arm_line_break, disarm_line_break, release_held,
};
#[cfg(target_os = "macos")]
pub(super) use native::{Prepared, hold_target, input_source_changed, warm_target};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contextual_edits_and_undo_are_exact_and_bounded() {
        for (source, target) in [
            ("nj vb ", "то ми "),
            ("Ш рфму ", "I have "),
            ("Ш have ", "I have "),
            ("nj vb тепер ", "то ми тепер "),
        ] {
            let forward = plan(source, target).unwrap();
            assert_eq!(forward.backspaces, source.chars().count());
            assert_eq!(plan_undo(target, source).unwrap().replacement, source);
            assert!(plan_undo(target.trim_end(), source.trim_end()).is_some());
        }
        for (source, target) in [
            ("Ш have ", "I delete "),
            ("Ш have ", "I have\n"),
            ("Ш  have ", "I  have "),
            ("Ш have a question ", "I have a question "),
            ("hello world ", "руддщ цщкдв "),
            ("Ш have", "I have"),
            ("Ш have ", "I have"),
            ("Ш have ", "I hаve "),
        ] {
            assert!(plan(source, target).is_none(), "{source:?} -> {target:?}");
        }
    }

    #[test]
    fn partial_word_edits_and_undo_are_exact_and_bounded() {
        for (source, target) in [("сщте", "cont"), ("ghbd", "прив"), ("Сщте", "Cont")] {
            let edit = plan(source, target).unwrap();
            assert_eq!(edit.backspaces, 4);
            assert_eq!(edit.replacement, target);
            assert_eq!(plan_undo(target, source).unwrap().replacement, source);
            let shorter_source: String = source.chars().take(3).collect();
            let shorter_target: String = target.chars().take(3).collect();
            assert!(plan_undo(&shorter_target, &shorter_source).is_some());
        }
        for (source, target) in [
            ("сщте", "test"),
            ("сщте ", "cont"),
            ("сщте", "cont "),
            ("сщте\n", "cont\n"),
            ("сщ", "co"),
        ] {
            assert!(plan(source, target).is_none(), "{source:?} -> {target:?}");
        }
    }

    #[test]
    fn control_and_format_characters_cannot_enter_either_side_of_an_edit() {
        let forbidden = [
            '\n', '\r', '\t', '\x1b', '\0', '\x08', '\x7f', '\u{200b}', '\u{200d}', '\u{202e}',
            '\u{2066}', '\u{0301}',
        ];
        let mut checked = 0;
        for control in forbidden {
            for position in 0..=4 {
                let mut bad = String::from("word");
                bad.insert(position, control);
                bad.push(' ');
                assert!(plan(&bad, "слово ").is_none(), "{bad:?}");
                assert!(plan("слово ", &bad).is_none(), "{bad:?}");
                assert!(plan_undo("слово ", &bad).is_none(), "{bad:?}");
                checked += 3;
            }
        }
        for delimiter in [',', '.', '!', '?', ':', ';', '…'] {
            let source = format!("ghbdsn{delimiter} ");
            let target = format!("привіт{delimiter} ");
            assert!(plan(&source, &target).is_some());
            assert!(plan_undo(&target, &source).is_some());
            assert!(plan("ghbdsn ", &target).is_none());
        }
        println!("{checked} control/format injection checks and 7 retained delimiters");
    }

    #[test]
    fn undo_restores_physical_punctuation_and_slash_prefixes_without_submission() {
        for (corrected, original) in [("добре ", "lj,ht "), ("розумію ", "hjpevs. "), ("/c", ".с")]
        {
            let plan = plan_undo(corrected, original).unwrap();
            assert_eq!(plan.backspaces, corrected.chars().count());
            assert_eq!(plan.replacement, original);
        }
        for (corrected, original) in [
            ("word ", "bad\n"),
            ("word ", "two words "),
            ("/c", ".с\x1b"),
        ] {
            assert!(plan_undo(corrected, original).is_none());
        }
    }

    #[test]
    fn undo_after_backspace_deleted_the_space_retypes_the_word_without_it() {
        for (corrected, original) in [
            ("hello", "руддщ"),
            ("привіт,", "ghbdsn,"),
            ("добре", "lj,ht"),
        ] {
            let plan = plan_undo(corrected, original).unwrap();
            assert_eq!(plan.backspaces, corrected.chars().count());
            assert_eq!(plan.replacement, original);
        }
        // Still only an edit the forward planner could have made.
        for (corrected, original) in [("word", "bad\n"), ("word", "two words"), ("word", "")] {
            assert!(plan_undo(corrected, original).is_none(), "{original:?}");
        }
    }

    #[test]
    fn counts_characters_and_preserves_the_boundary_without_word_kill() {
        let p = plan("руддщ ", "hello ").unwrap();
        assert_eq!(p.backspaces, 6);
        assert_eq!(p.replacement, "hello ");
        assert_eq!(plan("ghbdsn ", "привіт ").unwrap().backspaces, 7);
    }

    #[test]
    fn slash_commands_are_bounded_and_do_not_inject_submission_or_arguments() {
        let p = plan(".сщььфтв", "/command").unwrap();
        assert_eq!(p.backspaces, 8);
        assert_eq!(plan(".с", "/c").unwrap().replacement, "/c");
        for (source, target) in [
            (".с щ", "/c o"),
            (".с\n", "/c\n"),
            (".с;", "/c;"),
            (".с/", "/c/"),
            (".с", "/command"),
        ] {
            assert!(plan(source, target).is_none());
        }
    }

    #[test]
    fn accepts_physical_punctuation_that_produces_ukrainian_letters() {
        for (source, replacement) in [
            ("lj,ht ", "добре "),
            ("[jnskjcz ", "хотілося "),
            (";bnnz ", "життя "),
            ("{nj ", "Хто "),
        ] {
            let p = plan(source, replacement).expect(source);
            assert_eq!(p.backspaces, source.chars().count());
            assert_eq!(p.replacement, replacement);
        }
    }

    #[test]
    fn accepts_engine_decisions_for_the_reported_punctuation_words() {
        for (physical, expected) in [("lj,ht", "добре "), ("[jnskjcz", "хотілося ")] {
            let sample = upyr_core::WordSample::new(
                physical,
                format!("{physical} "),
                upyr_core::InputLayout::English,
            );
            let upyr_core::AutoDecision::Correct(correction) =
                upyr_core::evaluate(&sample, &upyr_core::AutoCorrectPolicy::default(), None)
            else {
                panic!("expected a correction for {physical}");
            };
            let p = plan(&correction.expected_source, &correction.replacement)
                .expect("recognized words must reach the terminal replacement backend");
            assert_eq!(p.backspaces, physical.chars().count() + 1);
            assert_eq!(p.replacement, expected);
        }
    }

    #[test]
    fn rejects_context_rewrites_control_sequences_and_unbounded_edits() {
        for (source, replacement) in [
            ("one two ", "один два "),
            ("word", "слово "),
            ("word  ", "слово  "),
            ("word ", "слово\n"),
            ("word ", "\x1bслово "),
            ("word ", "word "),
            (" ", "слово "),
            ("word ", " "),
            ("1234 ", "слово "),
            ("word\t ", "слово "),
            ("word\n ", "слово "),
            ("word\x1b ", "слово "),
            ("word ", "слово; "),
            ("rm -rf ", "слово "),
        ] {
            assert!(plan(source, replacement).is_none(), "{source:?}");
        }
        assert!(plan(&format!("{} ", "a".repeat(65)), "слово ").is_none());
    }
}
