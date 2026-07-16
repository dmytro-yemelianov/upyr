use device_query::Keycode;

use crate::{
    config::{AutoCorrectSensitivity, Config},
    layout::{Direction, convert, convert_with_mapping},
    system_layout::SystemLayout,
};

const ENGLISH_WORDS: &str = include_str!("dictionaries/english.txt");
const UKRAINIAN_WORDS: &str = include_str!("dictionaries/ukrainian.txt");
const LANGUAGE_MODEL: &[u8] = include_bytes!("models/language.ngm");
const MIN_NGRAM: usize = 2;
const MAX_NGRAM: usize = 5;
const MAX_CONTEXT_CHARACTERS: usize = 256;
#[cfg(test)]
const MODEL_MAGIC: &[u8; 8] = b"UPYRLM1\0";
const MODEL_HEADER_SIZE: usize = 12;
const MODEL_ENTRY_SIZE: usize = 17;
const MODEL_MAX_STRENGTH: i64 = 127;

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

    // A physical punctuation key can either be a letter in the other layout
    // (`,.` -> `бю`) or punctuation the user wants to keep. Score the
    // punctuation-preserving interpretation first, then fall back to the
    // ordinary whole-token conversion. This keeps `[ks,` -> `хліб`, while
    // allowing `Jkmuf,` -> `Ольга,`.
    if let Some(preserved) = Candidates::preserving_terminal_delimiter(sample, &candidates)
        && terminal_delimiter_is_likely(&preserved, config)
    {
        if let AutoDecision::Correct(correction) = evaluate_candidates(&preserved, config) {
            return AutoDecision::Correct(correction);
        }
    }

    evaluate_candidates(&candidates, config)
}

