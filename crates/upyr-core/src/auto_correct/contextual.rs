//! Bounded look-ahead for ambiguous short words. Uses actual visible text, so
//! deliberately switching layouts between words never reinterprets a whole run.
use super::*;
use std::borrow::Cow;

const MODEL: &[u8] = include_bytes!("../../assets/models/context.bgm");
const SHORT: &str = include_str!("../../assets/context-short-words.txt");
const HEADER: usize = 20;
const MAX_WORDS: usize = 3;
const MAX_CHARS: usize = 64;

fn u32_at(offset: usize) -> u32 {
    u32::from_le_bytes(MODEL[offset..offset + 4].try_into().unwrap())
}

fn word(index: usize) -> &'static str {
    let text = HEADER + (u32_at(8) as usize + 1) * 4;
    let start = u32_at(HEADER + index * 4) as usize;
    let end = u32_at(HEADER + (index + 1) * 4) as usize;
    std::str::from_utf8(&MODEL[text + start..text + end]).unwrap()
}

fn word_id(value: &str) -> Option<u32> {
    if value.len() > MAX_CHARS * 4 {
        return find_word_id(value);
    }
    WORD_IDS.with(|cache| cache.borrow_mut().get(value))
}

#[derive(Default)]
struct WordIds {
    entries: [Option<(Box<str>, Option<u32>)>; 16],
    next: usize,
}
impl WordIds {
    fn get(&mut self, value: &str) -> Option<u32> {
        if let Some((_, id)) = self
            .entries
            .iter()
            .flatten()
            .find(|(key, _)| key.as_ref() == value)
        {
            return *id;
        }
        let id = find_word_id(value);
        self.entries[self.next] = Some((value.into(), id));
        self.next = (self.next + 1) % self.entries.len();
        id
    }
}
thread_local! { static WORD_IDS: RefCell<WordIds> = RefCell::new(WordIds::default()); }

#[cfg(feature = "engine-bench")]
pub(super) fn reset_cache() {
    WORD_IDS.with(|cache| *cache.borrow_mut() = WordIds::default());
}

fn find_word_id(value: &str) -> Option<u32> {
    let mut low = 0usize;
    let mut high = u32_at(8) as usize;
    while low < high {
        let mid = low + (high - low) / 2;
        match word(mid).cmp(value) {
            std::cmp::Ordering::Less => low = mid + 1,
            std::cmp::Ordering::Greater => high = mid,
            std::cmp::Ordering::Equal => return Some(mid as u32),
        }
    }
    None
}

fn pair_count(left: &str, right: &str) -> u32 {
    let (Some(left), Some(right)) = (word_id(left), word_id(right)) else {
        return 0;
    };
    let key = (u64::from(left) << 32) | u64::from(right);
    let records = HEADER + (u32_at(8) as usize + 1) * 4 + u32_at(16) as usize;
    let mut low = 0usize;
    let mut high = u32_at(12) as usize;
    while low < high {
        let mid = low + (high - low) / 2;
        let offset = records + mid * 12;
        let stored = u64::from_le_bytes(MODEL[offset..offset + 8].try_into().unwrap());
        match stored.cmp(&key) {
            std::cmp::Ordering::Less => low = mid + 1,
            std::cmp::Ordering::Greater => high = mid,
            std::cmp::Ordering::Equal => return u32_at(offset + 8),
        }
    }
    0
}

fn layout(word: &str) -> Option<InputLayout> {
    if !word.is_empty() && word.chars().all(|c| c.is_ascii_alphabetic()) {
        Some(InputLayout::English)
    } else if !word.is_empty()
        && punto_trie::lowercase(word)
            .chars()
            .all(|c| "абвгґдеєжзиіїйклмнопрстуфхцчшщьюя".contains(c))
    {
        Some(InputLayout::Ukrainian)
    } else {
        None
    }
}

fn opposite(source: InputLayout) -> InputLayout {
    match source {
        InputLayout::English => InputLayout::Ukrainian,
        InputLayout::Ukrainian => InputLayout::English,
    }
}

fn direction(target: InputLayout) -> Direction {
    match target {
        InputLayout::English => Direction::UkrainianToEnglish,
        InputLayout::Ukrainian => Direction::EnglishToUkrainian,
    }
}

fn converted(word: &str, target: InputLayout, mapping: Option<&[(char, char)]>) -> String {
    match mapping {
        Some(mapping) => convert_with_mapping(word, direction(target), mapping).text,
        None => convert(word, direction(target)).text,
    }
}

