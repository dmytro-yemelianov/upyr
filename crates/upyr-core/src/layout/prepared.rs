//! One bounded, content-keyed custom mapping per calling thread.
use std::cell::RefCell;

use super::Direction;

// Tiny maps are cheaper to scan. Arbitrary public maps must not leave an
// unbounded persistent allocation; those keep the original linear converter.
const MIN_PAIRS: usize = 8;
const MAX_PAIRS: usize = 256;

pub(super) struct Index {
    ascii: [char; 128],
    unicode: Vec<(char, char)>,
}

impl Index {
    fn new(pairs: &[(char, char)], reverse: bool) -> Self {
        let pair = |(a, b)| if reverse { (b, a) } else { (a, b) };
        let mut ascii = std::array::from_fn(|i| i as u8 as char);
        for (from, to) in pairs.iter().copied().rev().map(pair) {
            if from.is_ascii() {
                ascii[from as usize] = to;
            }
        }
        let mut unicode: Vec<_> = pairs
            .iter()
            .copied()
            .map(pair)
            .filter(|(from, _)| !from.is_ascii())
            .collect();
        // Stable sorting preserves the original first-pair-wins rule.
        unicode.sort_by_key(|&(from, _)| from);
        unicode.dedup_by_key(|(from, _)| *from);
        Self { ascii, unicode }
    }

    pub(super) fn translate(&self, character: char) -> char {
        if character.is_ascii() {
            self.ascii[character as usize]
        } else {
            self.unicode
                .binary_search_by_key(&character, |&(from, _)| from)
                .map_or(character, |index| self.unicode[index].1)
        }
    }
}

pub(super) struct PreparedMapping {
    source: Vec<(char, char)>,
    forward: Index,
    reverse: Index,
}

impl PreparedMapping {
    fn new(pairs: &[(char, char)]) -> Self {
        Self {
            source: pairs.to_vec(),
            forward: Index::new(pairs, false),
            reverse: Index::new(pairs, true),
        }
    }

    pub(super) fn index(&self, direction: Direction) -> &Index {
        match direction {
            Direction::EnglishToUkrainian => &self.forward,
            Direction::UkrainianToEnglish => &self.reverse,
            Direction::Smart => unreachable!("direction is resolved before preparing conversion"),
        }
    }
}

thread_local! {
    static CACHE: RefCell<Option<Box<PreparedMapping>>> = const { RefCell::new(None) };
}

pub(super) fn with_prepared<R>(
    pairs: &[(char, char)],
    convert: impl FnOnce(Option<&PreparedMapping>) -> R,
) -> R {
    if !(MIN_PAIRS..=MAX_PAIRS).contains(&pairs.len()) {
        return convert(None);
    }
    #[cfg(feature = "engine-bench")]
    if benchmark::linear_enabled() {
        return convert(None);
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        // A caller may edit a slice in place; pointer identity is insufficient.
        if cache.as_ref().is_none_or(|cached| cached.source != pairs) {
            *cache = Some(Box::new(PreparedMapping::new(pairs)));
        }
        convert(cache.as_deref())
    })
}

#[cfg(feature = "engine-bench")]
pub mod benchmark {
    use super::*;
    use std::cell::Cell;
    thread_local! { static LINEAR: Cell<bool> = const { Cell::new(false) }; }
    pub(super) fn linear_enabled() -> bool {
        LINEAR.with(Cell::get)
    }
    /// Offline comparison only. Excluded from default production builds.
    pub fn set_linear_mapping(linear: bool) {
        LINEAR.with(|mode| mode.set(linear));
    }
    pub fn reset_mapping_cache() {
        CACHE.with(|cache| *cache.borrow_mut() = None);
    }
    pub fn mapping_heap_bytes() -> usize {
        CACHE.with(|cache| {
            cache.borrow().as_ref().map_or(0, |map| {
                std::mem::size_of::<PreparedMapping>()
                    + (map.source.capacity()
                        + map.forward.unicode.capacity()
                        + map.reverse.unicode.capacity())
                        * std::mem::size_of::<(char, char)>()
            })
        })
    }
}

#[cfg(test)]
mod tests;
