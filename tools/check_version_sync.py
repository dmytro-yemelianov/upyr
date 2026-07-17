#!/usr/bin/env python3
"""Verify that every user-visible release version matches Cargo metadata."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKSPACE_PACKAGES = ("upyr", "upyr-core", "upyr-wasm")


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def main() -> int:
    errors: list[str] = []
    manifest = read("Cargo.toml")
    workspace_package = re.search(
        r"(?ms)^\[workspace\.package\]\s*(.*?)(?=^\[|\Z)", manifest
    )
    version_match = (
        re.search(r'^version\s*=\s*"([^"]+)"', workspace_package.group(1), re.MULTILINE)
        if workspace_package
        else None
    )
    if not version_match:
        print("version verification failed: workspace.package.version is missing", file=sys.stderr)
        return 1
    version = version_match.group(1)

    for path in ("crates/upyr-core/Cargo.toml", "crates/upyr-wasm/Cargo.toml"):
        if not re.search(r"(?m)^version\.workspace\s*=\s*true\s*$", read(path)):
            errors.append(f"{path}: package version must inherit workspace.package.version")

    locked: dict[str, str] = {}
    for package in re.findall(r"(?ms)^\[\[package\]\]\s*(.*?)(?=^\[\[package\]\]|\Z)", read("Cargo.lock")):
        name = re.search(r'^name\s*=\s*"([^"]+)"', package, re.MULTILINE)
        package_version = re.search(r'^version\s*=\s*"([^"]+)"', package, re.MULTILINE)
        if name and package_version and name.group(1) in WORKSPACE_PACKAGES:
            locked[name.group(1)] = package_version.group(1)
    for package in WORKSPACE_PACKAGES:
        if locked.get(package) != version:
            errors.append(
                f"Cargo.lock: {package} is {locked.get(package, 'missing')}; expected {version}"
            )

    expected_text = {
        "README.md": f"**Release status:** v{version} public preview.",
        "CHANGELOG.md": f"## [{version}]",
        "site/index.html": f"Public preview · v{version}",
        "site/app.js": f'heroEyebrow: "Публічна попередня версія · v{version}"',
        ".github/ISSUE_TEMPLATE/bug_report.yml": f'placeholder: "{version}"',
        ".github/ISSUE_TEMPLATE/model_report.yml": f'placeholder: "{version} on macOS',
    }
    for path, expected in expected_text.items():
        source = read(path)
        if expected not in source:
            errors.append(f"{path}: missing version marker `{expected}`")

    public_version = re.compile(r"v(\d+\.\d+\.\d+)\s+(?:public preview|on macOS)")
    for path in ("README.md", "site/index.html", "site/app.js"):
        for found in public_version.findall(read(path)):
            if found != version:
                errors.append(f"{path}: public version v{found} does not match v{version}")

    if errors:
        print("version verification failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    print(f"version verification passed: workspace, lockfile, docs, site, and issue forms use {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
