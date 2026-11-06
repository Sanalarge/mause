use crate::flows::{Dir, Flow, Look};
use eframe::egui;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Popup(Option<(f32, f32)>),
    Toggle(Flow, bool),
    StartStop,
    Open,
    Quit,
}

#[derive(Clone, PartialEq, Default)]
pub struct Snapshot {
    pub running: bool,
    pub connected: bool,
    pub peer: String,
    pub mine: u16,
    pub peer_prefs: Option<u16>,
}

impl Snapshot {
    pub fn status(&self) -> String {
        if self.connected {
            format!("Bağlı: {}", self.peer)
        } else if self.running {
            "Karşı PC aranıyor".into()
        } else {
            "KVM paylaşımı kapalı".into()
        }
    }

    pub fn checked(&self, flow: Flow) -> bool {
        self.connected && flow.look(self.mine, self.peer_prefs) != Look::Off
    }

    pub fn wanted(&self, flow: Flow) -> bool {
        flow.wanted(self.mine, self.peer_prefs)
    }

    pub fn enabled(&self, flow: Flow) -> bool {
        self.connected && (self.peer_prefs.is_some() || !flow.needs_peer(self.wanted(flow)))
    }

    pub fn label(&self, flow: Flow) -> String {
        if self.connected && flow.look(self.mine, self.peer_prefs) == Look::PeerOff {
            format!("{} (karşı PC'de kapalı)", flow.sentence())
        } else {
            flow.sentence().into()
        }
    }

    fn run_label(&self) -> &'static str {
        if self.running {
            "KVM paylaşımını durdur"
        } else {
            "KVM paylaşımını başlat"
        }
    }
}

fn group_title(dir: Dir) -> &'static str {
    match dir {
        Dir::Out => "Bu PC'den karşıya",
        Dir::In => "Karşıdan bu PC'ye",
    }
}

type Queue = Arc<Mutex<Vec<Action>>>;

pub struct Tray {
    queue: Queue,
    shown: Snapshot,
    inner: imp::Inner,
}

impl Tray {
    pub fn new(ctx: &egui::Context) -> Option<Self> {
        let queue: Queue = Arc::default();
        let shown = Snapshot::default();
        let inner = imp::Inner::new(ctx.clone(), queue.clone(), &shown)?;
        Some(Self {
            queue,
            shown,
            inner,
        })
    }

    pub fn poll(&self) -> Vec<Action> {
        self.queue.lock().unwrap().drain(..).collect()
    }

    pub fn update(&mut self, snap: Snapshot, force: bool) {
        if force || snap != self.shown {
            self.inner.show(&snap);
            self.shown = snap;
        }
    }
}

