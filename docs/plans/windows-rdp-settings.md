# Windows RDP settings fix plan

Spec: [windows-rdp-settings.md](../specs/windows-rdp-settings.md).

- [x] Inspect capture, layout switching, settings startup and build configuration.
- [x] Use DX12/WARP-compatible rendering on Windows for both egui frontends.
- [x] Expose startup errors and document RDP input/default correction state.
- [x] Run cross-platform checks and installed Windows window smoke tests.
- [x] Deliver a new test installer with source provenance.
- [ ] Confirm settings and actual typing on HetzerWin with remote evidence.

## Verification

Source commit `998a852e13cdca64ea7e737eace6dd1a9efc68d1` passed all PR workflows.
Workspace tests, macOS/Windows Clippy, formatting, privacy, dependency licenses,
RustSec/OSV and Rust 1.87 checks passed. Windows package run
[43594964466](https://github.com/dmytro-yemelianov/upyr/actions/runs/43594964466)
verified installation/config preservation and both installed settings windows.
Each window initialized DX12, stayed responsive for 10 seconds and exited cleanly.
The selected adapter was CPU `Microsoft Basic Render Driver` (WARP), proving
startup without a hardware GPU or OpenGL driver on that CI desktop.

[Windows test installer](https://github.com/dmytro-yemelianov/upyr/actions/runs/43594964466/artifacts/14114440270)
requires GitHub sign-in and expires 2026-11-28. The package includes build
provenance, installer/portable checksums and instructions. It is a test build,
not a published release. Actual HetzerWin input validation remains pending;
the user has confirmed the old build's OpenGL initialization error but has not
yet tested correction with Scancode and automatic correction enabled.
