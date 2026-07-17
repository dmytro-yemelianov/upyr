# Signed N-gram v1 evaluation

Date: 2026-07-17  
Boundary corpus: `crates/upyr-core/fixtures/boundary-replay-v1.tsv`  
Boundary corpus SHA-256: `d66ab5071d6fa94cc5dfd820eda22c50fa648590b567d4472807477c5e462819`  
Production model SHA-256: `f1b91c8bafb6fc119eeb4e8eaf531b5c47c21a6059c031dac63410baf3656924`

This report freezes the decision boundary before a different scorer family is
allowed to replace the signed character N-gram model. Every committed row
materializes the physical word and retained context, active source layout,
visible source span, intended replacement, mapping profile, and expected
action. The test does not regenerate those fields through production mapping
code.

The selected training revision uses pinned 1,000,000-sentence English and
Ukrainian Leipzig news corpora, a 1.75× retained-gram budget, and minimum
quantized strength 32. Running the generator with no options reproduces it:

```sh
python3 tools/generate_ngram_model.py
cargo test -p upyr-core signed_ngram_v1_boundary_replay -- --nocapture
```

## Production result

| Metric | Result |
|---|---:|
| Materialized boundary cases | 191 |
| Expected corrections | 90 |
| Exact corrections | 90 |
| Expected keeps | 101 |
| False corrections | 0 |
| Wrong replacement/span/direction | 0 |
| Precision | 1.000 |
| Recall | 1.000 |

| Slice | Exact / positive | False / negative |
|---|---:|---:|
| English → Ukrainian | 41 / 41 | 0 / 0 |
| Ukrainian → English | 49 / 49 | 0 / 0 |
| Calibration | 21 / 21 | 0 / 23 |
| Evaluation | 52 / 52 | 0 / 64 |
| User regression | 17 / 17 | 0 / 14 |

| Category | Exact / positive | False / negative |
|---|---:|---:|
| Native English | 0 / 0 | 0 / 20 |
| Native Ukrainian | 0 / 0 | 0 / 28 |
| Wrong English → Ukrainian | 24 / 24 | 0 / 0 |
| Wrong Ukrainian → English | 20 / 20 | 0 / 0 |
| Proper names | 8 / 8 | 0 / 8 |
| Punctuation / physical mapping | 13 / 13 | 0 / 7 |
| Technical native | 0 / 0 | 0 / 25 |
| Technical wrong-layout context | 14 / 14 | 0 / 0 |
| Contextual phrases | 10 / 10 | 0 / 0 |
| Intentional short-word abstention | 0 / 0 | 0 / 12 |
| Reported regressions | 1 / 1 | 0 / 1 |

Lexical groups stay in one split, so native and wrong-layout variants cannot
leak across calibration and evaluation. User-reported cases remain in the
regression split.

## External clean holdout

The optional clean-safety gate samples 10,000 lowercase English and 10,000
lowercase Ukrainian words with frequency at least two from pinned Wikipedia
corpora. This is a different genre from the news training data. Corpora and the
generated 944 kB (922 KiB) TSV remain under `.cache`; neither is committed or
embedded in the application.

```sh
python3 tools/generate_clean_holdout.py
UPYR_CLEAN_HOLDOUT=.cache/upyr-benchmarks/clean-wikipedia-v1.tsv \
  cargo test -p upyr-core signed_ngram_v1_external_clean_holdout \
  -- --ignored --nocapture
```

Generated holdout SHA-256:
`ac08f185fe60864a9f2fbb3452f96388d9eed00e51c64d9e57bcb25ee70382be`.

| Metric | Result |
|---|---:|
| Clean English boundaries | 10,000 |
| Clean Ukrainian boundaries | 10,000 |
| False corrections | 0 |
| Normalized false corrections / 10k | 0.0 |

This is a much stronger safety check than the curated negative set, but it is
still a deterministic corpus sample rather than a real-world incidence claim.
Technical, mixed-language, host-specific, and event-sequence holdouts remain
separate work.

## Selection history

The original 100K model scored 89/90 on the expanded boundary corpus. A naive
1M replacement at the same capacity dropped to 88/90, proving that more corpus
alone was not sufficient. Increasing only quantized strength restored 89/90;
the selected 1M/1.75×/32 model reached 90/90 while retaining zero clean-holdout
false corrections.

The safety run also exposed four native Ukrainian collisions. Three came from
applying the Ukrainian physical-letter relaxation in the reverse direction;
the fourth was a genuinely plausible pair (`дупу` versus physical `lege`). The
policy now applies punctuation assistance only toward Ukrainian letters and
requires an extra margin when both language candidates are plausible. The four
cases are promoted into the committed regression corpus.

| Artifact | Entries | Raw | gzip -9 | Brotli 11 |
|---|---:|---:|---:|---:|
| Original 100K | about 100k | 1,701,814 B | 370,930 B | 224,432 B |
| Selected 1M | 173,964 | 2,957,400 B | 619,230 B | 370,861 B |

Binary-search inference gains only about one comparison at this size. The web
artifact pays approximately 146 KB more with Brotli; this remains below the
planned lazy-loaded model budget.

## Scorer boundary and next model

Production `evaluate` now delegates to a private pairwise `CandidateScorer`.
The current model independently scores source and target candidates through
that contract; injected-scorer tests cover ordinary and terminal-delimiter
paths. An ignored runner can evaluate any generated compatible `.ngm` artifact
against exactly the same corpus:

```sh
UPYR_CANDIDATE_MODEL=.cache/upyr-models/candidate.ngm \
  cargo test -p upyr-core candidate_ngram_boundary_replay \
  -- --ignored --nocapture
```

The artifact format is still signed N-gram v1. N-gram v2 should add independent
language/background plausibility and calibrated pairwise action evidence; it
must beat these correction and clean-safety gates before becoming production.

## Scope

The committed corpus evaluates scorer and policy behavior at a Space boundary.
It includes shifted punctuation, the built-in Ukrainian mapping, and the
reported macOS mapping where physical `\` produces `ʼ`. It does not replace an
event-sequence suite: navigation, unsupported keys, layout changes, correction
timing, tracker resets, and post-correction layout state must be replayed as
physical key events in the next benchmark layer.
