#!/usr/bin/env python3
"""Regression tests for the privacy architecture guard."""

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from tools.check_privacy import (
    ALLOWED_PRE_RENDERED_AUDIO_ROOT,
    WEB_CODE_PATTERNS,
    pre_rendered_audio_files,
    sensitive_log_field,
    validate_csp,
)


VALID_POLICY = (
    "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; "
    "object-src 'none'; base-uri 'none'; form-action 'none'"
)


class ContentSecurityPolicyTests(unittest.TestCase):
    def test_accepts_exact_required_directives(self) -> None:
        self.assertEqual(validate_csp(VALID_POLICY, Path("site/index.html")), [])

    def test_rejects_an_additional_connect_source(self) -> None:
        policy = VALID_POLICY.replace("connect-src 'self'", "connect-src 'self' https://example.com")
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

    def test_exempts_the_documented_anime_event_cue_exception(self) -> None:
        self.assertEqual(
            pre_rendered_audio_files((ALLOWED_PRE_RENDERED_AUDIO_ROOT,)), []
        )

    def test_still_detects_pre_rendered_audio_outside_the_exception(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            wav = root / "sounds" / "anime" / "event.wav"
            wav.parent.mkdir(parents=True)
            wav.write_bytes(b"RIFF")

            self.assertEqual(pre_rendered_audio_files((root,)), [wav])


class BrowserPrivacyApiTests(unittest.TestCase):
    def test_rejects_microphone_capture_apis(self) -> None:
        pattern = WEB_CODE_PATTERNS["microphone capture API"]
        self.assertIsNotNone(pattern.search("navigator.mediaDevices.getUserMedia({ audio: true })"))
        self.assertIsNotNone(pattern.search("new MediaRecorder(stream)"))

    def test_rejects_webrtc_even_with_connect_src_none(self) -> None:
        pattern = WEB_CODE_PATTERNS["WebRTC API"]
        self.assertIsNotNone(pattern.search("new RTCPeerConnection()"))


if __name__ == "__main__":
    unittest.main()