fn is_short(word: &str, target: InputLayout) -> bool {
    let language = match target {
        InputLayout::English => "english",
        InputLayout::Ukrainian => "ukrainian",
    };
    let lower = punto_trie::lowercase(word);
    SHORT
        .lines()
        .find(|line| line.starts_with(language))
        .is_some_and(|line| line.split_whitespace().skip(1).any(|w| w == lower))
        && (lower != "i" || word == "I")
}

fn protected(
    word: &str,
    source: InputLayout,
    policy: &AutoCorrectPolicy,
    mapping: Option<&[(char, char)]>,
) -> bool {
    let lower = punto_trie::lowercase(word);
    let language = match source {
        InputLayout::English => Language::English,
        InputLayout::Ukrainian => Language::Ukrainian,
    };
    let physical = match source {
        InputLayout::English => Cow::Borrowed(lower.as_ref()),
        InputLayout::Ukrainian => Cow::Owned(converted(&lower, InputLayout::English, mapping)),
    };
    known(language, &lower)
        || is_user_exception(policy, &lower)
        || policy
            .triggers
            .iter()
            .any(|rule| rule.action == TriggerAction::Keep && rule.matches(&physical, source))
        || (source == InputLayout::English
            && (lower == "vs" || word.len() == 1 || looks_like_deliberate_latin_identifier(word)))
        || (word.chars().count() > 1 && word.chars().all(char::is_uppercase))
}

/// Whether an undecided short word is worth retaining until the next boundary.
/// Input must end in exactly one space. No edits or policy decisions are cached.
pub fn has_pending_context(
    visible: &str,
    policy: &AutoCorrectPolicy,
    mapping: Option<&[(char, char)]>,
) -> bool {
    if policy.min_word_length > 4 {
        return false;
    }
    let Some(body) = visible.strip_suffix(' ') else {
        return false;
    };
    let word = body.rsplit(' ').next().unwrap_or("");
    if word.chars().count() > 3 {
        return false;
    }
    let Some(source) = layout(word) else {
        return false;
    };
    !protected(word, source, policy, mapping)
        && is_short(
            &converted(word, opposite(source), mapping),
            opposite(source),
        )
}

