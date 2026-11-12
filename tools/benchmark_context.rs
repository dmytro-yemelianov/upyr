//! rustc --edition=2024 -O tools/benchmark_context.rs --extern
//! upyr_core=target/release/libupyr_core.rlib -L dependency=target/release/deps
//! Measures only the added context evaluator, excluding event handling/editing.
use std::{hint::black_box, time::Instant};

fn main() {
    let policy = upyr_core::AutoCorrectPolicy::default();
    let groups: [(&str, &[&str]); 4] = [
        ("single_word", &["have ", "question ", "тепер ", "працювати "]),
        ("native_pairs", &["I have ", "то ми ", "fastest as ", "proof of ", "vs you ", "NJ VB "]),
        ("unsupported_pairs", &["Ш zzzzz ", "nj zzzzz ", "nj vs ", "Ш have! "]),
        ("corrections", &["Ш have ", "Ш рфму ", "nj vb ", "nj vb тепер "]),
    ];
    for (name, inputs) in groups {
        for _ in 0..1000 {
            for input in inputs { black_box(upyr_core::evaluate_context(input, &policy, None)); }
        }
        let mut samples = Vec::new();
        let mut accepted = 0usize;
        for _ in 0..7 {
            let start = Instant::now();
            for _ in 0..25000 {
                for input in inputs {
                    accepted += usize::from(black_box(upyr_core::evaluate_context(black_box(input), black_box(&policy), None)).is_some());
                }
            }
            samples.push(start.elapsed().as_nanos() as f64 / (25000 * inputs.len()) as f64);
        }
        samples.sort_by(f64::total_cmp);
        println!("{name}: median_ns={:.2}, min_ns={:.2}, max_ns={:.2}, accepted={}", samples[3], samples[0], samples[6], black_box(accepted));
    }
}
