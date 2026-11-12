//! Offline CPU/allocation benchmark; never observes or synthesizes user input.
#![allow(dead_code)]
#![cfg(not(test))]

// This standalone adapter includes platform-specific public re-exports.
#[allow(unused_imports)]
#[path = "../src/auto_correct.rs"]
mod auto_correct;
use device_query::Keycode;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    time::Instant,
};
use upyr::{config, system_layout};
use upyr_core::{AutoDecision, InputLayout, WordSample};

struct Counting;
static COUNT: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static CORRECTIONS: AtomicU64 = AtomicU64::new(0);
// Forward the exact allocator contract; counting is enabled only for a separate
// untimed replay, after all lazy caches and reusable buffers have warmed.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNT.load(Relaxed) {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size() as u64, Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if COUNT.load(Relaxed) {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(size as u64, Relaxed);
        }
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

// The baseline uses the owned-snapshot API. A later inherent method takes
// precedence, so this identical harness measures both implementations.
trait CurrentPartial {
    fn evaluate_current_partial(&mut self, config: &config::Config) -> AutoDecision;
}
impl CurrentPartial for auto_correct::AutoWordTracker {
    fn evaluate_current_partial(&mut self, config: &config::Config) -> AutoDecision {
        self.partial_word_sample()
            .map_or(AutoDecision::Continue, |sample| {
                self.evaluate_partial(&sample, config)
            })
    }
}

fn digest(hash: &mut u64, decision: AutoDecision) {
    let tag = match decision {
        AutoDecision::Continue => 0,
        AutoDecision::Reset => 1,
        AutoDecision::Correct(c) => {
            if COUNT.load(Relaxed) {
                CORRECTIONS.fetch_add(1, Relaxed);
            }
            for byte in c
                .expected_source
                .bytes()
                .chain([0])
                .chain(c.replacement.bytes())
                .chain([c.direction as u8])
            {
                *hash = hash
                    .wrapping_mul(1099511628211)
                    .wrapping_add(u64::from(byte));
            }
            2
        }
    };
    *hash = hash.wrapping_mul(1099511628211).wrapping_add(tag);
}

