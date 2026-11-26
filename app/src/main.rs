//! NIMO Control Center for Linux.
//!
//! `nimo-control-center` opens the window; `nimo-control-center --tray` starts in
//! the tray only (for autostart). Either way the saved lighting is re-applied,
//! because the keyboard forgets it across reboots.

slint::include_modules!();

mod hw;
mod keyboard;
mod sysinfo;
mod tray;

use ksni::blocking::TrayMethods;
use slint::{Color, ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Shared = Arc<Mutex<hw::Lighting>>;

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("nimo-control-center {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if std::env::args().any(|a| a == "--help" || a == "-h") {
        println!("nimo-control-center [--tray]\n  --tray     start in the system tray (used at login)\n  --version  print the version");
        return Ok(());
    }
    let tray_only = std::env::args().any(|a| a == "--tray");
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
    let shared: Shared = Arc::new(Mutex::new(hw::load_lighting()));

    // ---- initial state ----
    ui.set_version(format!("{} (Linux)", env!("CARGO_PKG_VERSION")).into());
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
    show_info(&ui);
    if let Some(p) = preview {
        ui.set_page(p);
    } else if driver_ok {
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
            let pct = if on { pct.clamp(50, 95) as u8 } else { 100 };
            background(&w, "change the charge limit", move || hw::set_charge_limit(pct));
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
    let refresh = {
        let (w, s) = (weak.clone(), shared.clone());
        let tray = tray.clone();
        move || {
            let ui = w.unwrap();
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
                t.update(|t| {
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
