//! Lazy eight-way cache with bounded clock eviction. A miss replaces at most
//! one token; neither a global clear nor a hash-table rebuild is needed.
use super::{TOKEN_CACHE_CAPACITY, TokenContribution};
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

const WAYS: usize = 8;
const BUCKETS: usize = TOKEN_CACHE_CAPACITY / WAYS;
const PAGE_BUCKETS: usize = 128;
const PAGES: usize = BUCKETS / PAGE_BUCKETS;
const MAX_TOKEN_BYTES: usize = 256;
/// A bitmap ranks occupied slots into a dense, bounded vector. Empty slots
/// cost one bit instead of a pointer; page growth moves at most 128 buckets.
#[derive(Default)]
struct Page {
    occupied: [u64; 2],
    buckets: Vec<Bucket>,
}
impl Page {
    fn position(&self, slot: usize) -> (usize, bool) {
        let half = slot / 64;
        let bit = 1_u64 << (slot % 64);
        let before = if half == 0 {
            0
        } else {
            self.occupied[0].count_ones()
        };
        let rank = before + (self.occupied[half] & (bit - 1)).count_ones();
        (rank as usize, self.occupied[half] & bit != 0)
    }
    fn get(&mut self, slot: usize) -> Option<&mut Bucket> {
        let (rank, present) = self.position(slot);
        present.then(|| &mut self.buckets[rank])
    }
    fn get_or_insert(&mut self, slot: usize) -> &mut Bucket {
        let (rank, present) = self.position(slot);
        if !present {
            grow(&mut self.buckets, PAGE_BUCKETS);
            self.buckets.insert(rank, Bucket::default());
            self.occupied[slot / 64] |= 1_u64 << (slot % 64);
        }
        &mut self.buckets[rank]
    }
}

fn grow<T>(entries: &mut Vec<T>, limit: usize) {
    if entries.len() == entries.capacity() {
        let capacity = (entries.len() * 2).max(1).min(limit);
        entries.reserve_exact(capacity - entries.len());
    }
}

pub(super) struct TokenCache {
    pages: [Option<Box<Page>>; PAGES],
    hash: RandomState,
    len: usize,
}
impl Default for TokenCache {
    fn default() -> Self {
        Self {
            pages: std::array::from_fn(|_| None),
            hash: RandomState::new(),
            len: 0,
        }
    }
}
struct Entry {
    token: Box<str>,
    value: TokenContribution,
}
#[derive(Default)]
struct Bucket {
    entries: Vec<Entry>,
    recent: u8,
    hand: u8,
}

