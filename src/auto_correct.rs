use device_query::Keycode;

use crate::{
    config::{AutoCorrectSensitivity, Config},
    layout::{Direction, convert, convert_with_mapping},
    system_layout::SystemLayout,
};

const ENGLISH_WORDS: &str = include_str!("dictionaries/english.txt");
const UKRAINIAN_WORDS: &str = include_str!("dictionaries/ukrainian.txt");
const ENGLISH_BIGRAMS: &[&str] = &[
    "th", "he", "in", "er", "an", "re", "on", "at", "en", "nd", "ti", "es", "or", "te", "of", "ed",
    "is", "it", "al", "ar", "st", "to", "nt", "ng", "se", "ha", "as", "ou", "io", "le", "ve", "co",
    "me", "de", "hi", "ri", "ro", "ic", "ne", "ea", "ra", "ce", "li", "ch", "ll", "be", "ma", "si",
    "om", "ur", "lo",
];
const UKRAINIAN_BIGRAMS: &[&str] = &[
    "ст", "но", "на", "ро", "ов", "ен", "то", "ти", "ко", "пр", "ві", "ри", "ка", "ер", "не", "по",
    "ра", "ли", "ва", "ся", "та", "ні", "ал", "го", "ло", "ре", "во", "ий", "ть", "за", "ор", "ан",
    "ів", "ит", "ої", "ня", "ся", "ос", "тр", "де", "ль", "ак",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoKeyEvent {
    pub key: Keycode,
    pub shifted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSample {
    physical_english: String,
    source_layout: SystemLayout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoCorrection {
    pub expected_source: String,
    pub replacement: String,
    pub direction: Direction,
}

#[derive(Default)]
pub struct AutoWordTracker {
    physical_english: String,
    source_layout: Option<SystemLayout>,
}

impl AutoWordTracker {
    pub fn can_begin(key: Keycode) -> bool {
        matches!(
            key,
            Keycode::A
                | Keycode::B
                | Keycode::C
                | Keycode::D
                | Keycode::E
                | Keycode::F
                | Keycode::G
                | Keycode::H
                | Keycode::I
                | Keycode::J
                | Keycode::K
                | Keycode::L
                | Keycode::M
                | Keycode::N
                | Keycode::O
                | Keycode::P
                | Keycode::Q
                | Keycode::R
                | Keycode::S
                | Keycode::T
                | Keycode::U
                | Keycode::V
                | Keycode::W
                | Keycode::X
                | Keycode::Y
                | Keycode::Z
                | Keycode::Apostrophe
                | Keycode::Grave
                | Keycode::LeftBracket
                | Keycode::RightBracket
                | Keycode::BackSlash
                | Keycode::Semicolon
                | Keycode::Comma
                | Keycode::Dot
        )
    }

    pub fn is_empty(&self) -> bool {
        self.physical_english.is_empty()
    }

    pub fn set_source_layout(&mut self, layout: Option<SystemLayout>) {
        self.source_layout = layout;
        if layout.is_none() {
            self.physical_english.clear();
        }
    }

    pub fn clear(&mut self) {
        self.physical_english.clear();
        self.source_layout = None;
    }

    pub fn observe(&mut self, event: AutoKeyEvent) -> Option<WordSample> {
        match event.key {
            Keycode::Space => return self.finish(),
            Keycode::Backspace => {
                self.physical_english.pop();
                if self.physical_english.is_empty() {
                    self.source_layout = None;
                }
                return None;
            }
            Keycode::LShift
            | Keycode::RShift
            | Keycode::CapsLock
            | Keycode::LControl
            | Keycode::RControl
            | Keycode::LAlt
            | Keycode::RAlt
            | Keycode::LOption
            | Keycode::ROption
            | Keycode::Command
            | Keycode::RCommand
            | Keycode::LMeta
            | Keycode::RMeta => return None,
            _ => {}
        }

        let layout = self.source_layout?;
        if let Some(character) = physical_english_character(event.key, event.shifted, layout) {
            self.physical_english.push(character);
        } else {
            self.clear();
        }
        None
    }

    fn finish(&mut self) -> Option<WordSample> {
        let layout = self.source_layout.take()?;
        let physical_english = std::mem::take(&mut self.physical_english);
        (!physical_english.is_empty()).then_some(WordSample {
            physical_english,
            source_layout: layout,
        })
    }
}

pub fn evaluate(sample: &WordSample, config: &Config) -> Option<AutoCorrection> {
    let ukrainian = match crate::system_layout::installed_mapping() {
        Ok(Some(mapping)) => {
            convert_with_mapping(
                &sample.physical_english,
                Direction::EnglishToUkrainian,
                &mapping,
            )
            .text
        }
        _ => convert(&sample.physical_english, Direction::EnglishToUkrainian).text,
    };
    let (source, target, direction, source_language, target_language) = match sample.source_layout {
        SystemLayout::English => (
            sample.physical_english.clone(),
            ukrainian,
            Direction::EnglishToUkrainian,
            Language::English,
            Language::Ukrainian,
        ),
        SystemLayout::Ukrainian => (
            ukrainian,
            sample.physical_english.clone(),
            Direction::UkrainianToEnglish,
            Language::Ukrainian,
            Language::English,
        ),
    };
    let source_normalized = normalize(&source);
    let target_normalized = normalize(&target);
    if source_normalized.chars().count() < config.auto_correct_min_word_length
        || source_normalized == target_normalized
        || config
            .auto_correct_exceptions
            .iter()
            .any(|exception| normalize(exception) == source_normalized)
    {
        return None;
    }

    let source_known = known(source_language, &source_normalized);
    let target_known = known(target_language, &target_normalized);
    let source_score = likelihood(source_language, &source_normalized);
    let target_score = likelihood(target_language, &target_normalized);
    let should_correct = match config.auto_correct_sensitivity {
        AutoCorrectSensitivity::Conservative => target_known && !source_known,
        AutoCorrectSensitivity::Balanced => {
            (target_known && !source_known)
                || (!source_known && target_score >= 2 && target_score >= source_score + 3)
        }
        AutoCorrectSensitivity::Aggressive => {
            (target_known && !source_known)
                || (!source_known && target_score >= 1 && target_score > source_score)
        }
    };

    should_correct.then_some(AutoCorrection {
        expected_source: source,
        replacement: target,
        direction,
    })
}

#[derive(Clone, Copy)]
enum Language {
    English,
    Ukrainian,
}

fn known(language: Language, word: &str) -> bool {
    let dictionary = match language {
        Language::English => ENGLISH_WORDS,
        Language::Ukrainian => UKRAINIAN_WORDS,
    };
    dictionary.lines().any(|candidate| candidate == word)
}

fn likelihood(language: Language, word: &str) -> i32 {
    let bigrams = match language {
        Language::English => ENGLISH_BIGRAMS,
        Language::Ukrainian => UKRAINIAN_BIGRAMS,
    };
    let characters: Vec<char> = word.chars().collect();
    characters
        .windows(2)
        .filter(|pair| {
            let pair: String = pair.iter().collect();
            bigrams.contains(&pair.as_str())
        })
        .count() as i32
}

fn normalize(word: &str) -> String {
    word.trim_matches(|character: char| matches!(character, '\'' | '’' | '-' | '—'))
        .to_lowercase()
}

fn physical_english_character(key: Keycode, shifted: bool, layout: SystemLayout) -> Option<char> {
    let letter = match key {
        Keycode::A => Some('a'),
        Keycode::B => Some('b'),
        Keycode::C => Some('c'),
        Keycode::D => Some('d'),
        Keycode::E => Some('e'),
        Keycode::F => Some('f'),
        Keycode::G => Some('g'),
        Keycode::H => Some('h'),
        Keycode::I => Some('i'),
        Keycode::J => Some('j'),
        Keycode::K => Some('k'),
        Keycode::L => Some('l'),
        Keycode::M => Some('m'),
        Keycode::N => Some('n'),
        Keycode::O => Some('o'),
        Keycode::P => Some('p'),
        Keycode::Q => Some('q'),
        Keycode::R => Some('r'),
        Keycode::S => Some('s'),
        Keycode::T => Some('t'),
        Keycode::U => Some('u'),
        Keycode::V => Some('v'),
        Keycode::W => Some('w'),
        Keycode::X => Some('x'),
        Keycode::Y => Some('y'),
        Keycode::Z => Some('z'),
        _ => None,
    };
    if let Some(letter) = letter {
        return Some(if shifted {
            letter.to_ascii_uppercase()
        } else {
            letter
        });
    }

    let character = match key {
        Keycode::Apostrophe if layout == SystemLayout::English && !shifted => '\'',
        Keycode::Grave if layout == SystemLayout::Ukrainian => '`',
        Keycode::LeftBracket if layout == SystemLayout::Ukrainian => '[',
        Keycode::RightBracket if layout == SystemLayout::Ukrainian => ']',
        Keycode::BackSlash if layout == SystemLayout::Ukrainian => '\\',
        Keycode::Semicolon if layout == SystemLayout::Ukrainian => ';',
        Keycode::Apostrophe if layout == SystemLayout::Ukrainian => '\'',
        Keycode::Comma if layout == SystemLayout::Ukrainian => ',',
        Keycode::Dot if layout == SystemLayout::Ukrainian => '.',
        _ => return None,
    };
    Some(if shifted {
        match character {
            '`' => '~',
            '[' => '{',
            ']' => '}',
            '\\' => '|',
            ';' => ':',
            '\'' => '"',
            ',' => '<',
            '.' => '>',
            _ => character,
        }
    } else {
        character
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(physical_english: &str, source_layout: SystemLayout) -> WordSample {
        WordSample {
            physical_english: physical_english.to_owned(),
            source_layout,
        }
    }

    #[test]
    fn recognizes_mistyped_ukrainian_greeting() {
        let correction = evaluate(
            &sample("ghbdsn", SystemLayout::English),
            &Config {
                auto_correct: true,
                ..Config::default()
            },
        )
        .unwrap();

        assert_eq!(correction.expected_source, "ghbdsn");
        assert_eq!(correction.replacement, "привіт");
        assert_eq!(correction.direction, Direction::EnglishToUkrainian);
    }

    #[test]
    fn recognizes_mistyped_english_greeting() {
        let correction = evaluate(
            &sample("hello", SystemLayout::Ukrainian),
            &Config::default(),
        )
        .unwrap();

        assert_eq!(correction.expected_source, "руддщ");
        assert_eq!(correction.replacement, "hello");
        assert_eq!(correction.direction, Direction::UkrainianToEnglish);
    }

    #[test]
    fn leaves_valid_words_and_exceptions_alone() {
        assert!(evaluate(&sample("hello", SystemLayout::English), &Config::default()).is_none());
        assert!(
            evaluate(
                &sample("ghbdsn", SystemLayout::English),
                &Config {
                    auto_correct_exceptions: vec!["ghbdsn".to_owned()],
                    ..Config::default()
                }
            )
            .is_none()
        );
    }

    #[test]
    fn tracker_finishes_a_word_on_space() {
        let mut tracker = AutoWordTracker::default();
        tracker.set_source_layout(Some(SystemLayout::English));
        for key in [
            Keycode::G,
            Keycode::H,
            Keycode::B,
            Keycode::D,
            Keycode::S,
            Keycode::N,
        ] {
            assert!(
                tracker
                    .observe(AutoKeyEvent {
                        key,
                        shifted: false,
                    })
                    .is_none()
            );
        }

        let word = tracker
            .observe(AutoKeyEvent {
                key: Keycode::Space,
                shifted: false,
            })
            .unwrap();
        assert_eq!(word.physical_english, "ghbdsn");
        assert_eq!(word.source_layout, SystemLayout::English);
        assert!(tracker.is_empty());
    }
}
