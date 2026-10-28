# Changelog

Notable user-visible changes to Upyr are recorded here. The project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html); compatibility may
change between minor releases while the version is below 1.0.

## [Unreleased]

### Planned

- Notarized Developer ID distribution for macOS.
- Broader Windows and Linux desktop validation.
- Browser adapter and npm delivery for `upyr-wasm`.

## [0.4.1] - 2026-10-27

### Fixed

- Keep accepted terminal corrections with trailing punctuation reversible,
  validate mapped punctuation against the installed keyboard map, and support
  macOS's backquote position for `ґ` in typing replacement.
- Preserve punctuation runs such as `...` without turning earlier periods into
  `ю`; keep a known word's last letter before the remaining marks.
- Cover custom slash commands containing digits, hyphens and underscores with
  generated editing sequences. Add an offline headless corpus/runner and a
  read-only native macOS layout-map integration check.
- Re-arm freshly typed slash-command prefixes after clicks, Escape, navigation,
  shortcuts and edits, without requiring an extra Return. Discard the previous
  tracked span and keep known inline prose unchanged.
- Recognize three-letter terminal words whose first physical punctuation key
  produces a Ukrainian letter (`[ez` → `хуя`, `;er` → `жук`, `];f` → `їжа`).
  Retain dictionary/model confidence, configured exceptions, technical-token
  guards and explicit minimum lengths above four characters.
- Expand the compact Ukrainian lexicon for common initial-letter and colloquial
  word forms, and cover the reported cases in the opt-in Superset input probe.

### Security

- Update `event-listener` to 5.4.2 for RUSTSEC-2026-0221. Document four
  unmaintained-dependency notices with exact, time-limited OSV exceptions and
  check pull requests with OSV before merging.

## [0.4.0] - 2026-10-27

Integrated GlobeSwitch, guarded terminal correction and output-aware feedback.

### Added

- A dedicated Exceptions tab with an editable table for words that automatic correction must always leave alone, replacing the single comma-separated text field on the Automatic tab in both settings UIs.
- An indicator style setting (letters, flag, or both) for the floating language indicator, in both settings UIs.
- Anime Reactions' six application-event cues (auto-correct, manual
  conversion, layout switch, pause, resume, error), rendered offline with the
  ElevenLabs Sound Effects API by the new `tools/generate_event_sound_pack.py`
  and bundled the same way as the Original pack's event cues.
- macOS undo for the last automatic correction (`Cmd+Alt+Z`, configurable in Shortcuts). Restores the original text and exact input source in the same focused field, including terminal and slash-prefix corrections. Available for 30 seconds before further typing, navigation or pointer activity.
- Separate macOS headphone sound limit in Feedback (12% by default, 0–30%), with current output and effective gain. Speakers retain master volume. Headphone cues have softened edges and peak attenuation, never overlap, and stop on output changes. Unknown outputs also use the quiet limit. Schema 12 preserves existing sound preferences during migration.
- A searchable country-flag picker in macOS Feedback settings, showing all 259 bundled country and region flags with names and codes. Assign flags per keyboard layout with live previews, explicit Save Flags, automatic defaults and Revert. Saving merges only edited layouts and preserves other preferences.
- Opt-in line-start slash-command correction in terminal interfaces, including the leading slash (`.сщььфтв` → `/command`), before Space or submission.
- Live embedded flag preview in macOS Feedback settings, with a preview-layout selector, the actual GlobeSwitch renderer and saved per-layout overrides.
- Opt-in macOS terminal/TUI typing replacement for applications listed in `terminal_typing_apps`. A freshly tracked single word is replaced using exact Backspace counts and Unicode key events, without Copy/Paste or Enter. Input, focus changes, cursor controls and mouse activity cancel pending edits. Configuration schema 10 keeps this backend disabled until selected.
- The original GlobeSwitch pixel-flag row on macOS, including its 259 pixel flags, animated centering, accent glow, pointer-screen placement and fade. Select **GlobeSwitch flags** in indicator settings. Configuration schema 9 adds `[globe_switch.layout_flags]` for region or custom-text overrides.
- Integrated GlobeSwitch on macOS: opt-in listen-only Globe-key taps cycle all enabled layouts directly, with configurable tap duration, skipped input sources and recent-layout order. No separate app or helper is required.
- A native General settings control, `upyr globe` configuration/diagnostic commands, and `upyr globe migrate` to uninstall standalone GlobeSwitch before enabling its replacement. Pause/quit restore macOS handling; `globe restore` recovers the saved preference after an unclean exit.
- Globe switches use Upyr feedback and invalidate pending automatic corrections.

### Changed

