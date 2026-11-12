# Settings localization plan

Spec: [settings-localization.md](../specs/settings-localization.md).

- [x] L1 — Add persisted UI language with Ukrainian default and shared catalog.
- [x] L2 — Localize pixel/shared forms and add the General language selector;
  preserve language-neutral values and bilingual search.
- [x] L3 — Connect native controls and terminal display to the same preference.
- [x] L4 — Test default/English/save/revert/search and UI coverage; build/check.
- [x] L5 — Isolated visual check, backed-up installation and final evidence.

Keep translation separate from engine behavior and user text. Do not change
the user's config file just to select the new default.

## Evidence

- Full workspace: 447 passed, 6 intentionally ignored. Final focused settings
  suite: 38 passed. The subsequent stronger selector assertion also passed;
  switching the language changes no other config fields.
- macOS and Windows GNU all-target/all-feature Clippy passed with warnings denied.
  Formatting, privacy/version verification and release build for all three bins passed.
- Headless checks cover old/new defaults, English serialization, concurrent merge,
  bilingual search, all seven Ukrainian pages at 480 px, an actual picker click,
  Save/Revert/Reload, unchanged user values, and Ukrainian TUI with English commands.
- Isolated live pixel window: readable Ukrainian, switch to English, then Revert
  to Ukrainian. Native window: Ukrainian default, English selection and Save,
  isolated config readback, then English after reopening with the selector restored.
- Desktop pixel controls were visually checked; the macOS AX bridge exposed
  window/menu elements but not egui controls. Widget semantics were tested via
  headless AccessKit. Unknown OS/backend diagnostic details remain in English.
- Evidence: `.stos/settings-localization/` (local, excluded from git).
- Installed `/Applications/Upyr.app` (0.5.0, arm64), runtime source `5b89204`.
  Previous app retained in `.stos/install-settings-localization-20261112/previous/`.
  Verified all three installed binary hashes, restarted one background process,
  checked CLI/helper entrypoints and a Ukrainian TUI save using an isolated config.
  User config SHA-256 and launch-at-login status remained unchanged; the effective
  settings language is `uk`. Final local report: `.stos/settings-localization/report.json`.
