# Complete pixel settings

Status: implementation in progress. Date: 2026-11-12.

The two-page pixel prototype became the primary menu entry before it covered
the existing settings. Bring every control in the native settings window into
the pixel window. Keep its staged edits, merge-on-save, validation, Unicode
editing and dirty-close behavior. Use the existing egui forms and host actions
where possible; do not create another implementation of OS settings effects.

## Coverage

| Page | Controls and actions |
| --- | --- |
| General | Direction, switch layout, restore clipboard, launch at login and repair/remove, macOS Globe-key switching |
| Correction | Existing sensitivity/minimum/delay, macOS terminal correction and Superset slash commands |
| Exceptions | Existing Unicode add/edit/remove |
| Shortcuts | Physical-key selection/previous-word recorders, macOS Undo, reset/cancel, modifier gesture/action/timeout |
| Feedback | Indicator enabled/duration/style/preview; macOS source selection and searchable country flag picker with automatic fallback; master volume, headphone ceiling/output status, sound pack, keyboard sounds and all event previews |
| Advanced | Copy, paste and clipboard restore delays |
| About | Version, privacy, implementation, license and project links |

Use the shared seven-page navigation and search catalog. Pages and controls
must fit a 480 × 400 window through wrapping and scrolling. Keep an explicit
native-interface fallback. The local TUI keeps its current Correction/Exceptions
scope; engine behavior and model data are outside this change.

## State and host effects

All editable config remains in one SettingsDraft. Existing egui form methods
may act as view adapters over that draft. Numeric inputs use either retained
text validation or bounded numeric widgets. Defaults modifies the draft only.
Revert restores its saved base. Search and tab changes never discard edits.

Launch-at-login is an OS preference outside config.toml: track its desired and
observed states separately, include unsaved changes in dirty handling, apply
only after successful config Save, and keep a failed request available to retry.
Repair/Remove and preview buttons remain explicit immediate host actions.
Reload refreshes config and the observed startup state; read failures preserve
edits. Never exercise startup mutations or audio playback during automatic tests.

Flag choices are staged in the config and merge per input source. Use the
existing bundled PNGs and localization, with lazy texture decoding. Automatic
removes only that source's override; preserve custom text and unrelated layouts.
No assets are downloaded. Reuse the already-locked image decoder on macOS.

Shortcut capture uses physical keys, supports Escape cancellation, rejects
duplicates and prevents captured input from also activating a form control.
The existing settings instance guard continues to suppress background actions.

## Acceptance

- Every row above is available without leaving pixel settings.
- Headless frames cover all seven pages, search, Unicode and small-window layout.
- Focused tests cover recorder conflicts/cancellation, staged edits/defaults,
  startup dirty/retry behavior, flag reset and concurrent merge preservation.
- Existing settings tests, formatting and platform Clippy checks pass.
- Visually inspect an isolated-config window; preserve the user's config and
  OS startup preference. Build/install with backup and record verification limits.
