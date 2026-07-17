#!/usr/bin/env python3
"""Validate the persisted codebase-memory graph artifact."""

from __future__ import annotations

import argparse
import json
import shutil
import sqlite3
import subprocess
import tempfile
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--metadata",
        type=Path,
        default=Path(".codebase-memory/artifact.json"),
    )
    parser.add_argument(
        "--artifact",
        type=Path,
        default=Path(".codebase-memory/graph.db.zst"),
    )
    return parser.parse_args()


def count_rows(database: Path, table: str) -> int:
    with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as connection:
        row = connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()
    if row is None:
        raise RuntimeError(f"missing count result for {table}")
    return int(row[0])


def validate_json_properties(connection: sqlite3.Connection, table: str) -> None:
    for row_id, properties in connection.execute(f"SELECT id, properties FROM {table}"):
        try:
            json.loads(properties)
        except (TypeError, json.JSONDecodeError) as error:
            raise SystemExit(
                f"{table} row {row_id} contains non-portable JSON properties: {error}"
            ) from error


def main() -> None:
    args = parse_args()
    if shutil.which("zstd") is None:
        raise SystemExit("zstd is required to validate the code graph")

    metadata = json.loads(args.metadata.read_text(encoding="utf-8"))
    expected_nodes = int(metadata.get("nodes", 0))
    expected_edges = int(metadata.get("edges", 0))
    if expected_nodes <= 0 or expected_edges <= 0:
        raise SystemExit("persisted code graph metadata must be non-empty")

    compressed_size = args.artifact.stat().st_size
    if compressed_size != int(metadata.get("compressed_size", -1)):
        raise SystemExit(
            "compressed artifact size does not match artifact.json: "
            f"{compressed_size} != {metadata.get('compressed_size')}"
        )

    subprocess.run(["zstd", "--test", "--quiet", str(args.artifact)], check=True)
    with tempfile.TemporaryDirectory(prefix="upyr-code-graph-") as directory:
        database = Path(directory) / "graph.db"
        with database.open("wb") as output:
            subprocess.run(
                ["zstd", "--decompress", "--quiet", "--stdout", str(args.artifact)],
                check=True,
                stdout=output,
            )

        original_size = database.stat().st_size
        if original_size != int(metadata.get("original_size", -1)):
            raise SystemExit(
                "decompressed artifact size does not match artifact.json: "
                f"{original_size} != {metadata.get('original_size')}"
            )

        with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as connection:
            validate_json_properties(connection, "nodes")
            validate_json_properties(connection, "edges")
            integrity = [row[0] for row in connection.execute("PRAGMA integrity_check")]
        if integrity != ["ok"]:
            raise SystemExit(f"persisted code graph failed SQLite integrity check: {integrity}")

        actual_nodes = count_rows(database, "nodes")
        actual_edges = count_rows(database, "edges")
        if (actual_nodes, actual_edges) != (expected_nodes, expected_edges):
            raise SystemExit(
                "persisted graph counts do not match artifact.json: "
                f"{actual_nodes}/{actual_edges} != {expected_nodes}/{expected_edges}"
            )

    print(
        "code graph OK: "
        f"{expected_nodes} nodes, {expected_edges} edges, {compressed_size} compressed bytes"
    )


if __name__ == "__main__":
    main()
