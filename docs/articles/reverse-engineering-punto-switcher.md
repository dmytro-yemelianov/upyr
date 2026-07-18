# Reverse-engineering Punto Switcher, and what a clean-room rebuild taught me

*How Upyr reconstructs the "you typed that in the wrong keyboard layout" trick for
English ↔ Ukrainian — the two-layer design borrowed from Punto Switcher, the part
that turned out to be the actual hard problem, and the benchmark findings that
contradicted my own intuitions.*

---

## The problem in one line

You mean to type `привіт`, but the Ukrainian layout wasn't active, so the keys land
as `ghbdsn`. Or you mean `hello` and get `руддщ`. Every Slavic-language desktop user
does this a hundred times a day. [Punto Switcher](https://en.wikipedia.org/wiki/Punto_Switcher)
— Yandex's long-running Russian tool — made its name by fixing it automatically: watch
what gets typed, decide it's wrong-layout gibberish, and silently retype it in the other
layout.

Upyr is a clean-room reimplementation of that behaviour in Rust, for English ↔ Ukrainian,
that shares no code with Punto or with the other prior art it studied
([KeyboardSwitch](https://github.com/TolikPylypchuk/KeyboardSwitch)). This is an account
of what had to be reverse-engineered from *behaviour* — because the interesting parts were
never documented — and what a deliberately harsh benchmark said once it worked.

The two examples above are not hand-waving; they round-trip through Upyr's actual physical
mapping table:

```
ghbdsn → привіт
[ks,   → хліб
```

That second one is the whole story in four characters. Hold that thought.

## What was actually there to reverse-engineer

The layout conversion itself is trivial. US-QWERTY and Ukrainian ЙЦУКЕН are both fixed
positional layouts, so the mapping is a static table:

```
English:    qwertyuiop[]asdfghjkl;'zxcvbnm,./`\
Ukrainian:  йцукенгшщзхїфівапролджєячсмитьбю.'ґ
```

If reversing keystrokes were the problem, this would be a fifty-line program. It isn't.
**The hard problem is the decision, not the transformation:** given a word the user just
finished, should you leave it alone or silently rewrite it? Get it wrong in the
correcting direction and you have corrupted text the user *did* mean. That failure — a
"false correction" — is the one that makes people uninstall the tool, because it destroys
trust in a way that a missed correction never does.

Punto's answer, learned from watching how it behaves rather than from any spec, is a
**two-layer decision**:

1. A small table of **deterministic triggers** — physical key sequences whose intended
   layout is unambiguous — consulted first, allowed to override everything.
2. A **statistical language model** for everything else, tuned to abstain when unsure.

Upyr rebuilds both layers. The trigger layer is modelled directly on Punto's `triggers.dat`
— the source comment says so in as many words — and the statistical layer is a signed
character n-gram scorer trained from scratch on public corpora. The rest of this article
is about the three things that made the rebuild non-obvious.

## Insight 1: model the *physical keys*, not the characters

The naive design scores "is `ghbdsn` more English-like or Ukrainian-like?" That is already
wrong, and `[ks,` shows why.

On a Ukrainian layout, the physical keys `[ ] ; ' \ , . /` are **letters**
(`х ї ж є ґ б ю .`), not punctuation. So the four physical keys `[ k s ,` are not "a bracket,
two letters, and a comma" — they are the Ukrainian word `хліб` (bread). A character-level
model looking at the visible string `[ks,` sees punctuation-riddled noise and refuses to
touch it. The correct interpretation is a perfectly ordinary common word.

Upyr's tracker therefore works in a **layout-independent physical-key space** (its key names
deliberately mirror the browser `KeyboardEvent.code` values), and the scorer is asked about
the *intended* candidate — the word those keys would produce in the other layout — not the
bytes currently on screen. This one reframing is what lets `[ks, → хліб` work while still
leaving a genuine `Jkmuf, → Ольга,` with its trailing comma intact.

It also creates a landmine, which becomes Finding 3.

## Insight 2: triggers exist to cover the model's deliberate blind spot

The statistical model is tuned to **abstain under uncertainty** — precision over recall,
because false corrections are the unforgivable failure. A direct consequence: it will not
correct short words. There simply isn't enough signal in three characters to clear a
confidence bar set high enough to be safe. And short words are exactly the most frequent
words in any language (`не`, `як`, `на`, `що`…).

That is what the trigger table is *for*. It is not a general dictionary — the seed is
twelve entries. Each one is a specific physical sequence whose intended layout is not in
doubt, given a deterministic path to correct (or to be explicitly preserved) that bypasses
the model's confidence threshold and minimum-length floor entirely. Triggers are also
source-layout-aware: a "correct this" rule for a Ukrainian word typed on the English layout
must never fire against the already-correct Ukrainian text those same physical keys produce
on the Ukrainian layout.

The mental model that took longest to arrive: **the model and the triggers are not
redundant.** The model is a high-precision instrument that is honest about not knowing.
The triggers are where you encode the cases you *do* know for certain, including the ones
the model is structurally designed to miss.

## Insight 3: the scorer, and how it's kept honest

The production scorer is a **signed character n-gram model**. It learns, per language, which
character sequences are plausible, and scores a candidate word by how well its n-grams cover
known-good sequences. "Signed" and "quantized strength" are the knobs; the artifact is a
~2.9 MB table of ~174,000 retained grams, trained from pinned 1-million-sentence English and
Ukrainian Leipzig news corpora.

Two policy rules sit on top of the raw score, and both exist because the benchmark caught a
real failure (below):

- The **relaxed threshold** that helps physical punctuation keys become Ukrainian letters
  runs in *one direction only*. Let it run in reverse and ordinary Ukrainian words like
  `рубці` get mistaken for punctuation-heavy English gibberish like `he,ws`.
- When *both* language interpretations already look natural, the pair is a genuine keyboard
  collision, not a clear wrong-layout signal. The policy then demands an extra half-margin
  of advantage rather than rewriting text on a narrow win.

## The findings

Two benchmark layers guard the behaviour. Both freeze their corpora by SHA-256 so a result
is reproducible rather than a vibe.

### Finding A — at the decision boundary, precision and recall both hit 1.000

The signed-n-gram v1 boundary corpus is 191 hand-materialized cases — 90 that must correct,
101 that must be left alone — spanning wrong-layout words both directions, native words in
both languages, proper names, punctuation/physical-mapping edge cases, and technical tokens.

| Metric | Result |
|---|---:|
| Cases | 191 |
| Exact corrections | 90 / 90 |
| False corrections | **0** |
| Precision | **1.000** |
| Recall | 1.000 |

This is the number that matters most, because the 101 "keep" cases include the traps:
`FAANG`, `SaaS`, `NASDAQ`, native words that happen to look cross-layout-plausible. Zero
false corrections on that set is the whole point of the tool.

### Finding B — "more data" made it *worse*, and that was the useful part

The intuition going in was "bigger corpus → better model." The record says otherwise:

- Original 100K-sentence model: **89/90** on the boundary set.
- Naive 1M-sentence model at the same capacity: **88/90** — *worse*.
- Increasing only the quantized-strength floor: back to 89/90.
- The selected 1M / 1.75× / 32 configuration: **90/90**, with zero clean-holdout false
  corrections.

More corpus alone was actively counterproductive; the gain came from capacity and
quantization tuning, not volume. This is the kind of thing you only learn by holding the
evaluation fixed and changing one variable at a time — and it's why the benchmark exists
before the "obvious" upgrade, not after.

### Finding C — the safety net caught four real collisions

An external clean-safety gate runs 10,000 English + 10,000 Ukrainian ordinary words —
sampled from Wikipedia, a deliberately *different genre* from the news training data — and
counts how many get falsely corrected. Result: **0 / 20,000.**

But an earlier run surfaced four native Ukrainian collisions. Three were the one-directional
relaxation firing in reverse; the fourth was a genuinely plausible pair (`дупу` vs the
physically corresponding `lege`). Those four are now promoted into a committed regression
corpus so they can never silently come back. The two policy rules in Insight 3 are their
direct legacy.

### Finding D — the most surprising result: the recall gap is a *choice*, not a limit

The precision benchmarks say nothing about **recall** — how often a genuinely wrong-layout
word actually gets fixed. A separate benchmark materializes 1,200 frequency-ranked Ukrainian
words and 1,000 English words as the physical keys a user would press with the wrong layout,
and runs them through production `evaluate`:

| Profile | UK → EN | EN → UK |
|---|---:|---:|
| **Conservative (default)** | 75.1% | 94.3% |
| Aggressive, `min_word_length = 2` | **92.3%** | **99.4%** |

And by word length, on the default profile:

| Length | Recall |
|---|---:|
| ≤ 3 chars | **0.0%** |
| 4–6 chars | 93.1% |
| 7+ chars | 98.5% |

Three things fall out of this, and they reframe the whole project:

1. **The default abstains on every short word — a flat 0%.** With `min_word_length = 4`,
   the most frequent words in the language are never touched. That is the single largest
   recall hole, and it is entirely deliberate.
2. **The model is strong; the conservatism is policy.** Aggressive thresholds reach 92% / 99%
   with *no model change*. So the ~25% of Ukrainian words the default misses is a precision-
   over-recall decision, not a ceiling. You can have the recall whenever you decide the
   false-correction cost is acceptable.
3. **Broad coverage won't come from the trigger table.** Twelve targeted rules don't move
   aggregate recall; real short-word coverage needs a proper dictionary (the current
   exact-match set is 296 stop-words) or a less conservative default — not more triggers.

That second point is the finding I'd least expected. I went in assuming the missed
corrections were the model failing. The benchmark proved the model was fine and the *policy*
was choosing to stay quiet — which is a far better problem to have, because a dial is easier
to turn than a model is to retrain.

## What this is, and isn't

The honest boundaries, stated the way the benchmark docs state them:

- Every result here is a **deterministic corpus sample**, not a real-world incidence claim.
  "0 false corrections in 20,000 words" is a strong safety signal, not a promise about your
  actual typing.
- The boundary corpus evaluates the decision **at a single word boundary** (a Space). It is
  not yet an event-sequence suite: navigation keys, mid-word layout changes, correction
  timing, tracker resets, and post-correction layout state are a separate, unbuilt benchmark
  layer.
- Recall at the default setting is deliberately incomplete by design.

## The takeaway

The reverse-engineering that mattered wasn't the keyboard table — that's public and static.
It was reconstructing the *judgement*: a two-layer decision where deterministic triggers
cover the cases you know and a self-doubting statistical model handles the rest, all of it
operating on physical keys rather than the characters on screen, and all of it pinned down
by a benchmark harsh enough to overrule the author's intuitions twice (bigger corpus is not
better; the recall gap is a policy dial, not a model wall).

Punto Switcher shipped this behaviour twenty years ago and never had to explain how. Building
it back from scratch, the surprise was how much of the design is really about knowing when
*not* to act.

---

*Upyr is an independent Rust project; it is not affiliated with Yandex or Punto Switcher and
shares no code with Punto Switcher or KeyboardSwitch. Benchmark artifacts and reproduction
commands live under [`docs/benchmarks/`](../benchmarks/); the frozen corpora are pinned by
SHA-256 in those documents.*
