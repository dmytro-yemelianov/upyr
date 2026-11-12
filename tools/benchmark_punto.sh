#!/bin/sh
# Build with the same generator as Cargo, without the application dependencies.
set -eu
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
bench_dir=$(mktemp -d "${TMPDIR:-/tmp}/upyr-punto.XXXXXX")
trap 'rm -rf "$bench_dir"' EXIT HUP INT TERM
rustc --edition=2024 -O "$repo/crates/upyr-core/build.rs" -o "$bench_dir/generate"
OUT_DIR="$bench_dir" "$bench_dir/generate" > /dev/null
OUT_DIR="$bench_dir" rustc --edition=2024 -O "$repo/tools/benchmark_punto.rs" -o "$bench_dir/benchmark"
if [ "$#" -gt 0 ]; then
    cp "$bench_dir/benchmark" "$1"
else
    "$bench_dir/benchmark"
fi
