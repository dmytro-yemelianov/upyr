//! Standalone benchmark: tools/benchmark_punto.sh
//! Reports allocator-requested live/peak bytes, not process RSS or allocator overhead.
#![allow(dead_code)]

#[path = "../crates/upyr-core/src/auto_correct/punto_rules.rs"]
mod punto_rules;
#[path = "../crates/upyr-core/src/auto_correct/punto_model.rs"]
mod punto_model;
#[path = "../crates/upyr-core/src/auto_correct/punto_tables.rs"]
mod punto_tables;
#[path = "../crates/upyr-core/src/auto_correct/punto_trie.rs"]
mod punto_trie;

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

struct Counted;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

fn added(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Relaxed) + bytes;
    PEAK.fetch_max(live, Relaxed);
    ALLOCATIONS.fetch_add(1, Relaxed);
}

unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() {
            added(layout.size());
        }
        result
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, old: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(ptr, old, size) };
        if !result.is_null() {
            LIVE.fetch_sub(old.size(), Relaxed);
            added(size);
        }
        result
    }
}

#[global_allocator]
static ALLOCATOR: Counted = Counted;

fn main() {
    let mut probes = vec![
        "сщте",
        "cont",
        "сщтт",
        "conn",
        "ghbd",
        "прив",
        "дівк",
        "lsdr",
        "СЩТЕ",
        "CONT",
        "hello",
        "руддщ",
        "rust",
        "кгіе",
        "abcdefghijk",
        "qwertyuiop",
        "неповний",
        "CONTOSO",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    for line in include_str!("../crates/upyr-core/assets/punto/ps.txt")
        .lines()
        .skip(1)
        .filter(|s| !s.is_empty())
        .step_by(127)
    {
        let key = line
            .split_once(' ')
            .filter(|(tag, _)| tag.starts_with('_'))
            .map_or(line, |(_, key)| key);
        probes.push(key.to_owned());
        probes.push(format!("x{key}x"));
    }
    let mut build_us = Vec::new();
    let mut retained = 0;
    let mut peak = 0;
    let mut build_allocations = 0;
    for _ in 0..7 {
        let start_bytes = LIVE.load(Relaxed);
        PEAK.store(start_bytes, Relaxed);
        let start_allocations = ALLOCATIONS.load(Relaxed);
        let start = Instant::now();
        let rules = black_box(punto_rules::RuleSet::builtin());
        let elapsed = start.elapsed().as_micros();
        retained = LIVE.load(Relaxed) - start_bytes;
        peak = PEAK.load(Relaxed) - start_bytes;
        build_allocations = ALLOCATIONS.load(Relaxed) - start_allocations;
        drop(rules);
        build_us.push(elapsed);
    }
    let rules = punto_rules::RuleSet::builtin();
    let table_bytes = rules.storage_bytes();
    let all_table_bytes = table_bytes
        + punto_tables::TRIGGERS.storage_bytes()
        + punto_tables::EXCEPTIONS.storage_bytes()
        + punto_tables::CASE_EXCEPTIONS.storage_bytes();
    let loops = 2000;
    let queries = loops * probes.len();
    let mut lookup_ns = Vec::new();
    let mut lookup_allocations = 0;
    let mut matches = 0;
    for _ in 0..7 {
        matches = 0;
        let start_allocations = ALLOCATIONS.load(Relaxed);
        let start = Instant::now();
        for _ in 0..loops {
            for source in &probes {
                matches += usize::from(black_box(rules.first_match(black_box(source))).is_some());
            }
        }
        lookup_allocations = ALLOCATIONS.load(Relaxed) - start_allocations;
        lookup_ns.push(start.elapsed().as_nanos() / queries as u128);
    }
    build_us.sort();
    lookup_ns.sort();
    println!(
        "{{\"probes\":{},\"queries_per_run\":{queries},\"matches_per_run\":{matches},\"build_median_us\":{},\"table_bytes\":{table_bytes},\"all_table_bytes\":{all_table_bytes},\"retained_bytes\":{retained},\"build_peak_bytes\":{peak},\"build_allocations\":{build_allocations},\"lookup_median_ns\":{},\"lookup_allocations\":{lookup_allocations}}}",
        probes.len(),
        build_us[3],
        lookup_ns[3]
    );
}
