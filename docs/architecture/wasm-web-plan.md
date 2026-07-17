# Decision engine and WebAssembly plan

Status: in progress; portable core and headless WASM ABI implemented

This document defines two coordinated delivery tracks:

- improve automatic correction with a pairwise, context-aware decision engine;
- expose the same engine to websites through WebAssembly.

The tracks share one portable Rust core. The web integration must not reimplement
mapping, scoring, or correction policy in JavaScript.

## Implemented core boundary

[`upyr-core`](../../crates/upyr-core) now owns layout conversion, portable
physical-key types, short-context tracking, candidate generation, dictionaries,
the N-gram model, decision policy, and all portable behavioral tests.

The root [`layout.rs`](../../src/layout.rs) preserves the existing public API as
a re-export. The root [`auto_correct.rs`](../../src/auto_correct.rs) is a desktop
adapter that translates `device_query::Keycode`, maps desktop configuration into
core policy, obtains the installed OS mapping, and injects it into the core.
`SystemLayout` remains source-compatible as a re-export of the core
`InputLayout` type.

Desktop clipboard, global-hook, tray, settings, windowing, and single-instance
dependencies remain outside the core dependency graph. The dedicated WASM CI
job compiles both portable crates without any desktop package, runs the binding
contract in Node through `wasm-bindgen-test`, and checks the measured release
size.

The selected self-contained 1M-corpus core model is 2,957,400 bytes. It
compresses to approximately 619 KB with gzip and 371 KB with Brotli, so the
complete model remains viable for the first lazy-loaded web artifact.

Completed extraction work:

- workspace and self-contained `upyr-core` package;
- injected layout mapping and portable `PhysicalKey`/`InputLayout` types;
- desktop key/config/layout adapters with compatibility re-exports;
- model, dictionaries, unit tests, and synthetic replay tests moved into core;
- pairwise scorer seam and frozen materialized boundary replay;
- headless `upyr-wasm` conversion/session API with strict serialized options;
- `KeyboardEvent.code` adapter, layout persistence, IME/shortcut resets,
  mapping overrides, and conservative Caps Lock handling;
- generated TypeScript contract annotations, native tests, WASM/Node smoke
  tests, strict workspace Clippy, and measured size gates in CI.

## Decision

The current root package remains the desktop host. The workspace now has two
additional Rust crates; the TypeScript facade and demo remain the next web
delivery layer:

```text
Cargo.toml
src/                         desktop application and OS adapters
crates/
  upyr-core/                 portable mapping and decision engine
  upyr-wasm/                 wasm-bindgen ABI only
packages/
  upyr-web/                  TypeScript facade and DOM adapters
demo/
  web/                       static integration playground
```

`upyr-core` may use ordinary Rust `std`; there is no current benefit in making it
`no_std`. It must not depend on filesystem, network, clipboard, UI, global input
hooks, `web-sys`, or an operating-system layout API.

The root desktop package converts OS events and configuration into core types.
`upyr-wasm` depends only on `upyr-core`, `wasm-bindgen`, `js-sys`, and minimal
serialization glue. DOM access remains in TypeScript because selection, undo,
controlled components, and editor transactions are browser/framework concerns.

## Portable core contract

The core owns these concepts:

```rust
pub enum PhysicalKey {
    KeyA,
    // Remaining writing-system positions follow KeyboardEvent.code names.
    BracketLeft,
    BracketRight,
    Semicolon,
    Quote,
    Backslash,
    Comma,
    Period,
    Slash,
    Backquote,
    Space,
    Backspace,
    Unsupported,
}

pub struct PhysicalKeyEvent {
    pub key: PhysicalKey,
    pub shifted: bool,
}

pub enum AutoDecision {
    Correct(AutoCorrection),
    Continue,
    Reset,
}
```

The browser wrapper adds composition/modifier metadata, serialized reasons, and
the suggest/auto host action. These properties remain required:

- physical keys are independent of `device_query`, `rdev`, and the DOM;
- layout mapping overrides and correction policy are session inputs; the built-in
  model stays atomically versioned with the core artifact;
- the core never queries the OS or mutates UI;
- a session can consume events, return a decision, and be explicitly reset;
- decisions return `expected_source`, not Rust byte offsets;
- scorer implementation is private behind a stable interface so N-gram v1,
  N-gram v2, and experimental char-CNN implementations do not change hosts.

The model reader must support both `Model::builtin()` and validated owned bytes.
Header, format version, entry count, length, language set, and allocation limits
must be checked in production. The current test-only validation is not enough
for a future caller-supplied artifact.

## Decision engine track

The target engine is:

```text
physical events
  -> candidate lattice
  -> language naturalness head
  -> role/shape head
  -> online context and host priors
  -> pairwise action ranker
  -> KEEP / CORRECT / WAIT / RESET
  -> expected-source verification in the host
```

The first learned upgrade remains character N-grams, but the objective changes
from one signed English/Ukrainian score to calibrated candidate ranking:

- independent English, Ukrainian, and background plausibility;
- structural roles such as natural word, brand, acronym, identifier, URL, and
  path;