- Configuration schema 7 adds `indicator_style`; migrations keep today's combined letters-and-flag presentation.
- Configuration schema 8 adds `[globe_switch]`. Upgrades leave the feature off and preserve all existing correction, shortcut and feedback settings.
- The cross-platform settings UI enables AccessKit again on Windows and Linux.
- Linux/X11 tray support now uses StatusNotifierItem over D-Bus instead of the GTK/AppIndicator stack; layout feedback uses a Freedesktop notification.
- The project MSRV is now Rust 1.87 so AccessKit can use the quick-xml-free `zbus_xml` parser line.
- macOS preview documentation now names the exact Gatekeeper warning users see for the unnotarized build and clarifies that updates are checked manually via GitHub Releases.
- README and upyr.org now document the crates.io Cargo install and Homebrew tap install paths alongside the GitHub Releases preview.
- The install-critical `upyr`, `upyr-core`, and `upyr-audio` crates are now published to crates.io.

### Fixed

- Word and prefix conversion no longer strands the caret when the focused app (notably terminal emulators) drops the Shift modifier from the selection chord instead of extending a selection with it: Upyr now detects the resulting empty copy and moves the caret back to its original position.
- Capture the undo target at the correction itself. Slash-command tracking can outlive a focused editor within the same window; undo now binds to the field actually edited while still refusing changes after focus moves elsewhere.
- Send terminal replacements as single-character keypresses. Multi-character Unicode input can take Chromium's composition path and let a following letter reach the terminal before the replacement. Add an opt-in Superset regression probe covering real terminal input, undo and focus changes.
- Ignore nonactivating windows from other processes when validating terminal focus. A desktop overlay above Superset must not reject a valid word edit.
- Commit terminal layout changes only after the target accepts the word edit; cancelled corrections no longer switch layouts silently. Successful automatic replacements show the active-layout indicator even when that layout was already selected.
- Prefer a known complete Ukrainian word over a punctuation-preserving guess from its shorter stem (`hjpevs.` → `розумію`, retaining the final `ю`).
- Deliver terminal Backspace/typing as one target-process event-stream batch; prevent a following letter from being consumed by a delayed Backspace. Use window identity and native input counters on the terminal edit path.
- Apply the short-word terminal policy in both directions (`nfr` → `так`, `zr` → `як`) and accept physical punctuation keys that form Ukrainian letters (`lj,ht` → `добре`, `[jnskjcz` → `хотілося`) in the typing backend.
- Recognize English contractions such as `it's`, and evaluate terminal words individually rather than rejecting a correction containing an earlier ambiguous phrase. Common short English words (`not`, `so`, `for`, `the`) can establish the English layout under the default terminal policy.
- Preserve terminal target identity by frontmost application/window when an Electron terminal does not expose an Accessibility text element.
- Select the destination macOS layout before sending Paste. Changing it while Cmd+V is queued can make macOS replay the event and insert the text twice.
- Run macOS automatic-correction observation through AppKit's main-thread event monitor. Avoid rdev's background Carbon translation, which terminates the process on macOS versions that enforce the input-source queue contract.
- Keep the macOS event loop responsive during Copy/Paste delays. Conversion phases use scheduled deadlines, cancel before paste if input or focus changes, and resume word tracking before delayed clipboard cleanup. Cleanup failures never move the caret after a committed replacement.
- Restore Rust 1.86 compatibility in the desktop word tracker; its let-chain required a newer compiler than the workspace's declared minimum.
- Ignore punctuation when detecting the dominant script for smart conversion; text such as `a....` now converts from English instead of treating periods as Ukrainian letters.
- Preserve clipboard data copied by another application while Upyr's temporary value or delayed clipboard restoration is pending. Native change counters distinguish even a later copy of identical text.
- Apply the correction engine's exact replacement after verifying a GUI selection; do not recompute it from already rendered, potentially mixed text.
- Skip unverified automatic terminal edits instead of deleting words with Ctrl+W and assuming Ctrl+Y can restore an arbitrary terminal program.
- Replace shortcut registrations transactionally and retain the previous keyboard monitors if a reloaded shortcut is occupied. Pausing, resuming and reloading cancel stale automatic corrections on every desktop platform.
- Replace an unrelated persisted code graph with Upyr's graph; check repository identity and source-file coverage in CI, and normalize indexer JSON escaping.

### Security

- Update `webbrowser` to 1.2.2 for RUSTSEC-2026-0257 and replace yanked `clipboard-win` 5.4.1 with 5.4.3.
- Removed the Linux GTK/glib dependency chain and its accepted advisory entries.
- Kept the compatible `wayland-scanner` backport documented after confirming upstream master's scanner is not compatible with the released `wayland-client` stack used by `winit`/`eframe`.

