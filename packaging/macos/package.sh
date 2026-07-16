#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
VERSION=${UPYR_VERSION:-$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")}
DIST="$ROOT/dist"
APP="$DIST/Upyr.app"
CONTENTS="$APP/Contents"
SETTINGS_APP="$CONTENTS/Helpers/Upyr Settings.app"
SETTINGS_CONTENTS="$SETTINGS_APP/Contents"

rm -rf "$APP"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources" \
    "$SETTINGS_CONTENTS/MacOS" "$SETTINGS_CONTENTS/Resources"
cp "$ROOT/target/release/upyr-background" "$CONTENTS/MacOS/upyr-background"
cp "$ROOT/target/release/upyr" "$CONTENTS/MacOS/upyr"
cp "$ROOT/target/release/upyr-settings" "$SETTINGS_CONTENTS/MacOS/upyr-settings"
cp "$ROOT/LICENSE" "$ROOT/README.md" "$ROOT/THIRD_PARTY_NOTICES.md" "$CONTENTS/Resources/"
sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/macos/Info.plist" > "$CONTENTS/Info.plist"
sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/macos/Settings-Info.plist" > "$SETTINGS_CONTENTS/Info.plist"
chmod 755 "$CONTENTS/MacOS/upyr-background" "$CONTENTS/MacOS/upyr" \
    "$SETTINGS_CONTENTS/MacOS/upyr-settings"

if [ -n "${UPYR_MACOS_SIGNING_IDENTITY:-}" ]; then
    codesign --force --deep --options runtime --timestamp \
        --sign "$UPYR_MACOS_SIGNING_IDENTITY" "$APP"
else
    # An ad-hoc signature makes local/test bundles structurally complete. Release
    # CI uses a Developer ID identity when the repository secrets are configured.
    codesign --force --deep --sign - "$APP"
fi

rm -f "$DIST/upyr-macos-universal-$VERSION.dmg" "$DIST/upyr-macos-universal-$VERSION.zip"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$DIST/upyr-macos-universal-$VERSION.zip"
hdiutil create -quiet -fs HFS+ -format UDZO -volname "Upyr $VERSION" \
    -srcfolder "$APP" "$DIST/upyr-macos-universal-$VERSION.dmg"

codesign --verify --deep --strict "$APP"
plutil -lint "$CONTENTS/Info.plist"
plutil -lint "$SETTINGS_CONTENTS/Info.plist"
