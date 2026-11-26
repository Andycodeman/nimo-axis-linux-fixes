# Windows session: Control Center capture + driver install

Instructions for Claude Code running **on Windows** on the NIMO AXIS 395 ("nimo-windows"). Work through the steps in order and log what you did. Ask the person for anything only they can do (clicking in an app, typing a key, UAC prompts).

## Ground rules

- **Never write secrets anywhere:** no product key, passwords or tokens in files, logs, commits or this repo. The repo will be made public.
- **Raw data goes in `C:\nimo-setup\captures\` only**, never in the repo. Linux reads it straight off the disk later.
- **The USB capture records the keyboard.** While it runs, the person must not type passwords or anything private. Tell them this before you start it.
- **Don't touch disks, partitions, the boot setup (GRUB/EFI), BitLocker or Secure Boot.** Don't run any NIMO factory recovery image: it wipes the drive, including Linux.
- Install **only** the RGB Control Center (this laptop has the RGB keyboard), never the White one.
- Work in an **elevated** PowerShell (Run as administrator).
- When finished, **shut down normally**. Fast Startup and hibernation are off and must stay off; otherwise Linux can't safely read the disk.

## What's already on disk

```
C:\nimo-setup\
  nimo-pack\03_Audio\...                       Realtek audio driver 6.0.9823.1 (matches SUBSYS_2782A246)
  nimo-pack\07_FP\...                          Microarray fingerprint driver 16.8.21.356 (lists 3274:8012)
  nimo-pack\10_Tool\03_ControlCenter\RGB_Keyboard_Backlight\Control Center_Setup_V1.0.8_20260818.exe
  repo\                                        a copy of this repo
  captures\                                    (create it) your output
```

Don't install anything else from NIMO's pack: the Wi-Fi/BT and graphics drivers there are older than what's installed.

## Step 1: Activate Windows

The edition is Windows 11 Pro. Ask the person to paste the **Pro** key from NIMO's email (Joyce, 2026-11-26) into the terminal. Don't save it.

```powershell
slmgr /ipk <key>      # the person runs this or pastes the key
slmgr /ato
slmgr /xpr
```

Write only the result (activated or not, and the error code if it failed) to `captures\01-activation.txt`.

## Step 2: Baseline snapshot (before Control Center)

```powershell
$C = 'C:\nimo-setup\captures'; New-Item -ItemType Directory -Force "$C\02-baseline" | Out-Null
pnputil /enum-drivers                                  > "$C\02-baseline\drivers.txt"
Get-PnpDevice | Sort-Object Class,FriendlyName | Export-Csv "$C\02-baseline\pnp.csv" -NoTypeInformation
Get-Service | Sort-Object Name | Select-Object Name,DisplayName,Status,StartType | Export-Csv "$C\02-baseline\services.csv" -NoTypeInformation
Get-CimInstance Win32_SystemDriver | Select-Object Name,PathName,State,StartMode | Export-Csv "$C\02-baseline\kernel-drivers.csv" -NoTypeInformation
Get-ScheduledTask | Select-Object TaskPath,TaskName,State | Export-Csv "$C\02-baseline\tasks.csv" -NoTypeInformation
Get-CimClass -Namespace root\wmi | Select-Object CimClassName | Export-Csv "$C\02-baseline\wmi-classes.csv" -NoTypeInformation
reg export HKLM\SOFTWARE "$C\02-baseline\hklm-software.reg" /y
reg export HKLM\SYSTEM\CurrentControlSet\Services "$C\02-baseline\services.reg" /y
Get-ChildItem 'C:\Program Files','C:\Program Files (x86)','C:\ProgramData' -Directory | Select-Object FullName > "$C\02-baseline\program-dirs.txt"
```

## Step 3: Get the capture tools (official sources only)

- **Process Monitor**: https://download.sysinternals.com/files/ProcessMonitor.zip (Microsoft). Unzip to `C:\nimo-setup\tools\procmon\`.
- **Wireshark with USBPcap**: https://www.wireshark.org/download.html (Windows x64 installer). Tick **USBPcap** during setup. A reboot is needed for USBPcap.
- The WMI tracing is built into Windows; nothing to download.

## Step 4: Capture the Control Center install

```powershell
$C = 'C:\nimo-setup\captures'; New-Item -ItemType Directory -Force "$C\10-install" | Out-Null
$P = 'C:\nimo-setup\tools\procmon\Procmon64.exe'
& $P /AcceptEula /Quiet /Minimized /BackingFile "$C\10-install\install.pml"
Start-Sleep 5
```

Ask the person to run `Control Center_Setup_V1.0.8_20260818.exe` and click through it. Record every choice in `10-install\notes.md`, but **don't** reboot yet if it asks. Then:

```powershell
& $P /Terminate
```

Repeat the Step 2 snapshot into `captures\11-after-install\`. Then:

- Find where it installed (diff `program-dirs.txt`, check the new services and kernel drivers). Copy the **whole install folder** to `captures\10-install\files\`, plus any new `.sys` drivers it added (from `C:\Windows\System32\drivers`) and any files it dropped in `C:\ProgramData`.
- For every `.exe`/`.dll` there, record whether it is .NET or native:
  ```powershell
  Get-ChildItem "$C\10-install\files" -Recurse -Include *.exe,*.dll | ForEach-Object {
    $net = try { [Reflection.AssemblyName]::GetAssemblyName($_.FullName) | Out-Null; 'dotnet' } catch { 'native' }
    "{0}`t{1}`t{2}" -f $net, $_.VersionInfo.FileVersion, $_.FullName } > "$C\10-install\binaries.txt"
  ```
- Reboot now if the installer asked for it.

## Step 5: Capture every Control Center setting

Start all three captures, then work through the checklist with the person.

```powershell
$C = 'C:\nimo-setup\captures\20-runtime'; New-Item -ItemType Directory -Force $C | Out-Null
# 1. Process Monitor (file / registry / process activity)
& 'C:\nimo-setup\tools\procmon\Procmon64.exe' /AcceptEula /Quiet /Minimized /BackingFile "$C\runtime.pml"
# 2. WMI activity (which WMI classes and methods the app calls)
wevtutil sl Microsoft-Windows-WMI-Activity/Trace /e:true /q:true
logman start nimo-wmi -p Microsoft-Windows-WMI-Activity 0xFFFFFFFFFFFFFFFF 0xff -o "$C\wmi.etl" -ets
```

3. **USB capture of the ITE keyboard controller (USB 048d:8910).** Run `USBPcapCMD.exe` (in the Wireshark/USBPcap folder) once with no arguments: it lists the root hubs and their devices. Find the hub that has 048d:8910 and capture only that hub:

```powershell
& 'C:\Program Files\USBPcap\USBPcapCMD.exe' -d \\.\USBPcap<N> -o "$C\usb-ite.pcap" -A
```

(Run it in its own window and stop it with Ctrl+C at the end.)

**The checklist.** Open Control Center. For **each** item, write a line in `$C\actions.log` **before** the person clicks: `HH:mm:ss.fff  <what is about to be set>`. Then ask them to make that one change, and wait ~3 seconds before the next one. Use `Get-Date -Format 'HH:mm:ss.fff'` for the time.

- **Keyboard RGB:** off; each brightness level, lowest to highest; static colour red, then green, then blue, then white, then one custom colour (note its hex); every effect/mode, one at a time; each speed setting for one effect; direction, if there is one; per-zone or per-key colour, if offered (set one zone to a distinct colour).
- **Performance / power / fan modes:** every mode one at a time (e.g. quiet / balanced / performance / custom); every custom fan-curve option, if any.
- **Every other toggle in the app**, both ways: battery charge limit, Fn lock, Windows-key lock, touchpad lock, boot sound, display overdrive, and so on. Screenshot each page of the app into `$C\screens\`.
- **Hotkeys outside the app:** each Fn+key combo that changes the backlight, fan/performance mode, mic, airplane mode or the touchpad.

Then stop everything:

```powershell
& 'C:\nimo-setup\tools\procmon\Procmon64.exe' /Terminate
logman stop nimo-wmi -ets
wevtutil epl Microsoft-Windows-WMI-Activity/Operational "$C\wmi-operational.evtx"
wevtutil sl Microsoft-Windows-WMI-Activity/Trace /e:false /q:true
# Ctrl+C the USBPcapCMD window
```

Also save the current values of the app's WMI classes, if Step 4 showed new or used ones (for example in `root\wmi`):

```powershell
Get-CimClass -Namespace root\wmi | Where-Object CimClassName -notmatch '^(MS|MSNdis|MSStorage|WMI|Win32|__|CIM|Msft)' |
  ForEach-Object { "== $($_.CimClassName)"; try { Get-CimInstance -Namespace root\wmi -ClassName $_.CimClassName | Format-List * | Out-String } catch { "  (no instances: $_)" } } > "$C\wmi-vendor-instances.txt"
