//! Early correction uses Punto triggers or persistent substring/prefix rules,
//! never evidence from previous words. Abstention must leave the tracker intact.

use super::*;

/// Corrects a clearly wrong layout before the word ends. Hosts must switch the
/// input layout with the edit so that the remaining keys complete the word.
/// Unlike `evaluate`, this only replaces the current word, without a separator.
pub fn evaluate_partial(
    sample: &WordSample,
    policy: &AutoCorrectPolicy,
    mapping: Option<&[(char, char)]>,
) -> AutoDecision {
    evaluate_partial_with_lookup(sample, policy, mapping, prefix_lexicon::languages)
}

/// Reusable lexical/rule transitions, integer score prefixes and scratch buffers
/// for one early-correction stream. Policy and mapping are checked on every call;
/// exception normalization is keyed by the complete current exception list.
/// Call `reset` when the host clears its word/context.
#[derive(Debug, Default, Clone)]
pub struct PartialWordState {
    cursor: prefix_lexicon::Cursor,
    normalized: String,
    ukrainian: String,
    lowercase: String,
    canonical: String,
    mapping: MappingState,
    exceptions: Vec<String>,
    normalized_exceptions: Vec<String>,
    source_score: PrefixScoreState,
    target_score: PrefixScoreState,
    punto: punto_early::EarlyState,
}

impl PartialWordState {
    pub fn reset(&mut self) {
        self.cursor.reset();
        self.source_score.reset();
        self.target_score.reset();
        self.punto.reset();
    }

    pub fn evaluate(
        &mut self,
        sample: &WordSample,
        policy: &AutoCorrectPolicy,
        mapping: Option<&[(char, char)]>,
    ) -> AutoDecision {
        self.evaluate_physical(&sample.physical_word, sample.source_layout, policy, mapping)
    }

