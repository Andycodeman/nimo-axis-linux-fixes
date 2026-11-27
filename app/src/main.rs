//! NIMO Control Center for Linux.
//!
//! `nimo-control-center` opens the window; `nimo-control-center --tray` starts in
//! the tray only (for autostart). Either way the saved lighting is re-applied,
//! because the keyboard forgets it across reboots. Only one copy runs: a second
//! launch shows the first copy's window (see `single`).

slint::include_modules!();

mod hw;
mod keyboard;
mod single;
mod sysinfo;
mod tray;
mod update;

use ksni::blocking::TrayMethods;
use slint::{Color, ComponentHandle, ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Shared = Arc<Mutex<hw::Lighting>>;
type Updates = Arc<Mutex<update::State>>;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn to_colour(c: [u8; 3]) -> Color {
    Color::from_rgb_u8(c[0], c[1], c[2])
}

fn from_colour(c: Color) -> [u8; 3] {
    [c.red(), c.green(), c.blue()]
}

/// Keycap icons (ui/keys/*.svg), drawn for this app to match the printed legends.
fn key_icon(name: &str) -> Option<slint::Image> {
    const ICONS: &[(&str, &[u8])] = &[
        ("arrow-down", include_bytes!("../ui/keys/arrow-down.svg").as_slice()),
        ("arrow-left", include_bytes!("../ui/keys/arrow-left.svg").as_slice()),
        ("arrow-right", include_bytes!("../ui/keys/arrow-right.svg").as_slice()),
        ("arrow-up", include_bytes!("../ui/keys/arrow-up.svg").as_slice()),
        ("bright-down", include_bytes!("../ui/keys/bright-down.svg").as_slice()),
        ("bright-up", include_bytes!("../ui/keys/bright-up.svg").as_slice()),
        ("copilot", include_bytes!("../ui/keys/copilot.svg").as_slice()),
        ("display", include_bytes!("../ui/keys/display.svg").as_slice()),
        ("fan-pm", include_bytes!("../ui/keys/fan-pm.svg").as_slice()),
        ("gear", include_bytes!("../ui/keys/gear.svg").as_slice()),
        ("kblight-down", include_bytes!("../ui/keys/kblight-down.svg").as_slice()),
        ("kblight-up", include_bytes!("../ui/keys/kblight-up.svg").as_slice()),
        ("mic-off", include_bytes!("../ui/keys/mic-off.svg").as_slice()),
        ("mute", include_bytes!("../ui/keys/mute.svg").as_slice()),
        ("next", include_bytes!("../ui/keys/next.svg").as_slice()),
        ("play-pause", include_bytes!("../ui/keys/play-pause.svg").as_slice()),
        ("prev", include_bytes!("../ui/keys/prev.svg").as_slice()),
        ("snowflake", include_bytes!("../ui/keys/snowflake.svg").as_slice()),
        ("space-light", include_bytes!("../ui/keys/space-light.svg").as_slice()),
        ("touchpad-off", include_bytes!("../ui/keys/touchpad-off.svg").as_slice()),
        ("vol-down", include_bytes!("../ui/keys/vol-down.svg").as_slice()),
        ("vol-up", include_bytes!("../ui/keys/vol-up.svg").as_slice()),
        ("win", include_bytes!("../ui/keys/win.svg").as_slice()),
    ];
    let (_, data) = ICONS.iter().find(|(n, _)| *n == name)?;
    slint::Image::load_from_svg_data(data).ok()
}

fn key_caps() -> Vec<KeyCap> {
    keyboard::KEYS
        .iter()
        .enumerate()
        .map(|(i, k)| KeyCap {
            icon: key_icon(k.icon).unwrap_or_default(),
            has_icon: !k.icon.is_empty(),
            x: k.x,
            y: k.y,
            w: k.w,
            h: k.h,
            zone: k.zone,
            label: k.label.into(),
            sub: k.sub.into(),
            idx: i as i32,
        })
        .collect()
}

/// Which preset the colours match, or -1.
fn preset_of(l: &hw::Lighting) -> i32 {
    (0..hw::PRESETS.len())
        .find(|&i| {
            let mut p = l.clone();
            p.set_preset(i);
            p.zones == l.zones && p.bars == l.bars
        })
        .map_or(-1, |i| i as i32)
}

/// Your saved presets, shared by the window and the tray.
fn user_presets() -> &'static Mutex<Vec<hw::UserPreset>> {
    static P: std::sync::OnceLock<Mutex<Vec<hw::UserPreset>>> = std::sync::OnceLock::new();
    P.get_or_init(|| Mutex::new(hw::load_presets()))
}

