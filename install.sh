#!/bin/sh
# NIMO AXIS 395 (N161L) Linux support: the nimo-wmi driver (DKMS) and NIMO Control Center.
#
#   curl -fsSL https://raw.githubusercontent.com/Andycodeman/nimo-axis-linux-fixes/main/install.sh | sh
#   wget -qO-  https://raw.githubusercontent.com/Andycodeman/nimo-axis-linux-fixes/main/install.sh | sh
#
# or, from a clone of the repo: ./install.sh
#
# Run it as your normal user (not with sudo): it asks for your password when it needs it.
# Undo everything with uninstall.sh.
set -eu

REPO="Andycodeman/nimo-axis-linux-fixes"
BRANCH="${NIMO_BRANCH:-main}"
# Overridable for testing.
SOURCE_URL="${NIMO_SOURCE_URL:-https://github.com/$REPO/archive/refs/heads/$BRANCH.tar.gz}"
APP_URL="${NIMO_APP_URL:-https://github.com/$REPO/releases/latest/download/nimo-control-center-x86_64}"
DEB_URL="${NIMO_DEB_URL:-https://github.com/$REPO/releases/latest/download/nimo-control-center_amd64.deb}"

say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
note() { printf '    %s\n' "$*"; }
die() { printf '\n\033[1;31mError: %s\033[0m\n' "$*" >&2; exit 1; }
# Questions come from the terminal even when the script itself arrives through a pipe.
# /dev/tty can exist and be readable with no terminal behind it (cron, setsid, ssh without -t);
# opening it is the only real test, and a failed "read </dev/tty" would end the script (set -e).
tty_ok() { (: </dev/tty) 2>/dev/null; }
ask() {
	printf '%s ' "$1"
	if tty_ok; then read -r REPLY </dev/tty || REPLY=""; else REPLY=""; echo; fi
}
fetch() { # fetch URL FILE
	if command -v curl >/dev/null; then curl -fsSL "$1" -o "$2"
	elif command -v wget >/dev/null; then wget -qO "$2" "$1"
	else return 1; fi
}

[ "$(id -u)" -ne 0 ] || die "run this as your normal user, not with sudo. It asks for your password when it needs it."
[ "$(uname -m)" = x86_64 ] || die "this is for the NIMO AXIS 395 (x86_64)."
USER_NAME=$(id -un)

product=$(cat /sys/class/dmi/id/product_name 2>/dev/null || echo unknown)
if [ "$product" != "N161L" ]; then
	printf 'This is for the NIMO AXIS 395 (N161L), but this computer is "%s".\n' "$product"
	printf 'The driver will refuse to load on it.\n'
	ask "Continue anyway? [y/N]"
	[ "$REPLY" = y ] || [ "$REPLY" = Y ] || exit 1
fi

# Remove a script-based install from before the .deb existed, so there's only one copy.
remove_script_install() {
	[ -e /usr/local/bin/nimo-control-center ] || [ -e /etc/udev/rules.d/70-nimo-wmi.rules ] || return 0
	note "Removing the earlier script-based install first."
	pkill -x -f '[^ ]*nimo-control-center( --replaced)?( --tray)?' 2>/dev/null || true
	sudo rm -f /usr/local/bin/nimo-control-center /usr/local/bin/nimo-control-center-launch \
		/usr/local/share/applications/nimo-control-center.desktop \
		/usr/local/share/icons/hicolor/scalable/apps/nimo-control-center.svg \
		/etc/xdg/autostart/nimo-control-center.desktop /etc/udev/rules.d/70-nimo-wmi.rules \
		/etc/modules-load.d/nimo-wmi.conf
	sudo rm -rf /usr/local/lib/nimo-wmi
	sudo modprobe -r nimo_wmi 2>/dev/null || true
	for old in $(dkms status nimo-wmi 2>/dev/null | sed -n 's|^nimo-wmi/\([^,:]*\).*|\1|p' | sort -u); do
		sudo dkms remove "nimo-wmi/$old" --all >/dev/null 2>&1 || true
		sudo rm -rf "/usr/src/nimo-wmi-$old"
	done
}

