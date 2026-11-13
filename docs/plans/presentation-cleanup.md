# Website and README cleanup plan

Spec: [presentation-cleanup.md](../specs/presentation-cleanup.md).

- [x] Inspect the live landing page and current README.
- [x] Shorten README; preserve detailed reference and section links.
- [x] Simplify the landing page and bilingual demo presentation.
- [x] Verify desktop/mobile rendering, links, demo and site checks.

Initial findings: README has 543 lines; the 1280×720 hero pushes its primary
button below the viewport; the demo exposes a full keyboard, technical controls
and a password test immediately, with untranslated English instructions in UK.

Release history also exposes per-version changes through a disclosure, keeping
download links visible. Fixed stale release-navigation anchors and long inline
code wrapping on narrow screens.

Verification completed on 2026-11-13:

- README: 543 → 120 lines; detailed commands and explanations remain in
  `docs/reference.md`. Locally rendered Markdown inspected at desktop and 390 px.
- Homepage: English and Ukrainian inspected at desktop and 320/390 px; primary
  download and browser-demo actions fit in the first viewport. Optional demo
  controls and package-manager commands expand on request.
- Features, How, Privacy and Releases: narrow-screen rendering checked; no
  horizontal overflow at 320 px, including expanded release history.
- The real WASM example produced `привіт hello хліб codex`; language switching,
  layout controls, password exclusion and mobile navigation checked.
- Site-content, version and privacy checks pass; 21 Python tests pass; JavaScript
  syntax and whitespace checks pass; 136 local links resolve.
- Changes are local. No deployment, push, engine change or native-app install.
