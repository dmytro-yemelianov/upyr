import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from tools import coverage_upload as coverage


class CoverageUploadTests(unittest.TestCase):
    def fixture(self):
        metrics = {name: {"lines": {"count": 2, "covered": 1}} for name in coverage.PREFIXES}
        text = "".join(f"SF:{prefix}lib.rs\nDA:1,3\nDA:2,0\nLF:2\nLH:1\nend_of_record\n"
                       for prefix in coverage.PREFIXES.values())
        return text, metrics

    def test_absolute_paths_become_relative_and_test_files_are_excluded(self):
        text, metrics = self.fixture()
        text = text.replace("SF:", f"SF:{coverage.ROOT}/")
        text += "SF:tests/integration.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n"
        normalized = coverage.normalize_lcov(text, metrics)
        self.assertEqual(normalized, self.fixture()[0])
        self.assertNotIn(str(coverage.ROOT), normalized)

    def test_missing_crate_duplicate_source_and_path_escape_fail(self):
        text, metrics = self.fixture()
        invalid = [text.split("end_of_record\n", 1)[1], text + text,
                   text.replace("src/lib.rs", "../src/lib.rs", 1), text[:-14]]
        for value in invalid:
            with self.subTest(value=value[:60]), self.assertRaises(ValueError):
                coverage.normalize_lcov(value, metrics)

    def test_counter_and_line_tampering_fail(self):
        text, metrics = self.fixture()
        for old, new in [("LF:2", "LF:3"), ("LH:1", "LH:2"), ("DA:2,0", "DA:1,0"),
                         ("DA:1,3", "DA:1,-1"), ("LF:2", "LF:2\nLF:2"),
                         ("DA:1,3", "DA:0,3")]:
            with self.subTest(old=old, new=new), self.assertRaises(ValueError):
                coverage.normalize_lcov(text.replace(old, new, 1), metrics)

    def test_json_totals_must_match(self):
        text, metrics = self.fixture()
        metrics["upyr-core"]["lines"]["covered"] = 2
        with self.assertRaises(ValueError):
            coverage.normalize_lcov(text, metrics)

    def test_llvm_expansion_summary_can_differ_from_da_records(self):
        text, metrics = self.fixture()
        # Observed in the real LLVM export: macro accounting adds summary lines
        # without adding corresponding DA records. Preserve the observations.
        text = text.replace("LF:2\nLH:1", "LF:3\nLH:2", 1)
        metrics["upyr"]["lines"] = {"count": 3, "covered": 2}
        self.assertEqual(coverage.normalize_lcov(text, metrics), text)

    def test_upload_requires_identity_clean_success_linux_and_intact_evidence(self):
        text, metrics = self.fixture()
        commit = "a" * 40
        report = {"status": "passed", "source": {"commit": commit, "dirty": False},
                  "host": {"system": "Linux"}, "metrics": {"coverage": metrics},
                  "evidence_sha256": {"coverage.lcov": hashlib.sha256(text.encode()).hexdigest()}}
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / "coverage.lcov").write_text(text)
            def save():
                (root / "report.json").write_text(json.dumps(report))
            save()
            coverage.verify_upload(root, commit)
            for expected in ("b" * 40, "main", "abc"):
                with self.assertRaises(ValueError):
                    coverage.verify_upload(root, expected)
            for target, key, value in [(report, "status", "failed"),
                                       (report["source"], "dirty", True),
                                       (report["host"], "system", "Darwin")]:
                original = target[key]
                target[key] = value
                save()
                with self.assertRaises(ValueError):
                    coverage.verify_upload(root, commit)
                target[key] = original
            save()
            (root / "coverage.lcov").write_text(text + "\n")
            with self.assertRaises(ValueError):
                coverage.verify_upload(root, commit)


if __name__ == "__main__":
    unittest.main()
