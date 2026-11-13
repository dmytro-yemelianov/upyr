# Quality integrations plan

Spec: [quality-integrations.md](../specs/quality-integrations.md).

1. Verify official integration requirements, existing repository configuration and
   the current ASVS version.
2. Export and validate LCOV from the existing audit run; add failure-case tests.
3. Add isolated Codecov publication and a dedicated dependency-license workflow.
4. Prepare FOSSA configuration and request only the missing account setup step.
5. Document the security review's applicability, evidence and remaining gaps.
6. Run relevant local checks and PR CI, merge, verify actual service results, then
   update only badges supported by those results.
