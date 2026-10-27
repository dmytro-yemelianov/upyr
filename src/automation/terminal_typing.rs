//! Opt-in terminal/TUI edits of a single freshly tracked word. No clipboard,
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

/// Undo only an edit which the forward terminal planner could have produced.
pub(super) fn plan_undo<'a>(expected: &str, original: &'a str) -> Option<Plan<'a>> {
    plan_undo_with_mapping(expected, original, None)
}

pub(super) fn plan_undo_with_mapping<'a>(
    expected: &str,
    original: &'a str,
    mapping: Option<&[(char, char)]>,
) -> Option<Plan<'a>> {
    plan_with_mapping(original, expected, mapping)?;
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
        fn CGEventSetFlags(event: Event, flags: u64);
        fn CGEventKeyboardSetUnicodeString(event: Event, length: usize, text: *const u16);
        fn CGEventSetIntegerValueField(event: Event, field: u32, value: i64);
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
        _source: Owned,
        trigger: Owned,
        marker: i64,
    }

    // Quartz event/source objects are owned by the batch and are accessed only
    // by the receiving worker after this transfer. No AppKit objects move.
    unsafe impl Send for Prepared {}

    impl Prepared {
        pub fn new(plan: &Plan<'_>) -> Result<Self> {
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
            static NEXT_BATCH: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
            let marker = NEXT_BATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let trigger = Owned::new(unsafe { CGEventCreateKeyboardEvent(source.0, 0, false) })?;
            unsafe {
                CGEventSetFlags(trigger.0.cast_mut(), 0);
                CGEventSetIntegerValueField(trigger.0.cast_mut(), 42, marker);
            }
            Ok(Self {
                events,
                _source: source,
                trigger,
                marker,
            })
        }

        pub fn post(
            self,
            target_pid: i32,
            target_window: i64,
            stamp: [u32; 6],
            on_ready: impl FnOnce() -> bool,
        ) -> Result<bool> {
            batch::post(self, target_pid, target_window, stamp, on_ready)
        }
    }

    pub(in crate::automation) fn warm_target(pid: i32) -> Result<()> {
        batch::warm(pid)
    }
}

#[cfg(target_os = "macos")]
pub(super) use native::{Prepared, warm_target};

#[cfg(test)]
mod tests {
    use super::*;

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
