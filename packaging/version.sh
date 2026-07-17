#!/bin/sh
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
PACKAGE_ID=$(cargo pkgid --quiet --manifest-path "$ROOT/Cargo.toml" -p upyr)

# Cargo has emitted both `name@version` and legacy `name:version` package-ID
# suffixes. Accept either representation so packaging does not depend on the
# runner's Cargo formatting, then validate that the extracted value is SemVer.
case "$PACKAGE_ID" in
    *@*) VERSION=${PACKAGE_ID##*@} ;;
    *#*)
        VERSION=${PACKAGE_ID##*#}
        VERSION=${VERSION##*:}
        ;;
    *) VERSION= ;;
esac

if ! printf '%s\n' "$VERSION" \
    | LC_ALL=C grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'; then
    echo "error: could not read the Upyr version from Cargo package ID '$PACKAGE_ID'" >&2
    exit 1
fi

if [ -n "${UPYR_VERSION:-}" ] && [ "$UPYR_VERSION" != "$VERSION" ]; then
    echo "error: UPYR_VERSION '$UPYR_VERSION' does not match Cargo version '$VERSION'" >&2
    exit 1
fi

printf '%s\n' "$VERSION"
