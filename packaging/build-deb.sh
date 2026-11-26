#!/bin/sh
# Build the nimo-control-center .deb (driver source for DKMS + the app).
#
#   packaging/build-deb.sh [path/to/nimo-control-center binary] [output dir]
#
# Defaults: app/target/release/nimo-control-center and dist/.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
APP=${1:-$ROOT/app/target/release/nimo-control-center}
OUT=${2:-$ROOT/dist}
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/app/Cargo.toml" | head -1)
DRV_VERSION=$(sed -n 's/^PACKAGE_VERSION="\(.*\)"/\1/p' "$ROOT/driver/dkms.conf")
[ -x "$APP" ] || { echo "No app binary at $APP; build it first (app/build-release.sh)." >&2; exit 1; }

PKG=$(mktemp -d)
trap 'rm -rf "$PKG"' EXIT
D=$PKG/root

# The app
install -D -m 755 "$APP" "$D/usr/bin/nimo-control-center"
install -D -m 755 "$ROOT/app/nimo-control-center-launch" "$D/usr/bin/nimo-control-center-launch"
install -D -m 644 "$ROOT/app/nimo-control-center.desktop" "$D/usr/share/applications/nimo-control-center.desktop"
install -D -m 644 "$ROOT/app/ui/icon.svg" "$D/usr/share/icons/hicolor/scalable/apps/nimo-control-center.svg"
mkdir -p "$D/etc/xdg/autostart"
sed 's|^Exec=.*|Exec=nimo-control-center-launch --tray|; s|^Name=.*|Name=NIMO Control Center (tray)|' \
	"$ROOT/app/nimo-control-center.desktop" > "$D/etc/xdg/autostart/nimo-control-center.desktop"
chmod 644 "$D/etc/xdg/autostart/nimo-control-center.desktop"

# The driver source, built by DKMS on the user's machine
for f in nimo-wmi.c wmcc-fixed.h Makefile dkms.conf; do
	install -D -m 644 "$ROOT/driver/$f" "$D/usr/src/nimo-wmi-$DRV_VERSION/$f"
done
install -D -m 755 "$ROOT/driver/udev/permissions.sh" "$D/usr/lib/nimo-wmi/permissions.sh"
mkdir -p "$D/usr/lib/udev/rules.d" "$D/usr/lib/modules-load.d"
sed 's|/usr/local/lib/nimo-wmi/|/usr/lib/nimo-wmi/|' "$ROOT/driver/udev/70-nimo-wmi.rules" \
	> "$D/usr/lib/udev/rules.d/70-nimo-wmi.rules"
echo nimo-wmi > "$D/usr/lib/modules-load.d/nimo-wmi.conf"
chmod 644 "$D/usr/lib/udev/rules.d/70-nimo-wmi.rules" "$D/usr/lib/modules-load.d/nimo-wmi.conf"

# Docs and licences
DOC=$D/usr/share/doc/nimo-control-center
install -D -m 644 "$ROOT/README.md" "$DOC/README.md"
cat > "$DOC/copyright" <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: nimo-axis-linux-fixes
Source: https://github.com/Andycodeman/nimo-axis-linux-fixes

Files: *
Copyright: 2026 Andycodeman and contributors
License: GPL-3.0-or-later
 The app (/usr/bin/nimo-control-center). See /usr/share/common-licenses/GPL-3.

Files: usr/src/nimo-wmi-*
Copyright: 2026 Andycodeman and contributors
License: GPL-2.0-or-later
 The nimo-wmi kernel driver. See /usr/share/common-licenses/GPL-2.
EOF
chmod 644 "$DOC/copyright"
gzip -9n -c "$ROOT/CHANGELOG.md" > "$DOC/changelog.gz"
chmod 644 "$DOC/changelog.gz"
find "$D" -type d -exec chmod 755 {} +

# Package metadata and maintainer scripts
mkdir -p "$D/DEBIAN"
SIZE=$(du -sk --exclude=DEBIAN "$D" | cut -f1)
sed "s/@VERSION@/$VERSION/; s/@SIZE@/$SIZE/" "$ROOT/packaging/debian/control.in" > "$D/DEBIAN/control"
for s in postinst prerm postrm; do
	sed "s/@DRV_VERSION@/$DRV_VERSION/" "$ROOT/packaging/debian/$s" > "$D/DEBIAN/$s"
	chmod 755 "$D/DEBIAN/$s"
done
install -m 644 "$ROOT/packaging/debian/conffiles" "$D/DEBIAN/conffiles"

mkdir -p "$OUT"
DEB=$OUT/nimo-control-center_${VERSION}_amd64.deb
dpkg-deb --root-owner-group -Zxz --build "$D" "$DEB" >/dev/null
echo "$DEB"
