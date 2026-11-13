# Evidence-backed self-audit

2026-11-13. Publish a reproducible engineering self-assessment for Upyr, inspired
by the Rapsody audit reports. It is not an independent certification or an
aggregate security score.

Every run identifies its commit, dirty-tree state, source fingerprint, lockfile,
tool versions, platform, commands and completion time. Keep raw evidence and its
checksums alongside JSON, Markdown and reusable SVG badges. Failed, missing or
malformed measurements must never produce a passing audit.

Measure workspace line/function coverage per crate; gate portable core line
coverage at 90%, below the measured macOS baseline of 92.48%. Do not imply that
native-host coverage measures browser execution, all platforms or live GUI use.
Report test failures and ignored tests, RustSec vulnerabilities and maintenance
warnings, dependency license/source policy, formatting, Clippy, rustdoc, privacy
checks, engine correctness/allocation/memory budgets, and synthetic latency
percentiles. Timing is observational, not a noisy shared-runner gate.

Run automatically on PRs, main, weekly and by request with read-only permissions.
Publish a workflow summary and evidence artifact even on failure. Link existing
three-platform, WASM and security workflows with their distinct scope; do not
claim their latest results were measured by this run. Following the user's RAPS
badge example, keep the README to two compact rows: release, Cargo version, asset
downloads, documentation, license and Rust minimum; then CI, security, self-audit,
release core coverage and security policy. Live workflow badges follow main;
coverage identifies a verified release snapshot and host. Explain their scope
in the detailed report. Do not imply unconfigured FOSSA/Codecov integrations or
an ASVS assessment.

Attach the audit for the exact release commit to the 0.6.0 preview. Document
unmeasured areas and remaining warnings explicitly. Do not add compliance logos
or copy stale Rapsody claims.
