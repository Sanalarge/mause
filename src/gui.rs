use crate::audio::{self, Dev, Kind};
use crate::config::{Config, Side};
use crate::core::{self, Control, Decision, Perms, Policy, Shared, State};
use crate::discover::{self, Peers};
use crate::flows::{self, Dir, Flow, Kind as FlowKind, Look, Pref, Where};
use crate::hardware;
use crate::screen;
use crate::transfer::{self, Status};
use crate::tray::Action as TrayAction;
use crate::update;
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Margin, RichText, Sense, Stroke, StrokeKind, Vec2,
};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::design::{self, BODY, Icon, KeyKind, LEAD, Palette, RADIUS_PANEL};

pub(crate) fn screenshot_on_request(ctx: &egui::Context) {
    use std::sync::atomic::AtomicU32;
    static FRAMES: AtomicU32 = AtomicU32::new(0);
    let Ok(path) = std::env::var("MAUSE_SHOT") else {
        return;
    };
    ctx.request_repaint();
    if FRAMES.fetch_add(1, Ordering::Relaxed) != 20 {
        return;
    }
    let shot = egui::ScreenshotCallback::new(move |image| {
        let [w, h] = image.size;
        let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
        let mut jpeg = Vec::new();
        let r = jpeg_encoder::Encoder::new(&mut jpeg, 92)
            .encode(&rgba, w as u16, h as u16, jpeg_encoder::ColorType::Rgba)
            .map_err(|e| e.to_string())
            .and_then(|()| std::fs::write(&path, &jpeg).map_err(|e| e.to_string()));
        if let Err(e) = r {
            eprintln!("ekran görüntüsü kaydedilemedi: {e}");
        }
        std::process::exit(0);
    });
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(shot));
}

pub const TOPBAR: f32 = 76.0;

pub fn run() {
    let icon = egui::IconData {
        rgba: crate::logo::rgba(128),
        width: 128,
        height: 128,
    };
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 800.0])
            .with_min_inner_size([820.0, 580.0])
            .with_decorations(false)
            .with_title("Mause")
            .with_app_id("mause")
            .with_icon(Arc::new(icon)),
        centered: true,
        #[cfg(target_os = "linux")]
        event_loop_builder: std::env::var_os("DISPLAY").map(|_| {
            Box::new(|b: &mut eframe::EventLoopBuilder<eframe::UserEvent>| {
                use winit::platform::x11::EventLoopBuilderExtX11;
                b.with_x11();
            }) as _
        }),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "Mause",
        opts,
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    );
}

#[derive(Clone, PartialEq)]
enum Upd {
    Unknown,
    Checking,
    Latest,
    Available(update::Release),
    Installing,
    Failed(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Share,
    Audio,
    Files,
    Usb,
    Settings,
    Log,
}

#[derive(Clone, Copy, PartialEq)]
enum SettingsPage {
    General,
    Connection,
    Sharing,
    About,
}

impl SettingsPage {
    const ALL: [SettingsPage; 4] = [
        SettingsPage::General,
        SettingsPage::Connection,
        SettingsPage::Sharing,
        SettingsPage::About,
    ];

    fn title(self) -> &'static str {
        match self {
            SettingsPage::General => "Genel",
            SettingsPage::Connection => "Bağlantı ve güvenlik",
            SettingsPage::Sharing => "Paylaşım",
            SettingsPage::About => "Hakkında",
        }
    }

    fn icon(self) -> Icon {
        match self {
            SettingsPage::General => Icon::Settings,
            SettingsPage::Connection => Icon::Shield,
            SettingsPage::Sharing => Icon::Share,
            SettingsPage::About => Icon::Log,
        }
    }
}

impl Page {
    const ALL: [Page; 6] = [
        Page::Share,
        Page::Audio,
        Page::Files,
        Page::Usb,
        Page::Settings,
        Page::Log,
    ];
    fn title(self) -> &'static str {
        match self {
            Page::Share => "Paylaşım",
            Page::Audio => "Ses",
            Page::Files => "Dosyalar",
            Page::Usb => "USB",
            Page::Settings => "Ayarlar",
            Page::Log => "Günlük",
        }
    }
    fn tab(self) -> &'static str {
        self.title()
    }
    fn icon(self) -> Icon {
        match self {
            Page::Share => Icon::Share,
            Page::Audio => Icon::Audio,
            Page::Files => Icon::Files,
            Page::Usb => Icon::Usb,
            Page::Settings => Icon::Settings,
            Page::Log => Icon::Log,
        }
    }
}

#[derive(Default)]
struct AudioView {
    ok: bool,
    sinks: Vec<Dev>,
    sources: Vec<Dev>,
    def_sink: String,
    def_source: String,
    out_vol: (u32, bool),
    in_vol: (u32, bool),
}

impl AudioView {
    fn refresh() -> Self {
        Self {
            ok: audio::available(),
            sinks: audio::list(Kind::Sink),
            sources: audio::list(Kind::Source),
            def_sink: audio::default(Kind::Sink).unwrap_or_default(),
            def_source: audio::default(Kind::Source).unwrap_or_default(),
            out_vol: audio::volume(Kind::Sink).unwrap_or((0, false)),
            in_vol: audio::volume(Kind::Source).unwrap_or((0, false)),
        }
    }
}

struct Popup {
    anchor: Option<(f32, f32)>,
    focused: bool,
    opened: Instant,
    height: f32,
}

impl Popup {
    const SIZE: Vec2 = Vec2::new(480.0, 460.0);

    fn new(anchor: Option<(f32, f32)>) -> Self {
        Self {
            anchor,
            focused: false,
            opened: Instant::now(),
            height: Self::SIZE.y,
        }
    }
}

struct FlowView {
    look: Look,
    want: bool,
    enabled: bool,
    live: bool,
    tip: String,
}

fn flow_icon(kind: FlowKind) -> Icon {
    match kind {
        FlowKind::Control => Icon::Keyboard,
        FlowKind::Audio => Icon::Audio,
        FlowKind::Mic => Icon::Mic,
        FlowKind::Files => Icon::Files,
        FlowKind::Clip => Icon::Clipboard,
    }
}

enum PopupAction {
    Toggle(Flow, bool),
    Watch(bool),
    Flow(String, Flow, bool),
    WatchOn(String, bool),
    Device(String),
    StartStop,
    Hide,
    Open,
    Quit,
    Close,
}

struct App {
    page: Page,
    settings_page: SettingsPage,
    logo: Option<egui::TextureHandle>,
    audio: AudioView,
    audio_at: Instant,
    cfg: Config,
    shared: Option<Arc<Shared>>,
    peers: Peers,
    perms: (bool, bool),
    show_secret: bool,
    restart_pending: bool,
    pair_ip: String,
    perm_at: Instant,
    missing: Vec<String>,
    grant_msg: Arc<Mutex<Option<String>>>,
    auto_grant_tried: bool,
    grant_handled: bool,
    perm_modal: bool,
    devs_modal: bool,
    notes_open: bool,
    bell_rect: egui::Rect,
    splash: Option<crate::splash::Splash>,
    devices: Vec<(String, &'static str)>,
    tray: Option<crate::tray::Tray>,
    popup: Option<Popup>,
    popup_device: Option<String>,
    tray_click: Option<Instant>,
    local: Pane,
    remote: Arc<Mutex<Pane>>,
    peer_files: Option<bool>,
    local_dirty: Arc<AtomicBool>,
    file_dialog: Option<FileDialog>,
    pair_done_at: Option<Instant>,
    screen_tex: Option<(egui::TextureHandle, u64)>,
    camera: Option<CameraView>,
    torch_on: HashSet<String>,
    vcam_note: Arc<Mutex<Option<String>>>,
    #[cfg(target_os = "windows")]
    vdisplay_note: Arc<Mutex<Option<String>>>,
    screen_ctrl: bool,
    screen_mods: [bool; 4],
    screen_down: Vec<u16>,
    usb_local: Option<Result<Vec<crate::usb::Dev>, String>>,
    update: Arc<Mutex<Upd>>,
    usb_tools: Arc<Mutex<Option<String>>>,
    update_at: Option<Instant>,
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        let mut cfg = Config::load();
        discover::set_self_id(&cfg.id);
        cfg.save();
        if let Some((w, h)) = crate::platform::detect_screen() {
            (cfg.w, cfg.h) = (w, h);
        }
        apply_style(ctx, cfg.theme);
        let av = AudioView::refresh();
        if !cfg.audio_sink.is_empty() && av.sinks.iter().any(|d| d.name == cfg.audio_sink) {
            audio::set_default(Kind::Sink, &cfg.audio_sink);
        }
        if !cfg.audio_source.is_empty() && av.sources.iter().any(|d| d.name == cfg.audio_source) {
            audio::set_default(Kind::Source, &cfg.audio_source);
        }
        let mut app = Self {
            page: match std::env::var("MAUSE_PAGE").as_deref() {
                Ok("share" | "home" | "kvm") => Page::Share,
                Ok("audio") => Page::Audio,
                Ok("files") => Page::Files,
                Ok("usb") => Page::Usb,
                Ok("settings") => Page::Settings,
                Ok("log") => Page::Log,
                _ => Page::Share,
            },
            settings_page: SettingsPage::General,
            logo: None,
            local: Pane::default(),
            remote: Arc::default(),
            peer_files: None,
            local_dirty: Arc::default(),
            file_dialog: None,
            pair_done_at: None,
            audio: AudioView::refresh(),
            audio_at: Instant::now(),
            cfg,
            shared: None,
            peers: discover::peers(),
            perms: crate::platform::check_permissions(),
            show_secret: false,
            restart_pending: false,
            pair_ip: String::new(),
            perm_at: Instant::now(),
            missing: crate::platform::unreadable_devices(),
            grant_msg: Arc::default(),
            auto_grant_tried: false,
            grant_handled: false,
            perm_modal: true,
            devs_modal: std::env::var("MAUSE_DEVELOPERS").as_deref() == Ok("1"),
            notes_open: std::env::var("MAUSE_NOTES").as_deref() == Ok("1"),
            bell_rect: egui::Rect::NOTHING,
            splash: [
                "MAUSE_SHOT",
                "MAUSE_PAGE",
                "MAUSE_DEVELOPERS",
                "MAUSE_POPUP",
                "MAUSE_NO_SPLASH",
            ]
            .iter()
            .all(|k| std::env::var_os(k).is_none())
            .then(crate::splash::Splash::new),
            devices: crate::platform::input_devices(),
            tray: crate::tray::Tray::new(ctx),
            tray_click: None,
            popup: (std::env::var("MAUSE_POPUP").as_deref() == Ok("1")).then(|| Popup::new(None)),
            popup_device: None,
            screen_tex: None,
            camera: None,
            torch_on: HashSet::new(),
            vcam_note: Arc::default(),
            #[cfg(target_os = "windows")]
            vdisplay_note: Arc::default(),
            screen_ctrl: false,
            screen_mods: [false; 4],
            screen_down: Vec::new(),
            usb_local: None,
            update: Arc::new(Mutex::new(Upd::Unknown)),
            usb_tools: Arc::default(),
            update_at: None,
        };
        app.restart_pending = true;
        app.start_pending_kvm();
        app
    }

    fn sync_perms(&mut self) {
        if !self.cfg.active_peer.is_empty() {
            let bits = flows::bits(&self.cfg)
                | if self.cfg.allow_screen_out {
                    flows::SCREEN_OUT
                } else {
                    0
                };
            let id = self.cfg.active_peer.clone();
            self.cfg.device_perms.retain(|(i, _)| *i != id);
            self.cfg.device_perms.push((id.clone(), bits));
            if let Some(sh) = &self.shared {
                sh.device_perms.lock().unwrap().insert(id, bits);
            }
        }
        if let Some(sh) = &self.shared {
            sh.metrics_share
                .store(self.cfg.share_metrics, Ordering::Relaxed);
            sh.metrics_show
                .store(self.cfg.show_metrics, Ordering::Relaxed);
            if !self.cfg.show_metrics {
                sh.metrics_peer.lock().unwrap().clear();
            }
            *sh.perms.lock().unwrap() = Perms::from_cfg(&self.cfg);
            sh.remote_prefs
                .store(self.cfg.allow_remote_prefs, Ordering::Relaxed);
        }
        crate::platform::set_excluded(self.cfg.excluded.clone());
        self.cfg.save();
    }

    fn secret_valid(&self) -> bool {
        self.cfg.secret.is_empty() || crate::config::secret_ok(&self.cfg.secret)
    }

    fn sync_policy(&self) {
        if let Some(sh) = &self.shared {
            *sh.policy.lock().unwrap() = Policy::from_cfg(&self.cfg);
        }
        self.cfg.save();
    }

    fn unpair_device(&mut self, id: &str) {
        let connected = self
            .shared
            .as_ref()
            .is_some_and(|shared| shared.links.lock().unwrap().iter().any(|l| l.id == id));
        self.cfg.paired.retain(|(paired, _, _)| paired != id);
        self.cfg.trusted.retain(|(trusted, _)| trusted != id);
        self.cfg.held.retain(|held| held != id);
        self.cfg.unplace(id);
        self.cfg.peer_profiles.retain(|(i, _)| i != id);
        self.cfg.device_perms.retain(|(i, _)| i != id);
        if self.cfg.active_peer == id {
            self.cfg.activate_peer("");
            self.cfg.peer_profiles.retain(|(i, _)| i != id);
        }
        if let Some(sh) = &self.shared {
            sh.pairs.lock().unwrap().remove(id);
        }
        self.sync_policy();
        if connected {
            self.restart();
        }
    }

    fn hold_device(&mut self, id: &str) {
        if !self.cfg.held.iter().any(|held| held == id) {
            self.cfg.held.push(id.to_owned());
        }
        self.sync_policy();
        self.restart();
    }

    fn resume_device(&mut self, id: &str) {
        self.cfg.held.retain(|held| held != id);
        self.sync_policy();
    }

    fn absorb_learned(&mut self) {
        let Some(sh) = &self.shared else { return };
        let list: Vec<(String, String)> = sh.learned.lock().unwrap().drain(..).collect();
        let mut changed = false;
        for (id, name) in list {
            if !self.cfg.trusted.iter().any(|(i, _)| *i == id)
                && !self.cfg.blocked.iter().any(|(i, _)| *i == id)
            {
                self.cfg.trusted.push((id, name));
                changed = true;
            }
        }
        let pairs: Vec<(String, String, String)> = sh.new_pairs.lock().unwrap().drain(..).collect();
        for (id, name, secret) in pairs {
            self.cfg.paired.retain(|(i, _, _)| *i != id);
            self.cfg.held.retain(|held| *held != id);
            self.cfg.paired.push((id.clone(), name, secret));
            self.cfg.blocked.retain(|(i, _)| *i != id);
            changed = true;
        }
        if changed {
            self.cfg.save();
        }
    }

    fn request_banners(&mut self, ui: &mut egui::Ui) {
        let Some(sh) = self.shared.clone() else {
            return;
        };
        let pending: Vec<core::Request> = sh
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.decision.is_none())
            .cloned()
            .collect();
        for r in pending {
            let mut choice: Option<(Decision, bool)> = None;
            let signal = Palette::of(ui).signal;
            let who = if r.name.is_empty() {
                "Bir cihaz"
            } else {
                &r.name
            };
            let title = if r.sas.is_some() {
                format!("{who} sizinle eşleşmek istiyor")
            } else {
                format!("{who} bağlanmak istiyor")
            };
            notice(ui, signal, Icon::Link, &title, |ui| {
                if let Some(code) = &r.sas {
                    ui.add_space(4.0);
                    ui.label(RichText::new(code).font(FontId::monospace(30.0)));
                    hint(
                        ui,
                        &format!(
                            "Adres {}. Karşı PC'nin ekranındaki kod bununla aynıysa Eşleş'e basın, değilse Reddet.",
                            r.ip
                        ),
                    );
                } else {
                    hint(
                        ui,
                        &format!(
                            "Adres {}. Parolayı doğru girdi. Kabul ederseniz mouse, klavye ve dosya paylaşımı açılır.",
                            r.ip
                        ),
                    );
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    let accept = if r.sas.is_some() {
                        "Eşleş"
                    } else {
                        "Kabul et"
                    };
                    if design::key(ui, accept, KeyKind::Primary, true, 110.0).clicked() {
                        choice = Some((Decision::Accept, false));
                    }
                    if r.sas.is_none()
                        && design::key(ui, "Her zaman kabul et", KeyKind::Plain, true, 0.0)
                            .clicked()
                    {
                        choice = Some((Decision::Accept, true));
                    }
                    if design::key(ui, "Reddet", KeyKind::Plain, true, 0.0).clicked() {
                        choice = Some((Decision::Reject, false));
                    }
                    if design::key(ui, "Engelle", KeyKind::Danger, true, 0.0).clicked() {
                        choice = Some((Decision::Block, true));
                    }
                });
            });
            if let Some((d, remember)) = choice {
                if remember && !r.dev_id.is_empty() {
                    match d {
                        Decision::Accept => {
                            if !self.cfg.trusted.iter().any(|(i, _)| *i == r.dev_id) {
                                self.cfg.trusted.push((r.dev_id.clone(), r.name.clone()));
                            }
                        }
                        Decision::Block => {
                            self.cfg.trusted.retain(|(i, _)| *i != r.dev_id);
                            if !self.cfg.blocked.iter().any(|(i, _)| *i == r.dev_id) {
                                self.cfg.blocked.push((r.dev_id.clone(), r.name.clone()));
                            }
                        }
                        Decision::Reject => {}
                    }
                    self.sync_policy();
                }
                if let Some(x) = sh
                    .requests
                    .lock()
                    .unwrap()
                    .iter_mut()
                    .find(|x| x.id == r.id)
                {
                    x.decision = Some(d);
                }
            }
        }
    }

    fn set_pref(&mut self, pref: Pref, on: bool) {
        flows::set(&mut self.cfg, pref, on);
        if let Some(sh) = &self.shared {
            sh.apply_pref(pref, on);
        }
        self.sync_perms();
    }

    fn toggle_flow(&mut self, flow: Flow, on: bool) {
        let peer = self.peer_prefs();
        let id = self.cfg.active_peer.clone();
        self.toggle_flow_for(&id, flow, on, peer);
    }

    fn device_bits(&self, id: &str) -> u16 {
        let global = flows::bits(&self.cfg)
            | if self.cfg.allow_screen_out {
                flows::SCREEN_OUT
            } else {
                0
            };
        self.cfg
            .device_perms
            .iter()
            .find(|(d, _)| d == id)
            .map_or(global, |(_, bits)| *bits)
    }

    fn set_device_pref(&mut self, id: &str, bit: u16, on: bool) {
        let bits = self.device_bits(id);
        let bits = if on { bits | bit } else { bits & !bit };
        self.cfg.device_perms.retain(|(d, _)| d != id);
        self.cfg.device_perms.push((id.to_owned(), bits));
        if self.cfg.active_peer == id {
            if let Some(pref) = Pref::ALL.into_iter().find(|pref| pref.bit() == bit) {
                flows::set(&mut self.cfg, pref, on);
            } else if bit == flows::SCREEN_OUT {
                self.cfg.allow_screen_out = on;
            }
        }
        if let Some(sh) = &self.shared {
            sh.device_perms.lock().unwrap().insert(id.to_owned(), bits);
        }
        self.cfg.save();
    }

    fn toggle_flow_for(&mut self, id: &str, flow: Flow, on: bool, peer: Option<u16>) {
        for (at, pref, value) in flow.sync_changes(on) {
            match at {
                Where::Local if !id.is_empty() => self.set_device_pref(id, pref.bit(), value),
                Where::Local => self.set_pref(pref, value),
                Where::Remote => {
                    if let (Some(sh), Some(bits)) = (&self.shared, peer) {
                        if (bits & pref.bit() != 0) != value {
                            sh.out_prefs
                                .lock()
                                .unwrap()
                                .push((id.to_owned(), pref, value));
                        }
                    } else if let Some(sh) = &self.shared {
                        sh.out_prefs
                            .lock()
                            .unwrap()
                            .push((id.to_owned(), pref, value));
                    }
                }
            }
        }
    }

    fn absorb_remote_prefs(&mut self) {
        let Some(sh) = &self.shared else { return };
        let changed: Vec<(String, u16)> = sh
            .device_perms
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, bits)| !self.cfg.device_perms.contains(&((*id).clone(), **bits)))
            .map(|(id, bits)| (id.clone(), *bits))
            .collect();
        if !changed.is_empty() {
            for (id, bits) in changed {
                if self.cfg.active_peer == id {
                    for pref in Pref::ALL {
                        flows::set(&mut self.cfg, pref, bits & pref.bit() != 0);
                    }
                    self.cfg.allow_screen_out = bits & flows::SCREEN_OUT != 0;
                }
                self.cfg.device_perms.retain(|(d, _)| *d != id);
                self.cfg.device_perms.push((id, bits));
            }
            self.cfg.save();
        }
        let list: Vec<(Pref, bool)> = sh.in_prefs.lock().unwrap().drain(..).collect();
        if list.is_empty() {
            return;
        }
        for (pref, on) in list {
            flows::set(&mut self.cfg, pref, on);
        }
        self.cfg.save();
    }

    fn absorb_connected_peer(&mut self) {
        if self.sync_connected_peer() {
            self.cfg.save();
        }
        if let Some(sh) = &self.shared {
            *sh.config.lock().unwrap() = Some(self.cfg.clone());
        }
    }

    fn sync_connected_peer(&mut self) -> bool {
        let Some(sh) = self.shared.clone() else {
            return false;
        };
        let links = sh.links.lock().unwrap().clone();
        let selected = sh.primary_link().map(|l| l.id).unwrap_or_default();
        let profile_changed = self.cfg.activate_peer(&selected);
        let mut changed = profile_changed;
        for link in &links {
            if link.id.is_empty() {
                continue;
            }
            if self.cfg.side_of(&link.id).is_none() {
                self.cfg.place(&link.id, link.side, &link.parent);
                changed = true;
            }
            for (id, name) in &mut self.cfg.trusted {
                if *id == link.id && !link.name.is_empty() && *name != link.name {
                    *name = link.name.clone();
                    changed = true;
                }
            }
            for (id, name, _) in &mut self.cfg.paired {
                if *id == link.id && !link.name.is_empty() && *name != link.name {
                    *name = link.name.clone();
                    changed = true;
                }
            }
        }
        if changed {
            sh.apply_connection_settings(&self.cfg);
        }
        if profile_changed {
            self.screen_tex = None;
            self.peer_files = None;
            *self.remote.lock().unwrap() = Pane::default();
            for (kind, selected, devices) in [
                (Kind::Sink, &self.cfg.audio_sink, &self.audio.sinks),
                (Kind::Source, &self.cfg.audio_source, &self.audio.sources),
            ] {
                if !selected.is_empty() && devices.iter().any(|d| d.name == *selected) {
                    audio::set_default(kind, selected);
                }
            }
        }
        if self
            .popup_device
            .as_ref()
            .is_some_and(|id| !links.iter().any(|l| l.key() == *id))
        {
            self.popup_device = None;
        }
        changed
    }

    fn peer_prefs(&self) -> Option<u16> {
        self.shared
            .as_ref()
            .and_then(|sh| *sh.peer_prefs.lock().unwrap())
    }

    fn tray_snapshot(&self) -> crate::tray::Snapshot {
        let state = self
            .shared
            .as_ref()
            .map(|s| s.state.lock().unwrap().clone())
            .unwrap_or(State::Stopped);
        let (connected, peer) = match state {
            State::Connected { peer, .. } => (true, peer),
            _ => (false, String::new()),
        };
        crate::tray::Snapshot {
            running: self.sharing_on(),
            connected,
            peer,
            mine: flows::bits(&self.cfg),
            peer_prefs: self.peer_prefs(),
        }
    }

    fn tray_tick(&mut self, ctx: &egui::Context) {
        let Some(tray) = &self.tray else { return };
        let actions = tray.poll();
        let clicked = !actions.is_empty();
        for action in actions {
            match action {
                TrayAction::Popup(_)
                    if self
                        .tray_click
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(450)) =>
                {
                    self.tray_click = None;
                    self.show(ctx);
                }
                TrayAction::Popup(anchor) => {
                    self.tray_click = Some(Instant::now());
                    self.popup = if self.popup.is_some() {
                        None
                    } else {
                        Some(Popup::new(anchor))
                    };
                }
                TrayAction::Toggle(flow, on) => self.toggle_flow(flow, on),
                TrayAction::StartStop => self.toggle_access(),
                TrayAction::Open => self.show(ctx),
                TrayAction::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
        let snap = self.tray_snapshot();
        if let Some(tray) = &mut self.tray {
            tray.update(snap, clicked);
        }
    }

    fn request_grant(&self) {
        let out = self.grant_msg.clone();
        *out.lock().unwrap() = Some("bekleniyor".into());
        std::thread::spawn(move || {
            let r = crate::platform::grant_permissions();
            *out.lock().unwrap() = Some(match r {
                Ok(()) => "tamam".into(),
                Err(e) => format!("olmadı: {e}"),
            });
        });
    }

    fn running(&self) -> bool {
        self.shared
            .as_ref()
            .is_some_and(|s| !s.done.load(Ordering::Relaxed))
    }

    fn stopping(&self) -> bool {
        self.running() && self.shared.as_ref().is_some_and(|s| s.stopping())
    }

    fn sharing_on(&self) -> bool {
        self.running() && !self.stopping()
    }

    fn access_stopping(&self) -> bool {
        self.stopping()
    }

    fn can_start_access(&self) -> bool {
        !self.access_stopping() && self.cfg.w > 0 && self.secret_valid()
    }

    fn start_access(&mut self) {
        if !self.can_start_access() {
            return;
        }
        self.start();
    }

    fn stop_access(&mut self) {
        self.stop();
    }

    fn toggle_access(&mut self) {
        if self.access_stopping() {
            return;
        }
        if self.sharing_on() {
            self.stop_access();
        } else {
            self.start_access();
        }
    }

    fn pending_kvm_ready(&self) -> bool {
        self.restart_pending
            && !self.running()
            && self.perms.0
            && self.cfg.w > 0
            && self.secret_valid()
    }

    fn start_pending_kvm(&mut self) {
        if self.pending_kvm_ready() {
            self.restart_pending = false;
            self.start();
        }
    }

    fn access_status(
        &self,
        state: &State,
        running: bool,
        palette: Palette,
    ) -> (String, Color32, Icon) {
        if self.access_stopping() {
            return (
                "Bağlantı kapatılıyor…".into(),
                palette.signal,
                Icon::Refresh,
            );
        }
        status(state, running, palette)
    }

    fn start(&mut self) {
        if self.running() {
            self.restart_pending |= self.stopping();
            return;
        }
        self.cfg.save();
        let sh = Shared::new(false);
        let (cfg, sh2) = (self.cfg.clone(), sh.clone());
        std::thread::spawn(move || core::run(cfg, sh2));
        self.shared = Some(sh);
    }

    fn stop(&mut self) {
        self.restart_pending = false;
        if let Some(s) = &self.shared {
            s.request_stop();
        }
    }

    fn restart(&mut self) {
        self.cfg.save();
        if self.running() {
            self.stop();
            self.restart_pending = true;
        }
    }

    fn send_files(&self, files: Vec<PathBuf>) {
        let Some(sh) = &self.shared else { return };
        let dir = {
            let r = self.remote.lock().unwrap();
            (!r.path.is_empty() && r.error.is_none()).then(|| r.path.clone())
        };
        for f in files {
            match &dir {
                Some(d) => {
                    let (sh2, pane) = (sh.clone(), self.remote.clone());
                    transfer::upload(sh.clone(), f, d.clone(), move || reload_remote(&sh2, &pane));
                }
                None => transfer::send_file(sh.clone(), f, false),
            }
        }
    }

    fn open_local(&mut self, base: &str, child: &str) {
        let r = transfer::listing(base, child)
            .map(|(path, entries)| transfer::Listing { path, entries })
            .map_err(|e| std::io::Error::other(format!("Klasör açılamadı: {e}")));
        self.local.fill(r);
    }

    fn go_back(&mut self, remote: bool) {
        if remote {
            let target = {
                let mut r = self.remote.lock().unwrap();
                let target = r.back.pop();
                r.going_back = target.is_some();
                target
            };
            if let Some(path) = target {
                self.open_remote(path, String::new());
            }
        } else if let Some(path) = self.local.back.pop() {
            self.local.going_back = true;
            self.open_local(&path, "");
        }
    }

    fn make_folder(&mut self, remote: bool, name: String) {
        if remote {
            let Some(sh) = self.shared.clone() else {
                return;
            };
            let pane = self.remote.clone();
            let dir = pane.lock().unwrap().path.clone();
            std::thread::spawn(move || {
                let r = transfer::remote_mkdir(&sh, &dir, &name);
                pane.lock().unwrap().status = Some(match &r {
                    Ok(()) => (format!("“{name}” klasörü oluşturuldu."), false),
                    Err(e) => (format!("Klasör oluşturulamadı: {e}"), true),
                });
                if r.is_ok() {
                    reload_remote(&sh, &pane);
                }
            });
        } else {
            let r = transfer::make_dir(&self.local.path, &name);
            let path = self.local.path.clone();
            if r.is_ok() {
                self.local.going_back = true;
                self.open_local(&path, "");
            }
            self.local.status = Some(match r {
                Ok(_) => (format!("“{name}” klasörü oluşturuldu."), false),
                Err(e) => (format!("Klasör oluşturulamadı: {e}"), true),
            });
        }
    }

    fn delete_items(&mut self, remote: bool, names: Vec<String>) {
        let count = names.len();
        let summary = move |failed: Vec<String>| match failed.first() {
            None => (format!("{count} öğe silindi."), false),
            Some(first) => (format!("{} öğe silinemedi: {first}", failed.len()), true),
        };
        if remote {
            let Some(sh) = self.shared.clone() else {
                return;
            };
            let pane = self.remote.clone();
            let dir = pane.lock().unwrap().path.clone();
            std::thread::spawn(move || {
                let failed: Vec<String> = names
                    .iter()
                    .filter_map(|n| {
                        transfer::remote_delete(&sh, &dir, n)
                            .err()
                            .map(|e| format!("{n}: {e}"))
                    })
                    .collect();
                reload_remote(&sh, &pane);
                pane.lock().unwrap().status = Some(summary(failed));
            });
        } else {
            let dir = self.local.path.clone();
            let failed: Vec<String> = names
                .iter()
                .filter_map(|n| transfer::remove(&dir, n).err().map(|e| format!("{n}: {e}")))
                .collect();
            self.local.going_back = true;
            self.open_local(&dir, "");
            self.local.status = Some(summary(failed));
        }
    }

    fn file_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.file_dialog.as_mut() else {
            return;
        };
        let palette = Palette::new(ctx.global_style().visuals.dark_mode);
        let frame = egui::Frame::new()
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.rule_strong))
            .corner_radius(RADIUS_PANEL)
            .inner_margin(Margin::same(24));
        let mut done: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::unique("file_dialog")).frame(frame).show(ctx, |ui| {
            ui.set_width(420.0);
            match dialog {
                FileDialog::NewFolder { remote, name } => {
                    let place = if *remote { "Karşı bilgisayarda" } else { "Bu bilgisayarda" };
                    if modal_header(ui, "Yeni klasör", cfg!(target_os = "macos")).clicked() {
                        done = Some(false);
                    }
                    hint(ui, &format!("{place}, açık klasörün içinde oluşturulur."));
                    ui.add_space(12.0);
                    let r = ui.add(
                        egui::TextEdit::singleline(name)
                            .hint_text("Klasör adı")
                            .desired_width(f32::INFINITY)
                            .min_size(Vec2::new(0.0, 34.0))
                            .align(Align2::LEFT_CENTER),
                    );
                    if !r.has_focus() && !r.lost_focus() && name.is_empty() {
                        r.request_focus();
                    }
                    let invalid = name.chars().any(|c| "/\\:*?\"<>|".contains(c));
                    let ok = !name.trim().is_empty() && !invalid;
                    if invalid {
                        ui.add_space(6.0);
                        ui.label(RichText::new("Ad şu karakterleri içeremez: / \\ : * ? \" < > |").size(12.0).color(palette.bad));
                    }
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if design::key(ui, "Oluştur", KeyKind::Primary, ok, 110.0).clicked() || (enter && ok) {
                            done = Some(true);
                        }
                        if design::key(ui, "İptal", KeyKind::Plain, true, 90.0).clicked() {
                            done = Some(false);
                        }
                    });
                }
                FileDialog::Delete { remote, names } => {
                    if modal_header(ui, "Silinsin mi?", cfg!(target_os = "macos")).clicked() {
                        done = Some(false);
                    }
                    let place = if *remote { "karşı bilgisayardan" } else { "bu bilgisayardan" };
                    hint(ui, &format!(
                        "{} öğe {place} kalıcı olarak silinecek. Klasörler içindekilerle birlikte silinir; bu işlem geri alınamaz.",
                        names.len()
                    ));
                    ui.add_space(10.0);
                    for n in names.iter().take(6) {
                        ui.label(RichText::new(format!("• {n}")).size(13.0).color(palette.ink));
                    }
                    if names.len() > 6 {
                        ui.label(RichText::new(format!("ve {} öğe daha", names.len() - 6)).size(12.5).color(palette.muted));
                    }
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if design::key(ui, "Sil", KeyKind::Danger, true, 100.0).clicked() {
                            done = Some(true);
                        }
                        if design::key(ui, "İptal", KeyKind::Plain, true, 90.0).clicked() {
                            done = Some(false);
                        }
                    });
                }
            }
        });
        if modal.should_close() && done.is_none() {
            done = Some(false);
        }
        let Some(confirmed) = done else {
            return;
        };
        let Some(dialog) = self.file_dialog.take() else {
            return;
        };
        if !confirmed {
            return;
        }
        match dialog {
            FileDialog::NewFolder { remote, name } => {
                self.make_folder(remote, name.trim().to_owned())
            }
            FileDialog::Delete { remote, names } => self.delete_items(remote, names),
        }
    }

    fn open_remote(&self, base: String, child: String) {
        let Some(sh) = self.shared.clone() else {
            return;
        };
        let pane = self.remote.clone();
        {
            let mut p = pane.lock().unwrap();
            p.loading = true;
            p.error = None;
        }
        std::thread::spawn(move || {
            let r = transfer::list_remote(&sh, &base, &child);
            pane.lock().unwrap().fill(r);
        });
    }
}

