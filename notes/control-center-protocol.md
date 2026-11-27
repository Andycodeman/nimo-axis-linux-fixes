# How NIMO Control Center talks to the hardware

Worked out on 2026-11-26 from three sources:

- the Windows capture session ([windows-capture-20261126.md](windows-capture-20261126.md)),
- the decompiled Control Center v1.0.8 and the factory PowerModeUtil and KeyDetect tools (all .NET, decompiled with ILSpy 11.2),
- this laptop's ACPI tables (`acpidump`, `iasl -d`, and the binary MOF from `/sys/bus/wmi/devices/05901221-…/bmof`).

Raw captures and decompiled sources are not in this repo. Only the protocol is described here.

## The short version

- **Nothing goes over USB.** The ITE 048d:8910 device is just the keyboard. Lighting, key locks, power modes and fans all go through **ACPI WMI** (`\_SB.EWMI`, `PNP0C14`), and the EC does the work.
- The Windows UI (`ASHControlCenter.exe`) sends JSON over local HTTP to `ControlCenterService.exe` (LocalSystem), which makes the WMI calls.
- Every vendor method takes one `Data` value (a buffer or u64 on Windows). It returns a buffer whose **first 2 bytes are a status code** (0 = OK, 1 = not supported, 2 = invalid parameter) and whose **payload starts at byte 2**. That is why the app shifts every result right by 16.
- **Firmware bug: 17 of the 21 WMI methods fail on Linux.** Details below. The keyboard method (`WMDA`) is not affected, so lighting and key locks already work from Linux.

## GUID → ACPI method map

| WMI class | GUID | Linux object id | ACPI method |
|---|---|---|---|
| EmdAcpi_AppKBLedControl (lighting, key locks) | 8600ACCE-FB9B-443E-86F4-3C867398AAE5 | DA | `WMDA` ✅ works |
| EmdAcpi_PowerModeControl | 4BCA6480-4D03-4674-84CB-26B4C8F5CFC2 | BF | `WMBF` ❌ bug |
| SetOemPowerSwitch (what the app actually uses for power modes) | ABBC0FB8-8EA1-11D1-A000-C90629100000 | CC | `WMCC` ❌ bug |
| EmdAcpi_FanControl | D06385DE-BC53-4B9B-85DC-674936D9549F | AF | `WMAF` ❌ bug |
| EmdAcpi_ECInformation | 861F913E-1B25-41E5-B4DD-D1FD9091713F | AD | `WMAD` ❌ bug |
| EmdAcpi_BatteryChargeRationing | 6B40A935-7FEF-42B6-B08D-6C79B57D6C35 | DD | `WMDD` ❌ bug |
| EmdAcpi_FunctionKey | 67DAF0CA-A83E-4D58-B462-CD7201A2E6D6 | DB | `WMDB` ❌ bug |
| EmdAcpi_Keyboard_BackLight | 2C70BE1B-C60E-452D-9413-EE888A830CBD | DC | `WMDC` ❌ bug |
| EmdAcpi_BatteryInfo | 6377F67E-DA51-45F6-8898-33EF5F1F5A16 | DF | `WMDF` ❌ bug |
| EmdAcpi_ShipMode | A654F3A7-8548-4E33-B464-6D8B21B2DA88 | DE | **never call** |
| EmdAcpi_SetSecureBoot | DBCDEBCC-2EB0-44A7-9413-7E566B42E2CD | BB | **never call** |
| EmdAcpi_LoadDefault (BIOS defaults) | C60B0BC4-B53B-41A6-9D24-51E33C742906 | AC | **never call** |
| EmdAcpi_SetPxeControl | A9135B56-66A2-48F0-A74B-F940F0911F36 | BD | **never call** |
| EmdAcpi_AutoPowerOn | CC97948F-484D-4D94-B5A4-BCD468B048F9 | BE | **never call** |
| AcpiTest_EcLid (EC test mode) | 72CCF8D6-FF88-4B77-AED9-702724DA0925 | AA | **never call** |
| EmdAcpi_GetTypeCVBUS | 5399E2B7-1046-4058-936D-882EE7492C5B | BA | |
| EmdAcpi_LedControl | 971F2336-4433-4124-8B8E-28FB454E88AA | AE | |

