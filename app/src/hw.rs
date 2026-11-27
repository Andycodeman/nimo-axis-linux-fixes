//! Talking to the nimo-wmi driver (sysfs) and power-profiles-daemon (D-Bus).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::update;

/// The keyboard's WMI GUID. Since kernel 6.17 the device is named `<GUID>-<N>`
/// (N handed out by the kernel) unless CONFIG_ACPI_WMI_LEGACY_DEVICE_NAMES=y,
/// so never hard-code the directory: look it up with [`wmi_dev`].
const WMI_GUID: &str = "8600ACCE-FB9B-443E-86F4-3C867398AAE5";
const LED: &str = "/sys/class/leds/nimo::kbd_backlight";

/// Effect names as the driver's `kbd_effect` file spells them, in firmware order.
pub const EFFECTS: [&str; 9] = [
    "static",
    "twinkle",
    "wave",
    "breath",
    "cycle",
    "reactive",
    "ripple",
    "spiral_rainbow",
    "rainbow_ripple",
];

/// Human names for the same list.
pub const EFFECT_LABELS: [&str; 9] = [
    "Always",
    "Twinkle",
    "Wave",
    "Breath",
    "Cycle",
    "Reactive",
    "Ripple",
    "Rainbow",
    "Rainbow ripple",
];

/// Keyboard sleep timer choices (seconds), as in Control Center. 0 = never.
pub const SLEEP_TIMES: [u16; 9] = [60, 300, 600, 900, 1200, 1800, 3600, 7200, 0];

/// Effects that only light keys as you press them.
pub const TYPING_EFFECTS: [usize; 3] = [5, 6, 8];

/// Power profiles as power-profiles-daemon names them, Quiet → Performance.
pub const PROFILES: [&str; 3] = ["power-saver", "balanced", "performance"];
pub const PROFILE_LABELS: [&str; 3] = ["Quiet", "Balanced", "Performance"];

/// Hotkey toggles: driver file name, label.
pub const KEYS: [(&str, &str); 3] = [
    ("win_key", "Windows key"),
    ("fn_key", "Function key"),
    ("prtsc_key", "Print Screen key"),
];

/// The nimo-wmi device directory under `sys`: the first entry named after the
/// GUID (bare, or with the kernel's `-N` suffix) that has `kbd_effect`. Devices
/// our driver bound come first, then any on the WMI bus.
fn find_wmi_dev(sys: &Path) -> Option<PathBuf> {
    ["bus/wmi/drivers/nimo-wmi", "bus/wmi/devices"]
        .iter()
        .find_map(|dir| {
            fs::read_dir(sys.join(dir))
                .ok()?
                .flatten()
                .filter(|e| {
                    let name = e.file_name();
                    let name = name.to_string_lossy();
                    name == WMI_GUID
                        || name
                            .strip_prefix(WMI_GUID)
                            .is_some_and(|rest| rest.starts_with('-'))
                })
                .map(|e| e.path())
                .filter(|p| p.join("kbd_effect").exists())
                .min()
        })
}

/// Looked up on every call: the name can change if the WMI bus re-registers.
fn wmi_dev() -> Option<PathBuf> {
    find_wmi_dev(Path::new("/sys"))
}

/// Path of one of the driver's keyboard files.
fn kbd(file: &str) -> io::Result<PathBuf> {
    wmi_dev()
        .map(|d| d.join(file))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the nimo-wmi driver isn't loaded"))
}

pub fn driver_loaded() -> bool {
    wmi_dev().is_some()
}

/// The running nimo-wmi build's `srcversion`; it changes when an upgrade reloads the driver.
pub fn driver_srcversion() -> Option<String> {
    read("/sys/module/nimo_wmi/srcversion").ok()
}

fn read(path: impl AsRef<Path>) -> io::Result<String> {
    Ok(fs::read_to_string(path)?.trim().to_string())
}

fn write(path: impl AsRef<Path>, value: &str) -> io::Result<()> {
    let path = path.as_ref();
    fs::write(path, value).map_err(|e| {
        let hint = if e.kind() == io::ErrorKind::PermissionDenied {
            " (is the udev rule installed and are you in the 'nimo' group? log out and back in after adding it)"
        } else {
            ""
        };
        io::Error::new(e.kind(), format!("{}: {e}{hint}", path.display()))
    })
}