fn apply_style(ctx: &egui::Context, theme: u8) {
    design::install_fonts(ctx);
    design::apply(ctx, theme);
}

fn section(ui: &mut egui::Ui, title: Option<&str>, add: impl FnOnce(&mut egui::Ui)) {
    design::rule(ui);
    ui.add_space(20.0);
    if let Some(title) = title {
        ui.label(design::strong(title, LEAD));
        ui.add_space(10.0);
    }
    add(ui);
    ui.add_space(24.0);
}

fn stream_controls(ui: &mut egui::Ui, id: &str, o: &mut screen::Opts) {
    egui::ComboBox::from_id_salt((id, "display_mode"))
        .selected_text(if o.extend.is_some() { "Ek monitör olarak kullan" } else { "Mevcut ekranı izle" })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut o.extend, None, "Mevcut ekranı izle");
            for (size, label) in [((1920, 1080), "Ek monitör: 1920×1080"), ((2560, 1440), "Ek monitör: 2560×1440"), ((3840, 2160), "Ek monitör: 3840×2160")] {
                ui.selectable_value(&mut o.extend, Some(size), label);
            }
        }).response.on_hover_text("Ek monitör modunda, ekranını izlediğiniz PC'nin masaüstü bu pencereye genişletilir. Windows'ta ekranı paylaşılan PC'ye sanal ekran sürücüsü kurulmalıdır.");
    let pick = |ui: &mut egui::Ui, key: &str, value: &mut u16, choices: &[(u16, &str)]| {
        let current = choices
            .iter()
            .find(|(v, _)| v == value)
            .map_or("Özel", |(_, n)| n);
        egui::ComboBox::from_id_salt((id, key))
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (v, n) in choices {
                    ui.selectable_value(value, *v, *n);
                }
            });
    };
    let mut q = o.quality as u16;
    pick(
        ui,
        "quality",
        &mut q,
        &[
            (40, "Kalite: düşük"),
            (60, "Kalite: orta"),
            (75, "Kalite: yüksek"),
            (90, "Kalite: en iyi"),
        ],
    );
    o.quality = q as u8;
    let mut fps = o.fps as u16;
    pick(
        ui,
        "fps",
        &mut fps,
        &[
            (5, "5 kare/sn"),
            (10, "10 kare/sn"),
            (15, "15 kare/sn"),
            (24, "24 kare/sn"),
            (30, "30 kare/sn"),
        ],
    );
    o.fps = fps as u8;
    pick(
        ui,
        "res",
        &mut o.max_w,
        &[
            (1280, "720p"),
            (1920, "1080p"),
            (2560, "1440p"),
            (0, "Tam çözünürlük"),
        ],
    );
    ui.checkbox(&mut o.cursor, "İmleç");
}

struct Node {
    key: String,
    id: String,
    name: String,
    status: &'static str,
    tone: Color32,
    live: bool,
    pinned: bool,
    parent: String,
    side: Side,
    cell: (i32, i32),
    link: Option<core::Link>,
}

struct CameraView {
    id: String,
    camera: String,
    title: String,
    view: Arc<Mutex<screen::View>>,
    handle: screen::Handle,
    texture: Option<(egui::TextureHandle, u64, u16)>,
    turns: Arc<AtomicU16>,
    window: bool,
    feeder: Option<crate::vcam::Feeder>,
}

enum LayoutAction {
    VirtualOn(String, hardware::Cap),
    VirtualOff,
    SetupVirtual,
    CloseCamera,
    Camera(String, hardware::Cap),
    Torch(String),
    Vibrate(String),
    Place(String, Side, String),
    Pin(String, Side),
    Remove(String),
    Flow(String, Flow, bool),
    ScreenOut(String, bool),
    Watch(String, bool),
    Usb(String),
}

const STRIP: f32 = 62.0;

use crate::layout::step;

fn link_points(
    from: egui::Rect,
    to: egui::Rect,
    side: Side,
    (from_foot, to_foot): (Vec2, Vec2),
) -> (egui::Pos2, egui::Pos2, [egui::Pos2; 2]) {
    match side {
        Side::Left => (
            from.left_center(),
            to.right_center(),
            [from.left_top(), from.left_bottom()],
        ),
        Side::Right => (
            from.right_center(),
            to.left_center(),
            [from.right_top(), from.right_bottom()],
        ),
        Side::Top => (
            from.center_top(),
            to.center_bottom() + to_foot,
            [from.left_top(), from.right_top()],
        ),
        Side::Bottom => (
            from.center_bottom() + from_foot,
            to.center_top(),
            [from.left_bottom(), from.right_bottom()],
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn mini_key(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    p: Palette,
    at: egui::Pos2,
    id: impl std::hash::Hash + std::fmt::Debug,
    icon: Icon,
    (on, peer_off, live): (bool, bool, bool),
    enabled: bool,
) -> egui::Response {
    let rect = egui::Rect::from_center_size(at, Vec2::splat(22.0));
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let r = ui.interact(rect, ui.scope_id().with(id), sense);
    let hot = enabled && (r.hovered() || r.has_focus());
    let (fill, border, ink) = match (enabled, on) {
        (false, _) => (Color32::TRANSPARENT, p.rule, p.rule_strong),
        (true, true) => (p.inset, p.ok, p.ok),
        (true, false) if peer_off => (if hot { p.hover } else { p.inset }, p.signal, p.ink2),
        (true, false) if hot => (p.hover, p.field, p.ink),
        (true, false) => (p.inset, p.rule_strong, p.ink2),
    };
    painter.circle(
        at,
        11.0,
        fill,
        Stroke::new(if on || peer_off { 1.5 } else { 1.0 }, border),
    );
    design::icon(painter, at, icon, ink, 12.0);
    if live {
        painter.circle_filled(at + Vec2::new(8.0, -8.0), 3.0, p.ok);
    }
    if enabled {
        r.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        r
    }
}

const PAIRED_NOTICE: Duration = Duration::from_secs(8);

fn code_boxes(ui: &mut egui::Ui, code: &str) {
    let palette = Palette::of(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (n, digit) in code.chars().filter(|c| !c.is_whitespace()).enumerate() {
            if n > 0 && n % 3 == 0 {
                ui.add_space(12.0);
            }
            let (rect, _) = ui.allocate_exact_size(Vec2::new(40.0, 50.0), Sense::hover());
            ui.painter().rect(
                rect,
                10,
                palette.surface,
                Stroke::new(1.0, palette.rule_strong),
                StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                digit,
                design::semibold(24.0),
                palette.ink,
            );
        }
    });
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
fn monitor(
    painter: &egui::Painter,
    palette: Palette,
    screen: egui::Rect,
    stand: f32,
    title: &str,
    sub: &str,
    sub_color: Color32,
    border: Color32,
    performance: bool,
) {
    let neck = egui::Rect::from_min_size(
        egui::pos2(screen.center().x - 5.0, screen.bottom()),
        Vec2::new(10.0, stand - 5.0),
    );
    let base = egui::Rect::from_center_size(
        egui::pos2(screen.center().x, screen.bottom() + stand - 2.5),
        Vec2::new(56.0, 5.0),
    );
    painter.rect_filled(neck, 0, palette.rule_strong);
    painter.rect_filled(base, 3, palette.rule_strong);
    painter.rect(
        screen,
        12,
        palette.surface,
        Stroke::new(1.5, border),
        StrokeKind::Inside,
    );
    painter.rect_filled(screen.shrink(6.0), 8, palette.hover);
    let mut job = egui::text::LayoutJob::simple_singleline(
        title.to_owned(),
        design::semibold(14.0),
        palette.ink,
    );
    job.wrap.max_width = screen.width() - 24.0;
    job.wrap.max_rows = 1;
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(
            screen.center().x - galley.size().x / 2.0,
            screen.center().y - if performance { 0.0 } else { 20.0 },
        ),
        galley,
        palette.ink,
    );
    painter.text(
        screen.center() + Vec2::new(0.0, if performance { 22.0 } else { 10.0 }),
        Align2::CENTER_CENTER,
        sub,
        FontId::proportional(12.0),
        sub_color,
    );
}

#[derive(Clone, Copy, PartialEq, Default)]
enum SortKey {
    #[default]
    Name,
    Modified,
    Size,
}

#[derive(Default)]
struct Pane {
    path: String,
    input: String,
    entries: Vec<transfer::Entry>,
    selected: BTreeSet<String>,
    anchor: Option<String>,
    error: Option<String>,
    loading: bool,
    peer: Option<String>,
    back: Vec<String>,
    going_back: bool,
    editing: bool,
    search: Option<String>,
    sort: SortKey,
    descending: bool,
    status: Option<(String, bool)>,
}

impl Pane {
    fn fill(&mut self, r: std::io::Result<transfer::Listing>) {
        self.loading = false;
        let going_back = std::mem::take(&mut self.going_back);
        match r {
            Ok(l) => {
                if !going_back && !self.path.is_empty() && self.path != l.path {
                    self.back.push(std::mem::take(&mut self.path));
                    if self.back.len() > 64 {
                        self.back.remove(0);
                    }
                }
                self.input = l.path.clone();
                self.path = l.path;
                self.entries = l.entries;
                self.selected.clear();
                self.anchor = None;
                self.error = None;
                self.editing = false;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn visible(&self) -> Vec<&transfer::Entry> {
        let needle = self.search.as_deref().unwrap_or("").trim().to_lowercase();
        let mut list: Vec<&transfer::Entry> = self
            .entries
            .iter()
            .filter(|e| needle.is_empty() || e.name.to_lowercase().contains(&needle))
            .collect();
        list.sort_by(|a, b| {
            let order = match self.sort {
                SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortKey::Modified => a.modified.cmp(&b.modified),
                SortKey::Size => a.size.cmp(&b.size),
            };
            b.dir.cmp(&a.dir).then(if self.descending {
                order.reverse()
            } else {
                order
            })
        });
        list
    }

    fn selected_items(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| self.selected.contains(&e.name))
            .map(|e| e.name.clone())
            .collect()
    }

    fn sort_by(&mut self, key: SortKey) {
        if self.sort == key {
            self.descending = !self.descending;
        } else {
            self.sort = key;
            self.descending = false;
        }
    }

    fn select(&mut self, name: &str, order: &[String], command: bool, shift: bool) {
        match (command, shift, self.anchor.clone()) {
            (_, true, Some(anchor)) => {
                let at = |n: &str| order.iter().position(|o| o == n);
                if let (Some(a), Some(b)) = (at(&anchor), at(name)) {
                    let (from, to) = (a.min(b), a.max(b));
                    if !command {
                        self.selected.clear();
                    }
                    self.selected.extend(order[from..=to].iter().cloned());
                }
            }
            (true, _, _) => {
                if !self.selected.remove(name) {
                    self.selected.insert(name.to_owned());
                }
                self.anchor = Some(name.to_owned());
            }
            _ => {
                self.selected.clear();
                self.selected.insert(name.to_owned());
                self.anchor = Some(name.to_owned());
            }
        }
    }
}

fn reload_remote(sh: &Arc<Shared>, pane: &Arc<Mutex<Pane>>) {
    let path = pane.lock().unwrap().path.clone();
    if !path.is_empty() {
        let r = transfer::list_remote(sh, &path, "");
        let mut p = pane.lock().unwrap();
        p.going_back = true;
        p.fill(r);
    }
}

fn path_crumbs(path: &str) -> Vec<(String, String)> {
    if path.is_empty() || path == transfer::DISKS {
        return vec![(transfer::DISKS.to_owned(), transfer::DISKS.to_owned())];
    }
    let windows = path.contains('\\') || path.as_bytes().get(1) == Some(&b':');
    let sep = if windows { '\\' } else { '/' };
    let mut out = Vec::new();
    let mut acc = String::new();
    if !windows {
        out.push(("/".to_owned(), "/".to_owned()));
        acc.push('/');
    }
    for part in path.split(['/', '\\']).filter(|p| !p.is_empty()) {
        if windows && acc.is_empty() {
            acc = format!("{part}{sep}");
        } else {
            if !acc.ends_with(sep) {
                acc.push(sep);
            }
            acc.push_str(part);
        }
        out.push((part.to_owned(), acc.clone()));
    }
    out
}

fn path_os_icon(path: &str) -> Icon {
    let lower = path.to_lowercase();
    if path.contains('\\') || path.as_bytes().get(1) == Some(&b':') {
        Icon::Windows
    } else if lower.starts_with("/storage")
        || lower.starts_with("/sdcard")
        || lower.contains("/android/")
    {
        Icon::Android
    } else if lower.starts_with("/users/") || lower.starts_with("/volumes") {
        Icon::Apple
    } else if lower.starts_with('/') {
        Icon::Linux
    } else {
        Icon::Screens
    }
}

fn local_os_icon() -> Icon {
    if cfg!(target_os = "windows") {
        Icon::Windows
    } else if cfg!(target_os = "macos") {
        Icon::Apple
    } else {
        Icon::Linux
    }
}

enum PaneAction {
    Open(String, String),
    Back,
    Transfer(Vec<String>),
    NewFolder,
    Delete(Vec<String>),
}

enum FileDialog {
    NewFolder { remote: bool, name: String },
    Delete { remote: bool, names: Vec<String> },
}

struct PaneHead<'a> {
    title: &'a str,
    subtitle: &'a str,
    os: Icon,
    remote: bool,
    can_act: bool,
    note: Option<&'a str>,
    hot: bool,
}

fn os_badge(ui: &mut egui::Ui, palette: Palette, icon: Icon, active: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
    let (fill, ink) = if active {
        (palette.accent, design::ON_ACCENT)
    } else {
        (palette.hover, palette.ink2)
    };
    ui.painter().rect_filled(rect, 10, fill);
    design::icon(ui.painter(), rect.center(), icon, ink, 20.0);
}

fn transfer_key(ui: &mut egui::Ui, text: &str, incoming: bool, enabled: bool) -> egui::Response {
    let palette = Palette::of(ui);
    let ink = if enabled {
        design::ON_ACCENT
    } else {
        palette.muted
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), design::medium(13.0), ink);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, r) = ui.allocate_exact_size(Vec2::new(galley.size().x + 46.0, 30.0), sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, text));
    let fill = match (enabled, r.hovered()) {
        (false, _) => palette.hover,
        (true, true) => palette.accent_edge,
        (true, false) => palette.accent,
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 8, fill);
    let (icon, icon_x, text_x) = if incoming {
        (Icon::CaretLeft, rect.left() + 17.0, rect.left() + 30.0)
    } else {
        (Icon::CaretRight, rect.right() - 17.0, rect.left() + 14.0)
    };
    design::icon(
        painter,
        egui::pos2(icon_x, rect.center().y),
        icon,
        ink,
        15.0,
    );
    painter.galley(
        egui::pos2(text_x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    if enabled {
        r.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        r
    }
}

fn crumb_label(ui: &mut egui::Ui, text: &str, last: bool) -> egui::Response {
    let palette = Palette::of(ui);
    let font = if last {
        design::semibold(13.0)
    } else {
        design::medium(13.0)
    };
    let ink = if last { palette.ink } else { palette.ink2 };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, ink);
    let (rect, r) = ui.allocate_exact_size(galley.size() + Vec2::new(10.0, 8.0), Sense::click());
    if r.hovered() {
        ui.painter().rect_filled(rect, 6, palette.hover);
    }
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, ink);
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn crumb_bar(ui: &mut egui::Ui, pane: &mut Pane, usable: bool, width: f32) -> Option<PaneAction> {
    let palette = Palette::of(ui);
    let mut action = None;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 32.0), Sense::hover());
    ui.painter().rect(
        rect,
        8,
        palette.inset,
        Stroke::new(1.0, palette.rule_strong),
        StrokeKind::Inside,
    );
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(6.0, 0.0)))
            .layout(Layout::right_to_left(Align::Center)),
    );
    inner.spacing_mut().item_spacing.x = 2.0;
    let menu = icon_button(&mut inner, Icon::CaretDown, "Konum", usable);
    egui::Popup::menu(&menu).show(|ui| {
        ui.set_min_width(190.0);
        if ui.button("Ev klasörü").clicked() {
            action = Some(PaneAction::Open(String::new(), String::new()));
        }
        if ui.button("Diskler").clicked() {
            action = Some(PaneAction::Open(transfer::DISKS.into(), String::new()));
        }
        if ui.button("Yolu yaz").clicked() {
            pane.editing = true;
        }
    });
    inner.with_layout(Layout::left_to_right(Align::Center), |ui| {
        if pane.editing {
            let r = ui.add(
                egui::TextEdit::singleline(&mut pane.input)
                    .frame(egui::Frame::NONE)
                    .desired_width(ui.available_width())
                    .font(FontId::proportional(13.0)),
            );
            if !r.has_focus() && !r.lost_focus() {
                r.request_focus();
            }
            if r.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    action = Some(PaneAction::Open(
                        pane.input.trim().to_owned(),
                        String::new(),
                    ));
                }
                pane.editing = false;
            }
            return;
        }
        egui::ScrollArea::horizontal()
            .id_salt(("crumbs", rect.left() as i32))
            .stick_to_right(true)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let crumbs = path_crumbs(&pane.path);
                    let count = crumbs.len();
                    for (n, (label, full)) in crumbs.into_iter().enumerate() {
                        if n > 0 && label != "/" {
                            let (at, _) =
                                ui.allocate_exact_size(Vec2::new(14.0, 24.0), Sense::hover());
                            design::icon(
                                ui.painter(),
                                at.center(),
                                Icon::CaretRight,
                                palette.muted,
                                12.0,
                            );
                        }
                        if usable && crumb_label(ui, &label, n + 1 == count).clicked() {
                            action = Some(PaneAction::Open(full, String::new()));
                        }
                    }
                });
            });
    });
    action
}

fn column_header(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    text: &str,
    key: SortKey,
    pane: &Pane,
    right: bool,
) -> egui::Response {
    let palette = Palette::of(ui);
    let r = ui.interact(rect, ui.scope_id().with(("column", text)), Sense::click());
    let active = pane.sort == key;
    let ink = if active || r.hovered() {
        palette.ink
    } else {
        palette.muted
    };
    let painter = ui.painter();
    let font = design::medium(12.0);
    let galley = painter.layout_no_wrap(text.to_owned(), font, ink);
    let width = galley.size().x;
    let x = if right {
        rect.right() - 8.0 - width
    } else {
        rect.left() + 8.0
    };
    painter.galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    if active {
        let arrow = if pane.descending {
            Icon::CaretDown
        } else {
            Icon::CaretUp
        };
        let ax = if right { x - 10.0 } else { x + width + 10.0 };
        design::icon(painter, egui::pos2(ax, rect.center().y), arrow, ink, 11.0);
    }
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn file_pane(
    ui: &mut egui::Ui,
    pane: &mut Pane,
    head: PaneHead,
    height: f32,
) -> Option<PaneAction> {
    let palette = Palette::of(ui);
    let mut action = None;
    let usable = head.note.is_none();
    let in_disks = pane.path == transfer::DISKS || pane.path.is_empty();
    let files = pane.selected_items();
    let outer = ui.max_rect();
    ui.set_width(ui.available_width());

    ui.horizontal(|ui| {
        os_badge(ui, palette, head.os, usable);
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(design::strong(head.title, 14.0));
            ui.add(
                egui::Label::new(RichText::new(head.subtitle).size(12.0).color(palette.muted))
                    .truncate(),
            );
        });
    });
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if icon_button(ui, Icon::CaretLeft, "Geri", usable && !pane.back.is_empty()).clicked() {
            action = Some(PaneAction::Back);
        }
        if icon_button(ui, Icon::CaretUp, "Üst klasör", usable && !in_disks).clicked() {
            action = Some(PaneAction::Open(pane.path.clone(), "..".into()));
        }
        let width = (ui.available_width() - 2.0 * 36.0).max(80.0);
        if let Some(a) = crumb_bar(ui, pane, usable, width) {
            action = Some(a);
        }
        let searching = pane.search.is_some();
        if icon_button(ui, Icon::Search, "Bu klasörde ara", usable).clicked() {
            pane.search = if searching { None } else { Some(String::new()) };
        }
        if icon_button(ui, Icon::Refresh, "Yenile", usable && !pane.path.is_empty()).clicked() {
            action = Some(PaneAction::Open(pane.path.clone(), String::new()));
        }
    });
    if let Some(search) = pane.search.as_mut() {
        ui.add_space(6.0);
        let r = ui.add(
            egui::TextEdit::singleline(search)
                .hint_text("Bu klasörde ara")
                .desired_width(ui.available_width())
                .min_size(Vec2::new(0.0, 30.0))
                .align(Align2::LEFT_CENTER),
        );
        if r.gained_focus() || search.is_empty() && !r.has_focus() && !r.lost_focus() {
            r.request_focus();
        }
        if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            pane.search = None;
        }
    }
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if icon_button(ui, Icon::Home, "Ev klasörü", usable).clicked() {
            action = Some(PaneAction::Open(String::new(), String::new()));
        }
        if icon_button(ui, Icon::FolderPlus, "Yeni klasör", usable && !in_disks).clicked() {
            action = Some(PaneAction::NewFolder);
        }
        if icon_button(
            ui,
            Icon::Trash,
            "Seçilenleri sil",
            usable && !in_disks && !pane.selected.is_empty(),
        )
        .clicked()
        {
            action = Some(PaneAction::Delete(pane.selected.iter().cloned().collect()));
        }
        let more = icon_button(ui, Icon::More, "Diğer", usable);
        egui::Popup::menu(&more).show(|ui| {
            ui.set_min_width(190.0);
            if ui.button("Tümünü seç").clicked() {
                pane.selected = pane.visible().iter().map(|e| e.name.clone()).collect();
            }
            if ui.button("Seçimi temizle").clicked() {
                pane.selected.clear();
            }
            if ui.button("Yolu kopyala").clicked() {
                ui.ctx().copy_text(pane.path.clone());
            }
            if ui.button("Diskler").clicked() {
                action = Some(PaneAction::Open(transfer::DISKS.into(), String::new()));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let count = files.len();
            let label = match (head.remote, count > 1) {
                (true, true) => format!("Al ({count})"),
                (true, false) => "Al".to_owned(),
                (false, true) => format!("Gönder ({count})"),
                (false, false) => "Gönder".to_owned(),
            };
            let tip = if head.remote {
                "Seçili dosya ve klasörleri bu bilgisayara al"
            } else {
                "Seçili dosya ve klasörleri karşıya gönder"
            };
            if transfer_key(ui, &label, head.remote, usable && head.can_act && count > 0)
                .on_hover_text(tip)
                .clicked()
            {
                action = Some(PaneAction::Transfer(files.clone()));
            }
        });
    });
    if let Some((text, error)) = &pane.status {
        ui.add_space(6.0);
        ui.label(RichText::new(text).size(12.5).color(if *error {
            palette.bad
        } else {
            palette.ok
        }));
    }
    ui.add_space(8.0);

    let (date_w, size_w) = (150.0, 86.0);
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    let name_rect = egui::Rect::from_min_max(
        row.min,
        egui::pos2(row.right() - date_w - size_w, row.bottom()),
    );
    let date_rect = egui::Rect::from_min_max(
        egui::pos2(name_rect.right(), row.top()),
        egui::pos2(row.right() - size_w, row.bottom()),
    );
    let size_rect = egui::Rect::from_min_max(egui::pos2(date_rect.right(), row.top()), row.max);
    let mut sort = None;
    if column_header(ui, name_rect, "Ad", SortKey::Name, pane, false).clicked() {
        sort = Some(SortKey::Name);
    }
    if column_header(
        ui,
        date_rect,
        "Değiştirildi",
        SortKey::Modified,
        pane,
        false,
    )
    .clicked()
    {
        sort = Some(SortKey::Modified);
    }
    if column_header(ui, size_rect, "Boyut", SortKey::Size, pane, true).clicked() {
        sort = Some(SortKey::Size);
    }
    if let Some(key) = sort {
        pane.sort_by(key);
    }
    let painter = ui.painter();
    for x in [name_rect.right(), date_rect.right()] {
        painter.vline(
            x,
            row.y_range().shrink(5.0),
            Stroke::new(1.0, palette.rule_strong),
        );
    }
    painter.hline(
        row.x_range(),
        row.bottom(),
        Stroke::new(1.0, palette.rule_strong),
    );

    let visible: Vec<transfer::Entry> = pane.visible().into_iter().cloned().collect();
    let order: Vec<String> = visible.iter().map(|e| e.name.clone()).collect();
    egui::ScrollArea::vertical()
        .id_salt(("pane", head.remote))
        .max_height(height)
        .min_scrolled_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let message = if let Some(n) = head.note {
                Some((n.to_owned(), palette.muted))
            } else if let Some(e) = &pane.error {
                Some((e.clone(), palette.bad))
            } else if pane.loading && pane.entries.is_empty() {
                Some(("Yükleniyor…".to_owned(), palette.muted))
            } else if visible.is_empty()
                && pane.search.as_deref().is_some_and(|s| !s.trim().is_empty())
            {
                Some(("Aramayla eşleşen öğe yok.".to_owned(), palette.muted))
            } else if visible.is_empty() && !pane.path.is_empty() {
                Some(("Bu klasör boş.".to_owned(), palette.muted))
            } else {
                None
            };
            if let Some((text, color)) = message {
                ui.add_space(12.0);
                ui.add(egui::Label::new(RichText::new(text).size(13.0).color(color)).wrap());
                ui.add_space(8.0);
                if head.note.is_some() || visible.is_empty() {
                    return;
                }
            }
            ui.spacing_mut().item_spacing.y = 2.0;
            let modifiers = ui.input(|i| i.modifiers);
            for (n, e) in visible.iter().enumerate() {
                let (rect, r) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let selected = pane.selected.contains(&e.name);
                let painter = ui.painter();
                if selected {
                    painter.rect_filled(rect, 6, palette.selected);
                } else if r.hovered() {
                    painter.rect_filled(rect, 6, palette.hover);
                } else if n % 2 == 0 {
                    painter.rect_filled(rect, 6, palette.inset);
                }
                let ink = if selected {
                    palette.signal
                } else {
                    palette.ink
                };
                let (icon, icon_ink) = if e.dir {
                    (Icon::Folder, palette.signal)
                } else {
                    (Icon::File, palette.ink2)
                };
                design::icon(
                    painter,
                    rect.left_center() + Vec2::new(16.0, 0.0),
                    icon,
                    icon_ink,
                    16.0,
                );
                let mut job = egui::text::LayoutJob::simple_singleline(
                    e.name.clone(),
                    FontId::proportional(13.0),
                    ink,
                );
                job.wrap.max_width = name_rect.width() - 40.0;
                job.wrap.max_rows = 1;
                let galley = painter.layout_job(job);
                painter.galley(
                    egui::pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0),
                    galley,
                    ink,
                );
                let small = FontId::proportional(12.0);
                painter.text(
                    egui::pos2(date_rect.left() + 8.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    format_time(e.modified),
                    small.clone(),
                    palette.muted,
                );
                let size = if e.dir {
                    String::new()
                } else {
                    format_size(e.size)
                };
                painter.text(
                    egui::pos2(rect.right() - 8.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    size,
                    small,
                    palette.muted,
                );
                if r.double_clicked() {
                    if e.dir {
                        action = Some(PaneAction::Open(pane.path.clone(), e.name.clone()));
                    } else if head.can_act {
                        action = Some(PaneAction::Transfer(vec![e.name.clone()]));
                    }
                } else if r.clicked() {
                    pane.select(&e.name, &order, modifiers.command, modifiers.shift);
                } else if r.secondary_clicked() && !selected {
                    pane.select(&e.name, &order, false, false);
                }
                r.context_menu(|ui| {
                    ui.set_min_width(180.0);
                    if e.dir && ui.button("Aç").clicked() {
                        action = Some(PaneAction::Open(pane.path.clone(), e.name.clone()));
                    }
                    let files = pane.selected_items();
                    let verb = if head.remote {
                        "Bu bilgisayara al"
                    } else {
                        "Karşıya gönder"
                    };
                    if ui
                        .add_enabled(head.can_act && !files.is_empty(), egui::Button::new(verb))
                        .clicked()
                    {
                        action = Some(PaneAction::Transfer(files));
                    }
                    if ui.button("Sil").clicked() {
                        action = Some(PaneAction::Delete(pane.selected.iter().cloned().collect()));
                    }
                });
            }
        });

    let typing = ui.ctx().memory(|m| m.focused().is_some());
    if usable && !typing && ui.rect_contains_pointer(outer) {
        let (delete, back, all) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::Backspace),
                i.modifiers.command && i.key_pressed(egui::Key::A),
            )
        });
        if delete && !pane.selected.is_empty() && !in_disks {
            action = Some(PaneAction::Delete(pane.selected.iter().cloned().collect()));
        } else if back && !pane.back.is_empty() {
            action = Some(PaneAction::Back);
        } else if all {
            pane.selected = pane.visible().iter().map(|e| e.name.clone()).collect();
        }
    }
    if head.hot {
        ui.painter().rect_stroke(
            ui.min_rect().expand(4.0),
            12,
            Stroke::new(1.5, palette.signal),
            StrokeKind::Outside,
        );
    }
    action
}

