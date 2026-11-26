# Windows capture session, 2026-11-26

A cleaned-up copy of the summary that Claude Code wrote on Windows while following [windows/INSTRUCTIONS.md](../windows/INSTRUCTIONS.md). The raw captures (Procmon, WMI ETW, USB pcap, registry dumps) stay on the laptop's Windows partition and are not published. The USB capture contains keystrokes.

What the captures mean for Linux is in [control-center-protocol.md](control-center-protocol.md).

## What was done

| Time | Step |
|---|---|
| before 01:51 | Activation attempt; baseline snapshot; Procmon and Wireshark 4.6.10 + USBPcap downloaded from official sources, signatures checked |
| 01:53–01:56 | Control Center RGB v1.0.8 installed under Procmon, then an after-install snapshot |
| 02:00–02:34 | Runtime capture (Procmon + WMI-Activity ETW + USBPcap on the ITE keyboard), a 72-step checklist covering every setting |
| 02:38 | NIMO's fingerprint and Realtek audio drivers installed |
| 02:40 | Reboot; hardware tests |

## Control Center install

- Inno Setup installer, signed by Emdoor Digital Technology Co., Ltd. SHA-256 `0FDBEC8C54180A3B21046D8D6E4DC162AC070A32F950AB85B2C51A0C5601A06D`.
- Installs to `C:\Users\Public\App\ControlCenter\`, plus one new service, `ControlCenterService` (Automatic, LocalSystem).
- **No kernel drivers, INFs, new WMI classes or scheduled tasks.** The WMI classes it uses come with the BIOS.
- Everything is .NET apart from the Inno uninstaller: `ASHControlCenter.exe` (UI), `ControlCenterService.exe`, `CommonFunctionLib.dll`, `HttpUnitsLib.dll`, and third-party libraries (HidSharp, LibreHardwareMonitorLib, NAudio, SharpDX, Newtonsoft.Json and others).
- Factory tools already installed: KeyDetect (hotkey OSD) and PowerModeUtil. Both are .NET too.

## WMI calls seen

All of them are on `ACPI\PNP0C14\EmdWmi_0`, namespace `root\wmi`, and they come from the service, not the UI.

| Class::Method | Calls | Trigger |
|---|---|---|
| EmdAcpi_AppKBLedControl::Set4ZoneKBLed | 96 | Every lighting Save |
| EmdAcpi_AppKBLedControl::SetKBLedSleepTimer | 6 | Keyboard sleep timer. **All failed** ("Invalid object"): a firmware bug |
| EmdAcpi_AppKBLedControl::SetKBHotkey | 6 | Windows key / Print Screen / Function key toggles |
| SetOemPowerSwitch::SetOemPowerSwitch | 3 + 3 | Power mode. PowerModeUtil repeats each call |
| GetCpuTemperature, GetFanSpeed, GetPowerModeState | ~1900 each | Polled every ~1 s |

No WMI calls for speaker mute (Windows audio API) or the touchpad toggle (a Ctrl+Win+F24 keypress).

**USB:** only keyboard reports and one keyboard-LED output report. No lighting traffic over USB.

## Lighting

4 zones, not per-key. Effects: Always, Breath, Rainbow, Ripple, Cycle, Twinkle (the code has more). Speed and brightness sliders. Changes reach the EC only when **Save** is clicked.

## Hotkeys

- Control Center's **"Function key" toggle is Fn-lock**. With it on, Fn+F-keys do nothing and Win+F-keys act as the hotkeys. With it off, Fn+F1 etc. work as usual.
- Fn+Space = keyboard light on/off. Snowflake key = fan boost on/off. Gear key = opens Settings. Mic key = mic mute.

## Drivers and hardware tests

- Microarray fingerprint driver 16.8.21.356 on USB 3274:8012: OK. Windows Hello enrolment works.
- Realtek audio 6.0.9823.1 on `SUBSYS_2782A246`: OK. Speakers and the internal mic both work.

## Oddities

1. **KeyDetect logs every keystroke**, with timestamps, in plain text to `C:\Users\Public\App\OSD\KeyDetect\Log\<date>.txt`, readable by any user. The log was deleted and `KeyDetectService` disabled. To undo: `Set-Service KeyDetectService -StartupType Automatic; Start-Service KeyDetectService`.
2. The keyboard sleep timer never works on Windows (see above).
3. The firmware has a battery charge limit (`EmdAcpi_BatteryChargeRationing`) that Control Center doesn't offer.
