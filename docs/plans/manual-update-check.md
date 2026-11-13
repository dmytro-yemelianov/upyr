# Manual update check plan

Spec: [manual-update-check.md](../specs/manual-update-check.md).

- [x] Implement bounded HTTPS lookup, semantic release selection and worker state.
- [x] Add localized About controls in shared/pixel and native settings, plus search.
- [x] Update privacy guard, documentation and regression coverage.
- [x] Run focused/workspace checks, Windows cross-check and explicit live lookup.

Verification evidence will be recorded here. Existing published 0.6.0 artifacts
are unchanged; this feature is initially an unreleased source change.

## Verification

- Workspace/all-target offline runner: **455 passed, 0 failed, 7 ignored**.
  The new checker has eight offline tests; the ninth, a live GitHub lookup,
  is ignored by default and passed when explicitly invoked.
- Both language variants render the About controls at 480 px. Tests verify idle
  rendering/polling starts no worker, an in-flight check disables the button,
  duplicate requests are ignored, worker failure permits retry, and a successful
  newer-version result reveals the release link.
- Clippy with warnings denied passed for macOS and Windows GNU, workspace,
  all targets/features. `cargo +1.87.0 check --workspace --all-targets --locked`
  passed. All three macOS release binaries built successfully.
- Privacy guard, 23 Python privacy/site tests, site content, version consistency,
  formatting, diff whitespace, and dependency license/source policy passed.
  The new trust-anchor data's complete CDLA-Permissive-2.0 notice is bundled;
  its license exception is restricted to webpki-roots 1.0.12.
- Visually inspected the Ukrainian native About page using an isolated config:
  the new button and explanation fit without overlap. The test windows closed
  before the driver could complete a button action, so an end-to-end native GUI
  click is not claimed. The HTTPS worker itself passed an explicit live lookup.
- Local evidence: `.stos/manual-update-check/` and
  `target/terminal-edge-report.json` (not committed). Windows runtime UI and
  installer checks still require Windows; the local Windows result is a cross-check.

No release has been published and no installed application has been replaced.
