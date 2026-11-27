#!/bin/sh
# Removes everything install.sh added: NIMO Control Center, the nimo-wmi
# driver, the permissions rule and the 'nimo' group.
#
#   curl -fsSL https://raw.githubusercontent.com/Andycodeman/nimo-axis-linux-fixes/main/uninstall.sh | sh
#   wget -qO-  https://raw.githubusercontent.com/Andycodeman/nimo-axis-linux-fixes/main/uninstall.sh | sh
#
# Your saved lighting and presets (~/.config/nimo-control-center) are kept;
# add --purge (./uninstall.sh --purge, or "| sh -s -- --purge") to delete them too.
set -eu

say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }

[ "$(id -u)" -ne 0 ] || { echo "Run this as your normal user (it uses sudo itself)."; exit 1; }

# The ite_829x block install.sh offers when TUXEDO's drivers are installed. Only our own file, marked.
ITE_BLOCK=/etc/modprobe.d/nimo-block-ite_829x.conf
if grep -qs 'nimo-axis-linux-fixes' "$ITE_BLOCK"; then
	say "Removing the ite_829x block"
	sudo rm -f "$ITE_BLOCK"
	echo "Removed $ITE_BLOCK, so TUXEDO's ite_829x"
	echo "is no longer blocked: it can load again after a reboot (and reset the keyboard lighting)."
fi

# Installed as a package: let apt remove it (its scripts reset the charge limit too).
if dpkg-query -W -f='${Status}' nimo-control-center 2>/dev/null | grep -q "ok installed"; then
	say "Removing the nimo-control-center package"
	sudo env DEBIAN_FRONTEND=noninteractive apt-get purge -y nimo-control-center
	if [ "${1:-}" = "--purge" ]; then
		rm -rf "$HOME/.config/nimo-control-center"
	fi
	say "Done"
	exit 0
fi

say "Removing NIMO Control Center"
# Match the app's whole command line only, never a shell that merely mentions it.
pkill -x -f '[^ ]*nimo-control-center( --replaced)?( --tray)?' 2>/dev/null || true
sudo rm -f /usr/local/bin/nimo-control-center /usr/local/bin/nimo-control-center-launch \
	/usr/local/share/applications/nimo-control-center.desktop \
	/usr/local/share/icons/hicolor/scalable/apps/nimo-control-center.svg \
	/etc/xdg/autostart/nimo-control-center.desktop
rm -f "$HOME/.local/bin/nimo-control-center" \
	"$HOME/.local/share/applications/nimo-control-center.desktop" \
	"$HOME/.local/share/icons/hicolor/scalable/apps/nimo-control-center.svg" \
	"$HOME/.config/autostart/nimo-control-center.desktop"
if [ "${1:-}" = "--purge" ]; then
	rm -rf "$HOME/.config/nimo-control-center"
fi

say "Removing the nimo-wmi driver"
# The charge limit lives in the laptop's controller and would outlast the driver.
for t in /sys/class/power_supply/BAT*/charge_control_end_threshold; do
	[ -e "$t" ] || continue
	echo 100 | sudo tee "$t" >/dev/null 2>&1 || true
done
# Leave the fans on auto, not Max, once the driver is gone.
for t in /sys/bus/wmi/devices/8600ACCE-*/fan_mode; do
	[ -e "$t" ] || continue
	echo auto | sudo tee "$t" >/dev/null 2>&1 || true
done
sudo modprobe -r nimo_wmi 2>/dev/null || true
for v in $(dkms status nimo-wmi 2>/dev/null | sed -n 's|^nimo-wmi/\([^,:]*\).*|\1|p' | sort -u); do
	sudo dkms remove "nimo-wmi/$v" --all || true
	sudo rm -rf "/usr/src/nimo-wmi-$v"
done
sudo rm -f /etc/modules-load.d/nimo-wmi.conf

say "Removing the permissions rule and the 'nimo' group"
sudo rm -f /etc/udev/rules.d/70-nimo-wmi.rules
sudo rm -rf /usr/local/lib/nimo-wmi
sudo udevadm control --reload
sudo groupdel nimo 2>/dev/null || true

if systemctl is-active -q power-profiles-daemon 2>/dev/null; then
	sudo systemctl restart power-profiles-daemon
fi

say "Done"
echo "The charge limit is back to 100 %. The keyboard keeps its current lighting until the next reboot,"
echo "and the power-mode fix the driver loaded stays until then too (it's harmless)."
