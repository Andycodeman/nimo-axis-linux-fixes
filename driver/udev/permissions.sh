#!/bin/sh
# Called by 70-nimo-wmi.rules with the WMI device's sysfs path.
dev="$1"
allow() {
	[ -e "$1" ] || return 0
	chgrp nimo "$1" && chmod g+w "$1"
}
for f in kbd_effect kbd_speed kbd_zone_colors kbd_lightbar_colors kbd_sleep_timer win_key fn_key prtsc_key; do
	allow "$dev/$f"
done
allow /sys/class/leds/nimo::kbd_backlight/brightness
allow /sys/class/leds/nimo::kbd_backlight/multi_intensity
for b in /sys/class/power_supply/BAT*/charge_control_end_threshold; do
	allow "$b"
done
