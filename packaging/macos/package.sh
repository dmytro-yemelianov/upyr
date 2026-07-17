#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
VERSION=$(sh "$ROOT/packaging/version.sh")
DIST="$ROOT/dist"
APP="$DIST/Upyr.app"
CONTENTS="$APP/Contents"
SETTINGS_APP="$CONTENTS/Helpers/Upyr Settings.app"
SETTINGS_CONTENTS="$SETTINGS_APP/Contents"
ZIP="$DIST/upyr-macos-universal-$VERSION.zip"
DMG="$DIST/upyr-macos-universal-$VERSION.dmg"
DMG_ROOT="$DIST/.upyr-dmg-root"
NOTARY_ZIP="$DIST/.upyr-notary-upload.zip"
NOTARIZE=false

cleanup() {
    rm -rf "$DMG_ROOT"
    rm -f "$NOTARY_ZIP"
}
trap cleanup EXIT HUP INT TERM

fail() {
    echo "error: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "required command '$1' was not found"
}

verify_universal_binary() {
    binary=$1
    test -x "$binary" || fail "missing executable: $binary"
    lipo "$binary" -verify_arch arm64 x86_64 >/dev/null \
        || fail "$binary is not universal for arm64 and x86_64"
    archs=$(lipo -archs "$binary")
    case "$archs" in
        "arm64 x86_64"|"x86_64 arm64") ;;
        *) fail "$binary has unexpected architectures: $archs" ;;
    esac
}

verify_code_signature() {
    codesign --verify --strict --verbose=2 "$1"
}

sign_code() {
    path=$1
    identifier=$2
    if [ -n "${UPYR_MACOS_SIGNING_IDENTITY:-}" ]; then
        codesign --force --options runtime --timestamp \
            --sign "$UPYR_MACOS_SIGNING_IDENTITY" "$path"
    else
        requirement="=designated => identifier \"$identifier\""
        codesign --force --sign - --identifier "$identifier" \
            --requirements "$requirement" "$path"
    fi
}

submit_for_notarization() {
    xcrun notarytool submit "$1" \
        --apple-id "$APPLE_ID" \
        --password "$APPLE_APP_PASSWORD" \
        --team-id "$APPLE_TEAM_ID" \
        --wait
}

verify_dmg_layout() {
    mount_point=$(mktemp -d "${TMPDIR:-/tmp}/upyr-dmg.XXXXXX")
    status=0
    hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mount_point" "$DMG" \
        || status=$?
    if [ "$status" -eq 0 ]; then
        test -d "$mount_point/Upyr.app" || status=1
        test -L "$mount_point/Applications" || status=1
        test "$(readlink "$mount_point/Applications" 2>/dev/null || true)" = "/Applications" \
            || status=1
        hdiutil detach -quiet "$mount_point" || status=1
    fi
    rmdir "$mount_point" 2>/dev/null || true
    test "$status" -eq 0 || fail "DMG layout verification failed"
}

require_command codesign
require_command ditto
require_command hdiutil
require_command lipo
require_command plutil

# Notarization credentials are deliberately all-or-nothing. Release-tag CI
# additionally requires all of them before it starts building.
notary_fields=0
for value in "${APPLE_ID:-}" "${APPLE_APP_PASSWORD:-}" "${APPLE_TEAM_ID:-}"; do
    if [ -n "$value" ]; then
        notary_fields=$((notary_fields + 1))
    fi
done
if [ "$notary_fields" -ne 0 ]; then
    [ "$notary_fields" -eq 3 ] \
        || fail "APPLE_ID, APPLE_APP_PASSWORD, and APPLE_TEAM_ID must be provided together"
    [ -n "${UPYR_MACOS_SIGNING_IDENTITY:-}" ] \
        || fail "notarization requires UPYR_MACOS_SIGNING_IDENTITY"
    require_command spctl
    require_command xcrun
    NOTARIZE=true
fi

verify_universal_binary "$ROOT/target/release/upyr-background"
verify_universal_binary "$ROOT/target/release/upyr"
verify_universal_binary "$ROOT/target/release/upyr-settings"

rm -rf "$APP" "$DMG_ROOT"
rm -f "$DMG" "$ZIP" "$NOTARY_ZIP"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources" \
    "$SETTINGS_CONTENTS/MacOS" "$SETTINGS_CONTENTS/Resources"