    /// Borrow the live word instead of allocating a word/context snapshot.
    /// Non-ASCII callers retain the general Unicode evaluator's semantics.
    pub fn evaluate_physical(
        &mut self,
        physical: &str,
        layout: InputLayout,
        policy: &AutoCorrectPolicy,
        mapping: Option<&[(char, char)]>,
    ) -> AutoDecision {
        if !physical.is_ascii() {
            return evaluate_partial(
                &WordSample::new(physical, physical, layout),
                policy,
                mapping,
            );
        }
        if physical.len() < policy.min_word_length.max(4) || physical.len() > 64 {
            return AutoDecision::Continue;
        }
        // One candidate is always the literal ASCII physical word. The later
        // alphabetic/identifier guards therefore already reject these inputs,
        // independently of a custom map or an explicit Correct trigger.
        if !physical.bytes().all(|c| c.is_ascii_alphabetic())
            || (layout == InputLayout::English && looks_like_deliberate_latin_identifier(physical))
        {
            return AutoDecision::Continue;
        }
        self.normalized.clear();
        self.normalized.push_str(physical);
        self.normalized.make_ascii_lowercase();
        self.ukrainian.clear();
        self.canonical.clear();
        let builtin = builtin_ascii_mapping();
        let actual_mapping = self.mapping.prepare(mapping);
        let mut compatible = true;
        for c in physical.bytes() {
            let default = builtin[c as usize];
            let actual = actual_mapping[c as usize];
            compatible &= actual == default;
            self.ukrainian.push(actual);
            self.canonical.push(match default {
                'і' => 'ы',
                'І' => 'Ы',
                'ї' => 'ъ',
                'Ї' => 'Ъ',
                'є' => 'э',
                'Є' => 'Э',
                '\'' => 'ё',
                '₴' => 'Ё',
                'ґ' => '\\',
                'Ґ' => '|',
                _ => default,
            });
        }
        let user_match = policy.triggers.iter().any(|rule| {
            rule.action == TriggerAction::Correct
                && rule.physical.ends_with('*')
                && !rule.physical.starts_with('*')
                && rule.matches(&self.normalized, layout)
        });
        if !(user_match || compatible && self.punto.matches(physical, &self.canonical, layout)) {
            return AutoDecision::Continue;
        }
        if !user_match {
            let languages = self.cursor.languages(&self.normalized);
            let (source, target) = match layout {
                InputLayout::English => (prefix_lexicon::ENGLISH, prefix_lexicon::UKRAINIAN),
                InputLayout::Ukrainian => (prefix_lexicon::UKRAINIAN, prefix_lexicon::ENGLISH),
            };
            if languages & source != 0 || languages & target == 0 {
                return AutoDecision::Continue;
            }
        }
        let english_lower = self.normalized.as_str();
        let ukrainian_lower = lowercase_reusing(&self.ukrainian, &mut self.lowercase);
        let (source, target, source_raw, target_raw, source_language, target_language, direction) =
            match layout {
                InputLayout::English => (
                    english_lower,
                    ukrainian_lower.as_ref(),
                    physical,
                    self.ukrainian.as_str(),
                    Language::English,
                    Language::Ukrainian,
                    Direction::EnglishToUkrainian,
                ),
                InputLayout::Ukrainian => (
                    ukrainian_lower.as_ref(),
                    english_lower,
                    self.ukrainian.as_str(),
                    physical,
                    Language::Ukrainian,
                    Language::English,
                    Direction::UkrainianToEnglish,
                ),
            };
        if !source.chars().all(char::is_alphabetic)
            || !target.chars().all(char::is_alphabetic)
            || source == target
            || dictionary_prefixes(source_language).contains(source)
        {
            return AutoDecision::Continue;
        }
        if !user_match {
            let source_score = self.source_score.score(source_language, source);
            let target_score = self.target_score.score(target_language, target);
            let (coverage, advantage, _) = model_thresholds(Sensitivity::Conservative);
            if target_score < coverage || target_score - source_score < advantage {
                return AutoDecision::Continue;
            }
        }
        if self.exceptions != policy.exceptions {
            self.exceptions.clone_from(&policy.exceptions);
            self.normalized_exceptions = policy
                .exceptions
                .iter()
                .map(|s| normalize_word(s).into_owned())
                .collect();
        }
        if self.normalized_exceptions.iter().any(|exception| {
            !exception.is_empty()
                && (exception.starts_with(source) || source.starts_with(exception))
        }) || policy.triggers.iter().any(|trigger| {
            if trigger.action != TriggerAction::Keep || trigger.source_layout != layout {
                return false;
            }
            let stem = trigger.physical.trim_matches('*');
            trigger.physical.starts_with('*')
                || stem.starts_with(&self.normalized)
                || self.normalized.starts_with(stem)
        }) {
            return AutoDecision::Continue;
        }
        AutoDecision::Correct(AutoCorrection {
            expected_source: source_raw.to_owned(),
            replacement: target_raw.to_owned(),
            direction,
        })
    }
}

/// ASCII and Cyrillic casing is scalar-local. Reuse a buffer for their uppercase
/// forms; uncommon mappings retain full Unicode casing (including final sigma).
fn lowercase_reusing<'a>(text: &'a str, scratch: &'a mut String) -> std::borrow::Cow<'a, str> {
    let mut uppercase = false;
    for c in text.chars() {
        if !(c.is_ascii() || ('\u{400}'..'\u{500}').contains(&c)) {
            return punto_trie::lowercase(text);
        }
        uppercase |= c.is_uppercase();
    }
    if !uppercase {
        return std::borrow::Cow::Borrowed(text);
    }
    scratch.clear();
    scratch.extend(text.chars().flat_map(char::to_lowercase));
    std::borrow::Cow::Borrowed(scratch.as_str())
}

fn builtin_ascii_mapping() -> &'static [char; 128] {
    static MAP: OnceLock<[char; 128]> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = std::array::from_fn(|i| i as u8 as char);
        // Preserve first-pair-wins semantics, including duplicate positions.
        for (from, to) in crate::default_physical_mapping().into_iter().rev() {
            if from.is_ascii() {
                map[from as usize] = to;
            }
        }
        map
    })
}

/// A custom slice may be edited in place, so cache by its complete contents.
/// The bounded direct table is lazy: built-in mappings need no owned cache.
#[derive(Debug, Default, Clone)]
struct MappingState {
    custom: Option<Box<CustomMapping>>,
}