Events (notify ids): `EmdAcpi_MonitorFunctionKey` 66A685FD-… (D1), `EmdAcpi_AppKBLightState` 5A433D20-… (D2), `EmdAcpi_AppKBLightLevel` DF190144-… (D3).

Method ids run 1, 2, 3… in the order the MOF lists them. The ACPI `Switch (Arg1)` cases confirm this.

## Keyboard: `WMDA` (works on Linux)

| Id | Method | Input | Output |
|---|---|---|---|
| 1 | SetDeviceStatus | | |
| 2 | GetDeviceStates | byte 0 = device type (2 = keyboard) | keyboard type and max level |
| **3** | **Set4ZoneKBLed** | 8 bytes, see below | status |
| 4 | GetKBLightingEffect | | current effect/colour table |
| 5 / 6 | SetPreKeyKBLed / RestorePreKeyKBStates | (per-key boards; not this one) | |
| **7** | **SetKBHotkey** | u16, see below | status |
| **8** | **GetKBHotkey** | | u16 at bytes 2–3 |
| 9 | AppInitComplete | | no-op |
| 10 | SetKBLedSleepTimer | u16 | **broken, see below** |
| 11 | GetKBLightState | | byte 0 = EC reg 0x59 (KBLF) |
| 12 | GetKBLightLevel | | byte 0 = EC reg 0x58 (KBLL), 1–10 |

### Set4ZoneKBLed: 8 bytes, little-endian u64

