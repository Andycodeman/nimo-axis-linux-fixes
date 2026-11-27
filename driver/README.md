# nimo-wmi: kernel driver for the NIMO AXIS 395 (N161L)

Version 0.8 (2026-11-27). The protocol behind it is in [../notes/control-center-protocol.md](../notes/control-center-protocol.md).

## What it does

| Feature | Where | Tested on the N161L |
|---|---|---|
| Keyboard backlight on/off and brightness 0–10 | `/sys/class/leds/nimo::kbd_backlight/brightness` | ✅ off, then on again (seen on the keyboard) |
| One colour for the whole keyboard | `/sys/class/leds/nimo::kbd_backlight/multi_intensity` (`R G B`, 0–255) | ✅ command accepted |
| Colour per zone (4 zones, left to right) | `kbd_zone_colors` (`RRGGBB RRGGBB RRGGBB RRGGBB`) | ✅ red/green/blue/white seen on the keyboard |
| Effects | `kbd_effect`: static, twinkle, wave, breath, cycle, reactive, ripple, spiral_rainbow, rainbow_ripple | ✅ wave seen on the keyboard |
| Effect speed 1–10 | `kbd_speed` | not yet |
| Windows key / Fn key / Print Screen key on/off | `win_key`, `fn_key`, `prtsc_key` (1 = on, matching Control Center's toggles) | ✅ prtsc_key 1→0→1, read back from the firmware |
| Fan speeds | hwmon `nimo`: `fan1_input`, `fan2_input` (`sensors`) | ✅ 1808 / 2007 rpm |
| Fan mode (Auto / Max) | `fan_mode` (`auto`, `max`; shows the last mode the driver set). Auto at every load, sent again after resume | not yet: Max spins up, Auto settles, survives suspend |
| Battery charge limit | `/sys/class/power_supply/BAT0/charge_control_end_threshold` (20–100; 100 = no limit) | ✅ 80, then 100, read back from the EC |
| Battery charge start | `/sys/class/power_supply/BAT0/charge_control_start_threshold` (20–99, below the end; 0 while there's no limit) | ✅ at 80 %: end 85 / start 75 held; start 82 charged within 5 s; back to 80/75 stopped it (one-point overshoot); EC read back 80/75 |
| Side light bars (left, right) | `kbd_lightbar_colors` (`RRGGBB RRGGBB`) | ✅ magenta/purple seen on the bars |
| Keyboard sleep timer | `kbd_sleep_timer` (seconds, 0 = never; Control Center offers 60–7200) | ✅ off after 1 min, back on a keypress. (It never worked on Windows: see the protocol notes.) |
| Power modes (Quiet / Balanced / Performance) | `platform_profile` (KDE/GNOME power menu via power-profiles-daemon) | ✅ all three, read back from the EC; the power menu drives it |

`kbd_effect`, `kbd_speed`, `kbd_zone_colors`, `kbd_lightbar_colors`, `kbd_sleep_timer`, `win_key`, `fn_key`, `prtsc_key` and `fan_mode` live in `/sys/bus/wmi/devices/8600ACCE-FB9B-443E-86F4-3C867398AAE5/`. Since kernel 6.17 the kernel adds a number to that name (`8600ACCE-…-12`) unless it was built with `CONFIG_ACPI_WMI_LEGACY_DEVICE_NAMES=y` (Ubuntu is; CachyOS and Arch aren't), so find it through `/sys/bus/wmi/drivers/nimo-wmi/`, which links only the device the driver bound.

The LED is named `kbd_backlight`, so UPower (and with it KDE's and GNOME's keyboard brightness controls) should pick it up. That hasn't been checked yet.

## Install

Use `install.sh` at the top of the repo: it installs this driver with DKMS, the permissions rule and the app. `uninstall.sh` removes it all.

## Build and try it by hand

```sh
make
sudo modprobe led-class-multicolor
sudo insmod ./nimo-wmi.ko
```

To install it with DKMS so it is rebuilt for every new kernel:

```sh
sudo cp -r . /usr/src/nimo-wmi-0.1
sudo dkms install nimo-wmi/0.1
```

With Secure Boot on, DKMS signs the module with your MOK key (enrol it once with `mokutil`).

It only loads on DMI vendor "Nimo" + product "N161L". `force=1` overrides that, but the EC offsets may be wrong on other Emdoor models.

## Notes

- Lighting changes are not remembered by the firmware across a reboot; that depends on the EC. A user-space app will re-apply them at login.
- The firmware's light-state/level registers follow the Fn+F8/F9 keys, not this driver, so `brightness` is the driver's own copy. A change made with the Fn keys won't show up there.
- Fans and the charge limit use the EC directly, because the firmware's WMI methods for them abort on Linux (`AE_AML_BUFFER_LIMIT`). The writes are exactly the ones the firmware method would make.
- **Power modes** go through the firmware's own routine (`WMCC`), because the firmware re-applies that mode's AMD power limits on every plug/unplug. The BIOS copy hits the field-size bug, so the driver loads a fixed copy as a small SSDT (`aml/wmcc-fixed.asl`, method `\_SB.EWMI.NPWR`) with `acpi_load_table()` and calls that. On a fixed BIOS it uses NIMO's method and loads nothing. (`acpi_install_method()` to patch `WMCC` in place doesn't work on Ubuntu's 7.0.0-44 kernel: it rejects every buffer with AE_BAD_PARAMETER, because its opcode check reads an `aml_end` it never sets.)
- **Never send zone mask 0x0F with static mode:** the firmware paints all four zones zone 0's colour. Restoring the lighting sends each zone separately.
- Not checked yet: whether effects (wave, cycle…) also animate the light bars or leave their colours alone.
- **A light-bar colour command stops a running effect after one pass.** So the driver sends the bars first and the effect last, and re-starts the effect after a bar change. Brightness and speed changes re-issue the effect with the new value. (Breath confirmed to keep running on the N161L.)
- **Every lighting command takes about 250 ms** (pauses inside NIMO's firmware routine). The driver only sends zones and bars whose colour actually changed: one zone takes ~0.26 s, a full repaint (4 zones + 2 bars) ~1.6 s.
- **Fan mode** uses the firmware's `FanControl` sequence (EC 0x30 = 0x11, then 1 or 2 in 0x32, each read back), as [emdoor-wmi](https://github.com/fewtarius/emdoor-wmi) (GPL-2.0-or-later) does. The snowflake key boosts the fans inside the EC without telling Linux, so after a press `fan_mode` still shows the last mode the driver set. Unloading leaves whatever mode was set; `uninstall.sh` and the .deb set Auto first.
- **Charge start threshold:** the firmware's own end write sets start to end − 1. The driver remembers a lower start you asked for (written before or after the end), and an end write sets start to the lower of that and end − 1. With no end limit (100), a start write is only remembered. The start-threshold idea also comes from emdoor-wmi.
- `udev/` holds the rule that lets the `nimo` group write these files without root.
