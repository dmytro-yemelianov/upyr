//! Portable offline regression checks. No OS layouts, user input or clock gates.
#[cfg(not(target_family = "wasm"))]
mod native {
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        hint::black_box,
        sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::Relaxed},
    };
    use upyr_core::{
        AutoCorrectPolicy, AutoDecision, InputLayout, PartialWordState, Sensitivity, WordSample,
        auto_correct::benchmark as engine,
        evaluate, evaluate_partial,
        layout::{Direction, append_converted, benchmark as mapping, default_physical_mapping},
    };

    struct Counted;
    static ENABLED: AtomicBool = AtomicBool::new(false);
    static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    static LIVE: AtomicI64 = AtomicI64::new(0);
    static PEAK: AtomicI64 = AtomicI64::new(0);

    fn allocated(bytes: usize) {
        ALLOCATIONS.fetch_add(1, Relaxed);
        let live = LIVE.fetch_add(bytes as i64, Relaxed) + bytes as i64;
        PEAK.fetch_max(live, Relaxed);
    }
    // Forward the allocator contract unchanged. The single-threaded executable
    // counts only explicit measurement windows after all static tables warm up.
    unsafe impl GlobalAlloc for Counted {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc(layout) };
            if !ptr.is_null() && ENABLED.load(Relaxed) {
                allocated(layout.size());
            }
            ptr
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            if ENABLED.load(Relaxed) {
                LIVE.fetch_sub(layout.size() as i64, Relaxed);
            }
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let result = unsafe { System.realloc(ptr, layout, size) };
            if !result.is_null() && ENABLED.load(Relaxed) {
                LIVE.fetch_sub(layout.size() as i64, Relaxed);
                allocated(size);
            }
            result
        }
    }
    #[global_allocator]
    static ALLOCATOR: Counted = Counted;

    fn memory(run: impl FnOnce()) -> (u64, u64, u64) {
        ALLOCATIONS.store(0, Relaxed);
        LIVE.store(0, Relaxed);
        PEAK.store(0, Relaxed);
        ENABLED.store(true, Relaxed);
        run();
        ENABLED.store(false, Relaxed);
        let live = LIVE.load(Relaxed);
        assert!(
            live >= 0,
            "measurement freed memory allocated outside its window"
        );
        (
            ALLOCATIONS.load(Relaxed),
            live as u64,
            PEAK.load(Relaxed) as u64,
        )
    }

    fn budget(name: &str, actual: u64) -> bool {
        let mut found = false;
        let mut passed = true;
        for line in include_str!("../fixtures/engine-budgets-v1.tsv")
            .lines()
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
        {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 3);
            if fields[0] != name {
                continue;
            }
            found = true;
            let limit: u64 = fields[2].parse().unwrap();
            let relation = fields[1];
            let ok = match relation {
                "max" => actual <= limit,
                "min" => actual >= limit,
                _ => panic!("unknown budget relation"),
            };
            println!(
                "{{\"check\":\"{name}\",\"relation\":\"{relation}\",\"actual\":{actual},\"limit\":{limit},\"passed\":{ok}}}"
            );
            passed &= ok;
        }
        assert!(found, "missing budget for {name}");
        passed
    }

    fn snapshots() -> Vec<WordSample> {
        include_str!("../fixtures/boundary-replay-v1.tsv")
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                let fields: Vec<_> = line.split('\t').collect();
                assert_eq!(fields.len(), 13);
                let layout = match fields[6] {
                    "english" => InputLayout::English,
                    "ukrainian" => InputLayout::Ukrainian,
                    _ => panic!("unknown fixture layout"),
                };
                WordSample::new(
                    fields[7].replace('␠', " "),
                    fields[8].replace('␠', " "),
                    layout,
                )
            })
            .collect()
    }

    fn reference_checks(samples: &[WordSample], pairs: &[(char, char)]) -> bool {
        let mut comparisons = 0;
        let mut mismatches = 0;
        let mut state = PartialWordState::default();
        for map in [None, Some(pairs)] {
            let policy = AutoCorrectPolicy::default();
            for sample in samples {
                state.reset();
                let text = sample.physical_word();
                let ends: Vec<_> = std::iter::once(0)
                    .chain(text.char_indices().map(|(i, c)| i + c.len_utf8()))
                    .collect();
                // Append then Backspace through the same stream.
                for &end in ends.iter().chain(ends.iter().rev()) {
                    let prefix = &text[..end];
                    let expected = evaluate_partial(
                        &WordSample::new(prefix, prefix, sample.source_layout()),
                        &policy,
                        map,
                    );
                    let actual =
                        state.evaluate_physical(prefix, sample.source_layout(), &policy, map);
                    comparisons += 1;
                    mismatches += u64::from(actual != expected);
                }
            }
        }
        let mut passed = budget("partial_reference_comparisons", comparisons);
        passed &= budget("partial_reference_mismatches", mismatches);
        let mut comparisons = 0;
        let mut mismatches = 0;
        let mut custom = pairs.to_vec();
        custom[0].1 = '🦀';
        custom.push(('q', 'x'));
        for map in [pairs, custom.as_slice()] {
            for sensitivity in [
                Sensitivity::Conservative,
                Sensitivity::Balanced,
                Sensitivity::Aggressive,
            ] {
                let policy = AutoCorrectPolicy {
                    sensitivity,
                    ..AutoCorrectPolicy::default()
                };
                for sample in samples {
                    mapping::set_linear_mapping(true);
                    let expected = evaluate(sample, &policy, Some(map));
                    mapping::set_linear_mapping(false);
                    let actual = evaluate(sample, &policy, Some(map));
                    comparisons += 1;
                    mismatches += u64::from(actual != expected);
                }
            }
        }
        passed &= budget("boundary_reference_comparisons", comparisons);
        passed &= budget("boundary_reference_mismatches", mismatches);
        passed
    }

    fn partial_budget(pairs: &[(char, char)]) -> bool {
        let mut state = PartialWordState::default();
        let policy = AutoCorrectPolicy::default();
        let words = ["continue", "working", "hello", "world"];
        let mut replay = || {
            let mut corrections = 0;
            for word in words {
                state.reset();
                for end in 0..=word.len() {
                    let result = state.evaluate_physical(
                        black_box(&word[..end]),
                        InputLayout::English,
                        &policy,
                        Some(pairs),
                    );
                    corrections += u64::from(matches!(result, AutoDecision::Correct(_)));
                    black_box(result);
                }
            }
            corrections
        };
        black_box(replay());
        let mut corrections = 0;
        let mem = memory(|| {
            for _ in 0..1000 {
                corrections += replay();
            }
        });
        budget("partial_warm_allocations", mem.0) & budget("partial_warm_corrections", corrections)
    }

    fn mapping_budgets(pairs: &[(char, char)]) -> bool {
        let mut output = String::with_capacity(256);
        let direction = Direction::EnglishToUkrainian;
        mapping::reset_mapping_cache();
        append_converted(&mut output, "dbghfdktyyz", direction, Some(pairs));
        let mut passed = budget(
            "mapping_standard_heap_bytes",
            mapping::mapping_heap_bytes() as u64,
        );
        let mem = memory(|| {
            for _ in 0..1000 {
                output.clear();
                append_converted(
                    &mut output,
                    black_box("dbghfdktyyz"),
                    direction,
                    Some(pairs),
                );
                black_box(&output);
            }
        });
        passed &= budget("mapping_warm_allocations", mem.0);
        let first: Vec<_> = (0..256)
            .map(|i| {
                (
                    char::from_u32(0x1000 + i).unwrap(),
                    char::from_u32(0x2000 + i).unwrap(),
                )
            })
            .collect();
        let second: Vec<_> = (0..256)
            .map(|i| {
                (
                    char::from_u32(0x3000 + i).unwrap(),
                    char::from_u32(0x4000 + i).unwrap(),
                )
            })
            .collect();
        mapping::reset_mapping_cache();
        let mem = memory(|| {
            for map in [&first, &second] {
                output.clear();
                append_converted(&mut output, "q", direction, Some(map));
                black_box(&output);
            }
        });
        passed &= budget(
            "mapping_max_heap_bytes",
            mapping::mapping_heap_bytes() as u64,
        );
        passed &= budget("mapping_replacement_live_bytes", mem.1);
        passed &= budget("mapping_replacement_peak_bytes", mem.2);
        mapping::reset_mapping_cache();
        let huge = vec![('q', 'x'); 257];
        for map in [&pairs[..7], huge.as_slice()] {
            output.clear();
            append_converted(&mut output, "q", direction, Some(map));
        }
        passed &= budget(
            "mapping_fallback_heap_bytes",
            mapping::mapping_heap_bytes() as u64,
        );
        passed
    }

    fn token_budgets() -> bool {
        let tokens: Vec<_> = (0..131072)
            .map(|mut i| {
                let mut token = String::from("qzx");
                for _ in 0..6 {
                    token.push((b'a' + (i % 26) as u8) as char);
                    i /= 26;
                }
                token.push_str("zxq");
                token
            })
            .collect();
        black_box(engine::token("warmup"));
        engine::reset_caches();
        let mut previous_size = 0;
        let mut size_regressions = 0;
        let mem = memory(|| {
            for token in &tokens {
                black_box(engine::token(token));
                let size = engine::cache_len();
                size_regressions += u64::from(size < previous_size);
                previous_size = size;
            }
        });
        let mut passed = budget("token_cache_entries", engine::cache_len() as u64);
        passed &= budget("token_cache_size_regressions", size_regressions);
        passed &= budget("token_cache_retained_bytes", mem.1);
        passed &= budget("token_cache_peak_bytes", mem.2);
        let last = tokens.last().unwrap();
        let hit = memory(|| {
            for _ in 0..1000 {
                black_box(engine::token(black_box(last)));
            }
        });
        passed &= budget("token_cache_hit_allocations", hit.0);
        engine::reset_caches();
        passed
    }

    pub fn run() -> bool {
        let pairs = default_physical_mapping();
        let samples = snapshots();
        let mut passed = reference_checks(&samples, &pairs);
        passed &= partial_budget(&pairs);
        passed &= mapping_budgets(&pairs);
        passed &= token_budgets();
        passed
    }
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    if !native::run() {
        std::process::exit(1);
    }
}

#[cfg(target_family = "wasm")]
fn main() {
    panic!("engine_contract requires a native target; use the WASM contract checks instead");
}