#[derive(Debug, Clone)]
struct CustomMapping {
    source: Vec<(char, char)>,
    ascii: [char; 128],
}

impl MappingState {
    fn prepare(&mut self, mapping: Option<&[(char, char)]>) -> &[char; 128] {
        let Some(mapping) = mapping else {
            return builtin_ascii_mapping();
        };
        if self
            .custom
            .as_ref()
            .is_none_or(|cache| cache.source != mapping)
        {
            let cache = self.custom.get_or_insert_with(|| {
                Box::new(CustomMapping {
                    source: Vec::new(),
                    ascii: std::array::from_fn(|i| i as u8 as char),
                })
            });
            cache.source.clear();
            cache.source.extend_from_slice(mapping);
            cache.ascii = std::array::from_fn(|i| i as u8 as char);
            // The public conversion API resolves duplicate keys from the
            // beginning of the slice, and leaves unmapped characters intact.
            for &(from, to) in mapping.iter().rev() {
                if from.is_ascii() {
                    cache.ascii[from as usize] = to;
                }
            }
        }
        &self.custom.as_ref().unwrap().ascii
    }
}

/// Integer prefix sums make append, Backspace and branch edits exact. Only
/// n-grams ending at a changed/new character need another model lookup.
/// At most 65 characters (including ^), four grams and weight <= 4 keep the
/// complete evidence well inside i32; division remains identical to reference.
#[derive(Debug, Clone)]
struct PrefixScoreState {
    characters: [char; 65],
    evidence: [i32; 65],
    len: usize,
}

impl Default for PrefixScoreState {
    fn default() -> Self {
        let mut characters = ['\0'; 65];
        characters[0] = '^';
        Self {
            characters,
            evidence: [0; 65],
            len: 1,
        }
    }
}

impl PrefixScoreState {
    fn reset(&mut self) {
        self.len = 1;
    }

    fn score(&mut self, language: Language, text: &str) -> f32 {
        if text.chars().count() > 64 {
            return prefix_score(language, text);
        }
        let mut changed = false;
        let mut len = 1;
        for (index, c) in text.chars().enumerate() {
            let index = index + 1;
            changed |= index >= self.len || self.characters[index] != c;
            if changed {
                self.characters[index] = c;
                let mut delta = 0;
                for size in MIN_NGRAM..=MAX_NGRAM.min(index + 1) {
                    delta += i32::from(
                        SIGNED_NGRAM_V1
                            .language_score(ngram_key(&self.characters[index + 1 - size..=index])),
                    ) * (size - 1) as i32;
                }
                self.evidence[index] = self.evidence[index - 1] + delta;
            }
            len += 1;
        }
        self.len = len;
        let maximum: i64 = (MIN_NGRAM..=MAX_NGRAM)
            .map(|size| {
                len.saturating_sub(size - 1) as i64 * MODEL_MAX_STRENGTH * (size - 1) as i64
            })
            .sum();
        if maximum == 0 {
            return 0.0;
        }
        let sign = if language == Language::English { -1 } else { 1 };
        (i64::from(self.evidence[len - 1]) * sign) as f32 / maximum as f32
    }
}

