# Upyr

[![CI](https://github.com/dmytro-yemelianov/upyr/actions/workflows/ci.yml/badge.svg)](https://github.com/dmytro-yemelianov/upyr/actions/workflows/ci.yml)
[![Security](https://github.com/dmytro-yemelianov/upyr/actions/workflows/security.yml/badge.svg)](https://github.com/dmytro-yemelianov/upyr/actions/workflows/security.yml)
[![OSV-Scanner](https://github.com/dmytro-yemelianov/upyr/actions/workflows/osv-scanner.yml/badge.svg)](https://github.com/dmytro-yemelianov/upyr/actions/workflows/osv-scanner.yml)
[![Rust 1.87+](https://img.shields.io/badge/rust-1.87%2B-b7410e.svg?logo=rust)](#build-and-run)
[![No telemetry](https://img.shields.io/badge/telemetry-none-58d6a8.svg)](#privacy-and-security)
[![License: MIT](https://img.shields.io/badge/license-MIT-58d6a8.svg)](LICENSE)

Upyr is a private, native English ↔ Ukrainian keyboard-layout fixer written in Rust. It turns text typed on the wrong physical layout into what you meant: `ghbdsn` → `привіт`, `руддщ` → `hello`.

**Release status:** v0.4.1 public preview. **There is no Apple-verified Upyr build yet.** The current macOS preview is ad-hoc signed, not Apple Developer ID signed, and not notarized; Gatekeeper may show "Apple could not verify Upyr is free of malware." Upyr has no automatic update checker, so check GitHub Releases manually for newer or notarized builds. macOS is the primary supported desktop target. Release CI is configured to build and smoke-test Windows and Linux/X11 packages, but those platforms remain preview targets. Upyr follows [Semantic Versioning](#versioning-and-releases); compatibility may change before 1.0.

[Download from GitHub Releases](https://github.com/dmytro-yemelianov/upyr/releases) · [Package-manager installs](#package-manager-installs) · [Product page](https://upyr.org/) · [Report an issue](https://github.com/dmytro-yemelianov/upyr/issues/new/choose)

## What it does

- Fixes selected text with `CmdOrCtrl+Alt+Space`.
- Fixes the word before the caret with `CmdOrCtrl+Alt+Backspace`.
- Optionally corrects a confidently mistyped word after Space; automatic correction is off by default.
- On macOS, `Cmd+Alt+Z` undoes the last automatic correction and restores its original input source, before further typing or navigation (up to 30 seconds).
- Detects and switches between installed English and Ukrainian input sources.
- Includes GlobeSwitch on macOS: a clean 🌐 tap cycles enabled layouts instantly,
  without macOS's picker or a second background application.
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

Release builds target macOS 11 or newer and contain Apple Silicon and Intel binaries. If macOS says Apple could not verify Upyr, that means the downloaded preview is not notarized; it is not an in-app update prompt and it is not something Upyr can fix after download. The recommended paths are to wait for a Developer ID/notarized release tracked in [issue #4](https://github.com/dmytro-yemelianov/upyr/issues/4), verify the release artifacts yourself, or build from source. Tagged macOS releases fail closed unless signing and notarization complete; check the release notes and artifact provenance rather than bypassing Gatekeeper.

## Package-manager installs

These channels build Upyr from source. They avoid the downloaded `.app` Gatekeeper quarantine path, but they are not Apple-notarized app bundles. On macOS, grant Accessibility permission to the `upyr-background` process when prompted.

Install from crates.io with Cargo:

```sh
cargo install upyr --locked
upyr convert ghbdsn
upyr settings
```

Install from the Homebrew tap:

```sh
brew tap dmytro-yemelianov/upyr
brew install upyr
brew services start upyr
upyr settings
```

For source-latest testing before the next package release, install directly from Git with `cargo install --git https://github.com/dmytro-yemelianov/upyr --locked --force`.

## Build and run

Upyr requires Rust 1.87 or newer.

```sh
cargo build --release --locked
./target/release/upyr doctor
./target/release/upyr settings
./target/release/upyr convert ghbdsn
```

On Windows, use `target\release\upyr-background.exe` for the tray app without a console window. On Ubuntu/Debian, install the desktop dependencies first:

```sh
sudo apt-get update
sudo apt-get install -y libx11-dev libxtst-dev libxkbcommon-dev
```

To build a local universal macOS bundle:

```sh
packaging/macos/generate-icon.sh
packaging/macos/build-universal.sh
packaging/macos/package.sh
```

Local packages are ad-hoc signed for development. Public release artifacts use the stricter release signing pipeline.

## Globe key on macOS

GlobeSwitch is built into Upyr's background process. Enable **Settings → General
→ Tap Globe (🌐) to switch keyboard layouts instantly**, or run `upyr globe enable`.
Upyr needs Input Monitoring or Accessibility access; restart it after granting
access if the monitor could not start. `upyr doctor` and `upyr globe status`
report the permission and system-key setting without requesting access.

If standalone GlobeSwitch is installed, run `upyr globe migrate` instead. This
uses its uninstaller to remove the old app and LaunchAgent and restore its
saved system setting before enabling Upyr's integrated switcher. The command
does not launch a second agent. Without the standalone uninstaller, uninstall
GlobeSwitch manually first; Upyr refuses to run two Globe handlers together.

A clean tap released within 600 ms selects the next enabled input source
directly through Carbon. There is no picker, no helper process and no clipboard
access on this path. All enabled keyboard layouts and input modes can be cycled,
including ones outside Upyr's English/Ukrainian correction pair. Holds,
fn+arrows, fn+E, media keys, other modifiers, clicks and scrolling cancel a tap.
The event tap is listen-only and always returns the original keyboard event.

While this option runs, Upyr saves **Press 🌐 key to**, sets it to **Do Nothing**
and refreshes macOS's input helpers so they do not switch a second time. Pausing,
quitting or disabling the option restores the saved setting, unless you changed
it yourself in System Settings. After a crash, `upyr globe restore` disables the
option and restores that durable backup. A live Upyr monitor owns the setting
until it stops; another process cannot claim it, even with a different config.

Advanced options live in the same TOML configuration as correction:

```toml
[globe_switch]
enabled = true
tap_window_ms = 600              # 100–2000
cycle_order = "list"             # or "recent" for the previous used layout
skipped_layouts = []             # input-source IDs from `upyr globe list`
```

Globe switches share Upyr's optional indicator and layout-switch sound. On macOS,
select **GlobeSwitch flags** (`indicator_style = "flag"`) to use the original
pixel-flag row near the bottom of the screen under the pointer: the active flag
is larger and glowing, the row slides with spring animation, then fades out.
The panel ignores clicks and cannot become the keyboard's key or main window.
Layouts without a flag show their name. Switching
also clears the correction context and invalidates corrections queued before
the tap. Automatic correction, sounds and the indicator retain their separate
opt-in settings. Region codes and custom text can be configured per input source:

```toml
[globe_switch.layout_flags]
"com.apple.keylayout.ABC" = "GB"
```

Configuration schema 9 adds these overrides; upgrades preserve existing feedback
choices. Migration uses Upyr's sound preferences.

To exercise the running integrated monitor without pressing a physical key,
compile `tools/globe-probe.swift` with `swiftc -O -o target/globe-probe
tools/globe-probe.swift`. The terminal needs Accessibility access. Run
`target/globe-probe press 4 400`, then `chord`, `hold`, `scroll`, or `type 5 40`.
The typing probe opens its own field and refuses to type into another app.
Synthetic taps exercise Upyr; physical-key testing is still needed to check
macOS's own Globe handling after a system update.

`target/globe-probe convert plain` checks selected-text conversion in its own
field using the default shortcut. `convert busy` checks a Globe tap while
`copy_delay_ms = 1000`; the pending conversion must cancel. `convert paste`
checks a Globe tap after replacement while `restore_delay_ms = 1800`.
Enable integrated Globe switching for the latter two probes and disable
automatic correction for these controlled checks. Each probe verifies focus
before sending input and fails if its own window cannot become active.
`convert auto` requires automatic correction and the ABC layout; it types
`ghbdsn ` into its own field and checks for `привіт `, preserved focus and
clipboard restoration. It selects ABC for the test; restore your preferred
input source afterward.

## How correction works

Upyr does not translate words. It reconstructs the characters produced by the same physical keys on the other layout, then decides whether the reconstructed text is more plausible.

1. The native input hook records physical key positions for the current short input boundary.
2. The installed EN/UK layouts provide the positional map; a deterministic built-in map is the fallback.
3. Upyr creates source and opposite-layout candidates while preserving case and punctuation.
4. Deterministic triggers, exact dictionaries, technical-token guards, exceptions, and a compact language model score the pair.
5. Only a candidate that clears the selected confidence policy is applied. Upyr replaces the text, switches the OS input source when configured, and, by default, attempts to restore the clipboard.

On macOS, Copy/Paste delays yield to the application event loop so Globe taps
and menu actions can still run. A temporary activity guard cancels a pending
replacement if input or focus changes; it observes activity without recording
characters. Clipboard cleanup continues after replacement without pausing
tracking for the next word, and preserves anything copied meanwhile.
Automatic correction uses AppKit's main-thread global event monitor and the
event's rendered characters, avoiding background Carbon layout queries.

The trigger layer covers high-certainty words that the statistical policy deliberately skips, especially short words. Built-in rules are exact physical-key matches today; the parser also supports leading/trailing `*` patterns (`word*`, `*word`, `*word*`) after the Punto reverse-engineering pass showed that Punto for Windows treats its `A` tag as an any-position wildcard. Upyr uses the finding as a rule-shape hint only; it does not ship Punto's rule data.

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

The corpus generator and the sound-pack asset generator are the only intentional network-capable development tools: when explicitly run, the former downloads pinned public archives and verifies SHA-256 before processing, and the latter calls the ElevenLabs Sound Effects API to render bundled event cues. Neither is called by the desktop app or WASM runtime.

Security CI adds RustSec dependency auditing, CodeQL analysis, pull-request dependency review, and scheduled scans. These controls reduce risk; they are not a claim that any non-trivial program is vulnerability-free. Please follow [`SECURITY.md`](SECURITY.md) to report a suspected vulnerability instead of opening a public issue.

Run the local verification suite with:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
python3 tools/check_privacy.py
python3 tools/check_site_content.py
python3 -m unittest tools.test_check_site_content tools.test_check_privacy tools.test_check_codebase_graph
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

The settings app exposes General, Automatic, Exceptions, Shortcuts, Feedback, and Advanced tabs. Automatic sensitivity can be conservative, balanced, or aggressive; deliberate strings can be added on the Exceptions tab. Feedback includes three local sound packs: Upyr Original, Pocket Arcade, and the opt-in Anime Reactions pack. Every key cue and most event cues are synthesized locally (the Anime pack's six application-event cues are the one exception; see below); keyboard feedback crosses into the app as physical key categories only and never stores typed characters. Use `UPYR_CONFIG` to select a different config file.

The versioned TOML schema currently uses `config_version = 12`. Older supported schemas migrate in memory; configurations from a newer unsupported schema are rejected rather than guessed. Migration preserves existing sound choices and adds `undo_hotkey = "CmdOrCtrl+Alt+Z"` (with a fallback if a conversion shortcut already uses it) and `sounds.headphone_volume_percent = 12`. See the generated default file after `upyr init` for every option and its current value.

On macOS, **Feedback → Headphone limit** controls a separate ceiling for Upyr's
sounds (0–30%, initially 12%). Speakers keep the existing master volume;
headphones use the lower of master volume and this ceiling. Unidentified outputs
also use the quiet profile. Upyr detects the current system sound output for
each cue, softens its attack and release, limits source peaks without amplifying
quiet samples, and avoids overlapping cues on headphones. Output changes stop
old cues; sounds are bound to their observed device. Settings shows the current
output and effective app gain. This changes only Upyr's gain, not macOS volume;
the percentage is not a measurement of sound pressure at the ear.

Sound-pack synthesis is offline and dependency-free in `upyr-audio`. Its pfxr-style engine creates short deterministic variations, while Anime Reactions uses a procedural glottal/formant synthesizer for non-character keys—there are no voice recordings, downloads, microphone access, or remote audio-service calls at runtime. Anime Reactions' six application-event cues (auto-correct, manual conversion, layout switch, pause, resume, error) are the one exception: they are bundled, pre-rendered assets, generated once offline with the ElevenLabs Sound Effects API by [`tools/generate_event_sound_pack.py`](tools/generate_event_sound_pack.py) and embedded in the binary (see [`assets/sounds/anime/README.md`](assets/sounds/anime/README.md)); like the n-gram corpus generator, this tool is a manual, developer-run step and is never invoked by the desktop app. Every other cue—every key click, and every other pack's event sounds—is synthesized on-device with no bundled audio file at all. Playback uses the operating system's native audio API. Audible feedback can reveal typing rhythm to people nearby, so keyboard sounds are always opt-in; headphones are recommended for the vocal-reaction pack.

## Platform status

On macOS, terminal/TUI input can opt into clipboard-free replacement:

```toml
terminal_typing_apps = ["com.superset.desktop"]
slash_command_apps = ["com.superset.desktop"]
```

This supports normal text entry in Codex/Claude terminal prompts in Superset.
After Space, Upyr replaces only a single word it just tracked in the same
focused window: it selects the destination layout, sends the exact number of
Backspaces and emits Unicode text. It never sends Enter or word-kill shortcuts.
The clipboard is untouched. Cursor/control keys, clicks, scrolling or a changed
focus cancel pending work. Further typing cancels a delayed correction; during
delivery of a prepared edit, text keys retain their order after the replacement.
Paste, history edits, multiword
rewrites and editor modes that reinterpret letter keys are outside this
backend; it tracks input events rather than reading the terminal's real buffer.
Other terminal applications remain disabled unless explicitly listed.

For an opt-in live regression run on macOS, select this workspace in Superset,
leave its window visible, then run:

```sh
python3 tools/superset_input_probe.py --workspace "$SUPERSET_WORKSPACE_ID"
```

This requires the installed Upyr app running with automatic, terminal and slash
correction enabled for Superset, the default undo shortcut, `swiftc`, and an
already-permitted `cua-driver`. It creates its own raw-input terminal tab,
checks an on-screen fixture code before typing physical keys, and closes only
that tab afterward. The sink never executes typed commands. The cases cover
mixed language, contractions, punctuation, commands, fast typing, undo and focus
changes. Keep the window unobstructed and avoid typing or clicking during the
run. Test text, results and an exact app-window screenshot stay in the printed
temporary directory; the probe leaves Upyr preferences and clipboard intact.

For an isolated, repeatable edge-case run (no GUI input), use:

```sh
python3 tools/run_terminal_edge_tests.py
```

The offline runner uses cached Cargo dependencies and writes a JSON report and
full log under `target/`. It checks the curated terminal corpus against portable
and frozen macOS keyboard maps, generated slash-command editing sequences,
control/format characters, punctuation, replacement and undo. On macOS, a
separate main-thread check reads the real installed layout map without switching
input sources. The live Superset probe above is optional and opens its own tab.
CI preserves each platform's JSON report and UTF-8 test log for 14 days, including
failed runs and partial output when the Cargo timeout is reached.

`slash_command_apps` additionally opts an interface into English `/commands`.
At the start of a freshly tracked input line, `.сщььфтв` becomes `/command`,
including the leading slash. Correction starts as soon as the prefix has a
letter (`.с` → `/c`), so command menus can open before Space. Custom names,
digits, hyphens and underscores work without dictionary entries. Upyr never
submits a command. Navigation, clicks and shortcuts discard the old tracked
span. A newly typed slash prefix can then start again without an extra Return;
only that fresh prefix is replaced. Known inline prose and pasted text are not
reinterpreted. Existing terminal history is not read.

The macOS Feedback settings include an embedded flag preview and a layout
selector. **All Country Flags…** opens a searchable picker of all 259 bundled
country and region flags, with names and country codes. Select a keyboard layout
and click a flag to preview it, then choose **Save Flags** to apply the choices.
**Use Automatic Flag** restores a layout’s default; **Revert** discards unsaved
flag changes. Saving flags preserves other settings. The indicator preview uses the live
indicator’s flag images, overrides, skipped layouts,
scale and glow. Style changes update the preview before saving; Preview Flag
shows the selected layout on screen without changing the keyboard layout.

The terminal policy scores the last word independently, recognizes English
contractions, and permits common two- and three-letter words in both languages
under the default minimum length. A custom minimum above four remains enforced.
English punctuation keys that produce Ukrainian letters are accepted in the
tracked source (`lj,ht` → `добре`, `[jnskjcz` → `хотілося`).
Terminal guards use application/window identity and native input counters without
blocking on Accessibility queries for every key. A target-process event tap delivers
the generated keys together in the target application's stream, so the next
letter cannot enter between Backspace and replacement. The ordinary correction
delay remains configurable and unchanged. The tap is prepared when typing starts,
passes input unchanged while idle and sleeps without polling.
The target validates the edit before the main thread commits the layout change;
a rejected edit leaves the layout unchanged. Successful automatic replacements
use the indicator and correction sound even if the target layout is already active.
Nonactivating overlays from other processes do not invalidate the terminal's
keyboard focus; the active application is checked again before committing.

`tools/globe-probe.swift convert terminal` checks replacement without any Paste
or pasteboard revision; `convert terminal-cancel` checks continued typing. For
these probes, list `"upyr-nonblocking-probe"` in `terminal_typing_apps`.
`convert terminal-english` starts on the Ukrainian layout and types
`it's not so good for english so far ` to check the English correction path.
`convert terminal-mixed` starts on ABC and checks `it's not так добре як хотілося б `;
`terminal-punctuation` and `terminal-bracket` independently check corrections
of words typed using the comma and bracket keys.
`terminal-interleave` injects another letter near the edit deadline and accepts
only an intact original word or a complete replacement with that letter preserved.

| Platform | Status | Notes |
| --- | --- | --- |
| macOS 11+ | Primary public-preview target | Native AppKit settings and feedback, universal Apple Silicon/Intel package, native input-source and rich pasteboard integration |
| Windows | Preview | Tray app, installer and portable ZIP, foreground-layout mapping, broad clipboard snapshot/restore support, and AccessKit-backed settings accessibility |
| Linux/X11 | Preview | DEB and tar package, XKB layout mapping, StatusNotifierItem tray, desktop-notification feedback, and AccessKit-backed settings accessibility; native Wayland global input awaits a portal-backed design |
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
