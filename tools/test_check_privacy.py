#!/usr/bin/env python3
"""Regression tests for the privacy architecture guard."""

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from tools.check_privacy import pre_rendered_audio_files, sensitive_log_field, validate_csp


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


class SensitiveLoggingTests(unittest.TestCase):
    def test_rejects_exact_physical_key_fields(self) -> None:
        self.assertEqual(sensitive_log_field('debug!(?key, "playback failed");'), "key")
        self.assertEqual(
            sensitive_log_field('warn!(keycode = ?event.keycode, "listener failed");'),
            "keycode",
        )

    def test_allows_privacy_preserving_key_categories(self) -> None:
        self.assertIsNone(sensitive_log_field('debug!(?cue, "keyboard sound unavailable");'))


class BundledAudioTests(unittest.TestCase):
    def test_detects_pre_rendered_audio_case_insensitively(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            wav = root / "event.WAV"
            wav.write_bytes(b"RIFF")
            (root / "notes.txt").write_text("generated locally", encoding="utf-8")

            self.assertEqual(pre_rendered_audio_files((root,)), [wav])

    def test_accepts_missing_asset_roots(self) -> None:
        with TemporaryDirectory() as directory:
            missing = Path(directory) / "missing"
            self.assertEqual(pre_rendered_audio_files((missing,)), [])


if __name__ == "__main__":
    unittest.main()