// ---- lighting ---------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Lighting {
    pub on: bool,
    pub brightness: u8, // 1-10
    pub effect: usize,  // index into EFFECTS
    pub speed: u8,      // 1-10
    pub zones: [[u8; 3]; 4],
    pub bars: [[u8; 3]; 2], // left, right light bar
    pub sleep: u16,         // seconds without typing before the light goes off, 0 = never
}

/// Colour presets: (name, [left bar, zone 1-4, right bar]).
pub const PRESETS: [(&str, [u32; 6]); 5] = [
    ("Rainbow", [0xff0000, 0xff8000, 0xffe600, 0x00e040, 0x0080ff, 0x8000ff]),
    ("Ocean", [0x00ffd0, 0x00e0ff, 0x00a0ff, 0x0070ff, 0x3040ff, 0x6020ff]),
    ("Sunset", [0xff0040, 0xff2a00, 0xff6a00, 0xffa000, 0xff40a0, 0xa020ff]),
    ("Forest", [0x00ff60, 0x20ff00, 0x80ff00, 0x00c040, 0x00ffa0, 0x40ff80]),
    ("White", [0xffffff; 6]),
];

fn rgb(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

impl Lighting {
    pub fn set_preset(&mut self, idx: usize) {
        let c = PRESETS[idx.min(PRESETS.len() - 1)].1;
        self.bars = [rgb(c[0]), rgb(c[5])];
        for z in 0..4 {
            self.zones[z] = rgb(c[z + 1]);
        }
    }
}

impl Default for Lighting {
    fn default() -> Self {
        // Rainbow, static, fairly bright.
        let mut l = Lighting {
            on: true,
            brightness: 8,
            effect: 0,
            speed: 5,
            zones: [[0; 3]; 4],
            bars: [[0; 3]; 2],
            sleep: 0,
        };
        l.set_preset(0);
        l
    }
}

fn hex(c: [u8; 3]) -> String {
    format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn set_on(l: &Lighting) -> io::Result<()> {
    let level = if l.on { l.brightness.max(1) } else { 0 };
    write(Path::new(LED).join("brightness"), &level.to_string())
}

pub fn set_brightness(l: &Lighting) -> io::Result<()> {
    set_on(l)
}

pub fn set_speed(l: &Lighting) -> io::Result<()> {
    write(kbd("kbd_speed")?, &l.speed.to_string())
}

/// Colours (and with them static mode), then the effect on top if it isn't static.
pub fn set_colours_and_effect(l: &Lighting) -> io::Result<()> {
    let zones: Vec<String> = l.zones.iter().map(|z| hex(*z)).collect();
    write(kbd("kbd_zone_colors")?, &zones.join(" "))?;
    write(
        kbd("kbd_lightbar_colors")?,
        &format!("{} {}", hex(l.bars[0]), hex(l.bars[1])),
    )?;
    if l.effect != 0 {
        set_speed(l)?;
        write(kbd("kbd_effect")?, EFFECTS[l.effect])?;
    }
    Ok(())
}

pub fn set_sleep(l: &Lighting) -> io::Result<()> {
    write(kbd("kbd_sleep_timer")?, &l.sleep.to_string())
}

pub fn set_effect(l: &Lighting) -> io::Result<()> {
    if l.effect == 0 {
        set_colours_and_effect(l)
    } else {
        set_speed(l)?;
        write(kbd("kbd_effect")?, EFFECTS[l.effect])
    }
}

/// Send the whole lighting state (used at start-up: the firmware forgets it).
/// Send the whole lighting state (at start-up and on Save: the keyboard forgets
/// it across reboots). The effect goes last, because other commands stop it.
pub fn apply_lighting(l: &Lighting) -> io::Result<()> {
    set_sleep(l)?;
    set_on(l)?;
    if l.on {
        set_colours_and_effect(l)?;
    }
    Ok(())
}

// ---- keys, battery, sensors -------------------------------------------------

pub fn key_enabled(file: &str) -> Option<bool> {
    kbd(file).and_then(read).ok().map(|v| v == "1")
}

pub fn set_key(file: &str, enabled: bool) -> io::Result<()> {
    write(kbd(file)?, if enabled { "1" } else { "0" })
}

fn battery() -> Option<PathBuf> {
    let dir = fs::read_dir("/sys/class/power_supply").ok()?;
    dir.flatten()
        .map(|e| e.path())
        .find(|p| p.join("charge_control_end_threshold").exists())
}

pub fn charge_limit() -> Option<u8> {
    read(battery()?.join("charge_control_end_threshold")).ok()?.parse().ok()
}

pub fn set_charge_limit(pct: u8) -> io::Result<()> {
    let bat = battery().ok_or_else(|| io::Error::other("no battery with a charge limit"))?;
    write(bat.join("charge_control_end_threshold"), &pct.to_string())
}

/// Start charging again below this % (0 while there's no limit).
pub fn charge_start() -> Option<u8> {
    read(battery()?.join("charge_control_start_threshold")).ok()?.parse().ok()
}

/// Whether the battery waits to drop below a lower start before recharging.
/// The firmware's own behaviour is start = end - 1 (recharge as soon as it
/// slips), so only a start below that, with a limit on, counts.
pub fn waits_to_recharge(end: u8, start: u8) -> bool {
    end < 100 && start > 0 && start + 1 < end
}

/// Needs the driver's end threshold set first: it must stay below it.
pub fn set_charge_start(pct: u8) -> io::Result<()> {
    let bat = battery().ok_or_else(|| io::Error::other("no battery with a charge limit"))?;
    write(bat.join("charge_control_start_threshold"), &pct.to_string())
}

// ---- fans -------------------------------------------------------------------

/// The driver's `fan_mode` values; the index is the app's fan mode.
pub const FAN_MODES: [&str; 2] = ["auto", "max"];

/// The selected mode in a `fan_mode` read such as `[auto] max`.
fn parse_fan_mode(text: &str) -> Option<usize> {
    let selected = text.split_whitespace().find_map(|w| w.strip_prefix('[')?.strip_suffix(']'))?;
    FAN_MODES.iter().position(|m| *m == selected)
}

/// The mode the driver last set. The snowflake key boosts the fans without
/// the driver seeing it, so this doesn't show that.
pub fn fan_mode() -> Option<usize> {
    parse_fan_mode(&kbd("fan_mode").and_then(read).ok()?)
}

pub fn set_fan_mode(i: usize) -> io::Result<()> {
    let mode = FAN_MODES.get(i).ok_or_else(|| io::Error::other("unknown fan mode"))?;
    write(kbd("fan_mode")?, mode)
}

fn hwmon(name: &str) -> Option<PathBuf> {
    let dir = fs::read_dir("/sys/class/hwmon").ok()?;
    dir.flatten()
        .map(|e| e.path())
        .find(|p| read(p.join("name")).map(|n| n == name).unwrap_or(false))
}

/// (fan 1, fan 2) in rpm.
pub fn fans() -> (u32, u32) {
    let Some(h) = hwmon("nimo") else { return (0, 0) };
    let f = |n| read(h.join(format!("fan{n}_input"))).ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    (f(1), f(2))
}

// ---- power profile (power-profiles-daemon) ------------------------------------

fn ppd() -> zbus::Result<zbus::blocking::Proxy<'static>> {
    let conn = zbus::blocking::Connection::system()?;
    zbus::blocking::Proxy::new_owned(
        conn,
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
        "org.freedesktop.UPower.PowerProfiles",
    )
}

/// Index into PROFILES.
pub fn profile() -> Option<usize> {
    let p: String = ppd().ok()?.get_property("ActiveProfile").ok()?;
    PROFILES.iter().position(|x| *x == p)
}

pub fn set_profile(idx: usize) -> io::Result<()> {
    let proxy = ppd().map_err(io::Error::other)?;
    proxy
        .set_property("ActiveProfile", PROFILES[idx])
        .map_err(|e| io::Error::other(format!("power-profiles-daemon: {e}")))
}

// ---- saved lighting -----------------------------------------------------------

fn config_file(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("nimo-control-center").join(name))
}

