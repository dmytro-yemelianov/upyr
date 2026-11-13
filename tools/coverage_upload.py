#!/usr/bin/env python3
"""Validate source-scoped LCOV and the evidence accepted for external publication."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
PREFIXES = {"upyr": "src/", "upyr-core": "crates/upyr-core/src/",
            "upyr-audio": "crates/upyr-audio/src/", "upyr-wasm": "crates/upyr-wasm/src/"}


def normalize_lcov(text: str, coverage: dict, root: Path = ROOT) -> str:
    """Reject inconsistent evidence; retain crate source records with relative paths."""
    totals = {name: dict(count=0, covered=0) for name in PREFIXES}
    seen, output = set(), []
    blocks = text.split("end_of_record")
    if blocks[-1].strip():
        raise ValueError("unterminated LCOV record")
    for block in blocks[:-1]:
        lines = [line for line in block.strip().splitlines() if not line.startswith("TN:")]
        fields = {}
        for line in lines:
            key, sep, value = line.partition(":")
            if not sep:
                raise ValueError("invalid LCOV field")
            fields.setdefault(key, []).append(value)
        if any(len(fields.get(key, [])) != 1 for key in ("SF", "LF", "LH")):
            raise ValueError("missing or duplicate LCOV source/counters")
        source = Path(fields["SF"][0])
        relative = (root / source).resolve().relative_to(root.resolve()).as_posix()
        crate = next((name for name, prefix in PREFIXES.items() if relative.startswith(prefix)), None)
        if crate is None:
            continue
        if relative in seen or not relative.endswith(".rs"):
            raise ValueError("duplicate or unexpected LCOV source")
        seen.add(relative)
        hits = {}
        for entry in fields.get("DA", []):
            values = entry.split(",")
            if len(values) not in (2, 3):
                raise ValueError("invalid LCOV line data")
            line, count = map(int, values[:2])
            if line <= 0 or count < 0 or line in hits:
                raise ValueError("duplicate or invalid LCOV line")
            hits[line] = count
        count, covered = int(fields["LF"][0]), int(fields["LH"][0])
        # LLVM's summary counters include macro/expansion accounting and need
        # not equal the number of DA records. Compare them with the JSON export
        # below, rather than fabricating or dropping source-line observations.
        if not hits or not 0 <= covered <= count:
            raise ValueError("invalid LCOV counters or missing line data")
        totals[crate]["count"] += count
        totals[crate]["covered"] += covered
        output.extend("SF:" + relative if line.startswith("SF:") else line for line in lines)
        output.append("end_of_record")
    for crate, counts in totals.items():
        if not counts["count"] or any(counts[key] != coverage[crate]["lines"][key]
                                      for key in ("count", "covered")):
            raise ValueError(f"LCOV does not match JSON coverage for {crate}")
    return "\n".join(output) + "\n"


def verify_upload(directory: Path, commit: str) -> None:
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("expected a full source commit")
    report = json.loads((directory / "report.json").read_text())
    if (report["status"] != "passed" or report["source"]["commit"] != commit
            or report["source"]["dirty"] is not False or report["host"]["system"] != "Linux"):
        raise ValueError("only a clean successful Linux audit of the expected commit may be uploaded")
    data = (directory / "coverage.lcov").read_bytes()
    if hashlib.sha256(data).hexdigest() != report["evidence_sha256"]["coverage.lcov"]:
        raise ValueError("LCOV evidence checksum mismatch")
    text = data.decode("utf-8")
    if normalize_lcov(text, report["metrics"]["coverage"]) != text:
        raise ValueError("LCOV is not normalized")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    verify_upload(args.directory, args.commit)
    print("Coverage source identity, checksum, paths and counters verified.")
