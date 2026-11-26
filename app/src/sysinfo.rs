//! Live readings (Dashboard) and static system details (System Information),
//! all from /proc and /sys so nothing needs root.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_num(path: impl AsRef<Path>) -> Option<f64> {
    read(path)?.parse().ok()
}

fn hwmon(name: &str) -> Option<PathBuf> {
    fs::read_dir("/sys/class/hwmon")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| read(p.join("name")).as_deref() == Some(name))
}

/// The amdgpu device directory (/sys/class/drm/cardN/device).
fn gpu_device() -> Option<PathBuf> {
    fs::read_dir("/sys/class/drm")
        .ok()?
        .flatten()
        .map(|e| e.path().join("device"))
        .find(|p| p.join("gpu_busy_percent").exists())
}

#[derive(Default, Clone, Copy)]
pub struct Metrics {
    pub cpu_mhz: f32,
    pub cpu_temp: f32,
    pub cpu_usage: f32,
    pub gpu_mhz: f32,
    pub gpu_temp: f32,
    pub gpu_usage: f32,
    pub battery: f32,
    pub memory: f32,
    pub storage: f32,
}

/// Keeps the previous /proc/stat sample, for CPU usage between two calls.
#[derive(Default)]
pub struct Sampler {
    last_busy: u64,
    last_total: u64,
}

impl Sampler {
    fn cpu_usage(&mut self) -> f32 {
        let Some(stat) = read("/proc/stat") else { return 0.0 };
        let Some(line) = stat.lines().next() else { return 0.0 };
        let v: Vec<u64> = line.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
        if v.len() < 5 {
            return 0.0;
        }
        let idle = v[3] + v[4];
        let total: u64 = v.iter().sum();
        let busy = total - idle;
        let (db, dt) = (busy.saturating_sub(self.last_busy), total.saturating_sub(self.last_total));
        self.last_busy = busy;
        self.last_total = total;
        if dt == 0 { 0.0 } else { db as f32 * 100.0 / dt as f32 }
    }

    pub fn sample(&mut self) -> Metrics {
        let mut m = Metrics { cpu_usage: self.cpu_usage(), ..Default::default() };

        // CPU: average current frequency over all cores, Tctl temperature.
        let mut sum = 0.0;
        let mut n = 0.0;
        if let Ok(dir) = fs::read_dir("/sys/devices/system/cpu") {
            for e in dir.flatten() {
                if let Some(khz) = read_num(e.path().join("cpufreq/scaling_cur_freq")) {
                    sum += khz;
                    n += 1.0;
                }
            }
        }
        if n > 0.0 {
            m.cpu_mhz = (sum / n / 1000.0) as f32;
        }
        if let Some(h) = hwmon("k10temp") {
            m.cpu_temp = read_num(h.join("temp1_input")).unwrap_or(0.0) as f32 / 1000.0;
        }

        // GPU (amdgpu).
        if let Some(h) = hwmon("amdgpu") {
            m.gpu_mhz = read_num(h.join("freq1_input")).unwrap_or(0.0) as f32 / 1e6;
            m.gpu_temp = read_num(h.join("temp1_input")).unwrap_or(0.0) as f32 / 1000.0;
        }
        if let Some(d) = gpu_device() {
            m.gpu_usage = read_num(d.join("gpu_busy_percent")).unwrap_or(0.0) as f32;
        }

        m.battery = battery_dir()
            .and_then(|b| read_num(b.join("capacity")))
            .unwrap_or(0.0) as f32;

        if let Some(mi) = read("/proc/meminfo") {
            let kb = |key: &str| {
                mi.lines()
                    .find(|l| l.starts_with(key))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0)
            };
            let (total, avail) = (kb("MemTotal:"), kb("MemAvailable:"));
            if total > 0.0 {
                m.memory = ((total - avail) * 100.0 / total) as f32;
            }
        }

        m.storage = root_usage().unwrap_or(0.0);
        m
    }
}

