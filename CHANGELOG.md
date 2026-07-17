# Changelog

Notable user-visible changes to Upyr are recorded here. The project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html); compatibility may
change between minor releases while the version is below 1.0.

## [Unreleased]

### Planned

- Notarized Developer ID distribution for macOS.
- Broader Windows and Linux validation and restored screen-reader integration
  for the cross-platform settings UI.
- Browser adapter and npm delivery for `upyr-wasm`.

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
- The Windows/Linux settings screen-reader bridge is temporarily unavailable.
- Public preview artifacts may be explicitly marked unnotarized; users should
  check each release's signing status before installing.

[Unreleased]: https://github.com/dmytro-yemelianov/upyr/compare/macos-preview-0.1.0...HEAD
[0.1.0]: https://github.com/dmytro-yemelianov/upyr/releases/tag/macos-preview-0.1.0