fn transfers_tile(ui: &mut egui::Ui, list: &[transfer::Transfer], height: f32) {
    let palette = Palette::of(ui);
    egui::Frame::new()
        .fill(palette.inset)
        .corner_radius(16)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(height);
            if list.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space((height / 2.0 - 30.0).max(0.0));
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
                    design::icon(ui.painter(), rect.center(), Icon::Share, palette.ink2, 24.0);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Devam eden aktarım yok")
                            .size(13.5)
                            .color(palette.ink2),
                    );
                });
                return;
            }
            let active = list.iter().filter(|t| t.status == Status::Active).count();
            ui.horizontal(|ui| {
                ui.label(design::strong("Aktarımlar", 14.0));
                if active > 0 {
                    ui.label(
                        RichText::new(format!("{active} dosya aktarılıyor"))
                            .size(12.0)
                            .color(palette.signal),
                    );
                }
            });
            ui.add_space(10.0);
            egui::ScrollArea::vertical()
                .id_salt("transfers")
                .max_height(height - 30.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (n, t) in list.iter().rev().enumerate() {
                        if n > 0 {
                            ui.add_space(4.0);
                            design::rule(ui);
                            ui.add_space(4.0);
                        }
                        transfer_row(ui, palette, t);
                    }
                });
        });
}

fn transfer_row(ui: &mut egui::Ui, palette: Palette, t: &transfer::Transfer) {
    ui.horizontal(|ui| {
        let (icon, tint, tip) = if t.incoming {
            (Icon::Download, palette.ok, "Bu bilgisayara geldi")
        } else {
            (Icon::Upload, palette.signal, "Karşı bilgisayara gitti")
        };
        let (rect, r) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::hover());
        design::icon(ui.painter(), rect.center(), icon, tint, 17.0);
        r.on_hover_text(tip);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add(egui::Label::new(RichText::new(&t.name).font(design::medium(13.0))).truncate())
                .on_hover_text(&t.name);
            ui.add(
                egui::Label::new(
                    RichText::new(format!("→ {}", t.dest))
                        .size(11.5)
                        .color(palette.muted),
                )
                .truncate(),
            )
            .on_hover_text(&t.dest);
            match &t.status {
                Status::Active => {
                    let f = if t.total == 0 {
                        0.0
                    } else {
                        t.done as f32 / t.total as f32
                    };
                    ui.horizontal(|ui| {
                        design::progress(ui, f, (ui.available_width() - 44.0).max(40.0));
                        ui.label(
                            RichText::new(format!("%{}", (f * 100.0).round() as u32))
                                .size(12.0)
                                .color(palette.ink2),
                        );
                    });
                }
                Status::Done(path) => {
                    let text = format!(
                        "{} · {}",
                        if t.incoming {
                            "Alındı"
                        } else {
                            "Gönderildi"
                        },
                        format_size(t.total)
                    );
                    ui.label(RichText::new(text).size(12.0).color(palette.ok))
                        .on_hover_text(path);
                }
                Status::Failed(e) => {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("Hata: {e}"))
                                .size(12.0)
                                .color(palette.bad),
                        )
                        .truncate(),
                    )
                    .on_hover_text(e);
                }
            }
        });
    });
}

fn format_time(secs: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(secs, 0)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

fn format_size(n: u64) -> String {
    const KB: f64 = 1024.0;
    let n = n as f64;
    if n < KB {
        format!("{n} bayt")
    } else if n < KB * KB {
        format!("{:.0} KB", n / KB)
    } else if n < KB * KB * KB {
        format!("{:.1} MB", n / KB / KB)
    } else {
        format!("{:.1} GB", n / KB / KB / KB)
    }
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, tip: &str, enabled: bool) -> egui::Response {
    let palette = Palette::of(ui);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(32.0), sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, tip));
    if enabled && r.hovered() {
        ui.painter().rect_filled(rect, 8, palette.hover);
    }
    design::icon(
        ui.painter(),
        rect.center(),
        icon,
        if enabled {
            palette.ink2
        } else {
            palette.rule_strong
        },
        18.0,
    );
    if enabled {
        r.on_hover_text(tip)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        r
    }
}

fn modal_close_button(ui: &mut egui::Ui) -> egui::Response {
    let palette = Palette::of(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Kapat"));
    let hovered = response.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 8, palette.bad);
    }
    let ink = if hovered {
        palette.ground
    } else {
        palette.ink2
    };
    let center = rect.center();
    let stroke = Stroke::new(1.6, ink);
    ui.painter().line_segment(
        [center + Vec2::new(-5.0, -5.0), center + Vec2::new(5.0, 5.0)],
        stroke,
    );
    ui.painter().line_segment(
        [center + Vec2::new(5.0, -5.0), center + Vec2::new(-5.0, 5.0)],
        stroke,
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            10,
            Stroke::new(2.0, palette.signal),
            StrokeKind::Outside,
        );
    }
    response
        .on_hover_text("Kapat")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn modal_header(ui: &mut egui::Ui, title: &str, macos: bool) -> egui::Response {
    let mut close = None;
    ui.horizontal(|ui| {
        if macos {
            close = Some(modal_close_button(ui));
            ui.add_space(4.0);
            ui.label(design::strong(title, LEAD));
        } else {
            ui.label(design::strong(title, LEAD));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                close = Some(modal_close_button(ui));
            });
        }
    });
    close.expect("modal başlığı kapatma denetimi oluşturmalı")
}

fn tile(ui: &mut egui::Ui, palette: Palette, selected: bool, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(palette.inset)
        .stroke(if selected {
            Stroke::new(1.5, palette.signal)
        } else {
            Stroke::new(1.0, palette.rule_strong)
        })
        .corner_radius(16)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn icon_badge(ui: &mut egui::Ui, palette: Palette, icon: Icon, selected: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
    let (fill, ink) = if selected {
        (palette.accent, design::ON_ACCENT)
    } else {
        (palette.hover, palette.ink2)
    };
    ui.painter().rect_filled(rect, 10, fill);
    design::icon(ui.painter(), rect.center(), icon, ink, 20.0);
}

fn status_pill(ui: &mut egui::Ui, palette: Palette, text: &str, color: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), design::CONTROL),
        Sense::hover(),
    );
    ui.painter().rect(
        rect,
        design::RADIUS_BUTTON,
        Color32::TRANSPARENT,
        Stroke::new(1.0, palette.rule_strong),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        design::medium(13.0),
        color,
    );
    response
}

fn notice(
    ui: &mut egui::Ui,
    tone: Color32,
    icon: Icon,
    title: &str,
    add: impl FnOnce(&mut egui::Ui),
) {
    let palette = Palette::of(ui);
    egui::Frame::new()
        .fill(palette.inset)
        .stroke(Stroke::new(1.0, tone))
        .corner_radius(design::RADIUS_BUTTON)
        .inner_margin(Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                design::icon(ui.painter(), rect.center(), icon, tone, 18.0);
                ui.label(design::strong(title, BODY).color(tone));
            });
            add(ui);
        });
    ui.add_space(12.0);
}

struct Developer {
    name: &'static str,
    initials: &'static str,
    github: &'static str,
    linkedin: &'static str,
}

const DEVELOPERS: [Developer; 2] = [
    Developer {
        name: "Muhammed Taymur",
        initials: "MT",
        github: "ByTaymur",
        linkedin: "https://www.linkedin.com/in/muhammed-t-aaa770281/",
    },
    Developer {
        name: "Serdar Kayışkıran",
        initials: "SK",
        github: "serdarkayiskiran",
        linkedin: "https://www.linkedin.com/in/serdarkayiskiran/",
    },
];

fn note_item(
    ui: &mut egui::Ui,
    tone: Color32,
    icon: Icon,
    title: &str,
    add: impl FnOnce(&mut egui::Ui),
) {
    let palette = Palette::of(ui);
    design::rule(ui);
    ui.add_space(10.0);
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(26.0, 20.0), Sense::hover());
        let dot = egui::pos2(rect.left() + 9.0, rect.center().y);
        ui.painter()
            .circle_filled(dot, 13.0, tone.gamma_multiply(0.16));
        design::icon(ui.painter(), dot, icon, tone, 15.0);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(design::strong(title, 13.5).color(palette.ink));
            add(ui);
        });
    });
    ui.add_space(10.0);
}

fn note_action(ui: &mut egui::Ui, text: &str, primary: bool) -> egui::Response {
    let palette = Palette::of(ui);
    let ink = if primary {
        design::ON_ACCENT
    } else {
        palette.ink
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), design::medium(12.0), ink);
    let size = Vec2::new(galley.size().x + 20.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text));
    let hot = response.hovered();
    let fill = match (primary, hot) {
        (true, false) => palette.accent,
        (true, true) => palette.accent_edge,
        (false, false) => palette.inset,
        (false, true) => palette.hover,
    };
    ui.painter().rect_filled(rect, 7.0, fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, ink);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).weak().small()).wrap());
}

fn setting_row<R>(
    ui: &mut egui::Ui,
    title: &str,
    description: Option<&str>,
    control_width: f32,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let total = ui.available_width();
    let text_width = (total - control_width - 16.0).max(150.0);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(text_width, 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(text_width);
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new(title).size(BODY)).wrap());
                if let Some(description) = description {
                    hint(ui, description);
                }
                ui.add_space(4.0);
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), control)
            .inner
    })
    .inner
}

fn toggle_row(
    ui: &mut egui::Ui,
    title: &str,
    description: Option<&str>,
    value: &mut bool,
    enabled: bool,
) -> egui::Response {
    setting_row(ui, title, description, 48.0, |ui| {
        design::toggle(ui, value, title, enabled)
    })
}

fn divider(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    design::rule(ui);
    ui.add_space(6.0);
}

fn settings_nav_item(
    ui: &mut egui::Ui,
    page: SettingsPage,
    selected: bool,
    update_ready: bool,
    compact: bool,
) -> egui::Response {
    let palette = Palette::of(ui);
    let width = ui.available_width();
    let height = if compact { 56.0 } else { 58.0 };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, page.title()));
    let fill = if selected {
        palette.selected
    } else if response.hovered() {
        palette.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, design::RADIUS_BUTTON, fill);
    if selected {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                rect.left_top() + Vec2::new(0.0, 10.0),
                rect.left_bottom() + Vec2::new(3.0, -10.0),
            ),
            2.0,
            palette.signal,
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            design::RADIUS_BUTTON + 1,
            Stroke::new(2.0, palette.signal),
            StrokeKind::Outside,
        );
    }
    let icon_center = if compact {
        rect.center()
    } else {
        egui::pos2(rect.left() + 23.0, rect.center().y)
    };
    design::icon(
        ui.painter(),
        icon_center,
        page.icon(),
        if selected {
            palette.signal
        } else {
            palette.ink2
        },
        if compact { 22.0 } else { 21.0 },
    );
    if !compact {
        let text_x = rect.left() + 46.0;
        ui.painter().text(
            egui::pos2(text_x, rect.center().y),
            Align2::LEFT_CENTER,
            page.title(),
            design::medium(15.0),
            if selected { palette.ink } else { palette.ink2 },
        );
    }
    if update_ready && page == SettingsPage::About {
        red_dot(
            ui.painter(),
            egui::pos2(rect.right() - 10.0, rect.top() + 12.0),
            4.0,
            palette,
        );
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if compact {
        response.on_hover_text(page.title())
    } else {
        response
    }
}

fn empty_state(ui: &mut egui::Ui, icon: Icon, title: &str, description: &str) {
    section(ui, None, |ui| {
        ui.set_min_height(200.0);
        let palette = Palette::of(ui);
        ui.vertical_centered(|ui| {
            ui.add_space(28.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
            design::icon(ui.painter(), rect.center(), icon, palette.muted, 28.0);
            ui.add_space(12.0);
            ui.label(design::strong(title, LEAD));
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(RichText::new(description).size(14.0).color(palette.muted)).wrap(),
            );
        });
    });
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(400));
        screenshot_on_request(ctx);
        if self.perm_at.elapsed() > Duration::from_secs(4) {
            self.perms = crate::platform::check_permissions();
            self.missing = crate::platform::unreadable_devices();
            self.devices = crate::platform::input_devices();
            self.perm_at = Instant::now();
        }
        if (!self.perms.0 || !self.perms.1)
            && !self.auto_grant_tried
            && crate::platform::can_grant()
        {
            self.auto_grant_tried = true;
            self.request_grant();
        }
        if !self.grant_handled && self.grant_msg.lock().unwrap().as_deref() == Some("tamam") {
            self.grant_handled = true;
            self.perms = crate::platform::check_permissions();
            self.missing = crate::platform::unreadable_devices();
            self.perm_at = Instant::now();
            self.restart_pending = true;
        }
        if self.audio_at.elapsed() > Duration::from_secs(2) {
            self.audio = AudioView::refresh();
            self.audio_at = Instant::now();
        }
        self.start_pending_kvm();
        let due = self
            .update_at
            .is_none_or(|t| t.elapsed() > Duration::from_secs(6 * 3600));
        if due {
            self.check_update();
        }
        self.absorb_learned();
        self.absorb_remote_prefs();
        self.absorb_connected_peer();
        self.tray_tick(ctx);
        if self.popup.is_some() || self.watching() {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        }
    }

    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        if self.logo.is_none() {
            let img = egui::ColorImage::from_rgba_unmultiplied([96, 96], &crate::logo::rgba(96));
            self.logo = Some(ctx.load_texture("logo", img, egui::TextureOptions::LINEAR));
        }
        self.popup_window(&ctx);
        self.screen_window(&ctx);
        self.camera_window(&ctx);
        if (!self.perms.0 || !self.perms.1) && self.perm_modal {
            self.perm_modal_window(&ctx);
        }
        if self.devs_modal {
            self.developers_window(&ctx);
        }
        let files: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !files.is_empty() {
            self.send_files(files);
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let running = self.running();
        let state = self
            .shared
            .as_ref()
            .map(|s| s.state.lock().unwrap().clone())
            .unwrap_or(State::Stopped);
        let notes = self.notice_count(&state);
        let update_ready = matches!(*self.update.lock().unwrap(), Upd::Available(_));

        let palette = Palette::new(ctx.global_style().visuals.dark_mode);
        let logo = self.logo.as_ref().map(|t| t.id());
        crate::chrome::resize_edges(root);
        crate::chrome::titlebar(root, logo, |ui| {
            self.top_controls(ui, crate::chrome::CONTROLS_WIDTH, notes)
        });
        egui::Panel::bottom("tabs")
            .show_separator_line(false)
            .exact_size(96.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(palette.surface))
            .show(root, |ui| {
                let bar = ui.max_rect();
                ui.painter().hline(
                    bar.x_range(),
                    bar.top() + 0.5,
                    Stroke::new(1.0, palette.rule),
                );
                let (width, gap) = (92.0, 12.0);
                let n = Page::ALL.len() as f32;
                let mut x = bar.center().x - (n * width + (n - 1.0) * gap) / 2.0;
                for page in Page::ALL {
                    let item = egui::Rect::from_min_size(
                        egui::pos2(x, bar.center().y - 34.0),
                        Vec2::new(width, 68.0),
                    );
                    x += width + gap;
                    let selected = self.page == page;
                    let response = ui.interact(
                        item,
                        ui.scope_id().with(("tab", page as u8)),
                        Sense::click(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(egui::Role::Tab, true, selected, page.tab())
                    });
                    let painter = ui.painter();
                    if selected {
                        painter.rect_filled(item, design::RADIUS_BUTTON, palette.accent);
                    } else if response.hovered() {
                        painter.rect_filled(item, design::RADIUS_BUTTON, palette.inset);
                    }
                    if response.has_focus() {
                        painter.rect_stroke(
                            item.expand(2.0),
                            design::RADIUS_BUTTON + 2,
                            Stroke::new(2.0, palette.signal),
                            StrokeKind::Outside,
                        );
                    }
                    let (icon_ink, text_ink) = if selected {
                        (design::ON_ACCENT, design::ON_ACCENT)
                    } else {
                        (palette.ink2, palette.muted)
                    };
                    design::icon(
                        painter,
                        item.center() - Vec2::new(0.0, 11.0),
                        page.icon(),
                        icon_ink,
                        24.0,
                    );
                    if page == Page::Settings && update_ready {
                        red_dot(
                            painter,
                            item.center() + Vec2::new(16.0, -22.0),
                            4.5,
                            palette,
                        );
                    }
                    painter.text(
                        item.center() + Vec2::new(0.0, 18.0),
                        Align2::CENTER_CENTER,
                        page.tab(),
                        if selected {
                            design::medium(13.5)
                        } else {
                            FontId::proportional(13.5)
                        },
                        text_ink,
                    );
                    if response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.page = page;
                    }
                }
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette.ground)
                    .inner_margin(Margin {
                        left: 16,
                        right: 16,
                        top: 0,
                        bottom: 16,
                    }),
            )
            .show(root, |ui| {
                egui::Frame::new()
                    .fill(palette.surface)
                    .corner_radius(RADIUS_PANEL)
                    .inner_margin(Margin {
                        left: 28,
                        right: 10,
                        top: 24,
                        bottom: 0,
                    })
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        ui.set_max_width(ui.available_width().min(design::MAX_CONTENT));
                        ui.label(
                            RichText::new(self.page.title())
                                .font(design::semibold(22.0))
                                .color(palette.ink),
                        );
                        ui.add_space(16.0);

                        self.request_banners(ui);

                        if self.page == Page::Settings {
                            self.page_settings(ui, &ctx);
                        } else {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    egui::Frame::new()
                                        .inner_margin(Margin {
                                            right: 18,
                                            bottom: 16,
                                            ..Default::default()
                                        })
                                        .show(ui, |ui| match self.page {
                                            Page::Share => self.page_share(ui, &state, running),
                                            Page::Audio => self.page_audio(ui),
                                            Page::Files => self.page_files(ui, &state, hovering),
                                            Page::Usb => self.page_usb(ui, &state),
                                            Page::Log => self.page_log(ui),
                                            Page::Settings => unreachable!(),
                                        });
                                });
                        }
                    });
            });
        if self.notes_open {
            self.notes_popover(&ctx, &state, notes);
        }
        if let Some(splash) = &mut self.splash {
            let logo = self.logo.as_ref().map(|t| t.id());
            if !splash.show(&ctx, logo) {
                self.splash = None;
            }
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn on_exit(&mut self) {
        self.cfg.save();
        self.stop();
    }
}

fn red_dot(painter: &egui::Painter, center: egui::Pos2, radius: f32, palette: Palette) {
    painter.circle_filled(center, radius + 1.5, palette.surface);
    painter.circle_filled(center, radius, palette.bad);
}

fn dir_title(dir: Dir) -> &'static str {
    match dir {
        Dir::Out => "Bu bilgisayardan karşıya",
        Dir::In => "Karşıdan bu bilgisayara",
    }
}

fn status(state: &State, running: bool, palette: Palette) -> (String, Color32, Icon) {
    match state {
        State::Stopped => ("Paylaşım kapalı".into(), palette.off, Icon::Off),
        State::Waiting if running => (
            "Bilgisayarlar aranıyor".into(),
            palette.signal,
            Icon::Search,
        ),
        State::Waiting => ("Paylaşım kapalı".into(), palette.off, Icon::Off),
        State::Connected {
            control: Control::Local,
            ..
        } => ("Bağlı".into(), palette.ok, Icon::Link),
        State::Connected {
            control: Control::Remote,
            ..
        } => ("Kontrol karşıda".into(), palette.ok, Icon::Link),
        State::Connected {
            control: Control::Controlled,
            ..
        } => ("Karşı PC kullanıyor".into(), palette.ok, Icon::Link),
    }
}

impl App {
    fn check_update(&mut self) {
        self.update_at = Some(Instant::now());
        let st = self.update.clone();
        {
            let mut s = st.lock().unwrap();
            if matches!(*s, Upd::Checking | Upd::Installing) {
                return;
            }
            *s = Upd::Checking;
        }
        std::thread::spawn(move || {
            let r = update::check();
            *st.lock().unwrap() = match r {
                Ok(Some(rel)) => Upd::Available(rel),
                Ok(None) => Upd::Latest,
                Err(e) => Upd::Failed(e),
            };
        });
    }

    fn install_update(&self, rel: update::Release) {
        let st = self.update.clone();
        *st.lock().unwrap() = Upd::Installing;
        std::thread::spawn(move || {
            if let Err(e) = update::install(&rel) {
                *st.lock().unwrap() = Upd::Failed(format!("Güncelleme kurulamadı: {e}"));
            }
        });
    }

    fn notice_count(&self, state: &State) -> usize {
        let perms_missing = !self.perms.0 || !self.perms.1;
        [
            self.has_update_notice(state),
            perms_missing,
            !self.secret_valid(),
        ]
        .into_iter()
        .filter(|shown| *shown)
        .count()
    }

    fn has_update_notice(&self, state: &State) -> bool {
        match &*self.update.lock().unwrap() {
            Upd::Available(_) | Upd::Installing => return true,
            Upd::Failed(e) if e.starts_with("Güncelleme kurulamadı") => return true,
            _ => {}
        }
        if !matches!(state, State::Connected { .. }) {
            return false;
        }
        self.shared
            .as_ref()
            .and_then(|sh| sh.peer_version.lock().unwrap().clone())
            .is_some_and(|v| {
                v.is_empty()
                    || update::newer(update::VERSION, &v)
                    || update::newer(&v, update::VERSION)
            })
    }

