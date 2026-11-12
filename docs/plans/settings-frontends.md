# Settings front ends implementation plan

Spec: [settings-frontends.md](../specs/settings-frontends.md).

- [ ] P1 — Shared draft/actions, exception normalization and pure merge/validation.
  Test invalid edits, revert/reload, normalization and concurrent changes (AC1–AC3).
- [ ] P2 — Desktop persistence host and integration into native/egui Save.
  Test failed/successful persistence without touching the real config (AC4).
- [ ] P3 — Explicit launch options and optional pixel GUI for Correction/Exceptions.
  Verify CLI dispatch, scrolling, focus, Unicode and dirty-close handling (AC5–AC6).
- [ ] P4 — Local line-oriented ANSI/TUI adapter over the same actions and host.
  Replay commands in an isolated config, including errors and terminal escaping (AC7).
- [ ] P5 — Workspace/engine checks, isolated GUI verification, docs, measured build size,
  reviewable commits and installation with backup (AC8–AC9).

Implementation order is P1 → P2 → P3 → P4 → P5. Keep the engine and model data
unchanged. Do not add remote services or import Pixel SSH assets. Record any
scope deviation and its evidence here before treating the task as complete.

## Evidence

Pending implementation.