fn evaluate_candidates(candidates: &Candidates, config: &Config) -> AutoDecision {
    let source_word = normalize_word(&candidates.source_word);
    let target_word = normalize_word(&candidates.target_word);
    if target_word.is_empty() || source_word == target_word {
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
    if matches!(candidates.source_language, Language::English)
        && (looks_like_deliberate_latin_identifier(&candidates.source_word)
            || looks_like_deliberate_latin_technical_token(&candidates.source_word))
        && !target_known
    {
        return AutoDecision::Reset;
    }
    // Measure the intended candidate, not the visible source token. On a
    // Ukrainian layout, physical letter keys such as `[];'\\,./` are ordinary
    // letters even though the same positions look like punctuation in English.
    let current_word_length = target_word.chars().count();
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
    let physical_punctuation_evidence = physical_layout_punctuation_evidence(
        &candidates.source_context,
        &candidates.target_context,
    );
    let required_advantage = if physical_punctuation_evidence > 0 {
        minimum_advantage / 2.0
    } else {
        minimum_advantage
    };
    let model_match = !source_known
        && context_characters >= minimum_characters.max(config.auto_correct_min_word_length)
        && target_model.grams >= 3
        && target_model.coverage >= minimum_coverage
        && advantage >= required_advantage;
    let physical_letter_model_match = !source_known
        && current_word_length >= config.auto_correct_min_word_length
        && physical_punctuation_evidence > 0
        && candidates
            .target_word
            .chars()
            .all(|character| character.is_alphabetic() || matches!(character, '\'' | '’'))
        && target_model.grams >= 3
        && target_model.coverage >= minimum_coverage
        && advantage >= -0.05;
    let source_name_model = language_likelihood(candidates.source_language, &source_word);
    let target_name_model = language_likelihood(candidates.target_language, &target_word);
    let proper_name_model_match = !source_known
        && current_word_length >= config.auto_correct_min_word_length
        && is_title_case_word(&candidates.target_word)
        && target_name_model.grams >= 3
        && target_name_model.coverage >= minimum_coverage.min(0.22)
        && target_name_model.coverage - source_name_model.coverage >= required_advantage.min(0.07);
    let source_model_match = context_characters
        >= minimum_characters.max(config.auto_correct_min_word_length)
        && source_model.grams >= 3
        && source_model.coverage >= minimum_coverage
        && source_model.coverage - target_model.coverage >= minimum_advantage;

    if dictionary_match || model_match || physical_letter_model_match || proper_name_model_match {
        return AutoDecision::Correct(AutoCorrection {
            expected_source: candidates.source_context.clone(),
            replacement: candidates.target_context.clone(),
            direction: candidates.direction,
        });
    }

    if (source_known && !target_known) || source_model_match {
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

    fn preserving_terminal_delimiter(sample: &WordSample, full: &Self) -> Option<Self> {
        // If the same physical key is punctuation in the target layout too,
        // keep the ordinary physical conversion. The alternative exists for
        // punctuation-to-letter ambiguity such as `Jkmuf,` -> `Ольгаб`.
        if full
            .target_word
            .chars()
            .last()
            .is_some_and(is_terminal_delimiter)
        {
            return None;
        }
        let delimiter = full.source_word.chars().last()?;
        if !is_terminal_delimiter(delimiter) {
            return None;
        }

        let mut physical_word = sample.physical_word.clone();
        physical_word.pop()?;
        if physical_word.is_empty() {
            return None;
        }

        let (physical_without_boundary, boundary) = sample
            .physical_context
            .strip_suffix(' ')
            .map_or((sample.physical_context.as_str(), ""), |context| {
                (context, " ")
            });
        let physical_prefix = physical_without_boundary.strip_suffix(&sample.physical_word)?;
        let physical_target_word = match sample.source_layout {
            SystemLayout::English => to_ukrainian(&physical_word),
            SystemLayout::Ukrainian => physical_word.clone(),
        };
        let physical_target_context = match sample.source_layout {
            SystemLayout::English => to_ukrainian(&format!("{physical_prefix}{physical_word}")),
            SystemLayout::Ukrainian => format!("{physical_prefix}{physical_word}"),
        };
        let target_word = format!("{physical_target_word}{delimiter}");
        let target_context = format!("{physical_target_context}{delimiter}{boundary}");

        Some(Self {
            source_word: full.source_word.clone(),
            target_word,
            source_context: full.source_context.clone(),
            target_context,
            direction: full.direction,
            source_language: full.source_language,
            target_language: full.target_language,
        })
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

struct NgramModel {
    bytes: &'static [u8],
}

impl NgramModel {
    fn entry_count(&self) -> usize {
        u32::from_le_bytes(self.bytes[8..12].try_into().expect("model header")) as usize
    }

    fn key_at(&self, index: usize) -> u128 {
        let offset = MODEL_HEADER_SIZE + index * MODEL_ENTRY_SIZE;
        u128::from_le_bytes(
            self.bytes[offset..offset + 16]
                .try_into()
                .expect("model key"),
        )
    }

    fn score_at(&self, index: usize) -> i8 {
        let offset = MODEL_HEADER_SIZE + index * MODEL_ENTRY_SIZE + 16;
        self.bytes[offset] as i8
    }

    fn language_score(&self, key: u128) -> i8 {
        let mut start = 0usize;
        let mut end = self.entry_count();
        while start < end {
            let middle = start + (end - start) / 2;
            match self.key_at(middle).cmp(&key) {
                std::cmp::Ordering::Less => start = middle + 1,
                std::cmp::Ordering::Greater => end = middle,
                std::cmp::Ordering::Equal => return self.score_at(middle),
            }
        }
        0
    }

    fn score(&self, language: Language, text: &str) -> ModelEvidence {
        let mut evidence = 0i64;
        let mut maximum = 0i64;
        let mut grams = 0usize;
        let language_sign = match language {
            Language::English => -1i64,
            Language::Ukrainian => 1i64,
        };
        for token in language_tokens(text) {
            for_each_ngram(&token, |gram, weight| {
                let weight = weight as i64;
                grams += 1;
                maximum += MODEL_MAX_STRENGTH * weight;
                evidence += self.language_score(gram) as i64 * language_sign * weight;
            });
        }
        ModelEvidence {
            coverage: if maximum == 0 {
                0.0
            } else {
                evidence as f32 / maximum as f32
            },
            grams,
        }
    }

    #[cfg(test)]
    fn is_valid(&self) -> bool {
        if self.bytes.len() < MODEL_HEADER_SIZE || &self.bytes[..8] != MODEL_MAGIC {
            return false;
        }
        let count = self.entry_count();
        if MODEL_HEADER_SIZE.checked_add(count.saturating_mul(MODEL_ENTRY_SIZE))
            != Some(self.bytes.len())
        {
            return false;
        }
        let mut previous = None;
        for index in 0..count {
            let key = self.key_at(index);
            if self.score_at(index) == 0 || previous.is_some_and(|value| value >= key) {
                return false;
            }
            previous = Some(key);
        }
        true
    }
}

fn language_likelihood(language: Language, text: &str) -> ModelEvidence {
    static MODEL: NgramModel = NgramModel {
        bytes: LANGUAGE_MODEL,
    };
    MODEL.score(language, text)
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

fn physical_layout_punctuation_evidence(source: &str, target: &str) -> usize {
    source
        .chars()
        .zip(target.chars())
        .filter(|(source, target)| source.is_alphabetic() != target.is_alphabetic())
        .count()
}

fn normalize_word(word: &str) -> String {
    word.trim_matches(|character: char| !character.is_alphabetic())
        .to_lowercase()
}

fn is_terminal_delimiter(character: char) -> bool {
    matches!(character, ',' | '.' | '!' | '?' | ':' | ';' | '…')
}

fn is_title_case_word(word: &str) -> bool {
    let mut letters = word.chars().filter(|character| character.is_alphabetic());
    letters.next().is_some_and(char::is_uppercase) && letters.all(char::is_lowercase)
}

fn looks_like_deliberate_latin_identifier(word: &str) -> bool {
    let token = word.trim_matches(|character: char| !character.is_alphabetic());
    if token.chars().count() < 2
        || !token
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return false;
    }

    let all_uppercase = token
        .chars()
        .all(|character| character.is_ascii_uppercase());
    let internal_uppercase = token
        .chars()
        .skip(1)
        .any(|character| character.is_ascii_uppercase());
    let has_lowercase = token
        .chars()
        .any(|character| character.is_ascii_lowercase());
    all_uppercase || (internal_uppercase && has_lowercase)
}

fn looks_like_deliberate_latin_technical_token(word: &str) -> bool {
    if word.contains("://") || word.contains("::") || word.contains('@') || word.contains('_') {
        return true;
    }
    if word
        .char_indices()
        .any(|(index, character)| character == '/' && index > 0 && index + 1 < word.len())
    {
        return true;
    }

    let Some((stem, suffix)) = word.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && stem
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        && matches!(
            suffix
                .trim_matches(|character: char| !character.is_ascii_alphabetic())
                .to_ascii_lowercase()
                .as_str(),
            "com"
                | "org"
                | "net"
                | "io"
                | "dev"
                | "app"
                | "rs"
                | "toml"
                | "json"
                | "yaml"
                | "yml"
                | "md"
                | "txt"
        )
}

fn terminal_delimiter_is_likely(candidates: &Candidates, config: &Config) -> bool {
    let source_word = normalize_word(&candidates.source_word);
    let target_word = normalize_word(&candidates.target_word);
    if target_word.chars().count() < config.auto_correct_min_word_length
        || known(candidates.source_language, &source_word)
    {
        return false;
    }
    if known(candidates.target_language, &target_word) {
        return true;
    }

    let source_model = language_likelihood(candidates.source_language, &source_word);
    let target_model = language_likelihood(candidates.target_language, &target_word);
    let advantage = target_model.coverage - source_model.coverage;
    let (minimum_coverage, minimum_advantage, _) =
        model_thresholds(config.auto_correct_sensitivity);
    if is_title_case_word(&candidates.target_word) {
        target_model.coverage >= minimum_coverage.min(0.22)
            && advantage >= minimum_advantage.min(0.07)
    } else {
        target_model.coverage >= minimum_coverage && advantage >= minimum_advantage
    }
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
    fn recognizes_mistyped_ukrainian_word_with_bracket_key() {
        let correction = correction(evaluate(
            &sample("pf[sl", SystemLayout::English),
            &Config::default(),
        ));

        assert_eq!(correction.expected_source, "pf[sl");
        assert_eq!(correction.replacement, "захід");
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
    fn recognizes_mistyped_proper_names_in_both_directions() {
        for (physical, layout, expected) in [
            ("Jkmuf", SystemLayout::English, "Ольга"),
            ("Olha", SystemLayout::Ukrainian, "Olha"),
        ] {
            let correction = correction(evaluate(&sample(physical, layout), &Config::default()));
            assert_eq!(correction.replacement, expected);
        }
    }

    #[test]
    fn preserves_terminal_punctuation_when_it_is_more_likely_than_a_layout_letter() {
        for (physical, layout, expected_source, expected_replacement) in [
            ("Jkmuf,", SystemLayout::English, "Jkmuf, ", "Ольга, "),
            ("Jkmuf.", SystemLayout::English, "Jkmuf. ", "Ольга. "),
            ("Olha?", SystemLayout::Ukrainian, "Щдрф, ", "Olha? "),
            ("Olha/", SystemLayout::Ukrainian, "Щдрф. ", "Olha. "),
            ("Olha,", SystemLayout::Ukrainian, "Щдрфб ", "Olha, "),
            ("Olha.", SystemLayout::Ukrainian, "Щдрфю ", "Olha. "),
        ] {
            let correction = correction(evaluate(
                &context_sample(physical, &format!("{physical} "), layout),
                &Config::default(),
            ));
            assert_eq!(correction.expected_source, expected_source);
            assert_eq!(correction.replacement, expected_replacement);
        }
    }

    #[test]
    fn keeps_terminal_physical_punctuation_as_a_letter_when_that_forms_a_word() {
        let correction = correction(evaluate(
            &sample("[ks,", SystemLayout::English),
            &Config::default(),
        ));

        assert_eq!(correction.expected_source, "[ks,");
        assert_eq!(correction.replacement, "хліб");
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
    fn recognizes_reported_mixed_entry_mode_ukrainian_start() {
        for source in ["entry", "mode", "quite"] {
            assert_eq!(
                evaluate(&sample(source, SystemLayout::English), &Config::default()),
                AutoDecision::Reset,
                "recognized English must end its language segment: {source}"
            );
        }

        let correction = correction(evaluate(
            &sample("idblrj", SystemLayout::English),
            &Config::default(),
        ));

        assert_eq!(correction.expected_source, "idblrj");
        assert_eq!(correction.replacement, "швидко");
        assert_eq!(correction.direction, Direction::EnglishToUkrainian);
    }

    #[test]
    fn tracker_ends_the_english_segment_before_reported_ukrainian_input() {
        let mut tracker = AutoWordTracker::default();
        let mut final_decision = AutoDecision::Continue;

        for character in "entry mode is quite idblrj ".chars() {
            if character != ' ' && tracker.needs_layout_check() {
                tracker.set_source_layout(Some(SystemLayout::English));
            }
            let key = if character == ' ' {
                Keycode::Space
            } else {
                character
                    .to_ascii_uppercase()
                    .to_string()
                    .parse()
                    .expect("test phrase uses supported physical keys")
            };
            if let Some(sample) = tracker.observe(AutoKeyEvent {
                key,
                shifted: false,
            }) {
                final_decision = evaluate(&sample, &Config::default());
                if final_decision == AutoDecision::Reset {
                    tracker.clear();
                }
            }
        }

        let correction = correction(final_decision);
        assert_eq!(correction.expected_source, "idblrj ");
        assert_eq!(correction.replacement, "швидко ");
    }

    #[test]
    fn continuous_fast_ukrainian_segment_remains_correctable_at_its_last_boundary() {
        let correction = correction(evaluate(
            &context_sample("ghj", "idblrj vf' dbghfdkznb ghj ", SystemLayout::English),
            &Config::default(),
        ));

        assert_eq!(correction.expected_source, "idblrj vf' dbghfdkznb ghj ");
        assert_eq!(correction.replacement, "швидко має виправляти про ");
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
    fn converts_reported_physical_punctuation_as_ukrainian_letters() {
        let correction = correction(evaluate(
            &context_sample(",'.", ",j ]] ';b [e.v,f ,'. ,'. ", SystemLayout::English),
            &Config::default(),
        ));

        assert_eq!(correction.expected_source, ",j ]] ';b [e.v,f ,'. ,'. ");
        assert_eq!(correction.replacement, "бо її єжи хуюмба бєю бєю ");
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
        for source in [
            "github",
            "codex",
            "dmytro",
            "Codex",
            "Dmytro",
            "Rust",
            "Apple",
            "Windows",
            "println",
            "github.com",
            "src/main.rs",
            "https://example.com",
        ] {
            assert!(!matches!(
                evaluate(&sample(source, SystemLayout::English), &Config::default()),
                AutoDecision::Correct(_)
            ));
        }
        for source in ["FAANG", "SaaS", "NASDAQ", "iPhone", "ServiceNow"] {
            assert_eq!(
                evaluate(&sample(source, SystemLayout::English), &Config::default()),
                AutoDecision::Reset,
                "deliberate Latin identifier must end the source-language segment: {source}"
            );
        }
        for source in ["github.com", "src/main.rs", "https://example.com"] {
            assert_eq!(
                evaluate(&sample(source, SystemLayout::English), &Config::default()),
                AutoDecision::Reset,
                "technical token must end the source-language segment: {source}"
            );
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
    fn embedded_model_is_a_large_language_tagged_ngram_index() {
        let model = NgramModel {
            bytes: LANGUAGE_MODEL,
        };

        assert!(model.is_valid());
        assert!(model.entry_count() > 90_000);
        assert!(model.language_score(ngram_key(&['^', 't', 'h'])) < 0);
        assert!(model.language_score(ngram_key(&['^', 'п', 'р'])) > 0);
        assert!(
            !LANGUAGE_MODEL
                .windows("перевіримо".len())
                .any(|window| window == "перевіримо".as_bytes())
        );
    }

    #[test]
    fn extracted_ngrams_fall_directly_into_their_language() {
        for text in ["configuration", "accessibility", "keyboard", "language"] {
            let english = language_likelihood(Language::English, text);
            let ukrainian = language_likelihood(Language::Ukrainian, text);
            assert!(english.coverage > 0.20, "weak English evidence for {text}");
            assert!(
                english.coverage > ukrainian.coverage,
                "misclassified English token {text}"
            );
        }

        for text in ["налаштування", "доступність", "клавіатура", "перемикання"]
        {
            let english = language_likelihood(Language::English, text);
            let ukrainian = language_likelihood(Language::Ukrainian, text);
            assert!(
                ukrainian.coverage > 0.20,
                "weak Ukrainian evidence for {text}"
            );
            assert!(
                ukrainian.coverage > english.coverage,
                "misclassified Ukrainian token {text}"
            );
        }
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
        for (key, character) in [
            (Keycode::LeftBracket, '['),
            (Keycode::RightBracket, ']'),
            (Keycode::Semicolon, ';'),
            (Keycode::Apostrophe, '\''),
            (Keycode::BackSlash, '\\'),
            (Keycode::Comma, ','),
            (Keycode::Dot, '.'),
            (Keycode::Slash, '/'),
        ] {
            assert_eq!(physical_english_character(key, false), Some(character));
        }
    }
}

#[cfg(test)]
#[path = "auto_correct_synthetic_tests.rs"]
mod synthetic_typing_tests;
