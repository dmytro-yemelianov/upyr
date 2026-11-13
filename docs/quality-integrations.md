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

After the first successful publication, verify the processed commit and numeric
badge at [Codecov](https://app.codecov.io/gh/dmytro-yemelianov/upyr). A successful
upload alone does not establish that the service processed the report. An explicit
retry accepts a successful `Self-audit` run ID through `workflow_dispatch`.

## Dependency license policy

[Dependency licenses](../.github/workflows/licenses.yml) runs the same pinned
`cargo-deny` policy as the self-audit, with all workspace packages/features and the
committed lockfile. Its status covers Cargo license metadata and allowed sources.
Asset provenance and redistribution terms remain in
[third-party notices](../THIRD_PARTY_NOTICES.md).

## FOSSA activation

The [manual, main-only FOSSA workflow](../.github/workflows/fossa.yml) and
[project configuration](../.fossa.yml) are ready. The project has no confirmed
FOSSA server result yet. CLI 3.20.10 has been checked locally in `--output` mode:
the explicit target is the root Cargo workspace, including its members. Unrelated
ignored checkouts/tooling are excluded. To connect it:

1. Sign in to FOSSA and create a **Push Only** API token, as recommended for public
   repositories by the [official integration guide](https://docs.fossa.com/docs/integrations/github-actions).
2. Add it as `FOSSA_API_KEY` in
   [this repository's Actions secrets](https://github.com/dmytro-yemelianov/upyr/settings/secrets/actions).
   Do not paste the key into chat, source files or PR comments.
3. Run **FOSSA policy review** on `main`. The pinned action analyzes the dependency
   graph, then requires `fossa test` to pass. Missing authentication is an explicit
   error, not a skipped successful scan.
4. Review the actual FOSSA project, selected policy and findings. Add its badge and
   recurring triggers only after the project is accessible and the result is
   verified. The local `cargo-deny` policy does not configure FOSSA's account policy.

No new account, broad GitHub OAuth grant or paid subscription is created by these
files. FOSSA covers its configured analysis and policy; it is not a blanket legal
certification of all distributed assets.
