# Ukrainian settings by default

Status: implemented, verified and installed. Date: 2026-11-12.

Make Ukrainian the default settings UI language and offer Українська / English
in General (Основні). Persist the choice as `ui_language = "uk" | "en"` in the
existing config. Files without that field use Ukrainian; no migration write is
needed merely to open settings. Preserve all seven pixel pages and native controls.

Use a shared translation catalog for page names, controls, help, confirmations,
search results and settings status messages. Keep English as an explicit option
and fallback for unknown backend diagnostics. Never translate exception words,
shortcut values, app/source IDs, user-defined flag text, file paths or URLs.
Search must continue to accept English terms and additionally match Ukrainian
labels. Preserve command syntax and stored activation keys.

Pixel settings preview the staged language immediately, include it in dirty
state, and apply ordinary Save/Revert/Reload semantics. The native editor saves
the same preference and explains that its language changes on reopening. Its
initial controls and flag gallery must use the chosen language. The local TUI
translates its display; existing English commands remain valid.

## Acceptance

- Old/missing-field and new configs default to Ukrainian; explicit English
  round-trips and merges without overwriting unrelated edits.
- All seven pixel pages, dialogs, buttons and common validation messages display
  Ukrainian by default; the language selector provides a usable English fallback.
- Native settings use the same catalog without altering user-editable content.
- Ukrainian and English searches locate the same controls.
- Tests cover defaults, language changes, revert/save, translation preservation
  and GUI labels; formatting, macOS/Windows Clippy and release build pass.
- Verify with an isolated config, install with backup, preserve user settings
  and launch-at-login state. No engine/model-data changes.
