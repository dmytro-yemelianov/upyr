#!/usr/bin/env python3
"""Run portable, offline engine correctness and allocation/memory budgets.

No desktop access or timing gate. Preserve JSON and logs on every failure.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
from pathlib import Path
import platform
import subprocess

ROOT = Path(__file__).resolve().parents[1]
BUDGETS = Path("crates/upyr-core/fixtures/engine-budgets-v1.tsv")


def load_budgets(path: Path) -> dict[tuple[str, str], int]:
    result = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        name, relation, raw_limit = line.split("\t")
        limit = int(raw_limit)
        key = (name, relation)
        if not name or relation not in {"min", "max"} or limit < 0 or key in result:
            raise ValueError(f"Invalid or duplicate budget: {line}")
        result[key] = limit
    if not result:
        raise ValueError("No engine budgets defined")
    return result


def validate(output: str, budgets: dict[tuple[str, str], int]) -> tuple[list[dict], list[str]]:
    checks = []
    failures = []
    seen = set()
    for line in output.splitlines():
        if not line.strip():
            continue
        try:
            row = json.loads(line)
            if not isinstance(row, dict):
                raise ValueError("expected an object")
            key = (row["check"], row["relation"])
            if key not in budgets or key in seen:
                raise ValueError("unknown or duplicate check")
            if type(row["actual"]) is not int or row["actual"] < 0:
                raise ValueError("actual must be a nonnegative integer")
            if type(row["limit"]) is not int or row["limit"] != budgets[key]:
                raise ValueError("reported limit differs from the checked-in budget")
            passed = row["actual"] <= row["limit"] if key[1] == "max" else row["actual"] >= row["limit"]
            if type(row["passed"]) is not bool or row["passed"] != passed:
                raise ValueError("inconsistent result")
            seen.add(key)
            checks.append(row)
            if not passed:
                failures.append(f"{key[0]}: {row['actual']} violates {key[1]} {row['limit']}")
        except (ValueError, TypeError, KeyError) as error:
            failures.append(f"Invalid engine report line: {error}")
    for name, relation in sorted(budgets.keys() - seen):
        failures.append(f"Missing check: {name}/{relation}")
    return checks, failures


def fingerprint() -> str:
    core = ROOT / "crates/upyr-core"
    paths = [ROOT / "Cargo.lock", core / "Cargo.toml", core / "build.rs", ROOT / "tools/run_engine_contract.py"]
    for directory in ["src", "assets", "fixtures", "examples"]:
        paths.extend(p for p in (core / directory).rglob("*") if p.is_file())
    digest = hashlib.sha256()
    for path in sorted(set(paths)):
        if path.is_file():
            digest.update(path.relative_to(ROOT).as_posix().encode() + b"\0")
            digest.update(path.read_bytes() + b"\0")
    return digest.hexdigest()


def decoded(value: str | bytes | None) -> str:
    return value.decode("utf-8", errors="replace") if isinstance(value, bytes) else value or ""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/engine-contract-report.json")
    parser.add_argument("--timeout", type=int, default=600)
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    command = ["cargo", "run", "--quiet", "-p", "upyr-core", "--example", "engine_contract",
               "--features", "engine-bench", "--release", "--locked", "--offline"]
    stdout = stderr = ""
    checks = []
    failures = []
    budgets = {}
    returncode = 0
    try:
        budgets = load_budgets(ROOT / BUDGETS)
        result = subprocess.run(command, cwd=ROOT, capture_output=True, encoding="utf-8",
                                errors="replace", timeout=args.timeout)
        stdout, stderr, returncode = result.stdout, result.stderr, result.returncode
        if returncode:
            failures.append(f"Cargo exited with status {returncode}")
    except subprocess.TimeoutExpired as error:
        stdout, stderr = decoded(error.stdout), decoded(error.stderr)
        failures.append(f"Cargo timed out after {args.timeout} seconds")
        returncode = 124
    except OSError as error:
        failures.append(str(error))
        returncode = 127
    except ValueError as error:
        failures.append(str(error))
        returncode = 1
    if budgets:
        checks, validation_failures = validate(stdout, budgets)
        failures.extend(validation_failures)
    if failures and not returncode:
        returncode = 1
    try:
        source_fingerprint = fingerprint()
    except OSError as error:
        source_fingerprint = None
        failures.append(f"Could not fingerprint engine sources: {error}")
        returncode = returncode or 1
    report = dict(schema_version=1, status="passed" if returncode == 0 else "failed",
                  completed_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  platform=platform.system(), machine=platform.machine(), returncode=returncode,
                  failures=failures, command=command, checks=checks, expected_checks=len(budgets),
                  source_fingerprint=source_fingerprint, no_gui_input=True, timing_gate=False)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    args.output.with_suffix(".log").write_text(stdout + stderr + "\n" + "\n".join(failures), encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return returncode


if __name__ == "__main__":
    raise SystemExit(main())
