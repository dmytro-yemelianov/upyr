use serde::{Deserialize, Serialize};

/// The common US-QWERTY positions covered by the initial layout.
const ENGLISH_LOWER: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,./`\\";
const UKRAINIAN_LOWER: &str = "йцукенгшщзхїфівапролджєячсмитьбю.'ґ";
const ENGLISH_UPPER: &str = "QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>?~|";
const UKRAINIAN_UPPER: &str = "ЙЦУКЕНГШЩЗХЇФІВАПРОЛДЖЄЯЧСМИТЬБЮ,₴Ґ";

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
    let (source_lower, target_lower, source_upper, target_upper) = match direction {
        Direction::EnglishToUkrainian => (
            ENGLISH_LOWER,
            UKRAINIAN_LOWER,
            ENGLISH_UPPER,
            UKRAINIAN_UPPER,
        ),
        Direction::UkrainianToEnglish => (
            UKRAINIAN_LOWER,
            ENGLISH_LOWER,
            UKRAINIAN_UPPER,
            ENGLISH_UPPER,
        ),
        Direction::Smart => unreachable!("smart direction is resolved before conversion"),
    };

    let text: String = input
        .chars()
        .map(|character| {
            translate_character(character, source_lower, target_lower)
                .or_else(|| translate_character(character, source_upper, target_upper))
                .unwrap_or(character)
        })
        .collect();

    Conversion {
        changed: text != input,
        text,
        direction,
    }
}

/// Converts with a mapping generated from two installed layouts. Each pair is
/// `(english_character, ukrainian_character)` from the same physical key.
pub fn convert_with_mapping(
    input: &str,
    requested: Direction,
    mapping: &[(char, char)],
) -> Conversion {
    let direction = resolve_direction(input, requested);
    let text: String = input
        .chars()
        .map(|character| {
            mapping
                .iter()
                .find_map(|(english, ukrainian)| match direction {
                    Direction::EnglishToUkrainian if character == *english => Some(*ukrainian),
                    Direction::UkrainianToEnglish if character == *ukrainian => Some(*english),
                    Direction::Smart => unreachable!("smart direction is resolved before mapping"),
                    _ => None,
                })
                .unwrap_or(character)
        })
        .collect();

    Conversion {
        changed: text != input,
        text,
        direction,
    }
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

fn translate_character(character: char, source: &str, target: &str) -> Option<char> {
    source
        .chars()
        .position(|candidate| candidate == character)
        .and_then(|index| target.chars().nth(index))
}

fn is_ukrainian_letter(character: char) -> bool {
    UKRAINIAN_LOWER.contains(character) || UKRAINIAN_UPPER.contains(character)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn generated_inputs_round_trip_without_touching_unmapped_characters() {
        let alphabet: Vec<char> = format!("{ENGLISH_LOWER}{ENGLISH_UPPER} 0123456789\n🚀")
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
