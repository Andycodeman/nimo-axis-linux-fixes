# NIMO Control Center for Linux

A Linux version of NIMO's Windows Control Center for the AXIS 395 (N161L), written in Rust with [Slint](https://slint.dev) (native drawing, no web engine; one ~13 MB binary) and [ksni](https://crates.io/crates/ksni) for the tray icon. It needs the `nimo-wmi` driver from `../driver`. Install both with `../install.sh`.

The same four pages as the Windows app:

- **Dashboard:** CPU and GPU frequency dials, temperatures and usage; battery, memory and storage; quick switches for speaker mute, touchpad, Windows key, PrtScrn key and Function key.
- **System Information:** model, CPU, GPU, mainboard, display panel, audio, network, OS, memory, storage, battery capacity and level, and live temperatures.
- **Lighting Setting:** the keyboard drawn from Control Center's own key map, with keycap icons drawn for this app. Click a key to paint its zone, or a light bar button to paint that bar. Colour wheel with R/G/B values, brightness, speed, keyboard sleep time, and all nine firmware effects, previewed live. Also built-in presets (Rainbow, the default; Ocean; Sunset; Forest; White) and **your own presets**: "Save as preset" names the current look (colours, light bars, effect, speed, brightness), it appears under the keyboard (× deletes it) and in the tray menu. Default resets the lighting. There's no Save button because every change is applied and saved as you make it. Effects are previewed zone by zone, like the real keyboard.
- **Power Mode:** both fans with spinning dials and RPM, Quiet / Balanced / High Performance (the same setting as the desktop's power menu), and the battery charge limit (the firmware has it; the Windows app doesn't show it).

Changes apply straight away and are saved to `~/.config/nimo-control-center/lighting.conf`; your presets are in `presets.conf` next to it. The keyboard forgets its lighting across reboots, so the tray copy started at login (`nimo-control-center --tray`) re-applies it.

Build: `cargo build --release`. `NIMO_CC_PREVIEW=<page 0-3>` opens a page without touching the hardware (for screenshots).
