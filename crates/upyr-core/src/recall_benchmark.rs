//! Recall benchmark: the fraction of real wrong-layout words the engine actually
//! fixes. This complements the precision-focused boundary replay and clean
//! holdout, which measure *false* corrections but never recall — the metric a
//! user feels when a mistyped word is left uncorrected.
//!
//! Corpus: Ukrainian words are frequency-ranked from the `ukr-proverbs-corpus`
//! dataset; English words are an alphabetic-spread sample of the system
//! dictionary. Each word is materialized as the physical key sequence a user
//! would press with the wrong layout active, then scored through production
//! `evaluate`.

use crate::{
    AutoCorrectPolicy, AutoDecision, Direction, InputLayout, Sensitivity, WordSample,
    builtin_triggers, convert, evaluate,
};

const UKRAINIAN: &str = include_str!("../fixtures/recall/ukrainian-words-v1.txt");
const ENGLISH: &str = include_str!("../fixtures/recall/english-words-v1.txt");

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphabetic() || *c == '\'')
        .collect::<String>()
        .to_lowercase()
}

fn words(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Outcome of replaying one word list typed on the *wrong* layout.
struct RecallRun {
    /// Corrected to exactly the intended word.
    hit: usize,
    /// Corrected, but to something other than the intended word. Unlike an
    /// abstention this damages text, so it is tracked separately from misses.
    wrong: Vec<String>,
    total: usize,
}

/// Recall over `words` of the intended language typed on the *wrong* layout.
fn recall(words: &[String], layout: InputLayout, policy: &AutoCorrectPolicy) -> RecallRun {
    let mut hit = 0;
    let mut wrong = Vec::new();
    for word in words {
        let physical = match layout {
            // Ukrainian intended, English layout active.
            InputLayout::English => convert(word, Direction::UkrainianToEnglish).text,
            // English intended, Ukrainian layout active.
            InputLayout::Ukrainian => word.clone(),
        };
        let sample = WordSample::new(physical.clone(), physical, layout);
        if let AutoDecision::Correct(correction) = evaluate(&sample, policy, None) {
            if norm(&correction.replacement) == norm(word) {
                hit += 1;
            } else {
                wrong.push(format!("{word} -> {}", correction.replacement.trim_end()));
            }
        }
    }
    RecallRun {
        hit,
        wrong,
        total: words.len(),
    }
}

fn pct(hit: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        100.0 * hit as f64 / total as f64
    }
}

#[test]
fn recall_v1_baseline_floor() {
    let uk = words(UKRAINIAN);
    let en = words(ENGLISH);
    let default = AutoCorrectPolicy::default();
    let with_triggers = AutoCorrectPolicy {
        triggers: builtin_triggers(),
        ..AutoCorrectPolicy::default()
    };
    let aggressive = AutoCorrectPolicy {
        sensitivity: Sensitivity::Aggressive,
        min_word_length: 2,
        ..AutoCorrectPolicy::default()
    };

    let uk_default = recall(&uk, InputLayout::English, &default);
    let uk_triggers = recall(&uk, InputLayout::English, &with_triggers);
    let uk_aggressive = recall(&uk, InputLayout::English, &aggressive);
    let en_default = recall(&en, InputLayout::Ukrainian, &default);
    let en_triggers = recall(&en, InputLayout::Ukrainian, &with_triggers);
    let en_aggressive = recall(&en, InputLayout::Ukrainian, &aggressive);

    for (label, run) in [
        ("UK->EN default  ", &uk_default),
        ("UK->EN +triggers", &uk_triggers),
        ("UK->EN aggressive", &uk_aggressive),
        ("EN->UK default  ", &en_default),
        ("EN->UK +triggers", &en_triggers),
        ("EN->UK aggressive", &en_aggressive),
    ] {
        println!(
            "recall {label} {:.1}% ({}/{}) wrong={}",
            pct(run.hit, run.total),
            run.hit,
            run.total,
            run.wrong.len()
        );
        for wrong in &run.wrong {
            println!("  wrong: {wrong}");
        }
    }

    // Regression floors (baseline measured 2026-07-18; see docs/benchmarks/recall-v1.md).
    assert!(
        pct(uk_default.hit, uk_default.total) >= 70.0,
        "UK->EN default recall regressed"
    );
    assert!(
        pct(en_default.hit, en_default.total) >= 90.0,
        "EN->UK default recall regressed"
    );
    // A wrong correction damages text, unlike an abstention. The remaining
    // default-profile case (`головою` -> `голово.`) is a genuine collision:
    // `голово` is the vocative of `голова`.
    assert!(
        uk_default.wrong.len() <= 1 && uk_triggers.wrong.len() <= 1,
        "UK->EN wrong corrections regressed"
    );
    assert!(
        en_default.wrong.is_empty() && en_triggers.wrong.is_empty(),
        "EN->UK wrong corrections regressed"
    );
    assert!(
        uk_aggressive.wrong.len() <= 6 && en_aggressive.wrong.is_empty(),
        "aggressive wrong corrections regressed"
    );
    // Triggers may only add recall, never remove it.
    assert!(uk_triggers.hit >= uk_default.hit, "triggers reduced recall");
    // The aggressive profile must recover the short-word segment the default
    // deliberately abstains on.
    assert!(
        uk_aggressive.hit > uk_default.hit && en_aggressive.hit > en_default.hit,
        "aggressive mode should raise recall over the conservative default"
    );
}
