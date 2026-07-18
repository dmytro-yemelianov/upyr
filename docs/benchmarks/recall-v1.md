# Recall v1 evaluation

Date: 2026-07-18
Corpus: `crates/upyr-core/fixtures/recall/{ukrainian,english}-words-v1.txt`
Runner: `cargo test -p upyr-core recall_v1 -- --nocapture`

The boundary replay (`signed-ngram-v1.md`) and the external clean holdout measure
**precision** and **false corrections**. Neither measures **recall** — how often a
genuinely wrong-layout word is actually corrected. This benchmark fills that gap:
every word is materialized as the physical key sequence a user would press with
the wrong layout active, then scored through production `evaluate`.

- **Ukrainian words** (1,200) are frequency-ranked from the `ukr-proverbs-corpus`
  dataset, typed on an English layout.
- **English words** (1,000) are an alphabetic-spread sample of the system
  dictionary, typed on a Ukrainian layout.

A case is a hit only when the decision is `Correct` and the replacement matches
the intended word.

## Result

| Profile | UK → EN | EN → UK |
|---|---:|---:|
| Conservative (default) | **75.1%** (901/1200) | **94.3%** (943/1000) |
| Conservative + built-in triggers | 75.1% (901/1200) | — |
| Aggressive, `min_word_length = 2` | 92.3% (1108/1200) | 99.4% (994/1000) |

### By word length (default profile, UK → EN)

| Length | Recall |
|---|---:|
| ≤ 3 chars | **0.0%** |
| 4–6 chars | 93.1% |
| 7+ chars | 98.5% |

## Reading

1. **The default profile abstains on every short word.** `min_word_length = 4`
   means the most frequent words (`не`, `як`, `на`, `що`, …) are never corrected —
   the single largest recall hole. Lowering the floor and using the aggressive
   thresholds recovers them (76% short-word recall) with no model change.
2. **The model itself is strong.** Aggressive reaches 92% / 99%, so the ~25% of
   Ukrainian words missed by default is a *policy* choice (precision over recall),
   not a model limitation.
3. **Built-in triggers do not move broad recall.** The 12-entry seed is targeted,
   not coverage. Broad recall needs a real dictionary (the current exact-match set
   is 296 stop-words) and/or a less conservative default; triggers exist for
   high-precision special cases and user/domain vocabulary.

## Floors

`recall_v1_baseline_floor` enforces: UK → EN default ≥ 70%, EN → UK default ≥ 90%,
triggers never reduce recall, and aggressive strictly beats the default in both
directions. These guard against regressions while the recall-raising work
(dictionary, default tuning) proceeds.
