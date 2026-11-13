"""Audit reports must not turn absent, failed or stale evidence into green badges."""
import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools import run_self_audit as audit


class SelfAuditTests(unittest.TestCase):
    def coverage(self, covered=95):
        return {"type": "llvm.coverage.json.export", "data": [{"files": [
            {"filename": str(audit.ROOT / prefix / "lib.rs"),
             "summary": {kind: dict(count=100, covered=covered) for kind in ("lines", "functions")}}
            for prefix in audit.CRATES.values()]}]}

    def test_coverage_is_weighted_and_scoped_to_crate_sources(self):
        data = self.coverage()
        data["data"][0]["files"].append({"filename": str(audit.ROOT / "tests/a.rs"), "summary": {}})
        result = audit.coverage_metrics(data)
        self.assertEqual(result["workspace"]["lines"], dict(count=400, covered=380, percent=95.0))
        self.assertEqual(result["upyr-core"]["lines"]["percent"], 95)

    def test_low_empty_missing_duplicate_and_invalid_coverage_fail(self):
        cases = [self.coverage(89), self.coverage(101), self.coverage(-1), self.coverage(True), {}]
        missing = self.coverage(); missing["data"][0]["files"].pop(); cases.append(missing)
        empty = self.coverage(); empty["data"][0]["files"] = []; cases.append(empty)
        duplicate = self.coverage(); duplicate["data"][0]["files"] *= 2; cases.append(duplicate)
        for data in cases:
            with self.subTest(data=data), self.assertRaises((ValueError, KeyError)):
                audit.coverage_metrics(data)

    def test_test_totals_keep_ignored_and_reject_failure_or_empty(self):
        self.assertEqual(audit.test_counts("test result: ok. 10 passed; 0 failed; 2 ignored;\n"
                                          "test result: ok. 4 passed; 0 failed; 1 ignored;"),
                         dict(passed=14, failed=0, ignored=3))
        for log in ("", "test result: FAILED. 10 passed; 1 failed; 0 ignored;"):
            with self.assertRaises(ValueError):
                audit.test_counts(log)

    def test_advisory_warnings_remain_visible(self):
        data = dict(vulnerabilities=dict(count=0, found=False, list=[]), database={}, lockfile={},
                    warnings={"yanked": [dict(package=dict(name="old", version="1.0"))]})
        self.assertEqual(audit.audit_metrics(data)["warnings"][0]["category"], "yanked")
        data["vulnerabilities"]["found"] = True
        with self.assertRaises(ValueError):
            audit.audit_metrics(data)

    def test_latency_requires_samples_and_ordered_percentiles(self):
        rows = [dict(workload=name, samples=6000, p50_ns=10, p95_ns=20, p99_ns=30, max_ns=40)
                for name in sorted(audit.MAPPING_WORKLOADS)]
        self.assertEqual(len(audit.latency_metrics("\n".join(map(json.dumps, rows)))), 9)
        for bad in [[], rows[:1], rows + rows[:1], [dict(r, p95_ns=100) for r in rows]]:
            with self.assertRaises(ValueError):
                audit.latency_metrics("\n".join(map(json.dumps, bad)))

    def failed_run(self, result):
        with tempfile.TemporaryDirectory() as scratch:
            out = Path(scratch) / "audit"
            identity = dict(commit="abc", dirty=False, tree_sha256="tree", lockfile_sha256="lock")
            with patch.object(audit, "identity", return_value=identity), \
                    patch.object(sys, "argv", ["audit", "--output", str(out)]), \
                    patch.object(audit.subprocess, "run", side_effect=result), \
                    contextlib.redirect_stdout(io.StringIO()):
                code = audit.main()
            return code, json.loads((out / "report.json").read_text()), (out / "self-audit.svg").read_text()

    def test_command_failure_and_timeout_keep_failed_report_and_badge(self):
        for result in (lambda *a, **kw: subprocess.CompletedProcess([], 101, "", "build failed"),
                       subprocess.TimeoutExpired("cargo", 1, output=b"partial log"),
                       FileNotFoundError("cargo missing")):
            with self.subTest(result=result):
                code, report, svg = self.failed_run(result)
                self.assertEqual((code, report["status"]), (1, "failed"))
                self.assertTrue(all(c["status"] == "failed" for c in report["checks"]))
                self.assertIn("failed", svg)
                self.assertIn("rustsec.log", report["evidence_sha256"])

    def test_zero_exit_without_measurements_does_not_pass(self):
        code, report, _ = self.failed_run(lambda *a, **kw: subprocess.CompletedProcess([], 0, "", ""))
        self.assertEqual((code, report["status"]), (1, "failed"))

    def test_nonempty_output_is_never_reused(self):
        with tempfile.TemporaryDirectory() as scratch:
            (Path(scratch) / "report.json").write_text('{"status":"passed"}')
            with patch.object(sys, "argv", ["audit", "--output", scratch]), \
                    patch.object(audit.subprocess, "run") as run, \
                    contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                audit.main()
            run.assert_not_called()

    def test_badge_escapes_labels(self):
        svg = audit.badge("<&", '"failed"', False)
        self.assertIn("&lt;&amp;", svg)
        self.assertNotIn("<&", svg)

    def test_unignored_in_tree_output_is_rejected_before_measurement(self):
        with patch.object(sys, "argv", ["audit", "--output", str(audit.ROOT / "audit-unignored-output")]), \
                patch.object(audit.subprocess, "run", return_value=subprocess.CompletedProcess([], 1)) as run, \
                patch.object(audit, "identity") as identity, \
                contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            audit.main()
        identity.assert_not_called()
        self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