/// Evaluate the last two or three *finished* visible words. A left-hand short
/// word must change, every adjacent target bigram needs corpus support, and
/// native words, user exceptions, Keep rules and identifiers win over context.
pub fn evaluate_context(
    visible: &str,
    policy: &AutoCorrectPolicy,
    mapping: Option<&[(char, char)]>,
) -> Option<AutoCorrection> {
    if policy.min_word_length > 4 {
        return None;
    }
    let boundary = if visible.ends_with(' ') { " " } else { "" };
    let body = visible.strip_suffix(' ').unwrap_or(visible);
    if !body.contains(' ') {
        return None;
    }
    let mut words = [""; MAX_WORDS];
    let mut count = 0;
    for word in body.rsplit(' ').take(MAX_WORDS) {
        count += 1;
        words[MAX_WORDS - count] = word;
    }
    let words = &words[MAX_WORDS - count..];
    for start in 0..words.len().saturating_sub(1) {
        let slice = &words[start..];
        if slice[0].chars().count() > 3 {
            continue;
        }
        let Some(source) = layout(slice[0]) else {
            continue;
        };
        let target = opposite(source);
        let mut replacements: [Cow<'_, str>; MAX_WORDS] =
            std::array::from_fn(|_| Cow::Borrowed(""));
        let mut replacement_count = 0;
        for (i, word) in slice.iter().enumerate() {
            let Some(actual) = layout(word) else { break };
            let changed = actual != target;
            if changed && protected(word, actual, policy, mapping) {
                break;
            }
            let replacement = if changed {
                Cow::Owned(converted(word, target, mapping))
            } else {
                Cow::Borrowed(*word)
            };
            if layout(&replacement) != Some(target)
                || (i + 1 < slice.len() && !is_short(&replacement, target))
                || (changed && word.chars().count() != replacement.chars().count())
            {
                break;
            }
            replacements[i] = replacement;
            replacement_count += 1;
        }
        if replacement_count != slice.len() {
            continue;
        }
        let minimum = match policy.sensitivity {
            Sensitivity::Conservative => 16,
            _ => 8,
        };
        let replacements = &replacements[..replacement_count];
        let mut lower: [Cow<'_, str>; MAX_WORDS] = std::array::from_fn(|_| Cow::Borrowed(""));
        for (index, word) in replacements.iter().enumerate() {
            lower[index] = punto_trie::lowercase(word);
        }
        if !lower[..replacement_count]
            .windows(2)
            .enumerate()
            .all(|(i, pair)| {
                let count = pair_count(&pair[0], &pair[1]);
                let original = pair_count(
                    &punto_trie::lowercase(slice[i]),
                    &punto_trie::lowercase(slice[i + 1]),
                );
                count >= minimum && count >= original.saturating_add(1).saturating_mul(16)
            })
        {
            continue;
        }
        let expected_source = joined_with_boundary(slice, boundary);
        let replacement = joined_with_boundary(replacements, boundary);
        if !contextual_edit_is_bounded(&expected_source, &replacement, mapping) {
            continue;
        }
        return Some(AutoCorrection {
            expected_source,
            replacement,
            direction: direction(target),
        });
    }
    None
}

fn joined_with_boundary<T: AsRef<str>>(words: &[T], boundary: &str) -> String {
    let capacity = words.iter().map(|w| w.as_ref().len()).sum::<usize>()
        + words.len().saturating_sub(1)
        + boundary.len();
    let mut result = String::with_capacity(capacity);
    for (index, word) in words.iter().enumerate() {
        if index != 0 {
            result.push(' ');
        }
        result.push_str(word.as_ref());
    }
    result.push_str(boundary);
    result
}

/// Shared with the terminal planner: exact positional conversions of at most
/// two short words followed by one anchor, preserving all spaces and lengths.
/// Unchanged words are allowed only in the target script. This is structural
/// validation; callers must still require language evidence and input guards.
pub fn contextual_edit_is_bounded(
    source: &str,
    target: &str,
    mapping: Option<&[(char, char)]>,
) -> bool {
    if source.ends_with(' ') != target.ends_with(' ') || source.chars().count() > MAX_CHARS {
        return false;
    }
    fn words(text: &str) -> Option<([&str; MAX_WORDS], usize)> {
        let mut words = [""; MAX_WORDS];
        let mut count = 0;
        for word in text.strip_suffix(' ').unwrap_or(text).split(' ') {
            *words.get_mut(count)? = word;
            count += 1;
        }
        Some((words, count))
    }
    let (Some((source, source_count)), Some((target, target_count))) =
        (words(source), words(target))
    else {
        return false;
    };
    let source = &source[..source_count];
    let target = &target[..target_count];
    if !(2..=MAX_WORDS).contains(&source.len())
        || source.len() != target.len()
        || source[0] == target[0]
    {
        return false;
    }
    let Some(target_layout) = layout(target[0]) else {
        return false;
    };
    source
        .iter()
        .zip(target)
        .enumerate()
        .all(|(i, (before, after))| {
            layout(before).is_some()
                && layout(after) == Some(target_layout)
                && before.chars().count() == after.chars().count()
                && (i + 1 == source.len() || before.chars().count() <= 3)
                && (before == after || converted(before, target_layout, mapping) == *after)
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn bounded_word_id_cache_preserves_hits_misses_and_evictions() {
        let mut cache = super::WordIds::default();
        for round in 0..3 {
            for index in (0..super::u32_at(8) as usize).step_by(127) {
                let value = super::word(index);
                assert_eq!(cache.get(value), super::find_word_id(value));
                assert_eq!(cache.get(value), Some(index as u32));
            }
            for missing in ["", "\0", "🤖", "missingwordxxxxxxxx", "ІЇЄҐ"] {
                assert_eq!(cache.get(missing), super::find_word_id(missing), "{round}");
                assert_eq!(cache.get(missing), super::find_word_id(missing));
            }
        }
    }

    use super::*;

    #[test]
    fn packed_model_is_sorted_and_complete() {
        assert_eq!(&MODEL[..8], b"UPYRBG1\0");
        let words = u32_at(8) as usize;
        let pairs = u32_at(12) as usize;
        let records = HEADER + (words + 1) * 4 + u32_at(16) as usize;
        assert_eq!(MODEL.len(), records + pairs * 12);
        for i in 0..words {
            assert_eq!(word_id(word(i)), Some(i as u32));
        }
        for i in 1..words {
            assert!(word(i - 1) < word(i));
        }
        let mut previous = None;
        for i in 0..pairs {
            let offset = records + i * 12;
            let key = u64::from_le_bytes(MODEL[offset..offset + 8].try_into().unwrap());
            assert!(previous.is_none_or(|p| p < key));
            previous = Some(key);
            assert_eq!(
                pair_count(word((key >> 32) as usize), word(key as u32 as usize)),
                u32_at(offset + 8)
            );
        }
        assert!(pair_count("i", "have") >= 16);
        assert!(pair_count("то", "ми") >= 16);
    }

    #[test]
    fn short_words_wait_for_supported_context() {
        let policy = AutoCorrectPolicy::default();
        for (source, target) in [
            ("nj vb ", "то ми "),
            ("Ш рфму ", "I have "),
            ("Ш have ", "I have "),
            ("Ш have", "I have"),
        ] {
            let correction =
                evaluate_context(source, &policy, None).unwrap_or_else(|| panic!("{source}"));
            assert_eq!(correction.expected_source, source);
            assert_eq!(correction.replacement, target);
        }
        for source in [
            "Ш ",
            "nj ",
            "vs you ",
            "NJ VB ",
            "то ми ",
            "I have ",
            "ш have ",
            "f have ",
            "Ш  have ",
            "Ш\nhave ",
            "Ш have! ",
            "SQL have ",
        ] {
            assert!(
                evaluate_context(source, &policy, None).is_none(),
                "{source}"
            );
        }
        for source in ["Ш ", "nj ", "vb "] {
            assert!(has_pending_context(source, &policy, None));
        }
        for source in ["vs ", "NJ ", "I ", "ми ", "Ш  "] {
            assert!(!has_pending_context(source, &policy, None));
        }
    }

    #[test]
    fn policies_and_layout_maps_are_checked_for_every_context_edit() {
        let mut policy = AutoCorrectPolicy::default();
        policy.exceptions.push("Ш".into());
        assert!(evaluate_context("Ш have ", &policy, None).is_none());
        policy.exceptions.clear();
        policy
            .triggers
            .push(crate::parse_trigger("Ш keep").unwrap().unwrap());
        assert!(evaluate_context("Ш have ", &policy, None).is_none());
        policy.triggers.clear();
        policy.min_word_length = 5;
        assert!(evaluate_context("nj vb ", &policy, None).is_none());
        assert!(!has_pending_context("Ш ", &policy, None));
        let mapping = [('n', 'т'), ('j', 'і'), ('v', 'м'), ('b', 'и')];
        assert!(
            evaluate_context("nj vb ", &AutoCorrectPolicy::default(), Some(&mapping)).is_none()
        );
    }

    #[test]
    #[ignore = "requires generated held-out sentences; UPYR_CONTEXT_HOLDOUT supplies the path"]
    fn context_external_native_pairs() {
        let path = std::env::var("UPYR_CONTEXT_HOLDOUT").expect("UPYR_CONTEXT_HOLDOUT");
        let input = std::fs::read_to_string(path).unwrap();
        let policy = AutoCorrectPolicy::default();
        let mut checks = 0;
        let mut failures = Vec::new();
        let mut eligible = 0;
        let mut whole_pair_recovered = 0;
        let mut short_only_recovered = 0;
        for line in input.lines() {
            let (language, pair) = line.split_once('\t').unwrap();
            checks += 1;
            if let Some(edit) = evaluate_context(&format!("{pair} "), &policy, None) {
                failures.push(format!("{pair:?}: {edit:?}"));
            }
            let intended = if language == "english" {
                InputLayout::English
            } else {
                InputLayout::Ukrainian
            };
            let (left, right) = pair.split_once(' ').unwrap();
            if is_short(left, intended) && layout(right) == Some(intended) {
                eligible += 1;
                let wrong = converted(pair, opposite(intended), None);
                let mixed = format!("{} {right}", converted(left, opposite(intended), None));
                for (input, count) in [
                    (&wrong, &mut whole_pair_recovered),
                    (&mixed, &mut short_only_recovered),
                ] {
                    if let Some(edit) = evaluate_context(&format!("{input} "), &policy, None) {
                        assert_eq!(edit.replacement, format!("{pair} "), "{input}");
                        *count += 1;
                    }
                }
            }
        }
        println!(
            "context native pairs: {checks}, false corrections: {}",
            failures.len()
        );
        println!(
            "context short-word pairs: {eligible}, whole-pair recovered: {whole_pair_recovered}, only-short-word recovered: {short_only_recovered}"
        );
        assert!(
            failures.is_empty(),
            "{}",
            failures
                .iter()
                .take(30)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