fn push(queue: &Queue, ctx: &egui::Context, action: Action) {
    queue.lock().unwrap().push(action);
    ctx.request_repaint();
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
fn flow_index(flow: Flow) -> usize {
    let kind = Flow::KINDS
        .iter()
        .position(|k| *k == flow.kind)
        .unwrap_or(0);
    kind + if flow.dir == Dir::In {
        Flow::KINDS.len()
    } else {
        0
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod imp {
    use super::*;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    pub struct Inner {
        _icon: TrayIcon,
        status: MenuItem,
        run: MenuItem,
        flows: Vec<(Flow, CheckMenuItem)>,
    }

    impl Inner {
        pub fn new(ctx: egui::Context, queue: Queue, snap: &Snapshot) -> Option<Self> {
            let menu = Menu::new();
            let status = MenuItem::new(snap.status(), false, None);
            let run = MenuItem::with_id("run", snap.run_label(), true, None);
            menu.append(&status).ok()?;
            let mut flows = Vec::new();
            for dir in [Dir::Out, Dir::In] {
                menu.append(&PredefinedMenuItem::separator()).ok()?;
                menu.append(&MenuItem::new(group_title(dir), false, None))
                    .ok()?;
                for kind in Flow::KINDS {
                    let flow = Flow { dir, kind };
                    let item = CheckMenuItem::with_id(
                        format!("flow-{}", flow_index(flow)),
                        snap.label(flow),
                        snap.enabled(flow),
                        snap.checked(flow),
                        None,
                    );
                    menu.append(&item).ok()?;
                    flows.push((flow, item));
                }
            }
            menu.append(&PredefinedMenuItem::separator()).ok()?;
            menu.append(&run).ok()?;
            menu.append(&MenuItem::with_id("open", "Mause'u aç", true, None))
                .ok()?;
            menu.append(&MenuItem::with_id("quit", "Çık", true, None))
                .ok()?;

            let rgba = crate::logo::rgba(32);
            let icon = tray_icon::Icon::from_rgba(rgba, 32, 32).ok()?;
            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip(format!("Mause: {}", snap.status()))
                .with_icon(icon)
                .with_menu_on_left_click(false)
                .build()
                .ok()?;

            let list: Vec<Flow> = flows.iter().map(|(f, _)| *f).collect();
            let (q, c) = (queue.clone(), ctx.clone());
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let id = e.id.0.as_str();
                let action = match id {
                    "run" => Some(Action::StartStop),
                    "open" => Some(Action::Open),
                    "quit" => Some(Action::Quit),
                    _ => id
                        .strip_prefix("flow-")
                        .and_then(|n| n.parse::<usize>().ok())
                        .and_then(|n| list.get(n))
                        .map(|flow| Action::Toggle(*flow, SHOWN.lock().unwrap().wanted(*flow))),
                };
                if let Some(action) = action {
                    push(&q, &c, action);
                }
            }));
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    rect,
                    ..
                } = e
                {
                    let x = rect.position.x + rect.size.width as f64 / 2.0;
                    push(
                        &queue,
                        &ctx,
                        Action::Popup(Some((x as f32, rect.position.y as f32))),
                    );
                }
            }));
            Some(Self {
                _icon: tray,
                status,
                run,
                flows,
            })
        }

        pub fn show(&mut self, snap: &Snapshot) {
            self.status.set_text(snap.status());
            self.run.set_text(snap.run_label());
            for (flow, item) in &self.flows {
                item.set_text(snap.label(*flow));
                item.set_checked(snap.checked(*flow));
                item.set_enabled(snap.enabled(*flow));
            }
            let _ = self
                ._icon
                .set_tooltip(Some(format!("Mause: {}", snap.status())));
            *SHOWN.lock().unwrap() = snap.clone();
        }
    }

    static SHOWN: Mutex<Snapshot> = Mutex::new(Snapshot {
        running: false,
        connected: false,
        peer: String::new(),
        mine: 0,
        peer_prefs: None,
    });
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use crate::flows::Kind;
    use ksni::blocking::{Handle, TrayMethods};
    use ksni::menu::{CheckmarkItem, StandardItem};
    use ksni::{MenuItem, ToolTip};

    struct Sni {
        snap: Snapshot,
        queue: Queue,
        ctx: egui::Context,
    }

    impl Sni {
        fn push(&self, action: Action) {
            push(&self.queue, &self.ctx, action);
        }
    }

    fn icon_name(kind: Kind) -> &'static str {
        match kind {
            Kind::Control => "input-keyboard",
            Kind::Audio => "audio-volume-high",
            Kind::Mic => "audio-input-microphone",
            Kind::Files => "folder",
            Kind::Clip => "edit-paste",
        }
    }

    impl ksni::Tray for Sni {
        fn id(&self) -> String {
            "mause".into()
        }
        fn title(&self) -> String {
            "Mause".into()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            let size = 32;
            let data = crate::logo::rgba(size as usize)
                .chunks_exact(4)
                .flat_map(|p| [p[3], p[0], p[1], p[2]])
                .collect();
            vec![ksni::Icon {
                width: size,
                height: size,
                data,
            }]
        }
        fn tool_tip(&self) -> ToolTip {
            ToolTip {
                title: "Mause".into(),
                description: self.snap.status(),
                ..Default::default()
            }
        }
        fn activate(&mut self, x: i32, y: i32) {
            let at = (x != 0 || y != 0).then_some((x as f32, y as f32));
            self.push(Action::Popup(at));
        }
        fn menu(&self) -> Vec<MenuItem<Self>> {
            let mut items: Vec<MenuItem<Self>> = vec![
                StandardItem {
                    label: self.snap.status(),
                    enabled: false,
                    icon_name: "network-wired".into(),
                    ..Default::default()
                }
                .into(),
            ];
            for dir in [Dir::Out, Dir::In] {
                items.push(MenuItem::Separator);
                items.push(
                    StandardItem {
                        label: group_title(dir).into(),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                );
                for kind in Flow::KINDS {
                    let flow = Flow { dir, kind };
                    let want = self.snap.wanted(flow);
                    items.push(
                        CheckmarkItem {
                            label: self.snap.label(flow),
                            checked: self.snap.checked(flow),
                            enabled: self.snap.enabled(flow),
                            icon_name: icon_name(kind).into(),
                            activate: Box::new(move |t: &mut Self| {
                                t.push(Action::Toggle(flow, want))
                            }),
                            ..Default::default()
                        }
                        .into(),
                    );
                }
            }
            items.push(MenuItem::Separator);
            items.push(
                StandardItem {
                    label: self.snap.run_label().into(),
                    icon_name: if self.snap.running {
                        "media-playback-stop"
                    } else {
                        "media-playback-start"
                    }
                    .into(),
                    activate: Box::new(|t: &mut Self| t.push(Action::StartStop)),
                    ..Default::default()
                }
                .into(),
            );
            items.push(
                StandardItem {
                    label: "Mause'u aç".into(),
                    icon_name: "window-new".into(),
                    activate: Box::new(|t: &mut Self| t.push(Action::Open)),
                    ..Default::default()
                }
                .into(),
            );
            items.push(
                StandardItem {
                    label: "Çık".into(),
                    icon_name: "application-exit".into(),
                    activate: Box::new(|t: &mut Self| t.push(Action::Quit)),
                    ..Default::default()
                }
                .into(),
            );
            items
        }
    }

    pub struct Inner {
        handle: Handle<Sni>,
    }

    impl Inner {
        pub fn new(ctx: egui::Context, queue: Queue, snap: &Snapshot) -> Option<Self> {
            let handle = Sni {
                snap: snap.clone(),
                queue,
                ctx,
            }
            .spawn()
            .ok()?;
            Some(Self { handle })
        }

        pub fn show(&mut self, snap: &Snapshot) {
            let snap = snap.clone();
            self.handle.update(move |t| t.snap = snap);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flows::{Kind, Pref};

    #[test]
    fn menu_is_disabled_until_connected_and_old_peers_only_allow_local_changes() {
        let all = Pref::ALL.into_iter().fold(0, |b, p| b | p.bit());
        let off = Snapshot {
            running: true,
            ..Default::default()
        };
        let old_peer = Snapshot {
            running: true,
            connected: true,
            peer: "B".into(),
            mine: all,
            peer_prefs: None,
        };
        let new_peer = Snapshot {
            peer_prefs: Some(all),
            ..old_peer.clone()
        };
        for flow in Flow::all() {
            assert!(!off.enabled(flow));
            assert!(new_peer.enabled(flow));
            assert_eq!(old_peer.enabled(flow), !flow.needs_peer(false), "{flow:?}");
        }
        assert_eq!(
            flow_index(Flow {
                dir: Dir::In,
                kind: Kind::Clip
            }),
            9
        );
    }
}
