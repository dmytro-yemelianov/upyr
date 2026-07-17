#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
PACKAGE_ID=$(cargo pkgid --quiet --manifest-path "$ROOT/Cargo.toml" -p upyr)
VERSION=${PACKAGE_ID##*@}

if [ -z "$VERSION" ] || [ "$VERSION" = "$PACKAGE_ID" ]; then
    echo "error: could not read the Upyr version from Cargo metadata" >&2
    exit 1
fi

if [ -n "${UPYR_VERSION:-}" ] && [ "$UPYR_VERSION" != "$VERSION" ]; then
    echo "error: UPYR_VERSION '$UPYR_VERSION' does not match Cargo version '$VERSION'" >&2
    exit 1
fi

printf '%s\n' "$VERSION"
