#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
SOURCE=${1:-"$ROOT/packaging/macos/UpyrIcon.svg"}
OUTPUT=${2:-"$ROOT/packaging/macos/Upyr.icns"}
WORK=$(mktemp -d "${TMPDIR:-/tmp}/upyr-icon.XXXXXX")
ICONSET="$WORK/Upyr.iconset"

cleanup() {
    rm -rf "$WORK"
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$ICONSET"

render() {
    name=$1
    size=$2
    sips -s format png -z "$size" "$size" "$SOURCE" \
        --out "$ICONSET/$name" >/dev/null
}

render icon_16x16.png 16
render icon_16x16@2x.png 32
render icon_32x32.png 32
render icon_32x32@2x.png 64
render icon_128x128.png 128
render icon_128x128@2x.png 256
render icon_256x256.png 256
render icon_256x256@2x.png 512
render icon_512x512.png 512
render icon_512x512@2x.png 1024

mkdir -p "$(dirname -- "$OUTPUT")"
iconutil -c icns "$ICONSET" -o "$OUTPUT"
test -s "$OUTPUT"
