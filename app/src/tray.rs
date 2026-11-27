//! System tray icon (StatusNotifierItem: KDE natively, GNOME via the AppIndicator extension).

use crate::hw;
use ksni::menu::{CheckmarkItem, RadioGroup, RadioItem, StandardItem, SubMenu};
use ksni::{Icon, MenuItem, ToolTip, Tray};
use std::sync::OnceLock;

/// What the tray shows. The UI thread refreshes it; tray actions go to `on_action`.
pub struct NimoTray {
    pub fans: (u32, u32),
    pub cpu_temp: Option<f32>,
    pub profile: Option<usize>,
    /// Index into `FAN_LABELS`.
    pub fan_mode: Option<usize>,
    pub light_on: bool,
    pub effect: usize,
    /// Built-in presets, then yours.
    pub presets: Vec<String>,
    /// The newer version on offer, or "" (badges the icon and adds a menu entry).
    pub update: String,
    pub on_action: Box<dyn Fn(Action) + Send>,
}

pub enum Action {
    Open,
    /// Open the window with the update panel.
    Update,
    Profile(usize),
    FanMode(usize),
    LightOn(bool),
    Effect(usize),
    Preset(usize),
    Quit,
}

/// Labels for `hw::FAN_MODES`, as the window's Fan speed row shows them.
const FAN_LABELS: [&str; hw::FAN_MODES.len()] = ["Auto", "Max (fan 1)"];

impl Tray for NimoTray {
    fn id(&self) -> String {
        "nimo-control-center".into()
    }

    fn title(&self) -> String {
        "NIMO Control Center".into()
    }

    // KDE draws IconName whenever it isn't empty, and GNOME's AppIndicator
    // extension doesn't reliably draw overlays, so while an update is on offer
    // the icon is a badged pixmap instead of the themed icon.
    fn icon_name(&self) -> String {
        if self.update.is_empty() { "nimo-control-center".into() } else { String::new() }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        if self.update.is_empty() {
            return Vec::new();
        }
        static BADGED: OnceLock<Vec<Icon>> = OnceLock::new();
        BADGED
            .get_or_init(|| BADGE_SIZES.iter().filter_map(|&px| badged_icon(px)).collect())
            .clone()
    }

    fn tool_tip(&self) -> ToolTip {
        let mode = self.profile.map(|i| hw::PROFILE_LABELS[i]).unwrap_or("unknown");
        // Max pins fan 1 only (fan 2 stays on the EC's curve), so say which fan.
        let fan_max = if self.fan_mode == Some(1) { " · fan 1 at Max" } else { "" };
        let temp = self.cpu_temp.map(|t| format!(" · CPU {t:.0} °C")).unwrap_or_default();
        let update = if self.update.is_empty() { String::new() } else { format!("\nUpdate to {} available", self.update) };
        ToolTip {
            title: "NIMO Control Center".into(),
            description: format!(
                "{mode} mode · fans {} / {} rpm{fan_max}{temp}{update}",
                self.fans.0, self.fans.1
            ),
            ..Default::default()
        }
    }