```

## Step 6: Install the remaining drivers (after the captures)

Keep these after the captures so they don't muddy them.

```powershell
cd 'C:\nimo-setup\nimo-pack\03_Audio\9823.1_UAD_WHQL_2025_0418_2782A246'
cmd /c InstallPackage.bat        # pnputil -a ... /install for each Realtek INF; press a key at the final "pause"
cd 'C:\nimo-setup\nimo-pack\07_FP\Signed_1152921505700150357\drivers\40bd9003-fca5-4731-b752-20bae32118e6'
pnputil /add-driver MafpWinbioDriver.inf /install
```

Reboot, then check:

- `Get-PnpDevice -Class MEDIA,Biometric | Format-Table Status,FriendlyName,InstanceId`: the codec (`...SUBSYS_2782A246...`) should now show a Realtek driver (6.0.9823.1), and the fingerprint reader the Microarray driver (16.8.21.356).
- Ask the person to play a sound (speakers) and record a few seconds with Voice Recorder (internal mic). Note whether each works.
- Optionally, the person enrols a finger in Windows Hello.

Save `pnputil /enum-drivers > captures\30-after-drivers.txt`.

## Step 7: Write the summary and shut down

- Write `captures\SUMMARY.md`: what was done and when, the install path, the binary list (.NET/native), new services and kernel drivers, the WMI classes and methods seen, the USB hub/device captured, the activation result, the audio/mic/fingerprint test results, and anything odd. **No key, no personal paths beyond the user folder name.**
- Optionally commit a *cleaned-up* copy of the summary to `repo\notes\windows-capture-YYYYMMDD.md` and push, if `gh` is logged in on Windows. Otherwise Linux will do it.
- Shut down with `shutdown /s /t 0`. Don't hibernate or sleep.
