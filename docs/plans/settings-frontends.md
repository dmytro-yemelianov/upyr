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
- [x] P5 — Workspace/engine checks, isolated GUI verification, docs, measured build size,
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

The release build was installed at `/Applications/Upyr.app`, with the prior app
kept at `.stos/install-settings-frontends-20261112/previous/Upyr.app`. All three
binary hashes matched the prepared candidate, and the background app restarted.
Five installed-binary smoke cases passed: both help entry points, isolated init,
Unicode TUI save and invalid-edit/discard handling through the standalone helper.
The user's config hash remained unchanged throughout verification and installation.

macOS arm64 release sizes (bytes, before local packaging):

| Binary | Previous build | This build | Difference |
| --- | ---: | ---: | ---: |
| `upyr` | 15,165,088 | 19,427,392 | +4,262,304 |
| `upyr-background` | 14,416,272 | 14,483,712 | +67,440 |
| `upyr-settings` | 1,599,904 | 6,699,440 | +5,099,536 |

The optional GUI links the existing egui renderer into the macOS settings and
CLI binaries. This is an interface feature, with no speed or memory improvement
claim. Opening latency and resident memory were not benchmarked.

Detailed local evidence is retained in `.stos/settings-frontends/` and the
installation `build.json` (not committed). Implementation commit: `274505e`.
No remote push or remote CI run was performed.
