# Upyr

[![CI](https://github.com/dmytro-yemelianov/upyr/actions/workflows/ci.yml/badge.svg)](https://github.com/dmytro-yemelianov/upyr/actions/workflows/ci.yml)
[![Security](https://github.com/dmytro-yemelianov/upyr/actions/workflows/security.yml/badge.svg)](https://github.com/dmytro-yemelianov/upyr/actions/workflows/security.yml)
[![MIT License](https://img.shields.io/badge/license-MIT-58d6a8.svg)](LICENSE)

Upyr is a private, native English ↔ Ukrainian keyboard-layout fixer written in Rust. It turns text typed on the wrong physical layout into what you meant: `ghbdsn` → `привіт`, `руддщ` → `hello`.

**Release status:** v0.2.0 public preview. **The current preview download is not Apple Developer ID signed and is not notarized; its macOS bundle is ad-hoc signed only, so Gatekeeper may block it.** macOS is the primary supported desktop target. Release CI is configured to build and smoke-test Windows and Linux/X11 packages, but those platforms remain preview targets. Upyr follows [Semantic Versioning](#versioning-and-releases); compatibility may change before 1.0.

[Download from GitHub Releases](https://github.com/dmytro-yemelianov/upyr/releases) · [Product page](https://dmytro-yemelianov.github.io/upyr/) · [Report an issue](https://github.com/dmytro-yemelianov/upyr/issues/new/choose)

## What it does

- Fixes selected text with `CmdOrCtrl+Alt+Space`.
- Fixes the word before the caret with `CmdOrCtrl+Alt+Backspace`.
- Optionally corrects a confidently mistyped word after Space; automatic correction is off by default.
- Detects and switches between installed English and Ukrainian input sources.
- Uses the physical keys, including the standard Ukrainian punctuation row: `[];'\,./` ↔ `хїжєґбю.`.
- Protects intentional technical text such as `FAANG`, `SaaS`, `NASDAQ`, `iPhone`, URLs, paths, and configured exceptions.
- Provides searchable, tabbed settings, press-to-record shortcuts, an optional pointer-side language flag, and three locally synthesized sound packs with event/key controls and master volume.
- By default, snapshots supported clipboard formats and attempts to restore them after conversion; temporary conversion content is concealed from supported clipboard-history systems.

The menu-bar or system-tray app can convert text, pause or resume correction, open Settings, reload configuration, manage launch at login, and quit.

## Install on macOS

1. Open the [Releases page](https://github.com/dmytro-yemelianov/upyr/releases), read the signing status in the notes, and download the universal macOS DMG or ZIP.
2. Move **Upyr.app** to `/Applications` and launch it.
3. Grant **Accessibility** access when macOS asks. Upyr needs it to observe opted-in word boundaries and send Copy/Paste keystrokes.
4. Return to Upyr. When a new grant is detected, the app offers to restart so every input monitor starts with the permission.

Release builds target macOS 11 or newer and contain Apple Silicon and Intel binaries. Tagged macOS releases fail closed unless signing and notarization complete; check the release notes and artifact provenance rather than bypassing Gatekeeper.

## Build and run

Upyr requires Rust 1.86 or newer.

```sh
cargo build --release --locked
./target/release/upyr doctor
./target/release/upyr settings
./target/release/upyr convert ghbdsn
```

On Windows, use `target\release\upyr-background.exe` for the tray app without a console window. On Ubuntu/Debian, install the desktop dependencies first:

```sh
sudo apt-get update
sudo apt-get install -y libx11-dev libxtst-dev libxkbcommon-dev \
  libwayland-dev libgtk-3-dev libayatana-appindicator3-dev
```

To build a local universal macOS bundle:

```sh
packaging/macos/generate-icon.sh
packaging/macos/build-universal.sh
packaging/macos/package.sh
```

Local packages are ad-hoc signed for development. Public release artifacts use the stricter release signing pipeline.

## How correction works

Upyr does not translate words. It reconstructs the characters produced by the same physical keys on the other layout, then decides whether the reconstructed text is more plausible.

1. The native input hook records physical key positions for the current short input boundary.
2. The installed EN/UK layouts provide the positional map; a deterministic built-in map is the fallback.
3. Upyr creates source and opposite-layout candidates while preserving case and punctuation.
4. Exact dictionaries, technical-token guards, exceptions, and a compact language model score the pair.
5. Only a candidate that clears the selected confidence policy is applied. Upyr replaces the text, switches the OS input source when configured, and, by default, attempts to restore the clipboard.

The embedded model is a **signed character n-gram index**, not a general language model. It contains 173,964 packed 2–5-character records in about 2.8 MiB. Each record stores a character-sequence key and one signed confidence byte: negative for English, positive for Ukrainian. Evidence from the typed candidate is accumulated locally and looked up with binary search.

The index is generated from these pinned, checksum-verified Leipzig Corpora Collection inputs, each distributed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/):

- English: [`eng_news_2023_1M`](https://downloads.wortschatz-leipzig.de/corpora/eng_news_2023_1M.tar.gz), the 2023 English news snapshot with 1,000,000 sentences; SHA-256 `c8a5a5e72897aa5e367b0319c1884831c02aaf29bf81342de31ca1b1cc8f3e4c`.
- Ukrainian: [`ukr_news_2023_1M`](https://downloads.wortschatz-leipzig.de/corpora/ukr_news_2023_1M.tar.gz), the 2023 Ukrainian news snapshot with 1,000,000 sentences; SHA-256 `0901bff8b3fdb3a8c657137754b4214b8ea6f241572d3ff9b2ae718487412383`.

The provider terms and requested attribution are recorded in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md). Training tokens are alphabet-filtered and split into character n-grams; corpus sentences and word-frequency tables are not embedded in the app. The generator is reproducible:

```sh
python3 tools/generate_ngram_model.py
```

Model and policy behavior is measured against the frozen [signed N-gram v1 evaluation](docs/benchmarks/signed-ngram-v1.md), covering both directions, native text, names, punctuation, technical identifiers, contextual boundaries, and reported physical-key edge cases.

## Privacy and security

Upyr is designed to work without an account, server, or runtime network connection.

- **No telemetry or analytics.** The desktop and WASM runtime paths have no HTTP client, updater, crash reporter, advertising SDK, or analytics integration.
- **No remote inference.** Layout mapping, n-gram scoring, dictionaries, and settings stay on the device.
- **No typing history.** Automatic mode keeps only a bounded in-memory prefix for the active input boundary. It does not write typed text to logs or disk; resets discard the prefix.
- **Private configuration writes.** On macOS and Linux, Upyr replaces configuration atomically and enforces owner-only `0600` file permissions; Windows uses the per-user application-data ACL.
- **Opt-in observation.** Automatic correction, modifier gestures, sounds, and the language indicator are disabled by default.
- **Clipboard protection.** By default, Upyr snapshots supported clipboard formats and attempts to restore them. Restoration can be disabled, and an unsupported format or platform failure can prevent a complete restore. On macOS it uses the native pasteboard change counter and concealed-data hint instead of a text sentinel.
- **Permission restraint.** macOS Accessibility is requested only when required, an existing grant is accepted silently, and a missing grant is prompted at most once per process.
- **Inspectable artifacts.** The implementation is MIT-licensed, release builds are produced by GitHub Actions, and the release pipeline smoke-tests packaged conversion.

The corpus generator is the one intentional network-capable development tool: when explicitly run, it downloads pinned public archives and verifies SHA-256 before processing. It is not called by the desktop app or WASM runtime.

Security CI adds RustSec dependency auditing, CodeQL analysis, pull-request dependency review, and scheduled scans. These controls reduce risk; they are not a claim that any non-trivial program is vulnerability-free. Please follow [`SECURITY.md`](SECURITY.md) to report a suspected vulnerability instead of opening a public issue.

Run the local verification suite with:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
python3 tools/check_privacy.py
python3 tools/check_version_sync.py
python3 tools/check_codebase_graph.py
cargo audit
```

For an independent non-tracking review, inspect the normal dependency graph with `cargo tree --workspace --edges normal --locked` and search `src/` plus `crates/` for network clients, telemetry endpoints, or analytics SDK initialization. The [Security workflow](https://github.com/dmytro-yemelianov/upyr/actions/workflows/security.yml) records the automated checks for each revision.

## Configuration

Create or inspect the configuration with:

```sh
upyr init
upyr config-path
upyr autostart status
upyr autostart enable
upyr settings
```

The settings app exposes General, Automatic, Shortcuts, Feedback, and Advanced tabs. Automatic sensitivity can be conservative, balanced, or aggressive; deliberate strings can be added to `auto_correct_exceptions`. Feedback includes three local sound packs: Upyr Original, Pocket Arcade, and the opt-in Anime Reactions pack. Every event and key cue is synthesized locally; keyboard feedback crosses into the app as physical key categories only and never stores typed characters. Use `UPYR_CONFIG` to select a different config file.

The versioned TOML schema currently uses `config_version = 6`. Older supported schemas migrate in memory; configurations from a newer unsupported schema are rejected rather than guessed. Keyboard sounds remain disabled during migration and by default. See the generated default file after `upyr init` for every option and its current value.

Sound-pack synthesis is offline and dependency-free in `upyr-audio`. Its pfxr-style engine creates short deterministic variations, while Anime Reactions uses a procedural glottal/formant synthesizer for non-character keys. All cues are generated on-device; no recordings or pre-rendered sound files are bundled, and there are no downloads, microphone access, or remote audio-service calls. Playback uses the operating system's native audio API. Audible feedback can reveal typing rhythm to people nearby, so keyboard sounds are always opt-in; headphones are recommended for the vocal-reaction pack.

## Platform status

| Platform | Status | Notes |
| --- | --- | --- |
| macOS 11+ | Primary public-preview target | Native AppKit settings and feedback, universal Apple Silicon/Intel package, native input-source and rich pasteboard integration |
| Windows | Preview | Tray app, installer and portable ZIP, foreground-layout mapping, broad clipboard snapshot/restore support; the settings screen-reader bridge is temporarily unavailable |
| Linux/X11 | Preview | DEB and tar package, XKB layout mapping and GTK tray/feedback; the settings screen-reader bridge is temporarily unavailable, and native Wayland global input awaits a portal-backed design |
| WebAssembly | Engine preview | DOM-independent `upyr-core` binding with generated TypeScript contracts; browser adapter and npm delivery are planned |

## Architecture

```text
upyr-core          platform-neutral mapping, tracking, n-gram policy
upyr-wasm          DOM-independent WebAssembly API
upyr-audio         dependency-free pfxr clicks and procedural formant reactions
upyr               CLI and native background application
├── input hooks    physical key events and permission lifecycle
├── automation     guarded selection, replacement, clipboard snapshot/restore
├── system layout  installed EN/UK mapping and source switching
└── settings       AppKit on macOS; cross-platform UI elsewhere
```

The shared core has no operating-system or network dependency. Desktop adapters own permissions, input events, clipboard access, layout switching, feedback, and autostart. The WASM host contract is documented in [`crates/upyr-wasm/README.md`](crates/upyr-wasm/README.md).

## Versioning and releases

Upyr uses SemVer for app and crate versions. While the project is in `0.x`, minor releases may contain compatibility changes; patch releases are intended for backwards-compatible fixes. Git tags use `vMAJOR.MINOR.PATCH` and must exactly match the workspace package version before release packaging starts.

Explicitly unsigned development previews use a non-`v*` tag such as `macos-preview-0.1.0`; they are prereleases, do not enter the official tag publisher, and must state their unnotarized status prominently.

The app version, configuration schema, and n-gram model version are separate on purpose. A model or policy update does not imply a configuration migration, and a compatible app patch does not silently rename the model contract. Release notes identify platform support, signing status, migration impact, and model changes.

User-visible changes and known limitations are maintained in [`CHANGELOG.md`](CHANGELOG.md).

Tagged releases run formatting, Clippy, tests, security gates, package smoke tests, and platform packaging. macOS tags require a Developer ID identity and complete notarization credentials; the workflow refuses to publish a partially signed official macOS release.

## About and inspiration

The name **Upyr** comes from the Ukrainian *упир*: a shape-changing figure in folklore—an apt name for software that flips mistyped text into its intended form.

Upyr is an independent Rust implementation inspired by Punto Switcher and [TolikPylypchuk/KeyboardSwitch](https://github.com/TolikPylypchuk/KeyboardSwitch). KeyboardSwitch helped validate selected-text correction, following conversion with the matching OS input source, generated physical-key mappings, configurable modifier gestures, and careful startup/config migration. Upyr is not a port and shares no KeyboardSwitch code.

The native macOS input-source binding was adapted from the MIT-licensed [issw](https://github.com/0xAndoroid/issw); its notice is retained in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

Contributions and reproducible bug reports are welcome. Start with the [issue forms](https://github.com/dmytro-yemelianov/upyr/issues/new/choose) and include the OS version, Upyr version, input sources, expected text, typed physical sequence, and actual result. By participating, you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

[MIT](LICENSE)
