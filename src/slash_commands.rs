//! Fresh input-line prefixes in explicitly opted-in command interfaces.
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
    /// A click, navigation, paste shortcut, or other unknown edit loses the
    /// line boundary. Only a subsequently observed Return establishes it again.
    pub fn invalidate(&mut self) {
        self.physical.clear();
        self.visible.clear();
        self.blocked = true;
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
            Keycode::Backspace if !self.blocked => {
                self.physical.pop();
                self.visible.pop();
                return !self.physical.is_empty();
            }
            _ => {}
        }
        if self.blocked {
            return false;
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
        let Some(physical) = physical.filter(|c| {
            if self.physical.is_empty() {
                *c == '/'
            } else {
                c.is_ascii_alphanumeric() || matches!(c, '-' | '_')
            }
        }) else {
            self.invalidate();
            return false;
        };
        if self.physical.len() == 64 || layout.is_none() {
            self.invalidate();
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
    fn requires_line_start_and_does_not_reset_after_words_or_spaces() {
        for initial in [
            Keycode::A,
            Keycode::Space,
            Keycode::Left,
            Keycode::Escape,
            Keycode::LControl,
            Keycode::LMeta,
        ] {
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
}
