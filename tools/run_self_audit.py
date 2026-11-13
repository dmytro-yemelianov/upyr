#!/usr/bin/env python3
"""Measure Upyr's engineering checks; preserve evidence, including on failure.

Requires cargo-llvm-cov, llvm-tools-preview, cargo-audit and cargo-deny.
Uses synthetic tests only; never drives desktop input or loads user config.
"""
from __future__ import annotations

import argparse
import collections
import datetime
import hashlib
import html
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
CORE_LINE_FLOOR = 90.0
CRATES = {"upyr": "src/", "upyr-core": "crates/upyr-core/src/",
          "upyr-audio": "crates/upyr-audio/src/", "upyr-wasm": "crates/upyr-wasm/src/"}
MAPPING_WORKLOADS = {"mapping_" + name for name in
                     ("one_key", "short", "word", "context", "reverse", "cold", "churn", "tiny", "large")}
LIMITATIONS = [
    "Engineering self-assessment, not independent certification or a security guarantee.",
    "Coverage: native-host workspace test build, default features; inline unit tests are included. "
    "Other OS branches, actual WASM execution, doctests, live GUI/input and branch/MC/DC coverage are not measured here.",
    "RustSec scans the lockfile against its database at run time; maintenance/yanked warnings remain visible. "
    "OSV, CodeQL, Semgrep and Trivy run separately and are not implied by this result.",
    "License/source policy checks dependency metadata, not legal compliance of corpora, rules, sounds or fonts.",
    "Latency is a synthetic mapping benchmark on this host, not end-to-end typing latency; no timing gate.",
    "No formal proof, continuous fuzzing campaign, mutation score or independent penetration test is claimed.",
]


