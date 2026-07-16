use std::{collections::HashSet, sync::OnceLock};

use device_query::Keycode;

use crate::{
    config::{AutoCorrectSensitivity, Config},
    layout::{Direction, convert, convert_with_mapping},
    system_layout::SystemLayout,
};

const ENGLISH_WORDS: &str = include_str!("dictionaries/english.txt");
const UKRAINIAN_WORDS: &str = include_str!("dictionaries/ukrainian.txt");
const MIN_NGRAM: usize = 2;
const MAX_NGRAM: usize = 4;
const MAX_CONTEXT_CHARACTERS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoKeyEvent {
    pub key: Keycode,
    pub shifted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSample {
    physical_word: String,
    physical_context: String,
    source_layout: SystemLayout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoCorrection {
    pub expected_source: String,
    pub replacement: String,
    pub direction: Direction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoDecision {
    Correct(AutoCorrection),
    Continue,
    Reset,
}

#[derive(Default)]
pub struct AutoWordTracker {
    physical_context: String,
    current_word_start: Option<usize>,
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
                | Keycode::Slash
        )
    }

    pub fn needs_layout_check(&self) -> bool {
        self.current_word_start.is_none()
    }

    pub fn set_source_layout(&mut self, layout: Option<SystemLayout>) {
        if self.source_layout != layout {
            self.clear();
        }
        self.source_layout = layout;
    }

    pub fn clear(&mut self) {
        self.physical_context.clear();
        self.current_word_start = None;
        self.source_layout = None;
    }

    pub fn observe(&mut self, event: AutoKeyEvent) -> Option<WordSample> {
        match event.key {
            Keycode::Space => return self.finish_word(),
            Keycode::Backspace => {
                self.backspace();
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
        let Some(character) = physical_english_character(event.key, event.shifted) else {
            self.clear();
            return None;
        };

        if self.physical_context.chars().count() >= MAX_CONTEXT_CHARACTERS {
            self.clear();
            return None;
        }
        if self.current_word_start.is_none() {
            self.current_word_start = Some(self.physical_context.len());
        }
        self.physical_context.push(character);
        self.source_layout = Some(layout);
        None
    }

    fn finish_word(&mut self) -> Option<WordSample> {
        let layout = self.source_layout?;
        let word_start = self.current_word_start.take()?;
        let physical_word = self.physical_context[word_start..].to_owned();
        if physical_word.is_empty() {
            return None;
        }
        self.physical_context.push(' ');
        Some(WordSample {
            physical_word,
            physical_context: self.physical_context.clone(),
            source_layout: layout,
        })
    }

    fn backspace(&mut self) {
        self.physical_context.pop();
        if self.physical_context.is_empty() {
            self.clear();
            return;
        }
        self.current_word_start = if self.physical_context.ends_with(' ') {
            None
        } else {
            Some(
                self.physical_context
                    .rfind(' ')
                    .map_or(0, |index| index + 1),
            )
        };
    }
}

pub fn evaluate(sample: &WordSample, config: &Config) -> AutoDecision {
    let candidates = Candidates::new(sample);
    let source_word = normalize_word(&candidates.source_word);
    let target_word = normalize_word(&candidates.target_word);
    if source_word.is_empty() || target_word.is_empty() || source_word == target_word {
        return AutoDecision::Reset;
    }

    if config
        .auto_correct_exceptions
        .iter()
        .any(|exception| normalize_word(exception) == source_word)
    {
        return AutoDecision::Reset;
    }

    let source_known = known(candidates.source_language, &source_word);
    let target_known = known(candidates.target_language, &target_word);
    let current_word_length = source_word.chars().count();
    let context_characters = candidates
        .target_context
        .chars()
        .filter(|character| character.is_alphabetic())
        .count();

    let dictionary_match =
        current_word_length >= config.auto_correct_min_word_length && target_known && !source_known;
    let source_model = language_likelihood(candidates.source_language, &candidates.source_context);
    let target_model = language_likelihood(candidates.target_language, &candidates.target_context);
    let advantage = target_model.coverage - source_model.coverage;
    let (minimum_coverage, minimum_advantage, minimum_characters) =
        model_thresholds(config.auto_correct_sensitivity);
    let model_match = !source_known
        && context_characters >= minimum_characters.max(config.auto_correct_min_word_length)
        && target_model.grams >= 3
        && target_model.coverage >= minimum_coverage
        && advantage >= minimum_advantage;

    if dictionary_match || model_match {
        return AutoDecision::Correct(AutoCorrection {
            expected_source: candidates.source_context,
            replacement: candidates.target_context,
            direction: candidates.direction,
        });
    }

    if (source_known && !target_known) || has_unsafe_source_punctuation(&candidates.source_word) {
        AutoDecision::Reset
    } else {
        AutoDecision::Continue
    }
}

struct Candidates {
    source_word: String,
    target_word: String,
    source_context: String,
    target_context: String,
    direction: Direction,
    source_language: Language,
    target_language: Language,
}

impl Candidates {
    fn new(sample: &WordSample) -> Self {
        let ukrainian_word = to_ukrainian(&sample.physical_word);
        let ukrainian_context = to_ukrainian(&sample.physical_context);
        match sample.source_layout {
            SystemLayout::English => Self {
                source_word: sample.physical_word.clone(),
                target_word: ukrainian_word,
                source_context: sample.physical_context.clone(),
                target_context: ukrainian_context,
                direction: Direction::EnglishToUkrainian,
                source_language: Language::English,
                target_language: Language::Ukrainian,
            },
            SystemLayout::Ukrainian => Self {
                source_word: ukrainian_word,
                target_word: sample.physical_word.clone(),
                source_context: ukrainian_context,
                target_context: sample.physical_context.clone(),
                direction: Direction::UkrainianToEnglish,
                source_language: Language::Ukrainian,
                target_language: Language::English,
            },
        }
    }
}

fn to_ukrainian(physical_english: &str) -> String {
    match crate::system_layout::installed_mapping() {
        Ok(Some(mapping)) => {
            convert_with_mapping(physical_english, Direction::EnglishToUkrainian, &mapping).text
        }
        _ => convert(physical_english, Direction::EnglishToUkrainian).text,
    }
}

#[derive(Clone, Copy)]
enum Language {
    English,
    Ukrainian,
}

fn known(language: Language, word: &str) -> bool {
    dictionary(language)
        .lines()
        .any(|candidate| candidate == word)
}

fn dictionary(language: Language) -> &'static str {
    match language {
        Language::English => ENGLISH_WORDS,
        Language::Ukrainian => UKRAINIAN_WORDS,
    }
}

#[derive(Debug, Clone, Copy)]
struct ModelEvidence {
    coverage: f32,
    grams: usize,
}

struct LanguageModels {
    english: NgramProfile,
    ukrainian: NgramProfile,
}

struct NgramProfile {
    grams: HashSet<u128>,
}

impl NgramProfile {
    fn train(corpus: &str) -> Self {
        let mut grams = HashSet::new();
        for word in corpus
            .lines()
            .map(str::trim)
            .filter(|word| !word.is_empty())
        {
            for_each_ngram(word, |gram, _weight| {
                grams.insert(gram);
            });
        }
        Self { grams }
    }

    fn score(&self, text: &str) -> ModelEvidence {
        let mut hits = 0usize;
        let mut total = 0usize;
        for token in language_tokens(text) {
            for_each_ngram(&token, |gram, weight| {
                total += weight;
                if self.grams.contains(&gram) {
                    hits += weight;
                }
            });
        }
        ModelEvidence {
            coverage: if total == 0 {
                0.0
            } else {
                hits as f32 / total as f32
            },
            grams: total,
        }
    }
}

fn language_likelihood(language: Language, text: &str) -> ModelEvidence {
    static MODELS: OnceLock<LanguageModels> = OnceLock::new();
    let models = MODELS.get_or_init(|| LanguageModels {
        english: NgramProfile::train(ENGLISH_WORDS),
        ukrainian: NgramProfile::train(UKRAINIAN_WORDS),
    });
    match language {
        Language::English => models.english.score(text),
        Language::Ukrainian => models.ukrainian.score(text),
    }
}

fn language_tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|token| {
            token
                .chars()
                .filter(|character| character.is_alphabetic() || matches!(character, '\'' | '’'))
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

fn for_each_ngram(word: &str, mut visit: impl FnMut(u128, usize)) {
    let mut characters = Vec::with_capacity(word.chars().count() + 2);
    characters.push('^');
    characters.extend(word.to_lowercase().chars());
    characters.push('$');
    for size in MIN_NGRAM..=MAX_NGRAM {
        for gram in characters.windows(size) {
            visit(ngram_key(gram), size - 1);
        }
    }
}

fn ngram_key(characters: &[char]) -> u128 {
    characters
        .iter()
        .fold(characters.len() as u128, |key, character| {
            (key << 21) | (*character as u32 as u128)
        })
}

fn model_thresholds(sensitivity: AutoCorrectSensitivity) -> (f32, f32, usize) {
    match sensitivity {
        AutoCorrectSensitivity::Conservative => (0.28, 0.20, 4),
        AutoCorrectSensitivity::Balanced => (0.22, 0.13, 4),
        AutoCorrectSensitivity::Aggressive => (0.16, 0.07, 3),
    }
}

fn normalize_word(word: &str) -> String {
    word.trim_matches(|character: char| !character.is_alphabetic())
        .to_lowercase()
}

fn has_unsafe_source_punctuation(word: &str) -> bool {
    word.chars().any(|character| {
        !character.is_alphabetic()
            && !matches!(character, '\'' | '’')
            && !character.is_ascii_alphanumeric()
    })
}

fn physical_english_character(key: Keycode, shifted: bool) -> Option<char> {
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
        Keycode::Grave => '`',
        Keycode::LeftBracket => '[',
        Keycode::RightBracket => ']',
        Keycode::BackSlash => '\\',
        Keycode::Semicolon => ';',
        Keycode::Apostrophe => '\'',
        Keycode::Comma => ',',
        Keycode::Dot => '.',
        Keycode::Slash => '/',
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
            '/' => '?',
            _ => character,
        }
    } else {
        character
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(physical_word: &str, source_layout: SystemLayout) -> WordSample {
        WordSample {
            physical_word: physical_word.to_owned(),
            physical_context: physical_word.to_owned(),
            source_layout,
        }
    }

    fn context_sample(
        physical_word: &str,
        physical_context: &str,
        source_layout: SystemLayout,
    ) -> WordSample {
        WordSample {
            physical_word: physical_word.to_owned(),
            physical_context: physical_context.to_owned(),
            source_layout,
        }
    }

    fn correction(decision: AutoDecision) -> AutoCorrection {
        match decision {
            AutoDecision::Correct(correction) => correction,
            other => panic!("expected correction, got {other:?}"),
        }
    }

    #[test]
    fn recognizes_mistyped_ukrainian_greeting() {
        let correction = correction(evaluate(
            &sample("ghbdsn", SystemLayout::English),
            &Config {
                auto_correct: true,
                ..Config::default()
            },
        ));

        assert_eq!(correction.expected_source, "ghbdsn");
        assert_eq!(correction.replacement, "привіт");
        assert_eq!(correction.direction, Direction::EnglishToUkrainian);
    }

    #[test]
    fn recognizes_mistyped_english_greeting() {
        let correction = correction(evaluate(
            &sample("hello", SystemLayout::Ukrainian),
            &Config::default(),
        ));

        assert_eq!(correction.expected_source, "руддщ");
        assert_eq!(correction.replacement, "hello");
        assert_eq!(correction.direction, Direction::UkrainianToEnglish);
    }

    #[test]
    fn ngram_model_recognizes_words_missing_from_dictionary() {
        for (physical, expected) in [
            ("lfdfq", "давай"),
            ("gthtdshbvj", "перевіримо"),
            ("xjve", "чому"),
        ] {
            let correction = correction(evaluate(
                &sample(physical, SystemLayout::English),
                &Config::default(),
            ));
            assert_eq!(correction.replacement, expected);
        }
    }

    #[test]
    fn corrects_the_accumulated_prefix_when_confidence_becomes_high() {
        let correction = correction(evaluate(
            &context_sample(",elt", "nfr f xb ,elt ", SystemLayout::English),
            &Config::default(),
        ));

        assert_eq!(correction.expected_source, "nfr f xb ,elt ");
        assert_eq!(correction.replacement, "так а чи буде ");
    }

    #[test]
    fn leaves_valid_words_exceptions_and_technical_text_alone() {
        assert_eq!(
            evaluate(&sample("hello", SystemLayout::English), &Config::default()),
            AutoDecision::Reset
        );
        assert_eq!(
            evaluate(
                &sample("ghbdsn", SystemLayout::English),
                &Config {
                    auto_correct_exceptions: vec!["ghbdsn".to_owned()],
                    ..Config::default()
                }
            ),
            AutoDecision::Reset
        );
        for source in ["github", "codex", "dmytro", "println"] {
            assert!(!matches!(
                evaluate(&sample(source, SystemLayout::English), &Config::default()),
                AutoDecision::Correct(_)
            ));
        }
    }

    #[test]
    fn language_model_prefers_natural_target_text() {
        let source = language_likelihood(Language::English, "nfr f xb ,elt");
        let target = language_likelihood(Language::Ukrainian, "так а чи буде");

        assert!(target.coverage >= 0.28);
        assert!(target.coverage - source.coverage >= 0.20);
    }

    #[test]
    fn tracker_accumulates_words_from_the_input_boundary() {
        let mut tracker = AutoWordTracker::default();
        tracker.set_source_layout(Some(SystemLayout::English));
        for key in [Keycode::N, Keycode::F, Keycode::R] {
            assert!(
                tracker
                    .observe(AutoKeyEvent {
                        key,
                        shifted: false,
                    })
                    .is_none()
            );
        }

        let first = tracker
            .observe(AutoKeyEvent {
                key: Keycode::Space,
                shifted: false,
            })
            .unwrap();
        assert_eq!(first.physical_word, "nfr");
        assert_eq!(first.physical_context, "nfr ");

        for key in [Keycode::F, Keycode::X, Keycode::B] {
            tracker.observe(AutoKeyEvent {
                key,
                shifted: false,
            });
        }
        let second = tracker
            .observe(AutoKeyEvent {
                key: Keycode::Space,
                shifted: false,
            })
            .unwrap();
        assert_eq!(second.physical_word, "fxb");
        assert_eq!(second.physical_context, "nfr fxb ");
    }

    #[test]
    fn tracker_clears_on_navigation_and_layout_changes() {
        let mut tracker = AutoWordTracker::default();
        tracker.set_source_layout(Some(SystemLayout::English));
        tracker.observe(AutoKeyEvent {
            key: Keycode::A,
            shifted: false,
        });
        tracker.observe(AutoKeyEvent {
            key: Keycode::Left,
            shifted: false,
        });
        assert!(tracker.needs_layout_check());

        tracker.set_source_layout(Some(SystemLayout::English));
        tracker.observe(AutoKeyEvent {
            key: Keycode::A,
            shifted: false,
        });
        tracker.set_source_layout(Some(SystemLayout::Ukrainian));
        assert!(tracker.needs_layout_check());
    }

    #[test]
    fn physical_punctuation_supports_ukrainian_letters() {
        assert_eq!(physical_english_character(Keycode::Comma, false), Some(','));
        assert_eq!(physical_english_character(Keycode::Dot, false), Some('.'));
        assert_eq!(physical_english_character(Keycode::Slash, true), Some('?'));
    }
}
