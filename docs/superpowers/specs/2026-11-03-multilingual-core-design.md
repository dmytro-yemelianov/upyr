# Multilingual core: design

Status: draft for the owner's decision (2026-11-03), reviewed once by a
builder. Nothing here is built yet.

## Intent

Upyr fixes text typed on the wrong keyboard layout. Today every layer assumes
exactly two layouts, US-QWERTY English and Ukrainian. The owner wants the core
to be universal: any enabled pair (or set) of input sources, in any of the
supported languages, without English or Ukrainian being special.

The physical idea already generalises: a key press is a physical position, and
a layout maps positions to characters. "Wrong layout" means the same positions
read better under another enabled layout. What does not generalise today is
the data and the types around that idea.

## Current coupling (measured on `main`, 2026-11-03)

| Area | Where | What is binary |
|---|---|---|
| Language model | `crates/upyr-core/assets/models/language.ngm` (2.8 MiB, 173,964 entries), `auto_correct.rs:950` | Each n-gram carries one signed byte: negative = English, positive = Ukrainian. The format can only express two languages. |
| Types | `InputLayout {English, Ukrainian}` (`auto_correct.rs:26`, re-exported as `SystemLayout`), private `Language` (`:775`), `Direction {Smart, EnglishToUkrainian, UkrainianToEnglish}` (`layout.rs:32`) | Two variants each; ~165 references in `auto_correct.rs`, 116 in `upyr-wasm`, 85 in `system_layout.rs`, 63 in `src/auto_correct.rs`, 44 in `layout.rs`. |
| Built-in layout table | `layout.rs:9-16` | Hard-coded US-QWERTY ↔ Ukrainian strings. |
| Installed mapping | `system_layout.rs` (macOS, Windows, Linux) | Pairs one preferred English source with one preferred Ukrainian source into `(english_char, ukrainian_char)` pairs. |
| Lexicons | `assets/dictionaries/{english,ukrainian}.txt`, `triggers.txt` | One file per language; triggers default to English source. |
| Heuristics | `looks_like_deliberate_latin_identifier`, technical-token checks, short-word lists in `src/auto_correct.rs` | Latin/English-specific, Ukrainian short-word list. |
| Config | `direction` (`english-to-ukrainian`, …), indicator text `EN`/`UK` and two flags (`feedback.rs`) | Two languages. |
| Calibration | `model_thresholds()` and every benchmark | Thresholds tuned on EN/UK only; replay corpus and recall fixtures are EN/UK. |
| Generator | `tools/generate_ngram_model.py` | Already a `LANGUAGES` table with Leipzig Wortschatz sources; only the output format is two-class. |

Assets that already scale: `assets/globe-switch/language-regions.txt` (185
languages to flag regions) and the Globe-key switcher, which cycles any number
of enabled sources.

## Goals

- Any enabled input source whose language has a model can take part in
  correction; English and Ukrainian are data, not code.
- With two enabled layouts, behaviour and quality for EN/UK stay exactly as
  today: boundary replay 90/90 corrections, 0 false, 0 wrong; recall at least
  909/1200 (UK) and 943/1000 (EN), wrong corrections at or below today's.
- With three or more enabled layouts, a word is reinterpreted under each other
  enabled layout and the best-supported reading wins, with the same
  "abstain when unsure" policy.
- Adding a language is a data change: a layout table (or OS discovery), a
  model slice, a lexicon, short words, and its benchmark fixtures.

## Non-goals

- Input methods that are not one key → one character (CJK IMEs, Vietnamese
  Telex, dead-key composition sequences). They stay unsupported.
- Letters reachable only through AltGr/Option layers. *My call:* the tracker
  records plain and Shift positions today, and adding a third modifier layer
  is a separate change worth its own design if a chosen language needs it.
- Transliteration between scripts (a separate feature).
- Automatic language detection of whole documents or spell checking.

## Anti-goals (would be failure even if it "works")

- Any EN/UK regression on the existing benchmarks.
- Downloading models or lexicons at runtime (privacy promise: no network).
- A binary that grows without bound with every language (see size).
- Reading text that is not being typed (screen scraping) to choose a language.

## Design

### 1. Layouts are data

- `LanguageId`: a BCP-47 primary subtag (`en`, `uk`, `de`, …).
- `LayoutId`: a stable identifier (`us`, `uk-pc`, `de`, or the OS source id)
  with its `LanguageId`.
