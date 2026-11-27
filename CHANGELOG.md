# Changelog

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
