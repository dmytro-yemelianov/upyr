# Public update

2026-11-13. User authorized updated screenshots, an accumulated-change PR,
CI verification and publication of the website and README.

- [x] Build release binaries and capture the actual Ukrainian pixel editor with an isolated config.
- [x] Replace the previous native screenshot; align captions, alt text and asset checks.
- [ ] Verify the site, workspace tests, engine contract and PR checks.
- [ ] Merge through repository rules and verify the deployed site and README.

The screenshot shows the Correction page at 898×779. It contains no user data;
automatic correction is off, matching the default. The runtime and installed
user configuration are unchanged by screenshot capture.

PR #81: CI identified RUSTSEC-2026-0486 in rustix 1.1.4. Updated only that
package to 1.1.6 and its dependency edges; preserved unrelated lockfile choices.
The OSV policy is unchanged. The browser demo produced `привіт hello хліб codex`;
its 444,324-byte Brotli bundle passes the 512,000-byte budget.
