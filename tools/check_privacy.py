#!/usr/bin/env python3
"""Fail closed when Upyr gains network, telemetry, or web tracking paths.

This is intentionally a small, auditable guard rather than a claim that static
analysis can prove a negative. It checks the dependency lock, native/WASM source,
logging fields, and the published static site. Human review and GitHub security
scanning remain complementary controls.
"""

from __future__ import annotations

import re
import sys
from html.parser import HTMLParser
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RUNTIME_ROOTS = (
    ROOT / "src",
    ROOT / "crates" / "upyr-audio" / "src",
    ROOT / "crates" / "upyr-core" / "src",
    ROOT / "crates" / "upyr-wasm" / "src",
)

NETWORK_CRATES = {
    "awc",
    "curl",
    "ehttp",
    "hyper",
    "hyper-util",
    "isahc",
    "reqwest",
    "surf",
    "ureq",
}
TELEMETRY_CRATES = {
    "datadog",
    "newrelic",
    "opentelemetry",
    "posthog",
    "rollbar",
    "sentry",
    "segment",
}
SOURCE_PATTERNS = {
    "native socket API": re.compile(r"\b(?:std::net|TcpStream|UdpSocket|ToSocketAddrs)\b"),
    "HTTP client API": re.compile(r"\b(?:reqwest|ureq|hyper|isahc|ehttp|curl)::"),
    "telemetry SDK API": re.compile(r"\b(?:sentry|opentelemetry|posthog|segment|datadog|newrelic)::"),
}
WEB_CODE_PATTERNS = {
    "fetch": re.compile(r"\bfetch\s*\("),
    "XMLHttpRequest": re.compile(r"\bXMLHttpRequest\b"),
    "sendBeacon": re.compile(r"\bsendBeacon\s*\("),
    "WebSocket": re.compile(r"\bWebSocket\s*\("),
    "EventSource": re.compile(r"\bEventSource\s*\("),
    "analytics loader": re.compile(
        r"\b(?:gtag|posthog\.init|mixpanel\.init|analytics\.track)\s*\(", re.IGNORECASE
    ),
    "dynamic resource element": re.compile(
        r"\bcreateElement\s*\(\s*['\"](?:script|img|iframe|link|audio|video|source|track|embed|object)['\"]",
        re.IGNORECASE,
    ),
}
EXTERNAL_CSS_RESOURCE = re.compile(
    r"(?:@import\s+(?:url\()?\s*['\"]?(?:https?:)?//|url\(\s*['\"]?(?:https?:)?//)",
    re.IGNORECASE,
)
LOG_MACRO = re.compile(r"\b(?:trace|debug|info|warn|error)!\s*\(")
SENSITIVE_LOG_FIELD = re.compile(
    r"(?<![A-Za-z0-9_])(?:text|word|source|replacement|prefix|token|clipboard|key|keycode)(?![A-Za-z0-9_])",
    re.IGNORECASE,
)
STRING_LITERAL = re.compile(r'"(?:\\.|[^"\\])*"')
REQUIRED_CSP = {
    "default-src": ("'self'",),
    "connect-src": ("'none'",),
    "object-src": ("'none'",),
    "base-uri": ("'none'",),
    "form-action": ("'none'",),
}


class SiteParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.external_resources: list[tuple[str, str, str]] = []
        self.csp: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = {key.lower(): value or "" for key, value in attrs}
        if tag == "meta" and values.get("http-equiv", "").lower() == "content-security-policy":
            self.csp.append(values.get("content", ""))

        resource: str | None = None
        if tag in {"script", "img", "iframe", "audio", "video", "source", "track", "embed"}:
            resource = values.get("src")
        elif tag == "object":
            resource = values.get("data")
        elif tag == "link" and values.get("rel", "").lower() not in {"canonical", "alternate"}:
            resource = values.get("href")

        if resource and (resource.startswith(("http://", "https://", "//"))):
            self.external_resources.append((tag, "src/href", resource))

        poster = values.get("poster")
        if poster and poster.startswith(("http://", "https://", "//")):
            self.external_resources.append((tag, "poster", poster))

        for candidate in values.get("srcset", "").split(","):
            srcset_resource = candidate.strip().split(" ", 1)[0]
            if srcset_resource.startswith(("http://", "https://", "//")):
                self.external_resources.append((tag, "srcset", srcset_resource))

        style = values.get("style", "")
        if EXTERNAL_CSS_RESOURCE.search(style):
            self.external_resources.append((tag, "style", style))


def rust_files() -> list[Path]:
    return sorted(path for root in RUNTIME_ROOTS for path in root.rglob("*.rs"))


def package_names() -> set[str]:
    lock = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
    return set(re.findall(r'^name = "([^"]+)"$', lock, flags=re.MULTILINE))


