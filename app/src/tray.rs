//! System tray icon (StatusNotifierItem: KDE natively, GNOME via the AppIndicator extension).

use crate::hw;
use ksni::menu::{CheckmarkItem, RadioGroup, RadioItem, StandardItem, SubMenu};
use ksni::{MenuItem, ToolTip, Tray};

/// What the tray shows. The UI thread refreshes it; tray actions go to `on_action`.
pub struct NimoTray {
    pub fans: (u32, u32),
    pub cpu_temp: Option<f32>,
    pub profile: Option<usize>,
    pub light_on: bool,
    pub effect: usize,
    /// Built-in presets, then yours.
    pub presets: Vec<String>,
    pub on_action: Box<dyn Fn(Action) + Send>,
}

pub enum Action {
    Open,
    Profile(usize),
    LightOn(bool),
    Effect(usize),
    Preset(usize),
    Quit,
}

impl Tray for NimoTray {
    fn id(&self) -> String {
        "nimo-control-center".into()
    }

    fn title(&self) -> String {
        "NIMO Control Center".into()
    }

    fn icon_name(&self) -> String {
        "nimo-control-center".into()
    }

    fn tool_tip(&self) -> ToolTip {
        let mode = self.profile.map(|i| hw::PROFILE_LABELS[i]).unwrap_or("unknown");
        let temp = self.cpu_temp.map(|t| format!(" · CPU {t:.0} °C")).unwrap_or_default();
        ToolTip {
            title: "NIMO Control Center".into(),
            description: format!(
                "{mode} mode · fans {} / {} rpm{temp}",
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
        vec![
            StandardItem {
                label: "Open Control Center".into(),
                icon_name: "nimo-control-center".into(),
                activate: Box::new(|t: &mut Self| (t.on_action)(Action::Open)),
                ..Default::default()
            }
            .into(),
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
        ]
    }
}
