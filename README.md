# Upyr

Upyr is a privacy-first, native English ↔ Ukrainian keyboard-layout fixer written in Rust. It is an early cross-platform alternative to Punto Switcher for macOS, Windows, and Linux/X11.

Назва **Upyr** походить від українського «упир» — образу перевертня, того, хто перекидається. Так само Upyr «перекидає» текст, набраний у неправильній розкладці, у потрібну форму.

Upyr supports two local workflows. The explicit workflow is always available:

1. Select text typed in the wrong layout.
2. Press `CmdOrCtrl+Alt+Space`.
3. Upyr copies the selection, detects its direction, converts the physical-key positions, and pastes it back.

Automatic correction is opt-in. When enabled, Upyr can recognize a confidently mistyped word as soon as you press Space—for example, `ghbdsn` becomes `привіт` while an already valid `hello` remains unchanged. It also recognizes likely proper names (`Jkmuf` → `Ольга`, `Щдрф` → `Olha`) and scores an ambiguous final physical punctuation key both as a layout letter and as punctuation, so `Jkmuf,` becomes `Ольга,` without breaking `[ks,` → `хліб`. A native global key-down listener keeps only a short input prefix in memory; typed input is never logged or sent anywhere.

Upyr also adds a **U** icon to the macOS menu bar or system tray. Its menu can convert text, pause/resume Upyr, open the native settings window, reload the configuration, or quit the app.

For example, `ghbdsn` becomes `привіт`, and `руддщ` becomes `hello`. Processing stays on the device. The desktop and WASM runtimes have no network or telemetry code; corpus-generation tools download pinned public datasets only when invoked explicitly.

For a faster no-selection workflow, place the caret immediately after a mistyped word and press `CmdOrCtrl+Alt+Backspace`. Upyr selects the previous word and fixes it in place.

## Build and run

Install the current stable Rust toolchain, then:

```sh
cargo build --release
./target/release/upyr
```

On Ubuntu/Debian, install the desktop build dependencies first:

```sh
sudo apt-get update
sudo apt-get install -y libx11-dev libxtst-dev libxkbcommon-dev \
  libwayland-dev libgtk-3-dev libayatana-appindicator3-dev
```

On Windows, run `target\release\upyr-background.exe` for the tray application without a console window, or use `target\release\upyr.exe` for CLI commands and foreground diagnostics.

You can test the conversion engine without desktop permissions:

```sh
cargo run -- convert ghbdsn
printf 'руддщ' | cargo run -- convert --direction smart
./target/release/upyr convert --installed ghbdsn
./target/release/upyr doctor
./target/release/upyr settings
```

The shared engine also has a headless WebAssembly binding (no DOM adapter or npm
release yet):

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.126 --locked
cargo check -p upyr-core -p upyr-wasm --all-targets \
  --target wasm32-unknown-unknown --locked
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test -p upyr-wasm --target wasm32-unknown-unknown --locked
cargo build --release -p upyr-wasm --target wasm32-unknown-unknown --locked
wasm-bindgen --target nodejs --out-dir target/upyr-wasm-node \
  target/wasm32-unknown-unknown/release/upyr_wasm.wasm
