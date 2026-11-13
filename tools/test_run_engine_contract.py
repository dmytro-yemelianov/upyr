"""The gate must fail closed and keep evidence when builds or checks fail."""
import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools import run_engine_contract as runner


class ContractRunnerTests(unittest.TestCase):
    budgets = {("allocations", "max"): 0, ("comparisons", "min"): 10}

    def output(self, allocations=0):
        return "\n".join(json.dumps(row) for row in [
            dict(check="allocations", relation="max", actual=allocations, limit=0, passed=allocations == 0),
            dict(check="comparisons", relation="min", actual=12, limit=10, passed=True),
        ])

    def fixture(self, outcome):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            budget = root / runner.BUDGETS
            budget.parent.mkdir(parents=True)
            budget.write_text("allocations\tmax\t0\ncomparisons\tmin\t10\n", encoding="utf-8")
            output = root / "report.json"
            with patch.object(runner, "ROOT", root), \
                 patch.object(sys, "argv", ["runner", "--output", str(output)]), \
                 patch.object(runner.subprocess, "run", side_effect=[outcome]), \
                 contextlib.redirect_stdout(io.StringIO()):
                code = runner.main()
            return code, json.loads(output.read_text(encoding="utf-8")), output.with_suffix(".log").read_text(encoding="utf-8")

    def test_complete_success(self):
        code, report, _ = self.fixture(subprocess.CompletedProcess([], 0, self.output(), ""))
        self.assertEqual((code, report["status"], len(report["checks"])), (0, "passed", 2))
        self.assertFalse(report["timing_gate"])

    def test_budget_violation_fails_even_if_process_exits_zero(self):
        code, report, log = self.fixture(subprocess.CompletedProcess([], 0, self.output(1), ""))
        self.assertEqual(code, 1)
        self.assertEqual(report["status"], "failed")
        self.assertIn("violates max 0", log)

    def test_missing_duplicate_unknown_and_inconsistent_rows_fail(self):
        rows = self.output().splitlines()
        bad = json.loads(rows[0]); bad["passed"] = False
        changed_limit = json.loads(rows[0]); changed_limit["limit"] = 10
        for output in ["", rows[0], self.output() + "\n" + rows[0], self.output()+"\n{}",
                       json.dumps(bad)+"\n"+rows[1], json.dumps(changed_limit)+"\n"+rows[1], "not JSON"]:
            with self.subTest(output=output):
                _, failures = runner.validate(output, self.budgets)
                self.assertTrue(failures)

    def test_build_failure_preserves_nonzero_status_and_log(self):
        code, report, log = self.fixture(subprocess.CompletedProcess([], 101, "", "compile failure: привіт\n"))
        self.assertEqual((code, report["status"]), (101, "failed"))
        self.assertIn("compile failure: привіт", log)

    def test_timeout_keeps_partial_results(self):
        error = subprocess.TimeoutExpired("cargo", 600, output=self.output().splitlines()[0].encode(), stderr=b"partial error")
        code, report, log = self.fixture(error)
        self.assertEqual((code, len(report["checks"])), (124, 1))
        self.assertIn("partial error", log)
        self.assertIn("Missing check", log)

    def test_missing_cargo_keeps_report(self):
        code, report, log = self.fixture(FileNotFoundError("cargo missing"))
        self.assertEqual((code, report["status"]), (127, "failed"))
        self.assertIn("cargo missing", log)

    def test_invalid_timeout_never_starts_cargo(self):
        with patch.object(sys, "argv", ["runner", "--timeout", "0"]), \
             patch.object(runner.subprocess, "run") as execute, \
             contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            runner.main()
        execute.assert_not_called()


if __name__ == "__main__":
    unittest.main()
