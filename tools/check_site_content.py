#!/usr/bin/env python3
"""Verify the bilingual product tour and its frozen n-gram example."""

from __future__ import annotations

import re
import struct
from html.parser import HTMLParser
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HTML_PATH = ROOT / "site" / "index.html"
APP_PATH = ROOT / "site" / "app.js"
CSS_PATH = ROOT / "site" / "styles.css"
MODEL_PATH = ROOT / "crates" / "upyr-core" / "assets" / "models" / "language.ngm"
ENGLISH_DICTIONARY_PATH = ROOT / "crates" / "upyr-core" / "assets" / "dictionaries" / "english.txt"
UKRAINIAN_DICTIONARY_PATH = ROOT / "crates" / "upyr-core" / "assets" / "dictionaries" / "ukrainian.txt"
SCREENSHOT_PATH = ROOT / "site" / "assets" / "upyr-settings-feedback.webp"

MODEL_MAGIC = b"UPYRLM1\0"
MODEL_HEADER_SIZE = 12
MODEL_ENTRY_SIZE = 17
MODEL_MAX_STRENGTH = 127


def fail(message: str) -> None:
    raise SystemExit(f"site content verification failed: {message}")


def translation_keys(html: str) -> set[str]:
    return set(re.findall(r'data-i18n(?:-aria|-alt)?="([A-Za-z][A-Za-z0-9]*)"', html))


def ukrainian_translations(app: str) -> dict[str, str]:
    start = app.find("const ukrainian = {")
    end = app.find("\n  };", start)
    if start < 0 or end < 0:
        fail("could not locate the Ukrainian translation object")
    return dict(
        re.findall(
            r'^    ([A-Za-z][A-Za-z0-9]*):\s*"((?:\\.|[^"\\])*)",?$',
            app[start:end],
            re.MULTILINE,
        )
    )


def ngram_key(text: str) -> int:
    key = len(text)
    for character in text:
        key = (key << 21) | ord(character)
    return key


class SignedNgramModel:
    def __init__(self, data: bytes) -> None:
        if len(data) < MODEL_HEADER_SIZE or data[:8] != MODEL_MAGIC:
            fail("the signed n-gram artifact has an invalid header")
        self.data = data
        self.count = struct.unpack_from("<I", data, 8)[0]
        expected_size = MODEL_HEADER_SIZE + self.count * MODEL_ENTRY_SIZE
        if len(data) != expected_size:
            fail(f"the signed n-gram artifact is {len(data)} bytes; expected {expected_size}")

    def key_at(self, index: int) -> int:
        offset = MODEL_HEADER_SIZE + index * MODEL_ENTRY_SIZE
        return int.from_bytes(self.data[offset : offset + 16], "little")

    def score_at(self, index: int) -> int:
        offset = MODEL_HEADER_SIZE + index * MODEL_ENTRY_SIZE + 16
        return struct.unpack_from("b", self.data, offset)[0]

    def lookup(self, gram: str) -> int:
        target = ngram_key(gram)
        start = 0
        end = self.count
        while start < end:
            middle = start + (end - start) // 2
            candidate = self.key_at(middle)
            if candidate < target:
                start = middle + 1
            elif candidate > target:
                end = middle
            else:
                return self.score_at(middle)
        return 0

    def coverage(self, language: str, word: str) -> tuple[float, int]:
        characters = f"^{word.lower()}$"
        sign = -1 if language == "en" else 1
        evidence = 0
        maximum = 0
        grams = 0
        for size in range(2, 6):
            weight = size - 1
            for offset in range(len(characters) - size + 1):
                gram = characters[offset : offset + size]
                evidence += self.lookup(gram) * sign * weight
                maximum += MODEL_MAX_STRENGTH * weight
                grams += 1
        return evidence / maximum, grams


class SettingsImageParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.matches: list[dict[str, str]] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = {name: value or "" for name, value in attrs}
        if tag == "img" and values.get("src") == "assets/upyr-settings-feedback.webp":
            self.matches.append(values)


