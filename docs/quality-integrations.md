# Quality service connections

## Codecov

[Coverage publication](../.github/workflows/codecov.yml) consumes a successful
`Self-audit` run from `main`. It verifies that run's repository, workflow, event,
commit, clean source identity and LCOV checksum before using GitHub OIDC to upload
the report. No repository upload secret or PR execution with elevated permissions
is needed. Measurement and publication are separate workflows.

The report covers Linux-host native workspace crate sources, including inline
tests. Standalone tests/examples and dependencies are excluded. LLVM summary
counters are checked against the self-audit JSON; Codecov may compute a different
percentage from individual LCOV line records. These are different aggregations,
not evidence that one run exercised additional tests. Codecov comments and its
additional coverage gates are disabled; the local core >= 90% gate remains in
the self-audit.

The first publication was verified on 2026-11-13: Codecov processed commit
[`fa637ca`](https://app.codecov.io/github/dmytro-yemelianov/upyr/commit/fa637cadbaacaf314a06b1d2c8c83a85cb094d3c)
and reported **80.78%** (15,293 of 18,930 lines across 61 files), with a numeric
badge and completed processing state. The live [dashboard](https://app.codecov.io/gh/dmytro-yemelianov/upyr)
follows subsequent successful `main` audits. A successful upload alone does not
establish that the service processed a report. An explicit retry accepts a
successful `Self-audit` run ID through `workflow_dispatch`.

## Dependency license policy

[Dependency licenses](../.github/workflows/licenses.yml) runs the same pinned
`cargo-deny` policy as the self-audit, with all workspace packages/features and the
committed lockfile. Its status covers Cargo license metadata and allowed sources.
Asset provenance and redistribution terms remain in
[third-party notices](../THIRD_PARTY_NOTICES.md).

## FOSSA policy review

FOSSA is connected through the repository's push-only `FOSSA_API_KEY` secret.
The first [CI policy check](https://github.com/dmytro-yemelianov/upyr/actions/runs/43431551894)
on 2026-11-13 checked the maintainer-uploaded `aaac395` revision and **failed with
21 issues**. Authentication succeeded. The push-only key cannot retrieve issue
details, so these findings require review in the authenticated
[FOSSA report](https://app.fossa.com/projects/custom%2B59498%2Fgithub.com%2Fdmytro-yemelianov%2Fupyr/refs/branch/main/aaac395ecf16f9feb5f9c73c05d39a7bf6e72842).
The public license-scan badge also reported `failing`. This does not establish
that all 21 findings are vulnerabilities or license violations. The authenticated
dashboard separates them into **14 license findings and 7 outdated dependencies**.
The [initial findings review](security/fossa-review-2026-11-13.md) records exact
versions, source evidence, platform scope and remaining decisions.

The [manual, main-only workflow](../.github/workflows/fossa.yml) now uses two pinned
action invocations: first `fossa analyze`, then `fossa test` for the checked-out
revision. The [action's `run-tests` option](https://github.com/fossas/fossa-action/tree/29693cc50323968e039056be419b32989fc5880c#run-tests)
selects testing instead of analysis; the initial workflow mistakenly ran only the
test. The corrected [main run](https://github.com/dmytro-yemelianov/upyr/actions/runs/43433033499)
successfully uploaded `25d3d4e` and then failed its policy test with the same 21
open issues. Future revisions must be uploaded before their policy is checked.

[CLI 3.20.10 and the project configuration](../.fossa.yml) target only the root
Cargo workspace, including its members. Unrelated ignored checkouts/tooling are
excluded. Missing authentication fails explicitly. Recurring scans remain off
until the initial policy and findings are reviewed. The local `cargo-deny` policy
does not configure FOSSA's account policy.

To rotate the credential, create a **Push Only** token as recommended by the
[official integration guide](https://docs.fossa.com/docs/integrations/github-actions)
and update `FOSSA_API_KEY` in
[Actions secrets](https://github.com/dmytro-yemelianov/upyr/settings/secrets/actions).
Do not paste the key into chat, source files or PR comments. Review issue details
in the dashboard without expanding the CI credential's access.

No new account, broad GitHub OAuth grant or paid subscription is created by these
files. FOSSA covers its configured analysis and policy; it is not a blanket legal
certification of all distributed assets.
