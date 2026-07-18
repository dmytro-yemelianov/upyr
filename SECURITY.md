# Security policy

Upyr observes global keyboard events and temporarily uses the clipboard to perform
corrections, so privacy and input integrity are security boundaries, not optional
features.

## Supported versions

Security fixes are provided for the latest published release. Older builds may be
fixed when practical, but users should upgrade before reporting a problem that is
already resolved in the current release.

## Report a vulnerability privately

Do not open a public issue with exploit details, sensitive input, credentials, or
private logs. Use GitHub's **Report a vulnerability** form:

<https://github.com/dmytro-yemelianov/upyr/security/advisories/new>

Include the affected version and platform, impact, reproduction steps using
synthetic data, and a proposed mitigation if available. You may state your desired
credit and coordinated-disclosure timeline. The maintainer aims to acknowledge a
report within seven days and will keep the reporter informed while it is triaged.

If private vulnerability reporting is temporarily unavailable, open a public issue
asking for a security contact without including any vulnerability details.

## Privacy and non-tracking invariant

The shipped application is designed to work entirely on the device:

- It has no analytics, advertising, telemetry, crash-upload, account, or tracking
  service.
- It does not transmit keyboard events, selected text, clipboard contents,
  configuration, or n-gram decisions.
- Keyboard samples used for automatic correction remain transient in process
  memory. Logs contain outcomes and character counts, not the captured text.
- The language detector uses an embedded, language-tagged character n-gram model;
  inference does not call a remote model or service.
- Clipboard contents are changed only for a conversion. Restoration is enabled by
  default and attempts to restore supported formats after the synthetic paste;
  users can disable it, and platform failures are reported rather than hidden.
- Configuration is stored locally using the operating system's normal per-user
  application-data location. macOS and Linux writes are atomic and use owner-only
  `0600` permissions; Windows inherits the per-user directory ACL.

The project website contains no project-controlled analytics, cookies, tracking
pixels, or remote runtime resources. GitHub Pages itself is hosted by GitHub and is
subject to GitHub's own infrastructure and privacy practices.

These statements describe the reviewed architecture, not a mathematical proof.
CI includes a privacy guard for common network and telemetry APIs and for remote
website subresources; every release still requires code review.

## Security verification

Pull requests and the default branch are checked with:

- GitHub CodeQL analysis for Rust;
- Semgrep SAST (`p/rust`, `p/secure-defaults`) and an advisory security-lint
  Clippy pass;
- RustSec advisory scanning of the locked Cargo dependency graph;
- OSV-Scanner (cross-ecosystem CVEs) and a Trivy filesystem scan (vulnerabilities,
  leaked secrets, misconfigurations) on the default branch and weekly;
- GitHub dependency review for newly introduced vulnerable dependencies;
- Clippy, formatting, unit/integration tests, and cross-platform builds;
- a privacy-architecture guard that rejects common outbound network and tracking
  integrations in application code;
- pinned GitHub Actions, least-privilege workflow tokens, release checksums,
  GitHub artifact provenance attestations, a CycloneDX SBOM, and keyless Cosign
  signatures.

Tagged macOS and Windows releases must use the configured platform signing
identities; macOS packages are also notarized. Verification reduces risk but does
not guarantee that the software is vulnerability-free.

### Verifying a release

Every tagged release ships `SHA256SUMS.txt`, a Cosign signature
(`SHA256SUMS.txt.sig` + `SHA256SUMS.txt.pem`), a CycloneDX SBOM
(`upyr-sbom.tar.gz`), and a GitHub build-provenance attestation.

```sh
# 1. integrity
sha256sum --check SHA256SUMS.txt

# 2. authenticity — the checksums were signed by this repo's release workflow
cosign verify-blob \
  --certificate SHA256SUMS.txt.pem \
  --signature SHA256SUMS.txt.sig \
  --certificate-identity-regexp 'https://github.com/dmytro-yemelianov/upyr/.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS.txt

# 3. provenance — the artifact was built by this repo's workflow
gh attestation verify <artifact> --repo dmytro-yemelianov/upyr
```

macOS: `spctl -a -vv Upyr.app` and `codesign --verify --deep --strict Upyr.app`.
Windows: `signtool verify /pa /v <file>`.

## High-value report areas

Reports are especially useful when they involve:

- unintended capture, logging, persistence, or disclosure of typed text;
- clipboard restoration failures or exposure across process boundaries;
- synthetic key-event loops, modifier-state races, or text corruption;
- bypasses of accessibility/desktop-input permission handling;
- unsafe installer, autostart, update, signing, or notarization behavior;
- malicious or malformed n-gram model, configuration, or layout data;
- release artifact, CI, or dependency supply-chain compromise.
