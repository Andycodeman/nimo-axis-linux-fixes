# Changelog

## 1.4.0 (2026-11-27)

- **Fan speed Auto / Max.** The Power Mode page has a Fan speed row with **Auto** and **Max**, and the driver has a new `fan_mode` file (`auto` or `max`) in its WMI device's folder. It uses the firmware's own fan-control sequence: command 0x11 into EC 0x30, then the mode into EC 0x32, each read back. Max pins fan 1 at about 4000–4200 rpm; fan 2 stays on the EC's automatic curve, as in the firmware (seen on the laptop). Max isn't saved: every boot starts in Auto.
- The driver sets the fans to Auto when it loads, because the firmware can leave them at Max, and sends the fan mode again after every resume.
- `fan_mode` shows the last mode the driver set. The snowflake key boosts the fans inside the EC without telling Linux, so after a press `fan_mode` and the app can show the wrong mode. The page says so under the row.
- **When charging starts again.** With the charge limit on, a new slider sets **Start charging again below N %**, from 40 % to 5 below the limit, in steps of 5; it's greyed out while the limit is off. The driver adds `charge_control_start_threshold` to the battery (20–99, below the end limit; 0 while there's no limit), so **KDE's charge-limit page** (System Settings → Power Management → Advanced) can now show both fields. The firmware itself always sets the start to one below the end, so the driver remembers a lower start, whether it's written before or after the end, and an end write keeps it. The app writes the end first, then the start, and leaves the start alone on a driver older than 0.8.
- Uninstalling (the one-liner or removing the .deb) sets the fans back to Auto before it removes the driver.
- **Upgrades load the new driver without a reboot**, through the .deb (apt, Discover, the install one-liner) and a source install. When the old driver is still running and the new build is installed for the running kernel, the installer unloads the old one and loads the new one (compared by `srcversion`, so a same-version rebuild counts too). If the new build isn't installed for this kernel (a failed DKMS build), or the old driver won't unload, it leaves the old one running and says "reboot to finish".
- The keyboard light stays on through the reload: unloading the driver no longer turns it off.
- The app re-applies your lighting when the driver reloads or appears after being missing (a refresh after it shows up, so its files are open to the `nimo` group), also after it restarted itself for an upgrade.
- The fan pill follows the driver: a reload sets the fans back to Auto, and the pill now shows that.
- nimo-wmi 0.8.
- Not yet tested on the laptop: Auto bringing the fans back down after Max, suspend and resume, KDE's page, whether the EC honours a start below end − 1 (stop at 80 %, start again below 60 %), a reload with the light on, and an upgrade with the app running.
- Thanks to [fewtarius/emdoor-wmi](https://github.com/fewtarius/emdoor-wmi) (GPL-2.0-or-later) for the fan-control sequence and the start-threshold idea.

## 1.3.0 (2026-11-27)

- The app checks for a newer release once a day. It asks GitHub's API which release is the latest, sending only `User-Agent: nimo-control-center/<version>` and the reply's ETag from last time (so an unchanged answer doesn't count against GitHub's limit). Offline or any error, it stays quiet and tries again an hour later. What it found, and "Check for updates" on or off, are kept in `~/.config/nimo-control-center/update.conf`. Only the copy that owns the app's D-Bus name checks. `NIMO_UPDATE_URL` points the check at another server for testing.
- When a newer release is out, the window shows a banner with its first note, **Update…** and **Skip this version** (✕ hides it until the next start), and the sidebar says "Update to X available". **Update…** shows what's new, the installer's one-line command with **Copy**, and **Open a terminal**, which runs the installer in the first terminal it finds (`xdg-terminal-exec`, `x-terminal-emulator`, Konsole, GNOME Terminal, Console, Xfce Terminal, foot, kitty, Alacritty). One desktop notification per version offers the same **Update…** and **Skip this version**. System Information has the **Check for updates** switch and when it last checked.
- While an update is on offer, the tray icon gets a dot in its corner (drawn from the app's own icon), its tooltip says "Update to X available", and the tray menu has **Update to X…**, which opens the window at the update panel. Skipping the version or switching the check off removes them.
- README: what the update check sends, and how to turn it off.

## 1.2.3 (2026-11-26)

- Only one copy of NIMO Control Center runs. Every launch from the app menu, the installer or the login autostart used to start another copy with its own tray icon. Now the first copy owns the D-Bus name `io.github.andycodeman.NimoControlCenter`; opening the app again shows that copy's window, and the login `--tray` start leaves it alone.
- The app restarts itself after an upgrade. When apt, Discover, the installer or a source install replaces its file, the running copy checks that the new file runs and switches to it in place, in the tray if its window was hidden, without re-applying the lighting. Upgrading the .deb from an earlier version closes the old copies once and starts the new app in each user's tray (or at their next login).
- On CachyOS and Arch the app starts straight after installing, without logging out first. Until you log out and back in, your session doesn't have the `nimo` group yet, and the launcher used `sg` to add it, but Arch doesn't ship `sg`. So the installer's "start the app" did nothing, and opening it from the app menu failed. The launcher now uses `newgrp` when `sg` is missing, and if that fails too, starts the app anyway.

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