- `Layout`: for each physical key the plain and Shift characters, including
  the digit row and the ISO key left of Z. AltGr and Option layers are out of
  scope (see non-goals), so layouts that put letters only there (Polish
  Programmer, Czech QWERTY diacritics) are not supported. The built-in
  set ships US-QWERTY and Ukrainian-PC (today's table); the OS-discovered
  layout replaces the built-in one whenever it is available, as now.
- Conversion A → B goes through physical keys: character → key (A) → character
  (B). The installed mapping becomes one table per enabled layout instead of
  one EN↔UK pair list. The tracker keeps recording physical keys, so it does
  not change shape.
- `Direction` becomes a pair `(from: LayoutId, to: LayoutId)` or `Auto`.

### 2. Model format v2: independent scores per language

*My call* (the reviewer pointed out that one-vs-rest scores would shift every
time a language is added, which breaks the EN/UK regression gate):

- Each language has its own slice: n-gram → quantised strength for that
  language alone. Adding or removing a language never changes another
  language's numbers.
- A reading is scored under its own language's slice only. The comparison
  between languages happens at runtime, and only among the enabled layouts,
  so with EN and UK enabled the decision depends on the EN and UK slices and
  nothing else.
- `coverage(text, L)` keeps its current meaning: the weighted share of the
  text's n-grams that support L. Thresholds stay per-sensitivity and gain an
  optional per-pair calibration override, measured, not guessed.
- Phase 2 gate: the v2 EN and UK slices must reproduce today's benchmarks. If
  independent scoring cannot match the current signed differential, the EN↔UK
  pair keeps the v1 signed table as its pair slice and only new pairs use v2;
  that fallback is decided by measurement, not in advance.
- Languages are separate slices in one file, with a header listing them, so a
  build can include a subset (see size).
- The generator grows a `--languages en,uk,…` option; the Leipzig source table
  already has the shape.

Size: v1 is 2,957,400 bytes for two languages at the current budgets. A first
estimate is 1.2–1.6 MiB per added language at the same budgets, to be measured
in phase 2 before committing to it.

The web demo has a hard ceiling today: `tools/check_wasm_size.py` fails the
build above 3,750,000 raw / 512,000 Brotli bytes, and the embedded model
already takes 2,957,400 of the raw budget. Any added language breaks it, so
the WASM build either embeds EN/UK only and loads other slices from the same
origin on demand, or the limits are raised (owner's decision, below). The
desktop app is unaffected by this limit.

### 3. Lexicons and heuristics per language

- `assets/dictionaries/<lang>.txt`, short words `<lang>-short.txt`, and
  `triggers.txt` rules carry their layout (the column added in #54).
- Technical-token and identifier checks apply by script (Latin, Cyrillic, …)
  rather than by "English".

### 4. Choosing the target with three or more layouts

The active layout is the source. Each other enabled layout gives one candidate
reading. Each candidate is judged exactly as today's single candidate
(dictionary, triggers, model margin, prefix sweep). The candidate with the
largest margin wins, and only if it clears the policy. *My calls:*

- Two layouts that produce the same string for the typed keys (for example
  two Cyrillic layouts that agree on every letter typed) give one candidate,
  not a tie; the switch goes to the layout of the same language that the user
  used last.
- A real tie between different readings means abstain.
- The EN/UK targets must also hold with a third layout enabled: the replay
  and recall suites run once with EN+UK and once with EN+UK+the new language,
  and both runs must meet the EN/UK floors.
- The layout switch after a correction follows the winner.
- A third layout joins correction only when the user turns it on ("Correct
  between" in Settings). Installing a source in the OS does not change
  behaviour by itself; the default stays today's pair.

### 5. Surfaces

- Config: migrate `direction` (`english-to-ukrainian` → `{from: us, to: uk-pc}`,
  `smart` → `auto`) with the existing versioned migrations; schema 13.
- Indicator: language code and flag from `language-regions.txt`, which already
  covers 185 languages.
- Settings: the direction pop-up becomes "Correct between" with the enabled
  layouts; per-language enable toggles if more than two are enabled.
- WASM API: `sourceLayout` takes a layout id; `mappingOverrides` becomes
  per-layout key tables. Keep accepting `english`/`ukrainian` as aliases for
  one release.

## Phases (each a PR series with the EN/UK benchmarks as a gate)

| Phase | Content | Exit criterion |
|---|---|---|
| 0 | Freeze EN/UK baselines as hard test floors (already mostly true) | CI fails on any regression |
| 1 | `LanguageId`/`LayoutId`/data-driven layouts in core, EN/UK only, no behaviour change | All benchmarks byte-identical |
| 2 | Model v2 format + generator, regenerate EN+UK | Benchmarks equal or better; size measured |
| 3 | Platform discovery of every enabled source (macOS/Windows/Linux), config migration | Existing users see no change |
| 4 | First new language end to end, with replay and recall fixtures | Its own replay: 0 false corrections |
| 5 | Settings, indicator, WASM API, site | Demo supports the new language |

Rough price: about the size of the current engine again, 8–12 PRs, most of it
in phases 1 and 2.

## Decided

- The physical-key model stays (it is already language-neutral).
- No runtime downloads in the app; models ship with it.
- EN/UK quality is the regression gate for every phase.

## My calls (check these while reading)

- Independent per-language model slices; comparison only among enabled
  layouts; v1 kept for EN↔UK if v2 cannot match it (section 2).
- Identical readings are agreement, real ties abstain, and EN/UK floors also
  hold with a third layout on (section 4).
- Extra layouts are opt-in (section 4).
- AltGr/Option layers out of scope (non-goals).

## Open for the owner (changes what gets built)

1. Which languages come first, how many are targeted, and whether Russian is
   in scope. This drives corpus work, fixtures, size, and whether AltGr
   layers are needed after all.
2. Same-script pairs (English ↔ German, Ukrainian ↔ Russian): in scope or
   not. Different-script pairs (Latin ↔ Cyrillic, Latin ↔ Greek) are what the
   engine does well today: almost every key changes. In a same-script pair
   only a few keys differ (QWERTY/QWERTZ swaps Y and Z; UK/RU differ in
   і/и/ї/є/ґ and a few more), so the wrong-layout signal is weak and false
   corrections are the main risk.
3. Size policy: embed every supported language in the app (+1.2–1.6 MiB each,
   estimate) or ship slices as files in the bundle and load only enabled
   ones; and for the web demo, load extra languages from the same origin on
   demand or raise the 3,750,000-byte limit.
4. Fixtures per language: the replay corpus (real typing sessions) and recall
   targets need a native speaker to curate or at least review. Who does that,
   or whether generated fixtures from held-out Leipzig sentences are enough
   to ship a language as "preview".

## Left to the builder

- Exact on-disk layout of the v2 model and its validation.
- Per-pair threshold calibration method, as long as it is measured on
  fixtures and documented in `docs/benchmarks/`.
- Naming of layout ids beyond the OS-provided ones.
