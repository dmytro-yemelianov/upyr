# Changelog

Notable user-visible changes to Upyr are recorded here. The project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html); compatibility may
change between minor releases while the version is below 1.0.

## [Unreleased]

### Planned

- Notarized Developer ID distribution for macOS.
- Broader Windows and Linux desktop validation.
- Browser adapter and npm delivery for `upyr-wasm`.

### Changed

- The cross-platform settings UI enables AccessKit again on Windows and Linux.
- Linux/X11 tray support now uses StatusNotifierItem over D-Bus instead of the
  GTK/AppIndicator stack; layout feedback uses a Freedesktop notification.
- The project MSRV is now Rust 1.87 so AccessKit can use the quick-xml-free
  `zbus_xml` parser line.
- macOS preview documentation now names the exact Gatekeeper warning users see
  for the unnotarized build and clarifies that updates are checked manually via
  GitHub Releases.

### Security

- Removed the Linux GTK/glib dependency chain and its accepted advisory entries.
- Kept the compatible `wayland-scanner` backport documented after confirming
  upstream master's scanner is not compatible with the released `wayland-client`
  stack used by `winit`/`eframe`.

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

[Unreleased]: https://github.com/dmytro-yemelianov/upyr/compare/macos-preview-0.3.0...HEAD
[0.3.0]: https://github.com/dmytro-yemelianov/upyr/releases/tag/macos-preview-0.3.0
[0.2.0]: https://github.com/dmytro-yemelianov/upyr/releases/tag/macos-preview-0.2.0
[0.1.0]: https://github.com/dmytro-yemelianov/upyr/releases/tag/macos-preview-0.1.0
