//! Read-only physical-key prefix language masks, generated from pinned lexicons.
//! Matching tails are shared in an acyclic automaton; lookup allocates nothing.

const MODEL: &[u8] = include_bytes!("../../assets/models/prefixes.lxp");
const HEADER: usize = 20;
const NODE: usize = 4;
const EDGE: usize = 4;

pub(super) const ENGLISH: u8 = 1;
pub(super) const UKRAINIAN: u8 = 2;

fn u32_at(offset: usize) -> u32 {
    u32::from_le_bytes(
        MODEL[offset..offset + 4]
            .try_into()
            .expect("prefix model field"),
    )
}

const DEAD: u32 = u32::MAX;
const MAX_PREFIX: usize = 64;

fn next(node: u32, key: u8) -> u32 {
    if node == DEAD {
        return DEAD;
    }
    let edges = HEADER + u32_at(8) as usize * NODE;
    let record = u32_at(HEADER + node as usize * NODE);
    let start = (record >> 8) as usize;
    let count = ((record >> 2) & 63) as usize;
    let edge_at = |index| u32_at(edges + (start + index) * EDGE);
    let found = if count <= 4 {
        (0..count).find(|&i| edge_at(i) as u8 == key)
    } else {
        let mut low = 0;
        let mut high = count;
        while low < high {
            let mid = low + (high - low) / 2;
            if (edge_at(mid) as u8) < key {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        (low < count && edge_at(low) as u8 == key).then_some(low)
    };
    found.map_or(DEAD, |index| edge_at(index) >> 8)
}

fn language_mask(node: u32) -> u8 {
    if node == DEAD {
        0
    } else {
        (u32_at(HEADER + node as usize * NODE) & 3) as u8
    }
}

/// Bit 0: at least one English continuation; bit 1: Ukrainian continuation.
/// Absence is evidence about this lexicon, not proof that a word cannot exist.
pub(super) fn languages(physical: &str) -> u8 {
    if !physical.is_ascii() {
        return 0;
    }
    let mut node = u32_at(16);
    for key in physical.bytes() {
        node = next(node, key);
        if node == DEAD {
            return 0;
        }
    }
    language_mask(node)
}

/// Cached states for every prefix of one bounded, normalized physical word.
/// Comparing actual bytes makes reuse safe after edits, cancellations, layout
/// changes and missed samples. Only the changed suffix traverses the automaton.
#[derive(Debug, Clone)]
pub(super) struct Cursor {
    keys: [u8; MAX_PREFIX],
    states: [u32; MAX_PREFIX + 1],
    length: usize,
    #[cfg(test)]
    transitions: usize,
}

impl Default for Cursor {
    fn default() -> Self {
        let mut states = [DEAD; MAX_PREFIX + 1];
        states[0] = u32_at(16);
        Self {
            keys: [0; MAX_PREFIX],
            states,
            length: 0,
            #[cfg(test)]
            transitions: 0,
        }
    }
}

impl Cursor {
    pub(super) fn reset(&mut self) {
        self.length = 0;
    }

    pub(super) fn languages(&mut self, physical: &str) -> u8 {
        if physical.len() > MAX_PREFIX || !physical.is_ascii() {
            self.reset();
            return languages(physical);
        }
        let keys = physical.as_bytes();
        let old = &self.keys[..self.length];
        let shared = if keys.starts_with(old) {
            self.length
        } else if old.starts_with(keys) {
            keys.len()
        } else {
            old.iter().zip(keys).position(|(a, b)| a != b).unwrap_or(0)
        };
        for (index, &key) in keys.iter().enumerate().skip(shared) {
            #[cfg(test)]
            {
                self.transitions += 1;
            }
            self.states[index + 1] = next(self.states[index], key);
            self.keys[index] = key;
        }
        self.length = keys.len();
        language_mask(self.states[self.length])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_automaton_has_valid_sorted_edges_and_language_masks() {
        assert_eq!(&MODEL[..8], b"UPYRLP1\0");
        let nodes = u32_at(8) as usize;
        let edges = u32_at(12) as usize;
        let start = HEADER + nodes * NODE;
        assert_eq!(MODEL.len(), start + edges * EDGE);
        assert!((u32_at(16) as usize) < nodes);
        for node in 0..nodes {
            let offset = HEADER + node * NODE;
            let record = u32_at(offset);
            let first = (record >> 8) as usize;
            let length = ((record >> 2) & 63) as usize;
            let mask = (record & 3) as u8;
            assert!((1..=3).contains(&mask));
            assert!(first + length <= edges);
            let mut previous = None;
            for i in first..first + length {
                let edge = u32_at(start + i * EDGE);
                let key = edge as u8;
                let child = (edge >> 8) as usize;
                assert!(key.is_ascii());
                assert!(previous.is_none_or(|p| p < key));
                assert!(child < node, "acyclic bottom-up state numbering");
                assert_eq!(
                    (u32_at(HEADER + child * NODE) & 3) as u8 & mask,
                    (u32_at(HEADER + child * NODE) & 3) as u8
                );
                previous = Some(key);
            }
        }
    }

    #[test]
    fn physical_prefixes_distinguish_languages_and_preserve_ambiguity() {
        assert_eq!(languages("dbgh"), UKRAINIAN);
        assert_eq!(languages("cont"), ENGLISH);
        assert_ne!(languages("rehn") & ENGLISH, 0);
        assert_ne!(languages("lsdr") & UKRAINIAN, 0);
        assert_eq!(languages("випр"), 0);
    }

    #[test]
    fn incremental_cursor_only_walks_new_or_changed_suffixes() {
        let mut cursor = Cursor::default();
        for length in 1..="dbghfdktyyz".len() {
            let prefix = &"dbghfdktyyz"[..length];
            assert_eq!(cursor.languages(prefix), languages(prefix));
            assert_eq!(cursor.transitions, length);
        }
        let walked = cursor.transitions;
        for prefix in ["dbghfdktyyz", "dbghfdktyy", "dbgh", "dbgh"] {
            assert_eq!(cursor.languages(prefix), languages(prefix));
            assert_eq!(cursor.transitions, walked);
        }
        assert_eq!(cursor.languages("dbghx"), languages("dbghx"));
        assert_eq!(cursor.transitions, walked + 1);
        cursor.languages("dbghf");
        assert_eq!(cursor.transitions, walked + 2);
        cursor.reset();
        cursor.languages("dbgh");
        assert_eq!(cursor.transitions, walked + 6);
    }

    #[test]
    fn cursor_recovers_from_dead_branches_resets_and_unsupported_inputs() {
        let mut cursor = Cursor::default();
        for word in [
            "",
            "cont",
            "cont?",
            "cont??",
            "cont?",
            "cont",
            "continue",
            "conn",
            "CONN",
            "conn",
            "rehn",
            "rehnquist",
            "випр",
            "dbgh",
            "",
            "lsdr",
        ] {
            assert_eq!(cursor.languages(word), languages(word), "{word}");
        }
        for length in [63, 64, 65, 80, 64, 4] {
            let word = "a".repeat(length);
            assert_eq!(cursor.languages(&word), languages(&word));
        }
        assert_eq!(cursor.languages("dbgh"), UKRAINIAN);
        cursor.reset();
        assert_eq!(cursor.languages("cont"), ENGLISH);
    }
}
