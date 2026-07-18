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

/// Recall over `words` of the intended language typed on the *wrong* layout.
fn recall(words: &[String], layout: InputLayout, policy: &AutoCorrectPolicy) -> (usize, usize) {
    let mut hit = 0;
    for word in words {
        let physical = match layout {
            // Ukrainian intended, English layout active.
            InputLayout::English => convert(word, Direction::UkrainianToEnglish).text,
            // English intended, Ukrainian layout active.
            InputLayout::Ukrainian => word.clone(),
        };
        let sample = WordSample::new(physical.clone(), physical, layout);
        if matches!(
            evaluate(&sample, policy, None),
            AutoDecision::Correct(ref correction) if norm(&correction.replacement) == norm(word)
        ) {
            hit += 1;
        }
    }
    (hit, words.len())
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

    let (uk_default, uk_total) = recall(&uk, InputLayout::English, &default);
    let (uk_triggers, _) = recall(&uk, InputLayout::English, &with_triggers);
    let (uk_aggressive, _) = recall(&uk, InputLayout::English, &aggressive);
    let (en_default, en_total) = recall(&en, InputLayout::Ukrainian, &default);
    let (en_aggressive, _) = recall(&en, InputLayout::Ukrainian, &aggressive);

    println!(
        "recall UK->EN default     {:.1}% ({uk_default}/{uk_total})",
        pct(uk_default, uk_total)
    );
    println!(
        "recall UK->EN +triggers   {:.1}% ({uk_triggers}/{uk_total})",
        pct(uk_triggers, uk_total)
    );
    println!(
        "recall UK->EN aggressive  {:.1}% ({uk_aggressive}/{uk_total})",
        pct(uk_aggressive, uk_total)
    );
    println!(
        "recall EN->UK default     {:.1}% ({en_default}/{en_total})",
        pct(en_default, en_total)
    );
    println!(
        "recall EN->UK aggressive  {:.1}% ({en_aggressive}/{en_total})",
        pct(en_aggressive, en_total)
    );

    // Regression floors (baseline measured 2026-07-18; see docs/benchmarks/recall-v1.md).
    assert!(
        pct(uk_default, uk_total) >= 70.0,
        "UK->EN default recall regressed"
    );
    assert!(
        pct(en_default, en_total) >= 90.0,
        "EN->UK default recall regressed"
    );
    // Triggers may only add recall, never remove it.
    assert!(uk_triggers >= uk_default, "triggers reduced recall");
    // The aggressive profile must recover the short-word segment the default
    // deliberately abstains on.
    assert!(
        uk_aggressive > uk_default && en_aggressive > en_default,
        "aggressive mode should raise recall over the conservative default"
    );
}
