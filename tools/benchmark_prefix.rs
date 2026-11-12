//! Build/run via tools/benchmark_prefix.sh. Timings exclude fixture allocation.
#![allow(dead_code)]
#[path = "../crates/upyr-core/src/auto_correct/prefix_lexicon.rs"]
mod prefix_lexicon;

use std::{hint::black_box, time::Instant};
use upyr_core::{AutoCorrectPolicy, AutoDecision, InputLayout, WordSample};

struct Query {
    word: String,
    reset: bool,
    sample: WordSample,
}

fn add_word(queries: &mut Vec<Query>, physical: &str, layout: InputLayout, editing: bool) {
    let mut word = String::new();
    let mut reset = true;
    let mut emit = |word: &str| {
        queries.push(Query {
            word: word.to_owned(),
            reset,
            sample: WordSample::new(word, word, layout),
        });
        reset = false;
    };
    for c in physical.chars() {
        word.push(c);
        emit(&word);
    }
    if editing {
        for _ in 0..3 {
            word.pop();
            emit(&word);
        }
        // Enter a dead branch, recover with Backspace, then continue typing.
        word.push('?');
        emit(&word);
        word.pop();
        emit(&word);
        word.push('e');
        emit(&word);
        word.pop();
        emit(&word);
        for c in physical.chars().skip(word.len()).collect::<Vec<_>>() {
            word.push(c);
            emit(&word);
        }
    }
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("path to clean Wikipedia TSV");
    let contents = std::fs::read_to_string(path).unwrap();
    let entries = contents
        .lines()
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .map(|s| s.split('\t').collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let policy = AutoCorrectPolicy::default();
    for editing in [false, true] {
        let mut queries = Vec::new();
        for entry in &entries {
            let layout = if entry[1] == "english" {
                InputLayout::English
            } else {
                InputLayout::Ukrainian
            };
            add_word(&mut queries, entry[2], layout, editing);
        }
        let mut samples = Vec::new();
        for entry in entries.iter().step_by(20) {
            for layout in [InputLayout::English, InputLayout::Ukrainian] {
                add_word(&mut samples, entry[2], layout, editing);
            }
        }
        #[cfg(prefix_incremental)]
        let mut cursor = prefix_lexicon::Cursor::default();
        #[cfg(prefix_incremental)]
        let mut state = upyr_core::PartialWordState::default();
        // Warm lazy dictionaries and the production evaluation path.
        for query in &samples {
            black_box(upyr_core::evaluate_partial(&query.sample, &policy, None));
        }
        let mut index_times = Vec::new();
        let mut pipeline_times = Vec::new();
        let mut index_checksum = 0u64;
        let mut pipeline_checksum = 0u64;
        for _ in 0..7 {
            index_checksum = 0;
            let start = Instant::now();
            for _ in 0..3 {
                for query in &queries {
                    #[cfg(prefix_incremental)]
                    let mask = {
                        if query.reset {
                            cursor.reset();
                        }
                        cursor.languages(black_box(&query.word))
                    };
                    #[cfg(not(prefix_incremental))]
                    let mask = prefix_lexicon::languages(black_box(&query.word));
                    index_checksum = index_checksum
                        .wrapping_mul(31)
                        .wrapping_add(black_box(mask) as u64);
                }
            }
            index_times.push(start.elapsed().as_nanos() as f64 / (queries.len() * 3) as f64);
            pipeline_checksum = 0;
            let start = Instant::now();
            for _ in 0..3 {
                for query in &samples {
                    #[cfg(prefix_incremental)]
                    let decision = {
                        if query.reset {
                            state.reset();
                        }
                        state.evaluate(black_box(&query.sample), &policy, None)
                    };
                    #[cfg(not(prefix_incremental))]
                    let decision =
                        upyr_core::evaluate_partial(black_box(&query.sample), &policy, None);
                    let value = match black_box(decision) {
                        AutoDecision::Continue => 0,
                        AutoDecision::Reset => 1,
                        AutoDecision::Correct(c) => 2 + c.replacement.len() as u64,
                    };
                    pipeline_checksum = pipeline_checksum.wrapping_mul(31).wrapping_add(value);
                }
            }
            pipeline_times.push(start.elapsed().as_nanos() as f64 / (samples.len() * 3) as f64);
        }
        index_times.sort_by(f64::total_cmp);
        pipeline_times.sort_by(f64::total_cmp);
        let mode = if cfg!(prefix_incremental) {
            "incremental"
        } else {
            "root"
        };
        let workload = if editing { "editing" } else { "typing" };
        #[cfg(prefix_incremental)]
        let state_bytes = std::mem::size_of::<upyr_core::PartialWordState>();
        #[cfg(not(prefix_incremental))]
        let state_bytes = 0;
        println!(
            "{{\"mode\":\"{mode}\",\"workload\":\"{workload}\",\"index_queries\":{},\"pipeline_queries\":{},\"index_median_ns\":{:.2},\"pipeline_median_ns\":{:.2},\"index_checksum\":{index_checksum},\"pipeline_checksum\":{pipeline_checksum},\"state_bytes\":{state_bytes}}}",
            queries.len() * 3,
            samples.len() * 3,
            index_times[3],
            pipeline_times[3]
        );
    }
}