    fn notes_popover(&mut self, ctx: &egui::Context, state: &State, count: usize) {
        let palette = Palette::new(ctx.global_style().visuals.dark_mode);
        let screen = ctx.content_rect();
        let width = 360.0_f32.min(screen.width() - 32.0);
        let anchor = if self.bell_rect.is_positive() {
            self.bell_rect
        } else {
            egui::Rect::from_min_size(egui::pos2(screen.right() - 240.0, 16.0), Vec2::splat(32.0))
        };
        let left = (anchor.right() - width).clamp(16.0, (screen.right() - width - 16.0).max(16.0));
        let area = egui::Area::new(egui::Id::unique("notes_popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(left, anchor.bottom() + 10.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.rule_strong))
                    .corner_radius(RADIUS_PANEL)
                    .inner_margin(Margin::same(16))
                    .shadow(egui::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(90),
                    })
                    .show(ui, |ui| {
                        ui.set_width(width - 32.0);
                        ui.horizontal(|ui| {
                            ui.label(design::strong("Bildirimler", LEAD));
                            if count > 0 {
                                ui.label(
                                    RichText::new(count.to_string())
                                        .size(12.0)
                                        .color(palette.muted),
                                );
                            }
                        });
                        ui.add_space(10.0);
                        if count == 0 {
                            hint(ui, "Yeni bildirim yok.");
                            return;
                        }
                        self.update_banner(ui, state);
                        if !self.perms.0 || !self.perms.1 {
                            self.perm_banner(ui);
                        }
                        if !self.secret_valid() {
                            self.weak_secret_banner(ui);
                        }
                    });
            });
        let on_bell = ctx
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| self.bell_rect.contains(p));
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if escape || (area.response.clicked_elsewhere() && !on_bell) {
            self.notes_open = false;
        }
    }

    fn update_banner(&mut self, ui: &mut egui::Ui, state: &State) {
        let palette = Palette::of(ui);
        let upd = self.update.lock().unwrap().clone();
        let peer = match state {
            State::Connected { peer, .. } => Some(if peer.is_empty() {
                "Karşı PC".to_owned()
            } else {
                peer.clone()
            }),
            _ => None,
        };
        let peer_version = self
            .shared
            .as_ref()
            .and_then(|sh| sh.peer_version.lock().unwrap().clone());
        let open_page = |page: &str| crate::platform::open_path(Path::new(page));
        match upd {
            Upd::Available(rel) => {
                note_item(
                    ui,
                    palette.signal,
                    Icon::Download,
                    &format!("Mause'un yeni sürümü hazır: {}", rel.version),
                    |ui| {
                        hint(
                            ui,
                            &format!(
                                "Bu bilgisayarda {} var. Güncelleme bir dakika kadar sürer, Mause kendiliğinden yeniden açılır.",
                                update::VERSION
                            ),
                        );
                        ui.add_space(6.0);
                        if note_action(ui, "Güncelle", true).clicked() {
                            self.install_update(rel.clone());
                        }
                    },
                );
                return;
            }
            Upd::Installing => {
                note_item(
                    ui,
                    palette.signal,
                    Icon::Download,
                    "Güncelleniyor…",
                    |ui| {
                        hint(
                            ui,
                            "Yeni sürüm indirilip kuruluyor. Parola sorulursa girin; Mause birazdan yeniden açılacak.",
                        );
                    },
                );
                return;
            }
            Upd::Failed(e) if e.starts_with("Güncelleme kurulamadı") => {
                let mut retry = false;
                note_item(
                    ui,
                    palette.warn,
                    Icon::Alert,
                    "Güncelleme yapılamadı",
                    |ui| {
                        hint(ui, &e);
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            retry = note_action(ui, "Yeniden dene", true).clicked();
                            if note_action(ui, "İndirme sayfasını aç", false).clicked() {
                                open_page(&update::releases_page());
                            }
                        });
                    },
                );
                if retry {
                    *self.update.lock().unwrap() = Upd::Unknown;
                    self.check_update();
                }
                return;
            }
            _ => {}
        }
        let (Some(peer), Some(v)) = (peer, peer_version) else {
            return;
        };
        if v.is_empty() || update::newer(update::VERSION, &v) {
            note_item(
                ui,
                palette.warn,
                Icon::Alert,
                &format!("{peer} eski bir Mause kullanıyor"),
                |ui| {
                    hint(
                        ui,
                        &format!(
                            "Ekran ayarları, uzaktan kontrol ve USB gibi yeni özellikler iki tarafta aynı sürüm olunca çalışır. \
                         {peer} bilgisayarında Mause'u güncelleyin: orada Ayarlar → Hakkında → Güncellemeleri denetle. \
                         O düğme yoksa Mause'u oradan bir kez indirme sayfasından kurun."
                        ),
                    );
                    ui.add_space(6.0);
                    if note_action(ui, "İndirme sayfasını aç", false).clicked() {
                        open_page(&update::releases_page());
                    }
                },
            );
        } else if update::newer(&v, update::VERSION) {
            let mut check = false;
            note_item(
                ui,
                palette.warn,
                Icon::Alert,
                "Bu bilgisayardaki Mause eski",
                |ui| {
                    hint(
                        ui,
                        &format!(
                            "{peer} bilgisayarında Mause {v}, burada {} var. Yeni özellikler için bu bilgisayarı da güncelleyin.",
                            update::VERSION
                        ),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        check = note_action(ui, "Güncellemeyi denetle", true).clicked();
                        if note_action(ui, "İndirme sayfasını aç", false).clicked() {
                            open_page(&update::releases_page());
                        }
                    });
                },
            );
            if check {
                self.check_update();
            }
        }
    }

    fn weak_secret_banner(&mut self, ui: &mut egui::Ui) {
        let signal = Palette::of(ui).signal;
        note_item(ui, signal, Icon::Shield, "Ortak parola çok kısa", |ui| {
            hint(
                ui,
                "Ortak parola en az 4 karakter olmalı. Eşleşme kullanıyorsanız parolayı silebilirsiniz; ya da 6 haneli bir kod üretip aynısını diğer bilgisayara girin.",
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if note_action(ui, "Kod üret", true).clicked() {
                    self.cfg.secret = crate::config::random_secret();
                    self.show_secret = true;
                    self.cfg.save();
                }
                if note_action(ui, "Parolayı sil", false).clicked() {
                    self.cfg.secret.clear();
                    self.cfg.save();
                }
            });
        });
    }

    fn perm_banner(&mut self, ui: &mut egui::Ui) {
        let warning = Palette::of(ui).warn;
        note_item(ui, warning, Icon::Alert, "Giriş izni gerekiyor", |ui| {
            hint(
                ui,
                "Mause'un mouse ve klavyeyi yakalayabilmesi için sistem izni gerekiyor.",
            );
            ui.add_space(6.0);
            if note_action(ui, "İzinleri ayarla", true).clicked() {
                self.perm_modal = true;
            }
        });
    }

    fn developers_window(&mut self, ctx: &egui::Context) {
        let palette = Palette::new(ctx.global_style().visuals.dark_mode);
        let frame = egui::Frame::new()
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.rule_strong))
            .corner_radius(RADIUS_PANEL)
            .inner_margin(Margin::same(24));
        let modal = egui::Modal::new(egui::Id::unique("developers_modal"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(540.0);
                let palette = Palette::of(ui);
                if modal_header(ui, "Geliştirenler", cfg!(target_os = "macos")).clicked() {
                    ui.close();
                }
                ui.add_space(2.0);
                hint(
                    ui,
                    &format!(
                        "Mause {} bu kişiler tarafından geliştiriliyor.",
                        env!("CARGO_PKG_VERSION")
                    ),
                );
                ui.add_space(14.0);
                let mut open = None;
                ui.columns(DEVELOPERS.len(), |cols| {
                    for (ui, dev) in cols.iter_mut().zip(DEVELOPERS.iter()) {
                        egui::Frame::new()
                            .fill(palette.inset)
                            .stroke(Stroke::new(1.0, palette.rule))
                            .corner_radius(design::RADIUS_BUTTON)
                            .inner_margin(Margin::same(16))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let (rect, _) =
                                        ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                                    ui.painter()
                                        .circle_filled(rect.center(), 22.0, palette.accent);
                                    ui.painter().text(
                                        rect.center(),
                                        Align2::CENTER_CENTER,
                                        dev.initials,
                                        design::semibold(16.0),
                                        design::ON_ACCENT,
                                    );
                                    ui.add_space(4.0);
                                    ui.vertical(|ui| {
                                        ui.add_space(3.0);
                                        ui.label(design::strong(dev.name, BODY));
                                        ui.label(
                                            RichText::new(format!("@{}", dev.github))
                                                .size(design::META)
                                                .color(palette.muted),
                                        );
                                    });
                                });
                                ui.add_space(14.0);
                                let github = format!("https://github.com/{}", dev.github);
                                let github_accessible = format!("{} GitHub profilini aç", dev.name);
                                let linkedin_accessible =
                                    format!("{} LinkedIn profilini aç", dev.name);
                                let half = ((ui.available_width() - 8.0) / 2.0).floor();
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    if design::key_with(
                                        ui,
                                        Some(Icon::GitHub),
                                        "GitHub",
                                        &github_accessible,
                                        KeyKind::Plain,
                                        true,
                                        half,
                                    )
                                    .on_hover_text(&github)
                                    .clicked()
                                    {
                                        open = Some(github.clone());
                                    }
                                    if design::key_with(
                                        ui,
                                        Some(Icon::LinkedIn),
                                        "LinkedIn",
                                        &linkedin_accessible,
                                        KeyKind::Plain,
                                        true,
                                        half,
                                    )
                                    .on_hover_text(dev.linkedin)
                                    .clicked()
                                    {
                                        open = Some(dev.linkedin.to_owned());
                                    }
                                });
                            });
                    }
                });
                if let Some(url) = open {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            });
        if modal.should_close() {
            self.devs_modal = false;
        }
    }

    fn perm_modal_window(&mut self, ctx: &egui::Context) {
        let modal = egui::Modal::new(egui::Id::unique("perm_modal")).show(ctx, |ui| {
            ui.set_width(460.0);
            let palette = Palette::of(ui);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                design::icon(ui.painter(), rect.center(), Icon::Alert, palette.warn, 18.0);
                ui.label(design::strong("Giriş izni gerekiyor", BODY).color(palette.warn));
            });
            ui.add_space(8.0);
            let (msg, cmd) = crate::platform::permission_help();
            if !self.missing.is_empty() {
                ui.label(
                    RichText::new(format!("Okunamayan cihazlar: {}", self.missing.join(", ")))
                        .color(palette.bad),
                );
                hint(
                    ui,
                    "Bu cihazlar yakalanamadığı için klavye ve mouse karşıya geçmez ya da tuşlar bu PC'de de çalışır.",
                );
            }
            hint(ui, msg);
            ui.add_space(10.0);
            let panes = crate::platform::permission_panes();
            if !panes.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for (i, name) in panes.into_iter().enumerate() {
                        if design::key(ui, name, KeyKind::Primary, true, 0.0).clicked() {
                            crate::platform::open_permission_pane(i);
                        }
                    }
                });
                ui.add_space(6.0);
            }
            let state = self.grant_msg.lock().unwrap().clone();
            let failed = state.as_deref().is_some_and(|m| m.starts_with("olmadı"));
            if crate::platform::can_grant() {
                ui.horizontal_wrapped(|ui| {
                    let waiting = state.as_deref() == Some("bekleniyor");
                    if design::key(ui, "İzni ver", KeyKind::Primary, !waiting, 120.0).clicked() {
                        self.request_grant();
                    }
                    let (text, color) = match state.as_deref() {
                        Some("bekleniyor") => ("Parola penceresi açıldı, orada onaylayın.".to_owned(), palette.signal),
                        Some("tamam") => (
                            "Verildi. Kalıcı olması için oturumu bir kez kapatıp açın.".to_owned(),
                            palette.ok,
                        ),
                        Some(e) => (e.to_owned(), palette.bad),
                        None => ("Bilgisayarınızın parolası istenecek.".to_owned(), palette.muted),
                    };
                    ui.label(RichText::new(text).size(14.0).color(color));
                });
                ui.add_space(4.0);
            }
            ui.horizontal_wrapped(|ui| {
                if !cmd.is_empty() && (failed || !crate::platform::can_grant()) {
                    hint(ui, "Ya da terminalde:");
                    ui.code(cmd);
                    if design::key(ui, "Kopyala", KeyKind::Plain, true, 0.0).clicked() {
                        ui.ctx().copy_text(cmd.into());
                    }
                }
                if design::key(ui, "Yeniden denetle", KeyKind::Plain, true, 0.0).clicked() {
                    self.perms = crate::platform::check_permissions();
                }
                if design::key(ui, "Daha sonra", KeyKind::Plain, true, 0.0).clicked() {
                    ui.close();
                }
            });
        });
        if modal.should_close() {
            self.perm_modal = false;
        }
    }

    fn page_share(&mut self, ui: &mut egui::Ui, state: &State, running: bool) {
        self.pairing_notice(ui);
        self.share_computers(ui, state, running);
        self.kvm_layout(ui, state, running);
    }

    fn performance_visible(&self, rect: egui::Rect) -> bool {
        self.cfg.show_metrics
            && self.cfg.metrics_mask != 0
            && rect.height() >= 84.0
            && rect.width() >= 110.0
    }

    fn performance_row(
        &self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        palette: Palette,
        rect: egui::Rect,
        peer: Option<&str>,
    ) {
        if !self.performance_visible(rect) {
            return;
        }
        let sample = self
            .shared
            .as_ref()
            .and_then(|sh| match peer {
                Some(id) => sh.metrics_peer.lock().unwrap().get(id).cloned(),
                None => sh.metrics_local.lock().unwrap().clone(),
            })
            .filter(|s| s.at.elapsed() < Duration::from_secs(6));
        let count = self.cfg.metrics_mask.count_ones() as f32;
        let cell = ((rect.width() - 20.0) / count).min(34.0);
        let left = rect.center().x - count * cell / 2.0;
        let mut column = 0;
        for (category, name) in ["İşlemci", "RAM", "GPU", "Disk", "Ağ"].iter().enumerate() {
            if self.cfg.metrics_mask & (1 << category) == 0 {
                continue;
            }
            let center = egui::pos2(left + (column as f32 + 0.5) * cell, rect.top() + 15.0);
            column += 1;
            let value = sample.as_ref().and_then(|s| s.sample.ratio(category));
            let color = if sample.is_none() {
                palette.muted
            } else if value.is_some_and(|v| v >= 0.9) {
                palette.warn
            } else {
                palette.ink2
            };
            match category {
                0 => {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(center, Vec2::splat(8.0)),
                        1,
                        Stroke::new(1.2, color),
                        StrokeKind::Inside,
                    );
                    for d in [-2.0, 2.0] {
                        for side in [-1.0, 1.0] {
                            painter.line_segment(
                                [
                                    center + Vec2::new(d, side * 4.0),
                                    center + Vec2::new(d, side * 6.0),
                                ],
                                Stroke::new(1.0, color),
                            );
                            painter.line_segment(
                                [
                                    center + Vec2::new(side * 4.0, d),
                                    center + Vec2::new(side * 6.0, d),
                                ],
                                Stroke::new(1.0, color),
                            );
                        }
                    }
                }
                1 => {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(center, Vec2::new(13.0, 7.0)),
                        1,
                        Stroke::new(1.0, color),
                        StrokeKind::Inside,
                    );
                    for x in [-4.0, 0.0, 4.0] {
                        painter.rect_filled(
                            egui::Rect::from_center_size(
                                center + Vec2::new(x, 0.0),
                                Vec2::new(2.0, 3.0),
                            ),
                            0,
                            color,
                        );
                    }
                }
                2 => {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(center, Vec2::new(14.0, 9.0)),
                        1,
                        Stroke::new(1.0, color),
                        StrokeKind::Inside,
                    );
                    painter.circle_stroke(center, 2.5, Stroke::new(1.0, color));
                }
                3 => {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(center, Vec2::new(11.0, 13.0)),
                        2,
                        Stroke::new(1.0, color),
                        StrokeKind::Inside,
                    );
                    painter.circle_stroke(
                        center + Vec2::new(0.0, -1.0),
                        3.0,
                        Stroke::new(1.0, color),
                    );
                }
                _ => design::icon(painter, center, Icon::Link, color, 14.0),
            }
            let label = value.map_or_else(
                || {
                    if sample.is_some() && category == 4 {
                        "↕".into()
                    } else {
                        "—".into()
                    }
                },
                |v| format!("{:.0}%", v * 100.0),
            );
            painter.text(
                center + Vec2::new(0.0, 13.0),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(9.0),
                color,
            );
            let detail = sample.as_ref().map_or_else(|| if peer.is_some() { "Karşı PC performans paylaşımı kapalı, desteklenmiyor veya ölçüm bekleniyor.".to_owned() } else { "Ölçüm bekleniyor.".to_owned() }, |s| s.sample.detail(category));
            let response = ui.interact(
                egui::Rect::from_center_size(
                    center + Vec2::new(0.0, 6.0),
                    Vec2::new(cell - 2.0, 30.0),
                ),
                ui.scope_id()
                    .with(("performance", peer.unwrap_or("local"), category)),
                Sense::hover(),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{name}: {detail}"))
            });
            response.on_hover_text(detail);
        }
    }

    fn kvm_layout(&mut self, ui: &mut egui::Ui, state: &State, running: bool) {
        if ui.checkbox(&mut self.cfg.show_metrics, "Cihaz performanslarını göster").on_hover_text("İşlemci, RAM, GPU, disk ve ağ göstergeleri. Karşı PC de kendi performans paylaşımını açmalıdır.").changed() {
            self.sync_perms();
        }
        let palette = Palette::of(ui);
        let links: Vec<core::Link> = self
            .shared
            .as_ref()
            .map(|sh| sh.links.lock().unwrap().clone())
            .unwrap_or_default();
        let nodes = self.layout_nodes(&links, running, palette);
        let mut taken: HashSet<(i32, i32)> = nodes.iter().map(|n| n.cell).collect();
        taken.insert((0, 0));
        let xs = taken.iter().map(|c| c.0);
        let (min_x, max_x) = (xs.clone().min().unwrap_or(0), xs.max().unwrap_or(0));
        let ys = taken.iter().map(|c| c.1);
        let (min_y, max_y) = (ys.clone().min().unwrap_or(0), ys.max().unwrap_or(0));
        let (cols, rows) = ((max_x - min_x + 1) as f32, (max_y - min_y + 1) as f32);

        let action = egui::ScrollArea::horizontal()
            .id_salt("device_layout_horizontal")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let width = ui
                    .available_width()
                    .max(cols * 90.0 + (cols - 1.0) * 88.0 + 104.0);
                let aspect = (self.cfg.h as f32 / self.cfg.w.max(1) as f32).clamp(0.5, 0.75);
                let (gap_x, gap_y, margin) = (88.0, 64.0, 52.0);
                let mw = (width * 0.24)
                    .clamp(132.0, 190.0)
                    .min((width - 2.0 * margin - (cols - 1.0) * gap_x) / cols)
                    .max(90.0);
                let screen = Vec2::new(mw, (mw * aspect).round());
                let stand = 16.0;
                let strip = if nodes.iter().any(|n| n.link.is_some()) {
                    STRIP
                } else {
                    0.0
                };
                let monitor_h = screen.y + stand + strip;
                let canvas_h =
                    (rows * monitor_h + (rows - 1.0) * gap_y + 2.0 * margin).max(monitor_h + 120.0);
                let (canvas, _) =
                    ui.allocate_exact_size(Vec2::new(width, canvas_h), Sense::hover());
                let painter = ui.painter().clone();
                painter.rect(
                    canvas,
                    16,
                    palette.inset,
                    Stroke::new(1.0, palette.rule_strong),
                    StrokeKind::Inside,
                );
                let dots = painter.with_clip_rect(canvas.shrink(2.0));
                let mut y = canvas.top() + 14.0;
                while y < canvas.bottom() {
                    let mut x = canvas.left() + 14.0;
                    while x < canvas.right() {
                        dots.circle_filled(egui::pos2(x, y), 1.0, palette.rule_strong);
                        x += 22.0;
                    }
                    y += 22.0;
                }

                let lift = Vec2::new(0.0, (stand + strip) / 2.0);
                let foot_of = |key: &str| {
                    let has_strip = nodes.iter().any(|n| n.key == key && n.link.is_some());
                    Vec2::new(0.0, stand + if has_strip { STRIP } else { 0.0 })
                };
                let (mid_x, mid_y) = ((min_x + max_x) as f32 / 2.0, (min_y + max_y) as f32 / 2.0);
                let rect_of = |cell: (i32, i32)| {
                    let at = canvas.center()
                        + Vec2::new(
                            (cell.0 as f32 - mid_x) * (screen.x + gap_x),
                            (cell.1 as f32 - mid_y) * (monitor_h + gap_y),
                        )
                        - lift;
                    egui::Rect::from_center_size(at, screen)
                };
                let cell_of = |key: &str| {
                    nodes
                        .iter()
                        .find(|n| n.key == key)
                        .map_or((0, 0), |n| n.cell)
                };
                let own = rect_of((0, 0));
                monitor(
                    &painter,
                    palette,
                    own,
                    stand,
                    &self.cfg.name,
                    "Bu bilgisayar",
                    palette.ink2,
                    palette.rule_strong,
                    self.performance_visible(own),
                );
                self.performance_row(ui, &painter, palette, own, None);

                let mut action: Option<LayoutAction> = None;
                for node in &nodes {
                    let from = rect_of(cell_of(&node.parent));
                    let to = rect_of(node.cell);
                    let feet = (foot_of(&node.parent), foot_of(&node.key));
                    let (a, b, edge) = link_points(from, to, node.side, feet);
                    let tone = node.tone;
                    if node.live {
                        painter.line_segment([a, b], Stroke::new(2.0, tone));
                    } else {
                        painter.extend(egui::Shape::dashed_line(
                            &[a, b],
                            Stroke::new(1.5, tone),
                            6.0,
                            5.0,
                        ));
                    }
                    let mid = a + (b - a) / 2.0;
                    painter.circle(mid, 15.0, palette.surface, Stroke::new(1.5, tone));
                    design::icon(&painter, mid, Icon::Share, tone, 15.0);
                    let inset = if matches!(node.side, Side::Left | Side::Right) {
                        Vec2::new(0.0, 12.0)
                    } else {
                        Vec2::new(12.0, 0.0)
                    };
                    painter
                        .line_segment([edge[0] + inset, edge[1] - inset], Stroke::new(3.0, tone));
                    if let Some(a) = self.layout_device(ui, &painter, palette, to, stand, node) {
                        action = Some(a);
                    }
                }

                let anchors = std::iter::once((String::new(), String::new(), (0, 0), None)).chain(
                    nodes
                        .iter()
                        .filter(|n| !n.id.is_empty())
                        .map(|n| (n.key.clone(), n.id.clone(), n.cell, Some(n.side.opposite()))),
                );
                for (key, id, cell, back) in anchors {
                    let rect = rect_of(cell);
                    for side in Side::ALL {
                        if Some(side) == back || taken.contains(&step(cell, side)) {
                            continue;
                        }
                        let at = match side {
                            Side::Left => rect.left_center() - Vec2::new(36.0, 0.0),
                            Side::Right => rect.right_center() + Vec2::new(36.0, 0.0),
                            Side::Top => rect.center_top() - Vec2::new(0.0, 30.0),
                            Side::Bottom => {
                                rect.center_bottom() + foot_of(&key) + Vec2::new(0.0, 30.0)
                            }
                        };
                        if let Some(a) =
                            self.layout_plus(ui, &painter, palette, (&key, &id), side, at, &links)
                        {
                            action = Some(a);
                        }
                    }
                }
                action
            })
            .inner;
        match action {
            Some(LayoutAction::Place(id, side, parent)) => {
                let unpinned = nodes
                    .iter()
                    .find(|n| n.id == parent && !n.pinned && !parent.is_empty());
                if let Some(p) = unpinned {
                    self.cfg.place(&parent, p.side, "");
                }
                self.cfg.place(&id, side, &parent);
                self.restart();
            }
            Some(LayoutAction::Pin(id, side)) => {
                self.cfg.place(&id, side, "");
                self.cfg.save();
            }
            Some(LayoutAction::Remove(id)) => {
                self.cfg.unplace(&id);
                self.restart();
            }
            Some(
                action @ (LayoutAction::Camera(..)
                | LayoutAction::VirtualOn(..)
                | LayoutAction::VirtualOff
                | LayoutAction::SetupVirtual
                | LayoutAction::CloseCamera
                | LayoutAction::Torch(..)
                | LayoutAction::Vibrate(..)),
            ) => self.hardware_action(action),
            Some(LayoutAction::Flow(id, flow, on)) => self.device_flow(&id, flow, on),
            Some(LayoutAction::ScreenOut(id, on)) => {
                self.set_device_pref(&id, flows::SCREEN_OUT, on)
            }
            Some(LayoutAction::Watch(id, on)) => {
                if let (true, Some(sh)) = (on, &self.shared) {
                    sh.set_target(core::Feat::Screen, &id);
                }
                self.set_watching(on);
            }
            Some(LayoutAction::Usb(id)) => {
                if let Some(sh) = &self.shared {
                    let now = sh.target(core::Feat::Usb);
                    sh.set_target(core::Feat::Usb, if now == id { "" } else { &id });
                }
            }
            None => {}
        }

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let blocked = !self.cfg.allow_control_out;
            let (label, kind, tip) = if blocked {
                ("Karşıya geçişi aç", KeyKind::Primary, "Mouse ve klavye yeniden kenardan karşıya geçebilir.")
            } else {
                (
                    "Kontrolü geri al",
                    KeyKind::Plain,
                    "Kontrol hemen bu bilgisayara döner ve karşıya geçiş kapanır. Acil kısayol: Ctrl + Alt + Home",
                )
            };
            if design::key(ui, label, kind, true, 170.0).on_hover_text(tip).clicked() {
                self.set_pref(Pref::ControlOut, blocked);
            }
            if blocked {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, palette.off);
                ui.label(RichText::new("Karşıya geçiş kapalı").size(13.0).color(palette.ink2));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{} × {}", self.cfg.w, self.cfg.h)).monospace().color(palette.muted));
            });
        });
        ui.add_space(14.0);
        design::rule(ui);
        ui.add_space(14.0);
        self.link_note(ui, state);
    }

    fn device_name(&self, id: &str) -> Option<String> {
        self.cfg
            .paired
            .iter()
            .find(|(i, _, _)| i == id)
            .map(|(_, n, _)| n.clone())
            .or_else(|| {
                self.cfg
                    .trusted
                    .iter()
                    .find(|(i, _)| i == id)
                    .map(|(_, n)| n.clone())
            })
    }

    fn layout_nodes(&self, links: &[core::Link], running: bool, palette: Palette) -> Vec<Node> {
        let live = |l: &core::Link, pinned: bool, parent: String, side: Side| Node {
            key: l.key(),
            id: l.id.clone(),
            name: if l.name.is_empty() {
                "Karşı bilgisayar".to_owned()
            } else {
                l.name.clone()
            },
            status: match l.control {
                Control::Local => "Bağlı",
                Control::Remote => "Mouse burada",
                Control::Controlled => "Bu bilgisayarı yönetiyor",
            },
            tone: palette.ok,
            live: true,
            pinned,
            parent,
            side,
            cell: (0, 0),
            link: Some(l.clone()),
        };
        let (status, tone) = if running {
            ("Aranıyor", palette.signal)
        } else {
            ("Bağlı değil", palette.off)
        };
        let mut pending: Vec<Node> = self
            .cfg
            .placed
            .iter()
            .filter(|(id, ..)| !running || links.iter().any(|l| l.id == *id))
            .map(
                |(id, side, parent)| match links.iter().find(|l| l.id == *id) {
                    Some(l) => live(l, true, l.parent.clone(), l.side),
                    None => Node {
                        key: id.clone(),
                        id: id.clone(),
                        name: self
                            .device_name(id)
                            .unwrap_or_else(|| "Karşı bilgisayar".to_owned()),
                        status,
                        tone,
                        live: false,
                        pinned: true,
                        parent: parent.clone(),
                        side: *side,
                        cell: (0, 0),
                        link: None,
                    },
                },
            )
            .collect();
        for l in links {
            if l.id.is_empty() || self.cfg.side_of(&l.id).is_none() {
                pending.push(live(l, false, l.parent.clone(), l.side));
            }
        }
        pending.sort_by_key(|node| !node.live);
        let positions = crate::layout::resolve(
            pending
                .iter()
                .map(|node| crate::layout::Placement {
                    key: node.key.clone(),
                    parent: node.parent.clone(),
                    side: node.side,
                    cell: (0, 0),
                })
                .collect(),
        );
        positions
            .into_iter()
            .filter_map(|position| {
                let index = pending.iter().position(|node| node.key == position.key)?;
                let mut node = pending.remove(index);
                node.parent = position.parent;
                node.side = position.side;
                node.cell = position.cell;
                Some(node)
            })
            .collect()
    }

    fn layout_candidates(
        &self,
        parent: &str,
        side: Side,
        links: &[core::Link],
    ) -> Vec<(String, String, Option<Side>)> {
        let mut list: Vec<(String, String, Option<Side>)> = self
            .cfg
            .paired
            .iter()
            .map(|(id, name, _)| (id.clone(), name.clone(), self.cfg.side_of(id)))
            .collect();
        for l in links {
            if l.id.is_empty() {
                continue;
            }
            match list.iter_mut().find(|(id, _, _)| *id == l.id) {
                Some(entry) => entry.2 = entry.2.or(Some(l.side)),
                None => list.push((l.id.clone(), l.name.clone(), Some(l.side))),
            }
        }
        let blocked = self.cfg.ancestry(parent);
        list.retain(|(id, _, _)| {
            !blocked.contains(id)
                && !(self.cfg.parent_of(id) == Some(parent) && self.cfg.side_of(id) == Some(side))
        });
        list
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_plus(
        &self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        palette: Palette,
        (key, parent): (&str, &str),
        side: Side,
        at: egui::Pos2,
        links: &[core::Link],
    ) -> Option<LayoutAction> {
        let hit = egui::Rect::from_center_size(at, Vec2::splat(32.0));
        let r = ui.interact(
            hit,
            ui.scope_id().with(("plus", key, side.to_u8())),
            Sense::click(),
        );
        let label = format!("{} tarafa bilgisayar ekle", capitalize(side.tr()));
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone()));
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&r));
        let hot = r.hovered() || r.has_focus() || open;
        let (fill, ink) = if hot {
            (palette.accent, design::ON_ACCENT)
        } else {
            (palette.surface, palette.ink2)
        };
        painter.circle(
            at,
            15.0,
            fill,
            Stroke::new(
                1.0,
                if hot {
                    palette.accent
                } else {
                    palette.rule_strong
                },
            ),
        );
        design::icon(painter, at, Icon::Plus, ink, 14.0);
        let r = r
            .on_hover_text(label)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let mut chosen = None;
        egui::Popup::menu(&r).show(|ui| {
            let candidates = self.layout_candidates(parent, side, links);
            ui.set_min_width(220.0);
            ui.label(
                RichText::new(format!("{} tarafa ekle", capitalize(side.tr())))
                    .size(13.0)
                    .color(palette.muted),
            );
            if candidates.is_empty() {
                ui.label(
                    RichText::new(
                        "Eşleşmiş başka bilgisayar yok. Önce yukarıdaki listeden eşleştirin.",
                    )
                    .size(13.0)
                    .color(palette.ink2),
                );
            }
            for (id, name, at) in &candidates {
                let text = match at {
                    Some(_) => format!("{name}  (buraya taşı)"),
                    None => name.clone(),
                };
                if ui.button(text).clicked() {
                    chosen = Some(LayoutAction::Place(id.clone(), side, parent.to_owned()));
                }
            }
        });
        chosen
    }

    fn layout_device(
        &self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        palette: Palette,
        rect: egui::Rect,
        stand: f32,
        node: &Node,
    ) -> Option<LayoutAction> {
        let r = ui.interact(
            rect,
            ui.scope_id().with(("device", &node.key)),
            Sense::click(),
        );
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&r));
        let border = if r.hovered() || open {
            palette.accent
        } else {
            node.tone
        };
        monitor(
            painter,
            palette,
            rect,
            stand,
            &node.name,
            node.status,
            node.tone,
            border,
            node.live && self.performance_visible(rect),
        );
        if node.live {
            self.performance_row(ui, painter, palette, rect, Some(&node.id));
        }
        let r = if node.live && self.performance_visible(rect) {
            r
        } else {
            r.on_hover_text(format!("{}: {}", node.name, node.status))
        };
        let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
        let mut chosen = node
            .link
            .as_ref()
            .and_then(|link| self.device_strip(ui, painter, palette, rect, stand, link));
        egui::Popup::menu(&r).show(|ui| {
            ui.set_min_width(200.0);
            ui.label(RichText::new(&node.name).size(13.0).color(palette.muted));
            if node.id.is_empty() {
                ui.label(
                    RichText::new("Bu bağlantı eşleşme olmadan kuruldu.")
                        .size(13.0)
                        .color(palette.ink2),
                );
            } else if !node.pinned {
                if ui.button("Bu konuma sabitle").clicked() {
                    chosen = Some(LayoutAction::Pin(node.id.clone(), node.side));
                }
            } else if ui.button("Bu konumdan kaldır").clicked() {
                chosen = Some(LayoutAction::Remove(node.id.clone()));
            }
            let info = self
                .shared
                .as_ref()
                .and_then(|sh| sh.devices.lock().unwrap().get(&node.id).cloned());
            if let (Some(info), false) = (info, node.id.is_empty()) {
                ui.separator();
                let line = hardware::status_line(&info.status);
                if !line.is_empty() {
                    ui.label(RichText::new(line).size(12.5).color(palette.ink2));
                }
                if info.caps.is_empty() {
                    ui.label(
                        RichText::new("Telefonda donanım paylaşımı kapalı.")
                            .size(12.5)
                            .color(palette.muted),
                    );
                }
                for cap in &info.caps {
                    match cap.kind.as_str() {
                        "camera" => {
                            if ui.button(format!("Kamera: {}", cap.label)).clicked() {
                                chosen = Some(LayoutAction::Camera(node.id.clone(), cap.clone()));
                            }
                        }
                        "torch" => {
                            let on = self.torch_on.contains(&node.id);
                            let text = if on {
                                "El fenerini kapat"
                            } else {
                                "El fenerini aç"
                            };
                            if ui.button(text).clicked() {
                                chosen = Some(LayoutAction::Torch(node.id.clone()));
                            }
                        }
                        "vibrate" if ui.button("Telefonu titret").clicked() => {
                            chosen = Some(LayoutAction::Vibrate(node.id.clone()));
                        }
                        _ => {}
                    }
                }
            }
        });
        chosen
    }

    fn hardware_action(&mut self, action: LayoutAction) {
        match action {
            LayoutAction::Camera(id, cap) => {
                let ready = self.ensure_camera(&id, &cap);
                if let (true, Some(camera)) = (ready, &mut self.camera) {
                    camera.window = true;
                }
            }
            LayoutAction::CloseCamera => {
                let idle = self.camera.as_mut().map(|camera| {
                    camera.window = false;
                    camera.feeder.is_none()
                });
                if idle == Some(true) {
                    self.close_camera();
                }
            }
            LayoutAction::VirtualOn(id, cap) => {
                let ready = self.ensure_camera(&id, &cap);
                if let (true, Some(camera)) = (ready, &mut self.camera) {
                    match crate::vcam::start(camera.view.clone(), camera.turns.clone()) {
                        Ok(feeder) => camera.feeder = Some(feeder),
                        Err(error) => {
                            if let Some(sh) = &self.shared {
                                sh.log(format!("Sanal kamera başlatılamadı: {error}"));
                            }
                        }
                    }
                }
            }
            LayoutAction::VirtualOff => {
                let idle = self.camera.as_mut().map(|camera| {
                    camera.feeder = None;
                    !camera.window
                });
                if idle == Some(true) {
                    self.close_camera();
                }
            }
            LayoutAction::SetupVirtual => {
                let note = self.vcam_note.clone();
                *note.lock().unwrap() =
                    Some("Kuruluyor… yönetici parolası penceresi açılabilir.".into());
                std::thread::spawn(move || {
                    *note.lock().unwrap() = Some(match crate::vcam::setup() {
                        Ok(()) => "Sanal kamera hazır. Bağla'ya basabilirsiniz.".into(),
                        Err(error) => format!("Kurulamadı: {error}"),
                    });
                });
            }
            LayoutAction::Torch(id) => {
                let on = !self.torch_on.remove(&id);
                if on {
                    self.torch_on.insert(id.clone());
                }
                self.send_device(&id, hardware::TORCH, vec![on as u8]);
            }
            LayoutAction::Vibrate(id) => self.send_device(&id, hardware::VIBRATE, Vec::new()),
            _ => {}
        }
    }

    fn hardware_section(&mut self, ui: &mut egui::Ui) {
        let Some(sh) = self.shared.clone() else {
            return;
        };
        let links = sh.links.lock().unwrap().clone();
        if links.is_empty() {
            return;
        }
        let devices = sh.devices.lock().unwrap().clone();
        let palette = Palette::of(ui);
        let virtual_ready = crate::vcam::available();
        let vcam_note = self.vcam_note.lock().unwrap().clone();
        let mut action: Option<LayoutAction> = None;
        section(ui, Some("Karşı cihazın donanımı"), |ui| {
            for (index, link) in links.iter().enumerate() {
                if index > 0 {
                    divider(ui);
                }
                ui.label(design::strong(&link.name, LEAD));
                let Some(info) = devices.get(&link.id) else {
                    hint(
                        ui,
                        "Bu cihazdan donanım bilgisi gelmedi. Cihazdaki Mause'un güncel olması gerekir (telefonda en son APK).",
                    );
                    continue;
                };
                let line = hardware::status_line(&info.status);
                if !line.is_empty() {
                    ui.label(RichText::new(line).size(13.0).color(palette.ink2));
                }
                if info.caps.is_empty() {
                    hint(
                        ui,
                        "Cihazda donanım paylaşımı kapalı. Telefonda Mause → Donanım kartından açın.",
                    );
                    continue;
                }
                for cap in &info.caps {
                    match cap.kind.as_str() {
                        "camera" => {
                            let same = |c: &CameraView| c.id == link.id && c.camera == cap.id;
                            let open = self.camera.as_ref().is_some_and(|c| same(c) && c.window);
                            let linked = self
                                .camera
                                .as_ref()
                                .is_some_and(|c| same(c) && c.feeder.is_some());
                            setting_row(
                                ui,
                                &format!("Kamera: {}", cap.label),
                                Some("Canlı görüntü bu bilgisayarda ayrı bir pencerede açılır."),
                                120.0,
                                |ui| {
                                    let text = if open { "Kapat" } else { "Aç" };
                                    if design::key(ui, text, KeyKind::Plain, true, 0.0).clicked() {
                                        action = Some(if open {
                                            LayoutAction::CloseCamera
                                        } else {
                                            LayoutAction::Camera(link.id.clone(), cap.clone())
                                        });
                                    }
                                },
                            );
                            divider(ui);
                            if virtual_ready {
                                setting_row(
                                    ui,
                                    &format!("PC kamerası olarak bağla: {}", cap.label),
                                    Some(
                                        "Tarayıcı, Zoom, OBS gibi uygulamalarda \"Mause Telefon Kamerasi\" adıyla kamera olarak görünür.",
                                    ),
                                    150.0,
                                    |ui| {
                                        let text = if linked {
                                            "Bağlantıyı kes"
                                        } else {
                                            "Bağla"
                                        };
                                        if design::key(ui, text, KeyKind::Plain, true, 0.0)
                                            .clicked()
                                        {
                                            action = Some(if linked {
                                                LayoutAction::VirtualOff
                                            } else {
                                                LayoutAction::VirtualOn(
                                                    link.id.clone(),
                                                    cap.clone(),
                                                )
                                            });
                                        }
                                    },
                                );
                            } else {
                                setting_row(
                                    ui,
                                    "PC kamerası olarak bağla",
                                    Some(
                                        "Bir kerelik kurulum gerekir (yönetici onayı); sonrasında sanal kamera her açılışta hazırdır.",
                                    ),
                                    190.0,
                                    |ui| {
                                        if design::key(
                                            ui,
                                            "Sanal kamerayı kur",
                                            KeyKind::Primary,
                                            true,
                                            0.0,
                                        )
                                        .clicked()
                                        {
                                            action = Some(LayoutAction::SetupVirtual);
                                        }
                                    },
                                );
                                if let Some(note) = vcam_note.clone() {
                                    hint(ui, &note);
                                }
                            }
                        }
                        "torch" => {
                            let on = self.torch_on.contains(&link.id);
                            setting_row(
                                ui,
                                "El feneri",
                                Some("Telefonun el fenerini açar ya da kapatır."),
                                120.0,
                                |ui| {
                                    let text = if on { "Kapat" } else { "Aç" };
                                    if design::key(ui, text, KeyKind::Plain, true, 0.0).clicked() {
                                        action = Some(LayoutAction::Torch(link.id.clone()));
                                    }
                                },
                            );
                        }
                        "vibrate" => {
                            setting_row(
                                ui,
                                "Titreşim",
                                Some("Telefonu titreterek yerini bulmanızı sağlar."),
                                120.0,
                                |ui| {
                                    if design::key(ui, "Titret", KeyKind::Plain, true, 0.0)
                                        .clicked()
                                    {
                                        action = Some(LayoutAction::Vibrate(link.id.clone()));
                                    }
                                },
                            );
                        }
                        _ => {}
                    }
                }
            }
        });
        if let Some(action) = action {
            self.hardware_action(action);
        }
    }

    fn send_device(&self, id: &str, op: u8, data: Vec<u8>) {
        if let Some(sh) = &self.shared {
            sh.device_cmds
                .lock()
                .unwrap()
                .push((id.to_owned(), op, data));
        }
    }

    fn ensure_camera(&mut self, id: &str, cap: &hardware::Cap) -> bool {
        if self
            .camera
            .as_ref()
            .is_some_and(|camera| camera.id == id && camera.camera == cap.id)
        {
            return true;
        }
        self.close_camera();
        let Some(sh) = self.shared.clone() else {
            return false;
        };
        let ip = sh
            .links
            .lock()
            .unwrap()
            .iter()
            .find(|link| link.id == id)
            .map(|link| link.ip.clone());
        let secret = ip
            .as_ref()
            .and_then(|ip| sh.peer_secrets.lock().unwrap().get(ip).cloned());
        let (Some(ip), Some(secret)) = (ip, secret) else {
            sh.log("Kamera açılamadı: cihaz bağlı değil");
            return false;
        };
        let view = Arc::new(Mutex::new(screen::View::default()));
        let Some(handle) =
            screen::start_receiver_port(hardware::CAMERA_PORT, ip, secret, view.clone())
        else {
            sh.log("Kamera açılamadı: 24807 portu kullanımda");
            return false;
        };
        self.send_device(id, hardware::CAMERA_START, cap.id.clone().into_bytes());
        self.camera = Some(CameraView {
            id: id.to_owned(),
            camera: cap.id.clone(),
            title: cap.label.clone(),
            view,
            handle,
            texture: None,
            turns: Arc::new(AtomicU16::new(cap.rotation / 90)),
            window: false,
            feeder: None,
        });
        true
    }

    fn close_camera(&mut self) {
        if let Some(camera) = self.camera.take() {
            self.send_device(&camera.id, hardware::CAMERA_STOP, Vec::new());
            drop(camera.feeder);
            drop(camera.handle);
        }
    }

    fn camera_window(&mut self, ctx: &egui::Context) {
        let Some(camera) = &mut self.camera else {
            return;
        };
        if !camera.window {
            return;
        }
        let builder = egui::ViewportBuilder::default()
            .with_title(format!("Telefon kamerası · {}", camera.title))
            .with_inner_size([640.0, 520.0])
            .with_min_inner_size([320.0, 260.0]);
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("mause_camera"),
            builder,
            |ui, _class| {
                close = ui.ctx().input(|i| i.viewport().close_requested());
                ui.ctx().request_repaint_after(Duration::from_millis(33));
                let palette = Palette::of(ui);
                let turns = camera.turns.load(Ordering::Relaxed);
                let (frame, note) = {
                    let view = camera.view.lock().unwrap();
                    (
                        view.frame
                            .as_ref()
                            .map(|f| (f.w, f.h, f.seq, f.rgb.clone())),
                        view.note.clone(),
                    )
                };
                if let Some((w, h, seq, rgb)) = frame {
                    let stale = camera
                        .texture
                        .as_ref()
                        .is_none_or(|(_, last, shown)| *last != seq || *shown != turns);
                    if stale {
                        let (tw, th, turned) = hardware::rotate_rgb(w, h, &rgb, turns);
                        let image = egui::ColorImage::from_rgb([tw, th], &turned);
                        camera.texture = Some((
                            ui.ctx().load_texture(
                                "telefon-kamerasi",
                                image,
                                egui::TextureOptions::LINEAR,
                            ),
                            seq,
                            turns,
                        ));
                    }
                }
                egui::Panel::bottom("camera_bar")
                    .frame(
                        egui::Frame::new()
                            .fill(palette.surface)
                            .inner_margin(Margin::symmetric(12, 8)),
                    )
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if design::key(ui, "Döndür", KeyKind::Plain, true, 0.0).clicked() {
                                camera.turns.store((turns + 1) % 4, Ordering::Relaxed);
                            }
                            if design::key(ui, "Kapat", KeyKind::Plain, true, 0.0).clicked() {
                                close = true;
                            }
                        });
                    });
                let rect = ui.available_rect_before_wrap();
                ui.painter().rect_filled(rect, 0, Color32::BLACK);
                match &camera.texture {
                    Some((texture, ..)) => {
                        let size = texture.size_vec2();
                        let scale = (rect.width() / size.x).min(rect.height() / size.y);
                        let fit = egui::Rect::from_center_size(rect.center(), size * scale);
                        ui.painter().image(
                            texture.id(),
                            fit,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    None => {
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            note.unwrap_or_else(|| "Kamera bekleniyor…".to_owned()),
                            FontId::new(BODY, egui::FontFamily::Name("body".into())),
                            palette.ink2,
                        );
                    }
                }
            },
        );
        if close {
            self.hardware_action(LayoutAction::CloseCamera);
        }
    }

    fn device_strip(
        &self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        palette: Palette,
        rect: egui::Rect,
        stand: f32,
        link: &core::Link,
    ) -> Option<LayoutAction> {
        let sh = self.shared.as_ref()?;
        let serves = |feat| sh.serves(feat, &link.id);
        let device = self.device_bits(&link.id);
        let pitch = 26.0;
        let mut chosen = None;
        for (row, dir) in [Dir::Out, Dir::In].into_iter().enumerate() {
            let y = rect.bottom() + stand + 18.0 + row as f32 * pitch;
            let count = if dir == Dir::Out { 6.0 } else { 7.0 };
            let left = rect.center().x - (count - 1.0) * pitch / 2.0;
            let arrow = if dir == Dir::Out { "→" } else { "←" };
            painter.text(
                egui::pos2(left - 20.0, y),
                Align2::CENTER_CENTER,
                arrow,
                FontId::proportional(12.0),
                palette.muted,
            );
            let mut slot = 0.0;
            let mut next = || {
                let at = egui::pos2(left + slot * pitch, y);
                slot += 1.0;
                at
            };
            for kind in Flow::KINDS {
                let flow = Flow { dir, kind };
                let v = self.flow_view(sh, link, flow);
                let id = ("mini", &link.key(), dir == Dir::Out, kind as u8);
                let look = (v.look == Look::On, v.look == Look::PeerOff, v.live);
                if mini_key(
                    ui,
                    painter,
                    palette,
                    next(),
                    id,
                    flow_icon(kind),
                    look,
                    v.enabled,
                )
                .on_hover_text(v.tip)
                .clicked()
                {
                    chosen = Some(LayoutAction::Flow(link.id.clone(), flow, v.want));
                }
            }
            if dir == Dir::Out {
                let on = device & flows::SCREEN_OUT != 0;
                let tip = format!(
                    "{} bu bilgisayarın ekranını izleyebilsin: {}",
                    link.name,
                    if link.screen_out {
                        "şu an izliyor"
                    } else if on {
                        "açık"
                    } else {
                        "kapalı"
                    }
                );
                let id = ("mini", &link.key(), true, 9u8);
                if mini_key(
                    ui,
                    painter,
                    palette,
                    next(),
                    id,
                    Icon::Screens,
                    (on, false, link.screen_out),
                    true,
                )
                .on_hover_text(tip)
                .clicked()
                {
                    chosen = Some(LayoutAction::ScreenOut(link.id.clone(), !on));
                }
            } else {
                let watching = self.watching() && serves(core::Feat::Screen);
                let tip = format!(
                    "{} ekranını ayrı pencerede izle: {}",
                    link.name,
                    if watching { "açık" } else { "kapalı" }
                );
                let id = ("mini", &link.key(), false, 9u8);
                let live = watching && self.screen_tex.is_some();
                if mini_key(
                    ui,
                    painter,
                    palette,
                    next(),
                    id,
                    Icon::Screens,
                    (watching, false, live),
                    true,
                )
                .on_hover_text(tip)
                .clicked()
                {
                    chosen = Some(LayoutAction::Watch(link.id.clone(), !watching));
                }
                let usb = !link.id.is_empty() && sh.target(core::Feat::Usb) == link.id;
                let tip = if usb {
                    format!(
                        "USB sayfası {} ile çalışıyor. Tıklayınca seçim kalkar.",
                        link.name
                    )
                } else {
                    format!("USB sayfası {} ile çalışsın", link.name)
                };
                let id = ("mini", &link.key(), false, 10u8);
                if mini_key(
                    ui,
                    painter,
                    palette,
                    next(),
                    id,
                    Icon::Usb,
                    (usb, false, false),
                    !link.id.is_empty(),
                )
                .on_hover_text(tip)
                .clicked()
                {
                    chosen = Some(LayoutAction::Usb(link.id.clone()));
                }
            }
        }
        chosen
    }

    fn flow_view(&self, sh: &Shared, link: &core::Link, flow: Flow) -> FlowView {
        let serves = |feat| sh.serves(feat, &link.id);
        let mut mine = self.device_bits(&link.id) & !(Pref::RelayIn.bit() | Pref::MicIn.bit());
        for (feat, pref) in [
            (core::Feat::Audio, Pref::RelayIn),
            (core::Feat::Mic, Pref::MicIn),
        ] {
            if serves(feat) {
                mine |= self.device_bits(&link.id) & pref.bit();
            }
        }
        let peer = link.prefs;
        let recent = |k: usize| {
            sh.relay_last[k]
                .lock()
                .unwrap()
                .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
        };
        let moving = |incoming: bool| {
            serves(core::Feat::Files)
                && sh
                    .transfers
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|t| t.status == Status::Active && t.incoming == incoming)
        };
        let look = flow.look(mine, peer);
        let want = flow.wanted(mine, peer);
        let live = look == Look::On
            && match (flow.dir, flow.kind) {
                (Dir::Out, FlowKind::Control) => link.control == Control::Remote,
                (Dir::In, FlowKind::Control) => link.control == Control::Controlled,
                (Dir::Out, FlowKind::Audio) => link.sending[0],
                (Dir::Out, FlowKind::Mic) => link.sending[1],
                (Dir::In, FlowKind::Audio) => recent(0),
                (Dir::In, FlowKind::Mic) => recent(1),
                (dir, FlowKind::Files) => moving(dir == Dir::In),
                (_, FlowKind::Clip) => false,
            };
        let state = match look {
            Look::On if live => "şu an aktarılıyor",
            Look::On => "açık",
            Look::PeerOff => "karşıda kapalı",
            Look::Off => "kapalı",
        };
        FlowView {
            look,
            want,
            enabled: peer.is_some() || !flow.needs_peer(want),
            live,
            tip: format!("{}: {state}", flow.sentence().replace("karşı", &link.name)),
        }
    }

    fn popup_link(&self) -> Option<core::Link> {
        let sh = self.shared.as_ref()?;
        let links = sh.links.lock().unwrap().clone();
        let chosen = self
            .popup_device
            .as_ref()
            .and_then(|id| links.iter().find(|l| l.key() == *id));
        let primary = sh.primary_link();
        chosen
            .cloned()
            .or(primary)
            .or_else(|| links.first().cloned())
    }

    fn popup_keys(&self, ui: &mut egui::Ui, dir: Dir, link: &core::Link) -> Vec<PopupAction> {
        let Some(sh) = self.shared.as_ref() else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        for kind in Flow::KINDS {
            let flow = Flow { dir, kind };
            let v = self.flow_view(sh, link, flow);
            let on = v.look == Look::On;
            if design::keycap_with(
                ui,
                flow_icon(kind),
                flow.short(),
                on,
                v.look == Look::PeerOff,
                v.live,
                v.enabled,
            )
            .on_hover_text(v.tip)
            .clicked()
            {
                actions.push(PopupAction::Flow(link.id.clone(), flow, v.want));
            }
        }
        if dir == Dir::In {
            let watching = self.watching() && sh.serves(core::Feat::Screen, &link.id);
            let live = watching && self.screen_tex.is_some();
            if design::keycap(ui, Icon::Screens, "Ekran", watching, live, true)
                .on_hover_text(format!("{} ekranını ayrı pencerede izle", link.name))
                .clicked()
            {
                actions.push(PopupAction::WatchOn(link.id.clone(), !watching));
            }
        }
        actions
    }

    fn device_flow(&mut self, id: &str, flow: Flow, on: bool) {
        let Some(sh) = self.shared.clone() else {
            return;
        };
        let peer = sh
            .links
            .lock()
            .unwrap()
            .iter()
            .find(|l| l.id == id)
            .and_then(|l| l.prefs);
        let feat = match flow.kind {
            FlowKind::Audio if flow.dir == Dir::In => Some(core::Feat::Audio),
            FlowKind::Mic if flow.dir == Dir::In => Some(core::Feat::Mic),
            FlowKind::Files => Some(core::Feat::Files),
            _ => None,
        };
        if let Some(feat) = feat {
            if on || feat == core::Feat::Files {
                sh.set_target(feat, id);
            } else if !sh.serves(feat, id) {
                return;
            }
        }
        self.toggle_flow_for(id, flow, on, peer);
    }

    fn link_note(&self, ui: &mut egui::Ui, state: &State) {
        if !matches!(state, State::Connected { .. }) {
            return;
        }
        let palette = Palette::of(ui);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
            design::icon(ui.painter(), rect.center(), Icon::Lock, palette.ok, 16.0);
            ui.label(
                RichText::new("Bağlantılar uçtan uca şifreli. Kart altındaki ikonlar o cihazla paylaşılanları gösterir.")
                    .size(13.0)
                    .color(palette.muted),
            );
        });
    }

    fn screen_key(&self, ui: &mut egui::Ui, connected: bool) -> Option<bool> {
        let watching = self.watching();
        let live = watching && self.screen_tex.is_some();
        let tip = if connected {
            "Karşının ekranını ayrı pencerede izle. Karşıda Paylaşım → Bu bilgisayardan karşıya → Ekran açık olmalı."
        } else {
            "Karşının ekranını izle. Bağlanınca kullanılabilir."
        };
        let enabled = connected || watching;
        let r = design::keycap(ui, Icon::Screens, "Ekran", watching, live, enabled);
        let r = if enabled {
            r.on_hover_text(tip)
        } else {
            r.on_disabled_hover_text(tip)
        };
        r.clicked().then_some(!watching)
    }

    fn set_watching(&mut self, on: bool) {
        if let Some(sh) = &self.shared {
            sh.screen_want.store(on, Ordering::Relaxed);
        }
        if !on {
            self.screen_tex = None;
        }
    }

    fn top_controls(&mut self, ui: &mut egui::Ui, right_reserved: f32, notes: usize) {
        let palette = Palette::of(ui);
        let bar = ui.max_rect();
        let painter = ui.painter().clone();
        let brand_right = bar.left() + 220.0;
        let right = bar.right() - right_reserved - 24.0;
        let mut name_left = right + 12.0;
        if right - brand_right > 150.0 {
            let mut name = egui::text::LayoutJob::simple_singleline(
                self.cfg.name.clone(),
                design::semibold(14.0),
                palette.ink,
            );
            name.wrap.max_width = right - brand_right - 24.0;
            name.wrap.max_rows = 1;
            let galley = painter.layout_job(name);
            name_left = right - galley.size().x.max(74.0);
            painter.galley(
                egui::pos2(right - galley.size().x, bar.center().y - 18.0),
                galley,
                palette.ink,
            );
            painter.text(
                egui::pos2(right, bar.center().y + 10.0),
                Align2::RIGHT_CENTER,
                "Bu bilgisayar",
                FontId::proportional(11.5),
                palette.muted,
            );
        }
        self.bell_button(ui, egui::pos2(name_left - 32.0, bar.center().y), notes);
    }

    fn bell_button(&mut self, ui: &mut egui::Ui, center: egui::Pos2, notes: usize) {
        let palette = Palette::of(ui);
        let rect = egui::Rect::from_center_size(center, Vec2::splat(36.0));
        self.bell_rect = rect;
        let response = ui
            .interact(rect, ui.scope_id().with("notes_bell"), Sense::click())
            .on_hover_text("Bildirimler")
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Bildirimler"));
        let painter = ui.painter();
        if self.notes_open {
            painter.rect_filled(rect, design::RADIUS_BUTTON, palette.inset);
        } else if response.hovered() {
            painter.rect_filled(rect, design::RADIUS_BUTTON, palette.hover);
        }
        let ink = if notes > 0 { palette.ink } else { palette.ink2 };
        design::icon(painter, center, Icon::Bell, ink, 20.0);
        if notes > 0 {
            let badge = center + Vec2::new(9.0, -9.0);
            red_dot(painter, badge, 8.0, palette);
            painter.text(
                badge,
                Align2::CENTER_CENTER,
                notes.min(9).to_string(),
                design::semibold(10.0),
                Color32::WHITE,
            );
        }
        if response.clicked() {
            self.notes_open = !self.notes_open;
        }
    }

    fn watching(&self) -> bool {
        self.shared
            .as_ref()
            .is_some_and(|sh| sh.screen_want.load(Ordering::Relaxed))
    }

    fn screen_window(&mut self, ctx: &egui::Context) {
        let Some(sh) = self
            .shared
            .clone()
            .filter(|sh| sh.screen_want.load(Ordering::Relaxed))
        else {
            self.screen_tex = None;
            self.release_remote_keys();
            return;
        };
        let title = match &*sh.state.lock().unwrap() {
            State::Connected { peer, .. } if !peer.is_empty() => format!("{peer} ekranı"),
            _ => "Karşı ekran".to_owned(),
        };
        let builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([1100.0, 740.0])
            .with_min_inner_size([520.0, 300.0]);
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("mause_screen"),
            builder,
            |ui, _class| {
                close = ui.ctx().input(|i| i.viewport().close_requested());
                ui.ctx().request_repaint_after(Duration::from_millis(30));
                let palette = Palette::of(ui);
                let (may_control, monitors) = sh.screen_peer.lock().unwrap().clone();
                egui::Panel::top("screen_bar")
                    .frame(
                        egui::Frame::new()
                            .fill(palette.surface)
                            .inner_margin(Margin::symmetric(12, 8)),
                    )
                    .show(ui, |ui| {
                        self.screen_toolbar(ui, &sh, may_control, &monitors)
                    });
                let note = {
                    let mut v = sh.screen.lock().unwrap();
                    if let Some(f) = v.frame.as_mut() {
                        if self
                            .screen_tex
                            .as_ref()
                            .is_none_or(|(_, seq)| *seq != f.seq)
                        {
                            let img = egui::ColorImage::from_rgb([f.w, f.h], &f.rgb);
                            match &mut self.screen_tex {
                                Some((tex, seq)) => {
                                    tex.set(img, egui::TextureOptions::LINEAR);
                                    *seq = f.seq;
                                }
                                None => {
                                    let tex = ui.ctx().load_texture(
                                        "mause_screen",
                                        img,
                                        egui::TextureOptions::LINEAR,
                                    );
                                    self.screen_tex = Some((tex, f.seq));
                                }
                            }
                        }
                    } else {
                        self.screen_tex = None;
                    }
                    v.note.clone()
                };
                let rect = ui.available_rect_before_wrap();
                ui.painter().rect_filled(rect, 0, Color32::BLACK);
                match &self.screen_tex {
                    Some((tex, _)) => {
                        let size = tex.size_vec2();
                        let scale = (rect.width() / size.x).min(rect.height() / size.y);
                        let fit = egui::Rect::from_center_size(rect.center(), size * scale);
                        ui.painter().image(
                            tex.id(),
                            fit,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        if self.screen_ctrl && may_control {
                            self.forward_input(ui.ctx(), fit);
                        } else {
                            self.release_remote_keys();
                        }
                    }
                    None => {
                        let text = note.unwrap_or_else(|| "Görüntü bekleniyor…".to_owned());
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            text,
                            FontId::new(BODY, egui::FontFamily::Name("body".into())),
                            palette.ink2,
                        );
                    }
                }
            },
        );
        if close {
            sh.screen_want.store(false, Ordering::Relaxed);
            self.screen_tex = None;
            self.release_remote_keys();
        }
    }

    fn screen_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        sh: &Arc<Shared>,
        may_control: bool,
        monitors: &[String],
    ) {
        let mut o = self.cfg.screen;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if o.extend.is_none() && !monitors.is_empty() {
                let current = monitors.get(o.monitor as usize).cloned().unwrap_or_else(|| monitors[0].clone());
                egui::ComboBox::from_id_salt("screen_monitor").selected_text(current).show_ui(ui, |ui| {
                    for (i, name) in monitors.iter().enumerate() {
                        ui.selectable_value(&mut o.monitor, i as u8, name);
                    }
                });
            }
            stream_controls(ui, "bar", &mut o);
            let fullscreen = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
            if ui.button(if fullscreen { "Pencereye dön" } else { "Tam ekran" }).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            ui.separator();
            let tip = if may_control {
                "Açıkken bu penceredeki mouse ve klavye karşı PC'yi yönetir."
            } else {
                "Karşı PC izin vermiyor: karşıda Ayarlar → Ekran paylaşımı → \"İzlerken bu PC'yi kontrol edebilsin\" açılmalı."
            };
            let mut ctrl = self.screen_ctrl && may_control;
            let r = ui.add_enabled(may_control, egui::Checkbox::new(&mut ctrl, "Kontrol et"));
            let r = if may_control { r.on_hover_text(tip) } else { r.on_disabled_hover_text(tip) };
            if r.changed() {
                self.screen_ctrl = ctrl;
            }
        });
        if o != self.cfg.screen {
            self.cfg.screen = o;
            *sh.screen_opts.lock().unwrap() = o;
            self.cfg.save();
        }
    }

    fn forward_input(&mut self, ctx: &egui::Context, fit: egui::Rect) {
        use crate::platform::ev;
        let mut evs: Vec<crate::platform::Ev> = Vec::new();
        let at = |p: egui::Pos2| -> [crate::platform::Ev; 2] {
            let n = ((p - fit.min) / fit.size()).clamp(Vec2::ZERO, Vec2::splat(1.0)) * 65535.0;
            [
                (screen::EV_ABS, 0, n.x as i32),
                (screen::EV_ABS, 1, n.y as i32),
            ]
        };
        let (events, mods, focused) = ctx.input(|i| (i.events.clone(), i.modifiers, i.focused));
        if !focused {
            self.release_remote_keys();
            return;
        }
        let new_mods = [mods.ctrl, mods.shift, mods.alt, mods.mac_cmd];
        evs.extend(crate::keymap::modifier_changes(self.screen_mods, new_mods));
        self.screen_mods = new_mods;
        for e in events {
            match e {
                egui::Event::PointerMoved(p) if fit.contains(p) => evs.extend(at(p)),
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } if fit.contains(pos) || !pressed => {
                    let code = match button {
                        egui::PointerButton::Primary => ev::BTN_LEFT,
                        egui::PointerButton::Secondary => ev::BTN_RIGHT,
                        egui::PointerButton::Middle => ev::BTN_MIDDLE,
                        egui::PointerButton::Extra1 => ev::BTN_SIDE,
                        egui::PointerButton::Extra2 => ev::BTN_EXTRA,
                    };
                    evs.extend(at(pos));
                    evs.push((ev::EV_KEY, code, pressed as i32));
                }
                egui::Event::MouseWheel { unit, delta, .. } => {
                    let notches = match unit {
                        egui::MouseWheelUnit::Line => delta,
                        egui::MouseWheelUnit::Point => delta / 50.0,
                        egui::MouseWheelUnit::Page => delta * 3.0,
                    };
                    for (hi, lo, v) in [
                        (ev::REL_WHEEL_HI_RES, ev::REL_WHEEL, notches.y),
                        (ev::REL_HWHEEL_HI_RES, ev::REL_HWHEEL, -notches.x),
                    ] {
                        if v != 0.0 {
                            evs.push((ev::EV_REL, hi, (v * 120.0) as i32));
                            if v.abs() >= 1.0 {
                                evs.push((ev::EV_REL, lo, v as i32));
                            }
                        }
                    }
                }
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat: false,
                    ..
                } => {
                    if let Some(code) = crate::keymap::evdev(physical_key.unwrap_or(key)) {
                        evs.push((ev::EV_KEY, code, pressed as i32));
                    }
                }
                _ => {}
            }
        }
        if !evs.is_empty() {
            crate::keymap::track_down(&mut self.screen_down, &evs);
            crate::core::screen_input(evs);
        }
    }

    fn release_remote_keys(&mut self) {
        let mut evs = crate::keymap::modifier_changes(self.screen_mods, [false; 4]);
        self.screen_mods = [false; 4];
        evs.extend(crate::keymap::release_down(&mut self.screen_down));
        if !evs.is_empty() {
            crate::core::screen_input(evs);
        }
    }

    fn popup_window(&mut self, ctx: &egui::Context) {
        let Some(popup) = &self.popup else { return };
        let size = Vec2::new(Popup::SIZE.x, popup.height);
        let mut builder = egui::ViewportBuilder::default()
            .with_title("Mause")
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_active(true)
            .with_transparent(true)
            .with_inner_size(size);
        if let Some((x, y)) = popup.anchor {
            let ppp = ctx.pixels_per_point();
            let (x, y) = (x / ppp, y / ppp);
            let left = (x - size.x / 2.0).max(8.0);
            let top = if y > size.y + 24.0 {
                y - size.y - 12.0
            } else {
                y + 36.0
            };
            builder = builder.with_position([left, top]);
        }
        let state = self
            .shared
            .as_ref()
            .map(|s| s.state.lock().unwrap().clone())
            .unwrap_or(State::Stopped);
        let running = self.sharing_on();
        let stopping = self.access_stopping();
        let device = self.popup_link();
        let links: Vec<core::Link> = self
            .shared
            .as_ref()
            .map(|sh| sh.links.lock().unwrap().clone())
            .unwrap_or_default();
        let mut actions: Vec<PopupAction> = Vec::new();
        let mut measured = None;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("mause_tray_popup"),
            builder,
            |ui, _class| {
                let (focused, escape, close) = ui.ctx().input(|i| {
                    (
                        i.viewport().focused,
                        i.key_pressed(egui::Key::Escape),
                        i.viewport().close_requested(),
                    )
                });
                if let Some(p) = &mut self.popup {
                    match focused {
                        Some(true) => p.focused = true,
                        Some(false)
                            if p.focused && p.opened.elapsed() > Duration::from_millis(500) =>
                        {
                            actions.push(PopupAction::Close)
                        }
                        Some(false) => p.focused = false,
                        None => {}
                    }
                }
                if escape || close {
                    actions.push(PopupAction::Close);
                }
                let palette = Palette::of(ui);
                egui::CentralPanel::default()
                    .frame(egui::Frame::new())
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .fill(palette.surface)
                            .stroke(Stroke::new(1.0, palette.rule_strong))
                            .corner_radius(RADIUS_PANEL)
                            .inner_margin(Margin::symmetric(18, 16))
                            .show(ui, |ui| {
                                ui.set_min_size(ui.available_size());
                                let content = ui.scope(|ui| {
                                    ui.horizontal(|ui| {
                                        let (rect, _) = ui.allocate_exact_size(
                                            Vec2::new(96.0, 24.0),
                                            Sense::hover(),
                                        );
                                        let logo = self.logo.as_ref().map(|t| t.id());
                                        design::brand(
                                            ui.painter(),
                                            rect.left_center(),
                                            logo,
                                            palette,
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                let (text, color, icon) =
                                                    self.access_status(&state, running, palette);
                                                let text = match (&state, &device) {
                                                    (State::Connected { .. }, Some(_))
                                                        if links.len() > 1 =>
                                                    {
                                                        format!("{} cihaz bağlı", links.len())
                                                    }
                                                    (State::Connected { .. }, Some(l))
                                                        if !l.name.is_empty() =>
                                                    {
                                                        l.name.clone()
                                                    }
                                                    (State::Connected { peer, .. }, _)
                                                        if !peer.is_empty() =>
                                                    {
                                                        peer.clone()
                                                    }
                                                    _ => text,
                                                };
                                                egui::Frame::new()
                                                    .fill(palette.inset)
                                                    .corner_radius(255)
                                                    .inner_margin(Margin {
                                                        left: 8,
                                                        right: 10,
                                                        top: 3,
                                                        bottom: 3,
                                                    })
                                                    .show(ui, |ui| {
                                                        ui.with_layout(
                                                            Layout::left_to_right(Align::Center),
                                                            |ui| {
                                                                ui.spacing_mut().item_spacing.x =
                                                                    6.0;
                                                                let (rect, _) = ui
                                                                    .allocate_exact_size(
                                                                        Vec2::splat(14.0),
                                                                        Sense::hover(),
                                                                    );
                                                                design::icon(
                                                                    ui.painter(),
                                                                    rect.center(),
                                                                    icon,
                                                                    color,
                                                                    14.0,
                                                                );
                                                                ui.add(
                                                                    egui::Label::new(
                                                                        RichText::new(text)
                                                                            .size(design::META)
                                                                            .color(color),
                                                                    )
                                                                    .truncate(),
                                                                );
                                                            },
                                                        );
                                                    });
                                            },
                                        );
                                    });
                                    ui.add_space(10.0);
                                    design::rule(ui);
                                    ui.add_space(10.0);
                                    if links.len() > 1 {
                                        let names: Vec<&str> =
                                            links.iter().map(|l| l.name.as_str()).collect();
                                        let at = device
                                            .as_ref()
                                            .and_then(|d| links.iter().position(|l| l.key() == d.key()))
                                            .unwrap_or(0);
                                        if let Some(i) =
                                            design::segmented(ui, "popup_device", &names, at)
                                        {
                                            actions.push(PopupAction::Device(links[i].key()));
                                        }
                                        ui.add_space(10.0);
                                    }
                                    for dir in [Dir::Out, Dir::In] {
                                        ui.label(
                                            RichText::new(dir_title(dir))
                                                .size(12.5)
                                                .color(palette.muted),
                                        );
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 12.0;
                                            if let Some(link) = &device {
                                                actions.extend(self.popup_keys(ui, dir, link));
                                                return;
                                            }
                                            if let Some((flow, on)) =
                                                self.flow_keys(ui, dir, &state)
                                            {
                                                actions.push(PopupAction::Toggle(flow, on));
                                            }
                                            if dir == Dir::In {
                                                let connected =
                                                    matches!(state, State::Connected { .. });
                                                if let Some(on) = self.screen_key(ui, connected) {
                                                    actions.push(PopupAction::Watch(on));
                                                }
                                            }
                                        });
                                        ui.add_space(8.0);
                                    }

                                    design::rule(ui);
                                    ui.add_space(10.0);
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        let third = ((ui.available_width() - 24.0) / 4.0).floor();
                                        let can_start = self.can_start_access();
                                        let (label, kind) = if stopping {
                                            ("Durduruluyor…", KeyKind::Plain)
                                        } else if running {
                                            ("Durdur", KeyKind::Plain)
                                        } else {
                                            ("Başlat", KeyKind::Primary)
                                        };
                                        let usable = !stopping && (running || can_start);
                                        if design::key(ui, label, kind, usable, third).clicked() {
                                            actions.push(PopupAction::StartStop);
                                        }
                                        if design::key(ui, "Gizle", KeyKind::Plain, true, third)
                                            .on_hover_text("Pencereyi gizle; arka planda çalışmaya devam eder, durum simgeden izlenir")
                                            .clicked()
                                        {
                                            actions.push(PopupAction::Hide);
                                        }
                                        if design::key(ui, "Pencere", KeyKind::Plain, true, third)
                                            .clicked()
                                        {
                                            actions.push(PopupAction::Open);
                                        }
                                        if design::key(ui, "Çık", KeyKind::Plain, true, third)
                                            .clicked()
                                        {
                                            actions.push(PopupAction::Quit);
                                        }
                                    });
                                });
                                measured = Some(content.response.rect.height());
                            });
                    });
            },
        );
        if let (Some(h), Some(p)) = (measured, &mut self.popup) {
            let fit = (h + 34.0).ceil().max(200.0);
            if (fit - p.height).abs() > 1.0 {
                p.height = fit;
                ctx.request_repaint();
            }
        }
        for action in actions {
            match action {
                PopupAction::Toggle(flow, on) => self.toggle_flow(flow, on),
                PopupAction::Watch(on) => self.set_watching(on),
                PopupAction::Flow(id, flow, on) => self.device_flow(&id, flow, on),
                PopupAction::WatchOn(id, on) => {
                    if let (true, Some(sh)) = (on, &self.shared) {
                        sh.set_target(core::Feat::Screen, &id);
                    }
                    self.set_watching(on);
                }
                PopupAction::Device(key) => self.popup_device = Some(key),
                PopupAction::StartStop => self.toggle_access(),
                PopupAction::Hide => self.hide(ctx),
                PopupAction::Open => self.show(ctx),
                PopupAction::Quit => {
                    self.popup = None;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                PopupAction::Close => self.popup = None,
            }
        }
    }

    fn show(&mut self, ctx: &egui::Context) {
        self.popup = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn hide(&mut self, ctx: &egui::Context) {
        self.popup = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    fn flow_keys(&self, ui: &mut egui::Ui, dir: Dir, state: &State) -> Option<(Flow, bool)> {
        let connected = matches!(state, State::Connected { .. });
        let mine = flows::bits(&self.cfg);
        let peer = self.peer_prefs();
        let mut toggle = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for kind in Flow::KINDS {
                let flow = Flow { dir, kind };
                let look = flow.look(mine, peer);
                let on = connected && look == Look::On;
                let peer_off = connected && look == Look::PeerOff;
                let want = flow.wanted(mine, peer);
                let enabled = connected && (peer.is_some() || !flow.needs_peer(want));
                let live = on && self.flow_live(flow, state);
                let icon = match kind {
                    FlowKind::Control => Icon::Keyboard,
                    FlowKind::Audio => Icon::Audio,
                    FlowKind::Mic => Icon::Mic,
                    FlowKind::Files => Icon::Files,
                    FlowKind::Clip => Icon::Clipboard,
                };
                let tip = if !connected {
                    format!("{}. Bağlanınca kullanılabilir.", flow.sentence())
                } else if live {
                    format!("{}. Şu an aktarılıyor.", flow.sentence())
                } else if peer_off {
                    format!(
                        "{}: bu PC'de açık, karşı PC'de kapalı. Karşı PC'de açılmalı.{}",
                        flow.sentence(),
                        if want {
                            ""
                        } else {
                            " Tıklarsan burada da kapanır."
                        }
                    )
                } else {
                    format!(
                        "{}: {}",
                        flow.sentence(),
                        if on { "açık" } else { "kapalı" }
                    )
                };
                let response =
                    design::keycap_with(ui, icon, flow.short(), on, peer_off, live, enabled);
                let response = if enabled {
                    response.on_hover_text(tip)
                } else {
                    response.on_disabled_hover_text(tip)
                };
                if response.clicked() {
                    toggle = Some((flow, want));
                }
            }
        });
        toggle
    }

    fn flow_live(&self, flow: Flow, state: &State) -> bool {
        let Some(sh) = self.shared.as_ref() else {
            return false;
        };
        let recent = |k: usize| {
            sh.relay_last[k]
                .lock()
                .unwrap()
                .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
        };
        let control = match state {
            State::Connected { control, .. } => Some(*control),
            _ => None,
        };
        match (flow.dir, flow.kind) {
            (Dir::Out, FlowKind::Control) => control == Some(Control::Remote),
            (Dir::In, FlowKind::Control) => control == Some(Control::Controlled),
            (Dir::Out, FlowKind::Audio) => sh.sending[0].load(Ordering::Relaxed),
            (Dir::Out, FlowKind::Mic) => sh.sending[1].load(Ordering::Relaxed),
            (Dir::In, FlowKind::Audio) => recent(0),
            (Dir::In, FlowKind::Mic) => recent(1),
            (dir, FlowKind::Files) => sh
                .transfers
                .lock()
                .unwrap()
                .iter()
                .any(|t| t.status == Status::Active && t.incoming == (dir == Dir::In)),
            (_, FlowKind::Clip) => false,
        }
    }

    fn pairing_notice(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self
            .shared
            .as_ref()
            .and_then(|sh| sh.pairing.lock().unwrap().clone())
        else {
            self.pair_done_at = None;
            return;
        };
        if p.done && !p.failed {
            let at = *self.pair_done_at.get_or_insert_with(Instant::now);
            if at.elapsed() > PAIRED_NOTICE {
                self.dismiss_pairing();
                return;
            }
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        } else {
            self.pair_done_at = None;
        }
        let palette = Palette::of(ui);
        let tone = if p.failed {
            palette.bad
        } else if p.done {
            palette.ok
        } else {
            palette.signal
        };
        let (icon, title, detail) = if p.failed {
            (
                Icon::Alert,
                format!("{} ile eşleşme tamamlanamadı", p.name),
                capitalize(&p.state),
            )
        } else if p.done {
            (
                Icon::Check,
                format!("{} ile eşleşildi", p.name),
                "Aynı ağda Mause açıkken iki bilgisayar kendiliğinden bağlanır.".to_owned(),
            )
        } else {
            (
                Icon::Link,
                format!("{} ile eşleşiliyor", p.name),
                capitalize(&p.state),
            )
        };
        let mut dismiss = false;
        egui::Frame::new()
            .fill(palette.inset)
            .stroke(Stroke::new(1.0, tone.gamma_multiply(0.55)))
            .corner_radius(14)
            .inner_margin(Margin::symmetric(18, 16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 20.0, tone.gamma_multiply(0.16));
                    design::icon(ui.painter(), rect.center(), icon, tone, 20.0);
                    ui.add_space(6.0);
                    let room = if p.done { 116.0 } else { 0.0 };
                    ui.allocate_ui_with_layout(
                        Vec2::new((ui.available_width() - room).max(120.0), 40.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(design::strong(&title, 15.0));
                            ui.horizontal(|ui| {
                                if !p.done {
                                    ui.add(egui::Spinner::new().size(12.0).color(palette.signal));
                                }
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&detail).size(12.5).color(palette.muted),
                                    )
                                    .wrap(),
                                );
                            });
                        },
                    );
                    if p.done {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let label = if p.failed { "Kapat" } else { "Tamam" };
                            dismiss = design::key(ui, label, KeyKind::Plain, true, 96.0).clicked();
                        });
                    }
                });
                if let (Some(code), false) = (&p.sas, p.done) {
                    ui.add_space(14.0);
                    code_boxes(ui, code);
                    ui.add_space(10.0);
                    hint(
                        ui,
                        "İki bilgisayarda da aynı kodu görüyorsanız karşı bilgisayarda Eşleş'e basın.",
                    );
                }
            });
        ui.add_space(12.0);
        if dismiss {
            self.dismiss_pairing();
        }
    }

    fn dismiss_pairing(&mut self) {
        self.pair_done_at = None;
        if let Some(sh) = &self.shared {
            *sh.pairing.lock().unwrap() = None;
        }
    }

    fn kvm_computers(&mut self, ui: &mut egui::Ui, state: &State, running: bool) {
        let palette = Palette::of(ui);
        let linked: Vec<(String, String)> = match state {
            State::Connected { .. } => self
                .shared
                .as_ref()
                .map(|sh| {
                    sh.links
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|l| (l.id.clone(), l.name.clone()))
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let list: Vec<(String, String, String, String)> = {
            let mut peers = self.peers.lock().unwrap();
            peers.retain(|_, peer| peer.seen.elapsed() < Duration::from_secs(7));
            let mut v: Vec<_> = peers
                .iter()
                .filter(|(_, peer)| peer.id != discover::self_id())
                .map(|(ip, peer)| {
                    (
                        ip.clone(),
                        peer.name.clone(),
                        peer.port.clone(),
                        peer.id.clone(),
                    )
                })
                .collect();
            v.sort();
            v
        };
        let mut send: Option<core::PairTarget> = None;
        let mut unpair: Option<String> = None;
        let mut hold: Option<String> = None;
        let mut resume: Option<String> = None;
        egui::Frame::new()
            .fill(palette.inset)
            .stroke(Stroke::new(1.0, palette.rule_strong))
            .corner_radius(14)
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                    design::icon(ui.painter(), rect.center(), Icon::Link, palette.ink2, 18.0);
                    let can_send = running && !self.pair_ip.trim().is_empty();
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.pair_ip)
                            .hint_text("IP adresiyle bağlan, ör. 192.168.1.20")
                            .frame(egui::Frame::NONE)
                            .desired_width(ui.available_width() - 110.0)
                            .min_size(Vec2::new(0.0, design::CONTROL))
                            .align(Align2::LEFT_CENTER),
                    );
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let tip = if running {
                            "Eşleşme isteği gönderir"
                        } else {
                            "KVM seçiliyken paylaşım otomatik başlar; izinleri kontrol edin"
                        };
                        if (design::key(ui, "Bağlan", KeyKind::Primary, can_send, 96.0)
                            .on_hover_text(tip)
                            .clicked()
                            || enter)
                            && can_send
                        {
                            let ip = self.pair_ip.trim().to_string();
                            send = Some(core::PairTarget {
                                ip: ip.clone(),
                                port: self.cfg.port.clone(),
                                name: ip,
                            });
                        }
                    });
                });
            });
        ui.add_space(12.0);
        if !running || list.is_empty() {
            ui.horizontal(|ui| {
                if running {
                    ui.add(egui::Spinner::new().size(14.0).color(palette.signal));
                    ui.label(
                        RichText::new("Ağdaki bilgisayarlar aranıyor")
                            .size(13.0)
                            .color(palette.ink2),
                    );
                    ui.label(
                        RichText::new("· diğer bilgisayarda da Mause açık olmalı")
                            .size(13.0)
                            .color(palette.muted),
                    );
                } else {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, palette.off);
                    ui.label(
                        RichText::new("KVM otomatik bağlantısı bekleniyor")
                            .size(13.0)
                            .color(palette.ink2),
                    );
                }
            });
            ui.add_space(12.0);
        }

        let gap = 12.0;
        let avail = ui.available_width();
        let cols = ((avail + gap) / (220.0 + gap)).floor().max(1.0) as usize;
        let width = (avail - gap * (cols as f32 - 1.0)) / cols as f32;
        let saved: Vec<(String, String)> = self
            .cfg
            .paired
            .iter()
            .filter(|(id, _, _)| !list.iter().any(|(_, _, _, found)| found == id))
            .map(|(id, name, _)| (id.clone(), name.clone()))
            .collect();
        let count = list.len() + saved.len();
        for row in 0..count.div_ceil(cols) {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for n in row * cols..(row * cols + cols).min(count) {
                    ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Min), |ui| {
                        ui.set_width(width);
                        if let Some((ip, name, port, id)) = list.get(n) {
                            let title = if name.is_empty() { ip.as_str() } else { name.as_str() };
                            let here = linked.iter().any(|(i, n)| i == id || n == title);
                            let paired = self.cfg.paired.iter().any(|(i, _, _)| i == id);
                            let blocked = self.cfg.blocked.iter().any(|(i, _)| i == id);
                            let held = self.cfg.held.iter().any(|i| i == id);
                            tile(ui, palette, here, |ui| {
                                ui.horizontal(|ui| {
                                    icon_badge(ui, palette, Icon::Screens, here);
                                    ui.vertical(|ui| {
                                        ui.spacing_mut().item_spacing.y = 0.0;
                                        ui.add(egui::Label::new(design::strong(title, 15.0)).truncate());
                                        ui.label(RichText::new(ip).size(12.0).color(palette.muted));
                                    });
                                });
                                ui.add_space(14.0);
                                let full = ui.available_width();
                                if here && paired {
                                    status_pill(ui, palette, "Bağlı", palette.signal);
                                    ui.add_space(10.0);
                                    ui.horizontal(|ui| {
                                        if design::key(ui, "Bağlantıyı kes", KeyKind::Plain, true, 0.0)
                                            .on_hover_text("Bağlantıyı kapatır; eşleşme kalır, Bağlan ile yeniden bağlanırsınız.")
                                            .clicked()
                                        {
                                            hold = Some(id.clone());
                                        }
                                        if design::key(ui, "Kaldır", KeyKind::Plain, true, 0.0)
                                            .on_hover_text("Eşleşme parolasını bu bilgisayardan sil")
                                            .clicked()
                                        {
                                            unpair = Some(id.clone());
                                        }
                                    });
                                } else if paired && held {
                                    status_pill(ui, palette, "Bağlantı kesildi", palette.muted);
                                    ui.add_space(10.0);
                                    ui.horizontal(|ui| {
                                        if design::key(ui, "Bağlan", KeyKind::Primary, running, 0.0)
                                            .on_hover_text("Eşleşmiş cihaza yeniden bağlanır.")
                                            .clicked()
                                        {
                                            resume = Some(id.clone());
                                        }
                                        if design::key(ui, "Kaldır", KeyKind::Plain, true, 0.0)
                                            .on_hover_text("Eşleşme parolasını bu bilgisayardan sil")
                                            .clicked()
                                        {
                                            unpair = Some(id.clone());
                                        }
                                    });
                                } else if here {
                                    status_pill(ui, palette, "Bağlı", palette.signal);
                                } else if blocked {
                                    status_pill(ui, palette, "Engelli", palette.bad);
                                } else if paired {
                                    ui.horizontal(|ui| {
                                        status_pill(ui, palette, "Eşleşmiş", palette.ink2)
                                            .on_hover_text("Eşleşmiş cihaza Mause kendiliğinden bağlanır.");
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if design::key(ui, "Kaldır", KeyKind::Plain, true, 0.0)
                                                .on_hover_text("Eşleşme parolasını bu bilgisayardan sil")
                                                .clicked()
                                            {
                                                unpair = Some(id.clone());
                                            }
                                        });
                                    });
                                } else if design::key(ui, "Bağlan", KeyKind::Primary, running, full)
                                    .on_hover_text("Eşleşme isteği gönderir; karşıda iki ekrandaki kod aynıysa Eşleş'e basılır.")
                                    .clicked()
                                {
                                    send = Some(core::PairTarget { ip: ip.clone(), port: port.clone(), name: name.clone() });
                                }
                            });
                        } else if let Some((id, name)) = n.checked_sub(list.len()).and_then(|k| saved.get(k)) {
                            tile(ui, palette, false, |ui| {
                                ui.horizontal(|ui| {
                                    icon_badge(ui, palette, Icon::Screens, false);
                                    ui.vertical(|ui| {
                                        ui.spacing_mut().item_spacing.y = 0.0;
                                        ui.add(egui::Label::new(design::strong(name, 15.0)).truncate());
                                        ui.label(RichText::new("Ağda görünmüyor").size(12.0).color(palette.muted));
                                    });
                                });
                                ui.add_space(14.0);
                                ui.horizontal(|ui| {
                                    status_pill(ui, palette, "Kayıtlı", palette.muted)
                                        .on_hover_text("Eşleşmiş cihaz. Ağda göründüğünde Mause kendiliğinden bağlanır.");
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if design::key(ui, "Kaldır", KeyKind::Plain, true, 0.0)
                                            .on_hover_text("Eşleşme parolasını bu bilgisayardan sil")
                                            .clicked()
                                        {
                                            unpair = Some(id.clone());
                                        }
                                    });
                                });
                            });
                        }
                    });
                }
            });
            ui.add_space(gap);
        }
        if let Some(id) = unpair {
            self.unpair_device(&id);
        }
        if let Some(id) = hold {
            self.hold_device(&id);
        }
        if let Some(id) = resume {
            self.resume_device(&id);
        }
        if let (Some(target), Some(sh)) = (send, &self.shared) {
            *sh.pairing.lock().unwrap() = None;
            *sh.pair_target.lock().unwrap() = Some(target);
        }
    }

    fn page_audio(&mut self, ui: &mut egui::Ui) {
        if !self.audio.ok {
            let description = if cfg!(target_os = "linux") {
                "PipeWire / PulseAudio bağlantısını ve pactl, parec, pacat araçlarını kontrol edin."
            } else {
                "Bu bilgisayarda ses cihazı bulunamadı."
            };
            empty_state(ui, Icon::Audio, "Ses paylaşımı kullanılamıyor", description);
            return;
        }
        section(ui, Some("Bu bilgisayarın ses cihazları"), |ui| {
            for (n, (kind, title)) in [
                (Kind::Sink, "Çıkış (hoparlör / kulaklık)"),
                (Kind::Source, "Giriş (mikrofon)"),
            ]
            .into_iter()
            .enumerate()
            {
                if n > 0 {
                    divider(ui);
                }
                let (devs, def, vol) = match kind {
                    Kind::Sink => (
                        self.audio.sinks.clone(),
                        self.audio.def_sink.clone(),
                        self.audio.out_vol,
                    ),
                    Kind::Source => (
                        self.audio.sources.clone(),
                        self.audio.def_source.clone(),
                        self.audio.in_vol,
                    ),
                };
                let cur = devs
                    .iter()
                    .find(|d| d.name == def)
                    .map(|d| d.desc.clone())
                    .unwrap_or_else(|| "Cihaz yok".into());
                setting_row(ui, title, None, 300.0, |ui| {
                    egui::ComboBox::from_id_salt(title)
                        .width(280.0)
                        .selected_text(cur)
                        .show_ui(ui, |ui| {
                            for d in &devs {
                                if ui.selectable_label(d.name == def, &d.desc).clicked() {
                                    audio::set_default(kind, &d.name);
                                    match kind {
                                        Kind::Sink => {
                                            self.cfg.audio_sink = d.name.clone();
                                            self.audio.def_sink = d.name.clone();
                                        }
                                        Kind::Source => {
                                            self.cfg.audio_source = d.name.clone();
                                            self.audio.def_source = d.name.clone();
                                        }
                                    }
                                    self.cfg.save();
                                }
                            }
                        });
                });
                if !audio::HAS_VOLUME {
                    continue;
                }
                setting_row(ui, "Ses seviyesi", None, 340.0, |ui| {
                    let mut mute = vol.1;
                    if design::toggle(ui, &mut mute, "Sessiz", true).changed() {
                        audio::set_mute(kind, mute);
                        match kind {
                            Kind::Sink => self.audio.out_vol.1 = mute,
                            Kind::Source => self.audio.in_vol.1 = mute,
                        }
                        self.audio_at = Instant::now();
                    }
                    ui.label(
                        RichText::new("Sessiz")
                            .size(14.0)
                            .color(Palette::of(ui).muted),
                    );
                    ui.add_space(8.0);
                    let mut v = vol.0 as f32;
                    ui.spacing_mut().slider_width = 180.0;
                    if ui
                        .add(egui::Slider::new(&mut v, 0.0..=100.0).suffix("%").integer())
                        .changed()
                    {
                        audio::set_volume(kind, v as u32);
                        match kind {
                            Kind::Sink => self.audio.out_vol.0 = v as u32,
                            Kind::Source => self.audio.in_vol.0 = v as u32,
                        }
                        self.audio_at = Instant::now();
                    }
                });
            }
        });

        section(ui, Some("Ağ üzerinden aktarım"), |ui| {
            hint(ui, "İki PC bağlanınca devreye girer.");
            ui.add_space(6.0);
            for k in 0..2usize {
                let (label, description) = if k == 0 {
                    (
                        "Karşı bilgisayarın sesini burada çal",
                        "Karşı PC'nin sesi bu PC'nin hoparlöründen çıkar.",
                    )
                } else {
                    (
                        "Karşı bilgisayarın mikrofonunu burada kullan",
                        if cfg!(target_os = "linux") {
                            "Karşı PC'nin mikrofonu bu PC'de \"Mause-Mikrofon\" adlı giriş olarak görünür."
                        } else {
                            "Karşı PC'nin mikrofonu BlackHole / VB-CABLE gibi bir sanal ses kablosu varsa ona çalınır ve uygulamalarda mikrofon olarak seçilebilir; yoksa hoparlörden duyulur."
                        },
                    )
                };
                let mut on = if k == 0 {
                    self.cfg.relay_in
                } else {
                    self.cfg.mic_in
                };
                let live = on
                    && self
                        .shared
                        .as_ref()
                        .and_then(|s| *s.relay_last[k].lock().unwrap())
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(1));
                let changed = setting_row(ui, label, Some(description), 150.0, |ui| {
                    let changed = design::toggle(ui, &mut on, label, true).changed();
                    if on {
                        let palette = Palette::of(ui);
                        if live {
                            ui.label(RichText::new("Alınıyor").size(14.0).color(palette.ok));
                        } else {
                            ui.label(RichText::new("Bekleniyor").size(14.0).color(palette.muted));
                        }
                    }
                    changed
                });
                if changed {
                    self.set_pref(if k == 0 { Pref::RelayIn } else { Pref::MicIn }, on);
                }
                divider(ui);
            }
            for k in 0..2usize {
                let (devs, cur_name, title) = if k == 0 {
                    (
                        self.audio.sinks.clone(),
                        self.cfg.relay_src.clone(),
                        "Gönderilecek ses çıkışı",
                    )
                } else {
                    (
                        self.audio.sources.clone(),
                        self.cfg.mic_src.clone(),
                        "Gönderilecek mikrofon",
                    )
                };
                let cur = devs
                    .iter()
                    .find(|d| d.name == cur_name)
                    .map(|d| d.desc.clone())
                    .unwrap_or_else(|| "Varsayılan".into());
                let pick = setting_row(
                    ui,
                    title,
                    Some("Karşı PC isterse bu PC'den gönderilir."),
                    300.0,
                    |ui| {
                        let mut pick: Option<String> = None;
                        egui::ComboBox::from_id_salt(("relay_src", k))
                            .width(280.0)
                            .selected_text(cur)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(cur_name.is_empty(), "Varsayılan")
                                    .clicked()
                                {
                                    pick = Some(String::new());
                                }
                                for d in &devs {
                                    if ui.selectable_label(d.name == cur_name, &d.desc).clicked() {
                                        pick = Some(d.name.clone());
                                    }
                                }
                            });
                        pick
                    },
                );
                if let Some(p) = pick {
                    if let Some(s) = &self.shared {
                        s.src.lock().unwrap()[k] = p.clone();
                    }
                    if k == 0 {
                        self.cfg.relay_src = p
                    } else {
                        self.cfg.mic_src = p
                    }
                    self.cfg.save();
                }
                divider(ui);
            }
            setting_row(
                ui,
                "Tampon (gecikme)",
                Some(
                    "Düşük değer az gecikme verir ama Wi-Fi'da çıtırtı yapabilir. Kablolu ağda 30-50 ms yeterli. Değişiklik, aktarımı kapatıp açınca geçerli olur.",
                ),
                280.0,
                |ui| {
                    let mut l = self.cfg.relay_latency as f32;
                    ui.spacing_mut().slider_width = 180.0;
                    if ui
                        .add(
                            egui::Slider::new(&mut l, 20.0..=400.0)
                                .suffix(" ms")
                                .integer(),
                        )
                        .changed()
                    {
                        self.cfg.relay_latency = l as u32;
                        if let Some(s) = &self.shared {
                            s.latency.store(l as u32, Ordering::Relaxed);
                        }
                        self.cfg.save();
                    }
                },
            );
        });
    }

    fn page_files(&mut self, ui: &mut egui::Ui, state: &State, hovering: bool) {
        let links: Vec<core::Link> = match (state, &self.shared) {
            (State::Connected { .. }, Some(sh)) => sh.links.lock().unwrap().clone(),
            _ => Vec::new(),
        };
        let current = self.shared.as_ref().and_then(|sh| {
            links
                .iter()
                .position(|l| sh.serves(core::Feat::Files, &l.id))
        });
        if links.len() > 1 {
            let names: Vec<&str> = links.iter().map(|l| l.name.as_str()).collect();
            let picked = ui
                .horizontal(|ui| {
                    ui.label(
                        RichText::new("Karşı cihaz")
                            .size(13.0)
                            .color(Palette::of(ui).muted),
                    );
                    ui.add_space(8.0);
                    design::segmented(ui, "files_device", &names, current.unwrap_or(0))
                })
                .inner;
            if let (Some(i), Some(sh)) = (picked, &self.shared) {
                sh.set_target(core::Feat::Files, &links[i].id);
            }
            ui.add_space(12.0);
        }
        let chosen = self
            .shared
            .as_ref()
            .and_then(|sh| links.iter().find(|l| sh.serves(core::Feat::Files, &l.id)));
        let peer = match state {
            State::Connected { peer, .. } => Some(chosen.map_or(peer.clone(), |l| l.name.clone())),
            _ => None,
        };
        let allowed = chosen
            .map_or(self.peer_prefs(), |l| l.prefs)
            .map(|bits| bits & Pref::FilesIn.bit() != 0);
        let permission_changed = allowed != self.peer_files;
        self.peer_files = allowed;
        let reload = {
            let mut r = self.remote.lock().unwrap();
            if r.peer != peer || permission_changed {
                *r = Pane {
                    peer: peer.clone(),
                    ..Default::default()
                };
                peer.is_some()
            } else {
                false
            }
        };
        if reload {
            self.open_remote(String::new(), String::new());
        }
        if self.local.path.is_empty() || self.local_dirty.swap(false, Ordering::Relaxed) {
            let path = self.local.path.clone();
            self.open_local(&path, "");
        }

        let height = (ui.ctx().content_rect().height() - 440.0).max(200.0);
        let remote_ready = {
            let r = self.remote.lock().unwrap();
            !r.path.is_empty() && r.error.is_none()
        };
        let transfers: Vec<transfer::Transfer> = self
            .shared
            .as_ref()
            .map(|s| s.transfers.lock().unwrap().clone())
            .unwrap_or_default();
        let (side, gap) = (290.0, 12.0);
        let actions = if ui.available_width() >= 960.0 {
            let mut actions = [None, None];
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = gap;
                let left = ui.available_width() - side - gap;
                ui.allocate_ui_with_layout(
                    Vec2::new(left, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(left);
                        actions = self.file_columns(ui, &peer, hovering, remote_ready, height);
                    },
                );
                ui.allocate_ui_with_layout(
                    Vec2::new(side, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(side);
                        transfers_tile(ui, &transfers, height + 130.0);
                    },
                );
            });
            actions
        } else {
            let actions = self.file_columns(ui, &peer, hovering, remote_ready, height);
            ui.add_space(12.0);
            transfers_tile(ui, &transfers, 260.0);
            actions
        };
        let [local_action, remote_action] = actions;
        match local_action {
            Some(PaneAction::Open(base, child)) => {
                self.local.status = None;
                self.open_local(&base, &child)
            }
            Some(PaneAction::Back) => self.go_back(false),
            Some(PaneAction::Transfer(names)) => {
                if let Some(sh) = self.shared.clone() {
                    let dir = self.remote.lock().unwrap().path.clone();
                    for name in names {
                        let (sh2, pane) = (sh.clone(), self.remote.clone());
                        transfer::upload(
                            sh.clone(),
                            Path::new(&self.local.path).join(name),
                            dir.clone(),
                            move || reload_remote(&sh2, &pane),
                        );
                    }
                }
            }
            Some(PaneAction::NewFolder) => {
                self.file_dialog = Some(FileDialog::NewFolder {
                    remote: false,
                    name: String::new(),
                })
            }
            Some(PaneAction::Delete(names)) if !names.is_empty() => {
                self.file_dialog = Some(FileDialog::Delete {
                    remote: false,
                    names,
                })
            }
            _ => {}
        }
        match remote_action {
            Some(PaneAction::Open(base, child)) => {
                self.remote.lock().unwrap().status = None;
                self.open_remote(base, child)
            }
            Some(PaneAction::Back) => self.go_back(true),
            Some(PaneAction::Transfer(names)) => {
                if let Some(sh) = self.shared.clone() {
                    let dir = self.remote.lock().unwrap().path.clone();
                    for name in names {
                        let dirty = self.local_dirty.clone();
                        let folder = self
                            .remote
                            .lock()
                            .unwrap()
                            .entries
                            .iter()
                            .any(|e| e.name == name && e.dir);
                        let download = if folder {
                            transfer::download_folder
                        } else {
                            transfer::download
                        };
                        download(
                            sh.clone(),
                            dir.clone(),
                            name,
                            PathBuf::from(&self.local.path),
                            move || dirty.store(true, Ordering::Relaxed),
                        );
                    }
                }
            }
            Some(PaneAction::NewFolder) => {
                self.file_dialog = Some(FileDialog::NewFolder {
                    remote: true,
                    name: String::new(),
                })
            }
            Some(PaneAction::Delete(names)) if !names.is_empty() => {
                self.file_dialog = Some(FileDialog::Delete {
                    remote: true,
                    names,
                })
            }
            _ => {}
        }
        let ctx = ui.ctx().clone();
        self.file_dialog_window(&ctx);
    }

    fn file_columns(
        &mut self,
        ui: &mut egui::Ui,
        peer: &Option<String>,
        hovering: bool,
        remote_ready: bool,
        height: f32,
    ) -> [Option<PaneAction>; 2] {
        let local_name = self.cfg.name.clone();
        let mut actions: [Option<PaneAction>; 2] = [None, None];
        ui.columns(2, |cols| {
            cols[0].add_space(2.0);
            actions[0] = file_pane(
                &mut cols[0],
                &mut self.local,
                PaneHead {
                    title: "Yerel Bilgisayar",
                    subtitle: &local_name,
                    os: local_os_icon(),
                    remote: false,
                    can_act: remote_ready,
                    note: None,
                    hot: false,
                },
                height,
            );
            let mut remote = self.remote.lock().unwrap();
            let subtitle = peer.clone().unwrap_or_else(|| "Bağlı değil".into());
            let os = if remote.path.is_empty() {
                Icon::Screens
            } else {
                path_os_icon(&remote.path)
            };
            cols[1].add_space(2.0);
            actions[1] = file_pane(
                &mut cols[1],
                &mut remote,
                PaneHead {
                    title: "Uzak Bilgisayar",
                    subtitle: &subtitle,
                    os,
                    remote: true,
                    can_act: true,
                    note: peer
                        .is_none()
                        .then_some("Karşı bilgisayarın dosyalarını görmek için önce bağlanın."),
                    hot: hovering && remote_ready,
                },
                height,
            );
        });
        actions
    }

    fn page_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let available = ui.available_size();
        let height = available.y.max(360.0);
        let compact = available.x < 940.0;
        let nav_width = if compact { 58.0 } else { 220.0 };
        let update_ready = matches!(*self.update.lock().unwrap(), Upd::Available(_));
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(nav_width, height),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(nav_width);
                    if compact {
                        ui.add_space(2.0);
                    } else {
                        ui.label(
                            RichText::new("AYARLAR MENÜSÜ")
                                .font(design::semibold(14.0))
                                .color(Palette::of(ui).ink2),
                        );
                        ui.add_space(12.0);
                    }
                    for page in SettingsPage::ALL {
                        if settings_nav_item(
                            ui,
                            page,
                            self.settings_page == page,
                            update_ready,
                            compact,
                        )
                        .clicked()
                        {
                            self.settings_page = page;
                        }
                        ui.add_space(4.0);
                    }
                },
            );
            ui.add(egui::Separator::default().vertical());
            ui.add_space(16.0);
            let page = self.settings_page;
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), height),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.label(
                        RichText::new(page.title())
                            .font(design::semibold(20.0))
                            .color(Palette::of(ui).ink),
                    );
                    ui.add_space(16.0);
                    egui::ScrollArea::vertical()
                        .id_salt("settings_content")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Frame::new()
                                .inner_margin(Margin {
                                    right: 18,
                                    bottom: 16,
                                    ..Default::default()
                                })
                                .show(ui, |ui| self.settings_content(ui, ctx, page));
                        });
                },
            );
        });
    }

    fn settings_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, page: SettingsPage) {
        if page == SettingsPage::Sharing && !self.cfg.active_peer.is_empty() {
            let name = self
                .shared
                .as_ref()
                .and_then(|sh| sh.primary_link())
                .map(|link| link.name)
                .unwrap_or_else(|| self.cfg.active_peer.clone());
            hint(
                ui,
                &format!(
                    "Paylaşım ayarları {name} için hatırlanır; bağlı bilgisayar değişince otomatik yüklenir."
                ),
            );
        }
        if page == SettingsPage::General {
            section(ui, Some("Uygulama"), |ui| {
                let mut au = autostart_on();
                if toggle_row(
                    ui,
                    "Bilgisayar açılınca Mause'u başlat",
                    (!AUTOSTART_OK).then_some("Bu platformda henüz desteklenmiyor."),
                    &mut au,
                    AUTOSTART_OK,
                )
                .on_disabled_hover_text("Bu platformda henüz desteklenmiyor")
                .changed()
                {
                    autostart_set(au);
                }
                divider(ui);
                let picked = setting_row(ui, "Tema", None, 240.0, |ui| {
                    design::segmented(
                        ui,
                        "theme",
                        &["Sistem", "Koyu", "Açık"],
                        self.cfg.theme as usize,
                    )
                });
                if let Some(i) = picked {
                    self.cfg.theme = i as u8;
                    apply_style(ctx, self.cfg.theme);
                    self.cfg.save();
                }
                divider(ui);
                let mut remote = self.cfg.allow_remote_prefs;
                if toggle_row(
                ui,
                "Eşleşmiş cihazlar paylaşım ayarlarımı değiştirebilsin",
                    Some("Karşı cihaz paylaşım akışlarını açıp kapatabilir; bu izin kapalıyken yalnızca kapatabilir."),
                &mut remote,
                true,
            )
            .changed()
            {
                self.cfg.allow_remote_prefs = remote;
                self.sync_perms();
            }
            });
        }

        if page == SettingsPage::Connection {
            section(ui, Some("Bu bilgisayar"), |ui| {
                let mut changed = false;
                setting_row(ui, "Bu PC'nin adı", None, 220.0, |ui| {
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut self.cfg.name)
                                .desired_width(200.0)
                                .min_size(Vec2::new(0.0, 34.0))
                                .align(Align2::LEFT_CENTER),
                        )
                        .lost_focus();
                });
                divider(ui);
                setting_row(ui, "Port", Some("İki PC'de aynı olmalı."), 100.0, |ui| {
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut self.cfg.port)
                                .desired_width(80.0)
                                .min_size(Vec2::new(0.0, 34.0))
                                .align(Align2::LEFT_CENTER),
                        )
                        .lost_focus();
                });
                divider(ui);
                setting_row(ui, "Ekran çözünürlüğü", None, 300.0, |ui| {
                    if design::key(ui, "Yeniden algıla", KeyKind::Plain, true, 0.0).clicked() {
                        if let Some((w, h)) = crate::platform::detect_screen() {
                            (self.cfg.w, self.cfg.h) = (w, h);
                        }
                    }
                    ui.add(egui::DragValue::new(&mut self.cfg.h).range(240..=16384));
                    ui.label("×");
                    ui.add(egui::DragValue::new(&mut self.cfg.w).range(320..=16384));
                });
                if changed {
                    self.restart();
                }
            });
        }

        if page == SettingsPage::Connection {
            self.secret_section(ui);
        }
        if page == SettingsPage::Sharing {
            self.roles_section(ui);
            self.screen_section(ui);
        }
        if page == SettingsPage::General {
            section(ui, Some("Performans göstergeleri"), |ui| {
                let mut changed = toggle_row(ui, "Bu PC'nin performansını bağlı cihazlarla paylaş", Some("Yalnızca toplam kullanım değerleri paylaşılır. İşlem, dosya ve ağ adresi listeleri gönderilmez."), &mut self.cfg.share_metrics, true).changed();
                changed |= toggle_row(ui, "Cihaz kartlarında performansı göster", Some("Değerler yaklaşık iki saniyede bir güncellenir. Karşı PC paylaşımı kapatabilir."), &mut self.cfg.show_metrics, true).changed();
                ui.horizontal_wrapped(|ui| {
                    for (index, label) in ["İşlemci", "RAM", "GPU", "Disk", "Ağ"].iter().enumerate()
                    {
                        let mut on = self.cfg.metrics_mask & (1 << index) != 0;
                        if ui.checkbox(&mut on, *label).changed() {
                            if on {
                                self.cfg.metrics_mask |= 1 << index;
                            } else {
                                self.cfg.metrics_mask &= !(1 << index);
                            }
                            changed = true;
                        }
                    }
                });
                if changed {
                    self.sync_perms();
                }
            });
        }

        if page == SettingsPage::Connection {
            section(ui, Some("Bağlantı izinleri"), |ui| {
                let mut req = self.cfg.require_approval;
                if toggle_row(
                ui,
                "Yeni cihazlar için onay iste",
                Some("Kapalıyken parolayı bilen herkes onaysız bağlanır. Engellenen cihazlarla hiçbir zaman bağlantı kurulmaz."),
                &mut req,
                true,
            )
            .changed()
            {
                self.cfg.require_approval = req;
                self.sync_policy();
            }
                divider(ui);
                ui.add_space(4.0);
                ui.label(design::strong("Güvenilen cihazlar", BODY));
                if self.cfg.trusted.is_empty() {
                    hint(ui, "Henüz yok. Bağlandığınız cihazlar buraya eklenir.");
                }
                let mut remove: Option<usize> = None;
                let mut block: Option<usize> = None;
                for (n, (_, name)) in self.cfg.trusted.iter().enumerate() {
                    let title = if name.is_empty() {
                        "Adsız cihaz"
                    } else {
                        name
                    };
                    setting_row(ui, title, None, 200.0, |ui| {
                        if design::key(ui, "Engelle", KeyKind::Danger, true, 0.0).clicked() {
                            block = Some(n);
                        }
                        if design::key(ui, "Kaldır", KeyKind::Plain, true, 0.0).clicked() {
                            remove = Some(n);
                        }
                    });
                }
                if let Some(n) = remove {
                    self.cfg.trusted.remove(n);
                    self.sync_policy();
                } else if let Some(n) = block {
                    let e = self.cfg.trusted.remove(n);
                    self.cfg.blocked.push(e);
                    self.sync_policy();
                }
                divider(ui);
                ui.add_space(4.0);
                ui.label(design::strong("Engellenen cihazlar", BODY));
                if self.cfg.blocked.is_empty() {
                    hint(ui, "Engellenen cihaz yok.");
                }
                let mut unblock: Option<usize> = None;
                for (n, (_, name)) in self.cfg.blocked.iter().enumerate() {
                    let title = if name.is_empty() {
                        "Adsız cihaz"
                    } else {
                        name
                    };
                    setting_row(ui, title, None, 150.0, |ui| {
                        if design::key(ui, "Engeli kaldır", KeyKind::Plain, true, 0.0).clicked() {
                            unblock = Some(n);
                        }
                    });
                }
                if let Some(n) = unblock {
                    self.cfg.blocked.remove(n);
                    self.sync_policy();
                }
            });
        }

        if page == SettingsPage::Sharing {
            section(ui, Some("Pano ile paylaşım"), |ui| {
                let supported = crate::clip::AVAILABLE;
                let unsupported = "Pano ile paylaşım bu platformda desteklenmiyor.";
                let copy = if cfg!(target_os = "macos") {
                    "Cmd+C"
                } else {
                    "Ctrl+C"
                };
                let carry_hint = format!(
                    "Dosyayı seçip {copy} yapın, mouse'u diğer PC'ye götürün; dosya orada İndirilenler/Mause'ye iner ve panoya konur."
                );
                let mut carry = self.cfg.carry_files;
                if toggle_row(
                    ui,
                    "Kopyalanan dosya ve klasörleri ekran geçişinde taşı",
                    Some(if supported {
                        carry_hint.as_str()
                    } else {
                        unsupported
                    }),
                    &mut carry,
                    supported,
                )
                .on_disabled_hover_text(unsupported)
                .changed()
                {
                    self.cfg.carry_files = carry;
                    if let Some(s) = &self.shared {
                        s.carry_files.store(carry, Ordering::Relaxed);
                    }
                    self.cfg.save();
                }
                divider(ui);
                let mut st = self.cfg.share_text;
                if toggle_row(
                    ui,
                    "Kopyalanan metni de taşı",
                    Some("Mouse karşıya geçince panodaki metin de karşıya gider."),
                    &mut st,
                    supported,
                )
                .on_disabled_hover_text(unsupported)
                .changed()
                {
                    self.set_pref(Pref::ShareText, st);
                }
                divider(ui);
                let inbox = transfer::inbox();
                if setting_row(
                    ui,
                    "Alınanlar klasörü",
                    Some(&inbox.display().to_string()),
                    140.0,
                    |ui| design::key(ui, "Klasörü aç", KeyKind::Plain, true, 0.0).clicked(),
                ) {
                    let _ = std::fs::create_dir_all(&inbox);
                    crate::platform::open_path(&inbox);
                }
            });
        }

        if page == SettingsPage::About {
            section(ui, Some("Güncelleme"), |ui| {
                let upd = self.update.lock().unwrap().clone();
                let status = match &upd {
                    Upd::Unknown => "Henüz denetlenmedi.".to_owned(),
                    Upd::Checking => "Denetleniyor…".to_owned(),
                    Upd::Latest => "Mause güncel.".to_owned(),
                    Upd::Available(r) => format!("Yeni sürüm hazır: {}", r.version),
                    Upd::Installing => "Güncelleniyor…".to_owned(),
                    Upd::Failed(e) => e.clone(),
                };
                let busy = matches!(upd, Upd::Checking | Upd::Installing);
                let title = format!("Bu bilgisayarda Mause {}", update::VERSION);
                let mut install = None;
                let check = setting_row(ui, &title, Some(&status), 300.0, |ui| {
                    if let Upd::Available(r) = &upd {
                        let key = design::key(ui, "Güncelle", KeyKind::Primary, true, 0.0);
                        red_dot(
                            ui.painter(),
                            key.rect.right_top() + Vec2::new(-3.0, 3.0),
                            4.5,
                            Palette::of(ui),
                        );
                        if key.clicked() {
                            install = Some(r.clone());
                        }
                    }
                    design::key(ui, "Güncellemeleri denetle", KeyKind::Plain, !busy, 0.0).clicked()
                });
                if let Some(r) = install {
                    self.install_update(r);
                } else if check {
                    self.check_update();
                }
            });

            section(ui, Some("Hakkında"), |ui| {
                let names = DEVELOPERS.map(|d| d.name).join(" ve ");
                let about = format!("Geliştirenler: {names}");
                if setting_row(
                    ui,
                    &format!("Mause {}", env!("CARGO_PKG_VERSION")),
                    Some(&about),
                    180.0,
                    |ui| design::key(ui, "Geliştirenler", KeyKind::Plain, true, 0.0).clicked(),
                ) {
                    self.devs_modal = true;
                }
            });
        }
    }

    fn share_computers(&mut self, ui: &mut egui::Ui, state: &State, running: bool) {
        let palette = Palette::of(ui);
        let paired = self.cfg.paired.len();
        let names: Vec<String> = self
            .shared
            .as_ref()
            .map(|sh| {
                sh.links
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|l| l.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let (summary, tone) = if names.len() > 1 {
            (format!("{} ile bağlı", names.join(", ")), palette.ok)
        } else if let State::Connected { peer, .. } = state {
            (format!("{peer} ile bağlı"), palette.ok)
        } else if paired == 0 && running {
            ("Ağdaki bilgisayarlar aranıyor".to_owned(), palette.signal)
        } else if paired == 0 {
            ("Henüz eşleşmiş bilgisayar yok".to_owned(), palette.muted)
        } else if paired == 1 {
            ("1 eşleşmiş bilgisayar".to_owned(), palette.ink2)
        } else {
            (format!("{paired} eşleşmiş bilgisayar"), palette.ink2)
        };
        ui.horizontal(|ui| {
            ui.label(design::strong("Bilgisayarlar", LEAD));
            ui.label(RichText::new("·").size(BODY).color(palette.muted));
            ui.label(RichText::new(summary).size(13.0).color(tone));
        });
        ui.add_space(4.0);
        hint(
            ui,
            "IP adresiyle doğrudan bağlanın veya ağda görünen ve daha önce eşleşmiş bilgisayarları seçin.",
        );
        ui.add_space(14.0);
        self.kvm_computers(ui, state, running);
        divider(ui);
        let mut changed = false;
        setting_row(
            ui,
            "Otomatik bağlantı adresi",
            Some("Özel bir ağ adresi gerekmiyorsa boş bırakın."),
            170.0,
            |ui| {
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.cfg.addr)
                            .hint_text("Otomatik")
                            .desired_width(150.0)
                            .min_size(Vec2::new(0.0, 34.0))
                            .align(Align2::LEFT_CENTER),
                    )
                    .lost_focus();
            },
        );
        if changed {
            self.restart();
        }
        ui.add_space(12.0);
    }

    fn secret_section(&mut self, ui: &mut egui::Ui) {
        section(ui, Some("Ortak parola"), |ui| {
            let mut changed = false;
            setting_row(
                ui,
                "Parola",
                Some(
                    "Eşleşme yerine iki PC'ye aynı parolayı yazarak da bağlanabilirsiniz (en az 4 karakter). Boş bırakılabilir.",
                ),
                250.0,
                |ui| {
                    if design::key(ui, "Kod üret", KeyKind::Plain, true, 0.0).clicked() {
                        self.cfg.secret = crate::config::random_secret();
                        self.show_secret = true;
                        changed = true;
                    }
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut self.cfg.secret)
                                .password(!self.show_secret)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(140.0)
                                .min_size(Vec2::new(0.0, 34.0))
                                .align(Align2::LEFT_CENTER),
                        )
                        .lost_focus();
                },
            );
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let enabled = !self.cfg.secret.is_empty();
                    if design::key(ui, "Kopyala", KeyKind::Plain, enabled, 0.0).clicked() {
                        ui.ctx().copy_text(self.cfg.secret.clone());
                    }
                    let label = if self.show_secret { "Gizle" } else { "Göster" };
                    if design::key(ui, label, KeyKind::Plain, enabled, 0.0).clicked() {
                        self.show_secret = !self.show_secret;
                    }
                });
            });
            if changed {
                self.restart();
            }
        });
    }

    fn roles_section(&mut self, ui: &mut egui::Ui) {
        section(ui, Some("Roller ve sınırlar"), |ui| {
            hint(
                ui,
                "Bu PC'nin tarafındaki izinler. Karşı PC'nin kendi ayarları da geçerlidir: ikisi birden izin vermedikçe özellik çalışmaz.",
            );
            ui.add_space(10.0);
            ui.label(design::strong(
                "Hangi PC'nin klavye ve mouse'u kullanılsın?",
                BODY,
            ));
            ui.add_space(4.0);
            let current = (self.cfg.allow_control_out, self.cfg.allow_control_in);
            let roles: [(&str, &str, bool, bool); 4] = [
                ("İkisi de", "İki PC birbirini yönetebilir.", true, true),
                (
                    "Bu PC yönetir",
                    "Bu PC'nin klavye ve mouse'u karşıyı yönetir; karşı bunu yönetemez.",
                    true,
                    false,
                ),
                (
                    "Karşı PC yönetir",
                    "Karşının klavye ve mouse'u bu PC'yi yönetir; bu PC karşıyı yönetemez.",
                    false,
                    true,
                ),
                (
                    "Kontrol kapalı",
                    "Yalnızca dosya, ses ve pano paylaşılır.",
                    false,
                    false,
                ),
            ];
            let mut changed = false;
            for (title, description, out, inn) in roles {
                let picked = setting_row(ui, title, Some(description), 48.0, |ui| {
                    design::radio(ui, current == (out, inn), title).clicked()
                });
                if picked && current != (out, inn) {
                    self.cfg.allow_control_out = out;
                    self.cfg.allow_control_in = inn;
                    changed = true;
                }
            }
            if changed {
                self.sync_perms();
            }
            divider(ui);
            ui.add_space(4.0);
            ui.label(design::strong(
                "Bu PC'nin hangi klavye ve mouse'ları karşıya geçsin?",
                BODY,
            ));
            if self.devices.is_empty() {
                hint(ui, "Cihaz bulunamadı ya da okuma izni yok.");
            }
            let mut dev_changed = false;
            for (name, kind) in self.devices.clone() {
                let mut used = !self.cfg.excluded.contains(&name);
                let description = if kind == "mouse" { "Mouse" } else { "Klavye" };
                if toggle_row(ui, &name, Some(description), &mut used, true).changed() {
                    self.cfg.excluded.retain(|n| *n != name);
                    if !used {
                        self.cfg.excluded.push(name);
                    }
                    dev_changed = true;
                }
            }
            if !self.devices.is_empty() {
                hint(
                    ui,
                    "Kapalı cihaz yalnızca bu PC'de kalır (örneğin ikinci bir klavyeniz varsa).",
                );
            }
            if dev_changed {
                self.sync_perms();
            }
        });
    }

    fn screen_section(&mut self, ui: &mut egui::Ui) {
        section(ui, Some("Ekran paylaşımı"), |ui| {
            #[cfg(target_os = "windows")]
            {
                hint(
                    ui,
                    "Bu PC'nin masaüstünü diğer PC'ye genişletmek için sürücüyü bu PC'ye kurun. Diğer PC'de Ekran penceresinden ‘Ek monitör olarak kullan’ı seçin. Ana ekranı görmek için ‘Mevcut ekranı izle’yi seçin.",
                );
                let ready = crate::vdisplay::available();
                let busy = self.vdisplay_note.lock().unwrap().as_deref() == Some("Kuruluyor…");
                if ui
                    .add_enabled(
                        !ready && !busy,
                        egui::Button::new(if ready {
                            "Sanal ekran sürücüsü hazır"
                        } else {
                            "Sanal ekran sürücüsünü kur"
                        }),
                    )
                    .clicked()
                {
                    let note = self.vdisplay_note.clone();
                    *note.lock().unwrap() = Some("Kuruluyor…".into());
                    std::thread::spawn(move || {
                        *note.lock().unwrap() = Some(match crate::vdisplay::setup() {
                            Ok(()) => "Sanal ekran sürücüsü hazır.".into(),
                            Err(e) => format!("Kurulamadı: {e}"),
                        });
                    });
                }
                if let Some(note) = self.vdisplay_note.lock().unwrap().as_ref() {
                    ui.label(note);
                }
                if ready && ui.button("Sanal ekranı kapat").on_hover_text("Aktif paylaşım yokken sanal monitörü Windows masaüstünden kaldırır; sürücü kurulu kalır.").clicked() {
                    *self.vdisplay_note.lock().unwrap() = Some(match crate::vdisplay::park() {
                        Ok(()) => "Sanal ekran kapatıldı; ek monitör paylaşımı başlayınca yeniden açılır.".into(),
                        Err(e) => e,
                    });
                }
                divider(ui);
            }
            let mut changed = toggle_row(
                ui,
                "Karşı PC bu PC'nin ekranını izleyebilsin",
                Some("Paylaşım sayfasındaki Ekran tuşuyla aynı ayar."),
                &mut self.cfg.allow_screen_out,
                true,
            )
            .changed();
            divider(ui);
            let can = self.cfg.allow_screen_out;
            changed |= toggle_row(
                ui,
                "İzlerken bu PC'yi kontrol edebilsin",
                Some("Karşı PC izleme penceresinde \"Kontrol et\"i açınca mouse ve klavyesi bu PC'yi yönetir."),
                &mut self.cfg.allow_screen_control,
                can,
            )
            .changed();
            if changed {
                self.sync_perms();
            }
            divider(ui);
            let mut o = self.cfg.screen;
            setting_row(
                ui,
                "Karşıyı izlerken",
                Some("İzleme penceresinin üst çubuğundan da değiştirilebilir."),
                470.0,
                |ui| {
                    stream_controls(ui, "settings", &mut o);
                },
            );
            if o != self.cfg.screen {
                self.cfg.screen = o;
                if let Some(sh) = &self.shared {
                    *sh.screen_opts.lock().unwrap() = o;
                }
                self.cfg.save();
            }
        });
    }

    fn page_usb(&mut self, ui: &mut egui::Ui, state: &State) {
        use crate::usb;
        let connected = matches!(state, State::Connected { .. });
        if let Err(e) = usb::tools() {
            let tone = Palette::of(ui).warn;
            let progress = self.usb_tools.clone();
            notice(
                ui,
                tone,
                Icon::Alert,
                "USB paylaşımı için araç gerekli",
                |ui| {
                    hint(ui, &e);
                    if cfg!(target_os = "macos") {
                        return;
                    }
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        let state = progress.lock().unwrap().clone();
                        let busy = state.as_deref() == Some("kuruluyor…");
                        if design::key(ui, "Araçları kur", KeyKind::Primary, !busy, 120.0).clicked()
                        {
                            *progress.lock().unwrap() = Some("kuruluyor…".into());
                            let p = progress.clone();
                            std::thread::spawn(move || {
                                *p.lock().unwrap() = Some(match usb::install_tools() {
                                    Ok(()) => "kuruldu".into(),
                                    Err(e) => format!("kurulamadı: {e}"),
                                });
                            });
                        }
                        if let Some(s) = state {
                            hint(ui, &s);
                        }
                    });
                },
            );
        }
        #[cfg(target_os = "linux")]
        let access_ready = usb::access_ready();
        #[cfg(not(target_os = "linux"))]
        let access_ready = true;
        #[cfg(target_os = "linux")]
        if !access_ready {
            let progress = self.usb_tools.clone();
            notice(
                ui,
                Palette::of(ui).signal,
                Icon::Shield,
                "USB erişimini bir kez ayarlayın",
                |ui| {
                    hint(
                        ui,
                        "Bu kurulum yönetici onayı ister. Sonrasında USB paylaşırken, bağlarken ve çıkarırken tekrar yönetici parolası sorulmaz. Yetki yalnız USB işlemlerini kapsar.",
                    );
                    let current = progress.lock().unwrap().clone();
                    let busy = current.as_deref() == Some("USB erişimi ayarlanıyor…");
                    if design::key(
                        ui,
                        "Parolasız USB erişimini ayarla",
                        KeyKind::Primary,
                        !busy,
                        290.0,
                    )
                    .clicked()
                    {
                        *progress.lock().unwrap() = Some("USB erişimi ayarlanıyor…".into());
                        let result = progress.clone();
                        std::thread::spawn(move || {
                            *result.lock().unwrap() = Some(match usb::configure_access() {
                                Ok(()) => {
                                    "USB erişimi hazır. Paylaşımda tekrar parola sorulmayacak."
                                        .into()
                                }
                                Err(e) => format!("USB erişimi kurulamadı: {e}"),
                            });
                        });
                    }
                    if let Some(text) = current {
                        hint(ui, &text);
                    }
                },
            );
        }
        self.hardware_section(ui);
        section(ui, Some("Karşı PC'nin USB cihazları"), |ui| {
            let Some(sh) = self.shared.clone() else {
                hint(ui, "Paylaşımı başlatıp karşı PC'ye bağlanınca listelenir.");
                return;
            };
            setting_row(
                ui,
                "Karşıda paylaşıma açılan cihazlar",
                Some(
                    "Bağla'ya basınca cihaz bu bilgisayara takılmış gibi çalışır; karşıda o sırada kullanılamaz.",
                ),
                120.0,
                |ui| {
                    if design::key(ui, "Listele", KeyKind::Plain, connected, 0.0).clicked() {
                        sh.usb_cmds
                            .lock()
                            .unwrap()
                            .push((usb::LIST_REQ, String::new()));
                    }
                },
            );
            if !connected {
                hint(ui, "Bağlanınca kullanılabilir.");
                return;
            }
            let remote = sh.usb_remote.lock().unwrap().clone();
            let attached = sh.usb_attached.lock().unwrap().clone();
            let status = sh.usb_status.lock().unwrap().clone();
            match &remote {
                None => hint(ui, "Listele'ye basın."),
                Some(v) if v.is_empty() => hint(
                    ui,
                    "Karşıda paylaşıma açık cihaz yok. Karşıda USB sayfasından cihaz açılmalı.",
                ),
                Some(_) => {}
            }
            for d in remote.unwrap_or_default() {
                divider(ui);
                let on = attached.iter().any(|a| a.busid == d.busid);
                let note = status.get(&d.busid).cloned().unwrap_or_else(|| {
                    if on {
                        format!("Bu bilgisayara takılı · {}", d.id)
                    } else {
                        d.id.clone()
                    }
                });
                setting_row(ui, &d.name, Some(&note), 110.0, |ui| {
                    let busy = status.get(&d.busid).is_some_and(|s| s.ends_with('…'));
                    let (label, op) = if on {
                        ("Çıkar", usb::DETACH)
                    } else {
                        ("Bağla", usb::ATTACH)
                    };
                    let kind = if on { KeyKind::Plain } else { KeyKind::Primary };
                    if design::key(ui, label, kind, !busy, 0.0).clicked() {
                        sh.usb_status
                            .lock()
                            .unwrap()
                            .insert(d.busid.clone(), "işlem bekliyor…".into());
                        sh.usb_cmds.lock().unwrap().push((op, d.busid.clone()));
                    }
                });
            }
        });
        section(ui, Some("Bu PC'nin USB cihazları"), |ui| {
            let mut changed = toggle_row(
                ui,
                "Karşı PC bu PC'nin USB cihazlarını bağlayabilsin",
                Some("Yalnızca aşağıda açtığınız cihazlar karşıda görünür. Cihaz karşıya verilince bu PC'de kullanılamaz."),
                &mut self.cfg.allow_usb_out,
                access_ready,
            )
            .changed();
            divider(ui);
            setting_row(ui, "Takılı cihazlar", None, 120.0, |ui| {
                if design::key(ui, "Yenile", KeyKind::Plain, true, 0.0).clicked()
                    || self.usb_local.is_none()
                {
                    self.usb_local = Some(usb::list_local());
                }
            });
            match self.usb_local.clone() {
                Some(Err(e)) => hint(ui, &e),
                Some(Ok(devs)) => {
                    if devs.is_empty() {
                        hint(ui, "Takılı USB cihazı yok.");
                    }
                    let exported = self
                        .shared
                        .as_ref()
                        .map(|sh| sh.usb_exported.lock().unwrap().clone())
                        .unwrap_or_default();
                    for d in devs {
                        let mut on = self.cfg.usb_shared.contains(&d.id);
                        let note = if exported.contains(&d.busid) {
                            format!("Şu an karşıda kullanılıyor · {}", d.id)
                        } else {
                            d.id.clone()
                        };
                        if toggle_row(
                            ui,
                            &d.name,
                            Some(&note),
                            &mut on,
                            self.cfg.allow_usb_out && access_ready,
                        )
                        .changed()
                        {
                            self.cfg.usb_shared.retain(|i| *i != d.id);
                            if on {
                                self.cfg.usb_shared.push(d.id.clone());
                            }
                            changed = true;
                        }
                    }
                }
                None => {}
            }
            if changed {
                if let Some(sh) = &self.shared {
                    *sh.usb_allowed.lock().unwrap() = self.cfg.usb_shared.clone();
                }
                self.sync_perms();
            }
            hint(
                ui,
                if cfg!(target_os = "linux") {
                    "Klavye ya da mouse'u karşıya verirseniz bu PC'de çalışmaz; çıkarınca geri gelir. USB erişimi bir kez ayarlandıktan sonra tekrar yönetici parolası sorulmaz."
                } else {
                    "Klavye ya da mouse'u karşıya verirseniz bu PC'de çalışmaz; çıkarınca geri gelir. İşletim sisteminin sürücü yetkileri geçerlidir."
                },
            );
        });
    }

    fn page_log(&mut self, ui: &mut egui::Ui) {
        let lines: Vec<String> = self
            .shared
            .as_ref()
            .map(|s| s.log.lock().unwrap().clone())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            if design::key(ui, "Kopyala", KeyKind::Plain, !lines.is_empty(), 0.0).clicked() {
                ui.ctx().copy_text(lines.join("\n"));
            }
            if design::key(ui, "Temizle", KeyKind::Plain, !lines.is_empty(), 0.0).clicked() {
                if let Some(s) = &self.shared {
                    s.log.lock().unwrap().clear();
                }
            }
        });
        ui.add_space(12.0);
        if lines.is_empty() {
            empty_state(
                ui,
                Icon::Log,
                "Henüz bir bağlantı olayı yok",
                "Paylaşım başladığında bağlantı ve aktarım olaylarını burada izleyebilirsiniz.",
            );
            return;
        }
        let palette = Palette::of(ui);
        egui::Frame::new()
            .fill(palette.graphite)
            .corner_radius(RADIUS_PANEL)
            .inner_margin(Margin::symmetric(18, 16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for l in &lines {
                    ui.add(
                        egui::Label::new(
                            RichText::new(l)
                                .font(FontId::monospace(13.0))
                                .color(palette.graphite_ink),
                        )
                        .wrap(),
                    );
                }
            });
    }
}

