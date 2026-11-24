# FOSSA findings review — 2026-11-13

The first connected project reports **21 open issues: 13 flagged licenses,
1 denied license and 7 outdated dependencies**. These categories were read in
the authenticated FOSSA dashboard for `aaac395`; the corrected
[CI run for `25d3d4e`](https://github.com/dmytro-yemelianov/upyr/actions/runs/43433033499)
uploaded that new revision successfully and returned the same total. No issue
was ignored and no account policy was changed during this review.

FOSSA's dashboard also says **scan depth is limited**. Its displayed security
status is therefore not evidence of an unrestricted vulnerability assessment.
The project report requires authentication; the public license badge is failing.

The [Wayland follow-up](fossa-wayland-follow-up-2026-11-13.md) records the exact
Linux dependency constraints and the Plasma blur protocol reference. Both
findings remain open; the follow-up does not change the initial audit result.

## Evidence and scope

[The evidence manifest](fossa-review-2026-11-13.json) records the lockfile hash,
exact crate versions and checksums, upstream commits, source-file hashes, and
target-specific dependency observations. Source files were inspected in the
Cargo registry packages selected by the committed lockfile. This is an
engineering applicability review, not a blanket redistribution approval.

Dependency presence was checked with `cargo tree --locked --workspace --target
<target> --edges normal,build`, using default features, for both macOS
architectures, Windows MSVC and Linux GNU. These graphs describe selected build
dependencies, not proof that every dependency's bytes survive final linking.
They do not cover arbitrary feature combinations or redistributed source trees.

## The 14 license findings

| Package and exact version | Findings | Evidence and disposition |
| --- | ---: | --- |
| `epaint_default_fonts` 0.32.3 | 2: Bitstream Vera, Ubuntu Font 1.0 | Real embedded font licenses. `fonts/Hack-Regular.txt` preserves Hack/MIT and Bitstream terms; `fonts/UFL.txt` covers Ubuntu Light. Add readable distribution notices, retain the original fonts and names, and review usage under those terms. Do not relabel the font bundle MIT. |
| `orbclient` 0.3.55 | 2: GPL-2.0, GPL font exception | Real notices in `res/unifont-license.txt`; the crate's MIT metadata does not erase them. Absent from all four checked desktop build graphs. A desktop-only applicability decision must not apply to Redox builds or source redistribution. |
| `linux-raw-sys` 0.4.15 and 0.12.1 | 2: Apache with LLVM exception | Each manifest declares `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. MIT is an explicit alternative; review the selected license for these exact versions instead of treating every alternative as cumulative. |
| `r-efi` 5.3.0 and 6.0.0 | 2: LGPL-2.1-or-later | Each manifest declares `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. An MIT alternative exists; neither version appears in the checked desktop graphs. |
| `rustix` 0.38.44 and 1.1.6 | 2: Apache with LLVM exception | Each manifest offers the same explicit MIT alternative as `linux-raw-sys`; both appear in the Linux graph. |
| `wasi` 0.11.1+wasi-snapshot-preview1 | 1: Apache with LLVM exception | Manifest offers MIT as an alternative; absent from the checked desktop graphs. |
| `wit-bindgen` 0.46.0 | 1: Apache with LLVM exception | Manifest offers MIT as an alternative; absent from the checked desktop graphs. |
| `winit` 0.30.13 | 1: denied CC-BY-SA-4.0 | `docs/res/ATTRIBUTION.md` assigns CC-BY-SA to `keyboard_*.svg` documentation images. The crate declares Apache-2.0. Upyr's package scripts do not copy those upstream documentation images; this is a documentation-asset scope finding, not evidence that all winit code is CC-BY-SA. |
| `wayland-protocols-plasma` 0.3.12 | 1: LGPL-2.1 | Its manifest declares MIT, but bundled Plasma protocol XML files contain LGPL-2.1-or-later notices. It **is selected on Linux** through winit. Keep this open until the generated-binding and distribution scope is reviewed; do not dismiss it using only Cargo metadata. |

Primary sources include the pinned
[winit image attribution](https://github.com/rust-windowing/winit/blob/e9809ef54b18499bb4f2cac945719ecc2a61061b/docs/res/ATTRIBUTION.md),
[r-efi licensing declaration](https://github.com/r-efi/r-efi/blob/7e1b0322d31d625f81a5656096330934f9cd835d/README.md#license),
[rustix manifest](https://github.com/bytecodealliance/rustix/blob/4a563ad698b2c20ff848515821023d2265a788d0/Cargo.toml),
and [egui's embedded fonts](https://github.com/emilk/egui/blob/af96e0373c18477b77236e2bfc89735af007b1c2/crates/epaint_default_fonts/src/lib.rs).
The JSON manifest pins every inspected package, including the older versions.

## The seven outdated dependencies

| Package | Locked version | Selected desktop build graph |
| --- | --- | --- |
| `naga` | 25.0.1 | None of the four checked targets |
| `wgpu` | 25.0.2 | None of the four checked targets |
| `wgpu-core` | 25.0.2 | None of the four checked targets |
| `wgpu-core-deps-windows-linux-android` | 25.0.0 | None of the four checked targets |
| `wgpu-hal` | 25.0.2 | None of the four checked targets |
| `wgpu-types` | 25.0.0 | None of the four checked targets |
| `wayland-protocols-experimental` | 20250721.0.1 | Linux GNU |

Upyr enables eframe's `glow` backend; the six wgpu/naga packages remain in the
lockfile but are not selected by these builds. FOSSA's outdated-version category
does not itself establish a known vulnerability. Keep the inventory visible;
reassess these packages if the renderer or features change.

The Linux Wayland dependencies are selected even though Upyr explicitly requests
X11: eframe's accessibility integration enables `accesskit_winit` default
features, which enable winit's Wayland feature. Removing accessibility support
merely to remove a finding is not an acceptable fix. Investigate compatible
upstream updates and binding/license scope before changing this dependency path.

## Changes and remaining decisions

- CI now runs `fossa analyze` and then `fossa test` as two separate pinned action
  invocations. Both were observed against the new main revision; the policy gate
  preserves its failed result.
- [Third-party notices](../../THIRD_PARTY_NOTICES.md#egui-default-fonts) now include
  the four upstream font-license texts and copyright notices. Existing desktop
  packaging scripts already copy this file, so future packages include them.
  Published preview archives were not replaced by this documentation change.
- The eight alternate-license findings have evidence for an MIT selection;
  record any FOSSA conclusion for the exact package version and Upyr scope.
- The two orbclient findings and the winit documentation finding have bounded
  applicability evidence. Any exception must state the checked targets/assets;
  it must not silently cover other targets or source redistribution.
- Font usage, the Linux Plasma protocols and the active outdated Wayland package
  still need their specific follow-through. No global allow rule, ignore-all
  operation, paid-plan change or passing compliance claim was applied.

With the initial result and categories verified, FOSSA runs on main updates,
weekly and by manual dispatch. Open decisions remain failed findings. A later
passing result must link to the actual revision and any scoped policy decisions.
