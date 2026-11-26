# NIMO AXIS 395 (N161L) on Linux

Getting the NIMO AXIS AI Max+ 395 laptop (model N161L, built by Emdoor) fully working on Linux, starting with Kubuntu 26.04 / Ubuntu 26.04. This is the same approach as [samsung-galaxy-book-linux-fixes](https://github.com/Andycodeman/samsung-galaxy-book-linux-fixes).

> **This repo is private for now and will be made public.** Never commit secrets or personal data: product keys, passwords, tokens, serial numbers, email addresses, raw captures (USB captures can contain keystrokes), or registry dumps. Raw data stays on the laptop's own disk. Only cleaned-up findings and code go in here.

## Hardware

| Part | ID | Linux status |
|---|---|---|
| Wi-Fi 7: MediaTek MT7925 | PCI 14c3:7925 | Works (`mt7925e`) |
| Bluetooth: MediaTek MT7925 | USB 13d3:3604 | Works |
| Ethernet: Realtek 2.5GbE | PCI 10ec:8125 | Works (`r8169`) |
| Audio codec: Realtek ALC233/0235 | HDA `SUBSYS_2782A246` | Speakers work |
| Internal mic array: AMD ACP 7.0 DMIC | PCI 1022:15e2 | Under test (`acp_asoc_acp70: No matching ASoC machine driver`) |
| Webcam + IR camera | USB 3277:00be | Works (UVC) |
| Fingerprint: Microarray MAFP | USB 3274:8012 | Not in stock libfprint; community driver in progress |
| Keyboard RGB backlight / hotkeys | USB 048d:8910 (ITE) | **Unknown: no Linux control yet** |
| Fans / power modes | EC, through ACPI WMI | **Unknown: no Linux control yet** |
| NPU: AMD XDNA | PCI 1022:17f0 | Firmware loads (`amdxdna`) |

## Goal 1: a Linux "Control Center"

NIMO's Windows Control Center (RGB version v1.0.8, 2026-08-18) controls the keyboard RGB, fan and power modes, and possibly other settings. The plan:

1. **Windows:** install it under capture and record what it touches. Then record every setting change while capture runs: USB traffic to the ITE keyboard controller, WMI/ACPI calls, registry and driver activity. Steps: [windows/INSTRUCTIONS.md](windows/INSTRUCTIONS.md).
2. **Linux:** decompile the installed app (it's an Inno Setup 6.6 installer, too new for innoextract 1.9, so the files come from the Windows install). Match it against the captures and the ACPI tables (`acpidump`, WMI `_WDG` blocks), then write a small Linux tool / kernel module to do the same.
3. Package it for Kubuntu first, then other distros.

## Goal 2: everything else

Mic array, fingerprint, sleep/lid, and any other fixes. The notes from the first setup (kept privately for now) will be cleaned up and moved here.

## How Windows and Linux share data

Both OSes are on the same disk. Windows writes raw captures to `C:\nimo-setup\captures\`, and Linux reads them off the NTFS partition. Only cleaned-up findings get committed here.
