# Settings front ends implementation plan

Spec: [settings-frontends.md](../specs/settings-frontends.md).

- [x] P1 — Shared draft/actions, exception normalization and pure merge/validation.
  Test invalid edits, revert/reload, normalization and concurrent changes (AC1–AC3).
- [x] P2 — Desktop persistence host and integration into native/egui Save.
  Test failed/successful persistence without touching the real config (AC4).
- [x] P3 — Explicit launch options and optional pixel GUI for Correction/Exceptions.
  Verify CLI dispatch, scrolling, focus, Unicode and dirty-close handling (AC5–AC6).
- [x] P4 — Local line-oriented ANSI/TUI adapter over the same actions and host.
  Replay commands in an isolated config, including errors and terminal escaping (AC7).
- [ ] P5 — Workspace/engine checks, isolated GUI verification, docs, measured build size,
  reviewable commits and installation with backup (AC8–AC9).

Implementation order is P1 → P2 → P3 → P4 → P5. Keep the engine and model data
unchanged. Do not add remote services or import Pixel SSH assets. Record any
scope deviation and its evidence here before treating the task as complete.

## Evidence

2026-11-12, macOS arm64 / Rust 1.88. The spec and initial plan were committed
first as `8e86198`.

- `cargo test --workspace`: 434 passed, 6 ignored. Shared draft/persistence tests
  cover normalization, invalid text, concurrent changes and read/write failures.
- Three headless egui tests drive actual keyboard events at 480 × 400: long
  Ukrainian paste/selection, Tab/Shift+Tab, named semantic text fields, disabled
  Save for invalid input, dirty close and pending Add words protection. Save
  includes pending words; failed read/reload retains them; Revert discards them.
- Both GUI screens were visually observed using an isolated config. Native AX
  access became unavailable during inspection, so input/focus/semantic assertions
  above come from egui tests. A full OS screen-reader audit was not performed.
- Ten release CLI cases passed with an isolated config: help, exclusive flags,
  Unicode save, invalid/dirty discard, EOF, control-character escaping,
  single-instance conflicts, cleanup and malformed config protection.
- Formatting, macOS all-target/all-feature Clippy, privacy/version checks and
  Windows GNU cross-target all-target/all-feature Clippy passed. Windows runtime
  and remote CI were not run. The Windows GUI helper attaches to a parent
  terminal for explicit TUI/help invocations, following
  [Microsoft's console API contract](https://learn.microsoft.com/en-us/windows/console/attachconsole).
- The engine contract passed all 18 checks; language/model data and engine
  behavior were not changed. No new dependency packages or Pixel SSH assets.

Detailed local evidence is retained in `.stos/settings-frontends/` (not committed).
Installation and final build-size evidence follow after the checked build is installed.
