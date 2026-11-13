# Verified quality integrations

2026-11-13. Continue the RAPS-style quality work with independently inspectable
coverage and license checks, and a scoped security-control review.

Reuse the self-audit's instrumented test run to export LCOV. Keep only workspace
crate source files, use repository-relative paths, and require line totals to
match the existing JSON report. Preserve the LCOV checksum with the audit. LLVM
summary counters and per-line LCOV observations can differ; preserve both rather
than inventing observations to force Codecov to display the same percentage.

Publish successful, clean Linux audits from `main` to Codecov with GitHub OIDC.
Use a separate workflow: third-party availability must not change the result of
the local engineering assessment. Validate the source commit and evidence hash
before uploading. Do not run PR code or grant the measurement job write access.
Keep Codecov comments disabled. Show a Codecov badge only after a processed report
exists; retain the verified release coverage snapshot otherwise.

Expose the existing cargo-deny license/source policy as a separate CI check.
Prepare FOSSA as an explicit, main-only scan with a push-only repository secret;
missing authentication must fail with an actionable explanation, never pass.
Do not show a FOSSA badge before its project and policy result exist.
Use separate pinned action invocations for analysis and policy testing:
`run-tests: true` selects only `fossa test`. Review the initial result and findings
before enabling main/weekly scans; keep unresolved findings visible and do not
waive issues to obtain a passing badge. Serialize scans and never pass credentials
to PRs or arbitrary branches. A push-only key can return issue counts without
issue details.

Review ASVS 5.0 against the desktop application and public static WASM demo as
separate surfaces. Record applicability, evidence and unverified controls. Do not
translate CI success or chapter-level applicability into an ASVS level claim.

No runtime network behavior, user configuration, release version or desktop input
changes are required for this work.
