# Changelog

## 1.2.2 (2026-11-26)

- The installer warns about TUXEDO's `ite_829x` and offers to block it. That module (from `tuxedo-drivers`) also claims this keyboard, USB `048d:8910`, and turns the backlight off and the keys white whenever it loads and after every suspend. When it's installed, the installer explains in plain words what it would block, that only `ite_829x` is affected and the keyboard types as before, which one file it writes and how to undo it, then asks **Block ite_829x? [Y/n]**. Yes writes `/etc/modprobe.d/nimo-block-ite_829x.conf` and unloads the module; with no terminal to ask on, it prints the same explanation and what to do, and changes nothing. The uninstall one-liner and the .deb's purge remove that file again, and only if the installer wrote it. Tested in the CachyOS VM with a stand-in `ite_829x` and a fake 048d:8910 device: not installed, installed, loaded and bound, yes, no, no terminal, a reboot and `mkinitcpio -P`.
- Installer questions no longer end the script when there's no terminal to read from (cron, `setsid`, ssh without `-t`). `/dev/tty` can be there and readable but fail to open; the installer now tries opening it.
- Keyboard colours are right on the first login after a reboot. When the app restored your lighting at login, the four keyboard zones sometimes went back to cyan straight after it set them; the light bars kept their colours. The kernel applies a backlight brightness change a moment later, and when that landed after the zone colours, nimo-wmi took the colour it had read beforehand as a new colour for the whole keyboard. The driver (nimo-wmi 0.7) now reads the colour at the moment it applies the change, and the brightness it reports back is correct as well. Tested over six reboots in a CachyOS VM.

## 1.2.1 (2026-11-26)

- Keyboard controls work on kernels that number WMI devices (CachyOS, Arch, any kernel 6.17+ without legacy names). The app used to look for the driver's files under the bare WMI GUID, so on those kernels it showed "driver not loaded" and colours, effects, speed, the sleep timer and the key switches failed. It now finds the device under either name.
- CI runs the app's unit tests.

## 1.2.0 (2026-11-26)

- CachyOS and Arch: the one-line installer now works on pacman systems. It installs DKMS, the build tools and the headers for the running kernel (plus clang, llvm and lld on clang-built kernels), and stops with a clear message if the kernel was updated without a reboot or the headers don't match it. Tested on CachyOS with `linux-cachyos`, `linux-cachyos-lts` and Arch's `linux` kernel, Secure Boot off.
- Secure Boot on Arch: if the driver doesn't load, the installer explains how to enroll DKMS's key (`/var/lib/dkms/mok.pub`) or what to expect with sbctl.
- CI builds the driver against `linux-cachyos` headers as well as Ubuntu's.

## 1.1.0 (2026-11-26)

- .deb package: install with apt or by double-clicking; remove it the same way.
- Release builds now come from GitHub Actions on Ubuntu 24.04, so the app runs on Ubuntu 24.04 and newer.
- The one-line installer installs the .deb on Ubuntu/Debian.

## 1.0.0 (2026-11-26)

- nimo-wmi 0.6 driver: 4-zone keyboard RGB, side light bars, 9 effects, keyboard sleep timer, Win/Fn/PrtSc key switches, fan speeds, battery charge limit and power modes.
- NIMO Control Center: Dashboard, System Information, Lighting Setting and Power Mode, a tray icon, built-in presets and your own saved presets.
