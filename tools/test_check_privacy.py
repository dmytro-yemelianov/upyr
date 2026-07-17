#!/usr/bin/env python3
"""Regression tests for the privacy architecture guard."""

import unittest
from pathlib import Path

from tools.check_privacy import validate_csp


VALID_POLICY = (
    "default-src 'self'; script-src 'self'; connect-src 'none'; "
    "object-src 'none'; base-uri 'none'; form-action 'none'"
)


class ContentSecurityPolicyTests(unittest.TestCase):
    def test_accepts_exact_required_directives(self) -> None:
        self.assertEqual(validate_csp(VALID_POLICY, Path("site/index.html")), [])

    def test_rejects_an_additional_connect_source(self) -> None:
        policy = VALID_POLICY.replace("connect-src 'none'", "connect-src 'none' https://example.com")
        errors = validate_csp(policy, Path("site/index.html"))
        self.assertTrue(any("must be exactly" in error for error in errors))

    def test_rejects_duplicate_directives(self) -> None:
        policy = "connect-src https://example.com; " + VALID_POLICY
        errors = validate_csp(policy, Path("site/index.html"))
        self.assertTrue(any("duplicate `connect-src`" in error for error in errors))

    def test_reports_a_missing_required_directive(self) -> None:
        policy = VALID_POLICY.replace("form-action 'none'", "")
        errors = validate_csp(policy, Path("site/index.html"))
        self.assertTrue(any("CSP is missing `form-action 'none'`" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