secure_boot_help() {
	lsmod | grep -q '^nimo_wmi' && return 0
	if command -v mokutil >/dev/null && mokutil --sb-state 2>/dev/null | grep -q "SecureBoot enabled"; then
		note "Secure Boot is on, so the driver needs DKMS's signing key enrolled first."
		note "Pick a one-time password now. After a reboot, a blue \"MOK management\" screen appears:"
		note "choose Enroll MOK, then Continue, then Yes, type that password, then Reboot."
		sudo mokutil --import /var/lib/shim-signed/mok/MOK.der </dev/tty || true
		return 1
	fi
	note "The driver didn't load. 'sudo dmesg | grep nimo' says why."
	return 1
}

# TUXEDO's tuxedo-drivers (AUR tuxedo-drivers-dkms, TUXEDO's own .deb) has a module, ite_829x, that
# claims this laptop's keyboard by its USB id (048d:8910). It turns the backlight off and paints every
# key white each time it loads and on every resume, undoing nimo-wmi. Nothing else in it touches the N161L.
ITE_BLOCK=/etc/modprobe.d/nimo-block-ite_829x.conf
ite829x_check() {
	[ ! -e "$ITE_BLOCK" ] || return 0
	[ -d /sys/module/ite_829x ] || modinfo -n ite_829x >/dev/null 2>&1 ||
		dkms status tuxedo-drivers 2>/dev/null | grep -q . || return 0
	say "Found TUXEDO's \"ite_829x\" keyboard driver"
	note "You have TUXEDO's driver pack (tuxedo-drivers) installed. One part of it, ite_829x,"
	note "is meant for TUXEDO laptops, but it also takes over this NIMO's keyboard lighting:"
	note "every time it starts, and after every suspend, it turns the keyboard light off and"
	note "makes every key white, wiping out the lighting NIMO Control Center sets."
	echo
	note "If you block it:"
	note "  - Only ite_829x stops loading. The rest of tuxedo-drivers keeps working."
	note "  - Your keyboard types exactly as before; nothing on this NIMO needs ite_829x."
	note "  - It's one small file: $ITE_BLOCK"
	note "  - To undo it, run the uninstaller (it removes that file), or delete the file"
	note "    and reboot."
	note "If you don't, nothing changes, but your lighting may keep resetting."
	echo
	if ! tty_ok; then
		note "There's no terminal to ask on, so nothing was changed. To block it yourself, put"
		note "these two lines in $ITE_BLOCK and reboot:"
		note "    blacklist ite_829x"
		note "    install ite_829x /bin/false"
		note "The uninstaller only removes the file when the installer wrote it, so to undo"
		note "this one, delete the file and reboot."
		return 0
	fi
	ask "    Block ite_829x? [Y/n]   (Enter = yes)"
	case "$REPLY" in
	n* | N*)
		note "Not blocked; nothing was changed. Running this installer again asks again."
		return 0
		;;
	esac
	printf '%s\n' "# Written by nimo-axis-linux-fixes install.sh; its uninstall.sh removes it." \
		"# TUXEDO's ite_829x claims the NIMO AXIS 395 keyboard (USB 048d:8910) and resets its lighting." \
		"blacklist ite_829x" "install ite_829x /bin/false" | sudo tee "$ITE_BLOCK" >/dev/null
	note "Blocked: ite_829x won't load any more. The rest of tuxedo-drivers is untouched."
	note "This is the only change: $ITE_BLOCK"
	note "To undo it, run the uninstaller, or delete that file and reboot."
	if [ -d /sys/module/ite_829x ]; then
		if sudo modprobe -r ite_829x; then
			note "It was running, so it's unloaded now. NIMO Control Center puts your lighting"
			note "back when it starts."
		else
			note "It's still running and couldn't be stopped now; it won't load after a reboot."
		fi
	fi
}
# On other laptops (after "Continue anyway") ite_829x may be the right driver for their keyboard.
[ "$product" != N161L ] || ite829x_check

