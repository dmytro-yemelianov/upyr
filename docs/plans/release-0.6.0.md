# Release 0.6.0 plan

Spec: [release-0.6.0.md](../specs/release-0.6.0.md).

1. Synchronize versions and write public release notes.
2. Add explicit CI draft-preview publication and package install smoke checks.
3. Implement the [self-audit plan](self-audit.md), pass the release PR checks and
   merge to main.
4. Build all three platforms from that main commit; approve the existing release
   environment for this authorized run and inspect its draft assets, including
   the exact-commit self-audit evidence.
5. Download packages, verify checksums and macOS clean install/upgrade, install
   the verified bundle locally with backup and configuration preservation.
6. Publish the verified draft; verify public assets and the updated site.

Completion evidence belongs in the release's validation report and PR, so this
preparation plan does not require another source commit after publication.

Before publishing the draft, generate and attach the downloaded-package report:

```sh
python3 tools/check_macos_release.py --app /path/to/downloaded/Upyr.app \
  --previous-app /path/to/published-0.5.0/Upyr.app \
  --output target/release-validation-0.6.0.json
gh release upload preview-0.6.0 target/release-validation-0.6.0.json
```

Verify downloaded checksums and the completed Windows/Linux installation jobs
as well. Publish only after that report is attached. The site deployment checks
for a published version matching Cargo and retries on the release publication
event, so merging preparation does not advertise a draft as downloadable.