def sensitive_log_field(body: str) -> str | None:
    """Return a typed-data field referenced by a log macro body, if any."""
    match = SENSITIVE_LOG_FIELD.search(STRING_LITERAL.sub("", body))
    return match.group(0) if match else None


def check_dependencies(errors: list[str]) -> None:
    forbidden = package_names() & (NETWORK_CRATES | TELEMETRY_CRATES)
    if forbidden:
        errors.append("forbidden network/telemetry crates in Cargo.lock: " + ", ".join(sorted(forbidden)))


def check_runtime_source(errors: list[str]) -> None:
    for path in rust_files():
        source = path.read_text(encoding="utf-8")
        relative = path.relative_to(ROOT)
        for label, pattern in SOURCE_PATTERNS.items():
            for match in pattern.finditer(source):
                line = source.count("\n", 0, match.start()) + 1
                errors.append(f"{relative}:{line}: {label}: {match.group(0)}")

        lines = source.splitlines()
        index = 0
        while index < len(lines):
            if not LOG_MACRO.search(lines[index]):
                index += 1
                continue
            end = index
            depth = 0
            while end < min(index + 12, len(lines)):
                depth += lines[end].count("(") - lines[end].count(")")
                if depth <= 0 and ");" in lines[end]:
                    break
                end += 1
            body = "\n".join(lines[index : end + 1])
            field = sensitive_log_field(body)
            if field:
                errors.append(
                    f"{relative}:{index + 1}: logging a potentially typed-data field `{field}`"
                )
            index = end + 1


def validate_csp(policy: str, relative: Path) -> list[str]:
    """Require exact source lists for privacy-critical CSP directives."""
    errors: list[str] = []
    directives: dict[str, tuple[str, ...]] = {}
    duplicates: set[str] = set()

    for raw_directive in policy.split(";"):
        parts = raw_directive.split()
        if not parts:
            continue
        name = parts[0].lower()
        if name in directives:
            duplicates.add(name)
            continue
        directives[name] = tuple(parts[1:])

    for name in sorted(duplicates):
        errors.append(f"{relative}: CSP contains duplicate `{name}` directives")

    for name, required_sources in REQUIRED_CSP.items():
        actual_sources = directives.get(name)
        expected = f"{name} {' '.join(required_sources)}"
        if actual_sources is None:
            errors.append(f"{relative}: CSP is missing `{expected}`")
        elif actual_sources != required_sources:
            actual = " ".join(actual_sources) or "<empty>"
            errors.append(
                f"{relative}: CSP `{name}` must be exactly `{' '.join(required_sources)}`; "
                f"found `{actual}`"
            )

    return errors


def check_site(errors: list[str]) -> None:
    site = ROOT / "site"
    html_files = sorted(site.rglob("*.html")) if site.exists() else []
    if not html_files:
        errors.append("site contains no HTML pages")
        return

    for path in html_files:
        source = path.read_text(encoding="utf-8")
        relative = path.relative_to(ROOT)
        parser = SiteParser()
        parser.feed(source)
        for tag, attribute, value in parser.external_resources:
            errors.append(f"{relative}: external {tag} {attribute} loads {value}")
        if len(parser.csp) != 1:
            errors.append(f"{relative}: expected exactly one Content-Security-Policy meta tag")
        else:
            errors.extend(validate_csp(parser.csp[0], relative))
        for label, pattern in WEB_CODE_PATTERNS.items():
            match = pattern.search(source)
            if match:
                line = source.count("\n", 0, match.start()) + 1
                errors.append(f"{relative}:{line}: {label} is forbidden on the static site")

    for path in sorted(site.rglob("*.js")):
        source = path.read_text(encoding="utf-8")
        relative = path.relative_to(ROOT)
        for label, pattern in WEB_CODE_PATTERNS.items():
            match = pattern.search(source)
            if match:
                line = source.count("\n", 0, match.start()) + 1
                errors.append(f"{relative}:{line}: {label} is forbidden on the static site")

    for path in sorted(site.rglob("*.css")):
        source = path.read_text(encoding="utf-8")
        relative = path.relative_to(ROOT)
        for match in EXTERNAL_CSS_RESOURCE.finditer(source):
            line = source.count("\n", 0, match.start()) + 1
            errors.append(f"{relative}:{line}: external CSS resource is forbidden")


def main() -> int:
    errors: list[str] = []
    check_dependencies(errors)
    check_runtime_source(errors)
    check_site(errors)
    if errors:
        print("privacy verification failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1
    print(
        f"privacy verification passed: {len(rust_files())} runtime Rust files, "
        f"{len(package_names())} locked packages, and {len(list((ROOT / 'site').rglob('*.html')))} site pages"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
