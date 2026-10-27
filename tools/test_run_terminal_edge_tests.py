"""Report artifacts must survive failing tests and interrupted Cargo runs."""
import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools import run_terminal_edge_tests as runner


class ReportTests(unittest.TestCase):
    def run_fixture(self, outcome, *extra):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / 'Cargo.toml').write_text('[workspace.package]\nversion = "0.4.1"\n')
            report = root / 'artifacts/report.json'
            git = subprocess.CompletedProcess(['git'], 0, 'abc123\n', '')
            with patch.object(runner, 'ROOT', root), \
                 patch.object(sys, 'argv', ['runner', '--output', str(report), *extra]), \
                 patch.object(runner.subprocess, 'run', side_effect=[outcome, git]), \
                 contextlib.redirect_stdout(io.StringIO()):
                code = runner.main()
            return code, json.loads(report.read_text(encoding='utf-8')), report.with_suffix('.log').read_text(encoding='utf-8')

    def test_success_records_coverage_and_source_commit(self):
        output = ('406 curated evaluations\n4032 generated command sequences\n'
                  '180 control/format injection checks\n'
                  'test result: ok. 2 passed; 0 failed; 3 ignored;\n')
        code, report, log = self.run_fixture(subprocess.CompletedProcess([], 0, output, ''))
        self.assertEqual((code, report['status'], report['passed']), (0, 'passed', 2))
        self.assertEqual(report['source_commit'], 'abc123')
        self.assertEqual(report['coverage'], dict(curated_evaluations=406, generated_sequences=4032, control_format_checks=180))
        self.assertEqual(log, output)

    def test_failed_tests_preserve_both_streams_and_nonzero_exit(self):
        output = 'test result: FAILED. 2 passed; 1 failed; 0 ignored;\n'
        code, report, log = self.run_fixture(subprocess.CompletedProcess([], 101, output, 'panic details: привіт ґ\n'))
        self.assertEqual((code, report['status'], report['failed']), (101, 'failed', 1))
        self.assertIn('panic details', log)
        self.assertIn('привіт ґ', log)
        self.assertIn(output, log)

    def test_timeout_keeps_partial_byte_output(self):
        error = subprocess.TimeoutExpired('cargo', 1, output=b'partial build\n', stderr=b'partial error\n')
        code, report, log = self.run_fixture(error, '--timeout', '1')
        self.assertEqual((code, report['returncode']), (124, 124))
        self.assertEqual(report['status'], 'failed')
        self.assertIn('timed out after 1 seconds', report['failure'])
        self.assertIn('partial build', log)
        self.assertIn('partial error', log)

    def test_missing_cargo_still_writes_artifacts(self):
        code, report, log = self.run_fixture(FileNotFoundError('cargo missing'))
        self.assertEqual((code, report['status']), (127, 'failed'))
        self.assertIn('cargo missing', log)

    def test_nonpositive_timeout_is_rejected_before_execution(self):
        with patch.object(sys, 'argv', ['runner', '--timeout', '0']), \
             patch.object(runner.subprocess, 'run') as execute, \
             contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
            runner.main()
        self.assertEqual(error.exception.code, 2)
        execute.assert_not_called()


if __name__ == '__main__':
    unittest.main()