fn measure(name: &str, queries: usize, mut run: impl FnMut() -> u64) {
    let replays: usize = std::env::var("UPYR_ENGINE_BENCH_REPLAYS")
        .ok()
        .map(|s| s.parse().expect("positive replay count"))
        .unwrap_or(16);
    assert!(replays > 0);
    let checksum = run();
    let mut times = Vec::new();
    for _ in 0..9 {
        let start = Instant::now();
        for _ in 0..replays {
            assert_eq!(black_box(run()), checksum);
        }
        times.push(start.elapsed().as_nanos() as f64 / (queries * replays) as f64);
    }
    ALLOCS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    CORRECTIONS.store(0, Relaxed);
    COUNT.store(true, Relaxed);
    let counted = run();
    COUNT.store(false, Relaxed);
    assert_eq!(counted, checksum);
    times.sort_by(f64::total_cmp);
    println!(
        "{{\"workload\":\"{name}\",\"queries\":{queries},\"timed_replays\":{replays},\"median_ns\":{:.2},\"min_ns\":{:.2},\"max_ns\":{:.2},\"allocations\":{},\"allocated_bytes\":{},\"corrections\":{},\"checksum\":{checksum},\"core_state_bytes\":{},\"host_state_bytes\":{}}}",
        times[4],
        times[0],
        times[8],
        ALLOCS.load(Relaxed),
        BYTES.load(Relaxed),
        CORRECTIONS.load(Relaxed),
        std::mem::size_of::<upyr_core::PartialWordState>(),
        std::mem::size_of::<auto_correct::AutoWordTracker>()
    );
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| ".cache/upyr-benchmarks/clean-wikipedia-v1.tsv".into());
    let text = std::fs::read_to_string(path).unwrap();
    let words: Vec<_> = text
        .lines()
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .step_by(20)
        .map(|s| s.split('\t').nth(2).unwrap().to_owned())
        .collect();
    let config = config::Config::default();
    auto_correct::prepare();
    let installed = system_layout::shared_installed_mapping().unwrap();
    for (editing, casing) in [(false, 0), (true, 0), (false, 1), (false, 2)] {
        let mut queries = Vec::new();
        for word in &words {
            let word = match casing {
                1 => {
                    let mut word = word.clone();
                    if let Some(first) = word.get_mut(..1) {
                        first.make_ascii_uppercase();
                    }
                    word
                }
                2 => word.to_ascii_uppercase(),
                _ => word.clone(),
            };
            for layout in [InputLayout::English, InputLayout::Ukrainian] {
                let mut physical = String::new();
                let mut first = true;
                for c in word.chars().chain(if editing {
                    vec!['\u{8}', '\u{8}', 'x', '\u{8}', 'e']
                } else {
                    vec![]
                }) {
                    if c == '\u{8}' {
                        physical.pop();
                    } else {
                        physical.push(c);
                    }
                    queries.push((first, WordSample::new(&physical, &physical, layout)));
                    first = false;
                }
            }
        }
        for (suffix, mapping) in [("", None), ("_installed", installed.as_deref())] {
            let mut state = upyr_core::PartialWordState::default();
            let policy = upyr_core::AutoCorrectPolicy::default();
            let label = match (editing, casing) {
                (true, _) => "core_editing",
                (_, 1) => "core_titlecase",
                (_, 2) => "core_uppercase",
                _ => "core_typing",
            };
            measure(&format!("{label}{suffix}"), queries.len(), || {
                let mut hash = 0;
                for (reset, sample) in &queries {
                    if *reset {
                        state.reset();
                    }
                    digest(
                        &mut hash,
                        state.evaluate(black_box(sample), black_box(&policy), black_box(mapping)),
                    );
                }
                hash
            });
        }
    }
    for context in [false, true] {
        let mut events = Vec::new();
        let alphabet = [
            Keycode::A,
            Keycode::B,
            Keycode::C,
            Keycode::D,
            Keycode::E,
            Keycode::F,
            Keycode::G,
            Keycode::H,
            Keycode::I,
            Keycode::J,
            Keycode::K,
            Keycode::L,
            Keycode::M,
            Keycode::N,
            Keycode::O,
            Keycode::P,
            Keycode::Q,
            Keycode::R,
            Keycode::S,
            Keycode::T,
            Keycode::U,
            Keycode::V,
            Keycode::W,
            Keycode::X,
            Keycode::Y,
            Keycode::Z,
        ];
        for (i, word) in words.iter().enumerate() {
            let prefix = if context {
                if i % 2 == 0 { "we can " } else { "nj vb " }
            } else {
                ""
            };
            let mut first = true;
            for c in prefix.chars().chain(word.chars()) {
                let key = match c.to_ascii_lowercase() {
                    'a'..='z' => alphabet[c.to_ascii_lowercase() as usize - 'a' as usize],
                    ' ' => Keycode::Space,
                    '[' => Keycode::LeftBracket,
                    ']' => Keycode::RightBracket,
                    ';' => Keycode::Semicolon,
                    '\'' => Keycode::Apostrophe,
                    ',' => Keycode::Comma,
                    '.' => Keycode::Dot,
                    '/' => Keycode::Slash,
                    '`' => Keycode::Grave,
                    '\\' => Keycode::BackSlash,
                    _ => continue,
                };
                events.push((
                    first,
                    if i % 2 == 0 {
                        InputLayout::English
                    } else {
                        InputLayout::Ukrainian
                    },
                    auto_correct::AutoKeyEvent {
                        key,
                        shifted: c.is_ascii_uppercase(),
                        timestamp: None,
                    },
                ));
                first = false;
            }
        }
        let mut tracker = auto_correct::AutoWordTracker::default();
        measure(
            if context {
                "host_context"
            } else {
                "host_typing"
            },
            events.len(),
            || {
                let mut hash = 0;
                for (reset, layout, event) in &events {
                    if *reset {
                        tracker.clear();
                        tracker.set_source_layout(Some(*layout));
                    }
                    if tracker.observe(*event).is_none() {
                        digest(
                            &mut hash,
                            tracker.evaluate_current_partial(black_box(&config)),
                        );
                    }
                }
                hash
            },
        );
    }
}
