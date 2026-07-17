#!/usr/bin/env python3
"""Regression tests for the published site-content contract."""

import unittest

from tools.check_site_content import (
    APP_PATH,
    CSS_PATH,
    HTML_PATH,
    MODEL_PATH,
    SCREENSHOT_PATH,
    SettingsImageParser,
    SignedNgramModel,
    check_ngram_trace,
    ukrainian_translations,
    webp_dimensions,
)


class TranslationParserTests(unittest.TestCase):
    def test_extracts_only_string_values_and_preserves_empty_values_for_validation(self) -> None:
        source = '''
  const ukrainian = {
    present: "Так",
    empty: "",
    invalid: undefined
  };
'''
        self.assertEqual(
            ukrainian_translations(source),
            {"present": "Так", "empty": ""},
        )


class SettingsScreenshotTests(unittest.TestCase):
    def test_decodes_the_published_webp_dimensions(self) -> None:
        self.assertEqual(webp_dimensions(SCREENSHOT_PATH.read_bytes()), (1440, 1360))

    def test_rejects_a_truncated_webp(self) -> None:
        with self.assertRaises(SystemExit):
            webp_dimensions(b"RIFF\0\0\0\0WEBPVP8 ")

    def test_finds_only_the_expected_settings_asset(self) -> None:
        parser = SettingsImageParser()
        parser.feed(
            '<img src="elsewhere.webp"><img src="assets/upyr-settings-feedback.webp" '
            'width="1440" height="1360">'
        )
        self.assertEqual(len(parser.matches), 1)
        self.assertEqual(parser.matches[0]["width"], "1440")


class SignedModelContractTests(unittest.TestCase):
    def test_rejects_an_invalid_model_header(self) -> None:
        with self.assertRaises(SystemExit):
            SignedNgramModel(b"not a model")

    def test_matches_the_published_trace_evidence(self) -> None:
        model = SignedNgramModel(MODEL_PATH.read_bytes())
        english, english_grams = model.coverage("en", "ghbdsn")
        ukrainian, ukrainian_grams = model.coverage("uk", "привіт")
        self.assertEqual((english_grams, ukrainian_grams), (22, 22))
        self.assertAlmostEqual(english, 0.2088189, places=6)
        self.assertAlmostEqual(ukrainian, 0.7042520, places=6)

    def test_rejects_scores_swapped_between_grams(self) -> None:
        html = HTML_PATH.read_text(encoding="utf-8")
        swapped = html.replace(
            "<code>^g</code><b>−112</b>", "<code>^g</code><b>−83</b>"
        )
        swapped = swapped.replace(
            "<code>ghb</code><b>−83</b>", "<code>ghb</code><b>−112</b>"
        )
        with self.assertRaises(SystemExit):
            check_ngram_trace(
                swapped,
                APP_PATH.read_text(encoding="utf-8"),
                CSS_PATH.read_text(encoding="utf-8"),
            )

    def test_rejects_widths_swapped_between_language_bars(self) -> None:
        css = CSS_PATH.read_text(encoding="utf-8")
        swapped = css.replace("width: 20.9%;", "width: SWAP;")
        swapped = swapped.replace("width: 70.4%;", "width: 20.9%;")
        swapped = swapped.replace("width: SWAP;", "width: 70.4%;")
        with self.assertRaises(SystemExit):
            check_ngram_trace(
                HTML_PATH.read_text(encoding="utf-8"),
                APP_PATH.read_text(encoding="utf-8"),
                swapped,
            )


if __name__ == "__main__":
    unittest.main()