cp "$ROOT/target/release/upyr-background" "$CONTENTS/MacOS/upyr-background"
cp "$ROOT/target/release/upyr" "$CONTENTS/MacOS/upyr"
cp "$ROOT/target/release/upyr-settings" "$SETTINGS_CONTENTS/MacOS/upyr-settings"
cp "$ROOT/LICENSE" "$ROOT/README.md" "$ROOT/THIRD_PARTY_NOTICES.md" "$CONTENTS/Resources/"
test -s "$ROOT/packaging/macos/Upyr.icns" || fail "missing macOS application icon"
cp "$ROOT/packaging/macos/Upyr.icns" "$CONTENTS/Resources/Upyr.icns"
cp "$CONTENTS/Resources/Upyr.icns" "$SETTINGS_CONTENTS/Resources/Upyr.icns"
sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/macos/Info.plist" > "$CONTENTS/Info.plist"
sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/macos/Settings-Info.plist" \
    > "$SETTINGS_CONTENTS/Info.plist"
chmod 755 "$CONTENTS/MacOS/upyr-background" "$CONTENTS/MacOS/upyr" \
    "$SETTINGS_CONTENTS/MacOS/upyr-settings"

plutil -lint "$CONTENTS/Info.plist"
plutil -lint "$SETTINGS_CONTENTS/Info.plist"
test "$(plutil -extract CFBundleIconFile raw "$CONTENTS/Info.plist")" = "Upyr.icns"
test "$(plutil -extract CFBundleIconFile raw "$SETTINGS_CONTENTS/Info.plist")" = "Upyr.icns"
test "$(plutil -extract CFBundleIdentifier raw "$CONTENTS/Info.plist")" = "dev.Upyr.Upyr"
test "$(plutil -extract CFBundleIdentifier raw "$SETTINGS_CONTENTS/Info.plist")" = "dev.Upyr.Upyr.Settings"
test "$(plutil -extract CFBundleShortVersionString raw "$CONTENTS/Info.plist")" = "$VERSION"
test "$(plutil -extract CFBundleVersion raw "$CONTENTS/Info.plist")" = "$VERSION"
test "$(plutil -extract CFBundleShortVersionString raw "$SETTINGS_CONTENTS/Info.plist")" = "$VERSION"
test "$(plutil -extract CFBundleVersion raw "$SETTINGS_CONTENTS/Info.plist")" = "$VERSION"

# Sign every nested code object from the leaves outward. Do not use --deep for
# signing: it can mask missing or incorrectly ordered signatures.
sign_code "$CONTENTS/MacOS/upyr" "dev.Upyr.Upyr.CLI"
sign_code "$SETTINGS_CONTENTS/MacOS/upyr-settings" "dev.Upyr.Upyr.Settings.Executable"
sign_code "$SETTINGS_APP" "dev.Upyr.Upyr.Settings"
sign_code "$CONTENTS/MacOS/upyr-background" "dev.Upyr.Upyr.Executable"
sign_code "$APP" "dev.Upyr.Upyr"

verify_code_signature "$CONTENTS/MacOS/upyr"
verify_code_signature "$CONTENTS/MacOS/upyr-background"
verify_code_signature "$SETTINGS_CONTENTS/MacOS/upyr-settings"
verify_code_signature "$SETTINGS_APP"
verify_code_signature "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
verify_universal_binary "$CONTENTS/MacOS/upyr-background"
verify_universal_binary "$CONTENTS/MacOS/upyr"
verify_universal_binary "$SETTINGS_CONTENTS/MacOS/upyr-settings"

if [ "$NOTARIZE" = true ]; then
    ditto -c -k --sequesterRsrc --keepParent "$APP" "$NOTARY_ZIP"
    submit_for_notarization "$NOTARY_ZIP"
    xcrun stapler staple "$APP"
    xcrun stapler validate "$APP"
    verify_code_signature "$APP"
fi

# Build final archives only after the app has its notarization ticket, so both
# the ZIP and DMG carry the exact stapled bundle users will install.
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
mkdir -p "$DMG_ROOT"
ditto "$APP" "$DMG_ROOT/Upyr.app"
ln -s /Applications "$DMG_ROOT/Applications"
hdiutil create -quiet -fs HFS+ -format UDZO -volname "Upyr $VERSION" \
    -srcfolder "$DMG_ROOT" "$DMG"
hdiutil verify "$DMG" >/dev/null
verify_dmg_layout

if [ -n "${UPYR_MACOS_SIGNING_IDENTITY:-}" ]; then
    codesign --force --timestamp --sign "$UPYR_MACOS_SIGNING_IDENTITY" "$DMG"
    codesign --verify --verbose=2 "$DMG"
fi

if [ "$NOTARIZE" = true ]; then
    submit_for_notarization "$DMG"
    xcrun stapler staple "$DMG"
    xcrun stapler validate "$DMG"
    hdiutil verify "$DMG" >/dev/null
    spctl --assess --type execute --verbose=4 "$APP"
    spctl --assess --type open --context context:primary-signature --verbose=4 "$DMG"
fi

echo "Created $ZIP"
echo "Created $DMG"
