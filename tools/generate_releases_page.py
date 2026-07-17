#!/usr/bin/env python3
"""Generate site/releases.html from CHANGELOG.md.

CHANGELOG.md is the single source of truth for release notes. This script
renders it into a static page matching the rest of the site (same shell,
header, footer, and Content-Security-Policy), so the release history never
drifts from the changelog a contributor already maintains. It performs no
network access and reads only the repository's own files.
"""

from __future__ import annotations

import html
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CHANGELOG = ROOT / "CHANGELOG.md"
OUTPUT = ROOT / "site" / "releases.html"

VERSION_HEADER = re.compile(r"^## \[(?P<label>[^\]]+)\](?:\s*-\s*(?P<date>\S+))?\s*$")
SUBSECTION_HEADER = re.compile(r"^### (?P<title>.+)$")
REFERENCE_LINK = re.compile(r"^\[(?P<label>[^\]]+)\]:\s*(?P<url>\S+)\s*$")
INLINE_CODE = re.compile(r"`([^`]+)`")
INLINE_LINK = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")


class Release:
    def __init__(self, label: str, date: str | None) -> None:
        self.label = label
        self.date = date
        self.tagline: str | None = None
        self.sections: list[tuple[str, list[str]]] = []
        self.url: str | None = None

    @property
    def is_unreleased(self) -> bool:
        return self.label == "Unreleased"


def parse_changelog(text: str) -> list[Release]:
    lines = text.splitlines()
    references: dict[str, str] = {}
    body_lines: list[str] = []
    for line in lines:
        match = REFERENCE_LINK.match(line)
        if match:
            references[match.group("label")] = match.group("url")
        else:
            body_lines.append(line)

    releases: list[Release] = []
    current: Release | None = None
    current_section: list[str] | None = None
    pending_bullet: list[str] | None = None

    def flush_bullet() -> None:
        nonlocal pending_bullet
        if pending_bullet is not None and current_section is not None:
            current_section.append(" ".join(pending_bullet))
        pending_bullet = None

    for line in body_lines:
        header = VERSION_HEADER.match(line)
        if header:
            flush_bullet()
            current = Release(header.group("label"), header.group("date"))
            current.url = references.get(header.group("label"))
            releases.append(current)
            current_section = None
            continue
        if current is None:
            continue

        subsection = SUBSECTION_HEADER.match(line)
        if subsection:
            flush_bullet()
            current_section = []
            current.sections.append((subsection.group("title"), current_section))
            continue

        stripped = line.strip()
        if stripped.startswith("- "):
            flush_bullet()
            pending_bullet = [stripped[2:].strip()]
        elif stripped and pending_bullet is not None:
            pending_bullet.append(stripped)
        elif stripped and current_section is None and current.tagline is None:
            current.tagline = stripped

    flush_bullet()
    return releases


def render_inline(text: str) -> str:
    escaped = html.escape(text, quote=False)

    def code_sub(match: re.Match[str]) -> str:
        return f"<code>{match.group(1)}</code>"

    escaped = INLINE_CODE.sub(code_sub, escaped)

    def link_sub(match: re.Match[str]) -> str:
        return f'<a class="text-link" href="{match.group(2)}">{match.group(1)}</a>'

    return INLINE_LINK.sub(link_sub, escaped)


def render_release(release: Release) -> str:
    parts: list[str] = []
    heading = f"v{release.label}" if not release.is_unreleased else "Unreleased"
    css_class = "release-card release-card-unreleased" if release.is_unreleased else "release-card"
    parts.append(f'<article class="{css_class}">')
    parts.append('<div class="release-heading">')
    parts.append(f"<h2>{html.escape(heading)}</h2>")
    if release.date:
        parts.append(f'<time datetime="{html.escape(release.date)}">{html.escape(release.date)}</time>')
    parts.append("</div>")
    if release.tagline:
        parts.append(f'<p class="release-tagline">{render_inline(release.tagline)}</p>')

    for title, items in release.sections:
        if not items:
            continue
        parts.append('<div class="release-section">')
        parts.append(f'<h3>{html.escape(title)}</h3>')
        parts.append("<ul>")
        for item in items:
            parts.append(f"<li>{render_inline(item)}</li>")
        parts.append("</ul>")
        parts.append("</div>")

    if release.url and not release.is_unreleased:
        parts.append('<div class="release-actions">')
        parts.append(
            f'<a class="button button-ghost" href="{html.escape(release.url)}">'
            f"View release &amp; downloads<span aria-hidden=\"true\"> &#8594;</span></a>"
        )
        parts.append("</div>")

    parts.append("</article>")
    return "\n".join(parts)


