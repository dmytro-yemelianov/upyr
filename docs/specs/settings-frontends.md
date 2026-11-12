# Settings front ends v1

Status: implemented; verification is recorded in the plan. Owner: Upyr. Date: 2026-11-12.

The initial two-page GUI scope below is superseded by
[complete pixel settings](pixel-settings-parity.md). The TUI keeps the v1 scope.

## Goal and scope

Implement the shared-state/separate-renderer approach reviewed in
[settings-ui.md](../architecture/settings-ui.md), with an optional pixel-style
GUI and a local terminal interface. Deliver working Correction and Exceptions
screens, not a replacement for every native settings page.

The tray's Settings action opens the pixel-style GUI. Repeating it focuses the
existing pixel window. A Full Settings button opens the full desktop editor,
asking before discarding unsaved edits and releasing the instance guard before
launching the replacement window. Repair/startup flows and About still open the
full desktop editor. `upyr settings` without flags also opens the full editor.
`upyr settings --pixel` and `upyr-settings --pixel` open the pixel GUI;
`--tui` opens a local terminal
editor. These flags are mutually exclusive. Help must work without opening a
window or touching a configuration file. The new front ends show their limited
scope and explain how to open the full settings window.

## Shared state contract

`SettingsDraft` owns the loaded base and edited configuration. Typed
`SettingsAction` values edit correction, layout following, sensitivity, minimum
length, delay, and exception rows. Numeric text remains visible while invalid.
Validation reports errors without dropping edits. Revert returns to the loaded
base; Reload replaces both base and draft only after a successful host read.

Save normalizes exception rows (Unicode whitespace/comma splitting and
case-insensitive deduplication), validates values, reads the current config,
and applies the existing three-way field merge. The user's changed fields win;
untouched fields preserve concurrent CLI/background changes. Lists retain the
existing whole-list merge semantics. Validate the merged result before writing.
Only a successful atomic config write establishes a new clean base. Failure
preserves the draft and makes the error visible. A malformed current file is
not silently overwritten by the new save host.

Existing native and egui settings adapt their collected full config to the same
save operation. Their platform-specific forms, previews, shortcut recorders,
autostart actions and flag-gallery workflow stay in the desktop host. The pure
draft/reducer performs no disk, clipboard, input-hook, network or OS work.

## Pixel-style GUI

Use the existing egui/eframe stack: square borders, a restrained retro palette,
monospace labels, clear focus, scalable readable controls. Use semantic widgets
and the existing Unicode font fallback. This is a pixel-inspired form renderer,
not a port of Pixel SSH's framebuffer/WebGL shaders. No borrowed font assets,
CRT effects or new UI dependency is required for v1.

Correction contains automatic correction, switch OS layout, sensitivity,
minimum length, Space delay, and the shared mid-word/context explanations.
Exceptions supports adding, editing and deleting Unicode rows, including long
values in a scrolling area. Save, Revert and Reload are explicit actions. Show
unsaved state, save errors and successful save status. Confirm discarding dirty
state on Reload/close, including text pending in the Add words field. Save also
includes that pending text; Revert is an explicit discard action. Disable Save while
fields are invalid. Keyboard Tab/Shift+Tab, selection, paste and ordinary text
editing use egui semantics. Scale and scroll instead of clipping on small windows.

## Local terminal interface

A line-oriented ANSI interface is sufficient for v1: retain the terminal's
Unicode editing/paste and avoid raw-mode/global keyboard hooks. Commands cover
tab selection, correction fields, exception add/edit/remove, Save, Revert,
Reload, help and quit. Document command syntax on screen. Dirty Reload/quit
requires an explicit discard variant; EOF never saves. Sanitize control
characters before rendering user/config text into an ANSI terminal. Never
start SSH, a network listener or desktop input automation. Piped input/output
must remain usable without clear-screen escape sequences.

## Launch and persistence

Respect UPYR_CONFIG. Keep the existing settings single-instance guard, so the
background app suppresses global actions while an editor is open. A request
for a different front end while one is open reports that fact instead of
silently opening a second editor. Default launches keep their existing tab
activation behavior. Only the desktop save host persists configuration. New
front ends do not modify login items, input sources or permissions directly.

## Acceptance checks

- AC1: identical action sequences produce identical candidate configs.
- AC2: invalid numeric input stays visible and cannot be saved; reverting clears it.
- AC3: edited Unicode rows normalize correctly; untouched concurrent config survives.
- AC4: read/write failures retain dirty state; a successful save accepts merged values.
- AC5: CLI flag/help handling selects one front end without unexpected GUI launches.
- AC6: pixel GUI displays both screens, long Ukrainian text, keyboard focus and dirty-close prompt.
- AC7: TUI replay edits, saves, reloads and refuses implicit dirty discard; control text cannot inject ANSI.
- AC8: existing settings tests, full workspace tests, formatting, Clippy and engine contract pass.
- AC9: user configuration is unchanged by verification; GUI verification uses an isolated config.
- AC10: tray Settings opens pixel style; a second activation focuses it; Full
  Settings transitions safely to the full editor without losing dirty edits.

No latency, memory or binary-size improvement is claimed. Record build size and
smoke-test evidence. Windows/Linux CI remains configured; local results must
identify the platform actually tested.
