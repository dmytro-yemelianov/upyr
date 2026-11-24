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

## FOSSA activation follow-through

1. Verify the repository secret through a manual main-branch run.
2. Split the pinned action into analysis/upload, then policy testing; validate the
   workflow and run it on main after merging.
3. Review the initial 21 findings in the authenticated project dashboard; the
   push-only key intentionally cannot retrieve their details.
4. Publish the actual service status with its findings review. After confirming
   the result and categories, enable main/weekly scans while preserving failures
   and unresolved findings.
5. Preserve upstream font notices in the existing packaged notices file. Record
   alternate-license and platform/asset scope evidence for exact crate versions;
   keep Linux protocol licensing and the active outdated package open.
6. Apply approved applicability decisions only to the selected package versions
   in Upyr, with evidence and re-review conditions. Record the actual service
   outcome before updating status claims; do not add global or all-version rules.
7. Review the two active Linux Wayland findings separately. The
   [follow-up](../security/fossa-wayland-follow-up-2026-11-13.md) establishes why
   neither a compatible lockfile update nor an MIT-metadata dismissal resolves
   them. Keep both active pending a supported migration or distribution review.
