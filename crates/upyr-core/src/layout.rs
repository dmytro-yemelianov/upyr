//! Platform-neutral English-Ukrainian physical-key conversion.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

mod prepared;
#[cfg(feature = "engine-bench")]
#[doc(hidden)]
pub use prepared::benchmark;

/// The common US-QWERTY positions covered by the initial layout.
const ENGLISH_LOWER: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,./`\\";
const UKRAINIAN_LOWER: &str = "йцукенгшщзхїфівапролджєячсмитьбю.'ґ";
const ENGLISH_UPPER: &str = "QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>?~|";
const UKRAINIAN_UPPER: &str = "ЙЦУКЕНГШЩЗХЇФІВАПРОЛДЖЄЯЧСМИТЬБЮ,₴Ґ";
/// Shifted number-row keys whose symbol differs on the standard Ukrainian
/// layout (Shift+2, 3, 4, 6, 7). The other digit-row symbols are shared.
const ENGLISH_SHIFTED_DIGITS: &str = "@#$^&";
const UKRAINIAN_SHIFTED_DIGITS: &str = "\"№;:?";

fn builtin_pairs() -> impl Iterator<Item = (char, char)> {
    ENGLISH_LOWER
        .chars()
        .zip(UKRAINIAN_LOWER.chars())
        .chain(ENGLISH_UPPER.chars().zip(UKRAINIAN_UPPER.chars()))
        .chain(
            ENGLISH_SHIFTED_DIGITS
                .chars()
                .zip(UKRAINIAN_SHIFTED_DIGITS.chars()),
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    Smart,
    EnglishToUkrainian,
    UkrainianToEnglish,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    pub text: String,
    pub direction: Direction,
    pub changed: bool,
}

/// Returns the built-in US-QWERTY to Ukrainian physical-key mapping.
///
/// Consumers can use this as a baseline and replace individual pairs for an
/// installed or browser-supplied layout profile before calling
/// [`convert_with_mapping`].
pub fn default_physical_mapping() -> Vec<(char, char)> {
    builtin_pairs().collect()
}

/// Converts text as if the same physical keys had been typed in the other layout.
pub fn convert(input: &str, requested: Direction) -> Conversion {
    debug_assert_eq!(
        ENGLISH_LOWER.chars().count(),
        UKRAINIAN_LOWER.chars().count()
    );
    debug_assert_eq!(
        ENGLISH_UPPER.chars().count(),
        UKRAINIAN_UPPER.chars().count()
    );

    let direction = resolve_direction(input, requested);
    let table = builtin_mapping(direction);
    let text: String = input
        .chars()
        .map(|character| table.get(&character).copied().unwrap_or(character))
        .collect();

    Conversion {
        changed: text != input,
        text,
        direction,
    }
}

/// Precomputed built-in mapping table for a resolved direction, covering both
/// letter-case ranges. Replaces the previous per-character linear scans over the
/// layout strings so conversion is O(1) per character.
fn builtin_mapping(direction: Direction) -> &'static HashMap<char, char> {
    static ENGLISH_TO_UKRAINIAN: OnceLock<HashMap<char, char>> = OnceLock::new();
    static UKRAINIAN_TO_ENGLISH: OnceLock<HashMap<char, char>> = OnceLock::new();
    match direction {
        Direction::EnglishToUkrainian => {
            ENGLISH_TO_UKRAINIAN.get_or_init(|| build_builtin_mapping(direction))
        }
        Direction::UkrainianToEnglish => {
            UKRAINIAN_TO_ENGLISH.get_or_init(|| build_builtin_mapping(direction))
        }
        Direction::Smart => unreachable!("smart direction is resolved before conversion"),
    }
}

fn build_builtin_mapping(direction: Direction) -> HashMap<char, char> {
    let mut mapping = HashMap::new();
    for (english, ukrainian) in builtin_pairs() {
        let (from, to) = match direction {
            Direction::EnglishToUkrainian => (english, ukrainian),
            Direction::UkrainianToEnglish => (ukrainian, english),
            Direction::Smart => unreachable!("smart direction is resolved before conversion"),
        };
        // First position wins, matching the previous left-to-right scan.
        mapping.entry(from).or_insert(to);
    }
    mapping
}

/// Converts with a mapping generated from two installed layouts. Each pair is
/// `(english_character, ukrainian_character)` from the same physical key.
pub fn convert_with_mapping(
    input: &str,
    requested: Direction,
    mapping: &[(char, char)],
) -> Conversion {
    let direction = resolve_direction(input, requested);
    if input.is_empty() {
        return Conversion {
            text: String::new(),
            direction,
            changed: false,
        };
    }
    let text: String = prepared::with_prepared(mapping, |prepared| {
        if let Some(prepared) = prepared {
            let index = prepared.index(direction);
            return input
                .chars()
                .map(|character| index.translate(character))
                .collect();
        }
        input
            .chars()
            .map(|character| {
                mapping
                    .iter()
                    .find_map(|(english, ukrainian)| match direction {
                        Direction::EnglishToUkrainian if character == *english => Some(*ukrainian),
                        Direction::UkrainianToEnglish if character == *ukrainian => Some(*english),
                        Direction::Smart => {
                            unreachable!("smart direction is resolved before mapping")
                        }
                        _ => None,
                    })
                    .unwrap_or(character)
            })
            .collect()
    });

    Conversion {
        changed: text != input,
        text,
        direction,
    }
}

/// Appends converted text to an existing buffer without an intermediate string.
/// Smart direction is resolved from `input` alone and returned to the caller.
/// `None` uses the built-in mapping; `Some(&[])` leaves every character intact.
/// Like [`convert_with_mapping`], the first matching custom pair wins.
pub fn append_converted(
    output: &mut String,
    input: &str,
    requested: Direction,
    mapping: Option<&[(char, char)]>,
) -> Direction {
    let direction = resolve_direction(input, requested);
    if let Some(mapping) = mapping {
        if input.is_empty() {
            return direction;
        }
        prepared::with_prepared(mapping, |prepared| {
            if let Some(prepared) = prepared {
                let index = prepared.index(direction);
                output.extend(input.chars().map(|character| index.translate(character)));
                return;
            }
            output.extend(input.chars().map(|character| {
                mapping
                    .iter()
                    .find_map(|&(english, ukrainian)| match direction {
                        Direction::EnglishToUkrainian if character == english => Some(ukrainian),
                        Direction::UkrainianToEnglish if character == ukrainian => Some(english),
                        _ => None,
                    })
                    .unwrap_or(character)
            }));
        });
    } else {
        let table = builtin_mapping(direction);
        output.extend(
            input
                .chars()
                .map(|character| table.get(&character).copied().unwrap_or(character)),
        );
    }
    direction
}

/// Resolves smart mode using the dominant script in the selected text.
pub fn resolve_direction(input: &str, requested: Direction) -> Direction {
    if requested != Direction::Smart {
        return requested;
    }

    let latin_count = input
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .count();
    let ukrainian_count = input
        .chars()
        .filter(|character| is_ukrainian_letter(*character))
        .count();

    if ukrainian_count > latin_count {
        Direction::UkrainianToEnglish
    } else {
        Direction::EnglishToUkrainian
    }
}

fn is_ukrainian_letter(character: char) -> bool {
    character.is_alphabetic()
        && (UKRAINIAN_LOWER.contains(character) || UKRAINIAN_UPPER.contains(character))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appending_matches_independent_conversion_in_both_directions() {
        let builtin = default_physical_mapping();
        let custom = [('a', '🦀'), ('a', 'x'), ('b', '🦀'), (' ', '_'), ('і', 'i')];
        let unicode: String = (0..=0x10ffff)
            .step_by(257)
            .filter_map(char::from_u32)
            .collect();
        let text = format!(
            "{ENGLISH_LOWER}{ENGLISH_UPPER}{ENGLISH_SHIFTED_DIGITS}\n{UKRAINIAN_LOWER}{UKRAINIAN_UPPER}{UKRAINIAN_SHIFTED_DIGITS} a’b σΣ ß 🚀 {unicode}"
        );
        for input in ["", "ghbdsn", "руддщ", "a....", "ф....", text.as_str()] {
            for requested in [
                Direction::Smart,
                Direction::EnglishToUkrainian,
                Direction::UkrainianToEnglish,
            ] {
                for mapping in [
                    None,
                    Some(&[][..]),
                    Some(builtin.as_slice()),
                    Some(custom.as_slice()),
                ] {
                    let expected = match mapping {
                        Some(pairs) => convert_with_mapping(input, requested, pairs),
                        None => convert(input, requested),
                    };
                    for prefix in ["", "не враховуй цей префікс у Smart: "] {
                        let mut output = prefix.to_owned();
                        let direction = append_converted(&mut output, input, requested, mapping);
                        assert_eq!(output, format!("{prefix}{}", expected.text));
                        assert_eq!(direction, expected.direction);
                    }
                }
            }
        }
    }

    #[test]
    fn converts_mistyped_ukrainian() {
        let result = convert("ghbdsn? Erhf]yf!", Direction::EnglishToUkrainian);

        assert_eq!(result.text, "привіт, Україна!");
        assert!(result.changed);
    }

    #[test]
    fn converts_mistyped_english() {
        let result = convert("руддщ цщкдв", Direction::UkrainianToEnglish);

        assert_eq!(result.text, "hello world");
    }

    #[test]
    fn smart_mode_uses_dominant_script() {
        assert_eq!(
            resolve_direction("ghbdsn", Direction::Smart),
            Direction::EnglishToUkrainian
        );
        assert_eq!(
            resolve_direction("руддщ", Direction::Smart),
            Direction::UkrainianToEnglish
        );
    }

    #[test]
    fn punctuation_does_not_outvote_letters_in_smart_mode() {
        assert_eq!(
            resolve_direction("a....", Direction::Smart),
            Direction::EnglishToUkrainian
        );
        assert_eq!(convert("a....", Direction::Smart).text, "фюююю");
        assert_eq!(
            resolve_direction("ф....", Direction::Smart),
            Direction::UkrainianToEnglish
        );
        assert_eq!(
            resolve_direction("test,,'₴", Direction::Smart),
            Direction::EnglishToUkrainian
        );
    }

    #[test]
    fn converts_the_shifted_number_row_of_the_standard_ukrainian_layout() {
        assert_eq!(
            convert("@ghbdsn@ &", Direction::EnglishToUkrainian).text,
            "\"привіт\" ?"
        );
        assert_eq!(
            convert("#1 $ ^", Direction::EnglishToUkrainian).text,
            "№1 ; :"
        );
        assert_eq!(
            convert("\"руддщ\"?", Direction::UkrainianToEnglish).text,
            "@hello@&"
        );
    }

    #[test]
    fn preserves_unmapped_characters() {
        let result = convert("test 123 🚀", Direction::EnglishToUkrainian);

        assert_eq!(result.text, "еуіе 123 🚀");
    }

    #[test]
    fn mappings_round_trip() {
        let ukrainian = convert(ENGLISH_LOWER, Direction::EnglishToUkrainian);
        let english = convert(&ukrainian.text, Direction::UkrainianToEnglish);

        assert_eq!(english.text, ENGLISH_LOWER);
    }

    #[test]
    fn exposes_the_complete_default_physical_mapping() {
        let mapping = default_physical_mapping();

        assert_eq!(
            mapping.len(),
            ENGLISH_LOWER.chars().count() * 2 + ENGLISH_SHIFTED_DIGITS.chars().count()
        );
        assert!(mapping.contains(&('q', 'й')));
        assert!(mapping.contains(&('&', '?')));
        assert!(mapping.contains(&(']', 'ї')));
        assert!(mapping.contains(&('\\', 'ґ')));
        assert!(mapping.contains(&('Q', 'Й')));
    }

    #[test]
    fn generated_inputs_round_trip_without_touching_unmapped_characters() {
        let alphabet: Vec<char> =
            format!("{ENGLISH_LOWER}{ENGLISH_UPPER}{ENGLISH_SHIFTED_DIGITS} 0123456789\n🚀")
                .chars()
                .collect();
        let mut state = 0x4d59_5df4_d0f3_3173_u64;

        for _ in 0..2_000 {
            let length = 1 + usize::try_from(state % 80).unwrap();
            let input: String = (0..length)
                .map(|_| {
                    // A fixed LCG gives broad property-style coverage without a
                    // nondeterministic test dependency.
                    state = state
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    alphabet[usize::try_from(state % alphabet.len() as u64).unwrap()]
                })
                .collect();
            let ukrainian = convert(&input, Direction::EnglishToUkrainian);
            let round_trip = convert(&ukrainian.text, Direction::UkrainianToEnglish);

            assert_eq!(round_trip.text, input);
        }
    }

    #[test]
    fn converts_with_generated_physical_mapping() {
        let mapping = [('a', 'ф'), ('A', 'Ф'), ('1', '!')];

        let ukrainian = convert_with_mapping("aA1", Direction::EnglishToUkrainian, &mapping);
        let english =
            convert_with_mapping(&ukrainian.text, Direction::UkrainianToEnglish, &mapping);

        assert_eq!(ukrainian.text, "фФ!");
        assert_eq!(english.text, "aA1");
    }
}
