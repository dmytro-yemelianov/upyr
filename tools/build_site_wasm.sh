#!/bin/sh
# Builds the browser WASM bundle consumed by the live demo in site/ and
# stages it into site/wasm/. This directory is a build artifact (gitignored,
# like target/ and dist/): the GitHub Pages workflow runs this script fresh
# on every deploy, and a contributor runs it locally to preview the demo.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
OUT_DIR="$ROOT/site/wasm"

fail() {
    echo "error: $*" >&2
    exit 1
}

command -v wasm-bindgen >/dev/null 2>&1 || fail "wasm-bindgen CLI was not found; install with: cargo install wasm-bindgen-cli --version 0.2.126 --locked"

cd "$ROOT"
cargo build --release -p upyr-wasm --target wasm32-unknown-unknown --locked

rm -rf "$OUT_DIR"
wasm-bindgen --target web --out-dir "$OUT_DIR" --out-name upyr_wasm \
    target/wasm32-unknown-unknown/release/upyr_wasm.wasm

echo "Staged the browser WASM bundle at $OUT_DIR"
