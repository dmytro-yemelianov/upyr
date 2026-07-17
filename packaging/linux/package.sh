#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
VERSION=$(sh "$ROOT/packaging/version.sh")
ARCH=${UPYR_DEB_ARCH:-$(dpkg --print-architecture)}
DIST="$ROOT/dist"
PORTABLE="$DIST/upyr-linux-$ARCH-$VERSION"
DEB_ROOT="$DIST/deb-root"

rm -rf "$PORTABLE" "$DEB_ROOT"
mkdir -p "$PORTABLE" "$DEB_ROOT/DEBIAN" "$DEB_ROOT/usr/bin" \
    "$DEB_ROOT/usr/lib/upyr" "$DEB_ROOT/usr/share/applications" \
    "$DEB_ROOT/usr/share/icons/hicolor/scalable/apps" "$DEB_ROOT/usr/share/doc/upyr"

cp "$ROOT/target/release/upyr" "$PORTABLE/upyr"
cp "$ROOT/target/release/upyr-background" "$PORTABLE/upyr-background"
cp "$ROOT/target/release/upyr-settings" "$PORTABLE/upyr-settings"
cp "$ROOT/LICENSE" "$ROOT/README.md" "$ROOT/THIRD_PARTY_NOTICES.md" "$PORTABLE/"
chmod 755 "$PORTABLE/upyr" "$PORTABLE/upyr-background" "$PORTABLE/upyr-settings"

cp "$ROOT/target/release/upyr" "$DEB_ROOT/usr/bin/upyr"
cp "$ROOT/target/release/upyr-background" "$DEB_ROOT/usr/lib/upyr/upyr-background"
cp "$ROOT/target/release/upyr-settings" "$DEB_ROOT/usr/lib/upyr/upyr-settings"
cp "$ROOT/packaging/linux/upyr.desktop" "$DEB_ROOT/usr/share/applications/dev.Upyr.Upyr.desktop"
cp "$ROOT/packaging/linux/upyr.svg" "$DEB_ROOT/usr/share/icons/hicolor/scalable/apps/upyr.svg"
cp "$ROOT/LICENSE" "$ROOT/README.md" "$ROOT/THIRD_PARTY_NOTICES.md" "$DEB_ROOT/usr/share/doc/upyr/"
chmod 755 "$DEB_ROOT/usr/bin/upyr" "$DEB_ROOT/usr/lib/upyr/upyr-background" \
    "$DEB_ROOT/usr/lib/upyr/upyr-settings"

cat > "$DEB_ROOT/DEBIAN/control" <<EOF
Package: upyr
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Maintainer: Upyr contributors
Homepage: https://dmytro-yemelianov.github.io/upyr/
Depends: libgtk-3-0, libayatana-appindicator3-1, libx11-6, libgl1
Description: Private English-Ukrainian keyboard layout fixer
 Upyr fixes selected text or the previous word and follows the correction
 with the matching installed X11 keyboard layout.
EOF

tar -C "$DIST" -czf "$DIST/upyr-linux-$ARCH-$VERSION.tar.gz" "$(basename "$PORTABLE")"
dpkg-deb --root-owner-group --build "$DEB_ROOT" "$DIST/upyr-linux-$ARCH-$VERSION.deb"
dpkg-deb --info "$DIST/upyr-linux-$ARCH-$VERSION.deb" >/dev/null
test "$(dpkg-deb --field "$DIST/upyr-linux-$ARCH-$VERSION.deb" Version)" = "$VERSION"