    // Left click opens the window.
    fn activate(&mut self, _x: i32, _y: i32) {
        (self.on_action)(Action::Open);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut menu: Vec<MenuItem<Self>> = vec![
            StandardItem {
                label: "Open Control Center".into(),
                icon_name: "nimo-control-center".into(),
                activate: Box::new(|t: &mut Self| (t.on_action)(Action::Open)),
                ..Default::default()
            }
            .into(),
        ];
        if !self.update.is_empty() {
            menu.push(
                StandardItem {
                    label: format!("Update to {}…", self.update),
                    icon_name: "system-software-update".into(),
                    activate: Box::new(|t: &mut Self| (t.on_action)(Action::Update)),
                    ..Default::default()
                }
                .into(),
            );
        }
        menu.extend([
            MenuItem::Separator,
            SubMenu {
                label: "Power mode".into(),
                submenu: vec![
                    RadioGroup {
                        selected: self.profile.unwrap_or(usize::MAX),
                        select: Box::new(|t: &mut Self, i| {
                            t.profile = Some(i);
                            (t.on_action)(Action::Profile(i));
                        }),
                        options: hw::PROFILE_LABELS
                            .iter()
                            .map(|l| RadioItem {
                                label: (*l).into(),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Fans".into(),
                submenu: vec![
                    RadioGroup {
                        selected: self.fan_mode.unwrap_or(usize::MAX),
                        select: Box::new(|t: &mut Self, i| {
                            t.fan_mode = Some(i);
                            (t.on_action)(Action::FanMode(i));
                        }),
                        options: FAN_LABELS
                            .iter()
                            .map(|l| RadioItem {
                                label: (*l).into(),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Keyboard light".into(),
                checked: self.light_on,
                activate: Box::new(|t: &mut Self| {
                    t.light_on = !t.light_on;
                    (t.on_action)(Action::LightOn(t.light_on));
                }),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Lighting effect".into(),
                submenu: vec![
                    RadioGroup {
                        selected: self.effect,
                        select: Box::new(|t: &mut Self, i| {
                            t.effect = i;
                            (t.on_action)(Action::Effect(i));
                        }),
                        options: hw::EFFECT_LABELS
                            .iter()
                            .enumerate()
                            .map(|(i, l)| RadioItem {
                                label: if hw::TYPING_EFFECTS.contains(&i) {
                                    format!("{l} (as you type)")
                                } else {
                                    (*l).into()
                                },
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Lighting preset".into(),
                submenu: self
                    .presets
                    .iter()
                    .enumerate()
                    .map(|(i, name)| {
                        StandardItem {
                            label: name.clone(),
                            activate: Box::new(move |t: &mut Self| (t.on_action)(Action::Preset(i))),
                            ..Default::default()
                        }
                        .into()
                    })
                    .collect(),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| (t.on_action)(Action::Quit)),
                ..Default::default()
            }
            .into(),
        ]);
        menu
    }
}

/// Pixmap sizes for the badged icon; the tray picks the closest.
const BADGE_SIZES: [u32; 5] = [16, 22, 24, 32, 48];

/// The app's own icon (`ui/icon.svg`) with an update dot in the top-right corner.
fn badged_svg() -> String {
    const BADGE: &str = r##"<circle cx="50" cy="14" r="12" fill="#1fe0dc" stroke="#15171c" stroke-width="4"/>"##;
    include_str!("../ui/icon.svg").replacen("</svg>", &format!("  {BADGE}\n</svg>"), 1)
}

/// The badged icon at `px` × `px`, as ARGB32 in network byte order.
fn badged_icon(px: u32) -> Option<Icon> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_str(&badged_svg(), &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(px, px)?;
    let scale = px as f32 / tree.size().width();
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let data = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.alpha(), c.red(), c.green(), c.blue()]
        })
        .collect();
    Some(Icon { width: px as i32, height: px as i32, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tray(fan_mode: Option<usize>, sent: std::sync::mpsc::Sender<usize>) -> NimoTray {
        NimoTray {
            fans: (0, 0),
            cpu_temp: None,
            profile: None,
            fan_mode,
            light_on: true,
            effect: 0,
            presets: Vec::new(),
            update: String::new(),
            on_action: Box::new(move |a| {
                if let Action::FanMode(i) = a {
                    sent.send(i).unwrap();
                }
            }),
        }
    }

    #[test]
    fn fans_submenu_follows_and_sends_the_fan_mode() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut t = tray(Some(0), tx);
        let menu = t.menu();
        let labels: Vec<_> = menu
            .iter()
            .filter_map(|m| if let MenuItem::SubMenu(s) = m { Some(s.label.as_str()) } else { None })
            .collect();
        assert_eq!(labels[..2], ["Power mode", "Fans"]);
        let Some(MenuItem::SubMenu(fans)) = menu.into_iter().find(|m| matches!(m, MenuItem::SubMenu(s) if s.label == "Fans")) else {
            unreachable!()
        };
        let [MenuItem::RadioGroup(group)] = &fans.submenu[..] else { panic!("one radio group") };
        assert_eq!(group.selected, 0);
        let options: Vec<_> = group.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(options, ["Auto", "Max (fan 1)"]);
        assert!(!t.tool_tip().description.contains("Max"));
        (group.select)(&mut t, 1);
        assert_eq!(t.fan_mode, Some(1));
        assert_eq!(rx.try_recv(), Ok(1));
        assert!(t.tool_tip().description.contains("fan 1 at Max"));

        // Unknown mode (driver missing): nothing selected.
        let (tx, _rx) = std::sync::mpsc::channel();
        let Some(MenuItem::SubMenu(fans)) = tray(None, tx).menu().into_iter().find(|m| matches!(m, MenuItem::SubMenu(s) if s.label == "Fans")) else {
            unreachable!()
        };
        let [MenuItem::RadioGroup(group)] = &fans.submenu[..] else { panic!("one radio group") };
        assert_eq!(group.selected, usize::MAX);
    }

    #[test]
    fn badge_is_drawn_on_our_icon() {
        let svg = badged_svg();
        assert!(svg.starts_with(include_str!("../ui/icon.svg").trim_end().trim_end_matches("</svg>")));
        assert_eq!(svg.matches("<circle").count(), 1);
        for px in BADGE_SIZES {
            let icon = badged_icon(px).expect("renders");
            assert_eq!((icon.width, icon.height), (px as i32, px as i32));
            assert_eq!(icon.data.len(), (px * px * 4) as usize);
            // The badge's centre, (50, 14) of 64, is the accent colour, fully opaque.
            let (x, y) = (px * 50 / 64, px * 14 / 64);
            let i = ((y * px + x) * 4) as usize;
            assert_eq!(icon.data[i], 255, "{px}px alpha");
            assert!(icon.data[i + 2] > 200 && icon.data[i + 1] < 80, "{px}px colour {:?}", &icon.data[i..i + 4]);
        }
    }
}