## [0.3.0] - 2026-07-19

Detection and public-site preview.

### Added

- High-confidence trigger rules can now match exact words, prefixes, suffixes,
  and contains patterns (`word`, `word*`, `*word`, `*word*`). This is Upyr's
  clean-room counterpart to Punto Switcher for Windows consuming its `A` tag as
  an any-position wildcard.
- Recall and performance benchmark coverage for the trigger layer.
- English and Ukrainian reverse-engineering articles for the Punto Switcher
  rule format and the Mac/Windows `A`-tag divergence.
- A multipage product site, live Cloudflare Pages deployment at `upyr.org`, and
  article-level navigation with an English/Ukrainian language selector.

### Changed

- The trigger layer now uses wildcard-aware matching instead of exact-only
  comparisons for built-in high-confidence rules.
- Public project links now point at `https://upyr.org/`.
- Documentation now clearly separates Upyr's clean-room behavior from Punto's
  data files and binary-specific rule semantics.

### Security

- Added OSV, Trivy, SBOM, and Cosign-backed supply-chain checks around release
  and deployment workflows.
- Documented the currently accepted Linux GTK/egui advisory exposure and keeps
  it on a dated revisit path while the gtk3 replacement work remains parked.

## [0.2.0] - 2026-07-17

Sound-pack and keyboard-feedback preview.

### Added

- Upyr Original, Pocket Arcade, and Anime Reactions sound packs with native
  AppKit and cross-platform settings controls.
- Dependency-free `upyr-audio` synthesis with deterministic pfxr-style cue
  variations and locally generated, non-sampled formant reactions for
  non-character keys in the Anime pack.
- Opt-in physical-key feedback for characters, Space, Enter, Escape, Tab,
  modifiers, Caps Lock, Delete, Backspace, navigation, and function keys.

### Changed

- Configuration schema 6 adds `sounds.pack` and `sounds.key_clicks`; migrations
  preserve the original event theme and keep keyboard monitoring disabled.
- Keyboard cues branch from raw physical events before autocorrect filtering,
  are cached by pack/cue/variant, and are rate-limited for comfortable typing.
- Every pack now synthesizes both event and key cues locally. Pre-rendered raw
  audio assets were removed from the source tree and distributable packages.

### Security

- Audio synthesis is local-only and receives physical key categories rather
  than rendered text. It uses no recordings, microphone, network service,
  telemetry, or new third-party runtime dependency.
- Privacy CI and automated review reject pre-rendered audio assets in
  distributable source paths.
- Upyr drops keyboard cues while its own synthetic replacement input is active,
  preventing copy, paste, and correction keystrokes from producing feedback.

## [0.1.0] - 2026-07-17

Initial public preview.

### Added

- English ↔ Ukrainian correction for selected text and the previous word.
- Opt-in automatic correction with physical-key mapping, punctuation and case
  preservation, technical-token guards, and a compact local signed character
  n-gram model.
- Native macOS input-source integration, rich pasteboard snapshot/restore,
  Accessibility permission lifecycle, searchable AppKit settings, About page,
  language indicator, event sounds, and launch-at-login controls.
- Preview Windows, Linux/X11, and DOM-independent WebAssembly targets.
- Reproducible model generation and frozen mixed-language evaluation fixtures.
- Structured issue forms, private vulnerability reporting, CodeQL, RustSec,
  dependency review, privacy/version regression checks, and provenance-ready
  release workflows.

### Security

- Desktop and WASM runtime paths contain no network, telemetry, analytics, or
  remote-inference client.
- macOS and Linux configuration writes are atomic and owner-readable only.
- Tagged macOS and Windows releases fail closed when signing credentials are
  absent; macOS publication also requires successful notarization.

### Known limitations

- macOS is the primary supported target. Windows and Linux/X11 are previews.
- Native Wayland global input is not supported.
- Public preview artifacts may be explicitly marked unnotarized; users should
  check each release's signing status before installing.

[Unreleased]: https://github.com/dmytro-yemelianov/upyr/compare/macos-preview-0.4.1...HEAD
[0.4.1]: https://github.com/dmytro-yemelianov/upyr/releases/tag/macos-preview-0.4.1
[0.4.0]: https://github.com/dmytro-yemelianov/upyr/releases/tag/macos-preview-0.4.0
[0.3.0]: https://github.com/dmytro-yemelianov/upyr/releases/tag/macos-preview-0.3.0
[0.2.0]: https://github.com/dmytro-yemelianov/upyr/releases/tag/macos-preview-0.2.0
[0.1.0]: https://github.com/dmytro-yemelianov/upyr/releases/tag/macos-preview-0.1.0
