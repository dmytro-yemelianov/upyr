# Settings UI and the Pixel SSH approach

The settings refresh keeps the existing native macOS controls and egui interface,
with shared navigation, search and correction descriptions in
`src/settings/navigation.rs`. The Correction tab explains early switching and
short-word context, and distinguishes the pause after Space from immediate
prefix recognition. Stored tab commands and configuration values remain
compatible. Search accepts multiple words and Ukrainian aliases. Native
multiline labels wrap at the available width, including the mid-word explanation.

## Decision

Use Pixel SSH's separation of application state and rendering as a direction
for further settings work. An optional pixel interface and local terminal
interface are feasible. Do not replace the current settings UI with the whole
Pixel SSH application: its portfolio scenes are not a settings form framework.
The current change shares the catalog and search; it does not yet share the
complete editable draft, validation and effect handling between front ends.

The reviewed repository is `dmytro-yemelianov/pixel-ssh`, clean local revision
`a74e1a5ea71794332280776fce52dd0d3bc2e44b` (2026-11-12 review).

| Part | Evidence in Pixel SSH | Fit for Upyr |
| --- | --- | --- |
| State + events + rendering | `crates/core/src/state.rs`, `events.rs`, `views/`; platform-neutral `InputEvent` and `View` | Good architectural pattern: one settings draft and typed actions, several renderers. |
| Pixel rendering | Indexed framebuffer, separate palettes/fonts, WebGL renderer | Suitable for an optional retro appearance; keep readable scaling and effects off by default for forms. |
| Ukrainian text | `crates/framebuffer/src/font.rs` includes 94 Cyrillic glyphs at 8×8, 8×14 and 8×16, including І/Ї/Є/Ґ | Ukrainian display is already possible; arbitrary user text still needs a Unicode fallback. |
| Controls | `Element` contains Text, Rect, Sprite and Link; keyboard/pointer/resize events drive portfolio state | Editable fields, selection, paste, composition, focus order, errors and shortcut capture need additional controls. |
| Accessibility | Browser canvas has an accessible label and a standard-portfolio link | A settings form needs individual semantic controls, labels and values; the canvas description is not a replacement for those. |
| Terminal transport | ANSI renderer and SSH session host | Reuse the rendering idea for a local TUI. Upyr settings do not need an SSH server or guest authentication. |
| Host effects | Upyr owns config merging, login items, OS input sources, flags and permission checks | Keep these behind host actions; a browser preview must not imply it can change the system layout. |

No Pixel SSH code, fonts or assets are copied by this change. Its font provenance
and attribution records must accompany any future asset reuse.

## Incremental implementation

1. **Done:** shared tab catalog, search and correction explanations used by both
   existing interfaces, preserving activation commands and save behaviour.
2. Extract a pure `SettingsDraft` + `SettingsAction` reducer. Keep save/reload,
   dirty state, validation and concurrent-config merge identical across hosts.
   OS effects return explicit commands executed by the desktop host.
3. Prototype the Correction and Exceptions screens with an optional pixel
   renderer. Validate Ukrainian input, long exceptions, keyboard navigation,
   selection/paste, readable scaling and semantic accessibility before expanding.
4. Add a local ANSI/TUI adapter only if it serves an actual terminal workflow.
   It should use the same draft/actions and persistence, without network services.

A complete migration is justified when the shared model eliminates duplicated
behaviour and the pixel interface preserves ordinary form usability. A new
renderer alone is not an engine-speed improvement. Measure opening time,
resident memory and binary size against the native settings window before
making performance claims.
