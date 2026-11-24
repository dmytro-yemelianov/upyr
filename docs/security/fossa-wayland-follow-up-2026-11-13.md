# FOSSA: remaining Wayland findings

Follow-up to the [initial 21-finding review](fossa-review-2026-11-13.md).
The checks below use the unchanged lockfile from `94b4d28`. They do not resolve
either active Linux finding or change FOSSA policy.

## Outdated experimental protocols

The selected Linux dependency path is:

```text
upyr → eframe 0.32.3 → egui-winit 0.32.3
     → smithay-clipboard 0.7.3 → smithay-client-toolkit 0.20.0
     → wayland-protocols-experimental 20250721.0.1
```

The published [smithay-clipboard manifest](https://github.com/Smithay/smithay-clipboard/blob/26c2f53f15f6bdc4f41a442d0ae2c2d63bbc617c/Cargo.toml)
requires toolkit `0.20.0`; its caret range excludes `0.21`.
The [toolkit manifest](https://github.com/Smithay/client-toolkit/blob/8a0b33616145b38c7f989f8e468d57e69a2a6748/Cargo.toml)
requires experimental protocols `20250721.0.1`; its caret range excludes the
new calendar-major release `20251230`.

On 2026-11-13, the crates.io registry reports these latest stable releases:

| Package | Latest stable | Effect on the current path |
| --- | --- | --- |
| [smithay-clipboard](https://crates.io/crates/smithay-clipboard) | 0.7.3 | Already selected; still requires toolkit 0.20 |
| [smithay-client-toolkit](https://crates.io/crates/smithay-client-toolkit) | 0.21.1 | Outside the clipboard dependency's compatible range |
| [wayland-protocols-experimental](https://crates.io/crates/wayland-protocols-experimental) | 20251230.0.3 | Outside toolkit 0.20's compatible range |

A lockfile-only update cannot select those newer versions on this path. Keep
the outdated finding active. Recheck when a compatible clipboard release is
available, or when a separately tested dependency migration is undertaken.
This quality finding alone does not establish a vulnerability. Do not remove
clipboard or accessibility functionality merely to suppress it.

## Plasma protocol license

`wayland-protocols-plasma 0.3.12` remains the latest stable release as checked on
2026-11-13. Its MIT crate metadata is insufficient to dismiss the separate
license on the bundled protocol definitions.

In the pinned winit source,
[`kwin_blur.rs`](https://github.com/rust-windowing/winit/blob/e9809ef54b18499bb4f2cac945719ecc2a61061b/src/platform_impl/linux/wayland/types/kwin_blur.rs)
imports the Plasma blur client objects. The packaged `blur.xml` declares
`LGPL-2.1-or-later` and names Martin Gräßlin and Marco Martin as copyright holders.
The [upstream protocol](https://github.com/KDE/plasma-wayland-protocols/blob/master/src/protocols/blur.xml)
also carries that notice; the exact packaged file hash is preserved in the
[follow-up evidence](fossa-wayland-follow-up-2026-11-13.json).

The selected Linux graph includes this component through winit's Wayland
feature, enabled by the accessibility dependency path described in the initial
review. That is build-selection evidence, not proof about which bytes survive
linking. The source review did not establish an exception covering generated
bindings or complete distribution compliance. Keep the license finding active
until that scope and the applicable distribution obligations are established.

## Reproducibility

The [evidence manifest](fossa-wayland-follow-up-2026-11-13.json) records registry
version observations and SHA-256 hashes of the selected manifests, upstream
commit metadata, winit imports and Plasma protocol source. The original review
records the lockfile identity and four desktop dependency graphs. No dependency,
runtime feature, or release archive was changed by this follow-up.