impl Bucket {
    fn get(&mut self, token: &str) -> Option<TokenContribution> {
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.token.as_ref() == token {
                self.recent |= 1 << index;
                return Some(entry.value);
            }
        }
        None
    }
    fn insert(&mut self, token: &str, value: TokenContribution) -> bool {
        for (index, entry) in self.entries.iter_mut().enumerate() {
            if entry.token.as_ref() == token {
                entry.value = value;
                self.recent |= 1 << index;
                return false;
            }
        }
        let added = self.entries.len() < WAYS;
        let index = if added {
            self.entries.len()
        } else {
            // At most eight reference-bit checks, even if every slot was hit.
            for _ in 0..WAYS {
                let bit = 1 << self.hand;
                if self.recent & bit == 0 {
                    break;
                }
                self.recent &= !bit;
                self.hand = (self.hand + 1) % WAYS as u8;
            }
            self.hand as usize
        };
        let entry = Entry {
            token: token.into(),
            value,
        };
        if added {
            grow(&mut self.entries, WAYS);
            self.entries.push(entry);
        } else {
            self.entries[index] = entry;
        }
        self.recent |= 1 << index;
        self.hand = ((index + 1) % WAYS) as u8;
        added
    }
}
impl TokenCache {
    #[cfg(any(test, feature = "engine-bench"))]
    pub(super) fn len(&self) -> usize {
        self.len
    }
    pub(super) fn get(&mut self, token: &str) -> Option<TokenContribution> {
        if token.len() > MAX_TOKEN_BYTES {
            return None;
        }
        let index = self.hash.hash_one(token) as usize & (BUCKETS - 1);
        self.pages[index / PAGE_BUCKETS]
            .as_mut()?
            .get(index % PAGE_BUCKETS)?
            .get(token)
    }
    pub(super) fn insert(&mut self, token: &str, value: TokenContribution) {
        if token.len() > MAX_TOKEN_BYTES {
            return;
        }
        let index = self.hash.hash_one(token) as usize & (BUCKETS - 1);
        let page = self.pages[index / PAGE_BUCKETS].get_or_insert_with(Default::default);
        let bucket = page.get_or_insert(index % PAGE_BUCKETS);
        self.len += usize::from(bucket.insert(token, value));
        debug_assert!(self.len <= TOKEN_CACHE_CAPACITY);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn value(raw: i64) -> TokenContribution {
        TokenContribution {
            raw,
            maximum: 100,
            grams: 4,
        }
    }
    #[test]
    fn bucket_collision_and_clock_eviction_preserve_exact_keys() {
        let mut bucket = Bucket::default();
        for i in 0..WAYS {
            assert!(bucket.insert(&format!("word{i}"), value(i as i64)));
        }
        assert!(!bucket.insert("word2", value(99)));
        assert!(!bucket.insert("new", value(8)));
        assert!(bucket.get("word0").is_none());
        assert_eq!(bucket.get("word2").unwrap().raw, 99);
        for i in 0..100 {
            bucket.insert(&format!("next{i}"), value(i));
        }
        assert_eq!(bucket.entries.len(), WAYS);
        assert!(bucket.get("missing").is_none());
    }
    #[test]
    fn sparse_pages_preserve_rank_across_both_bitmap_halves() {
        let mut page = Page::default();
        assert_eq!(page.buckets.capacity(), 0);
        for step in 0..PAGE_BUCKETS {
            let slot = (step * 73 + 63) % PAGE_BUCKETS;
            assert!(page.get(slot).is_none());
            assert!(page.get_or_insert(slot).insert("token", value(slot as i64)));
            for prior in 0..=step {
                let slot = (prior * 73 + 63) % PAGE_BUCKETS;
                assert_eq!(
                    page.get(slot).unwrap().get("token"),
                    Some(value(slot as i64))
                );
            }
            assert_eq!(page.buckets.len(), step + 1);
            assert!(page.buckets.capacity() <= PAGE_BUCKETS);
            assert!(page.buckets.capacity() <= (step + 1).next_power_of_two());
        }
        assert_eq!(page.occupied, [u64::MAX; 2]);
        assert_eq!(page.buckets.capacity(), PAGE_BUCKETS);
    }
    #[test]
    fn bucket_grows_only_to_occupied_capacity_and_keeps_clock_policy() {
        let mut bucket = Bucket::default();
        for i in 0..WAYS {
            bucket.insert(&format!("word{i}"), value(i as i64));
            assert_eq!(bucket.entries.capacity(), (i + 1).next_power_of_two());
        }
        bucket.insert("first", value(10)); // all reference bits were set: evict slot 0
        bucket.get("word1"); // protect the next clock slot
        bucket.insert("second", value(11));
        assert_eq!(bucket.get("word1"), Some(value(1)));
        assert!(bucket.get("word2").is_none());
        assert_eq!(bucket.entries.capacity(), WAYS);
    }
    #[test]
    fn cache_is_lazy_bounded_and_bypasses_large_tokens() {
        let mut cache = TokenCache::default();
        assert!(cache.pages.iter().all(Option::is_none));
        for i in 0..TOKEN_CACHE_CAPACITY * 3 {
            let token = format!("word{i}");
            cache.insert(&token, value(i as i64));
            assert_eq!(cache.get(&token).unwrap().raw, i as i64);
        }
        assert!(cache.len() <= TOKEN_CACHE_CAPACITY);
        let len = cache.len();
        let token = "a".repeat(MAX_TOKEN_BYTES + 1);
        cache.insert(&token, value(0));
        assert!(cache.get(&token).is_none());
        assert_eq!(cache.len(), len);
    }
}
