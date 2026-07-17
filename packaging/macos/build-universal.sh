#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}

fail() {
    echo "error: $*" >&2
    exit 1
}

case "$MACOSX_DEPLOYMENT_TARGET" in
    11|11.0) ;;
    *) fail "MACOSX_DEPLOYMENT_TARGET must be 11 or 11.0" ;;
esac

cd "$ROOT"
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked -p upyr --target "$target"
done

mkdir -p "$ROOT/target/release"
for binary in upyr upyr-background upyr-settings; do
    output="$ROOT/target/release/$binary"
    lipo -create \
        "$ROOT/target/aarch64-apple-darwin/release/$binary" \
        "$ROOT/target/x86_64-apple-darwin/release/$binary" \
        -output "$output"
    lipo "$output" -verify_arch arm64 x86_64
    archs=$(lipo -archs "$output")
    case "$archs" in
        "arm64 x86_64"|"x86_64 arm64") ;;
        *) fail "$binary has unexpected architectures: $archs" ;;
    esac

    deployment_count=$(vtool -show-build "$output" \
        | awk '/^[[:space:]]+(minos|version) 11\.0$/ { count++ } END { print count + 0 }')
    [ "$deployment_count" -eq 2 ] \
        || fail "$binary does not target macOS 11.0 for both architectures"
done

echo "Built universal macOS binaries with deployment target $MACOSX_DEPLOYMENT_TARGET"