fn write_config(name: &str, text: &str) -> io::Result<()> {
    let path = config_file(name).ok_or_else(|| io::Error::other("no config dir"))?;
    write_file(&path, text)
}

fn write_file(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, text)
}

/// Apply one `key=value` line of a saved look to `l`.
fn parse_setting(l: &mut Lighting, k: &str, v: &str) {
    let v = v.trim();
    match k.trim() {
        "on" => l.on = v == "1",
        "brightness" => l.brightness = v.parse().unwrap_or(l.brightness).clamp(1, 10),
        "sleep" => l.sleep = v.parse().unwrap_or(0),
        "speed" => l.speed = v.parse().unwrap_or(l.speed).clamp(1, 10),
        "effect" => {
            if let Some(i) = EFFECTS.iter().position(|e| *e == v) {
                l.effect = i
            }
        }
        "bars" => {
            for (i, b) in v.split_whitespace().take(2).enumerate() {
                if let Some(c) = parse_hex(b) {
                    l.bars[i] = c;
                }
            }
        }
        "zones" => {
            for (i, z) in v.split_whitespace().take(4).enumerate() {
                if let Some(c) = parse_hex(z) {
                    l.zones[i] = c;
                }
            }
        }
        _ => {}
    }
}

fn format_look(l: &Lighting) -> String {
    let zones: Vec<String> = l.zones.iter().map(|z| hex(*z)).collect();
    format!(
        "brightness={}\neffect={}\nspeed={}\nzones={}\nbars={} {}\n",
        l.brightness,
        EFFECTS[l.effect],
        l.speed,
        zones.join(" "),
        hex(l.bars[0]),
        hex(l.bars[1])
    )
}

