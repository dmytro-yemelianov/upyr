# Short-word context correction

The desktop adapter can delay an unresolved short token until the following
word ends. `nj vb ` becomes `то ми `; both `Ш рфму ` and `Ш have ` become
`I have `. Space commits the decision. In ordinary editors, the existing held
Enter path can also correct the completed pair before submission. Terminal
mode requires a trailing Space and never synthesizes Enter.

The existing physical-key buffer retains the unresolved tail across a layout
change between words, including Globe switching. Actual visible layout runs,
not one assumed layout for the entire sentence, supply the candidates. Early
correction waits while a short token is pending, so it cannot discard that
token by correcting only the next prefix. No state is retained after an edit,
focus/reset/navigation/shortcut event or repeated Space. A switch inside a
word retains the existing conservative handling. The old single-language
evaluator only sees the current word when previous words used another layout.

This is bounded look-ahead, not semantic understanding. At most two leading
short words and one anchor can be rewritten, totaling at most 64 characters.
Every leading target is an explicit one-to-three-letter function word. The
first word must change. All target words share one script, while already
correct anchor words are preserved. Every adjacent target pair must occur at
least 16 times for Conservative sensitivity (8 for Balanced/Aggressive) and
exceed the retained source-pair count by a fixed 16-fold margin.
Absent source pairs mean no retained evidence, not proof of impossibility.

Known source words, single Latin letters, short Latin identifiers, uppercase
abbreviations, `vs`, user exceptions and Keep rules take precedence. `I` must
have its proper uppercase spelling; lowercase `ш` does not become `i`. A
configured minimum length above four disables this short-word relaxation.
Punctuation, multiple spaces and mixed-script words are not context candidates.
Some valid text or mistakes will remain ambiguous; no universal correction
claim is made. Deliberate EN/UK switching is tested separately.

## Data and lookup

`assets/models/context.bgm` in `upyr-core` contains **102,767 bigrams** over
34,777 word forms, using **1,772,768 bytes**. These are generated from the
same pinned CC BY 4.0 Leipzig English/Ukrainian 2023 1M news archives credited
in the third-party notices. Only whitespace-adjacent words are counted; pairs
do not cross punctuation or invalid language tokens. Normalization lowercases
and normalizes apostrophes. The left word must belong to the reviewed
`assets/context-short-words.txt` list. Counts below eight are omitted.

Format `UPYRBG1`: 20-byte little-endian header (magic, word count, pair count,
UTF-8 pool byte count), `word_count + 1` u32 offsets, a sorted UTF-8 word pool,
and sorted 12-byte `(left_id << 32 | right_id, count)` records. Both lookups
use binary search over embedded read-only bytes. There is no runtime table
construction, probabilistic hash or network lookup. Decision candidates use
bounded temporary strings. Input hashes, rules and output hash are recorded
in `context.json`.

Reproduce or verify:

```sh
python3 tools/generate_context_model.py --check
python3 -m unittest tools.test_generate_context_model
```

## Validation

Every sentence with `id % 100 == 0` is excluded before training. Its 271,217
native adjacent pairs produced **zero false context corrections**. This is
a sentence partition of the same news sources, not an independent-domain
holdout; common word pairs naturally occur in both partitions.

Among 84,286 held-out pairs with an eligible leading short word, synthetic
keyboard-layout corruption was recovered in 56,248 cases with both words
corrupted and 61,198 cases with only the leading word corrupted. These are
isolated context decisions, not end-to-end accuracy or latency guarantees.
Abbreviations, valid-source collisions and unsupported pairs abstain.

```sh
python3 tools/generate_context_model.py --holdout .cache/upyr-benchmarks/context-heldout.tsv
UPYR_CONTEXT_HOLDOUT="$PWD/.cache/upyr-benchmarks/context-heldout.tsv" \
  cargo test --release --locked --offline -p upyr-core context_external_native_pairs -- --ignored --nocapture
```

Host replay exercises the actual tracker, early/boundary decisions, simulated
layout changes, exact selected suffix and terminal forward/undo plans. It
covers the reported examples, native mixed sentences, abbreviations, manual
switching, Backspace, navigation, exceptions and a delayed early edit. Input
generation, focus and expected-text guards continue to control actual edits.