fn evaluate_partial_with_lookup(
    sample: &WordSample,
    policy: &AutoCorrectPolicy,
    mapping: Option<&[(char, char)]>,
    mut lookup: impl FnMut(&str) -> u8,
) -> AutoDecision {
    let length = sample.physical_word.chars().count();
    if length < policy.min_word_length.max(4) || length > 64 {
        return AutoDecision::Continue;
    }
    let physical = normalize_physical(&sample.physical_word);
    // Imported tables refer to fixed key positions. Preserve case for Punto's
    // case-sensitive exceptions, and abstain when a custom map changes them.
    let builtin_match = to_ukrainian(&sample.physical_word, mapping)
        == to_ukrainian(&sample.physical_word, None)
        && punto_early::matches(&sample.physical_word, sample.source_layout);
    let user_match = policy.triggers.iter().any(|rule| {
        rule.action == TriggerAction::Correct
            && rule.physical.ends_with('*')
            && !rule.physical.starts_with('*')
            && rule.matches(&physical, sample.source_layout)
    });
    if !builtin_match && !user_match {
        return AutoDecision::Continue;
    }
    // Lexical absence alone cannot cause correction: an imported rule is
    // still required. Both candidates share physical keys, so one automaton
    // lookup tells us whether either language has known word continuations.
    if !user_match {
        let languages = lookup(&physical);
        let (source, target) = match sample.source_layout {
            InputLayout::English => (prefix_lexicon::ENGLISH, prefix_lexicon::UKRAINIAN),
            InputLayout::Ukrainian => (prefix_lexicon::UKRAINIAN, prefix_lexicon::ENGLISH),
        };
        if languages & source != 0 || languages & target == 0 {
            return AutoDecision::Continue;
        }
    }
    let word = WordSample::new(
        &sample.physical_word,
        &sample.physical_word,
        sample.source_layout,
    );
    let candidates = Candidates::new(&word, mapping);
    let source = candidates.source_word().to_lowercase();
    let target = candidates.target_word().to_lowercase();
    // Punctuation, digits and mixed-case identifiers need a complete token.
    if !source.chars().all(char::is_alphabetic)
        || !target.chars().all(char::is_alphabetic)
        || source == target
        || (candidates.source_language == Language::English
            && looks_like_deliberate_latin_identifier(candidates.source_word()))
        || dictionary_prefixes(candidates.source_language).contains(source.as_str())
    {
        return AutoDecision::Continue;
    }
    // Russian invalid combinations can be legitimate Ukrainian (e.g. дівк…).
    // An imported trigger or persistent source rule is necessary, but EN/UK prefix evidence
    // must also favor its target. A scorer alone can never initiate a switch.
    if !user_match {
        let source_score = prefix_score(candidates.source_language, &source);
        let target_score = prefix_score(candidates.target_language, &target);
        let (coverage, advantage, _) = model_thresholds(Sensitivity::Conservative);
        if target_score < coverage || target_score - source_score < advantage {
            return AutoDecision::Continue;
        }
    }
    // Protect both an exception already typed and one still being completed.
    // A future suffix/substring Keep rule can always match a continuation, so
    // conservatively defer such words to the ordinary boundary policy.
    if policy.exceptions.iter().any(|exception| {
        let exception = normalize_word(exception);
        !exception.is_empty()
            && (exception.starts_with(&source) || source.starts_with(exception.as_ref()))
    }) || policy.triggers.iter().any(|trigger| {
        if trigger.action != TriggerAction::Keep || trigger.source_layout != sample.source_layout {
            return false;
        }
        let stem = trigger.physical.trim_matches('*');
        trigger.physical.starts_with('*')
            || stem.starts_with(&physical)
            || physical.starts_with(stem)
    }) {
        return AutoDecision::Continue;
    }

    AutoDecision::Correct(AutoCorrection {
        expected_source: candidates.source_word().to_owned(),
        replacement: candidates.target_word().to_owned(),
        direction: candidates.direction,
    })
}

fn prefix_score(language: Language, text: &str) -> f32 {
    // The word is still open: include its beginning, never a fabricated `$`.
    let chars = std::iter::once('^').chain(text.chars()).collect::<Vec<_>>();
    let mut evidence = 0i64;
    let mut maximum = 0i64;
    for size in MIN_NGRAM..=MAX_NGRAM {
        for gram in chars.windows(size) {
            let weight = (size - 1) as i64;
            evidence += SIGNED_NGRAM_V1.language_score(ngram_key(gram)) as i64 * weight;
            maximum += MODEL_MAX_STRENGTH * weight;
        }
    }
    if maximum == 0 {
        return 0.0;
    }
    let sign = match language {
        Language::English => -1,
        Language::Ukrainian => 1,
    };
    (evidence * sign) as f32 / maximum as f32
}