pub fn load_lighting() -> Lighting {
    let mut l = Lighting::default();
    if let Some(text) = config_file("lighting.conf").and_then(|p| fs::read_to_string(p).ok()) {
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                parse_setting(&mut l, k, v);
            }
        }
    }
    l
}

pub fn save_lighting(l: &Lighting) -> io::Result<()> {
    write_config("lighting.conf", &format!("on={}\nsleep={}\n{}", l.on as u8, l.sleep, format_look(l)))
}

// ---- your own presets ---------------------------------------------------------

/// A saved look: colours, light bars, effect, speed and brightness.
#[derive(Clone)]
pub struct UserPreset {
    pub name: String,
    pub look: Lighting,
}

impl UserPreset {
    /// Whether `l` shows this preset.
    pub fn matches(&self, l: &Lighting) -> bool {
        let p = &self.look;
        p.zones == l.zones && p.bars == l.bars && p.effect == l.effect && p.speed == l.speed && p.brightness == l.brightness
    }

    /// Put this preset's look on `l`, keeping on/off and the sleep timer.
    pub fn apply_to(&self, l: &mut Lighting) {
        let p = &self.look;
        l.zones = p.zones;
        l.bars = p.bars;
        l.effect = p.effect;
        l.speed = p.speed;
        l.brightness = p.brightness;
    }
}

/// presets.conf: a `[name]` line, then the look's `key=value` lines.
pub fn load_presets() -> Vec<UserPreset> {
    let mut out: Vec<UserPreset> = Vec::new();
    let Some(text) = config_file("presets.conf").and_then(|p| fs::read_to_string(p).ok()) else {
        return out;
    };
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push(UserPreset { name: name.to_string(), look: Lighting::default() });
        } else if let (Some(p), Some((k, v))) = (out.last_mut(), line.split_once('=')) {
            parse_setting(&mut p.look, k, v);
        }
    }
    out
}

pub fn save_presets(presets: &[UserPreset]) -> io::Result<()> {
    let text: String = presets
        .iter()
        .map(|p| format!("[{}]\n{}\n", p.name.replace(['[', ']', '\n'], ""), format_look(&p.look)))
        .collect();
    write_config("presets.conf", &text)
}

// ---- update check ---------------------------------------------------------------

/// update.conf: the "Check for updates" switch and what the last check found.
pub fn load_update() -> update::State {
    config_file("update.conf").map(|p| load_update_at(&p)).unwrap_or_default()
}

pub fn save_update(s: &update::State) -> io::Result<()> {
    let path = config_file("update.conf").ok_or_else(|| io::Error::other("no config dir"))?;
    save_update_at(&path, s)
}

fn load_update_at(path: &Path) -> update::State {
    fs::read_to_string(path).map(|t| update::State::parse(&t)).unwrap_or_default()
}