fn show_presets(ui: &AppWindow) {
    let names: Vec<slint::SharedString> =
        user_presets().lock().unwrap().iter().map(|p| p.name.as_str().into()).collect();
    ui.set_user_presets(ModelRc::new(VecModel::from(names)));
}

fn show_lighting(ui: &AppWindow, l: &hw::Lighting) {
    ui.set_light_on(l.on);
    ui.set_brightness(l.brightness as i32);
    ui.set_effect(l.effect as i32);
    ui.set_speed(l.speed as i32);
    let sleep_idx = hw::SLEEP_TIMES.iter().position(|&s| s == l.sleep).unwrap_or(8);
    ui.set_sleep_index(sleep_idx as i32);
    let zones: Vec<Color> = l.zones.iter().map(|z| to_colour(*z)).collect();
    ui.set_zones(ModelRc::new(VecModel::from(zones)));
    let bars: Vec<Color> = l.bars.iter().map(|b| to_colour(*b)).collect();
    ui.set_bars(ModelRc::new(VecModel::from(bars)));
    ui.set_preset(preset_of(l));
    let mine = user_presets().lock().unwrap().iter().position(|p| p.matches(l));
    ui.set_user_preset(mine.map_or(-1, |i| i as i32));
}

fn report(weak: &slint::Weak<AppWindow>, what: &str, result: io::Result<()>) {
    let msg = match result {
        Ok(()) => String::new(),
        Err(e) => format!("Couldn't {what}: {e}"),
    };
    let _ = weak.upgrade_in_event_loop(move |ui| ui.set_status(msg.into()));
}

type Job = Box<dyn FnOnce() + Send>;

/// One background thread runs every hardware write, in order. The firmware
/// pauses inside some commands (up to a second), so the window never waits for it.
fn hardware_queue() -> &'static std::sync::mpsc::Sender<Job> {
    static QUEUE: std::sync::OnceLock<std::sync::mpsc::Sender<Job>> = std::sync::OnceLock::new();
    QUEUE.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        std::thread::spawn(move || {
            for job in rx {
                job();
            }
        });
        tx
    })
}

/// Wait (up to 10 s) for the hardware queue to finish what it was given.
fn wait_for_hardware() {
    let (tx, rx) = std::sync::mpsc::channel();
    if hardware_queue().send(Box::new(move || {
        let _ = tx.send(());
    })).is_ok() {
        let _ = rx.recv_timeout(Duration::from_secs(10));
    }
}

/// Change the lighting state, show it straight away, then send it to the
/// keyboard and save it in the background.
fn change_lighting(
    weak: &slint::Weak<AppWindow>,
    shared: &Shared,
    change: impl FnOnce(&mut hw::Lighting) + Send + 'static,
    apply: fn(&hw::Lighting) -> io::Result<()>,
) {
    let snapshot = {
        let mut l = shared.lock().unwrap();
        change(&mut l);
        l.clone()
    };
    if let Some(ui) = weak.upgrade() {
        show_lighting(&ui, &snapshot);
    } else {
        let s = snapshot.clone();
        let _ = weak.upgrade_in_event_loop(move |ui| show_lighting(&ui, &s));
    }
    let weak = weak.clone();
    let _ = hardware_queue().send(Box::new(move || {
        let result = apply(&snapshot).and_then(|()| hw::save_lighting(&snapshot));
        report(&weak, "change the lighting", result);
    }));
}