# ---- Ubuntu/Debian: install the .deb (unless running from a clone) ----
if command -v apt-get >/dev/null && [ ! -f "$(dirname "$0")/driver/nimo-wmi.c" ] && [ -z "${NIMO_NO_DEB:-}" ]; then
	say "Downloading the NIMO Control Center package"
	WORK=$(mktemp -d)
	trap 'rm -rf "$WORK"' EXIT
	fetch "$DEB_URL" "$WORK/nimo-control-center_amd64.deb" || die "couldn't download $DEB_URL (is curl or wget installed?)"
	remove_script_install
	say "Installing it (apt builds the driver for your kernel; this takes a minute)"
	sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y "$WORK/nimo-control-center_amd64.deb"
	sudo modprobe nimo_wmi 2>/dev/null || true
	say "Done"
	note "Driver and app installed as the package \"nimo-control-center\"."
	note "Remove it in Discover / Software, or with:  sudo apt purge nimo-control-center"
	if secure_boot_help; then
		if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
			(setsid nimo-control-center-launch >/dev/null 2>&1 &) || true
		fi
	else
		note ""
		note "Reboot now (and enroll the key if asked). The driver loads after that."
	fi
	exit 0
fi

# ---- other systems, or a clone of the repo: build and install from source ----
# ---- get the source: this folder if it's a clone, otherwise download it ----
WORK=""
cleanup() { [ -z "$WORK" ] || rm -rf "$WORK"; }
trap cleanup EXIT
if [ -f "$(dirname "$0")/driver/nimo-wmi.c" ]; then
	SRC=$(cd "$(dirname "$0")" && pwd)
else
	say "Downloading nimo-axis-linux-fixes"
	WORK=$(mktemp -d)
	fetch "$SOURCE_URL" "$WORK/src.tar.gz" || die "couldn't download $SOURCE_URL (is curl or wget installed?)"
	mkdir "$WORK/src"
	tar -xzf "$WORK/src.tar.gz" -C "$WORK/src" --strip-components=1
	SRC="$WORK/src"
fi
DRV_VERSION=$(sed -n 's/^PACKAGE_VERSION="\(.*\)"/\1/p' "$SRC/driver/dkms.conf")

say "Installing what the driver needs to build (dkms, kernel headers)"
if command -v apt-get >/dev/null; then
	sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y -q \
		dkms build-essential "linux-headers-$(uname -r)" mokutil
elif command -v pacman >/dev/null; then
	KMOD="/usr/lib/modules/$(uname -r)"
	[ -d "$KMOD" ] || die "kernel $(uname -r) was updated since you booted it. Reboot into the new kernel, then run this again."
	# Arch and CachyOS kernel packages record their name in pkgbase; the headers are <name>-headers.
	kpkg=$(cat "$KMOD/pkgbase" 2>/dev/null || pacman -Qqo "$KMOD/vmlinuz" 2>/dev/null || true)
	pkgs="dkms base-devel fontconfig libxkbcommon libxkbcommon-x11"
	if [ -n "$kpkg" ] && pacman -Qq "$kpkg-headers" >/dev/null 2>&1; then
		: # already installed
	elif [ -n "$kpkg" ] && pacman -Si "$kpkg-headers" >/dev/null 2>&1; then
		pkgs="$pkgs $kpkg-headers"
	else
		note "No headers package found for kernel package \"${kpkg:-unknown}\"; using whatever is in $KMOD/build."
	fi
	# Kernels built with clang (CachyOS) need it to build modules too.
	if grep -q clang /proc/version; then pkgs="$pkgs clang llvm lld"; fi
	# shellcheck disable=SC2086 # $pkgs is a list of package names
	sudo pacman -S --needed --noconfirm $pkgs ||
		die "pacman couldn't install them. Update with 'sudo pacman -Syu', reboot, and run this again."
	[ -d "$KMOD/build" ] || die "there are no headers for kernel $(uname -r) (pacman's are probably for a newer kernel). Update with 'sudo pacman -Syu', reboot, and run this again."
else
	note "Not Ubuntu/Debian or Arch: install dkms, gcc, make and the headers for kernel $(uname -r) yourself."
	command -v dkms >/dev/null || die "dkms isn't installed."
fi

# ---- the app: a ready-built copy if it runs here, otherwise build it ----
say "Getting NIMO Control Center"
APP=""
if [ -x "$SRC/app/target/release/nimo-control-center" ] && [ -z "${NIMO_REBUILD:-}" ]; then
	APP="$SRC/app/target/release/nimo-control-center"
	note "Using the build in $SRC/app."
