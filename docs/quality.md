# Quality and self-audit

Upyr publishes an **engineering self-assessment** with reproducible measurements.
It is not an independent certification, a compliance seal or a single quality score.

[Latest self-audit runs](https://github.com/dmytro-yemelianov/upyr/actions/workflows/self-audit.yml)
· [All CI checks](https://github.com/dmytro-yemelianov/upyr/actions)
· [Release evidence](https://github.com/dmytro-yemelianov/upyr/releases)

Open a run's summary for the measurements; download its `self-audit-<commit>`
artifact for `report.md`, `report.json`, raw logs, coverage HTML and reusable SVG
badges. The preview release includes the evidence for its exact source commit.
CI artifacts expire after 90 days; release attachments retain the release snapshot.

## What is measured

| Area | Measurement / acceptance criterion | Scope |
| --- | --- | --- |
| Tests | Passed, failed and ignored totals; zero failures | Native-host workspace, all targets, default features |
| Coverage | Covered/total lines and functions for each crate and the workspace; core lines ≥ 90% | Native test build, including inline unit tests |
| Dependencies | RustSec known vulnerabilities = 0; every maintenance/yanked warning remains visible | Entire lockfile; database revision in raw report |
| Licenses / sources | `cargo-deny` allowlist and version-scoped exceptions; crates.io sources only | Cargo dependency metadata, not asset rights |
| Code checks | `rustfmt`, Clippy with warnings denied, rustdoc with warnings denied | Workspace; Clippy includes all targets/features |
| Privacy | Offline runtime / non-tracking architecture guard and its regression tests | Source/dependency and website invariants |
| Correctness / memory | 18 portable engine oracle, allocation and memory-budget checks | Versioned synthetic fixtures; exact limits in report |
| Latency | Mapping p50, p95, p99, max, sample count and allocations | Release build on the named host; observation, no timing gate |
| Evidence integrity | Commit, dirty-tree flag, source/lockfile hashes, tool versions, commands and SHA-256 of evidence | Source changes during a run fail the audit |

The core coverage floor starts below the first measured macOS baseline (92.48%).
Other crate and workspace percentages are reported without an arbitrary target.
A passing core badge does not imply that the entire application has 90% coverage.
Coverage can differ by OS; host tests of `upyr-wasm` are not browser execution.

## Complementary checks

The self-audit does **not** inherit green results from unrelated or older runs.
These separate workflows must be inspected for the same commit:

- [CI](../.github/workflows/ci.yml): macOS, Windows and Linux tests, minimum Rust,
  WASM/Node contracts and compressed bundle size.
- [Security](../.github/workflows/security.yml): CodeQL, Semgrep, dependency review
  and additional security lints.
- [OSV](../.github/workflows/osv-scanner.yml): cross-ecosystem advisory scan, with
  [documented exceptions and expiry dates](../osv-scanner.toml).
- [Trivy](../.github/workflows/trivy-fs.yml): high/critical fixable dependency
  findings, secrets and configuration scan. Lower severity and unfixed findings
  are outside that gate.
- [Release packages](../.github/workflows/release.yml): package conversion and
  installation checks, checksums and build provenance.
- [SBOM](../.github/workflows/sbom.yml): CycloneDX dependency inventory attached
  after publication.

## Reproduce

Install the platform dependencies listed in the [reference](reference.md#build-and-run),
then use Rust 1.95.0, Python 3 and the same pinned tools as CI:

```sh
rustup component add llvm-tools-preview rustfmt clippy --toolchain 1.95.0
cargo +1.95.0 install cargo-llvm-cov --version 0.9.0 --locked
cargo +1.95.0 install cargo-audit --version 0.22.1 --locked
cargo +1.95.0 install cargo-deny --version 0.19.8 --locked
RUSTUP_TOOLCHAIN=1.95.0 python3 tools/run_self_audit.py --output target/audit-new
```

Use a fresh output directory each time. No desktop input or real user config is
needed. The runner preserves failed checks and partial evidence; a failed setup
or canceled CI job is not a completed assessment. The workflow badge reflects the
workflow result, not a vulnerability guarantee or a percentage.

## Limits and open work

Line coverage measures execution, not assertion quality. Branch/MC/DC coverage,
mutation testing, a continuous fuzzing campaign, formal verification and an
independent penetration test are not measured. Synthetic mapping time excludes
the operating system's editor, clipboard and input-delivery latency.

Maintenance warnings are not suppressed by the audit. Changes in dependencies
or advisory databases can change the result without a source change. Licensing
checks cover Cargo metadata; the imported rules, corpora, sounds and fonts have
separate provenance in [third-party notices](../THIRD_PARTY_NOTICES.md). Passing
that check makes no claim of blanket redistribution rights.