node tools/smoke_wasm_node.cjs target/upyr-wasm-node/upyr_wasm.js
# Requires the `brotli` command (`brew install brotli` or `apt install brotli`).
python3 tools/check_wasm_size.py
```

See [`crates/upyr-wasm/README.md`](crates/upyr-wasm/README.md) for the current
`convertText`/`UpyrSession` contract and browser-host safety requirements.

Tagged releases produce installable artifacts:

- macOS: a universal Apple Silicon/Intel `Upyr.app` in DMG and ZIP form
- Windows: a per-user Inno Setup installer and a portable ZIP
- Linux/X11: a DEB package and a portable tarball

Build a local universal macOS package with:

```sh
packaging/macos/generate-icon.sh
packaging/macos/build-universal.sh
packaging/macos/package.sh
```

The resulting `dist/Upyr.app`, ZIP, and DMG are ad-hoc signed for local testing. Move `Upyr.app` to `/Applications` before enabling launch at login. Official tag builds fail closed unless a Developer ID Application identity and complete Apple notarization credentials are configured; they sign and notarize the app before the final ZIP/DMG are created, then sign, notarize, staple, and assess the DMG. Windows release CI signs both executables and the installer when its certificate secrets are present; otherwise the Windows artifacts are explicitly unsigned. See [`.github/workflows/release.yml`](.github/workflows/release.yml) for the required secret names.

## Configuration

Create the default config and print its location:

```sh
upyr init
upyr config-path
upyr autostart status
upyr autostart enable
upyr settings
```

Default configuration:

```toml
config_version = 4
hotkey = "CmdOrCtrl+Alt+Space"
last_word_hotkey = "CmdOrCtrl+Alt+Backspace"
direction = "smart"
copy_delay_ms = 90
paste_delay_ms = 40
switch_layout = true
show_layout_indicator = false
layout_indicator_duration_ms = 900
play_switch_sound = false
auto_correct = false
auto_correct_sensitivity = "conservative"
auto_correct_min_word_length = 4
auto_correct_delay_ms = 35
auto_correct_exceptions = []
modifier_gesture = "disabled"
modifier_gesture_action = "previous-word"
modifier_gesture_timeout_ms = 500
restore_clipboard = true
restore_delay_ms = 250
```

Set `UPYR_CONFIG` to use a different config path. Valid directions are `smart`, `english-to-ukrainian`, and `ukrainian-to-english`. Hotkey modifier names include `CmdOrCtrl`, `Cmd`, `Ctrl`, `Alt`, and `Shift`.

The optional modifier-only trigger can be `double-control`, `double-shift`, or `double-control-shift`; its action can be `previous-word` or `selection`. It is deliberately `disabled` by default, which means Upyr does not poll global keyboard state. When enabled, Upyr immediately reduces each sample to modifier flags plus an “other key pressed” bit; it does not retain or log key identities. Any ordinary key or unrelated modifier cancels the gesture. Enabling it requires Accessibility permission on macOS and an active X11 display on Linux.

Automatic correction is also deliberately disabled by default. Its sensitivity can be `conservative`, `balanced`, or `aggressive`. Upyr combines exact dictionary matches with a compact, frequency-weighted character 2–5-gram index generated from comparable 1,000,000-sentence English and Ukrainian news corpora. Training tokens are filtered by alphabet, split into character n-grams, and immediately tagged with an EN or UK confidence; the application embeds no corpus word-frequency table or sentences, only packed n-gram evidence (which can naturally include complete short words). Pairwise policy keeps the source when both interpretations are plausible and applies physical-punctuation assistance only in the direction where those positions become Ukrainian letters. Upyr keeps only a short, in-memory prefix from the current input boundary and converts that prefix when the target language becomes substantially more likely; navigation, layout changes, known or confidently recognized source-language segments, technical punctuation, and the 256-character limit reset the context. Deliberate Latin identifiers such as `FAANG`, `SaaS`, `NASDAQ`, `iPhone`, and `ServiceNow`, plus recognizable URL/path tokens, also close the current source-language segment so a later foreign-layout word cannot sweep them into a correction. Automatic mode uses OS key-down hooks instead of periodic keyboard snapshots, so brief presses during fast typing are not lost. Add project names, abbreviations, or other intentional strings to `auto_correct_exceptions`. The settings window validates and writes this configuration, and a running Upyr process reloads it automatically.

The committed language index can be reproduced with `python3 tools/generate_ngram_model.py`. The generator downloads pinned Leipzig Corpora Collection archives, verifies their SHA-256 checksums, filters mixed-script and malformed tokens, and writes the packed `crates/upyr-core/assets/models/language.ngm` artifact. The generated index currently contains 173,964 language-tagged n-grams in roughly 2.8 MiB; full training archives stay outside the repository.

Model and policy changes are measured against the frozen [signed N-gram v1 evaluation](docs/benchmarks/signed-ngram-v1.md). Its 191 materialized cases cover both directions, native text, technical identifiers, names, punctuation, short-word abstention, contextual phrases, and reported physical-key mappings without regenerating expected snapshots through production conversion code. An optional independently generated Wikipedia holdout adds 20,000 clean boundaries for false-correction screening.

Settings are organized into General, Automatic, Shortcuts, Feedback, and Advanced tabs. Parameter search jumps directly to the matching tab. Shortcut fields are press-to-record controls: they capture physical keys, require a modifier, render readable platform symbols, reject duplicate assignments, and offer an individual reset. macOS uses AppKit controls throughout the settings companion; Windows and Linux keep the same tab/search model in the cross-platform frontend.

Optional switch feedback is disabled by default. `show_layout_indicator` briefly displays the target language flag next to the pointer for `layout_indicator_duration_ms`, and `play_switch_sound` adds a local system sound. Feedback runs only after Upyr confirms a real OS input-source change. The overlay uses AppKit on macOS, a non-activating Win32 window on Windows, and a GTK popup on Linux/X11. Linux sound playback uses `canberra-gtk-play` when available.

When testing locally on macOS, the packaging script embeds stable designated requirements in its ad-hoc signatures. Accessibility approval therefore survives normal Upyr rebuilds even without a Developer ID certificate. The background app also ignores global shortcuts while Settings is open, allowing an existing shortcut to be recorded without running its action.

## Platform notes

### macOS

Grant Accessibility access to the terminal running Upyr, or to the packaged **Upyr** app, in **System Settings → Privacy & Security → Accessibility**. macOS needs this permission to observe opt-in word boundaries and to send Copy and Paste. Upyr checks the current trust state before initializing input monitors: an existing grant is accepted silently, while a missing permission is requested at most once per process. When access changes from denied to granted, Upyr detects the transition and offers to restart itself once. You can also choose **Save settings** so the background monitor retries initialization without a restart.

After a successful conversion, Upyr selects the matching installed English or Ukrainian input source. Set `switch_layout = false` to leave the active input source unchanged. Upyr derives the character mapping—including Shift and Option layers—from the installed `ABC`/`U.S.` and Ukrainian input sources, then falls back to its built-in map if native translation is unavailable. The standard macOS `Ukrainian` source is preferred over `Ukrainian-PC`, preserving its physical punctuation row (`[];'\\,./` → `хїжєʼбю.`); automatic mode treats those positions as language evidence instead of rejecting them as technical punctuation. Temporary conversion text is tagged with the standard concealed pasteboard hint, and Copy detection uses the native pasteboard change counter instead of placing a sentinel string on the clipboard. When restoration is enabled, Upyr snapshots and restores every readable macOS pasteboard item and format rather than reducing rich clipboard content to plain text.

