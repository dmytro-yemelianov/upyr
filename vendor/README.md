# Vendored dependencies

## `wayland-scanner` 0.31.10

Upyr vendors `wayland-scanner` because the released crate depends on the
advisory-affected `quick-xml` 0.39 line even when Upyr enables only X11 in its
Linux UI stack.

- Upstream: <https://github.com/Smithay/wayland-rs>
- Release source commit: `a3d7927d87799b2955bf491b51c7c2a3a82da661`
- Local changes: `quick-xml = "0.39"` to `quick-xml = "0.41"`, plus the
  corresponding `xml_content()` to `xml10_content()` API rename in `parse.rs`
- License: MIT; the upstream `LICENSE.txt` is included beside the source

To verify the two-line patch against the release commit:

```sh
git diff --no-index \
  /path/to/wayland-rs/wayland-scanner \
  vendor/wayland-scanner-0.31.10
```

Remove this vendored copy when a compatible patched release is available on
crates.io. Progress is tracked in GitHub issue #2.
