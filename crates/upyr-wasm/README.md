# upyr-wasm

`upyr-wasm` is the DOM-independent WebAssembly binding for Upyr's shared
English/Ukrainian correction engine. It is an internal workspace crate while the
TypeScript facade and npm package are being built.

## Current exports

- `convertText(text, direction)` performs explicit physical-layout conversion.
- `modelVersion()` identifies the embedded decision model.
- `new UpyrSession(options?)` creates a stateful suggestion or auto session.
- `session.setSourceLayout(layout?)` supplies the observed input layout.
- `session.keyDown(event)` consumes one normalized browser keydown.
- `session.reset()` clears tracked text while preserving policy/layout settings.
- `session.configure(options?)` atomically replaces settings and state.

`suggest` is the default mode. `auto` changes the returned decision kind from
`suggest` to `correct`; neither mode edits the DOM inside WASM.

The Rust source embeds TypeScript interfaces into the generated declaration.
The binding validates option names, policy bounds, exception sizes, and physical
mapping overrides before starting a session.

## Host responsibilities

The browser host must:

1. supply `KeyboardEvent.code`, `key`, modifier state, composition state, and an
   explicit `english` or `ukrainian` source layout;
2. derive `capsLock` and `altGraphKey` with `getModifierState`;
3. queue a Space-triggered decision until the matching `input` event, because
   correction strings include the Space that keydown has not inserted yet;
4. verify focus, selection, and the exact `expectedSource` suffix before showing
   or applying a replacement;
5. call `reset()` on composition start, paste/cut/drop, undo/redo, caret or
   selection changes, focus changes, and unmatched input events.

Only literal Space is a correction boundary in the first binding. Enter, Tab,
navigation, unsupported codes, dead keys, AltGraph, IME composition, and modified
ordinary keys reset tracking. While Caps Lock is active, physical punctuation
positions that become Ukrainian letters also reset until the core can represent
source and target case independently.

Websites whose Ukrainian layout maps Backslash to apostrophe can override the
built-in unshifted and Shift layers together:

```ts
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
```

Both shifted properties must be supplied together. `english` must name a
built-in unshifted physical position and `shiftedEnglish` must be that key's
actual Shift mate. English endpoints must be unique, and Ukrainian endpoints
must be unique except when one physical key intentionally produces the same
target on both layers. Reverse conversion then chooses that key's unshifted
English character as the canonical result.

## Verification

```sh
cargo test -p upyr-wasm --all-targets --locked
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

See the full [web architecture and delivery plan](../../docs/architecture/wasm-web-plan.md).
