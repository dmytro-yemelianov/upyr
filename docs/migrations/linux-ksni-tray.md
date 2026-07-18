# Linux migration: drop gtk3 (ksni tray + non-gtk feedback)

Status: **planned** — implementation pending one UX decision (see §4) and
Linux-desktop runtime testing.

## Why

OSV-Scanner flags 10 advisories that all trace to the **gtk-rs 0.18 (gtk3)**
stack, pulled in on Linux by `tray-icon`'s `gtk` feature (→ `libappindicator`
→ `gtk`/`gdk`/`atk`/`glib`/`pango`/`cairo`). gtk3-rs is unmaintained
(superseded by gtk4) and the one advisory with a fix — `glib` RUSTSEC-2024-0429,
fixed in `glib 0.20` — is gtk4-generation and cannot be paired with `gtk 0.18`.
`tray-icon` and `muda` at their latest versions still require gtk3 on Linux, so
**no version bump clears these**. The only path is to leave gtk3.

These advisories are currently accepted in `osv-scanner.toml` (matching the
existing cargo-audit policy); this migration removes the debt at the source, at
which point those ignore entries should be deleted.

## Current gtk3 touchpoints (all `cfg(target_os = "linux")`)

| File | Usage |
|---|---|
| `Cargo.toml` | `tray-icon` feature `gtk`; direct `gtk = "0.18.2"` |
| `src/tray.rs` | `TrayIcon` + `tray_icon::menu` (9 items incl. 2 `CheckMenuItem`), animated icon, tooltip |
| `src/app.rs:120` | `MenuEvent::set_event_handler` → `UserEvent::Menu` into the winit loop |
| `src/app.rs:90` | `gtk::init()` |
| `src/app.rs:785` | `about_to_wait` pumps `gtk::events_pending`/`main_iteration_do` every 50 ms |
| `src/feedback.rs` (linux `platform`) | `gtk::Window` popup near the cursor showing the RU/UA layout indicator |

## Target design

### 1. Dependencies (`Cargo.toml`)
- Move `tray-icon` to `cfg(not(target_os = "linux"))` (keep for macOS/Windows).
- Linux deps: add `ksni` (pure-Rust D-Bus StatusNotifierItem, no gtk); remove
  `gtk`. Add the chosen feedback backend (§4).
- Net effect: the entire gtk3/glib/cairo/pango stack leaves the Linux graph.

### 2. Tray (`src/tray.rs`)
Split into `#[cfg(not(linux))]` (unchanged `tray-icon` impl) and
`#[cfg(linux)]` (ksni). Keep the public surface identical so `app.rs` barely
changes:
- `Tray::new(&Config) -> Result<Self>`
- `Tray::action(&MenuEvent) -> Option<TrayAction>`
- `Tray::update(&Config, paused)`
- `Tray::set_flip_frame(u8)`

ksni model mapping:
- Implement `ksni::Tray` with `icon_pixmap()` (from the existing `icon_rgba`),
  `title()`/`tool_tip()` (from `tooltip()`), and `menu()` returning
  `StandardItem`/`CheckmarkItem`/`SubMenu` built from the same labels.
- ksni menu items carry `activate` closures instead of a global event id.
  Bridge them to the app by sending a `TrayAction` over a channel to a
  `winit::event_loop::EventLoopProxy` (replaces `MenuEvent::set_event_handler`).
  This lets `handle_menu` collapse into "receive a `TrayAction`" on Linux.
- `set_flip_frame`/`update` call `handle.update(|tray| { … })` on the ksni
  service handle (spawned in `Tray::new`).

### 3. Event loop (`src/app.rs`)
- Delete `gtk::init()` and the `about_to_wait` gtk pump on Linux. ksni runs its
  own D-Bus service thread; nothing needs pumping from winit.
- `UserEvent` gains a Linux `Tray(TrayAction)` variant fed by the ksni proxy.

### 4. Feedback indicator (`src/feedback.rs`) — **decision needed**
The current Linux popup is a small **borderless window pinned near the cursor**
showing "РУ"/"УA". Two ways to keep it gtk-free:

- **A. `notify-rust` desktop notification** (recommended for simplicity).
  D-Bus `org.freedesktop.Notifications`, zero gtk, ~10 lines. **UX change:** it
  renders as a normal notification (top/corner, themed by the DE) rather than a
  floating near-cursor chip.
- **B. Raw X11 override-redirect window** via `x11-dl` (already a dep).
  Preserves the exact near-cursor UX but is ~100–150 lines of Xlib and is
  X11-only (no Wayland). Higher risk, more testing.

## Risks
- StatusNotifierItem host support varies (GNOME needs the AppIndicator
  extension; KDE/most others work natively) — same practical constraint as
  today's libappindicator, so not a regression.
- Menu semantics (checkmark state, dynamic labels) must be re-verified per DE.
- Icon pixmap format (ksni uses ARGB32; current `icon_rgba` is RGBA) needs a
  channel-order conversion.

## Runtime testing checklist (Linux desktop; cannot be done in CI/sandbox)
- [ ] Tray icon appears on GNOME (with AppIndicator ext), KDE, XFCE.
- [ ] All 9 menu items fire the correct `TrayAction`.
- [ ] Pause / Autostart checkmarks reflect and toggle state.
- [ ] Status text + tooltip update on config reload and pause.
- [ ] Icon flip animation plays on a correction.
- [ ] Layout indicator shows on layout change (chosen backend) and does not
      steal focus.
- [ ] No gtk in `cargo tree` for the Linux build; OSV/`cargo audit` clean after
      deleting the gtk entries from `osv-scanner.toml`.
- [ ] Clean shutdown (ksni service thread joins; no leaked D-Bus name).

## Rollout
1. Decide §4 (A or B).
2. Implement behind `cfg(linux)`; `cargo check` + `cargo clippy` on Linux.
3. Runtime-test the checklist.
4. Remove the gtk-group entries from `osv-scanner.toml`; keep the `ttf-parser`
   (egui) entry until that is addressed separately.
