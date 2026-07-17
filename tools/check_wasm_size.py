#!/usr/bin/env python3
"""Report and enforce the first measured Upyr WASM size baseline."""

from __future__ import annotations

import argparse
import gzip
import shutil
import subprocess
from pathlib import Path


RAW_LIMIT = 3_750_000
BROTLI_LIMIT = 512_000


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "artifact",
        nargs="?",
        type=Path,
        default=Path("target/upyr-wasm-node/upyr_wasm_bg.wasm"),
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    artifact = args.artifact
    data = artifact.read_bytes()
    raw_size = len(data)
    gzip_size = len(gzip.compress(data, compresslevel=9, mtime=0))

    brotli = shutil.which("brotli")
    if brotli is None:
        raise SystemExit("brotli is required to measure the release artifact")
    result = subprocess.run(
        [brotli, "-q", "11", "-c", str(artifact)],
        check=True,
        capture_output=True,
    )
    brotli_size = len(result.stdout)

    print(f"artifact={artifact}")
    print(f"raw_bytes={raw_size} limit={RAW_LIMIT}")
    print(f"gzip_9_bytes={gzip_size}")
    print(f"brotli_11_bytes={brotli_size} limit={BROTLI_LIMIT}")

    failures = []
    if raw_size > RAW_LIMIT:
        failures.append(f"raw artifact exceeds its limit by {raw_size - RAW_LIMIT} bytes")
    if brotli_size > BROTLI_LIMIT:
        failures.append(
            f"Brotli artifact exceeds its limit by {brotli_size - BROTLI_LIMIT} bytes"
        )
    if failures:
        raise SystemExit("; ".join(failures))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