- pairwise `KEEP` versus mapped-candidate evidence;
- an explicit unknown/abstain outcome;
- a small online language/technical context state;
- host-provided application or origin priors.

The model is optimized first for false-correction rate, then recall at a fixed
false-positive budget.

## WebAssembly API

### Implemented headless binding

`upyr-wasm` currently exports `convertText`, `modelVersion`, and the stateful
`UpyrSession`. It has no DOM or network access:

```ts
await init();

const direct = convertText("ghbdsn", "english-to-ukrainian");
const session = new UpyrSession({
  mode: "suggest",
  sourceLayout: "english",
  mappingOverrides: [{
    english: "\\",
    ukrainian: "ʼ",
    shiftedEnglish: "|",
    shiftedUkrainian: "ʼ",
  }],
});

const decision = session.keyDown({
  code: event.code,
  key: event.key,
  shiftKey: event.shiftKey,
  capsLock: event.getModifierState("CapsLock"),
  ctrlKey: event.ctrlKey,
  altKey: event.altKey,
  metaKey: event.metaKey,
  altGraphKey: event.getModifierState("AltGraph"),
  isComposing: event.isComposing,
});
```

Options default to conservative, suggestion-only behavior. `sourceLayout` must
be supplied before text tracking because a website cannot reliably query the
active operating-system keyboard layout. `mappingOverrides` starts from Upyr's
built-in mapping and replaces individual physical pairs. A physical key can
provide both its unshifted and Shift layers; the binding rejects partial Shift
pairs, unrelated Shift mates, unknown positions, and ambiguous endpoints. When
both layers intentionally emit the same target, reverse conversion uses the
unshifted English character as canonical.

The generated declaration includes these principal result types:

```ts
interface UpyrPassiveDecision {
  kind: "wait" | "reset";
  reason: string;
  modelVersion: string;
  applyAfterInput: false;
  sourceLayout?: "english" | "ukrainian";
}

interface UpyrCorrectionDecision {
  kind: "suggest" | "correct";
  reason: "wrong-layout";
  modelVersion: string;
  applyAfterInput: true;
  sourceLayout: "english" | "ukrainian";
  targetLayout: "english" | "ukrainian";
  expectedSource: string;
  replacement: string;
  direction: "english-to-ukrainian" | "ukrainian-to-english";
}
```

There is deliberately no synthetic `confidence` field. The current scorer is a
policy decision, not a calibrated probability. `kind: "correct"` means the host
is authorized to apply a verified edit; the WASM module itself still performs no
mutation.

### Future TypeScript/DOM facade

The planned `createUpyr`, `createSession`, and `attach` convenience APIs belong
to `packages/upyr-web`. That layer will verify `expectedSource`, derive UTF-16
DOM ranges, preserve undo, handle controlled components, and apply edits after
the matching browser input event. It is not implemented or published to npm yet.

## Browser event contract

- `keydown.code` identifies the physical key position. `key` is also required so
  `Dead`, `Process`, and `Unidentified` events can reset instead of recording a
  character that was never committed.
- Normal text keys are not cancelled with `preventDefault()`. The browser or IME
  commits first, then Upyr evaluates a boundary.
- Version one evaluates a word only on literal Space. Enter, Tab, blur, and
  unsupported codes reset; they are not disguised as Space because that would
  make `expectedSource` disagree with the DOM.
- A Space decision is emitted during `keydown`, but both `expectedSource` and
  `replacement` include the Space. The host queues it until the corresponding
  `input` event, then checks focus, selection, and the exact source suffix before
  proposing or applying anything.
- Letters use `shiftKey XOR capsLock`; punctuation uses Shift alone. While Caps
  Lock is active, the seven punctuation positions that become Ukrainian letters
  reset for now because the core cannot yet render independent source/target
  case from one bit.
- `compositionstart` pauses and resets automatic tracking. Events marked
  `isComposing`, dead keys, and AltGraph input reset, and tracking resumes after
  `compositionend`.
- Paste, drop, undo/redo, caret movement, blur, and an unsupported physical code
  call the public `reset()` method. Explicit conversion remains available.
- The first DOM facade will support uncontrolled `<input type="text|search">` and
  `<textarea>` with `setRangeText`, a reentrancy guard, and a bubbling `input`
  event.
- Controlled React/Vue/Svelte inputs will use an `applyEdit` callback or a
  dedicated adapter. Direct DOM mutation is not considered reliable for them.
- Plain-text `contenteditable` comes after range and undo tests. Rich editors
  remain headless/suggestion-only until they have editor-specific transaction
  adapters.
- The future facade will never attach password, current/new-password
  autocomplete, OTP/numeric, readonly, disabled, or `data-upyr="off"` fields.
- Mobile/virtual keyboards without a useful physical `code` are unsupported by
  the session adapter for now; explicit conversion remains available.

A site SDK only works in fields on sites that install it. System-wide behavior on
arbitrary websites requires a separate browser-extension host over the same core.

