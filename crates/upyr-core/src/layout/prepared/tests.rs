use super::*;
use crate::layout::{
    Conversion, append_converted, convert_with_mapping, default_physical_mapping, resolve_direction,
};

fn reference(input: &str, requested: Direction, pairs: &[(char, char)]) -> Conversion {
    let direction = resolve_direction(input, requested);
    let text: String = input
        .chars()
        .map(|c| {
            pairs
                .iter()
                .find_map(|&(a, b)| match direction {
                    Direction::EnglishToUkrainian if a == c => Some(b),
                    Direction::UkrainianToEnglish if b == c => Some(a),
                    _ => None,
                })
                .unwrap_or(c)
        })
        .collect();
    Conversion {
        changed: text != input,
        text,
        direction,
    }
}

#[test]
fn prepared_mapping_matches_linear_oracle_for_unicode_collisions_and_limits() {
    let mut maps = vec![vec![], default_physical_mapping()];
    for size in [1, 7, 8, 9, 127, 128, 255, 256, 257, 1024] {
        maps.push(
            (0..size)
                .map(|i| {
                    (
                        char::from_u32(if i % 3 == 0 { i % 128 } else { 0x400 + i % 80 }).unwrap(),
                        char::from_u32(if i % 2 == 0 {
                            0x1f300 + i % 200
                        } else {
                            i % 128
                        })
                        .unwrap(),
                    )
                })
                .collect(),
        );
    }
    maps.push(vec![
        ('a', 'a'),
        ('a', '🦀'),
        ('b', '🦀'),
        ('c', '🦀'),
        ('🦀', 'x'),
        ('🦀', 'y'),
        ('\0', '\0'),
        (' ', '_'),
        ('і', 'i'),
        ('і', 'I'),
    ]);
    let unicode: String = (0..=0x10ffff)
        .step_by(257)
        .filter_map(char::from_u32)
        .collect();
    for map in maps {
        let keys: String = map.iter().flat_map(|&(a, b)| [a, b]).collect();
        for input in [
            "",
            "q",
            "nj",
            "ghbdsn",
            "руддщ",
            "\0 a’b 🚀 σΣ ß ",
            &keys,
            &unicode,
        ] {
            for direction in [
                Direction::Smart,
                Direction::EnglishToUkrainian,
                Direction::UkrainianToEnglish,
            ] {
                let expected = reference(input, direction, &map);
                assert_eq!(convert_with_mapping(input, direction, &map), expected);
                let mut appended = String::from("префікс: ");
                assert_eq!(
                    append_converted(&mut appended, input, direction, Some(&map)),
                    expected.direction
                );
                assert_eq!(appended, format!("префікс: {}", expected.text));
            }
        }
    }
}

#[test]
fn cache_follows_in_place_edits_and_keeps_only_one_bounded_map() {
    CACHE.with(|cache| *cache.borrow_mut() = None);
    let mut map = default_physical_mapping();
    let address = map.as_ptr();
    for turn in 0..20 {
        map[0].1 = if turn % 2 == 0 { '🦀' } else { 'ґ' };
        map[1] = ('q', 'x'); // First forward pair still wins.
        for direction in [Direction::EnglishToUkrainian, Direction::UkrainianToEnglish] {
            assert_eq!(
                convert_with_mapping("q 🦀 ґ x", direction, &map),
                reference("q 🦀 ґ x", direction, &map)
            );
        }
        assert_eq!(map.as_ptr(), address);
        CACHE.with(|cache| assert_eq!(cache.borrow().as_ref().unwrap().source, map));
    }
    let large = vec![('q', 'z'); MAX_PAIRS + 1];
    assert_eq!(
        convert_with_mapping("q", Direction::EnglishToUkrainian, &large).text,
        "z"
    );
    CACHE.with(|cache| assert_eq!(cache.borrow().as_ref().unwrap().source, map));
    CACHE.with(|cache| *cache.borrow_mut() = None);
    for small in [&[][..], &map[..MIN_PAIRS - 1]] {
        assert_eq!(
            convert_with_mapping("q", Direction::EnglishToUkrainian, small),
            reference("q", Direction::EnglishToUkrainian, small)
        );
        CACHE.with(|cache| assert!(cache.borrow().is_none()));
    }
    assert_eq!(convert_with_mapping("", Direction::Smart, &map).text, "");
    CACHE.with(|cache| assert!(cache.borrow().is_none()));
}
