#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <release-tag>" >&2
    exit 64
fi

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
VERSION=$(sh "$ROOT/packaging/version.sh")
EXPECTED="v$VERSION"

if [ "$1" != "$EXPECTED" ]; then
    echo "release tag '$1' does not match Cargo package version '$VERSION' (expected '$EXPECTED')" >&2
    exit 1
fi

echo "release tag $1 matches Cargo package version $VERSION"