The physical-key contract follows the standardized `KeyboardEvent.code` values:
[UI Events](https://www.w3.org/TR/uievents/) and
[UI Events code values](https://www.w3.org/TR/uievents-code/).

## Model and execution

Version one uses one lazy-loaded WASM artifact with the complete built-in model:

- code and model stay atomically compatible;
- no runtime network request occurs after initialization;
- hosting remains simple and privacy behavior is obvious;
- validated external model bytes remain a future delivery mode rather than part
  of the current public WASM ABI.

An external versioned model asset is introduced only if the complete artifact
misses the size/startup budget or independent model updates become a proven need.

Scoring stays synchronously on the main thread initially. DOM work already runs
there, and a worker round trip would add sequencing races for a small scorer. An
optional worker backend is reserved for a neural challenger, batch replay, or a
future model whose measured latency warrants it. Every asynchronous result must
carry a monotonically increasing sequence number and be rejected when stale.

The first optimized artifact establishes a real baseline instead of guessing
below the embedded model size:

| Metric | Measured baseline | Current CI limit |
|---|---:|---:|
| Generated Node `_bg.wasm`, raw | 3,119,062 bytes | 3,750,000 bytes |
| Generated Node `_bg.wasm`, gzip -9 | 686,421 bytes | report only |
| Generated Node `_bg.wasm`, Brotli 11 | 427,311 bytes | 512,000 bytes |
| TypeScript facade | not implemented | pending |
| Runtime memory/latency | not measured | pending pinned runner |

These are current snapshots produced with Rust 1.88.0 and wasm-bindgen 0.2.126;
the limits leave room for minor compressor/toolchain drift. Raw and Brotli size
are hard CI gates in `tools/check_wasm_size.py`. Performance
budgets will be added only after the DOM facade and a pinned reference runner can
measure the end-to-end path consistently.

## Parallel delivery

The WASM track does not wait for N-gram v2:

| Phase | Decision-engine track | WASM/web track |
|---|---|---|
| 0 | Freeze replay corpus and baseline metrics | Define serialized decision shape |
| 1 | Extract candidate/scorer/session core without behavior change | Prepare `upyr-wasm` skeleton |
| 2 | Add scorer interface and selected 1M v1 model | Ship tested headless binding and size baseline |
| 3 | Train and integrate calibrated N-gram v2 | Ship TypeScript facade and input/textarea playground |
| 4 | Add context state and host priors | Add DOM adapters, package tests, and browser QA |
| 5 | Benchmark neural challenger | Consume the winning scorer without changing the web ABI |

The only hard dependency is core extraction. Once the scorer contract exists,
model work and website integration proceed independently.

Current position: phases 0–2 are complete for the portable boundary and headless
binding. Calibrated N-gram v2 and the TypeScript/DOM package now proceed in
parallel.

## Build, test, and release

Build the current headless layer with:

```sh
cargo test -p upyr-core -p upyr-wasm --all-targets --locked
cargo check -p upyr-core -p upyr-wasm --all-targets \
  --target wasm32-unknown-unknown --locked
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
cargo test -p upyr-wasm --target wasm32-unknown-unknown --locked
cargo build --release -p upyr-wasm --target wasm32-unknown-unknown --locked
wasm-bindgen --target nodejs --out-dir target/upyr-wasm-node \
  target/wasm32-unknown-unknown/release/upyr_wasm.wasm
node tools/smoke_wasm_node.cjs target/upyr-wasm-node/upyr_wasm.js
python3 tools/check_wasm_size.py
```

Implemented CI gates are native core/wrapper tests, WASM compilation, four
WASM/Node serialization and ABI tests, a generated Node binding consumer smoke,
workspace format/Clippy, and raw/gzip/Brotli size measurement with raw and
Brotli limits.

The package track still needs browser-native ESM generation, npm assembly,
TypeScript consumer fixtures, the shared golden replay through the exported ABI,
Playwright in Chromium/Firefox/WebKit, SSR-safe imports, DOM/undo tests, and
runtime performance gates. The official deployment guide documents the future
web and bundler outputs: [wasm-bindgen deployment](https://wasm-bindgen.github.io/wasm-bindgen/reference/deployment.html).

`wasm-bindgen-test` supports Node and headless-browser modes:
[testing guide](https://wasm-bindgen.github.io/wasm-bindgen/wasm-bindgen-test/usage.html).

Required browser scenarios include Ukrainian physical punctuation, Shift/Caps,
AltGraph and dead keys, selection and caret movement, Backspace, paste, undo,
IME composition, emoji and combining marks, controlled input callbacks, password
exclusion, and stale-decision rejection.

## Definition of done

- Native desktop and WASM produce identical decisions for every golden replay.
- The package works in vanilla ESM and a representative bundler consumer.
- Chromium, Firefox, and WebKit are green.
- A single undo restores an automatic correction in supported fields.
- IME, password, OTP, disabled, and opted-out fields are untouched.
- Size and reference-runner performance budgets pass.
- Importing the package during SSR does not access `window` or start a fetch.
- No telemetry or text transfer exists; bounded context clears on reset/destroy.
- A model mismatch fails with a structured error instead of panicking.
- A static demo and integration guide cover headless, suggest, and auto modes.
- Browser limitations and the separate extension scope are documented explicitly.
