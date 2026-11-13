# Upyr 0.6.0 public preview

2026-11-13. Publish the already-tested correction engine, complete pixel settings
and Ukrainian interface in downloadable macOS, Windows and Linux packages.

Use the existing preview distribution policy and a `preview-0.6.0` prerelease
tag. The release workflow must build from main, prepare a draft with checksums
and provenance, and leave public publication until package verification. Keep
the existing official-tag credential gates unchanged. No crates.io/npm publish.

Synchronize workspace/path dependency versions, lockfile, public docs, website
and issue templates. Move completed Unreleased changes into 0.6.0; keep future
plans outside its release notes. Provide a short download/upgrade guide.

Verify portable conversion and package installation in CI; verify the downloaded
macOS bundle, DMG, clean install and upgrade from the published 0.5.0 package
using isolated configuration fixtures. Preserve the owner's configuration and
keep a backup when installing the verified macOS build locally.