/// Paint one target (-1 everything, 0-3 a zone, 4/5 a light bar) with a colour.
fn paint(weak: &slint::Weak<AppWindow>, shared: &Shared, target: i32, rgb: [u8; 3]) {
    change_lighting(
        weak,
        shared,
        move |l| {
            match target {
                0..=3 => l.zones[target as usize] = rgb,
                4 | 5 => l.bars[target as usize - 4] = rgb,
                _ => {
                    l.zones = [rgb; 4];
                    l.bars = [rgb; 2];
                }
            }
            // Colours show in Always and Breath; other effects make their own.
            if l.effect != 0 && l.effect != 3 {
                l.effect = 0;
            }
        },
        hw::set_colours_and_effect,
    );
}

/// Run a one-off hardware write on the hardware queue.
fn background(weak: &slint::Weak<AppWindow>, what: &'static str, f: impl FnOnce() -> io::Result<()> + Send + 'static) {
    let weak = weak.clone();
    let _ = hardware_queue().send(Box::new(move || report(&weak, what, f())));
}

fn show_info(ui: &AppWindow) {
    let i = sysinfo::gather();
    ui.set_hostname(i.hostname.into());
    ui.set_model(i.model.into());
    ui.set_cpu_name(i.cpu.into());
    ui.set_gpu_name(i.gpu.into());
    ui.set_board(i.board.into());
    ui.set_display(i.display.into());
    ui.set_audio(i.audio.into());
    ui.set_battery_capacity(i.battery_capacity.into());
    ui.set_os(i.os.into());
    ui.set_memory_size(i.memory.into());
    ui.set_storage_size(i.storage.into());
    ui.set_network(i.network.into());
}

// ---- update notice ----

fn show_update(ui: &AppWindow, s: &update::State) {
    let offer = if s.check { s.available(VERSION).unwrap_or("") } else { "" };
    ui.set_update_version(offer.into());
    let notes: Vec<String> = s.notes.iter().map(|n| format!("• {n}")).collect();
    ui.set_update_notes(notes.join("\n").into());
    ui.set_update_first_note(s.notes.first().map_or("", |n| n.as_str()).into());
    ui.set_update_url(s.url.as_str().into());
    ui.set_update_check(s.check);
    ui.set_update_checked(update::checked_text(s, update::now()).into());
}

/// Change update.conf (not in a preview), then show the result.
fn change_update(ui: &AppWindow, updates: &Updates, persist: bool, change: impl FnOnce(&mut update::State)) {
    let mut s = updates.lock().unwrap();
    change(&mut s);
    if persist {
        if let Err(e) = hw::save_update(&s) {
            ui.set_status(format!("Couldn't save the update setting: {e}").into());
        }
    }
    show_update(ui, &s);
}

fn skip_update(ui: &AppWindow, updates: &Updates, persist: bool) {
    change_update(ui, updates, persist, |s| s.skipped = s.latest.clone());
}

/// Show the window with the update panel (the notification's and the tray's "Update…").
fn open_update_panel(ui: &AppWindow) {
    let _ = ui.show();
    // Give a window that was hidden a moment to map before the popup.
    let w = ui.as_weak();
    slint::Timer::single_shot(Duration::from_millis(300), move || {
        if let Some(ui) = w.upgrade() {
            ui.invoke_show_update_panel();
        }
    });
}