fn save_update_at(path: &Path, s: &update::State) -> io::Result<()> {
    write_file(path, &s.format())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn waiting_to_recharge_needs_a_start_below_end_minus_one() {
        assert!(waits_to_recharge(80, 75));
        assert!(waits_to_recharge(80, 78));
        // The firmware's own end - 1, the driver's default.
        assert!(!waits_to_recharge(80, 79));
        // No limit: the start reads 0.
        assert!(!waits_to_recharge(100, 0));
        assert!(!waits_to_recharge(100, 75));
        // A limit with no start read back (a driver older than 0.8).
        assert!(!waits_to_recharge(80, 0));
    }

    /// A throwaway sysfs root, removed when dropped.
    struct FakeSys(PathBuf);

    impl FakeSys {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("nimo-cc-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("bus/wmi/devices")).unwrap();
            fs::create_dir_all(root.join("bus/wmi/drivers/nimo-wmi")).unwrap();
            FakeSys(root)
        }

        /// A WMI device directory, with `kbd_effect` if `ours`.
        fn device(&self, name: &str, ours: bool) -> PathBuf {
            let dir = self.0.join("bus/wmi/devices").join(name);
            fs::create_dir_all(&dir).unwrap();
            if ours {
                fs::write(dir.join("kbd_effect"), "static\n").unwrap();
            }
            dir
        }

        /// The driver's link to a device, as the kernel makes it on bind.
        fn bind(&self, name: &str) {
            let wmi = self.0.join("bus/wmi");
            symlink(wmi.join("devices").join(name), wmi.join("drivers/nimo-wmi").join(name)).unwrap();
        }
    }

    impl Drop for FakeSys {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn legacy_bare_name() {
        let sys = FakeSys::new("legacy");
        sys.device(WMI_GUID, true);
        sys.bind(WMI_GUID);
        let dev = find_wmi_dev(&sys.0).unwrap();
        assert_eq!(dev.file_name().unwrap(), WMI_GUID);
        assert!(dev.join("kbd_effect").exists());
    }

    #[test]
    fn numbered_name() {
        let sys = FakeSys::new("numbered");
        let name = format!("{WMI_GUID}-12");
        sys.device(&name, true);
        sys.bind(&name);
        assert_eq!(find_wmi_dev(&sys.0).unwrap().file_name().unwrap().to_str(), Some(name.as_str()));
    }

    #[test]
    fn skips_copies_without_our_files() {
        let sys = FakeSys::new("copies");
        sys.device(&format!("{WMI_GUID}-3"), false);
        sys.device(&format!("{WMI_GUID}-4"), true);
        let dev = find_wmi_dev(&sys.0).unwrap();
        assert_eq!(dev, sys.0.join("bus/wmi/devices").join(format!("{WMI_GUID}-4")));
    }

    #[test]
    fn driver_link_wins_over_devices() {
        let sys = FakeSys::new("driver-first");
        sys.device(&format!("{WMI_GUID}-1"), true);
        sys.device(&format!("{WMI_GUID}-9"), true);
        sys.bind(&format!("{WMI_GUID}-9"));
        let dev = find_wmi_dev(&sys.0).unwrap();
        assert_eq!(dev, sys.0.join("bus/wmi/drivers/nimo-wmi").join(format!("{WMI_GUID}-9")));
    }

    #[test]
    fn other_guids_and_lookalikes_ignored() {
        let sys = FakeSys::new("others");
        sys.device("05901221-D566-11D1-B2F0-00A0C9062910-2", true);
        sys.device(&format!("{WMI_GUID}X"), true);
        assert_eq!(find_wmi_dev(&sys.0), None);
    }

    #[test]
    fn update_conf_round_trip() {
        let dir = std::env::temp_dir().join(format!("nimo-cc-test-{}-update", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("nimo-control-center/update.conf");
        assert_eq!(load_update_at(&path), update::State::default(), "no file yet");

        let s = update::State {
            check: false,
            latest: "1.3.0".into(),
            notes: vec!["New.".into()],
            skipped: "1.2.9".into(),
            ..Default::default()
        };
        save_update_at(&path, &s).unwrap();
        assert_eq!(load_update_at(&path), s, "creates the directory and reads back");

        fs::write(&path, "garbage\0\nlatest=2.0.0\n").unwrap();
        assert_eq!(load_update_at(&path).latest, "2.0.0");
        assert!(load_update_at(&path).check);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fan_mode_parsing() {
        assert_eq!(parse_fan_mode("[auto] max"), Some(0));
        assert_eq!(parse_fan_mode("auto [max]\n"), Some(1));
        assert_eq!(parse_fan_mode("auto max"), None, "nothing selected");
        assert_eq!(parse_fan_mode("auto [turbo]"), None, "unknown mode");
        assert_eq!(parse_fan_mode(""), None);
        assert_eq!(FAN_MODES.iter().position(|m| *m == "max"), Some(1));
    }

    #[test]
    fn nothing_present() {
        let sys = FakeSys::new("empty");
        assert_eq!(find_wmi_dev(&sys.0), None);
        let _ = fs::remove_dir_all(sys.0.join("bus"));
        assert_eq!(find_wmi_dev(&sys.0), None);
    }
}