fn root_usage() -> Option<f32> {
    let path = std::ffi::CString::new("/").ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let total = st.f_blocks as f64;
    let free = st.f_bfree as f64;
    if total == 0.0 { None } else { Some(((total - free) * 100.0 / total) as f32) }
}

/// The laptop's own battery, not a wireless mouse's (those have scope "Device").
fn battery_dir() -> Option<PathBuf> {
    fs::read_dir("/sys/class/power_supply")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            read(p.join("type")).as_deref() == Some("Battery")
                && read(p.join("scope")).as_deref() != Some("Device")
        })
}

// ---- System Information ---------------------------------------------------------

pub struct Info {
    pub hostname: String,
    pub model: String,
    pub cpu: String,
    pub gpu: String,
    pub board: String,
    pub display: String,
    pub audio: String,
    pub battery_capacity: String,
    pub os: String,
    pub memory: String,
    pub storage: String,
    pub network: String,
}

pub fn battery_level() -> String {
    let Some(b) = battery_dir() else { return "No battery".into() };
    let pct = read(b.join("capacity")).unwrap_or_default();
    let status = read(b.join("status")).unwrap_or_default();
    format!("{status} ({pct}%)")
}

fn lspci(classes: &[&str]) -> Vec<String> {
    let Ok(out) = Command::new("lspci").arg("-mm").output() else { return vec![] };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('"').collect();
            // 00:00.0 "Class" "Vendor" "Device" ...
            if f.len() > 5 && classes.iter().any(|c| f[1] == *c) {
                let vendor = f[3].split_whitespace().next().unwrap_or("");
                Some(format!("{vendor} {}", f[5]))
            } else {
                None
            }
        })
        .collect()
}

/// Panel ID from EDID, the way Windows shows it (e.g. AUO81B5), plus resolution.
fn display() -> String {
    let Ok(dir) = fs::read_dir("/sys/class/drm") else { return "Unknown".into() };
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.contains("eDP") {
            continue;
        }
        let Ok(edid) = fs::read(e.path().join("edid")) else { continue };
        if edid.len() < 12 {
            continue;
        }
        let m = u16::from_be_bytes([edid[8], edid[9]]);
        let letters: String = [(m >> 10) & 31, (m >> 5) & 31, m & 31]
            .iter()
            .map(|c| (b'A' + *c as u8 - 1) as char)
            .collect();
        let product = u16::from_le_bytes([edid[10], edid[11]]);
        let mode = read(e.path().join("modes"))
            .and_then(|m| m.lines().next().map(str::to_string))
            .unwrap_or_default();
        return format!("{letters}{product:04X}  ·  {mode}");
    }
    "Unknown".into()
}

fn audio() -> String {
    if let Ok(out) = Command::new("wpctl").args(["inspect", "@DEFAULT_AUDIO_SINK@"]).output() {
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(desc) = text
            .lines()
            .find(|l| l.contains("node.description"))
            .and_then(|l| l.split('"').nth(1))
        {
            return desc.to_string();
        }
    }
    read("/proc/asound/cards")
        .and_then(|c| c.lines().next().and_then(|l| l.split(" - ").nth(1)).map(str::to_string))
        .unwrap_or_else(|| "Unknown".into())
}

