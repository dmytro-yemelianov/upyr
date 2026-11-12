# Complete pixel settings plan

Specification: [pixel-settings-parity.md](../specs/pixel-settings-parity.md).

- [x] P1 — Reuse the existing egui forms on macOS and add native-only controls.
- [x] P2 — Connect all seven pages and search to the pixel draft and save flow;
  include startup edits in dirty/revert/reload handling and stage Defaults.
- [x] P3 — Add flag selection/preview using existing bundled images and rules.
- [x] P4 — Verify control coverage, persistence and recorder behavior; run
  focused tests, formatting, cross-platform Clippy and release build.
- [x] P5 — Inspect the isolated GUI, install with backup and record evidence.

Work in this order. Do not alter engine behavior or language assets. Do not
toggle real login items or play sound previews as part of verification.

## Evidence

The initial specification/plan was committed first as `4dd0aac`.

- The existing egui forms now compile on macOS and serve as view adapters over
  the pixel draft for General, Shortcuts, Feedback, Advanced and About. Correction
  and Exceptions retain their text-edit validation and Unicode behavior.
- Mac-only terminal controls reuse the native checkbox helpers. Physical shortcut
  capture includes Undo, validates all conflicts and consumes captured events.
- Launch-at-login keeps desired state on failure; unchanged preferences are not
  rewritten. Flag choices use the config's per-key merge; automatic removes only
  the selected source override. Preview flags hide after the configured duration.
- A complete workspace run passed 441 tests with 6 ignored. Focused checks added
  after visual inspection cover readable shortcut labels without missing glyphs.
- Headless GUI checks cover seven pages at 480 px width, search navigation with
  dirty edits, native macOS controls, startup retry, capture/cancel/conflict and
  defaults/flag merge. No real login-item mutation or sound playback was tested.

All seven navigation entries and the three shortcut recorders were visually
observed in an isolated-config window. That check exposed missing macOS key
symbols in the monospace font; the egui forms now use plain key names such as
Cmd, Option and Backspace. Headless coverage also decodes every bundled flag
and clicks Automatic/UA in the picker, preserving another source's custom flag.
Final focused suite: 35 settings tests passed. macOS and Windows cross-target
Clippy, formatting, privacy and version checks passed; no remote CI was run.

The native accessibility snapshot exposed window/menu controls but not the
egui content on this Mac; semantic/input coverage comes from headless egui
tests. A full OS screen-reader audit and live audio/startup mutation tests
were not performed.

Implementation `4ea5cf6` was installed in `/Applications/Upyr.app`; the prior
app is retained under `.stos/install-settings-parity-20261112/previous/Upyr.app`.
All three prepared binary hashes matched the installation, and the background
app restarted. Both installed help entry points and an isolated TUI save passed.
The user's config hash and launch-at-login status remained unchanged.

Release binary sizes before local packaging: CLI 19,594,400 bytes, settings
6,883,024 bytes, background 14,483,744 bytes. Detailed local evidence is in
`.stos/settings-parity/`. No engine or language-model data changed.