fn dictionary_prefixes(language: Language) -> &'static HashSet<&'static str> {
    static ENGLISH: OnceLock<HashSet<&'static str>> = OnceLock::new();
    static UKRAINIAN: OnceLock<HashSet<&'static str>> = OnceLock::new();
    let cache = match language {
        Language::English => &ENGLISH,
        Language::Ukrainian => &UKRAINIAN,
    };
    cache.get_or_init(|| {
        dictionary(language)
            .iter()
            .flat_map(|word| {
                word.char_indices()
                    .map(|(index, c)| &word[..index + c.len_utf8()])
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(physical: &str, layout: InputLayout) -> WordSample {
        WordSample::new(physical, format!("earlier context {physical}"), layout)
    }

    #[test]
    fn reusable_lowercase_preserves_unicode_context_and_case_replacements() {
        let mut scratch = String::new();
        for text in [
            "контекст",
            "КОНТЕКСТ",
            "Виправлення",
            "ІЇЄҐ",
            "abcXYZ",
            "ΟΣ",
            "ΟΣΑ",
            "İ",
            "ǅ",
            "K",
            "😀",
            "ѠԀ",
            "",
            "СЩТЕ",
        ] {
            assert_eq!(
                lowercase_reusing(text, &mut scratch),
                text.to_lowercase(),
                "{text}"
            );
        }
        let mut state = PartialWordState::default();
        let policy = AutoCorrectPolicy {
            triggers: vec![Trigger::for_source_layout(
                "cont*",
                TriggerAction::Correct,
                InputLayout::Ukrainian,
            )],
            ..AutoCorrectPolicy::default()
        };
        for map in [
            vec![('C', 'Ο'), ('O', 'Σ'), ('N', 'Ο'), ('T', 'Σ')],
            vec![('C', 'İ'), ('O', 'ǅ'), ('N', 'А'), ('T', 'Б')],
        ] {
            for physical in [
                "CONT", "cont", "CONTE", "CONT", " cont", "cont ", "conT", "cont,", "cont-",
            ] {
                let sample = WordSample::new(physical, physical, InputLayout::Ukrainian);
                assert_eq!(
                    state.evaluate(&sample, &policy, Some(&map)),
                    evaluate_partial(&sample, &policy, Some(&map)),
                    "{physical}"
                );
            }
        }
    }

    #[test]
    fn prepared_mapping_preserves_missing_duplicate_and_mutated_pairs() {
        let mut state = MappingState::default();
        let mut custom = vec![('a', 'α'), ('a', 'б'), ('x', 'І'), ('Σ', 'σ')];
        let builtin = crate::default_physical_mapping();
        for round in 0..4 {
            for mapping in [
                None,
                Some(&[][..]),
                Some(builtin.as_slice()),
                Some(custom.as_slice()),
            ] {
                let table = state.prepare(mapping);
                for byte in 0..128u8 {
                    let physical = (byte as char).to_string();
                    let expected = to_ukrainian(&physical, mapping);
                    assert_eq!(table[byte as usize].to_string(), expected, "{byte}/{round}");
                }
            }
            custom[0].1 = if round % 2 == 0 { 'в' } else { 'α' };
            custom.reverse();
        }
    }

    #[test]
    fn streaming_decisions_equal_reference_under_adversarial_state_reuse() {
        let mut state = PartialWordState::default();
        let mut policy = AutoCorrectPolicy::default();
        let mut map = crate::default_physical_mapping();
        let alphabet = b"contdbghfxyzABC [];'.,/?123";
        let mut seed = 42u64;
        let mut physical = String::from("cont");
        for i in 0..2000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            match i % 13 {
                0 => physical = "dbghfdktyyz".into(),
                1 => physical = "continue".into(),
                2..=4 => {
                    physical.pop();
                }
                _ => physical.push(alphabet[(seed >> 32) as usize % alphabet.len()] as char),
            }
            policy.min_word_length = if i % 17 == 0 { 9 } else { 4 };
            policy.exceptions = if i % 19 == 0 {
                vec!["CONT".into(), "виправлення".into()]
            } else {
                vec![]
            };
            policy.triggers = if i % 7 == 0 {
                vec![Trigger::for_source_layout(
                    "cont*",
                    TriggerAction::Correct,
                    InputLayout::Ukrainian,
                )]
            } else {
                vec![]
            };
            map[0].1 = if i % 2 == 0 { 'й' } else { 'І' };
            for layout in [InputLayout::English, InputLayout::Ukrainian] {
                for mapping in [None, Some(map.as_slice())] {
                    let sample = WordSample::new(&physical, &physical, layout);
                    assert_eq!(
                        state.evaluate(&sample, &policy, mapping),
                        evaluate_partial(&sample, &policy, mapping),
                        "{physical} {layout:?} {i}"
                    );
                }
            }
            if i % 31 == 0 {
                state.reset();
            }
        }
    }

    #[test]
    fn incremental_scores_equal_reference_through_edits_and_language_changes() {
        let mut state = PrefixScoreState::default();
        for text in [
            "",
            "cont",
            "continu",
            "cont",
            "context",
            "випр",
            "виправлення",
            "випр",
            "ВИПР",
            "ΟΣ",
            "İ",
            "a",
            "",
            "^abc",
        ] {
            for language in [Language::English, Language::Ukrainian] {
                assert_eq!(
                    state.score(language, text).to_bits(),
                    prefix_score(language, text).to_bits(),
                    "{text}"
                );
            }
        }
        for length in [63, 64, 65, 64, 2] {
            let text = "x".repeat(length);
            assert_eq!(
                state.score(Language::English, &text).to_bits(),
                prefix_score(Language::English, &text).to_bits()
            );
        }
        state.reset();
        assert_eq!(
            state.score(Language::Ukrainian, "виправ").to_bits(),
            prefix_score(Language::Ukrainian, "виправ").to_bits()
        );
    }

    #[test]
    fn incremental_evaluation_rechecks_policy_case_layout_and_mapping() {
        let mut state = PartialWordState::default();
        let policies = [
            AutoCorrectPolicy::default(),
            AutoCorrectPolicy {
                min_word_length: 8,
                ..AutoCorrectPolicy::default()
            },
            AutoCorrectPolicy {
                exceptions: vec!["dbghfdktyyz".into(), "сщте".into()],
                ..AutoCorrectPolicy::default()
            },
            AutoCorrectPolicy {
                triggers: vec![Trigger::for_source_layout(
                    "cont*",
                    TriggerAction::Keep,
                    InputLayout::Ukrainian,
                )],
                ..AutoCorrectPolicy::default()
            },
        ];
        let changed = [('c', 'а'), ('o', 'б'), ('n', 'в'), ('t', 'г')];
        for physical in [
            "dbgh", "dbghf", "dbghfdk", "dbgh", "dbgx", "cont", "CONT", "cont?", "conn", "con",
            "cont", "rehn",
        ] {
            for layout in [InputLayout::English, InputLayout::Ukrainian] {
                for policy in &policies {
                    for mapping in [None, Some(changed.as_slice())] {
                        let sample = sample(physical, layout);
                        assert_eq!(
                            state.evaluate(&sample, policy, mapping),
                            evaluate_partial(&sample, policy, mapping),
                            "{physical}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn partial_words_correct_before_space_in_both_directions() {
        for (physical, layout, source, target) in [
            ("cont", InputLayout::Ukrainian, "сщте", "cont"),
            ("Cont", InputLayout::Ukrainian, "Сщте", "Cont"),
            ("CONT", InputLayout::Ukrainian, "СЩТЕ", "CONT"),
            ("conn", InputLayout::Ukrainian, "сщтт", "conn"),
            ("ghbd", InputLayout::English, "ghbd", "прив"),
            ("name", InputLayout::Ukrainian, "тфьу", "name"),
        ] {
            let AutoDecision::Correct(correction) = evaluate_partial(
                &sample(physical, layout),
                &AutoCorrectPolicy::default(),
                None,
            ) else {
                panic!("expected early correction of {source}");
            };
            assert_eq!(correction.expected_source, source);
            assert_eq!(correction.replacement, target);
        }
    }

    #[test]
    fn interior_rule_corrects_vypravlennia_before_the_word_boundary() {
        let policy = AutoCorrectPolicy::default();
        for length in 1..=4 {
            let physical = &"dbghfdktyyz"[..length];
            let decision = evaluate_partial(&sample(physical, InputLayout::English), &policy, None);
            if length < 4 {
                assert_eq!(decision, AutoDecision::Continue);
            } else {
                let AutoDecision::Correct(correction) = decision else {
                    panic!(
                        "expected dbgh → випр; scores: source={}, target={}",
                        prefix_score(Language::English, physical),
                        prefix_score(Language::Ukrainian, "випр")
                    );
                };
                assert_eq!(correction.expected_source, "dbgh");
                assert_eq!(correction.replacement, "випр");
            }
        }
    }

    #[test]
    fn interior_rules_preserve_user_exceptions_and_ambiguous_native_prefixes() {
        for physical in ["rehn", "rehnquist", "debug", "database"] {
            assert_eq!(
                evaluate_partial(
                    &sample(physical, InputLayout::English),
                    &AutoCorrectPolicy::default(),
                    None
                ),
                AutoDecision::Continue
            );
        }
        for policy in [
            AutoCorrectPolicy {
                exceptions: vec!["dbghfdktyyz".into()],
                ..AutoCorrectPolicy::default()
            },
            AutoCorrectPolicy {
                triggers: vec![Trigger::for_source_layout(
                    "dbgh*",
                    TriggerAction::Keep,
                    InputLayout::English,
                )],
                ..AutoCorrectPolicy::default()
            },
            AutoCorrectPolicy {
                min_word_length: 6,
                ..AutoCorrectPolicy::default()
            },
        ] {
            assert_eq!(
                evaluate_partial(&sample("dbgh", InputLayout::English), &policy, None),
                AutoDecision::Continue
            );
        }
    }

    #[test]
    fn partial_words_wait_for_evidence_even_with_aggressive_or_forced_policy() {
        let policy = AutoCorrectPolicy {
            sensitivity: Sensitivity::Aggressive,
            min_word_length: 2,
            triggers: vec![Trigger::for_source_layout(
                "*",
                TriggerAction::Correct,
                InputLayout::Ukrainian,
            )],
            ..AutoCorrectPolicy::default()
        };
        for physical in ["c", "co", "con", "here", "cont_", "cont1", "cont."] {
            assert_eq!(
                evaluate_partial(&sample(physical, InputLayout::Ukrainian), &policy, None),
                AutoDecision::Continue,
                "{physical}"
            );
        }
        for physical in ["cont", "with", "test", "GHBD", "ghBd", "scho"] {
            assert_eq!(
                evaluate_partial(&sample(physical, InputLayout::English), &policy, None),
                AutoDecision::Continue,
                "{physical}"
            );
        }
        let policy = AutoCorrectPolicy {
            min_word_length: 6,
            ..policy
        };
        assert_eq!(
            evaluate_partial(&sample("cont", InputLayout::Ukrainian), &policy, None),
            AutoDecision::Continue
        );
    }

    #[test]
    fn partial_words_respect_exceptions_and_future_keep_rules() {
        let sample = sample("cont", InputLayout::Ukrainian);
        for exception in ["сщте", "СЩТЕШТГУ", "сщт"] {
            let policy = AutoCorrectPolicy {
                exceptions: vec![exception.into()],
                ..AutoCorrectPolicy::default()
            };
            assert_eq!(
                evaluate_partial(&sample, &policy, None),
                AutoDecision::Continue
            );
        }
        for pattern in ["cont", "continue", "cont*", "*inue", "*tin*"] {
            let policy = AutoCorrectPolicy {
                triggers: vec![Trigger::for_source_layout(
                    pattern,
                    TriggerAction::Keep,
                    InputLayout::Ukrainian,
                )],
                ..AutoCorrectPolicy::default()
            };
            assert_eq!(
                evaluate_partial(&sample, &policy, None),
                AutoDecision::Continue,
                "{pattern}"
            );
        }
    }

    #[test]
    fn partial_words_use_the_installed_map() {
        // A map under which the same keys produce a legitimate source prefix
        // must veto the default map's decision.
        let mapping = [('c', 'р'), ('o', 'у'), ('n', 'к'), ('t', 'у')];
        assert_eq!(
            evaluate_partial(
                &sample("cont", InputLayout::Ukrainian),
                &AutoCorrectPolicy::default(),
                Some(&mapping)
            ),
            AutoDecision::Continue
        );
        let changed = [('c', 'а'), ('o', 'б'), ('n', 'в'), ('t', 'г')];
        assert_eq!(
            evaluate_partial(
                &sample("cont", InputLayout::Ukrainian),
                &AutoCorrectPolicy::default(),
                Some(&changed)
            ),
            AutoDecision::Continue
        );
    }

    #[test]
    fn only_explicit_user_prefix_rules_apply_before_the_boundary() {
        for (pattern, corrects) in [
            ("abcd*", true),
            ("abcde", false),
            ("*abcd", false),
            ("*abcd*", false),
        ] {
            let policy = AutoCorrectPolicy {
                triggers: vec![Trigger::for_source_layout(
                    pattern,
                    TriggerAction::Correct,
                    InputLayout::Ukrainian,
                )],
                ..AutoCorrectPolicy::default()
            };
            assert_eq!(
                matches!(
                    evaluate_partial(&sample("abcde", InputLayout::Ukrainian), &policy, None),
                    AutoDecision::Correct(_)
                ),
                corrects,
                "{pattern}"
            );
        }
    }

    #[test]
    fn partial_rules_preserve_native_prefixes_in_the_recall_corpora() {
        let corpora = [
            (
                InputLayout::English,
                include_str!("../../fixtures/recall/english-words-v1.txt"),
            ),
            (
                InputLayout::Ukrainian,
                include_str!("../../fixtures/recall/ukrainian-words-v1.txt"),
            ),
        ];
        let mut checked = 0;
        for (layout, corpus) in corpora {
            for word in corpus
                .lines()
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
            {
                let physical = match layout {
                    InputLayout::English => word.to_owned(),
                    InputLayout::Ukrainian => convert(word, Direction::UkrainianToEnglish).text,
                };
                for (index, character) in physical.char_indices() {
                    let prefix = &physical[..index + character.len_utf8()];
                    assert_eq!(
                        evaluate_partial(
                            &sample(prefix, layout),
                            &AutoCorrectPolicy::default(),
                            None
                        ),
                        AutoDecision::Continue,
                        "{word}: {prefix}"
                    );
                    checked += 1;
                }
            }
        }
        println!("{checked} native recall-corpus prefix checks");
    }

    #[test]
    fn completing_an_early_corrected_word_does_not_switch_its_tail_back() {
        let mut corrected = 0;
        for (target_layout, corpus) in [
            (
                InputLayout::English,
                include_str!("../../fixtures/recall/english-words-v1.txt"),
            ),
            (
                InputLayout::Ukrainian,
                include_str!("../../fixtures/recall/ukrainian-words-v1.txt"),
            ),
        ] {
            for word in corpus
                .lines()
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
            {
                let physical = match target_layout {
                    InputLayout::English => word.to_owned(),
                    InputLayout::Ukrainian => convert(word, Direction::UkrainianToEnglish).text,
                };
                let mut layout = match target_layout {
                    InputLayout::English => InputLayout::Ukrainian,
                    InputLayout::Ukrainian => InputLayout::English,
                };
                let mut buffer = String::new();
                for c in physical.chars() {
                    buffer.push(c);
                    if let AutoDecision::Correct(_) = evaluate_partial(
                        &sample(&buffer, layout),
                        &AutoCorrectPolicy::default(),
                        None,
                    ) {
                        assert_ne!(layout, target_layout, "tail of {word}: {buffer}");
                        layout = target_layout;
                        buffer.clear();
                        corrected += 1;
                    }
                }
                if layout == target_layout && !buffer.is_empty() {
                    let policy = AutoCorrectPolicy {
                        triggers: crate::triggers::builtin_triggers(),
                        ..AutoCorrectPolicy::default()
                    };
                    assert!(
                        !matches!(
                            super::super::evaluate(
                                &WordSample::new(&buffer, format!("{buffer} "), layout),
                                &policy,
                                None,
                            ),
                            AutoDecision::Correct(_)
                        ),
                        "word boundary after early correction of {word}: {buffer}"
                    );
                }
            }
        }
        assert!(corrected > 100, "{corrected} early corrections");
        println!("{corrected} early-corrected words completed without a second switch");
    }
}