/// Ask GitHub (on its own thread, never the hardware queue), remember the answer,
/// send the one desktop notification per version, and show what was found.
fn check_for_update(weak: slint::Weak<AppWindow>, updates: Updates, url: String, busy: Arc<AtomicBool>) {
    busy.store(true, Ordering::Relaxed);
    std::thread::spawn(move || {
        let etag = updates.lock().unwrap().etag.clone();
        let result = update::fetch(&url, &etag);
        let offer = {
            let mut s = updates.lock().unwrap();
            s.record(result, update::now());
            let _ = hw::save_update(&s);
            (s.check && s.should_notify(VERSION)).then(|| (s.latest.clone(), s.notes.clone()))
        };
        if let Some((version, notes)) = offer {
            let (w, u) = (weak.clone(), updates.clone());
            let on_choice = move |c: update::Choice| {
                let u = u.clone();
                let _ = w.upgrade_in_event_loop(move |ui| match c {
                    update::Choice::Open => {
                        let _ = ui.show();
                    }
                    update::Choice::Update => open_update_panel(&ui),
                    update::Choice::Skip => skip_update(&ui, &u, true),
                });
            };
            match update::notify(&version, &notes, on_choice) {
                Ok(()) => {
                    let mut s = updates.lock().unwrap();
                    s.notified = version;
                    let _ = hw::save_update(&s);
                }
                Err(e) => eprintln!("couldn't send the update notification ({e})"),
            }
        }
        let u = updates.clone();
        let _ = weak.upgrade_in_event_loop(move |ui| show_update(&ui, &u.lock().unwrap()));
        busy.store(false, Ordering::Relaxed);
    });
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("nimo-control-center {VERSION}");
        return Ok(());
    }
    if std::env::args().any(|a| a == "--help" || a == "-h") {
        println!("nimo-control-center [--tray]\n  --tray     start in the system tray (used at login)\n  --version  print the version");
        return Ok(());
    }
    let tray_only = std::env::args().any(|a| a == "--tray");
    // Internal: this process is the new file run in place of the old copy after
    // an upgrade, so the keyboard already shows the saved lighting.
    let replaced = std::env::args().any(|a| a == "--replaced");
    // Developer preview: NIMO_CC_PREVIEW=<page> opens that page and changes nothing.
    let preview: Option<i32> = std::env::var("NIMO_CC_PREVIEW").ok().and_then(|v| v.parse().ok());
    let ui = AppWindow::new()?;
    // Lets the desktop match the window to nimo-control-center.desktop and its icon.
    // Must come after the window object exists (it needs Slint's platform) but
    // before the window is shown.
    if let Err(e) = slint::set_xdg_app_id("nimo-control-center") {
        eprintln!("couldn't set the app id: {e}");
    }
    let weak = ui.as_weak();
    let instance = if preview.is_some() {
        None
    } else {
        let w = weak.clone();
        let handler = move |r| match r {
            single::Request::Activate => {
                let _ = w.upgrade_in_event_loop(|ui| {
                    let _ = ui.show();
                });
            }
            single::Request::Quit => {
                let _ = slint::invoke_from_event_loop(|| {
                    let _ = slint::quit_event_loop();
                });
            }
        };
        match single::claim(single::NAME, !tray_only, handler) {
            Ok(single::Claim::Owner(i)) => Some(i),
            Ok(single::Claim::Running) => return Ok(()),
            Err(e) => {
                eprintln!("couldn't check for another running copy ({e})");
                None
            }
        }
    };
    let shared: Shared = Arc::new(Mutex::new(hw::load_lighting()));
    // Only the copy that owns the D-Bus name checks for updates (never a preview).
    let checker = instance.is_some();
    // Developer preview: NIMO_CC_PREVIEW_UPDATE=<version> shows that update on offer.
    let updates: Updates = Arc::new(Mutex::new(match std::env::var("NIMO_CC_PREVIEW_UPDATE") {
        Ok(v) if preview.is_some() => update::State {
            latest: v,
            last_check: update::now() - 300,
            notes: vec![
                "Only one copy of NIMO Control Center runs.".into(),
                "The app restarts itself after an upgrade.".into(),
                "On CachyOS and Arch the app starts straight after installing, without logging out first.".into(),
            ],
            url: "https://github.com/Andycodeman/nimo-axis-linux-fixes/releases".into(),
            ..Default::default()
        },
        _ => hw::load_update(),
    }));
    let persist = preview.is_none();

    // ---- initial state ----
    ui.set_version(format!("{VERSION} (Linux)").into());
    let driver_ok = hw::driver_loaded();
    ui.set_driver_ok(driver_ok);
    ui.set_keys(ModelRc::new(VecModel::from(key_caps())));
    show_presets(&ui);
    {
        let l = shared.lock().unwrap();
        show_lighting(&ui, &l);
        ui.set_current_colour(to_colour(l.zones[0]));
    }
    ui.set_win_key(hw::key_enabled(hw::KEYS[0].0).unwrap_or(true));
    ui.set_fn_key(hw::key_enabled(hw::KEYS[1].0).unwrap_or(true));
    ui.set_prtsc_key(hw::key_enabled(hw::KEYS[2].0).unwrap_or(true));
    let limit = hw::charge_limit().unwrap_or(100);
    ui.set_limit_charge(limit < 100);
    ui.set_charge_limit(if limit < 100 { limit as i32 } else { 80 });
    // 0 (no limit) or none: just below the limit, the slider's top.
    let start = hw::charge_start().filter(|&s| s > 0).map_or(ui.get_charge_limit() - 5, i32::from);
    ui.set_charge_start(start);
    ui.set_fan_mode(hw::fan_mode().unwrap_or(0) as i32);
    show_info(&ui);
    ui.set_update_command(update::UPDATE_COMMAND.into());
    ui.set_update_has_terminal(update::session_terminal().is_some());
    show_update(&ui, &updates.lock().unwrap());
    if let Some(p) = preview {
        ui.set_page(p);
    } else if driver_ok && !replaced {
        change_lighting(&weak, &shared, |_| {}, hw::apply_lighting);
    }

    // ---- power mode ----
    {
        let w = weak.clone();
        ui.on_set_profile(move |i| {
            let i = i as usize;
            w.unwrap().set_profile(i as i32);
            background(&w, "change the power mode", move || hw::set_profile(i));
        });
    }

    // ---- lighting ----
    {
        let (w, s) = (weak.clone(), shared.clone());
        ui.on_set_light_on(move |on| change_lighting(&w, &s, move |l| l.on = on, hw::apply_lighting));
    }
    {
        let (w, s) = (weak.clone(), shared.clone());
        ui.on_set_brightness(move |b| {
            change_lighting(&w, &s, move |l| l.brightness = (b as u8).clamp(1, 10), hw::set_brightness)
        });
    }
    {
        let (w, s) = (weak.clone(), shared.clone());
        ui.on_set_effect(move |e| {
            if e == 0 {
                w.unwrap().set_selected_zone(-1);
            }
            change_lighting(&w, &s, move |l| l.effect = (e as usize).min(hw::EFFECTS.len() - 1), hw::set_effect)
        });
    }
    {
        let (w, s) = (weak.clone(), shared.clone());
        ui.on_set_speed(move |v| change_lighting(&w, &s, move |l| l.speed = (v as u8).clamp(1, 10), hw::set_speed));
    }
    {
        let (w, s) = (weak.clone(), shared.clone());
        ui.on_set_sleep(move |i| {
            let secs = hw::SLEEP_TIMES[(i as usize).min(hw::SLEEP_TIMES.len() - 1)];
            change_lighting(&w, &s, move |l| l.sleep = secs, hw::set_sleep)
        });
    }
    {
        let (w, s) = (weak.clone(), shared.clone());
        ui.on_set_preset(move |i| {
            w.unwrap().set_selected_zone(-1);
            let i = i as usize;
            change_lighting(
                &w,
                &s,
                move |l| {
                    l.set_preset(i);
                    l.effect = 0;
                },
                hw::set_colours_and_effect,
            );
        });
    }
    {
        let (w, s) = (weak.clone(), shared.clone());
        ui.on_set_colour(move |c| {
            let target = w.unwrap().get_selected_zone();
            paint(&w, &s, target, from_colour(c));
        });
    }
    {
        let (w, s) = (weak.clone(), shared.clone());
        ui.on_paint_target(move |target| {
            let ui = w.unwrap();
            ui.set_selected_zone(target);
            paint(&w, &s, target, from_colour(ui.get_current_colour()));
        });
    }
    {
        let w = weak.clone();
        ui.on_rgb_entered(move |r, g, b| {
            let ui = w.unwrap();
            let v = |t: &str| t.trim().parse::<u16>().ok().map(|n| n.min(255) as u8);
            match (v(&r), v(&g), v(&b)) {
                (Some(r), Some(g), Some(b)) => {
                    ui.set_current_colour(Color::from_rgb_u8(r, g, b));
                    ui.invoke_set_colour(Color::from_rgb_u8(r, g, b));
                }
                _ => ui.set_status("Enter red, green and blue as numbers from 0 to 255.".into()),
            }
        });
    }
    {
        let (w, s) = (weak.clone(), shared.clone());
        ui.on_lighting_default(move || {
            let ui = w.unwrap();
            ui.set_selected_zone(-1);
            let d = hw::Lighting::default();
            ui.set_current_colour(to_colour(d.zones[0]));
            change_lighting(&w, &s, move |l| *l = d, hw::apply_lighting);
        });
    }
    {
        let (w, s) = (weak.clone(), shared.clone());
        ui.on_save_preset(move |name| {
            let ui = w.unwrap();
            let look = s.lock().unwrap().clone();
            let result = {
                let mut presets = user_presets().lock().unwrap();
                let name = match name.trim() {
                    "" => format!("My preset {}", presets.len() + 1),
                    n => n.to_string(),
                };
                // Same name again replaces it.
                match presets.iter_mut().find(|p| p.name == name) {
                    Some(p) => p.look = look.clone(),
                    None => presets.push(hw::UserPreset { name, look: look.clone() }),
                }
                hw::save_presets(&presets)
            };
            show_presets(&ui);
            show_lighting(&ui, &look);
            report(&w, "save the preset", result);
        });
    }
    {
        let (w, s) = (weak.clone(), shared.clone());
        ui.on_apply_user_preset(move |i| {
            let Some(p) = user_presets().lock().unwrap().get(i as usize).cloned() else { return };
            w.unwrap().set_selected_zone(-1);
            change_lighting(&w, &s, move |l| p.apply_to(l), hw::apply_lighting);
        });
    }
    {
        let (w, s) = (weak.clone(), shared.clone());
        ui.on_delete_user_preset(move |i| {
            let ui = w.unwrap();
            let result = {
                let mut presets = user_presets().lock().unwrap();
                if (i as usize) < presets.len() {
                    presets.remove(i as usize);
                }
                hw::save_presets(&presets)
            };
            show_presets(&ui);
            show_lighting(&ui, &s.lock().unwrap());
            report(&w, "delete the preset", result);
        });
    }

    // ---- quick settings ----
    {
        let w = weak.clone();
        ui.on_set_key(move |i, on| {
            let file = hw::KEYS[i as usize].0;
            background(&w, "change the key setting", move || hw::set_key(file, on));
        });
    }
    {
        let w = weak.clone();
        ui.on_set_speaker_mute(move |m| background(&w, "change speaker mute", move || sysinfo::set_speaker_mute(m)));
    }
    {
        let w = weak.clone();
        ui.on_set_touchpad(move |on| background(&w, "change the touchpad", move || sysinfo::set_touchpad(on)));
    }
    {
        let w = weak.clone();
        ui.on_set_charge_limit(move |on, pct| {
            if !on {
                background(&w, "change the charge limit", || hw::set_charge_limit(100));
                return;
            }
            let pct = pct.clamp(50, 95);
            // The start must stay below the end: write the end first.
            let ui = w.unwrap();
            let start = ui.get_charge_start().clamp(20, pct - 5);
            ui.set_charge_start(start);
            let (end, start) = (pct as u8, start as u8);
            background(&w, "change the charge limit", move || {
                hw::set_charge_limit(end)?;
                match hw::set_charge_start(start) {
                    // A driver older than 0.8 has no start threshold.
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                    r => r,
                }
            });
        });
    }
    {
        let w = weak.clone();
        ui.on_set_charge_start(move |pct| {
            let ui = w.unwrap();
            if !ui.get_limit_charge() {
                return;
            }
            let start = pct.clamp(20, ui.get_charge_limit() - 5);
            ui.set_charge_start(start);
            background(&w, "change when charging starts", move || hw::set_charge_start(start as u8));
        });
    }
    {
        let w = weak.clone();
        ui.on_set_fan_mode(move |i| {
            w.unwrap().set_fan_mode(i);
            background(&w, "change the fan mode", move || hw::set_fan_mode(i as usize));
        });
    }

    // ---- update notice ----
    {
        let (w, u) = (weak.clone(), updates.clone());
        ui.on_update_toggled(move |on| change_update(&w.unwrap(), &u, persist, |s| s.check = on));
    }
    {
        let (w, u) = (weak.clone(), updates.clone());
        ui.on_update_skip(move || skip_update(&w.unwrap(), &u, persist));
    }
    {
        let w = weak.clone();
        ui.on_update_terminal(move || {
            let result = match update::session_terminal() {
                Some(argv) => update::spawn_detached(&argv),
                None => Err(io::Error::other("no terminal found; copy the command instead")),
            };
            report(&w, "open a terminal", result);
        });
    }
    {
        let (w, u) = (weak.clone(), updates.clone());
        ui.on_open_notes(move || {
            let url = u.lock().unwrap().url.clone();
            report(&w, "open the release notes", update::spawn_detached(&["xdg-open".into(), url]));
        });
    }

    // ---- tray ----
    let on_action = {
        let (w, s) = (weak.clone(), shared.clone());
        move |action: tray::Action| match action {
            tray::Action::Open => {
                let _ = w.upgrade_in_event_loop(|ui| {
                    let _ = ui.show();
                });
            }
            tray::Action::Update => {
                let _ = w.upgrade_in_event_loop(|ui| open_update_panel(&ui));
            }
            tray::Action::Profile(i) => {
                let _ = w.upgrade_in_event_loop(move |ui| ui.invoke_set_profile(i as i32));
            }
            tray::Action::LightOn(on) => change_lighting(&w, &s, move |l| l.on = on, hw::apply_lighting),
            tray::Action::Effect(e) => change_lighting(&w, &s, move |l| l.effect = e, hw::set_effect),
            tray::Action::Preset(i) => {
                let _ = w.upgrade_in_event_loop(move |ui| {
                    if i < hw::PRESETS.len() {
                        ui.invoke_set_preset(i as i32);
                    } else {
                        ui.invoke_apply_user_preset((i - hw::PRESETS.len()) as i32);
                    }
                });
            }
            tray::Action::Quit => {
                let _ = slint::invoke_from_event_loop(|| {
                    let _ = slint::quit_event_loop();
                });
            }
        }
    };
    let tray = {
        let l = shared.lock().unwrap();
        tray::NimoTray {
            fans: (0, 0),
            cpu_temp: None,
            profile: None,
            light_on: l.on,
            effect: l.effect,
            presets: Vec::new(),
            update: String::new(),
            on_action: Box::new(on_action),
        }
    }
    .spawn();
    let tray = match tray {
        Ok(handle) if preview.is_some() => {
            handle.shutdown();
            None
        }
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("no system tray ({e}); closing the window will quit");
            None
        }
    };
    if tray.is_none() {
        ui.window().on_close_requested(|| {
            let _ = slint::quit_event_loop();
            slint::CloseRequestResponse::HideWindow
        });
    }

    // ---- live readings every 2 s ----
    let sampler = RefCell::new(sysinfo::Sampler::default());
    let upgrade = RefCell::new(single::Upgrade::default());
    // NIMO_UPDATE_URL (a test server) checks at every start, straight away.
    let (update_url, update_test) = update::endpoint();
    let first_check = Cell::new(update_test);
    // Otherwise the first check waits a minute after start, so the network is up at login.
    let check_after = Instant::now() + if update_test { Duration::ZERO } else { Duration::from_secs(60) };
    let checking = Arc::new(AtomicBool::new(false));
    let refresh = {
        let (w, s, u) = (weak.clone(), shared.clone(), updates.clone());
        let tray = tray.clone();
        move || {
            let ui = w.unwrap();
            if checker && !checking.load(Ordering::Relaxed) && Instant::now() >= check_after {
                let due = u.try_lock().is_ok_and(|st| st.check && (first_check.replace(false) || st.due(update::now())));
                if due {
                    check_for_update(w.clone(), u.clone(), update_url.clone(), checking.clone());
                }
            }
            // An upgrade replaced the app's file: run the new one in this copy's place.
            if let Some(i) = &instance {
                if let Some(exe) = upgrade.borrow_mut().check() {
                    wait_for_hardware();
                    let mut args = vec!["--replaced"];
                    if !ui.window().is_visible() {
                        args.push("--tray");
                    }
                    let e = i.replace_with(&exe, &args);
                    eprintln!("couldn't start the upgraded app ({e}); keeping this copy");
                }
            }
            let m = sampler.borrow_mut().sample();
            let fans = hw::fans();
            let profile = hw::profile();
            ui.set_cpu_mhz(m.cpu_mhz);
            ui.set_cpu_temp(m.cpu_temp);
            ui.set_cpu_usage(m.cpu_usage);
            ui.set_gpu_mhz(m.gpu_mhz);
            ui.set_gpu_temp(m.gpu_temp);
            ui.set_gpu_usage(m.gpu_usage);
            ui.set_battery(m.battery);
            ui.set_memory(m.memory);
            ui.set_storage(m.storage);
            ui.set_fan1(fans.0 as f32);
            ui.set_fan2(fans.1 as f32);
            ui.set_battery_level(sysinfo::battery_level().into());
            if let Some(p) = profile {
                ui.set_profile(p as i32);
            }
            if ui.window().is_visible() && ui.get_page() == 1 {
                if let Ok(st) = u.try_lock() {
                    ui.set_update_checked(update::checked_text(&st, update::now()).into());
                }
            }
            if ui.window().is_visible() && ui.get_page() == 0 {
                if let Some(muted) = sysinfo::speaker_muted() {
                    ui.set_speaker_mute(muted);
                }
                if let Some(on) = sysinfo::touchpad_enabled() {
                    ui.set_touchpad(on);
                }
            }
            if let Some(t) = &tray {
                let (on, effect) = s.try_lock().map(|l| (l.on, l.effect)).unwrap_or((ui.get_light_on(), ui.get_effect() as usize));
                let mut names: Vec<String> = hw::PRESETS.iter().map(|(n, _)| n.to_string()).collect();
                names.extend(user_presets().lock().unwrap().iter().map(|p| p.name.clone()));
                let offer = ui.get_update_version().to_string();
                t.update(|t| {
                    t.update = offer;
                    t.presets = names;
                    t.fans = fans;
                    t.cpu_temp = Some(m.cpu_temp);
                    t.profile = profile;
                    t.light_on = on;
                    t.effect = effect;
                });
            }
        }
    };
    refresh();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_secs(2), refresh);

    if !tray_only || tray.is_none() {
        ui.show()?;
    }
    slint::run_event_loop_until_quit()?;
    Ok(())
}
