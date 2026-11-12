#!/bin/sh
set -eu
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
mode=${1:-incremental}
case "$mode" in root|incremental) ;; *) echo "usage: $0 [root|incremental] [output executable]" >&2; exit 2 ;; esac
bench_dir=$(mktemp -d "${TMPDIR:-/tmp}/upyr-prefix.XXXXXX")
trap 'rm -rf "$bench_dir"' EXIT HUP INT TERM
cargo build --manifest-path "$repo/Cargo.toml" --release --locked --offline -p upyr-core
set -- "$mode" "${2:-}"
if [ "$mode" = incremental ]; then
    rustc --edition=2024 -O --cfg prefix_incremental "$repo/tools/benchmark_prefix.rs" \
        --extern "upyr_core=$repo/target/release/libupyr_core.rlib" \
        -L "dependency=$repo/target/release/deps" -o "$bench_dir/benchmark"
else
    rustc --edition=2024 -O "$repo/tools/benchmark_prefix.rs" \
        --extern "upyr_core=$repo/target/release/libupyr_core.rlib" \
        -L "dependency=$repo/target/release/deps" -o "$bench_dir/benchmark"
fi
if [ -n "$2" ]; then
    cp "$bench_dir/benchmark" "$2"
else
    "$bench_dir/benchmark" "$repo/.cache/upyr-benchmarks/clean-wikipedia-v1.tsv"
fi
