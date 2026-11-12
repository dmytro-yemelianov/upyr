# upyr-core

Portable English-Ukrainian keyboard-layout conversion and correction engine for
Upyr.

The crate exposes physical-key layout conversion, deterministic trigger rules,
and the local signed n-gram scoring layer used by the desktop app. Runtime
decisions are local and do not require an account, telemetry endpoint, or remote
inference service.

The source package includes an offline correctness and memory-budget check:

```sh
cargo run -p upyr-core --example engine_contract --features engine-bench --release --locked --offline
```

It uses the committed fixtures, emits JSON lines and exits nonzero when a
budget fails. Cargo dependencies must already be cached. The check runs on
native targets without a desktop session; WASM has its own contract tests.
The `engine-bench` hooks are excluded from default application builds.
