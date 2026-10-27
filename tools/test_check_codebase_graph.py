"""Reject structurally valid graph artifacts for an unrelated repository."""

import sqlite3
import unittest
import json

from tools.check_codebase_graph import REQUIRED_FILES, validate_project
from tools.normalize_codebase_graph import normalize_properties


class ProjectIdentityTests(unittest.TestCase):
    def setUp(self):
        self.connection = sqlite3.connect(":memory:")
        self.addCleanup(self.connection.close)
        self.connection.execute("CREATE TABLE nodes (project TEXT, label TEXT, file_path TEXT)")
        self.connection.executemany(
            "INSERT INTO nodes VALUES (?, 'File', ?)",
            [("upyr", path) for path in REQUIRED_FILES],
        )

    def test_accepts_upyr_sources(self):
        validate_project({"project": "upyr"}, self.connection)

    def test_rejects_foreign_metadata(self):
        with self.assertRaisesRegex(SystemExit, "belongs to"):
            validate_project({"project": "another-project"}, self.connection)

    def test_renaming_metadata_cannot_disguise_a_foreign_graph(self):
        self.connection.execute("UPDATE nodes SET project = 'another-project'")
        with self.assertRaisesRegex(SystemExit, "foreign projects"):
            validate_project({"project": "upyr"}, self.connection)

    def test_rejects_an_unrelated_graph_with_the_same_project_label(self):
        self.connection.execute("DELETE FROM nodes")
        self.connection.execute("INSERT INTO nodes VALUES ('upyr', 'File', 'src/unrelated.rs')")
        with self.assertRaisesRegex(SystemExit, "missing Upyr source files"):
            validate_project({"project": "upyr"}, self.connection)


class PropertyNormalizationTests(unittest.TestCase):
    def setUp(self):
        self.connection = sqlite3.connect(":memory:")
        self.addCleanup(self.connection.close)
        for table in ("nodes", "edges"):
            self.connection.execute(f"CREATE TABLE {table} (id INTEGER, properties TEXT)")

    def test_escapes_control_characters_without_changing_the_value(self):
        raw = '{"argument":"перший\nдругий\tрядок"}'
        self.connection.execute("INSERT INTO edges VALUES (1, ?)", (raw,))
        self.assertEqual(normalize_properties(self.connection), 1)
        normalized = self.connection.execute("SELECT properties FROM edges").fetchone()[0]
        self.assertEqual(json.loads(normalized), json.loads(raw, strict=False))
        self.assertEqual(normalize_properties(self.connection), 0)

    def test_rejects_json_damage_beyond_control_character_escaping(self):
        self.connection.execute("INSERT INTO nodes VALUES (1, ?)", ('{"broken":',))
        with self.assertRaises(json.JSONDecodeError):
            normalize_properties(self.connection)


if __name__ == "__main__":
    unittest.main()