else
	[ -n "$WORK" ] || WORK=$(mktemp -d)
	if fetch "$APP_URL" "$WORK/nimo-control-center" && chmod +x "$WORK/nimo-control-center" &&
		"$WORK/nimo-control-center" --version >/dev/null 2>&1; then
		APP="$WORK/nimo-control-center"
		note "Downloaded the ready-built app ($("$APP" --version))."
	elif command -v cargo >/dev/null; then
		note "The ready-built app doesn't run on this system; building it with Rust (a few minutes)."
		if command -v apt-get >/dev/null; then
			sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y -q pkg-config libfontconfig-dev libxkbcommon-dev
		elif command -v pacman >/dev/null; then
			sudo pacman -S --needed --noconfirm pkgconf fontconfig libxkbcommon
		fi
		(cd "$SRC/app" && cargo build --release)
		APP="$SRC/app/target/release/nimo-control-center"
	else
		die "the ready-built app doesn't run on this system (it needs glibc 2.39 or newer, as in Ubuntu 24.04), and Rust isn't installed to build it. Install Rust from https://rustup.rs and run this again."
	fi
fi

# ---- driver ----
say "Installing the nimo-wmi $DRV_VERSION driver (DKMS)"
for old in $(dkms status nimo-wmi 2>/dev/null | sed -n 's|^nimo-wmi/\([^,:]*\).*|\1|p' | sort -u); do
	sudo dkms remove "nimo-wmi/$old" --all >/dev/null 2>&1 || true
	sudo rm -rf "/usr/src/nimo-wmi-$old"
done
sudo mkdir -p "/usr/src/nimo-wmi-$DRV_VERSION"
sudo cp "$SRC/driver/nimo-wmi.c" "$SRC/driver/wmcc-fixed.h" "$SRC/driver/Makefile" "$SRC/driver/dkms.conf" \
	"/usr/src/nimo-wmi-$DRV_VERSION/"
sudo dkms install "nimo-wmi/$DRV_VERSION"
echo nimo-wmi | sudo tee /etc/modules-load.d/nimo-wmi.conf >/dev/null

say "Letting the 'nimo' group use the controls without a password"
sudo groupadd -f -r nimo
sudo usermod -aG nimo "$USER_NAME"
sudo install -D -m 755 "$SRC/driver/udev/permissions.sh" /usr/local/lib/nimo-wmi/permissions.sh
sudo install -D -m 644 "$SRC/driver/udev/70-nimo-wmi.rules" /etc/udev/rules.d/70-nimo-wmi.rules
sudo udevadm control --reload

say "Loading the driver"
LOADED=1 # 0 = not loaded, 2 = the old build is still running
srcversion() { sudo modinfo -F srcversion nimo_wmi 2>/dev/null || true; }
# Switch a running driver to the build just installed, but only once it's installed for
# this kernel: after a failed build, unloading would leave no driver at all.
if [ -d /sys/module/nimo_wmi ] &&
	sudo dkms status -m nimo-wmi -v "$DRV_VERSION" -k "$(uname -r)" 2>/dev/null | grep -q ': installed' &&
	[ "$(cat /sys/module/nimo_wmi/srcversion 2>/dev/null || true)" != "$(srcversion)" ]; then
	sudo modprobe -r nimo_wmi 2>/dev/null || true
fi
if ! sudo modprobe nimo_wmi 2>/dev/null; then
	LOADED=0
elif [ "$(cat /sys/module/nimo_wmi/srcversion 2>/dev/null || true)" != "$(srcversion)" ]; then
	LOADED=2
fi
SECURE_BOOT=0
if command -v pacman >/dev/null; then
	# Arch has no MOK setup by default (often sbctl instead), so read the firmware's flag. 2 = on, not MOK.
	sb=$(od -An -tu1 -j4 -N1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c 2>/dev/null | tr -d ' ' || true)
	if [ "$sb" = 1 ]; then SECURE_BOOT=2; fi
elif mokutil --sb-state 2>/dev/null | grep -q "SecureBoot enabled"; then
	SECURE_BOOT=1