#[cfg(target_os = "linux")]
fn autostart_path() -> PathBuf {
    crate::platform::config_dir()
        .join("autostart")
        .join("mause.desktop")
}

#[cfg(target_os = "linux")]
fn autostart_on() -> bool {
    autostart_path().exists()
}

#[cfg(target_os = "linux")]
fn autostart_set(on: bool) {
    let p = autostart_path();
    if on {
        let exe = std::env::current_exe()
            .map(|e| e.display().to_string())
            .unwrap_or_else(|_| "mause".into());
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(
            p,
            format!(
                "[Desktop Entry]\nType=Application\nName=Mause\nComment=Ortak mouse ve klavye\nExec={exe}\nIcon=mause\nTerminal=false\nX-KDE-autostart-after=panel\n"
            ),
        );
    } else {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(target_os = "windows")]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(target_os = "windows")]
fn reg(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("reg")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(target_os = "windows")]
fn autostart_on() -> bool {
    reg(&["query", RUN_KEY, "/v", "Mause"])
}

#[cfg(target_os = "windows")]
fn autostart_set(on: bool) {
    if on {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let value = format!("\"{}\"", exe.display());
        reg(&[
            "add", RUN_KEY, "/v", "Mause", "/t", "REG_SZ", "/d", &value, "/f",
        ]);
    } else {
        reg(&["delete", RUN_KEY, "/v", "Mause", "/f"]);
    }
}

