#!/usr/bin/env python3
"""Escape raw control characters emitted in codebase-memory JSON properties.

Run after indexing with persistence enabled. Valid properties are left intact;
malformed JSON other than unescaped control characters is rejected.
"""

from __future__ import annotations

import json
import sqlite3
import subprocess
import tempfile
from contextlib import closing
from pathlib import Path

if __package__:
    from .check_codebase_graph import parse_args, validate_project
else:
    from check_codebase_graph import parse_args, validate_project


def normalize_properties(connection: sqlite3.Connection) -> int:
    repaired = 0
    for table in ("nodes", "edges"):
        for row_id, properties in connection.execute(f"SELECT id, properties FROM {table}").fetchall():
            try:
                json.loads(properties)
            except json.JSONDecodeError:
                value = json.loads(properties, strict=False)
                portable = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
                connection.execute(f"UPDATE {table} SET properties = ? WHERE id = ?", (portable, row_id))
                repaired += 1
    return repaired


def main() -> None:
    args = parse_args()
    metadata = json.loads(args.metadata.read_text(encoding="utf-8"))
    with tempfile.TemporaryDirectory(prefix="upyr-graph-normalize-") as directory:
        database = Path(directory) / "graph.db"
        with database.open("wb") as output:
            subprocess.run(["zstd", "-dc", str(args.artifact)], stdout=output, check=True)
        with closing(sqlite3.connect(database)) as connection:
            connection.execute("PRAGMA journal_mode = DELETE")
            validate_project(metadata, connection)
            with connection:
                repaired = normalize_properties(connection)
        compressed = Path(directory) / "graph.db.zst"
        subprocess.run(["zstd", "-3", "--quiet", str(database), "-o", str(compressed)], check=True)
        metadata["original_size"] = database.stat().st_size
        metadata["compressed_size"] = compressed.stat().st_size
        metadata["compression_level"] = 3
        args.artifact.write_bytes(compressed.read_bytes())
        args.metadata.write_text(json.dumps(metadata, indent=4) + "\n", encoding="utf-8")
    print(f"code graph normalized: {repaired} JSON properties repaired")


if __name__ == "__main__":
    main()
