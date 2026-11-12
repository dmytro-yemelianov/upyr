#!/usr/bin/env python3
"""Import the Punto Switcher 1.0 Mac rule tables from an extracted package.

This is deliberately an offline, reproducible importer.  It never downloads or
executes the archive: callers provide the extracted `dictionaries` directory.
The source byte hashes below identify the exact archived inputs that this
repository currently carries.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter
from pathlib import Path
from tempfile import NamedTemporaryFile


DEFAULT_OUTPUT = Path(__file__).resolve().parents[1] / "crates/upyr-core/assets/punto"
PROFILES = {
    "windows-4.4.2.334": {
        "product": "Punto Switcher for Windows 4.4.2.334",
        "archive_sha256": "cd337ea4252118aec42decab6720ced96c324eaa006be21d1bf7bf632d357a76",
        "archive_url": "https://web.archive.org/web/20180101000000id_/https://download.cdn.yandex.net/punto/PuntoSwitcherSetup.exe",
        "archive_note": "Wayback capture dated 2018-04-10; not a claim about the current Punto release.",
        "ps_sha256": "3ce67825518e7f6c48a81feaeb727c1f39c72645563b5b02e28d485c7931081f",
        "triggers_sha256": "e9e5c667482ba63c326a51aa4d048e7ce50e2d6aa303592edc2688f6ee100895",
        "ps_header": "PSVersion=20170301",
        "ps_row_count": 37613,
        "ps_rule_count": 37612,
        "skipped_empty_lines": 1,
        "trigger_count": 4511,
    },
    "mac-1.0": {
        "product": "Punto Switcher for Mac 1.0",
        "archive_sha256": "61faf6572c2be64b225f66418f92e8f74de2d4629599f825d2df583d7175d5bc",
        "archive_url": "https://www.macintoshrepository.org/download.php?id=65794",
        "archive_note": "Archived mirror, not a claim about the current Punto release.",
        "ps_sha256": "7b85336816eaa4e12f5afd39fbd407d4387cb2222bf59b19746017a7a67c1c7b",
        "triggers_sha256": "7b04530b6ceba28b94240e62a127dee7c0dff38310ab5c8a286ee7fb562d1717",
        "ps_header": "PSVersion=28",
        "ps_row_count": 26987,
        "ps_rule_count": 26987,
        "skipped_empty_lines": 0,
        "trigger_count": 2989,
    },
}


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_verified(source_dir: Path, profile: dict[str, str | int]) -> tuple[bytes, bytes]:
    ps = (source_dir / "ps.dat").read_bytes()
    triggers = (source_dir / "triggers.dat").read_bytes()
    for name, actual, expected in (
        ("ps.dat", sha256(ps), profile["ps_sha256"]),
        ("triggers.dat", sha256(triggers), profile["triggers_sha256"]),
    ):
        if actual != expected:
            raise ValueError(f"{name} SHA-256 is {actual}, expected {expected}")
    return ps, triggers


def decode_ps(source: bytes) -> str:
    decoded = bytes(byte if byte in (9, 10, 13) else byte ^ 0xAA for byte in source)
    return decoded.decode("cp1251")


def render(source_dir: Path, profile_name: str = "windows-4.4.2.334") -> dict[Path, bytes]:
    profile = PROFILES[profile_name]
    ps_raw, triggers_raw = read_verified(source_dir, profile)
    ps = decode_ps(ps_raw)
    triggers = triggers_raw.decode("cp1251")
    ps_lines = ps.splitlines()
    trigger_lines = triggers.splitlines()
    if not ps_lines or ps_lines[0] != profile["ps_header"]:
        raise ValueError("unexpected ps.dat version header")
    if len(ps_lines) != profile["ps_row_count"] + 1 or len(trigger_lines) != profile["trigger_count"]:
        raise ValueError("unexpected Punto table record count")

    rule_lines = [line for line in ps_lines[1:] if line]
    if len(rule_lines) != profile["ps_rule_count"]:
        raise ValueError("unexpected Punto non-empty rule count")
    tags = Counter(
        line.split(" ", 1)[0] if line.startswith("_") else "bare"
        for line in rule_lines
    )
    manifest = {
        "format": "upyr-punto-import-v1",
        "source": {
            "profile": profile_name,
            "product": profile["product"],
            "archive": {
                "sha256": profile["archive_sha256"],
                "url": profile["archive_url"],
                "note": profile["archive_note"],
            },
            "files": {
                "ps.dat": {"sha256": profile["ps_sha256"], "encoding": "cp1251", "xor": "0xaa except CR/LF/TAB"},
                "triggers.dat": {"sha256": profile["triggers_sha256"], "encoding": "cp1251"},
            },
        },
        "decoded": {
            "ps_utf8_sha256": sha256(ps.encode("utf-8")),
            "triggers_utf8_sha256": sha256(triggers.encode("utf-8")),
            "ps_header": ps_lines[0],
            "ps_row_count": profile["ps_row_count"],
            "ps_rule_count": profile["ps_rule_count"],
            "skipped_empty_lines": profile["skipped_empty_lines"],
            "trigger_count": profile["trigger_count"],
            "rule_tag_counts": dict(sorted(tags.items())),
            "exception_rule_count": sum(count for tag, count in tags.items() if "E" in tag),
        },
    }
    return {
        Path("ps.txt"): ps.encode("utf-8"),
        Path("triggers.txt"): triggers.encode("utf-8"),
        Path("manifest.json"): (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode("utf-8"),
    }


def atomic_write(path: Path, contents: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with NamedTemporaryFile(dir=path.parent, delete=False) as temporary:
        temporary.write(contents)
        temporary_path = Path(temporary.name)
    temporary_path.replace(path)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, required=True, help="extracted Punto dictionaries directory")
    parser.add_argument("--profile", choices=PROFILES, default="windows-4.4.2.334")
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--check", action="store_true", help="verify generated files without writing")
    args = parser.parse_args()
    if args.profile != "windows-4.4.2.334" and args.output_dir == DEFAULT_OUTPUT:
        parser.error("mac-1.0 requires an explicit --output-dir so it cannot replace the active Windows data")
    outputs = render(args.source_dir, args.profile)
    mismatches = [relative for relative, contents in outputs.items() if (args.output_dir / relative).read_bytes() != contents] if args.check else []
    if args.check:
        if mismatches:
            print("out of date: " + ", ".join(str(path) for path in mismatches))
            return 1
        print("Punto assets are current")
        return 0
    for relative, contents in outputs.items():
        atomic_write(args.output_dir / relative, contents)
    print(f"wrote {len(outputs)} files to {args.output_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