fi
if [ "$LOADED" = 0 ] && [ "$SECURE_BOOT" = 1 ]; then
	note "Secure Boot is on, so the driver needs DKMS's signing key enrolled first."
	MOK=/var/lib/shim-signed/mok/MOK.der
	if [ -f "$MOK" ]; then
		note "Pick a one-time password now. After a reboot, a blue \"MOK management\" screen appears:"
		note "choose Enroll MOK, then Continue, then Yes, type that password, then Reboot."
		sudo mokutil --import "$MOK" </dev/tty || true
	fi
elif [ "$LOADED" = 0 ] && [ "$SECURE_BOOT" = 2 ]; then
	note "The driver didn't load, and Secure Boot is on. 'sudo dmesg | grep nimo' says why."
	note "DKMS signed it with its own key, /var/lib/dkms/mok.pub. If you boot through shim, enroll it:"
	note "'sudo mokutil --import /var/lib/dkms/mok.pub', reboot, and choose Enroll MOK."
	note "With sbctl the kernel can't use that key; most Arch kernels load the module anyway."
elif [ "$LOADED" = 0 ]; then
	note "The driver didn't load. 'sudo dmesg | grep nimo' says why."
fi
# Power modes appear in KDE's/GNOME's power menu once power-profiles-daemon sees them.
if systemctl is-active -q power-profiles-daemon 2>/dev/null; then
	sudo systemctl restart power-profiles-daemon
fi

say "Installing NIMO Control Center"
# Match the app's whole command line only, never a shell that merely mentions it.
pkill -u "$(id -u)" -x -f '[^ ]*nimo-control-center( --replaced)?( --tray)?' 2>/dev/null || true
sudo install -D -m 755 "$APP" /usr/local/bin/nimo-control-center
sudo install -D -m 755 "$SRC/app/nimo-control-center-launch" /usr/local/bin/nimo-control-center-launch
sudo install -D -m 644 "$SRC/app/ui/icon.svg" /usr/local/share/icons/hicolor/scalable/apps/nimo-control-center.svg
sudo install -D -m 644 "$SRC/app/nimo-control-center.desktop" /usr/local/share/applications/nimo-control-center.desktop
sed 's|^Exec=.*|Exec=nimo-control-center-launch --tray|; s|^Name=.*|Name=NIMO Control Center (tray)|' \
	"$SRC/app/nimo-control-center.desktop" | sudo tee /etc/xdg/autostart/nimo-control-center.desktop >/dev/null
sudo gtk-update-icon-cache -q /usr/local/share/icons/hicolor 2>/dev/null || true
if command -v kbuildsycoca6 >/dev/null; then kbuildsycoca6 >/dev/null 2>&1 || true; fi
# Remove a per-user copy from an earlier manual install, so there's only one.
rm -f "$HOME/.local/bin/nimo-control-center" \
	"$HOME/.local/share/applications/nimo-control-center.desktop" \
	"$HOME/.local/share/icons/hicolor/scalable/apps/nimo-control-center.svg" \
	"$HOME/.config/autostart/nimo-control-center.desktop"

say "Done"
note "Driver:  nimo-wmi $DRV_VERSION, rebuilt automatically for new kernels."
note "App:     \"NIMO Control Center\" in your app menu; it also starts in the tray when you log in."
if [ "$LOADED" = 0 ] && [ "$SECURE_BOOT" = 1 ]; then
	note ""
	note "Reboot now and enroll the key (see above). The driver loads after that."
elif [ "$LOADED" = 0 ] && [ "$SECURE_BOOT" = 2 ]; then
	note ""
	note "The driver isn't loaded yet; see the Secure Boot note above."
elif [ "$LOADED" = 0 ]; then
	note ""
	note "The driver isn't loaded yet; reboot, then check 'sudo dmesg | grep nimo'."
else
	if [ "$LOADED" = 2 ]; then
		note ""
		note "The old driver is still running; reboot to finish."
	fi
	if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
		(setsid nimo-control-center-launch >/dev/null 2>&1 &) || true
	fi
fi
note "Remove it all with:  curl -fsSL https://raw.githubusercontent.com/$REPO/main/uninstall.sh | sh"