| Byte | Meaning |
|---|---|
| 0 | Zone mask: bit 0–3 = zones 0–3, bit 4 = left light bar, bit 5 = right light bar. 0 = whole keyboard (used with effects). |
| 1, 2, 3 | Red, green, blue (0–255) |
| 4 | Brightness 1–10 (the app's slider 0.1–1.0 × 10) |
| 5 | Effect speed 1–10 |
| 6 | Effect: 0 Always (static), 1 Twinkle, 2 Wave, 3 Breath, 4 Colour cycle, 5 Reactive, 6 Ripple, 7 Spiral rainbow, 8 Rainbow ripple, **254 = set one zone's colour** (with the zone mask), 255 = factory default |
| 7 | 1 = light on, 0 = light off |

How the app uses it:

- **A static colour per zone:** four calls with effect 254, each with one zone bit set, about 600 ms apart.
- **An effect:** one call with mask 0 and the effect number.
- **Off:** byte 7 = 0.

The firmware (`BST1`/`BST2`/`BST3`) picks the routine by the keyboard type it reads from the EC, then writes EC mailbox commands (ECMB 0x11 = mode, 0x18 = RGB).

### SetKBHotkey / GetKBHotkey: u16 bit flags

| Bit | Key | Meaning |
|---|---|---|
| 12 | Windows (Super) key | 1 = disabled |
| 13 | Function key (Fn-lock) | 1 = the app's toggle is "off" |
| 14 | Print Screen | 1 = disabled |

On Windows, "Function key" ON meant Fn+F-keys did nothing and Win+F-keys acted as the media keys. Read from Linux on 2026-11-26: value 0.

### Sleep timer: broken in the firmware

`WMDA` case 10 puts the value in the EC mailbox (ECMB 0x59) but never waits for the EC and never sets a status. It then falls through to `Return (Zero)`: an integer, not the buffer Windows expects. That is why every SetKBLedSleepTimer call failed on Windows with "Invalid object". The EC may still get the command. Untested from Linux.

### Touchpad toggle

There's no WMI call for this. The app sends **Ctrl+Win+F24**, the standard Windows precision-touchpad toggle.

## Power modes: `WMCC` / `WMBF` (bug on Linux)

The app calls `SetOemPowerSwitch` (`WMCC` method 1) with **1 = Quiet, 2 = Balanced, 3 = Performance**. The firmware stores that in the ACPI variable `ECPM`, copies it to EC register 0x7C (PWMD), and runs `E004`, which sets the AMD power limits (`E002` = ALIB):

| Mode | On AC (battery ≥ 50 %) | On battery |
|---|---|---|
| 3 Performance | STAPM 132 W, fast PPT 160 W, slow PPT 154 W | 30 W / 45 W / 40 W |
| 2 Balanced | 90 W / 115 W / 100 W | 30 W / 45 W / 40 W |
| 1 Quiet | 55 W / 81 W / 70 W | not read yet |

On AC with the battery below 50 % (and again below 20 %), Performance falls back to smaller limits.

`ECPM` starts at **3** (`Name (ECPM, 0x03)`) and `E004` runs when the EC comes up. So **Linux always boots in Performance mode**, whatever Windows last used. EC reg 0x7C read 0x03 on 2026-11-26.

`GetPowerModeState` (`WMBF` method 4, or `WMCC` method 2) returns `ECPM` at bytes 2–3. The `EmdAcpi_PowerModeControl` MOF names (1 Balanced, 2 Performance, 3 Quiet) don't match what the firmware does with them; trust the numbers above.

## Fans and temperature (bug on Linux; read the EC directly)

| | EC register | Read 2026-11-26 |
|---|---|---|
| Fan 1 RPM | 0x76 (high), 0x77 (low) | 0x0712 = 1810 rpm |
| Fan 2 RPM | 0x79 (high), 0x7A (low) | 0x07CE = 1998 rpm |
| Keyboard light level | 0x58 | 8 |
| Keyboard light flag | 0x59 | 0 |
| Power mode (PWMD) | 0x7C | 3 |
| Battery charge max / min (MXCP / MICP) | 0xBB / 0xBC | 0 / 0 (no limit) |

`FanControl` (`WMAF` method 2) writes 0x11 to EC 0x30, then 1 = auto, 2 = max, 3 = a third mode to EC 0x32, and checks each one reads back. The snowflake key does fan boost in hardware.

- **EC 0x30 (`XXTT`) / 0x32 (`TLID`) are a shared command mailbox, not fan registers.** 0x30 holds a command and 0x32 its argument. Fan control is command 0x11; other firmware methods (lid, `WMAE`, backlight) put 0xCC, 0x20, 0x21 and 0x24 in the same two registers (DSDT around 11211–11750 and 13285–13390). Those methods abort on Linux and `WMDA` never touches 0x30, so on Linux only nimo-wmi writes it, but 0x32 still isn't a reliable read-back of the fan mode.
- **The snowflake key is handled inside the EC.** No `_Qxx` handler writes 0x30/0x32 or raises an event, so Linux can't see a boost started from the key.
- nimo-wmi's `fan_mode` sends the same sequence (mode 3 isn't offered), forces Auto at every load (the firmware can leave Max, per emdoor-wmi), and sends the mode again after resume. It shows the last mode it set.
- **Max is one EC command with no per-fan argument** (`WMAF` case 2, DSDT 11799–11826): what each fan does is up to the EC.
- **The firmware never reads fan 2.** Only our driver reads 0x79/0x7A (`FN2H`/`FN2L`, DSDT 8749–8750). `WMAF` case 1 reads fan 1 only (DSDT 11789–11790), and so does NIMO's app (`WmiDataManager.cs:24-31`), so Windows never showed fan 2.
- **Mode 3 is "Stop"** (`WmiManager.cs:128`). nimo-wmi deliberately doesn't offer it.
- `FND1`/`FND2` (0x4B/0x4D) and the `FNS1`/`FNS2` bits (0x4F/0x4E bit 6) are unused by the firmware; probably fan duty and fan status.
- **Max pins fan 1 only** (owner-observed, 2026-11-27). With `fan_mode` = max, fan 1 held about 4000–4200 rpm constantly, while fan 2 kept following its own automatic curve: about 2000 rpm under load, dropping to 0 rpm during a light GPU load. So command 0x11 argument 2 boosts fan 1 and leaves fan 2 under the EC's automatic control.