pub fn gather() -> Info {
    let cpu = read("/proc/cpuinfo")
        .and_then(|c| {
            c.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "Unknown".into());
    // "AMD RYZEN AI MAX+ 395 w/ Radeon 8060S" names its integrated GPU.
    let gpu = match cpu.split(" w/ ").nth(1) {
        Some(g) => format!("AMD {g} Graphics"),
        None => lspci(&["VGA compatible controller", "Display controller"])
            .into_iter()
            .next()
            .unwrap_or_else(|| "Unknown".into()),
    };

    let mem_gib = read("/proc/meminfo")
        .and_then(|m| m.lines().next().and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<f64>().ok()))
        .unwrap_or(0.0)
        / 1024.0
        / 1024.0;
    // MemTotal leaves out the GPU's share; round up to the installed size.
    let mem = [8, 16, 24, 32, 48, 64, 96, 128, 192, 256]
        .into_iter()
        .find(|&s| s as f64 >= mem_gib)
        .unwrap_or(mem_gib.ceil() as i32);

    let mut disk_bytes = 0u64;
    if let Ok(dir) = fs::read_dir("/sys/block") {
        for e in dir.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("nvme") || n.starts_with("sd") {
                disk_bytes += read_num(e.path().join("size")).unwrap_or(0.0) as u64 * 512;
            }
        }
    }

    let os = read("/etc/os-release")
        .and_then(|o| {
            o.lines()
                .find(|l| l.starts_with("PRETTY_NAME="))
                .map(|l| l.trim_start_matches("PRETTY_NAME=").trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "Linux".into());
    let kernel = read("/proc/sys/kernel/osrelease").unwrap_or_default();

    let dmi = |f: &str| read(format!("/sys/class/dmi/id/{f}")).unwrap_or_default();
    let capacity = battery_dir()
        .and_then(|b| read_num(b.join("energy_full_design")))
        .map(|uwh| format!("{:.0} mWh", uwh / 1000.0))
        .unwrap_or_else(|| "Unknown".into());

    Info {
        hostname: read("/proc/sys/kernel/hostname").unwrap_or_default().to_uppercase(),
        model: format!("{} {}", dmi("sys_vendor"), dmi("product_name")),
        cpu,
        gpu,
        board: dmi("board_name"),
        display: display(),
        audio: audio(),
        battery_capacity: capacity,
        os: format!("{os}\nLinux {kernel}"),
        memory: format!("{mem}GB"),
        storage: format!("{:.0}GB", disk_bytes as f64 / 1e9),
        network: lspci(&["Ethernet controller", "Network controller"]).join("\n"),
    }
}

// ---- Quick settings: speaker mute, touchpad -------------------------------------

pub fn speaker_muted() -> Option<bool> {
    let out = Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).contains("MUTED"))
}

pub fn set_speaker_mute(mute: bool) -> std::io::Result<()> {
    let ok = Command::new("wpctl")
        .args(["set-mute", "@DEFAULT_AUDIO_SINK@", if mute { "1" } else { "0" }])
        .status()?
        .success();
    if ok { Ok(()) } else { Err(std::io::Error::other("wpctl set-mute failed")) }
}

/// KDE: the touchpad's KWin input device. GNOME: gsettings.
fn kwin_touchpad() -> Option<zbus::blocking::Proxy<'static>> {
    let conn = zbus::blocking::Connection::session().ok()?;
    let mgr = zbus::blocking::Proxy::new(
        &conn,
        "org.kde.KWin",
        "/org/kde/KWin/InputDevice",
        "org.kde.KWin.InputDeviceManager",
    )
    .ok()?;
    let names: Vec<String> = mgr.get_property("devicesSysNames").ok()?;
    for n in names {
        let dev = zbus::blocking::Proxy::new_owned(
            conn.clone(),
            "org.kde.KWin",
            format!("/org/kde/KWin/InputDevice/{n}"),
            "org.kde.KWin.InputDevice",
        )
        .ok()?;
        if dev.get_property::<bool>("touchpad").unwrap_or(false) {
            return Some(dev);
        }
    }
    None
}

pub fn touchpad_enabled() -> Option<bool> {
    if let Some(dev) = kwin_touchpad() {
        return dev.get_property("enabled").ok();
    }
    let out = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.peripherals.touchpad", "send-events"])
        .output()
        .ok()?;
    Some(!String::from_utf8_lossy(&out.stdout).contains("disabled"))
}

pub fn set_touchpad(enabled: bool) -> std::io::Result<()> {
    if let Some(dev) = kwin_touchpad() {
        return dev.set_property("enabled", enabled).map_err(std::io::Error::other);
    }
    let ok = Command::new("gsettings")
        .args([
            "set",
            "org.gnome.desktop.peripherals.touchpad",
            "send-events",
            if enabled { "enabled" } else { "disabled" },
        ])
        .status()?
        .success();
    if ok { Ok(()) } else { Err(std::io::Error::other("couldn't change the touchpad")) }
}
