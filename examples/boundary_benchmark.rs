//! Offline Space/Enter and cache-pressure probes; no desktop input is sent.
#![cfg(not(test))]
#![allow(dead_code)]
// This standalone adapter includes platform-specific public re-exports.
#[allow(unused_imports)]
#[path = "../src/auto_correct.rs"]
mod auto_correct;
use device_query::Keycode;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::Relaxed},
    time::Instant,
};
use upyr::{config, system_layout};
use upyr_core::{AutoDecision, auto_correct::benchmark as core};

struct Counted;
static COUNT: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicI64 = AtomicI64::new(0);
static PEAK: AtomicI64 = AtomicI64::new(0);
fn add(size: usize) {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(size as u64, Relaxed);
    let current = LIVE.fetch_add(size as i64, Relaxed) + size as i64;
    PEAK.fetch_max(current, Relaxed);
}
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && COUNT.load(Relaxed) {
            add(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if COUNT.load(Relaxed) {
            LIVE.fetch_sub(layout.size() as i64, Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(ptr, layout, size) };
        if !result.is_null() && COUNT.load(Relaxed) {
            LIVE.fetch_sub(layout.size() as i64, Relaxed);
            add(size);
        }
        result
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;

fn memory(run: impl FnOnce()) -> (u64, u64, i64, i64) {
    core::reset_caches();
    ALLOCS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    LIVE.store(0, Relaxed);
    PEAK.store(0, Relaxed);
    COUNT.store(true, Relaxed);
    run();
    COUNT.store(false, Relaxed);
    let result = (
        ALLOCS.load(Relaxed),
        BYTES.load(Relaxed),
        LIVE.load(Relaxed),
        PEAK.load(Relaxed),
    );
    assert!(result.2 >= 0);
    result
}

fn hash(hash: &mut u64, value: u64) {
    *hash = hash.wrapping_mul(1099511628211).wrapping_add(value);
}
fn decision(hash_value: &mut u64, result: AutoDecision) {
    match result {
        AutoDecision::Continue => hash(hash_value, 0),
        AutoDecision::Reset => hash(hash_value, 1),
        AutoDecision::Correct(c) => {
            hash(hash_value, 2);
            for b in c
                .expected_source
                .bytes()
                .chain([0])
                .chain(c.replacement.bytes())
                .chain([c.direction as u8])
            {
                hash(hash_value, u64::from(b));
            }
        }
    }
}
fn report(name: &str, mut times: Vec<u128>, checksum: u64, mem: (u64, u64, i64, i64)) {
    if let Ok(dir) = std::env::var("UPYR_BOUNDARY_TRACE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.json")),
            format!("{times:?}"),
        )
        .unwrap();
    }
    times.sort_unstable();
    let n = times.len();
    let q = |p: usize| times[((n * p).div_ceil(100)).saturating_sub(1).min(n - 1)];
    println!(
        "{{\"workload\":\"{name}\",\"samples\":{n},\"p50_ns\":{},\"p95_ns\":{},\"p99_ns\":{},\"max_ns\":{},\"checksum\":{checksum},\"allocations\":{},\"allocated_bytes\":{},\"retained_bytes\":{},\"peak_bytes\":{},\"snapshot_bytes\":{}}}",
        q(50),
        q(95),
        q(99),
        times[n - 1],
        mem.0,
        mem.1,
        mem.2,
        mem.3,
        std::mem::size_of::<auto_correct::WordSample>()
    );
}
fn letters(mut index: usize) -> String {
    let mut word = String::from("qzx");
    for _ in 0..6 {
        word.push((b'a' + (index % 26) as u8) as char);
        index /= 26;
    }
    word.push_str("zxq");
    word
}
fn key(c: char) -> auto_correct::AutoKeyEvent {
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
    let code = match c.to_ascii_lowercase() {
        'a'..='z' => alphabet[c.to_ascii_lowercase() as usize - 'a' as usize],
        ' ' => Keycode::Space,
        ',' => Keycode::Comma,
        '.' => Keycode::Dot,
        '[' => Keycode::LeftBracket,
        ']' => Keycode::RightBracket,
        ';' => Keycode::Semicolon,
        '\'' => Keycode::Apostrophe,
        '/' => Keycode::Slash,
        '-' => Keycode::Minus,
        _ => Keycode::Grave,
    };
    auto_correct::AutoKeyEvent {
        key: code,
        shifted: c.is_ascii_uppercase(),
        timestamp: None,
    }
}
fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "boundary".into());
    if mode == "mapping" {
        use upyr_core::{
            Direction::{EnglishToUkrainian, UkrainianToEnglish},
            layout::benchmark as maps,
        };
        let standard = upyr_core::default_physical_mapping();
        let mut alternate = standard.clone();
        alternate[0].1 = '🦀';
        let large: Vec<_> = (0..512)
            .map(|i| (char::from_u32(0x400 + i).unwrap(), 'x'))
            .collect();
        for (name, input, direction, pairs, cold, churn) in [
            (
                "mapping_one_key",
                "q",
                EnglishToUkrainian,
                standard.as_slice(),
                false,
                false,
            ),
            (
                "mapping_short",
                "nj",
                EnglishToUkrainian,
                standard.as_slice(),
                false,
                false,
            ),
            (
                "mapping_word",
                "dbghfdktyyz",
                EnglishToUkrainian,
                standard.as_slice(),
                false,
                false,
            ),
            (
                "mapping_context",
                "we can continue working with this context today",
                EnglishToUkrainian,
                standard.as_slice(),
                false,
                false,
            ),
            (
                "mapping_reverse",
                "ми можемо продовжити працювати сьогодні",
                UkrainianToEnglish,
                standard.as_slice(),
                false,
                false,
            ),
            (
                "mapping_cold",
                "dbghfdktyyz",
                EnglishToUkrainian,
                standard.as_slice(),
                true,
                false,
            ),
            (
                "mapping_churn",
                "qwerty",
                EnglishToUkrainian,
                standard.as_slice(),
                false,
                true,
            ),
            (
                "mapping_tiny",
                "qwerty",
                EnglishToUkrainian,
                &standard[..7],
                false,
                false,
            ),
            (
                "mapping_large",
                "і ї є ґ 🦀",
                EnglishToUkrainian,
                large.as_slice(),
                false,
                false,
            ),
        ] {
            maps::reset_mapping_cache();
            let run = |i| {
                let pairs = if churn && i % 2 == 0 {
                    alternate.as_slice()
                } else {
                    pairs
                };
                upyr_core::convert_with_mapping(black_box(input), direction, black_box(pairs))
            };
            black_box(run(0));
            let mut times = Vec::with_capacity(6000);
            let mut checksum = 0;
            for i in 0..6000 {
                if cold {
                    maps::reset_mapping_cache();
                }
                let start = Instant::now();
                let result = run(i);
                times.push(start.elapsed().as_nanos());
                for byte in result.text.bytes() {
                    hash(&mut checksum, u64::from(byte));
                }
                hash(&mut checksum, result.direction as u64);
                hash(&mut checksum, result.changed as u64);
            }
            maps::reset_mapping_cache();
            let mem = memory(|| {
                for i in 0..6000 {
                    if cold {
                        maps::reset_mapping_cache();
                    }
                    black_box(run(i));
                }
            });
            report(name, times, checksum, mem);
            println!(
                "{{\"workload\":\"{name}_cache\",\"mapping_heap_bytes\":{}}}",
                maps::mapping_heap_bytes()
            );
        }
        return;
    }
    if mode == "visible" {
        use upyr_core::InputLayout::{English, Ukrainian};
        let installed = upyr_core::default_physical_mapping();
        let custom = [('a', '🦀'), ('a', 'x'), (' ', '_'), ('і', 'i'), ('#', '№')];
        let single_english = [(English, "we can continue working with this context today")];
        let single_ukrainian = [(Ukrainian, "ghbdsn vb vj;tvj ghf[.dfnb c]jujlys")];
        let mixed = [
            (English, "we "),
            (Ukrainian, "vj;tvj "),
            (English, "continue "),
            (Ukrainian, "ghfw.dfnb "),
            (English, "today "),
            (Ukrainian, "c]jujlys "),
        ];
        let unicode = [(Ukrainian, "a і # 🚀 a’b σΣ ß"), (English, " — English")];
        for (name, runs, mapping) in [
            ("visible_english", single_english.as_slice(), None),
            ("visible_ukrainian", single_ukrainian.as_slice(), None),
            ("visible_mixed", mixed.as_slice(), None),
            (
                "visible_installed",
                mixed.as_slice(),
                Some(installed.as_slice()),
            ),
            (
                "visible_custom_unicode",
                unicode.as_slice(),
                Some(custom.as_slice()),
            ),
        ] {
            let probe = auto_correct::VisibleContextProbe::new(runs);
            black_box(probe.render(mapping));
            let mut times = Vec::with_capacity(6000);
            let mut checksum = 0;
            for _ in 0..6000 {
                let now = Instant::now();
                let rendered = probe.render(black_box(mapping));
                times.push(now.elapsed().as_nanos());
                for byte in rendered.as_ref().bytes() {
                    hash(&mut checksum, u64::from(byte));
                }
            }
            let mem = memory(|| {
                for _ in 0..6000 {
                    black_box(probe.render(black_box(mapping)));
                }
            });
            report(name, times, checksum, mem);
        }
        return;
    }
    if mode == "corpus-boundary" {
        use upyr_core::{AutoCorrectPolicy, InputLayout, Sensitivity, WordSample};
        let corpus =
            std::fs::read_to_string(".cache/upyr-benchmarks/clean-wikipedia-v1.tsv").unwrap();
        let mut words: Vec<&str> = corpus
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .step_by(20)
            .map(|line| line.split('\t').nth(2).unwrap())
            .collect();
        words.extend([
            "", "a ", "éΣ", "ІЇЄҐ", "Ghbdtn,", "Jkmuf...", "i'm", "vb", "vs",
        ]);
        let built_in = upyr_core::default_physical_mapping();
        let custom = [
            ('a', '界'),
            ('a', 'x'),
            ('b', 'і'),
            (' ', '🦀'),
            ('é', 'Σ'),
            ('Σ', 's'),
        ];
        let mut checksum = 0;
        let mut samples = 0;
        for mapping in [None, Some(built_in.as_slice()), Some(custom.as_slice())] {
            for sensitivity in [
                Sensitivity::Conservative,
                Sensitivity::Balanced,
                Sensitivity::Aggressive,
            ] {
                for layout in [InputLayout::English, InputLayout::Ukrainian] {
                    for (index, word) in words.iter().enumerate() {
                        let policy = AutoCorrectPolicy {
                            sensitivity,
                            min_word_length: if index % 11 == 0 { 5 } else { 4 },
                            exceptions: vec!["hello".into(), "привіт".into()],
                            ..Default::default()
                        };
                        for context in [
                            (*word).to_owned(),
                            format!("nj vb {word} "),
                            "unrelated context".to_owned(),
                        ] {
                            let sample = WordSample::new(*word, context, layout);
                            decision(
                                &mut checksum,
                                upyr_core::evaluate(&sample, &policy, mapping),
                            );
                            samples += 1;
                        }
                    }
                }
            }
        }
        println!(
            "{{\"workload\":\"corpus_boundary_decisions\",\"samples\":{samples},\"checksum\":{checksum}}}"
        );
        return;
    }
    if mode == "sparse" {
        black_box(core::token("warmup"));
        for size in [1, 8, 32, 128, 512, 2048] {
            let tokens: Vec<_> = (0..size).map(letters).collect();
            core::reset_caches();
            for token in &tokens {
                black_box(core::token(token));
            }
            let mut times = Vec::with_capacity(4096);
            let mut sum = 0;
            for index in 0..4096 {
                let now = Instant::now();
                let c = core::token(black_box(&tokens[index % size]));
                times.push(now.elapsed().as_nanos());
                hash(&mut sum, c.0 as u64);
            }
            let mem = memory(|| {
                for token in &tokens {
                    black_box(core::token(token));
                }
            });
            report(&format!("sparse_{size}"), times, sum, mem);
        }
        return;
    }
    if mode == "cache" || mode == "cache-long" {
        let pressure_len = if mode == "cache-long" { 131072 } else { 2048 };
        let tokens: Vec<_> = (0..(65536 + pressure_len)).map(letters).collect();
        // Warm tables/page mappings before comparing cache behavior.
        black_box(core::token("warmup"));
        core::reset_caches();
        let mut cold = Vec::with_capacity(2048);
        let mut sum = 0;
        for token in tokens.iter().take(2048) {
            let now = Instant::now();
            let c = core::token(black_box(token));
            cold.push(now.elapsed().as_nanos());
            hash(&mut sum, c.0 as u64);
        }
        let cold_mem = memory(|| {
            for token in tokens.iter().take(2048) {
                black_box(core::token(token));
            }
        });
        report("cold_tokens", cold, sum, cold_mem);
        core::reset_caches();
        for token in tokens.iter().take(65536) {
            black_box(core::token(token));
        }
        assert!(core::cache_len() <= 65536 && core::cache_len() > 32768);
        let mut pressure = Vec::with_capacity(pressure_len);
        let mut hot = Vec::with_capacity(pressure_len);
        let mut sum = 0;
        let mut first = 0;
        for (i, token) in tokens.iter().skip(65536).enumerate() {
            let now = Instant::now();
            let c = core::token(black_box(token));
            let ns = now.elapsed().as_nanos();
            if i == 0 {
                first = ns;
            }
            pressure.push(ns);
            hash(&mut sum, c.0 as u64);
            let now = Instant::now();
            let c = core::token(black_box(&tokens[32768 + i % 32768]));
            hot.push(now.elapsed().as_nanos());
            hash(&mut sum, c.0 as u64);
        }
        let timing_entries = core::cache_len();
        let mem = memory(|| {
            for token in &tokens {
                black_box(core::token(token));
            }
        });
        report("capacity_misses", pressure, sum, mem);
        report("retained_hot_tokens", hot, sum, mem);
        println!(
            "{{\"workload\":\"capacity_transition\",\"first_insert_ns\":{first},\"timing_entries_after\":{timing_entries},\"memory_entries_after\":{}}}",
            core::cache_len()
        );
        return;
    }
    if mode == "score" {
        let mut texts = Vec::new();
        let mut text = String::new();
        for word in [
            "we", "can", "continue", "working", "with", "this", "context", "today",
        ] {
            text.push_str(word);
            text.push(' ');
            texts.push(text.clone());
        }
        texts.extend([
            "we can continue working".into(),
            "we can Continue work, context!".into(),
            "ми можемо продовжити працювати".into(),
            "МИ можемо продовжити".into(),
            "ΟΣ ІЇЄҐ a’b \t42\n!!!".into(),
        ]);
        for t in &texts {
            black_box(core::score(t, false));
        }
        let mut times = Vec::with_capacity(4000);
        let mut sum = 0;
        for _ in 0..300 {
            for t in &texts {
                let now = Instant::now();
                let c = core::score(black_box(t), false);
                times.push(now.elapsed().as_nanos());
                hash(&mut sum, u64::from(c.0));
                hash(&mut sum, c.1 as u64);
            }
        }
        let mem = memory(|| {
            for _ in 0..300 {
                for t in &texts {
                    black_box(core::score(t, false));
                }
            }
        });
        report("context_scoring", times, sum, mem);
        #[cfg(target_os = "macos")]
        return;
    }
    #[cfg(target_os = "macos")]
    {
        use upyr_core::InputLayout;
        if mode == "boundary-routes" {
            let config = config::Config::default();
            auto_correct::prepare();
            let caps_cases: &[&[(&str, InputLayout)]] = &[
                &[("gHBDSN", InputLayout::English)],
                &[("hELLO", InputLayout::Ukrainian)],
                &[("ok gHBDSN", InputLayout::English)],
                &[("nfr", InputLayout::English)],
                &[("hello", InputLayout::English)],
            ];
            let mixed_cases: &[&[(&str, InputLayout)]] = &[
                &[
                    ("I ", InputLayout::Ukrainian),
                    ("have", InputLayout::English),
                ],
                &[
                    ("nj ", InputLayout::English),
                    ("vb", InputLayout::Ukrainian),
                ],
                &[
                    ("nj vb ", InputLayout::English),
                    ("ghbdsn", InputLayout::Ukrainian),
                ],
                &[
                    ("gh", InputLayout::English),
                    ("bdsn", InputLayout::Ukrainian),
                ],
                &[
                    ("we ", InputLayout::English),
                    ("can", InputLayout::Ukrainian),
                ],
            ];
            for (name, caps, enter, terminal, cases) in [
                ("caps_space", true, false, false, caps_cases),
                ("caps_enter", true, true, false, caps_cases),
                ("terminal_space", false, false, true, caps_cases),
                ("terminal_caps", true, false, true, caps_cases),
                ("mixed_space", false, false, false, mixed_cases),
                ("mixed_enter", false, true, false, mixed_cases),
            ] {
                let run = |mut timing: Option<&mut Vec<u128>>| {
                    let mut checksum = 0;
                    for _ in 0..300 {
                        for segments in cases {
                            let mut tracker = auto_correct::AutoWordTracker::default();
                            for (text, layout) in *segments {
                                tracker.set_source_layout(Some(*layout));
                                for c in text.chars() {
                                    black_box(tracker.observe(key(c)));
                                }
                            }
                            let start = Instant::now();
                            let result = if enter {
                                let sample = tracker.line_break_sample().unwrap();
                                auto_correct::evaluate_line_break(&sample, &config, caps)
                            } else {
                                let sample = tracker.observe(key(' ')).unwrap();
                                auto_correct::evaluate_with_caps_lock(
                                    &sample, &config, caps, terminal,
                                )
                            };
                            let elapsed = start.elapsed().as_nanos();
                            if let Some(times) = timing.as_mut() {
                                times.push(elapsed);
                            }
                            decision(&mut checksum, result);
                        }
                    }
                    checksum
                };
                black_box(run(None));
                let mut times = Vec::with_capacity(1500);
                let checksum = run(Some(&mut times));
                let mem = memory(|| assert_eq!(run(None), checksum));
                report(name, times, checksum, mem);
            }
            return;
        }
        let cold = mode == "cold-boundary";
        let config = config::Config::default();
        auto_correct::prepare();
        let cases = [
            ("we can continue", InputLayout::English),
            ("nj vb dbghfdktyyz", InputLayout::English),
            ("I have a question", InputLayout::English),
            ("Ghbdtn", InputLayout::English),
            ("cont", InputLayout::Ukrainian),
            ("nj vb", InputLayout::English),
            ("Vbh", InputLayout::English),
            ("ghbdtn,", InputLayout::English),
            ("fastest as proof of", InputLayout::English),
        ];
        for enter in [false, true] {
            let run = |mut timing: Option<&mut Vec<u128>>| {
                let mut sum = 0;
                for _ in 0..300 {
                    for (text, layout) in cases {
                        if cold {
                            core::reset_caches();
                        }
                        let mut tracker = auto_correct::AutoWordTracker::default();
                        tracker.set_source_layout(Some(layout));
                        let mut words = text.split(' ').peekable();
                        while let Some(word) = words.next() {
                            for c in word.chars() {
                                black_box(tracker.observe(key(c)));
                            }
                            let now = Instant::now();
                            let result = if enter {
                                let sample = tracker.line_break_sample().unwrap();
                                auto_correct::evaluate_line_break(&sample, &config, false)
                            } else {
                                let sample = tracker.observe(key(' ')).unwrap();
                                auto_correct::evaluate_with_caps_lock(
                                    &sample, &config, false, false,
                                )
                            };
                            let elapsed = now.elapsed().as_nanos();
                            if let Some(times) = timing.as_mut() {
                                times.push(elapsed);
                            }
                            decision(&mut sum, result);
                            if enter && words.peek().is_some() {
                                black_box(tracker.observe(key(' ')));
                            }
                        }
                    }
                }
                sum
            };
            black_box(run(None));
            let mut times = Vec::with_capacity(20000);
            let checksum = run(Some(&mut times));
            let mem = memory(|| {
                assert_eq!(run(None), checksum);
            });
            report(
                match (cold, enter) {
                    (false, false) => "desktop_space",
                    (false, true) => "desktop_enter",
                    (true, false) => "cold_desktop_space",
                    (true, true) => "cold_desktop_enter",
                },
                times,
                checksum,
                mem,
            );
        }
    }
}
