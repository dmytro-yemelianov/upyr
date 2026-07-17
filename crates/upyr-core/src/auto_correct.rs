//! Platform-neutral physical-key tracking and automatic-correction decisions.

use serde::{Deserialize, Serialize};

use crate::layout::{Direction, convert, convert_with_mapping};

const ENGLISH_WORDS: &str = include_str!("../assets/dictionaries/english.txt");
const UKRAINIAN_WORDS: &str = include_str!("../assets/dictionaries/ukrainian.txt");
const LANGUAGE_MODEL: &[u8] = include_bytes!("../assets/models/language.ngm");
const MIN_NGRAM: usize = 2;
const MAX_NGRAM: usize = 5;
const MAX_CONTEXT_CHARACTERS: usize = 256;
#[cfg(test)]
const MODEL_MAGIC: &[u8; 8] = b"UPYRLM1\0";
const MODEL_HEADER_SIZE: usize = 12;
const MODEL_ENTRY_SIZE: usize = 17;
const MODEL_MAX_STRENGTH: i64 = 127;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputLayout {
    English,
    Ukrainian,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Sensitivity {
    Conservative,
    Balanced,
    Aggressive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoCorrectPolicy {
    pub sensitivity: Sensitivity,
    pub min_word_length: usize,
    pub exceptions: Vec<String>,
}

impl Default for AutoCorrectPolicy {
    fn default() -> Self {
        Self {
            sensitivity: Sensitivity::Conservative,
            min_word_length: 4,
            exceptions: Vec::new(),
        }
    }
}

/// A layout-independent writing-system key position. Names intentionally match
/// browser `KeyboardEvent.code` values where possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PhysicalKey {
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    Backquote,
    BracketLeft,
    BracketRight,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Space,
    Backspace,
    Shift,
    CapsLock,
    Control,
    Alt,
    Meta,
    Unsupported,
}

impl PhysicalKey {
    /// Converts an ASCII letter to its physical writing-system position and
    /// whether Shift is required to reproduce its case.
    pub fn from_ascii_letter(character: char) -> Option<(Self, bool)> {
        let shifted = character.is_ascii_uppercase();
        let key = match character.to_ascii_lowercase() {
            'a' => Self::KeyA,
            'b' => Self::KeyB,
            'c' => Self::KeyC,
            'd' => Self::KeyD,
            'e' => Self::KeyE,
            'f' => Self::KeyF,
            'g' => Self::KeyG,
            'h' => Self::KeyH,
            'i' => Self::KeyI,
            'j' => Self::KeyJ,
            'k' => Self::KeyK,
            'l' => Self::KeyL,
            'm' => Self::KeyM,
            'n' => Self::KeyN,
            'o' => Self::KeyO,
            'p' => Self::KeyP,
            'q' => Self::KeyQ,
            'r' => Self::KeyR,
            's' => Self::KeyS,
            't' => Self::KeyT,
            'u' => Self::KeyU,
            'v' => Self::KeyV,
            'w' => Self::KeyW,
            'x' => Self::KeyX,
            'y' => Self::KeyY,
            'z' => Self::KeyZ,
            _ => return None,
        };
        Some((key, shifted))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalKeyEvent {
    pub key: PhysicalKey,
    pub shifted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSample {
    physical_word: String,
    physical_context: String,
    source_layout: InputLayout,
}

impl WordSample {
    pub fn new(
        physical_word: impl Into<String>,
        physical_context: impl Into<String>,
        source_layout: InputLayout,
    ) -> Self {
        Self {
            physical_word: physical_word.into(),
            physical_context: physical_context.into(),
            source_layout,
        }
    }
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
    source_layout: Option<InputLayout>,
}

impl AutoWordTracker {
    pub fn can_begin(key: PhysicalKey) -> bool {
        matches!(
            key,
            PhysicalKey::KeyA
                | PhysicalKey::KeyB
                | PhysicalKey::KeyC
                | PhysicalKey::KeyD
                | PhysicalKey::KeyE
                | PhysicalKey::KeyF
                | PhysicalKey::KeyG
                | PhysicalKey::KeyH
                | PhysicalKey::KeyI
                | PhysicalKey::KeyJ
                | PhysicalKey::KeyK
                | PhysicalKey::KeyL
                | PhysicalKey::KeyM
                | PhysicalKey::KeyN
                | PhysicalKey::KeyO
                | PhysicalKey::KeyP
                | PhysicalKey::KeyQ
                | PhysicalKey::KeyR
                | PhysicalKey::KeyS
                | PhysicalKey::KeyT
                | PhysicalKey::KeyU
                | PhysicalKey::KeyV
                | PhysicalKey::KeyW
                | PhysicalKey::KeyX
                | PhysicalKey::KeyY
                | PhysicalKey::KeyZ
                | PhysicalKey::Quote
                | PhysicalKey::Backquote
                | PhysicalKey::BracketLeft
                | PhysicalKey::BracketRight
                | PhysicalKey::Backslash
                | PhysicalKey::Semicolon
                | PhysicalKey::Comma
                | PhysicalKey::Period
                | PhysicalKey::Slash
        )
    }

    pub fn needs_layout_check(&self) -> bool {
        self.current_word_start.is_none()
    }

    pub fn set_source_layout(&mut self, layout: Option<InputLayout>) {
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

    pub fn observe(&mut self, event: PhysicalKeyEvent) -> Option<WordSample> {
        match event.key {
            PhysicalKey::Space => return self.finish_word(),
            PhysicalKey::Backspace => {
                self.backspace();
                return None;
            }
            PhysicalKey::Shift
            | PhysicalKey::CapsLock
            | PhysicalKey::Control
            | PhysicalKey::Alt
            | PhysicalKey::Meta => return None,
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

pub fn evaluate(
    sample: &WordSample,
    policy: &AutoCorrectPolicy,
    mapping: Option<&[(char, char)]>,
) -> AutoDecision {
    evaluate_with_scorer(sample, policy, mapping, &SIGNED_NGRAM_V1)
}

fn evaluate_with_scorer<S: CandidateScorer + ?Sized>(
    sample: &WordSample,
    policy: &AutoCorrectPolicy,
    mapping: Option<&[(char, char)]>,
    scorer: &S,
) -> AutoDecision {
    let candidates = Candidates::new(sample, mapping);

    // A physical punctuation key can either be a letter in the other layout
    // (`,.` -> `бю`) or punctuation the user wants to keep. Score the
    // punctuation-preserving interpretation first, then fall back to the
    // ordinary whole-token conversion. This keeps `[ks,` -> `хліб`, while
    // allowing `Jkmuf,` -> `Ольга,`.
    if let Some(preserved) = Candidates::preserving_terminal_delimiter(sample, &candidates, mapping)
        && terminal_delimiter_is_likely(&preserved, policy, scorer)
    {
        if let AutoDecision::Correct(correction) = evaluate_candidates(&preserved, policy, scorer) {
            return AutoDecision::Correct(correction);
        }
    }

    evaluate_candidates(&candidates, policy, scorer)
}

fn evaluate_candidates<S: CandidateScorer + ?Sized>(
    candidates: &Candidates,
    policy: &AutoCorrectPolicy,
    scorer: &S,
) -> AutoDecision {
    let source_word = normalize_word(&candidates.source_word);
    let target_word = normalize_word(&candidates.target_word);
    if target_word.is_empty() || source_word == target_word {
        return AutoDecision::Reset;
    }

    if policy
        .exceptions
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
        current_word_length >= policy.min_word_length && target_known && !source_known;
    let context_evidence = scorer.compare(candidates.context_pair());
    let source_model = context_evidence.source;
    let target_model = context_evidence.target;
    let advantage = context_evidence.advantage();
    let (minimum_coverage, minimum_advantage, minimum_characters) =
        model_thresholds(policy.sensitivity);
    let physical_punctuation_evidence = physical_layout_punctuation_evidence(
        &candidates.source_context,
        &candidates.target_context,
    );
    let physical_letter_target = matches!(candidates.target_language, Language::Ukrainian)
        && physical_punctuation_evidence > 0
        && candidates
            .target_word
            .chars()
            .all(|character| character.is_alphabetic() || matches!(character, '\'' | '’'));
    // The relaxed threshold exists for physical `[];'\\,./` keys becoming
    // Ukrainian letters. It must not run in reverse: otherwise ordinary
    // Ukrainian words such as `рубці` can be mistaken for punctuation-heavy
    // English gibberish such as `he,ws`.
    let base_required_advantage = if physical_letter_target {
        minimum_advantage / 2.0
    } else {
        minimum_advantage
    };
    // When both interpretations already look natural, the pair is a real
    // keyboard collision rather than a clear wrong-layout signal. Demand an
    // extra half-margin instead of changing text on a narrow win (`дупу`
    // versus the physically corresponding `lege` is a concrete example).
    let ambiguous_language_pair = !source_known
        && !target_known
        && source_model.coverage >= minimum_coverage
        && target_model.coverage >= minimum_coverage;
    let required_advantage = base_required_advantage
        + if ambiguous_language_pair {
            minimum_advantage / 2.0
        } else {
            0.0
        };
    let model_match = !source_known
        && context_characters >= minimum_characters.max(policy.min_word_length)
        && target_model.grams >= 3
        && target_model.coverage >= minimum_coverage
        && advantage >= required_advantage;
    let physical_letter_model_match = !source_known
        && current_word_length >= policy.min_word_length
        && physical_letter_target
        && target_model.grams >= 3
        && target_model.coverage >= minimum_coverage
        && advantage >= -0.05;
    let name_evidence = scorer.compare(candidates.text_pair(&source_word, &target_word));
    let source_name_model = name_evidence.source;
    let target_name_model = name_evidence.target;
    let proper_name_model_match = !source_known
        && current_word_length >= policy.min_word_length
        && is_title_case_word(&candidates.target_word)
        && target_name_model.grams >= 3
        && target_name_model.coverage >= minimum_coverage.min(0.22)
        && target_name_model.coverage - source_name_model.coverage >= required_advantage.min(0.07);
    let source_model_match = context_characters >= minimum_characters.max(policy.min_word_length)
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
    fn new(sample: &WordSample, mapping: Option<&[(char, char)]>) -> Self {
        let ukrainian_word = to_ukrainian(&sample.physical_word, mapping);
        let ukrainian_context = to_ukrainian(&sample.physical_context, mapping);
        match sample.source_layout {
            InputLayout::English => Self {
                source_word: sample.physical_word.clone(),
                target_word: ukrainian_word,
                source_context: sample.physical_context.clone(),
                target_context: ukrainian_context,
                direction: Direction::EnglishToUkrainian,
                source_language: Language::English,
                target_language: Language::Ukrainian,
            },
            InputLayout::Ukrainian => Self {
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

    fn preserving_terminal_delimiter(
        sample: &WordSample,
        full: &Self,
        mapping: Option<&[(char, char)]>,
    ) -> Option<Self> {
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
            InputLayout::English => to_ukrainian(&physical_word, mapping),
            InputLayout::Ukrainian => physical_word.clone(),
        };
        let physical_target_context = match sample.source_layout {
            InputLayout::English => {
                to_ukrainian(&format!("{physical_prefix}{physical_word}"), mapping)
            }
            InputLayout::Ukrainian => format!("{physical_prefix}{physical_word}"),
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

    fn context_pair(&self) -> CandidatePair<'_> {
        self.text_pair(&self.source_context, &self.target_context)
    }

    fn text_pair<'a>(&self, source: &'a str, target: &'a str) -> CandidatePair<'a> {
        CandidatePair {
            source: LanguageText {
                language: self.source_language,
                text: source,
            },
            target: LanguageText {
                language: self.target_language,
                text: target,
            },
        }
    }
}

fn to_ukrainian(physical_english: &str, mapping: Option<&[(char, char)]>) -> String {
    match mapping {
        Some(mapping) => {
            convert_with_mapping(physical_english, Direction::EnglishToUkrainian, mapping).text
        }
        None => convert(physical_english, Direction::EnglishToUkrainian).text,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    English,
    Ukrainian,
}

#[derive(Debug, Clone, Copy)]
struct LanguageText<'a> {
    language: Language,
    text: &'a str,
}

#[derive(Debug, Clone, Copy)]
struct CandidatePair<'a> {
    source: LanguageText<'a>,
    target: LanguageText<'a>,
}

#[derive(Debug, Clone, Copy)]
struct PairEvidence {
    source: ModelEvidence,
    target: ModelEvidence,
}

impl PairEvidence {
    fn advantage(self) -> f32 {
        self.target.coverage - self.source.coverage
    }
}

trait CandidateScorer {
    fn compare(&self, pair: CandidatePair<'_>) -> PairEvidence;
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

struct NgramModel<'a> {
    bytes: &'a [u8],
}

static SIGNED_NGRAM_V1: NgramModel<'static> = NgramModel {
    bytes: LANGUAGE_MODEL,
};

impl NgramModel<'_> {
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

impl CandidateScorer for NgramModel<'_> {
    fn compare(&self, pair: CandidatePair<'_>) -> PairEvidence {
        PairEvidence {
            source: self.score(pair.source.language, pair.source.text),
            target: self.score(pair.target.language, pair.target.text),
        }
    }
}

#[cfg(test)]
fn language_likelihood(language: Language, text: &str) -> ModelEvidence {
    SIGNED_NGRAM_V1.score(language, text)
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

fn model_thresholds(sensitivity: Sensitivity) -> (f32, f32, usize) {
    match sensitivity {
        Sensitivity::Conservative => (0.28, 0.20, 4),
        Sensitivity::Balanced => (0.22, 0.13, 4),
        Sensitivity::Aggressive => (0.16, 0.07, 3),
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

fn terminal_delimiter_is_likely<S: CandidateScorer + ?Sized>(
    candidates: &Candidates,
    policy: &AutoCorrectPolicy,
    scorer: &S,
) -> bool {
    let source_word = normalize_word(&candidates.source_word);
    let target_word = normalize_word(&candidates.target_word);
    if target_word.chars().count() < policy.min_word_length
        || known(candidates.source_language, &source_word)
    {
        return false;
    }
    if known(candidates.target_language, &target_word) {
        return true;
    }

    let evidence = scorer.compare(candidates.text_pair(&source_word, &target_word));
    let target_model = evidence.target;
    let advantage = evidence.advantage();
    let (minimum_coverage, minimum_advantage, _) = model_thresholds(policy.sensitivity);
    if is_title_case_word(&candidates.target_word) {
        target_model.coverage >= minimum_coverage.min(0.22)
            && advantage >= minimum_advantage.min(0.07)
    } else {
        target_model.coverage >= minimum_coverage && advantage >= minimum_advantage
    }
}

fn physical_english_character(key: PhysicalKey, shifted: bool) -> Option<char> {
    let letter = match key {
        PhysicalKey::KeyA => Some('a'),
        PhysicalKey::KeyB => Some('b'),
        PhysicalKey::KeyC => Some('c'),
        PhysicalKey::KeyD => Some('d'),
        PhysicalKey::KeyE => Some('e'),
        PhysicalKey::KeyF => Some('f'),
        PhysicalKey::KeyG => Some('g'),
        PhysicalKey::KeyH => Some('h'),
        PhysicalKey::KeyI => Some('i'),
        PhysicalKey::KeyJ => Some('j'),
        PhysicalKey::KeyK => Some('k'),
        PhysicalKey::KeyL => Some('l'),
        PhysicalKey::KeyM => Some('m'),
        PhysicalKey::KeyN => Some('n'),
        PhysicalKey::KeyO => Some('o'),
        PhysicalKey::KeyP => Some('p'),
        PhysicalKey::KeyQ => Some('q'),
        PhysicalKey::KeyR => Some('r'),
        PhysicalKey::KeyS => Some('s'),
        PhysicalKey::KeyT => Some('t'),
        PhysicalKey::KeyU => Some('u'),
        PhysicalKey::KeyV => Some('v'),
        PhysicalKey::KeyW => Some('w'),
        PhysicalKey::KeyX => Some('x'),
        PhysicalKey::KeyY => Some('y'),
        PhysicalKey::KeyZ => Some('z'),
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
        PhysicalKey::Backquote => '`',
        PhysicalKey::BracketLeft => '[',
        PhysicalKey::BracketRight => ']',
        PhysicalKey::Backslash => '\\',
        PhysicalKey::Semicolon => ';',
        PhysicalKey::Quote => '\'',
        PhysicalKey::Comma => ',',
        PhysicalKey::Period => '.',
        PhysicalKey::Slash => '/',
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
    use std::cell::RefCell;

    use super::*;

    #[derive(Default)]
    struct RecordingTargetScorer {
        pairs: RefCell<Vec<(String, String)>>,
    }

    impl CandidateScorer for RecordingTargetScorer {
        fn compare(&self, pair: CandidatePair<'_>) -> PairEvidence {
            self.pairs
                .borrow_mut()
                .push((pair.source.text.to_owned(), pair.target.text.to_owned()));
            PairEvidence {
                source: ModelEvidence {
                    coverage: 0.0,
                    grams: 8,
                },
                target: ModelEvidence {
                    coverage: 0.9,
                    grams: 8,
                },
            }
        }
    }

    fn evaluate(sample: &WordSample, policy: &AutoCorrectPolicy) -> AutoDecision {
        super::evaluate(sample, policy, None)
    }

    fn sample(physical_word: &str, source_layout: InputLayout) -> WordSample {
        WordSample {
            physical_word: physical_word.to_owned(),
            physical_context: physical_word.to_owned(),
            source_layout,
        }
    }

    fn context_sample(
        physical_word: &str,
        physical_context: &str,
        source_layout: InputLayout,
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
    fn injected_pairwise_scorer_can_drive_an_unknown_candidate() {
        let scorer = RecordingTargetScorer::default();
        let correction = correction(evaluate_with_scorer(
            &sample("zzzz", InputLayout::English),
            &AutoCorrectPolicy::default(),
            None,
            &scorer,
        ));

        assert_eq!(correction.expected_source, "zzzz");
        assert_eq!(correction.replacement, "яяяя");
        assert_eq!(correction.direction, Direction::EnglishToUkrainian);
        assert_eq!(scorer.pairs.borrow().len(), 2);
    }

    #[test]
    fn terminal_delimiter_branch_uses_the_injected_scorer() {
        let scorer = RecordingTargetScorer::default();
        let correction = correction(evaluate_with_scorer(
            &context_sample("zzzz,", "zzzz, ", InputLayout::English),
            &AutoCorrectPolicy::default(),
            None,
            &scorer,
        ));

        assert_eq!(correction.expected_source, "zzzz, ");
        assert_eq!(correction.replacement, "яяяя, ");
        assert_eq!(correction.direction, Direction::EnglishToUkrainian);
        let pairs = scorer.pairs.borrow();
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0], ("zzzz".to_owned(), "яяяя".to_owned()));
        assert!(
            pairs
                .iter()
                .any(|pair| pair == &("zzzz, ".to_owned(), "яяяя, ".to_owned()))
        );
    }

    #[test]
    fn recognizes_mistyped_ukrainian_greeting() {
        let correction = correction(evaluate(
            &sample("ghbdsn", InputLayout::English),
            &AutoCorrectPolicy::default(),
        ));

        assert_eq!(correction.expected_source, "ghbdsn");
        assert_eq!(correction.replacement, "привіт");
        assert_eq!(correction.direction, Direction::EnglishToUkrainian);
    }

    #[test]
    fn recognizes_mistyped_ukrainian_word_with_bracket_key() {
        let correction = correction(evaluate(
            &sample("pf[sl", InputLayout::English),
            &AutoCorrectPolicy::default(),
        ));

        assert_eq!(correction.expected_source, "pf[sl");
        assert_eq!(correction.replacement, "захід");
        assert_eq!(correction.direction, Direction::EnglishToUkrainian);
    }

    #[test]
    fn candidate_generation_uses_an_injected_physical_mapping() {
        let mapping = [
            ('a', 'п'),
            ('b', 'р'),
            ('c', 'и'),
            ('d', 'в'),
            ('e', 'і'),
            ('f', 'т'),
            (',', 'б'),
        ];
        let correction = correction(super::evaluate(
            &context_sample("abcdef,", "abcdef, ", InputLayout::English),
            &AutoCorrectPolicy::default(),
            Some(&mapping),
        ));

        assert_eq!(correction.expected_source, "abcdef, ");
        assert_eq!(correction.replacement, "привіт, ");
    }

    #[test]
    fn recognizes_mistyped_english_greeting() {
        let correction = correction(evaluate(
            &sample("hello", InputLayout::Ukrainian),
            &AutoCorrectPolicy::default(),
        ));

        assert_eq!(correction.expected_source, "руддщ");
        assert_eq!(correction.replacement, "hello");
        assert_eq!(correction.direction, Direction::UkrainianToEnglish);
    }

    #[test]
    fn recognizes_mistyped_proper_names_in_both_directions() {
        for (physical, layout, expected) in [
            ("Jkmuf", InputLayout::English, "Ольга"),
            ("Olha", InputLayout::Ukrainian, "Olha"),
        ] {
            let correction = correction(evaluate(
                &sample(physical, layout),
                &AutoCorrectPolicy::default(),
            ));
            assert_eq!(correction.replacement, expected);
        }
    }

    #[test]
    fn preserves_terminal_punctuation_when_it_is_more_likely_than_a_layout_letter() {
        for (physical, layout, expected_source, expected_replacement) in [
            ("Jkmuf,", InputLayout::English, "Jkmuf, ", "Ольга, "),
            ("Jkmuf.", InputLayout::English, "Jkmuf. ", "Ольга. "),
            ("Olha?", InputLayout::Ukrainian, "Щдрф, ", "Olha? "),
            ("Olha/", InputLayout::Ukrainian, "Щдрф. ", "Olha. "),
            ("Olha,", InputLayout::Ukrainian, "Щдрфб ", "Olha, "),
            ("Olha.", InputLayout::Ukrainian, "Щдрфю ", "Olha. "),
        ] {
            let correction = correction(evaluate(
                &context_sample(physical, &format!("{physical} "), layout),
                &AutoCorrectPolicy::default(),
            ));
            assert_eq!(correction.expected_source, expected_source);
            assert_eq!(correction.replacement, expected_replacement);
        }
    }

    #[test]
    fn keeps_terminal_physical_punctuation_as_a_letter_when_that_forms_a_word() {
        let correction = correction(evaluate(
            &sample("[ks,", InputLayout::English),
            &AutoCorrectPolicy::default(),
        ));

        assert_eq!(correction.expected_source, "[ks,");
        assert_eq!(correction.replacement, "хліб");
    }

    #[test]
    fn keeps_plausible_native_ukrainian_keyboard_collisions() {
        for (physical, visible) in [
            ("he,ws", "рубці"),
            ("lege", "дупу"),
            ("heis]", "рушії"),
            ("[ensh", "хутір"),
        ] {
            assert!(
                !matches!(
                    evaluate(
                        &sample(physical, InputLayout::Ukrainian),
                        &AutoCorrectPolicy::default()
                    ),
                    AutoDecision::Correct(_)
                ),
                "native Ukrainian word {visible:?} must not be corrected"
            );
        }
    }

    #[test]
    fn ngram_model_recognizes_words_missing_from_dictionary() {
        for (physical, expected) in [
            ("lfdfq", "давай"),
            ("gthtdshbvj", "перевіримо"),
            ("xjve", "чому"),
        ] {
            let correction = correction(evaluate(
                &sample(physical, InputLayout::English),
                &AutoCorrectPolicy::default(),
            ));
            assert_eq!(correction.replacement, expected);
        }
    }

    #[test]
    fn recognizes_reported_mixed_entry_mode_ukrainian_start() {
        for source in ["entry", "mode", "quite"] {
            assert_eq!(
                evaluate(
                    &sample(source, InputLayout::English),
                    &AutoCorrectPolicy::default()
                ),
                AutoDecision::Reset,
                "recognized English must end its language segment: {source}"
            );
        }

        let correction = correction(evaluate(
            &sample("idblrj", InputLayout::English),
            &AutoCorrectPolicy::default(),
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
                tracker.set_source_layout(Some(InputLayout::English));
            }
            let key = if character == ' ' {
                PhysicalKey::Space
            } else {
                PhysicalKey::from_ascii_letter(character)
                    .expect("test phrase uses supported physical keys")
                    .0
            };
            if let Some(sample) = tracker.observe(PhysicalKeyEvent {
                key,
                shifted: false,
            }) {
                final_decision = evaluate(&sample, &AutoCorrectPolicy::default());
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
            &context_sample("ghj", "idblrj vf' dbghfdkznb ghj ", InputLayout::English),
            &AutoCorrectPolicy::default(),
        ));

        assert_eq!(correction.expected_source, "idblrj vf' dbghfdkznb ghj ");
        assert_eq!(correction.replacement, "швидко має виправляти про ");
    }

    #[test]
    fn corrects_the_accumulated_prefix_when_confidence_becomes_high() {
        let correction = correction(evaluate(
            &context_sample(",elt", "nfr f xb ,elt ", InputLayout::English),
            &AutoCorrectPolicy::default(),
        ));

        assert_eq!(correction.expected_source, "nfr f xb ,elt ");
        assert_eq!(correction.replacement, "так а чи буде ");
    }

    #[test]
    fn converts_reported_physical_punctuation_as_ukrainian_letters() {
        let correction = correction(evaluate(
            &context_sample(",'.", ",j ]] ';b [e.v,f ,'. ,'. ", InputLayout::English),
            &AutoCorrectPolicy::default(),
        ));

        assert_eq!(correction.expected_source, ",j ]] ';b [e.v,f ,'. ,'. ");
        assert_eq!(correction.replacement, "бо її єжи хуюмба бєю бєю ");
    }

    #[test]
    fn leaves_valid_words_exceptions_and_technical_text_alone() {
        assert_eq!(
            evaluate(
                &sample("hello", InputLayout::English),
                &AutoCorrectPolicy::default()
            ),
            AutoDecision::Reset
        );
        assert_eq!(
            evaluate(
                &sample("ghbdsn", InputLayout::English),
                &AutoCorrectPolicy {
                    exceptions: vec!["ghbdsn".to_owned()],
                    ..AutoCorrectPolicy::default()
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
                evaluate(
                    &sample(source, InputLayout::English),
                    &AutoCorrectPolicy::default()
                ),
                AutoDecision::Correct(_)
            ));
        }
        for source in ["FAANG", "SaaS", "NASDAQ", "iPhone", "ServiceNow"] {
            assert_eq!(
                evaluate(
                    &sample(source, InputLayout::English),
                    &AutoCorrectPolicy::default()
                ),
                AutoDecision::Reset,
                "deliberate Latin identifier must end the source-language segment: {source}"
            );
        }
        for source in ["github.com", "src/main.rs", "https://example.com"] {
            assert_eq!(
                evaluate(
                    &sample(source, InputLayout::English),
                    &AutoCorrectPolicy::default()
                ),
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
        assert_eq!(model.entry_count(), 173_964);
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
        tracker.set_source_layout(Some(InputLayout::English));
        for key in [PhysicalKey::KeyN, PhysicalKey::KeyF, PhysicalKey::KeyR] {
            assert!(
                tracker
                    .observe(PhysicalKeyEvent {
                        key,
                        shifted: false,
                    })
                    .is_none()
            );
        }

        let first = tracker
            .observe(PhysicalKeyEvent {
                key: PhysicalKey::Space,
                shifted: false,
            })
            .unwrap();
        assert_eq!(first.physical_word, "nfr");
        assert_eq!(first.physical_context, "nfr ");

        for key in [PhysicalKey::KeyF, PhysicalKey::KeyX, PhysicalKey::KeyB] {
            tracker.observe(PhysicalKeyEvent {
                key,
                shifted: false,
            });
        }
        let second = tracker
            .observe(PhysicalKeyEvent {
                key: PhysicalKey::Space,
                shifted: false,
            })
            .unwrap();
        assert_eq!(second.physical_word, "fxb");
        assert_eq!(second.physical_context, "nfr fxb ");
    }

    #[test]
    fn tracker_clears_on_navigation_and_layout_changes() {
        let mut tracker = AutoWordTracker::default();
        tracker.set_source_layout(Some(InputLayout::English));
        tracker.observe(PhysicalKeyEvent {
            key: PhysicalKey::KeyA,
            shifted: false,
        });
        tracker.observe(PhysicalKeyEvent {
            key: PhysicalKey::Unsupported,
            shifted: false,
        });
        assert!(tracker.needs_layout_check());

        tracker.set_source_layout(Some(InputLayout::English));
        tracker.observe(PhysicalKeyEvent {
            key: PhysicalKey::KeyA,
            shifted: false,
        });
        tracker.set_source_layout(Some(InputLayout::Ukrainian));
        assert!(tracker.needs_layout_check());
    }

    #[test]
    fn physical_punctuation_supports_ukrainian_letters() {
        for (key, character) in [
            (PhysicalKey::BracketLeft, '['),
            (PhysicalKey::BracketRight, ']'),
            (PhysicalKey::Semicolon, ';'),
            (PhysicalKey::Quote, '\''),
            (PhysicalKey::Backslash, '\\'),
            (PhysicalKey::Comma, ','),
            (PhysicalKey::Period, '.'),
            (PhysicalKey::Slash, '/'),
        ] {
            assert_eq!(physical_english_character(key, false), Some(character));
        }
    }
}

#[cfg(test)]
#[path = "auto_correct/replay_benchmark.rs"]
mod replay_benchmark;

#[cfg(test)]
#[path = "auto_correct_synthetic_tests.rs"]
mod synthetic_typing_tests;