The CPU temperature from `WMAD` is the same value `k10temp` already gives Linux.

## Battery charge limit (bug on Linux)

`SetBatteryChargeRationing` (`WMDD` method 2) writes N to EC 0xBB (stop charging at N %) and N−1 to 0xBC (start again below that). Control Center doesn't show it, but the firmware has it. This maps directly to Linux's standard `charge_control_end_threshold`.

0xBC on its own maps to `charge_control_start_threshold`. nimo-wmi writes it alone, only below 0xBB. Because the firmware's end write always sets start to end − 1, the driver remembers a lower start and an end write sets 0xBC to min(start, end − 1). At load it takes the remembered start from the EC only when 0xBC < 0xBB − 1. With no limit (0xBB = 0) a start write is only remembered.

**The EC honours a start below end − 1** (tested 2026-11-27, at 80 % on AC). With end 85 / start 75 it held, with no charging for 40 s. Raising the start to 82 started charging (about 39 W) within 5 s. Going back to 80 / 75 stopped it after a ~15 s overshoot to 81 %, and EC 0xBB/0xBC read back 80/75.

While it holds, BAT0 still reads `status=Charging`, with `power_now=0`. `power_now` has no sign (it's positive on battery too), so the app tells charging from holding by the AC adapter's `online` plus `power_now` ≥ 0.5 W, not by `status`.

## The firmware bug that blocks Linux

Every vendor method except `WMDA` starts like this:

```
Local0 = Buffer (0x08){}                 // 64 bits
CreateField (Local0, 0x20, 0x3F, RES3)   // bits 32..94: 31 bits past the end
```

Windows' ACPI interpreter lets this through. Linux's (ACPICA) stops the whole method before it does anything:

```
ACPI BIOS Error (bug): AE_AML_BUFFER_LIMIT, Field [RES3] at bit offset/length 32/63 exceeds size of target Buffer (64 bits)
ACPI Error: Aborting method \_SB.EWMI.WMDD due to previous error (AE_AML_BUFFER_LIMIT)
```

Affected: `WMAA WMAC WMAD WMAE WMAF WMBA WMBB WMBC WMBD WMBE WMBF WMCC WMDB WMDC WMDD WMDE WMDF`. The length should be 0x20.

Ways around it:

1. **Read-only things from the EC directly:** fan RPM and the current mode. This is safe, and a kernel driver can do it with `ec_read()`.
2. **A small SSDT add-on**, loaded from the initrd (`CONFIG_ACPI_TABLE_UPGRADE=y` on Ubuntu's kernel). It would add fixed copies of the few methods we need under new names, so NIMO's DSDT stays untouched.
3. A full patched DSDT from the initrd. This works, but it has to be redone for every BIOS update.
4. Ask NIMO/Emdoor to fix it in the BIOS: one byte per method.

## Tested from Linux so far (read-only, via `acpi_call`)

| Call | Result |
|---|---|
| `WMDA` 8 GetKBHotkey | OK, value 0 |
| `WMDA` 11 GetKBLightState | OK, 0 |
| `WMDA` 12 GetKBLightLevel | OK, 8 |
| `WMDA` 4 GetKBLightingEffect | OK, returns a table |
| `WMAF` 1, `WMAD` 1/3, `WMBF` 4, `WMCC` 2, `WMDD` 1 | AE_AML_BUFFER_LIMIT (the bug) |

No set calls have been made from Linux yet.