PAGE_TEMPLATE = """<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="theme-color" content="#11151c">
    <meta name="color-scheme" content="dark">
    <meta
      http-equiv="Content-Security-Policy"
      content="default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'none'; connect-src 'self'; media-src 'none'; object-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'"
    >
    <meta
      name="description"
      content="Release history for Upyr, a private native English-Ukrainian keyboard-layout fixer. Generated from CHANGELOG.md."
    >
    <meta name="robots" content="index,follow">
    <meta property="og:type" content="website">
    <meta property="og:title" content="Upyr — Releases">
    <meta property="og:description" content="Release history for Upyr, generated from CHANGELOG.md.">
    <meta property="og:url" content="https://upyr.org/releases.html">
    <title>Upyr — Releases</title>
    <link rel="icon" href="favicon.svg" type="image/svg+xml">
    <link rel="stylesheet" href="styles.css">
  </head>
  <body>
    <a class="skip-link" href="#main">Skip to content</a>

    <header class="site-header">
      <div class="shell header-inner">
        <a class="brand" href="index.html" aria-label="Upyr home">
          <img src="favicon.svg" width="36" height="36" alt="">
          <span>Upyr</span>
        </a>
        <nav class="nav" aria-label="Main navigation">
          <a href="index.html#why">Why Upyr</a>
          <a href="index.html#how">How it works</a>
          <a href="index.html#privacy">Privacy</a>
          <a href="index.html#platforms">Platforms</a>
          <a href="releases.html" aria-current="page">Releases</a>
        </nav>
        <div class="header-actions">
          <a class="github-link" href="https://github.com/dmytro-yemelianov/upyr" aria-label="Upyr on GitHub">
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M12 .7a11.5 11.5 0 0 0-3.6 22.4c.6.1.8-.2.8-.6v-2.2c-3.3.7-4-1.4-4-1.4-.5-1.4-1.3-1.7-1.3-1.7-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1.1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.8-1.6-2.6-.3-5.4-1.3-5.4-5.7 0-1.3.5-2.3 1.2-3.1-.1-.3-.5-1.5.1-3.1 0 0 1-.3 3.2 1.2a11 11 0 0 1 5.8 0c2.2-1.5 3.2-1.2 3.2-1.2.6 1.6.2 2.8.1 3.1.8.8 1.2 1.9 1.2 3.1 0 4.4-2.8 5.4-5.4 5.7.4.4.8 1.1.8 2.2v3.3c0 .4.2.7.8.6A11.5 11.5 0 0 0 12 .7Z"/>
            </svg>
          </a>
        </div>
      </div>
    </header>

    <main id="main">
      <section class="section releases-page">
        <div class="shell">
          <div class="section-intro">
            <p class="kicker">Release history</p>
            <h1>Releases</h1>
            <p>Generated from <a class="text-link" href="https://github.com/dmytro-yemelianov/upyr/blob/main/CHANGELOG.md">CHANGELOG.md</a>. Packaged binaries are signed, checksummed, and attested by the release workflow; see each release for platform downloads.</p>
          </div>
          <div class="releases-list">
{releases}
          </div>
        </div>
      </section>
    </main>

    <footer class="site-footer">
      <div class="shell footer-inner">
        <div class="footer-brand">
          <img src="favicon.svg" width="32" height="32" alt="">
          <div><strong>Upyr</strong><span>Type what you meant.</span></div>
        </div>
        <div class="footer-links">
          <a href="https://github.com/dmytro-yemelianov/upyr">Source</a>
          <a href="https://github.com/dmytro-yemelianov/upyr/releases">All GitHub releases</a>
          <a href="https://github.com/dmytro-yemelianov/upyr/issues">Issues</a>
          <a href="https://github.com/dmytro-yemelianov/upyr/security">Security</a>
        </div>
        <p class="footer-meta">No cookies · No analytics · No external assets</p>
      </div>
    </footer>
  </body>
</html>
"""


def main() -> int:
    if not CHANGELOG.exists():
        print(f"error: {CHANGELOG} does not exist", file=sys.stderr)
        return 1

    releases = parse_changelog(CHANGELOG.read_text(encoding="utf-8"))
    if not releases:
        print("error: no releases parsed from CHANGELOG.md", file=sys.stderr)
        return 1

    rendered = "\n".join(render_release(release) for release in releases)
    page = PAGE_TEMPLATE.format(releases=rendered)
    OUTPUT.write_text(page, encoding="utf-8")
    print(f"wrote {OUTPUT} with {len(releases)} release entries")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
