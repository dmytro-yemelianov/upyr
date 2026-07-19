# upyr-core

Portable English-Ukrainian keyboard-layout conversion and correction engine for
Upyr.

The crate exposes physical-key layout conversion, deterministic trigger rules,
and the local signed n-gram scoring layer used by the desktop app. Runtime
decisions are local and do not require an account, telemetry endpoint, or remote
inference service.
