# Self-audit plan

Spec: [self-audit.md](../specs/self-audit.md).

1. Inspect Rapsody's prior approach and measure Upyr's coverage baseline.
2. Add a fail-closed runner, dependency policy and evidence-report tests.
3. Add read-only CI, compact README badge and a documented measurement matrix.
4. Run locally, review raw metrics and fix measurement or code failures.
5. Run on the release PR and main; include exact-commit evidence in 0.6.0.
6. Match the user's RAPS badge example with two compact README rows. Verify each
   image and destination; distinguish live workflow status from pinned release
   coverage and Cargo/package metadata. Document unsupported certification labels.

No desktop input is required. Preserve partial logs and failures instead of
silently reusing a previous successful report.