def webp_dimensions(data: bytes) -> tuple[int, int]:
    if len(data) < 20 or data[:4] != b"RIFF" or data[8:12] != b"WEBP":
        fail("settings screenshot is not a valid WebP asset")
    offset = 12
    while offset + 8 <= len(data):
        kind = data[offset : offset + 4]
        size = struct.unpack_from("<I", data, offset + 4)[0]
        payload = offset + 8
        if payload + size > len(data):
            fail("settings screenshot contains a truncated WebP chunk")
        if kind == b"VP8 " and size >= 10 and data[payload + 3 : payload + 6] == b"\x9d\x01\x2a":
            width = struct.unpack_from("<H", data, payload + 6)[0] & 0x3FFF
            height = struct.unpack_from("<H", data, payload + 8)[0] & 0x3FFF
            return width, height
        if kind == b"VP8L" and size >= 5 and data[payload] == 0x2F:
            bits = int.from_bytes(data[payload + 1 : payload + 5], "little")
            return (bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1
        if kind == b"VP8X" and size >= 10:
            width = int.from_bytes(data[payload + 4 : payload + 7], "little") + 1
            height = int.from_bytes(data[payload + 7 : payload + 10], "little") + 1
            return width, height
        offset = payload + size + (size % 2)
    fail("settings screenshot contains no decodable WebP image chunk")


def check_screenshot(html: str) -> None:
    if not SCREENSHOT_PATH.is_file():
        fail(f"missing {SCREENSHOT_PATH.relative_to(ROOT)}")
    data = SCREENSHOT_PATH.read_bytes()
    dimensions = webp_dimensions(data)
    if dimensions != (1440, 1360):
        fail(f"settings screenshot is {dimensions[0]}×{dimensions[1]}; expected 1440×1360")
    parser = SettingsImageParser()
    parser.feed(html)
    if len(parser.matches) != 1:
        fail("site must render exactly one settings screenshot")
    image = parser.matches[0]
    if image.get("width") != "1440" or image.get("height") != "1360":
        fail("settings screenshot markup must match its intrinsic 1440×1360 dimensions")


def check_sound_packs(html: str) -> None:
    packs = re.findall(r'data-sound-preview="([a-z]+)"', html)
    if len(packs) != 3 or set(packs) != {"original", "arcade", "anime"}:
        fail(f"unexpected sound preview controls: {packs}")


def check_ngram_trace(html: str, app: str, css: str) -> None:
    model = SignedNgramModel(MODEL_PATH.read_bytes())
    formatted_count = f"{model.count:,}"
    if formatted_count not in html or formatted_count.replace(",", " ") not in app:
        fail(f"site copy does not match the model's {formatted_count} records")
    formatted_size = f"{len(model.data) / (1024 * 1024):.1f}"
    if f"{formatted_size} MiB" not in html or f"{formatted_size.replace('.', ',')} МіБ" not in app:
        fail(f"site copy does not match the model's {formatted_size} MiB size")

    samples = {
        "^g": (-112, "record-en"),
        "ghb": (-83, "record-en"),
        "sn$": (-63, "record-en"),
        "^п": (127, "record-uk"),
        "при": (116, "record-uk"),
        "ивіт$": (43, "record-uk"),
    }
    for gram, (expected, language_class) in samples.items():
        actual = model.lookup(gram)
        if actual != expected:
            fail(f"model record {gram!r} is {actual}; the site expects {expected}")
        signed = f"+{actual}" if actual > 0 else f"−{abs(actual)}"
        record_pattern = re.compile(
            rf'<span\s+class="{language_class}">\s*<code>{re.escape(gram)}</code>'
            rf"\s*<b>{re.escape(signed)}</b>\s*</span>"
        )
        if not record_pattern.search(html):
            fail(
                f"site trace does not bind {gram!r} to {signed} in {language_class}"
            )

    english, english_grams = model.coverage("en", "ghbdsn")
    ukrainian, ukrainian_grams = model.coverage("uk", "привіт")
    advantage = ukrainian - english
    if (english_grams, ukrainian_grams) != (22, 22):
        fail(f"trace gram counts changed to {english_grams} and {ukrainian_grams}")
    coverages = (
        ("ghbdsn", "EN", english, "coverage-en"),
        ("привіт", "УК", ukrainian, "coverage-uk"),
    )
    for word, label, coverage, selector in coverages:
        value = f"{coverage:.3f}"
        row_pattern = re.compile(
            rf"<div>\s*<span>\s*<code>{re.escape(word)}</code>"
            rf"\s*<small>{label}\s*·\s*{re.escape(value)}</small>\s*</span>"
            rf'\s*<i>\s*<b\s+class="{selector}"></b>\s*</i>\s*</div>'
        )
        if not row_pattern.search(html):
            fail(
                f"site trace does not bind {word!r}, {label}, {value}, and {selector}"
            )
        width = f"{coverage * 100:.1f}%"
        bar_pattern = re.compile(
            rf"\.{selector}\s*\{{[^}}]*\bwidth:\s*{re.escape(width)}\s*;",
            re.DOTALL,
        )
        if not bar_pattern.search(css):
            fail(f"site trace does not bind {selector} to model width {width}")

    advantage_pattern = re.compile(
        rf'class="coverage-note"[^>]*>.*?<strong>\+{advantage:.3f}</strong>',
        re.DOTALL,
    )
    if not advantage_pattern.search(html):
        fail(f"site trace is missing the bound model advantage +{advantage:.3f}")

    english_words = set(ENGLISH_DICTIONARY_PATH.read_text(encoding="utf-8").splitlines())
    ukrainian_words = set(UKRAINIAN_DICTIONARY_PATH.read_text(encoding="utf-8").splitlines())
    if "ghbdsn" in english_words or "привіт" not in ukrainian_words:
        fail("the trace's known/unknown dictionary decision no longer matches the dictionaries")


def main() -> None:
    html = HTML_PATH.read_text(encoding="utf-8")
    app = APP_PATH.read_text(encoding="utf-8")
    css = CSS_PATH.read_text(encoding="utf-8")
    required = translation_keys(html)
    translations = ukrainian_translations(app)
    missing = sorted(required - translations.keys())
    if missing:
        fail("missing Ukrainian translations: " + ", ".join(missing))
    empty = sorted(key for key in required if not translations[key].strip())
    if empty:
        fail("empty Ukrainian translations: " + ", ".join(empty))

    check_screenshot(html)
    check_sound_packs(html)
    check_ngram_trace(html, app, css)
    print(
        "site content verification passed: "
        f"{len(required)} bilingual keys, 3 sound packs, and frozen n-gram trace values"
    )


if __name__ == "__main__":
    main()