### Windows

No elevated privileges are expected. Some elevated applications reject simulated input from a non-elevated Upyr process.

Upyr reads the keyboard-layout handle of the foreground window, generates the positional character map from the installed English/Ukrainian layouts, posts the target layout after conversion, and marks its temporary text to stay out of Windows Clipboard History and Cloud Clipboard. It falls back to the built-in map if Windows cannot expose a usable pair. Clipboard restoration preserves complete HGLOBAL-backed format sets—including Unicode text, HTML/RTF, file lists, DIB images, and registered formats—with contention retries and a 64 MiB safety limit. If the clipboard includes handle-only GDI formats, Upyr uses the safe text/image/file fallback rather than silently restoring a partial set. The target layouts must already be installed in Windows Settings.

### Linux

The global-hotkey backend currently supports X11. Wayland intentionally restricts global input; run under an X11 session/XWayland, or use the CLI converter until a desktop-portal hotkey backend is added. Clipboard timing can be increased for slower desktop environments.

Under X11, Upyr reads and locks the active XKB group after discovering configured groups with `setxkbmap -query`. It derives the positional map from those XKB groups and falls back to the built-in map if X11 cannot expose a usable pair. Install `setxkbmap` (commonly provided by `x11-xkb-utils`) and configure both `us` and `ua` groups. Linux still uses the sentinel fallback for guarded Copy detection, but both the sentinel and converted text carry the desktop clipboard-history exclusion MIME hint. Restoration prioritizes file lists and HTML with its plain-text alternative before falling back to text or images.

## What is included

- Reversible US-QWERTY ↔ Ukrainian positional mapping, including Ukrainian `і`, `ї`, `є`, and `ґ`
- Smart direction detection and explicit direction overrides
- Cross-platform global hotkey and layout-independent physical Copy/Paste shortcuts
- A second shortcut that fixes the previous word without manually selecting it
- Opt-in automatic correction after Space with conservative, balanced, and aggressive confidence levels
- Searchable, tabbed settings with native AppKit controls on macOS and press-to-record physical hotkey selectors with conflict detection
- Optional language-flag overlays next to the pointer and subtle switch sounds after confirmed layout changes
- Clipboard restoration for native rich formats, HTML, file lists, text, and images, plus guarded Copy detection that prevents accidental conversion when nothing is selected
- Full readable pasteboard-format restoration on macOS
- Native macOS, Windows, and Linux/X11 input-source detection and switching after conversion
- OS-derived physical-key mappings on macOS, Windows, and Linux/X11 with a deterministic built-in fallback
- Cross-platform single-instance enforcement so duplicate listeners cannot compete for hotkeys
- User-level launch-at-login controls through the tray or `upyr autostart`
- An opt-in double-Control, double-Shift, or double-Control+Shift trigger with no polling while disabled
- Versioned configuration with in-memory migration through schema version 4
- Configurable timing for applications with slow clipboard handling
- Native menu-bar/system-tray controls for conversion, pause, configuration, and quit
- A shared `upyr-core` decision engine plus a tested, DOM-independent WASM binding with generated TypeScript contracts
- Unit, deterministic property-style, and CLI integration tests plus CI builds for macOS, Windows, and Linux
- Universal macOS DMG/ZIP, Windows installer/ZIP, and Linux DEB/tar release packaging

## Roadmap

- [TypeScript DOM facade, playground, and npm integration](docs/architecture/wasm-web-plan.md) on the implemented headless WASM binding, developed in parallel with the calibrated N-gram v2 scorer
- Arbitrary Linux MIME-set restoration and Windows GDI-handle duplication beyond the current safe fallbacks
- Production code-signing/notarization credentials for official releases
- Per-application exclusions and editable custom dictionaries
- Additional keyboard layouts through data files

## Inspiration

Upyr is inspired by Punto Switcher and [TolikPylypchuk/KeyboardSwitch](https://github.com/TolikPylypchuk/KeyboardSwitch). KeyboardSwitch demonstrated several particularly useful product ideas: explicit selected-text correction, following a correction with the matching OS input source, generated physical-key mappings, configurable modifier gestures, and careful startup/config migration behavior.

Upyr is an independent Rust implementation, not a port. It keeps one small background process; the Rust settings companion exists only while its window is open. It avoids an always-running settings service and arbitrary cycling through every installed layout. The initial product stays focused on private English ↔ Ukrainian correction, with ordinary-key monitoring and automatic changes remaining opt-in.

The native macOS input-source binding was adapted from the MIT-licensed [issw](https://github.com/0xAndoroid/issw); its notice is retained in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

## License

MIT
