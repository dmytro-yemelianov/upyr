//! Freshly typed command prefixes in explicitly opted-in command interfaces.
//! No dictionary, terminal history, clipboard, or submission events are used.

use crate::{
    auto_correct::{AutoCorrection, AutoKeyEvent, physical_key},
    layout::Direction,
    system_layout::SystemLayout,
};
use device_query::Keycode;

#[derive(Default)]
pub(crate) struct SlashTracker {
    physical: String,
    visible: String,
    blocked: bool,
}

impl SlashTracker {
    /// Drop the previous edit span after pointer, navigation or shortcut
    /// activity. A newly typed physical slash can establish a fresh command
    /// token; it never includes text from before the reset.
    pub fn invalidate(&mut self) {
        *self = Self::default();
    }

    /// True while the first token belongs to the slash-command namespace;
    /// ordinary word correction must not reinterpret it as prose.
    pub fn observe(
        &mut self,
        event: AutoKeyEvent,
        layout: Option<SystemLayout>,
        mapping: &[(char, char)],
    ) -> bool {
        match event.key {
            Keycode::Enter | Keycode::NumpadEnter => {
                *self = Self::default();
                return false;
            }
            Keycode::LShift | Keycode::RShift => return !self.blocked && !self.physical.is_empty(),
            Keycode::Space => {
                self.invalidate();
                self.blocked = true;
                return false;
            }
            Keycode::Backspace if self.blocked => {
                self.invalidate();
                return false;
            }
            Keycode::Backspace => {
                self.physical.pop();
                self.visible.pop();
                return !self.physical.is_empty();
            }
            _ => {}
        }
        let physical = physical_key(event.key)
            .and_then(|key| {
                upyr_core::physical_character(upyr_core::PhysicalKeyEvent {
                    key,
                    shifted: event.shifted,
                })
            })
            .or(match (event.key, event.shifted) {
                (Keycode::Minus, false) => Some('-'),
                (Keycode::Minus, true) => Some('_'),
                (Keycode::Key0, false) => Some('0'),
                (Keycode::Key1, false) => Some('1'),
                (Keycode::Key2, false) => Some('2'),
                (Keycode::Key3, false) => Some('3'),
                (Keycode::Key4, false) => Some('4'),
                (Keycode::Key5, false) => Some('5'),
                (Keycode::Key6, false) => Some('6'),
                (Keycode::Key7, false) => Some('7'),
                (Keycode::Key8, false) => Some('8'),
                (Keycode::Key9, false) => Some('9'),
                _ => None,
            });
        if physical.is_none() {
            self.invalidate();
            return false;
        }
        if self.blocked {
            return false;
        }
        let Some(physical) = physical.filter(|c| {
            if self.physical.is_empty() {
                *c == '/'
            } else {
                c.is_ascii_alphanumeric() || matches!(c, '-' | '_')
            }
        }) else {
            self.invalidate();
            self.blocked = true; // A known preceding word/space is still inline prose.
            return false;
        };
        if self.physical.len() == 64 || layout.is_none() {
            self.invalidate();
            self.blocked = true;
            return false;
        }
        let visible = if layout == Some(SystemLayout::Ukrainian) {
            mapping
                .iter()
                .find_map(|(en, uk)| (*en == physical).then_some(*uk))
                .unwrap_or(physical)
        } else {
            physical
        };
        self.physical.push(physical);
        self.visible.push(visible);
        true
    }

    pub fn correction(&self, exceptions: &[String]) -> Option<AutoCorrection> {
        if self.blocked
            || !is_command(&self.physical)
            || !self.visible.starts_with('.')
            || self.visible == self.physical
            || exceptions.iter().any(|word| {
                word.eq_ignore_ascii_case(&self.physical)
                    || word == &self.visible
                    || word.eq_ignore_ascii_case(&self.physical[1..])
                    || word
                        .to_ascii_lowercase()
                        .trim_start_matches('/')
                        .starts_with(&self.physical[1..].to_ascii_lowercase())
            })
        {
            return None;
        }
        Some(AutoCorrection {
            expected_source: self.visible.clone(),
            replacement: self.physical.clone(),
            direction: Direction::UkrainianToEnglish,
        })
    }