#[cfg(target_os = "macos")]
fn autostart_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Library/LaunchAgents/io.github.mause.plist")
}

#[cfg(target_os = "macos")]
fn autostart_on() -> bool {
    autostart_path().exists()
}

#[cfg(target_os = "macos")]
fn autostart_set(on: bool) {
    let p = autostart_path();
    if on {
        let exe = std::env::current_exe()
            .map(|e| e.display().to_string())
            .unwrap_or_else(|_| "/Applications/Mause.app/Contents/MacOS/mause".into());
        let esc = exe
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(
            p,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>io.github.mause</string>\n<key>ProgramArguments</key><array><string>{esc}</string></array>\n<key>RunAtLoad</key><true/>\n</dict></plist>\n"
            ),
        );
    } else {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn autostart_on() -> bool {
    false
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn autostart_set(_: bool) {}

const AUTOSTART_OK: bool = cfg!(any(
    target_os = "linux",
    target_os = "windows",
    target_os = "macos"
));

#[cfg(test)]
fn ui_fixture(page: Page, theme: u8) -> App {
    App {
        page,
        settings_page: SettingsPage::General,
        logo: None,
        local: Pane::default(),
        remote: Arc::default(),
        peer_files: None,
        local_dirty: Arc::default(),
        file_dialog: None,
        pair_done_at: None,
        audio: AudioView::default(),
        audio_at: Instant::now(),
        cfg: Config {
            w: 1920,
            h: 1080,
            auto_connect: false,
            theme,
            name: "Uzun-Bilgisayar-Adi-Mac-1761234567890.local".into(),
            ..Config::default()
        },
        shared: None,
        peers: Arc::default(),
        perms: (true, true),
        show_secret: false,
        restart_pending: false,
        pair_ip: String::new(),
        perm_at: Instant::now(),
        missing: Vec::new(),
        grant_msg: Arc::default(),
        auto_grant_tried: true,
        grant_handled: true,
        perm_modal: false,
        devs_modal: false,
        notes_open: false,
        bell_rect: egui::Rect::NOTHING,
        splash: None,
        devices: vec![
            ("AT Translated Set 2 keyboard".into(), "klavye"),
            ("Logitech USB Receiver Mouse".into(), "mouse"),
        ],
        tray: None,
        popup: None,
        popup_device: None,
        tray_click: None,
        screen_tex: None,
        camera: None,
        torch_on: HashSet::new(),
        vcam_note: Arc::default(),
        #[cfg(target_os = "windows")]
        vdisplay_note: Arc::default(),
        screen_ctrl: false,
        screen_mods: [false; 4],
        screen_down: Vec::new(),
        usb_local: None,
        update: Arc::new(Mutex::new(Upd::Unknown)),
        usb_tools: Arc::default(),
        update_at: None,
    }
}

#[cfg(test)]
mod file_pane_tests {
    use super::*;

    fn entry(name: &str, dir: bool, size: u64, modified: i64) -> transfer::Entry {
        transfer::Entry {
            name: name.into(),
            dir,
            size,
            modified,
        }
    }

    fn listing(path: &str) -> std::io::Result<transfer::Listing> {
        Ok(transfer::Listing {
            path: path.into(),
            entries: vec![
                entry("b.txt", false, 50, 3),
                entry("Klasör", true, 0, 1),
                entry("a.txt", false, 900, 2),
            ],
        })
    }

    #[test]
    fn yol_parcalari_tiklanabilir_tam_yol_verir() {
        let windows = path_crumbs(r"C:\Users\sk");
        assert_eq!(
            windows,
            [
                ("C:".to_owned(), r"C:\".to_owned()),
                ("Users".to_owned(), r"C:\Users".to_owned()),
                ("sk".to_owned(), r"C:\Users\sk".to_owned()),
            ]
        );
        let unix = path_crumbs("/home/pi");
        assert_eq!(
            unix,
            [
                ("/".to_owned(), "/".to_owned()),
                ("home".to_owned(), "/home".to_owned()),
                ("pi".to_owned(), "/home/pi".to_owned()),
            ]
        );
        assert_eq!(path_crumbs(transfer::DISKS).len(), 1);
    }

    #[test]
    fn geri_gecmisi_yalnizca_yeni_klasorde_buyur() {
        let mut pane = Pane::default();
        pane.fill(listing("/a"));
        pane.fill(listing("/a/b"));
        pane.fill(listing("/a/b"));
        assert_eq!(pane.back, ["/a"]);
        let previous = pane.back.pop().unwrap();
        pane.going_back = true;
        pane.fill(listing(&previous));
        assert!(pane.back.is_empty());
        assert_eq!(pane.path, "/a");
    }

    #[test]
    fn klasorler_once_gelir_siralama_ve_arama_calisir() {
        let mut pane = Pane::default();
        pane.fill(listing("/a"));
        let names = |p: &Pane| {
            p.visible()
                .iter()
                .map(|e| e.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&pane), ["Klasör", "a.txt", "b.txt"]);
        pane.sort_by(SortKey::Size);
        assert_eq!(names(&pane), ["Klasör", "b.txt", "a.txt"]);
        pane.sort_by(SortKey::Size);
        assert_eq!(names(&pane), ["Klasör", "a.txt", "b.txt"]);
        pane.sort_by(SortKey::Modified);
        assert_eq!(names(&pane), ["Klasör", "a.txt", "b.txt"]);
        pane.search = Some("B.T".into());
        assert_eq!(names(&pane), ["b.txt"]);
    }

    #[test]
    fn coklu_secim_ctrl_ve_shift_ile_calisir() {
        let mut pane = Pane::default();
        pane.fill(listing("/a"));
        let order: Vec<String> = pane.visible().iter().map(|e| e.name.clone()).collect();
        pane.select("Klasör", &order, false, false);
        pane.select("b.txt", &order, false, true);
        assert_eq!(pane.selected.len(), 3);
        assert_eq!(pane.selected_items(), ["b.txt", "Klasör", "a.txt"]);
        pane.select("a.txt", &order, true, false);
        assert!(!pane.selected.contains("a.txt"));
        pane.select("a.txt", &order, false, false);
        assert_eq!(pane.selected.iter().collect::<Vec<_>>(), ["a.txt"]);
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    fn fixture(page: Page, theme: u8) -> App {
        ui_fixture(page, theme)
    }

    fn connected_pc(id: &str, name: &str, session: u64) -> core::Link {
        core::Link {
            id: id.into(),
            name: name.into(),
            session,
            ip: format!("10.0.0.{session}"),
            side: Side::Right,
            parent: String::new(),
            control: Control::Local,
            prefs: Some(1),
            sending: [false; 2],
            screen_out: false,
            version: Some("test".into()),
        }
    }

    #[test]
    fn connecting_a_different_pc_restores_its_settings_layout_and_current_name() {
        let mut app = fixture(Page::Share, 1);
        let sh = Shared::new(false);
        app.shared = Some(sh.clone());
        app.cfg.paired = vec![
            ("A".into(), "Old A".into(), "secret-a".into()),
            ("B".into(), "Old B".into(), "secret-b".into()),
        ];
        app.cfg.place("A", Side::Right, "");
        app.cfg.place("B", Side::Right, "");
        app.cfg.activate_peer("A");
        app.cfg.relay_latency = 150;
        app.cfg.relay_in = true;
        app.cfg.activate_peer("B");
        app.cfg.relay_latency = 50;
        for (id, name, session, latency, relay) in [
            ("A", "Current A", 1, 150, true),
            ("B", "Current B", 2, 50, false),
            ("A", "Renamed A", 3, 150, true),
        ] {
            let link = connected_pc(id, name, session);
            *sh.links.lock().unwrap() = vec![link.clone()];
            app.popup_device = Some("old-offline-id".into());
            app.sync_connected_peer();
            assert_eq!(app.cfg.active_peer, id);
            assert_eq!(app.cfg.relay_latency, latency);
            assert_eq!(sh.latency.load(Ordering::Relaxed), latency);
            assert_eq!(app.cfg.relay_in, relay);
            assert_eq!(app.popup_link().unwrap().name, name);
            assert!(app.popup_device.is_none());
            let nodes = app.layout_nodes(&[link], true, Palette::new(true));
            assert_eq!(
                nodes.len(),
                1,
                "saved offline PCs must not hide the connected PC"
            );
            assert_eq!(nodes[0].id, id);
            assert_eq!(nodes[0].name, name);
            assert_eq!(nodes[0].cell, (1, 0));
            assert_eq!(
                app.cfg.paired.iter().find(|(i, ..)| i == id).unwrap().1,
                name
            );
        }
        sh.links.lock().unwrap().clear();
        app.sync_connected_peer();
        assert!(app.cfg.active_peer.is_empty());
        assert!(app.layout_nodes(&[], true, Palette::new(true)).is_empty());
        assert!(app.cfg.side_of("A").is_some() && app.cfg.side_of("B").is_some());
    }

    #[test]
    fn saved_and_live_layout_collisions_display_every_pc_in_an_adjacent_cell() {
        let mut app = fixture(Page::Share, 1);
        let mut links = Vec::new();
        for session in 1..=6 {
            let id = format!("id{session}");
            app.cfg.place(&id, Side::Right, "");
            links.push(connected_pc(&id, &format!("PC {session}"), session));
        }
        for running in [false, true] {
            let nodes = app.layout_nodes(
                if running { &links } else { &[] },
                running,
                Palette::new(true),
            );
            assert_eq!(nodes.len(), 6);
            for (index, node) in nodes.iter().enumerate() {
                assert_eq!(node.cell, (index as i32 + 1, 0));
                assert_eq!(
                    node.parent,
                    if index == 0 {
                        String::new()
                    } else {
                        format!("id{index}")
                    }
                );
                assert_eq!(app.cfg.parent_of(&node.id), Some(""));
            }
        }
    }

    #[test]
    fn performance_icons_expose_measurements_and_respect_category_selection() {
        let mut app = fixture(Page::Share, 1);
        app.cfg.show_metrics = true;
        let sh = Shared::new(false);
        let mut sample = crate::metrics::Sample::default();
        sample.values[crate::metrics::CPU] = Some(2300);
        *sh.metrics_local.lock().unwrap() = Some(crate::metrics::TimedSample {
            at: Instant::now(),
            sample,
        });
        app.shared = Some(sh);
        for mask in [crate::metrics::ALL, 1] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            apply_style(&ctx, 1);
            app.cfg.metrics_mask = mask;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(220.0, 150.0),
                )),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(190.0, 115.0));
                app.performance_row(ui, &ui.painter().clone(), Palette::of(ui), rect, None);
            });
            let update = output.platform_output.accesskit_update.take().unwrap();
            assert!(update.nodes.iter().any(|(_, n)| {
                n.value()
                    .or(n.label())
                    .is_some_and(|l| l.contains("İşlemci kullanımı: %23"))
            }));
            assert_eq!(
                update.nodes.iter().any(|(_, n)| n
                    .value()
                    .or(n.label())
                    .is_some_and(|l| l.starts_with("RAM:"))),
                mask == crate::metrics::ALL
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn automatic_kvm_waits_for_permissions() {
        let mut app = fixture(Page::Share, 1);
        app.perms.0 = false;
        app.restart_pending = true;
        app.start_pending_kvm();
        assert!(app.shared.is_none());
        assert!(app.restart_pending);
        assert!(!app.pending_kvm_ready());
        app.perms.0 = true;
        assert!(app.pending_kvm_ready());
    }

    #[test]
    fn modal_close_icon_follows_platform_convention_and_is_accessible() {
        for macos in [false, true] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            apply_style(&ctx, 1);
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(640.0, 160.0),
                )),
                ..Default::default()
            };
            let mut close_rect = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(input, |ui| {
                ui.set_width(540.0);
                close_rect = modal_header(ui, "Geliştirenler", macos).rect;
            });
            assert_eq!(
                close_rect.center().x < 270.0,
                macos,
                "macOS kapatma ikonu solda, diğer platformlarda sağda olmalı"
            );
            let update = output
                .platform_output
                .accesskit_update
                .take()
                .expect("AccessKit ağacı oluşturulmalı");
            assert!(
                update
                    .nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some("Kapat")),
                "kapatma ikonunun erişilebilir adı olmalı"
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn pages_fit_the_minimum_window_in_both_themes() {
        for theme in [1, 2] {
            for page in Page::ALL {
                let ctx = egui::Context::default();
                apply_style(&ctx, theme);
                let mut app = fixture(page, theme);
                let width = 732.0;
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, 760.0),
                    )),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    ui.set_max_width(width);
                    match page {
                        Page::Share => app.page_share(ui, &State::Stopped, false),
                        Page::Audio => app.page_audio(ui),
                        Page::Files => app.page_files(ui, &State::Stopped, false),
                        Page::Usb => app.page_usb(ui, &State::Stopped),
                        Page::Settings => app.page_settings(ui, &ctx),
                        Page::Log => app.page_log(ui),
                    }
                    assert!(
                        ui.min_rect().width() <= width + 1.0,
                        "{} exceeds the minimum content width: {}",
                        page.title(),
                        ui.min_rect().width()
                    );
                });
                output.textures_delta.clear();
            }
        }
    }

    #[test]
    fn settings_categories_fit_the_minimum_window_in_both_themes() {
        for theme in [1, 2] {
            for settings_page in SettingsPage::ALL {
                let ctx = egui::Context::default();
                apply_style(&ctx, theme);
                let mut app = fixture(Page::Settings, theme);
                app.settings_page = settings_page;
                let width = 732.0;
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, 760.0),
                    )),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    ui.set_max_width(width);
                    app.page_settings(ui, &ctx);
                    assert!(
                        ui.min_rect().width() <= width + 1.0,
                        "{} exceeds the minimum content width: {}",
                        settings_page.title(),
                        ui.min_rect().width()
                    );
                });
                output.textures_delta.clear();
            }
        }
    }
}