def dump(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def capture(command: list[str]) -> str:
    return subprocess.check_output(command, cwd=ROOT, text=True, timeout=60).strip()


def identity() -> dict:
    names = capture(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"])
    hasher = hashlib.sha256()
    for name in sorted(set(names.split("\0")) - {""}):
        path = ROOT / name
        hasher.update(name.encode() + b"\0")
        hasher.update(path.read_bytes() if path.is_file() else b"<missing>")
        hasher.update(b"\0")
    return dict(commit=capture(["git", "rev-parse", "HEAD"]),
                dirty=bool(capture(["git", "status", "--porcelain"])),
                tree_sha256=hasher.hexdigest(), lockfile_sha256=digest(ROOT / "Cargo.lock"))


def coverage_metrics(data: dict, root: Path = ROOT) -> dict:
    """Group only crate source files; reject missing, duplicated or invalid data."""
    totals = {name: {kind: dict(count=0, covered=0) for kind in ("lines", "functions")}
              for name in CRATES}
    seen = set()
    if data.get("type") != "llvm.coverage.json.export" or len(data["data"]) != 1:
        raise ValueError("expected one LLVM coverage export")
    for file in data["data"][0]["files"]:
        path = Path(file["filename"]).resolve().relative_to(root.resolve()).as_posix()
        if path in seen:
            raise ValueError(f"duplicate coverage file: {path}")
        seen.add(path)
        crate = next((name for name, prefix in CRATES.items() if path.startswith(prefix)), None)
        if crate is None:
            continue
        for kind in ("lines", "functions"):
            count, covered = (file["summary"][kind][k] for k in ("count", "covered"))
            if type(count) is not int or type(covered) is not int or not 0 <= covered <= count:
                raise ValueError("invalid coverage counters")
            totals[crate][kind]["count"] += count
            totals[crate][kind]["covered"] += covered
    totals["workspace"] = {kind: {key: sum(t[kind][key] for t in totals.values())
                                  for key in ("count", "covered")} for kind in ("lines", "functions")}
    for kinds in totals.values():
        for counts in kinds.values():
            if not counts["count"]:
                raise ValueError("missing crate coverage")
            counts["percent"] = round(100 * counts["covered"] / counts["count"], 4)
    core = totals["upyr-core"]["lines"]
    if 100 * core["covered"] < CORE_LINE_FLOOR * core["count"]:
        raise ValueError(f"core line coverage is below {CORE_LINE_FLOOR}%")
    return totals


def test_counts(log: str) -> dict:
    rows = [tuple(map(int, row)) for row in re.findall(
        r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", log)]
    result = dict(zip(("passed", "failed", "ignored"), map(sum, zip(*rows)))) if rows else {}
    if not result.get("passed") or result["failed"]:
        raise ValueError("missing or failed test results")
    return result


def audit_metrics(data: dict) -> dict:
    vulnerabilities = data["vulnerabilities"]
    if (type(vulnerabilities["count"]) is not int or vulnerabilities["count"] != 0
            or vulnerabilities["found"] is not False or vulnerabilities["list"]):
        raise ValueError("RustSec reports vulnerabilities or inconsistent counters")
    warnings = []
    for category, entries in data["warnings"].items():
        for row in entries:
            warnings.append(dict(category=category, package=row["package"]["name"],
                                 version=row["package"]["version"],
                                 advisory=(row.get("advisory") or {}).get("id")))
    return dict(vulnerabilities=0, warnings=warnings, database=data["database"],
                lockfile=data["lockfile"])


def latency_metrics(output: str) -> list[dict]:
    timings, seen = [], set()
    for line in output.splitlines():
        row = json.loads(line)
        name = row["workload"]
        if name in seen:
            raise ValueError("duplicate benchmark workload")
        seen.add(name)
        if set(row) == {"workload", "mapping_heap_bytes"}:
            continue
        values = [row[k] for k in ("samples", "p50_ns", "p95_ns", "p99_ns", "max_ns")]
        if any(type(v) is not int or v < 0 for v in values) or values[0] < 100:
            raise ValueError("invalid latency counters")
        if values[1:] != sorted(values[1:]):
            raise ValueError("unordered latency percentiles")
        timings.append(row)
    if {row["workload"] for row in timings} != MAPPING_WORKLOADS:
        raise ValueError("incomplete mapping benchmark")
    return timings


def badge(label: str, value: str, passed: bool) -> str:
    left, right = max(65, len(label) * 7 + 16), max(55, len(value) * 7 + 16)
    title = html.escape(label + ": " + value)
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{left + right}" height="22" '
            f'role="img" aria-label="{title}"><title>{title}</title>'
            f'<rect width="{left}" height="22" fill="#444"/>'
            f'<rect x="{left}" width="{right}" height="22" fill="{"#286749" if passed else "#a33"}"/>'
            '<g fill="#fff" text-anchor="middle" font-family="Verdana,sans-serif" font-size="11">'
            f'<text x="{left / 2}" y="15">{html.escape(label)}</text>'
            f'<text x="{left + right / 2}" y="15">{html.escape(value)}</text></g></svg>\n')


def markdown(report: dict) -> str:
    source = report["source"]
    lines = ["# Upyr self-audit", "", f"**{report['status']}** · {report['completed_at']}", "",
             f"Source: `{source['commit']}` · dirty tree: `{source['dirty']}` · "
             f"{report['host']['system']} / {report['host']['machine']}", "",
             "This result applies to the source fingerprint in `report.json`.", "",
             "| Check | Result | Evidence |", "| --- | --- | --- |"]
    for check in report["checks"]:
        lines.append(f"| {check['name']} | {check['status']} | `{check['log']}` |")
        if check.get("error"):
            lines.append("")
            lines.append(f"Failure in {check['name']}: `{check['error'].replace('`', chr(39))}`")
            lines.append("")
    metrics = report["metrics"]
    if "tests" in metrics:
        t = metrics["tests"]
        lines += ["", f"Tests: **{t['passed']} passed**, {t['failed']} failed, {t['ignored']} ignored."]
    if "coverage" in metrics:
        lines += ["", "## Coverage", "", "Native-host default-feature test build; inline tests included.", "",
                  "| Scope | Lines | Functions |", "| --- | --- | --- |"]
        for name, item in metrics["coverage"].items():
            values = [f"{item[k]['percent']:.2f}% ({item[k]['covered']}/{item[k]['count']})" for k in ("lines", "functions")]
            lines.append(f"| {name} | {' | '.join(values)} |")
        lines += ["", f"Core line-coverage gate: ≥ {CORE_LINE_FLOOR}%. Other scopes are reported without a threshold."]
    if "rustsec" in metrics:
        a = metrics["rustsec"]
        lines += ["", "## Dependency audit", "", f"Known vulnerabilities: **{a['vulnerabilities']}**. "
                  f"Maintenance/yanked warnings: **{len(a['warnings'])}**."]
        for w in a["warnings"]:
            lines.append(f"- {w['package']} {w['version']}: {w['category']} ({w['advisory'] or 'no advisory ID'})")
    if "dependencies" in metrics:
        d = metrics["dependencies"]
        lines += ["", f"Locked dependency inventory: {d['packages']} packages; "
                  f"{d['duplicate_names']} names with multiple versions. Full license expressions: `metadata.json`."]
    if "engine" in metrics:
        lines += ["", "## Engine budgets", "", "| Check | Measured | Budget |", "| --- | ---: | ---: |"]
        for c in metrics["engine"]["checks"]:
            lines.append(f"| {c['check']} | {c['actual']} | {c['relation']} {c['limit']} |")
    if "latency" in metrics:
        lines += ["", "## Observed mapping latency", "", "Synthetic calls; no desktop input. Host-dependent, not a timing gate.", "",
                  "| Workload | Samples | p50 ns | p95 ns | p99 ns |", "| --- | ---: | ---: | ---: | ---: |"]
        for r in metrics["latency"]:
            lines.append(f"| {r['workload']} | {r['samples']} | {r['p50_ns']} | {r['p95_ns']} | {r['p99_ns']} |")
    lines += ["", "## Scope and limitations", ""] + [f"- {line}" for line in LIMITATIONS]
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/self-audit")
    parser.add_argument("--timeout", type=int, default=1800, help="Timeout per command in seconds")
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    out = args.output.resolve()
    if out.exists() and any(out.iterdir()):
        parser.error("output directory must be empty: never reuse stale audit evidence")
    out.mkdir(parents=True, exist_ok=True)
    report = dict(schema_version=1, status="running", source=identity(),
                  host=dict(system=platform.system(), release=platform.release(), machine=platform.machine(),
                            processor=platform.processor(), logical_cpus=os.cpu_count()),
                  run_url=(f"{os.environ.get('GITHUB_SERVER_URL', 'https://github.com')}/"
                           f"{os.environ['GITHUB_REPOSITORY']}/actions/runs/{os.environ['GITHUB_RUN_ID']}"
                           if os.environ.get("GITHUB_RUN_ID") else None),
                  tools={}, checks=[], metrics={}, limitations=LIMITATIONS,
                  started_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  completed_at=None, no_gui_input=True, timing_gate=False)

    def execute(name: str, command: list[str], *, raw: str | None = None, env: dict | None = None,
                validate=None) -> None:
        check = dict(name=name, command=command, status="failed", log=name + ".log")
        start = time.monotonic()
        stdout = stderr = ""
        try:
            result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
                                    errors="replace", timeout=args.timeout, env={**os.environ, **(env or {})})
            stdout, stderr = result.stdout, result.stderr
            check["returncode"] = result.returncode
            if raw:
                (out / raw).write_text(stdout, encoding="utf-8")
            if result.returncode:
                raise ValueError(f"command exited with status {result.returncode}")
            if validate:
                validate(stdout, stderr)
            check["status"] = "passed"
        except subprocess.TimeoutExpired as error:
            stdout = error.stdout.decode("utf-8", errors="replace") if isinstance(error.stdout, bytes) else error.stdout or ""
            stderr = error.stderr.decode("utf-8", errors="replace") if isinstance(error.stderr, bytes) else error.stderr or ""
            check.update(returncode=124, error=f"timed out after {args.timeout}s")
        except (OSError, ValueError, KeyError, TypeError) as error:
            check["error"] = str(error)
        check["seconds"] = round(time.monotonic() - start, 3)
        (out / check["log"]).write_text(stdout + stderr + "\n" + check.get("error", ""), encoding="utf-8")
        report["checks"].append(check)
        dump(out / "report.json", report)
        print(f"{name}: {check['status']} ({check['seconds']}s)", flush=True)

    for command in (["rustc", "--version"], ["cargo", "llvm-cov", "--version"],
                    ["cargo", "audit", "--version"], ["cargo", "deny", "--version"]):
        execute("version-" + command[-2], command,
                validate=lambda stdout, stderr, name=command[-2]: report["tools"].update({name: stdout.strip()}))

    def metadata(stdout: str, stderr: str) -> None:
        packages = json.loads(stdout)["packages"]
        if not packages:
            raise ValueError("empty dependency inventory")
        counts = collections.Counter(p["name"] for p in packages)
        report["metrics"]["dependencies"] = dict(packages=len(packages),
            duplicate_names=sum(n > 1 for n in counts.values()),
            license_expressions=dict(collections.Counter(p.get("license") or "UNKNOWN" for p in packages)))

    execute("dependencies", ["cargo", "metadata", "--format-version", "1", "--locked"], raw="metadata.json", validate=metadata)
    execute("licenses-sources", ["cargo", "deny", "--locked", "--format", "json", "check", "licenses", "sources"])
    execute("rustsec", ["cargo", "audit", "--json"], raw="rustsec.json",
            validate=lambda stdout, stderr: report["metrics"].update(rustsec=audit_metrics(json.loads(stdout))))
    execute("formatting", ["cargo", "fmt", "--all", "--", "--check"])
    execute("clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--all-features", "--locked", "--", "-D", "warnings"])
    execute("rustdoc", ["cargo", "doc", "--workspace", "--no-deps", "--locked"], env={"RUSTDOCFLAGS": "-D warnings"})
    execute("privacy", [sys.executable, "tools/check_privacy.py"])
    execute("report-tests", [sys.executable, "-m", "unittest", "tools.test_run_self_audit", "tools.test_run_engine_contract",
                            "tools.test_check_privacy"])

    def covered(stdout: str, stderr: str) -> None:
        data = json.loads((out / "coverage.json").read_text(encoding="utf-8"))
        report["metrics"]["coverage"] = coverage_metrics(data)
        report["metrics"]["tests"] = test_counts(stdout + stderr)

    execute("coverage", ["cargo", "llvm-cov", "--workspace", "--all-targets", "--locked", "--json",
                         "--summary-only", "--output-path", str(out / "coverage.json")], validate=covered)
    execute("coverage-html", ["cargo", "llvm-cov", "report", "--html", "--output-dir", str(out / "coverage-html")])

    def engine(stdout: str, stderr: str) -> None:
        data = json.loads((out / "engine.json").read_text(encoding="utf-8"))
        if (data["status"] != "passed" or data["failures"] or not data["checks"]
                or len(data["checks"]) != data["expected_checks"] or not all(c["passed"] for c in data["checks"])):
            raise ValueError("incomplete or failed engine contract")
        report["metrics"]["engine"] = data

    execute("engine", [sys.executable, "tools/run_engine_contract.py", "--output", str(out / "engine.json")], validate=engine)
    execute("latency", ["cargo", "run", "--quiet", "--release", "--locked", "--example", "boundary_benchmark",
                        "--features", "engine-bench", "--", "mapping"], raw="latency.jsonl",
            validate=lambda stdout, stderr: report["metrics"].update(latency=latency_metrics(stdout)))
    report["status"] = "passed" if all(c["status"] == "passed" for c in report["checks"]) else "failed"
    report["completed_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    if identity() != report["source"]:
        report["status"] = "failed"
        report["checks"].append(dict(name="source-stability", status="failed", log="report.json",
                                     error="source changed during measurement"))
    passed = report["status"] == "passed"
    (out / "self-audit.svg").write_text(badge("self-audit", report["status"], passed), encoding="utf-8")
    cov = report["metrics"].get("coverage", {}).get("upyr-core", {}).get("lines", {}).get("percent")
    (out / "core-coverage.svg").write_text(badge("core lines", f"{cov:.2f}%" if cov is not None else "unavailable", cov is not None and passed), encoding="utf-8")
    (out / "report.md").write_text(markdown(report), encoding="utf-8")
    report["evidence_sha256"] = {p.relative_to(out).as_posix(): digest(p) for p in sorted(out.rglob("*"))
                                 if p.is_file() and p.name != "report.json"}
    dump(out / "report.json", report)
    print(f"Self-audit {report['status']}: {out / 'report.md'}", flush=True)
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