    pub fn committed(&mut self) {
        if !self.blocked {
            self.visible.clone_from(&self.physical);
        }
    }
}

pub(crate) fn is_command(token: &str) -> bool {
    let Some(name) = token.strip_prefix('/') else {
        return false;
    };
    !name.is_empty()
        && token.len() <= 64
        && name.as_bytes()[0].is_ascii_alphabetic()
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(tracker: &mut SlashTracker, key: Keycode, layout: SystemLayout) -> bool {
        tracker.observe(
            AutoKeyEvent {
                key,
                shifted: false,
            },
            Some(layout),
            &upyr_core::default_physical_mapping(),
        )
    }
    fn command() -> SlashTracker {
        let mut tracker = SlashTracker::default();
        for k in [
            Keycode::Slash,
            Keycode::C,
            Keycode::O,
            Keycode::M,
            Keycode::M,
            Keycode::A,
            Keycode::N,
            Keycode::D,
        ] {
            assert!(key(&mut tracker, k, SystemLayout::Ukrainian));
        }
        tracker
    }
    #[test]
    fn converts_custom_command_without_space_or_dictionary() {
        let c = command().correction(&[]).unwrap();
        assert_eq!(c.expected_source, ".сщььфтв");
        assert_eq!(c.replacement, "/command");
    }
    #[test]
    fn opens_command_namespace_early_and_continues_in_english() {
        let mut tracker = SlashTracker::default();
        key(&mut tracker, Keycode::Slash, SystemLayout::Ukrainian);
        assert!(tracker.correction(&[]).is_none());
        key(&mut tracker, Keycode::C, SystemLayout::Ukrainian);
        assert_eq!(tracker.correction(&[]).unwrap().replacement, "/c");
        tracker.committed();
        key(&mut tracker, Keycode::O, SystemLayout::English);
        assert!(tracker.correction(&[]).is_none());
    }
    #[test]
    fn fresh_slash_prefix_recovers_after_unknown_editor_activity_without_return() {
        let mut tracker = command();
        tracker.invalidate(); // Pointer activity loses the old input position.
        key(&mut tracker, Keycode::Slash, SystemLayout::Ukrainian);
        key(&mut tracker, Keycode::C, SystemLayout::Ukrainian);
        assert_eq!(tracker.correction(&[]).unwrap().replacement, "/c");
        for edit in [
            Keycode::Escape,
            Keycode::Left,
            Keycode::LControl,
            Keycode::LMeta,
        ] {
            key(&mut tracker, edit, SystemLayout::Ukrainian);
            key(&mut tracker, Keycode::Slash, SystemLayout::Ukrainian);
            key(&mut tracker, Keycode::C, SystemLayout::Ukrainian);
            assert_eq!(
                tracker.correction(&[]).unwrap().replacement,
                "/c",
                "{edit:?}"
            );
        }
        let mut tracker = SlashTracker::default();
        key(&mut tracker, Keycode::A, SystemLayout::Ukrainian);
        key(&mut tracker, Keycode::Backspace, SystemLayout::Ukrainian);
        key(&mut tracker, Keycode::Slash, SystemLayout::Ukrainian);
        key(&mut tracker, Keycode::C, SystemLayout::Ukrainian);
        assert_eq!(tracker.correction(&[]).unwrap().replacement, "/c");
    }

    #[test]
    fn reset_never_reuses_a_previously_typed_command_span() {
        let mut tracker = command();
        tracker.invalidate();
        key(&mut tracker, Keycode::C, SystemLayout::Ukrainian);
        assert!(tracker.correction(&[]).is_none());
    }

    #[test]
    fn requires_line_start_and_does_not_reset_after_words_or_spaces() {
        for initial in [Keycode::A, Keycode::Space] {
            let mut tracker = SlashTracker::default();
            key(&mut tracker, initial, SystemLayout::Ukrainian);
            key(&mut tracker, Keycode::Slash, SystemLayout::Ukrainian);
            key(&mut tracker, Keycode::C, SystemLayout::Ukrainian);
            assert!(tracker.correction(&[]).is_none(), "{initial:?}");
            key(&mut tracker, Keycode::Enter, SystemLayout::Ukrainian);
            key(&mut tracker, Keycode::Slash, SystemLayout::Ukrainian);
            key(&mut tracker, Keycode::C, SystemLayout::Ukrainian);
            assert!(tracker.correction(&[]).is_some());
        }
    }
    #[test]
    fn handles_backspace_exceptions_and_existing_english_commands() {
        let mut tracker = command();
        assert!(tracker.correction(&["/command".into()]).is_none());
        key(&mut tracker, Keycode::Backspace, SystemLayout::Ukrainian);
        assert_eq!(tracker.correction(&[]).unwrap().replacement, "/comman");
        for _ in 0..7 {
            key(&mut tracker, Keycode::Backspace, SystemLayout::Ukrainian);
        }
        key(&mut tracker, Keycode::Slash, SystemLayout::English);
        key(&mut tracker, Keycode::C, SystemLayout::English);
        assert!(tracker.correction(&[]).is_none());
    }
    #[test]
    fn validates_bounded_command_syntax() {
        for token in ["/custom-command_2", "/clear", "/C"] {
            assert!(is_command(token));
        }
        for token in ["/", "/2", "/a/b", "/a b", "/a\n", "/a;rm", ".сщььфтв"] {
            assert!(!is_command(token));
        }
        assert!(!is_command(&format!("/{}", "a".repeat(64))));
    }

    #[test]
    fn generated_commands_preserve_exact_spans_across_commit_reset_and_backspace() {
        fn event(c: char) -> AutoKeyEvent {
            let key = match c.to_ascii_lowercase() {
                'a' => Keycode::A,
                'c' => Keycode::C,
                'm' => Keycode::M,
                'z' => Keycode::Z,
                '0' => Keycode::Key0,
                '1' => Keycode::Key1,
                '9' => Keycode::Key9,
                '-' | '_' => Keycode::Minus,
                '/' => Keycode::Slash,
                _ => panic!("unexpected generator alphabet"),
            };
            AutoKeyEvent {
                key,
                shifted: c.is_ascii_uppercase() || c == '_',
            }
        }
        let mapping = upyr_core::default_physical_mapping();
        let alphabet = b"acmzACMZ019-_";
        let mut checked = 0;
        for length in 1..=63 {
            for seed in 0_u64..32 {
                let mut state = seed + length as u64 * 33;
                let mut name = String::from(b"aCmZ"[seed as usize % 4] as char);
                for _ in 1..length {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    name.push(alphabet[(state >> 32) as usize % alphabet.len()] as char);
                }
                let command = format!("/{name}");
                assert!(is_command(&command));
                for commit in [false, true] {
                    let mut tracker = super::tests::command();
                    tracker.invalidate();
                    let mut layout = SystemLayout::Ukrainian;
                    for (index, c) in command.chars().enumerate() {
                        assert!(
                            tracker.observe(event(c), Some(layout), &mapping),
                            "{command} at {index} ({c})"
                        );
                        if commit && index == 1 {
                            assert_eq!(tracker.correction(&[]).unwrap().replacement, command[..2]);
                            tracker.committed();
                            layout = SystemLayout::English;
                        }
                    }
                    if commit {
                        assert!(tracker.correction(&[]).is_none());
                    } else {
                        assert_eq!(tracker.correction(&[]).unwrap().replacement, command);
                    }
                    for _ in 0..command.len() {
                        key(&mut tracker, Keycode::Backspace, layout);
                    }
                    key(&mut tracker, Keycode::Slash, SystemLayout::Ukrainian);
                    key(&mut tracker, Keycode::C, SystemLayout::Ukrainian);
                    assert_eq!(tracker.correction(&[]).unwrap().expected_source, ".с");
                    checked += 1;
                }
            }
        }
        println!("{checked} generated command sequences with commit/reset/backspace");
    }
}
