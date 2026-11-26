# Working on nimo-axis-linux-fixes

Linux support for the NIMO AXIS 395 (N161L): the `nimo-wmi` kernel driver (`driver/`) and NIMO Control Center (`app/`, Rust + Slint + ksni). The public repo is github.com/Andycodeman/nimo-axis-linux-fixes.

## Never

- **Commit secrets or personal data:** keys, passwords, tokens, product keys, serial numbers, real names or emails, home-network addresses, raw captures (USB captures contain keystrokes), registry dumps or decompiled NIMO code. Raw data stays outside the repo.
- **Ship NIMO's artwork or code.** The keycap icons (`app/ui/keys/`) and the app icon are original. Only the key *positions* in `app/src/keyboard.rs` come from Control Center's key map.
- **Point CI at a self-hosted runner.** The repo is public; fork pull requests could run code on it. GitHub's own runners are free for public repos.

## Layout

| Path | What |
|---|---|
| `driver/nimo-wmi.c` | The driver. Keyboard via WMI method `WMDA` (GUID 8600ACCE-…); fans and charge limit straight from the EC; power modes through `\_SB.EWMI.NPWR`. |
| `driver/aml/wmcc-fixed.asl` → `driver/wmcc-fixed.h` | Fixed copy of the firmware's power-mode method, loaded as an SSDT. Regenerate with `driver/aml/build.sh` (needs acpica-tools). |
| `driver/udev/` | Rule + script giving the `nimo` group write access to the driver's files. |
| `app/src/hw.rs` | Driver sysfs and power-profiles-daemon (D-Bus); lighting and presets storage. |
| `app/src/sysinfo.rs` | Dashboard and System Information readings (/proc, /sys, wpctl, KWin touchpad). |
| `app/src/main.rs` | UI wiring. Hardware writes go through one background queue (`hardware_queue`), so the window never waits for the firmware. |
| `app/ui/app.slint` | The whole UI. |
| `packaging/` | `build-deb.sh` and the .deb's maintainer scripts. |
| `install.sh` / `uninstall.sh` | One-liners: the .deb on Ubuntu/Debian, a source install elsewhere or from a clone. |
| `notes/control-center-protocol.md` | The decoded Control Center / ACPI protocol. Read it before changing the driver. |

## Firmware facts that bit us

- **17 of the firmware's WMI methods abort on Linux** with `AE_AML_BUFFER_LIMIT` (`CreateField(Local0, 0x20, 0x3F)` on an 8-byte buffer). `WMDA` is fine. Fans and the charge limit therefore use `ec_read`/`ec_write`, and power modes use the SSDT copy `NPWR`.
- `acpi_install_method()` rejects every buffer on Ubuntu's 7.0.0-44 kernel (it reads an `aml_end` it never sets), which is why the fix is loaded with `acpi_load_table()` under a new name instead.
- **Every lighting command takes ~250 ms** inside the firmware. Only send what changed (the driver diffs zones and bars).
- **Zone mask 0x0F with static mode paints all four zones zone 0's colour.** Restore zones one at a time.
- **A light-bar colour command stops a running effect after one pass.** Send the bars first and the effect last; brightness and speed changes re-issue the effect.
- The keyboard has **4 zones, not per-key lighting**. Every effect runs per zone, and the app's preview must too.
- Ripple, Reactive and Rainbow ripple only light keys as they're typed.
- The keyboard forgets its lighting at reboot; the app's tray instance re-applies it at login.
- The power mode lives in the firmware variable `ECPM` (boots as 3 = Performance), and the firmware re-applies its power limits on every AC plug/unplug.
- EC 0x58/0x59 follow the Fn+F8/F9 keys, not the driver, so there's no brightness readback.

## Build, test, release

```sh
make -C driver                       # driver (needs kernel headers)
cd app && cargo build --release      # app; app/build-release.sh strips build paths for releases
packaging/build-deb.sh               # .deb into dist/
NIMO_CC_PREVIEW=2 nimo-control-center   # open page 0-3 without touching hardware (screenshots)
```

- Screenshots: run the app on a hidden display (`Xvfb :97`) with `NIMO_CC_PREVIEW=<page>` and a throwaway `XDG_CONFIG_HOME`, then grab it with `xwd`.
- Test the one-liners piped (`sh < install.sh`), with `NIMO_DEB_URL` / `NIMO_SOURCE_URL` / `NIMO_APP_URL` pointing at local `file://` copies before publishing.
- CI (`.github/workflows/ci.yml`) builds the app on Ubuntu 24.04, builds the driver against Ubuntu 26.04 headers, and runs shellcheck.
- **Release:** bump `version` in `app/Cargo.toml`, add a `CHANGELOG.md` entry, commit, then `git tag vX.Y.Z && git push origin vX.Y.Z`. `release.yml` builds and publishes the app binary and the .deb, under versioned and stable (`…_amd64.deb`, `…-x86_64`) names.
- Bump `PACKAGE_VERSION` in `driver/dkms.conf` (and `MODULE_VERSION`) when the driver changes.
- Don't kill processes with `pkill -f <name>`: it matches the shell running the command. Use `pkill -x -f '[^ ]*nimo-control-center( --tray)?'`.
