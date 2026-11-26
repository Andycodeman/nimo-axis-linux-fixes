<div align="center">

<img src="app/ui/icon.svg" width="96" alt="">

# NIMO AXIS 395 on Linux

**A Linux driver and a Control Center app for the NIMO AXIS AI Max+ 395 (N161L) laptop.**
Keyboard RGB, side light bars, power modes, fans, key locks and battery charge limit, just like NIMO's Windows app, plus a few things it can't do.

[![License: GPL v3](https://img.shields.io/badge/app-GPL--3.0-blue)](LICENSE)
[![Driver: GPL v2](https://img.shields.io/badge/driver-GPL--2.0-blue)](driver/LICENSE)
![Ubuntu 26.04](https://img.shields.io/badge/Ubuntu%20%2F%20Kubuntu-26.04-E95420?logo=ubuntu&logoColor=white)
![Rust + Slint](https://img.shields.io/badge/app-Rust%20%2B%20Slint-orange?logo=rust)

<img src="docs/screenshots/keyboard-rainbow.gif" width="640" alt="The keyboard preview running the Rainbow effect">

</div>

---

## Install

One line. It asks for your password when it needs it.

```sh
curl -fsSL https://raw.githubusercontent.com/Andycodeman/nimo-axis-linux-fixes/main/install.sh | sh
```

No `curl`? Stock Ubuntu doesn't ship it. Use `wget` instead:

```sh
wget -qO- https://raw.githubusercontent.com/Andycodeman/nimo-axis-linux-fixes/main/install.sh | sh
```

That's it. **NIMO Control Center** opens straight away, appears in your app menu, and starts in the tray every time you log in.

**Prefer a package?** On Ubuntu/Debian the one-liner installs the `nimo-control-center` .deb anyway. You can also download [`nimo-control-center_amd64.deb`](https://github.com/Andycodeman/nimo-axis-linux-fixes/releases/latest/download/nimo-control-center_amd64.deb) from the [latest release](https://github.com/Andycodeman/nimo-axis-linux-fixes/releases/latest) and open it in Discover / Software, or run `sudo apt install ./nimo-control-center_amd64.deb`.

> **Secure Boot on?** The installer asks you to pick a one-time password. Reboot, choose **Enroll MOK → Continue → Yes** on the blue screen, and type that password. This lets the kernel load the driver; it's the same step as for any DKMS driver (VirtualBox, NVIDIA, …).

### Uninstall

Remove **NIMO Control Center** in Discover / Software, or:

```sh
sudo apt purge nimo-control-center
```

or the uninstall one-liner, which also cleans up installs made without the package:

```sh
curl -fsSL https://raw.githubusercontent.com/Andycodeman/nimo-axis-linux-fixes/main/uninstall.sh | sh
```

Every route removes the driver, the app and the permissions rule, and sets the battery charge limit back to 100 %. Your saved lighting and presets stay in `~/.config/nimo-control-center` unless you delete that folder (or pass `-s -- --purge` after `sh`).

---

## Screenshots

| Dashboard | System Information |
|---|---|
| ![Dashboard](docs/screenshots/dashboard.png) | ![System Information](docs/screenshots/system-information.png) |
| **Lighting Setting** | **Power Mode** |
| ![Lighting Setting](docs/screenshots/lighting.png) | ![Power Mode](docs/screenshots/power-mode.png) |

## What you get

### NIMO Control Center

The same four pages as NIMO's Windows Control Center, in the same look:

- **Dashboard:** CPU and GPU frequency dials, temperatures and usage; battery, memory and storage. Quick switches for speaker mute, touchpad, Windows key, PrtScrn key and Function key.
- **System Information:** model, CPU, GPU, mainboard, display panel, audio, network, OS, memory, storage and battery, with live temperatures.
- **Lighting Setting:**
  - The full keyboard, number pad included, glowing in its zone colours. Click a key to paint its zone, or a light-bar button to paint that bar.
  - All 9 firmware effects (Always, Breath, Rainbow, Wave, Cycle, Twinkle, Ripple, Reactive, Rainbow ripple), previewed live.
  - Colour wheel and R/G/B values, brightness, speed and keyboard sleep timer.
  - Built-in presets (Rainbow, Ocean, Sunset, Forest, White), plus **your own**: *Save as preset* stores the whole look.
- **Power Mode:** both fans with live RPM, and Quiet / Balanced / High Performance. Plus the **battery charge limit**: the firmware supports it, but the Windows app never showed it.
- **Tray icon:** power mode, keyboard light, effects and presets, with fan speeds on hover.

Changes apply as you make them and are re-applied when you log in. The keyboard itself forgets its lighting at every reboot.

### The nimo-wmi driver

A small kernel module (DKMS, so it rebuilds itself for new kernels) that exposes everything through standard Linux interfaces. The desktop's own settings use them too, not just the app:

| Feature | Where |
|---|---|
| Power modes | `platform_profile`, so **KDE's and GNOME's power menu** works |
| Fan speeds | hwmon `nimo` (`sensors`, system monitors) |
| Battery charge limit | `charge_control_end_threshold` on the battery |
| Keyboard brightness / colour | LED `nimo::kbd_backlight` |
| Zone colours, light bars, effects, speed, sleep timer, key switches | `/sys/bus/wmi/devices/8600ACCE-…/` |

Details: [driver/README.md](driver/README.md).

## Is it safe? What does it change?

Everything is one normal package, `nimo-control-center`, so your package manager knows every file and removes them all.

| Added | Why |
|---|---|
| `dkms` and your kernel's headers (pulled in by apt) | To build the driver, the same as any DKMS driver (NVIDIA, VirtualBox…). |
| The `nimo-wmi` driver source in `/usr/src`, built by DKMS | **Only loads on an N161L**: it checks the model and refuses anything else. |
| A `nimo` group (the computer's admins are added to it) and a udev rule | So the app can change settings without your password. |
| `/usr/bin/nimo-control-center`, its menu entry, icon and login autostart | The app. |

It doesn't touch your bootloader, firmware or BIOS settings, and it flashes nothing. One thing to know: the power-mode fix loads a corrected copy of one firmware routine into memory each time the driver starts. NIMO's original is left untouched, and nothing is written to the BIOS.

## Hardware status

| Part | ID | Linux status |
|---|---|---|
| Keyboard RGB (4 zones), side light bars, effects, sleep timer | EC, ACPI WMI | ✅ with this driver |
| Win / Fn / PrtSc key switches | EC, ACPI WMI | ✅ with this driver |
| Power modes (Quiet / Balanced / Performance) | EC + ACPI | ✅ with this driver |
| Fan speeds, battery charge limit | EC | ✅ with this driver |
| Wi-Fi 7: MediaTek MT7925 | PCI 14c3:7925 | ✅ Works (`mt7925e`) |
| Bluetooth: MediaTek MT7925 | USB 13d3:3604 | ✅ Works |
| Ethernet: Realtek 2.5GbE | PCI 10ec:8125 | ✅ Works (`r8169`) |
| Webcam + IR camera | USB 3277:00be | ✅ Works (UVC) |
| Keyboard (ITE) | USB 048d:8910 | ✅ Works |
| NPU: AMD XDNA | PCI 1022:17f0 | ✅ Firmware loads (`amdxdna`) |
| Audio codec: Realtek ALC233 | HDA `SUBSYS_2782A246` | 🟡 Detected; not fully tested |
| Internal mic array: AMD ACP 7.0 DMIC | PCI 1022:15e2 | 🟡 Works after lowering the input gain |
| Fingerprint: Microarray MAFP | USB 3274:8012 | ❌ Not in stock libfprint yet |

## How it was built

NIMO's Windows Control Center was installed under capture: Procmon, WMI tracing and USB capture of every setting ([windows/INSTRUCTIONS.md](windows/INSTRUCTIONS.md), [findings](notes/windows-capture-20261126.md)). The .NET app was decompiled, and its calls were matched against the laptop's ACPI tables.

Everything goes through the firmware's ACPI WMI interface, not USB. The full protocol is in **[notes/control-center-protocol.md](notes/control-center-protocol.md)**. Along the way it turned up:

- **A BIOS bug that blocks 17 firmware routines on Linux:** a field declared larger than its buffer. Windows ignores it; Linux refuses to run them. The driver works around it.
- **The keyboard sleep timer never worked on Windows** (a firmware bug); on Linux it does.
- The firmware has a **battery charge limit** that the Windows app never offered.

The protocol notes describe both bugs and the one-line BIOS fix for each.

## Building from source

```sh
git clone https://github.com/Andycodeman/nimo-axis-linux-fixes
cd nimo-axis-linux-fixes
app/build-release.sh               # needs Rust: https://rustup.rs (cargo build --release, minus build paths)
./install.sh                        # uses your build
```

Releases are built by GitHub Actions. The app is built on Ubuntu 24.04, so it runs there and on anything newer; the driver is tested on Linux 7.0 (Ubuntu 26.04, what the N161L ships with) and needs a recent kernel. To make a release, bump the version in `app/Cargo.toml`, add a `CHANGELOG.md` entry, and push a tag such as `v1.1.0`. `packaging/build-deb.sh` builds the .deb locally.

## Credits and disclaimer

This is an independent community project, **not affiliated with or endorsed by NIMO or Emdoor**. "NIMO" and "AXIS" are their trademarks and are used only to say which laptop this is for. The keycap icons, the app's icon and all code are original to this project. The keyboard layout's key positions come from Control Center's key map, so the drawing matches the real keyboard.

It works on the author's N161L. Use it at your own risk; it comes with no warranty (see the licences).

- App: [GPL-3.0-or-later](LICENSE). Built with [Slint](https://slint.dev) and [ksni](https://crates.io/crates/ksni).
- Driver: [GPL-2.0-or-later](driver/LICENSE), like the Linux kernel.

Same approach as [samsung-galaxy-book-linux-fixes](https://github.com/Andycodeman/samsung-galaxy-book-linux-fixes).
