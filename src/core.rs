use crate::audio;
use crate::config::{Config, Side};
use crate::discover;
use crate::flows::{self, Pref};
use crate::hardware;
use crate::net::{self, Frame, Secure, read_frame, write_frame};
use crate::platform::{self, Ev, Injector, ev};
use crate::screen;
use crate::usb;
use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
pub enum Control {
    Local,
    Remote,
    Controlled,
}

#[derive(Clone, PartialEq)]
pub enum State {
    Stopped,
    Waiting,
    Connected { peer: String, control: Control },
}

#[derive(Clone)]
pub struct Link {
    pub session: u64,
    pub id: String,
    pub name: String,
    pub ip: String,
    pub side: Side,
    pub parent: String,
    pub control: Control,
    pub prefs: Option<u16>,
    pub sending: [bool; 2],
    pub screen_out: bool,
    pub version: Option<String>,
}

impl Link {
    pub fn key(&self) -> String {
        if self.id.is_empty() {
            format!("#{}", self.session)
        } else {
            self.id.clone()
        }
    }
}

#[derive(Clone, Default)]
pub struct DeviceInfo {
    pub caps: Vec<hardware::Cap>,
    pub status: String,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Feat {
    Audio,
    Mic,
    Screen,
    Usb,
    Files,
}

#[derive(Default, Clone)]
pub struct Policy {
    pub require: bool,
    pub trusted: HashSet<String>,
    pub blocked: HashSet<String>,
}

impl Policy {
    pub fn from_cfg(c: &Config) -> Self {
        Self {
            require: c.require_approval,
            trusted: c
                .trusted
                .iter()
                .map(|(i, _)| i.clone())
                .chain(c.paired.iter().map(|(i, _, _)| i.clone()))
                .collect(),
            blocked: c
                .blocked
                .iter()
                .map(|(i, _)| i.clone())
                .chain(c.held.iter().cloned())
                .collect(),
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum Decision {
    Accept,
    Reject,
    Block,
}

#[derive(Clone)]
pub struct Request {
    pub id: u64,
    pub sas: Option<String>,
    pub dev_id: String,
    pub name: String,
    pub ip: String,
    pub decision: Option<Decision>,
}

#[derive(Clone)]
pub struct PairTarget {
    pub ip: String,
    pub port: String,
    pub name: String,
}

#[derive(Clone)]
pub struct PairStatus {
    pub name: String,
    pub sas: Option<String>,
    pub state: String,
    pub done: bool,
    pub failed: bool,
}

#[derive(Clone, Copy)]
pub struct Perms {
    pub control_out: bool,
    pub control_in: bool,
    pub audio_out: bool,
    pub mic_out: bool,
    pub files_in: bool,
    pub clip_in: bool,
    pub screen_out: bool,
    pub screen_control: bool,
    pub usb_out: bool,
}

impl Default for Perms {
    fn default() -> Self {
        Self {
            control_out: true,
            control_in: true,
            audio_out: true,
            mic_out: false,
            files_in: false,
            clip_in: true,
            screen_out: false,
            screen_control: false,
            usb_out: false,
        }
    }
}

impl Perms {
    pub fn from_cfg(c: &Config) -> Self {
        Self {
            control_out: c.allow_control_out,
            control_in: c.allow_control_in,
            audio_out: c.allow_audio_out,
            mic_out: c.allow_mic_out,
            files_in: c.allow_files_in,
            clip_in: c.allow_clip_in,
            screen_out: c.allow_screen_out,
            screen_control: c.allow_screen_control,
            usb_out: c.allow_usb_out,
        }
    }
    pub fn with_bits(mut self, bits: u16) -> Self {
        let on = |p: Pref| bits & p.bit() != 0;
        self.control_in = on(Pref::ControlIn);
        self.audio_out = on(Pref::AudioOut);
        self.mic_out = on(Pref::MicOut);
        self.files_in = on(Pref::FilesIn);
        self.clip_in = on(Pref::ClipIn);
        self.control_out = on(Pref::ControlOut);
        self.screen_out = bits & flows::SCREEN_OUT != 0;
        self
    }

    pub fn flags(&self) -> u32 {
        (self.control_in as u32)
            | (self.audio_out as u32) << 1
            | (self.mic_out as u32) << 2
            | (self.files_in as u32) << 3
            | (self.clip_in as u32) << 4
    }
}

pub struct Shared {
    #[cfg(test)]
    pub test_transfer: crate::test_support::TransferSettings,
    pub stop: AtomicBool,
    pub done: AtomicBool,
    pub state: Mutex<State>,
    pub log: Mutex<Vec<String>>,
    pub want: [AtomicBool; 2],
    pub sending: [AtomicBool; 2],
    pub relay_last: [Arc<Mutex<Option<Instant>>>; 2],
    pub src: Mutex<[String; 2]>,
    pub latency: AtomicU32,
    pub peer_ip: Mutex<Option<String>>,
    pub transfers: Mutex<Vec<crate::transfer::Transfer>>,
    pub next_id: AtomicU64,
    pub carry_files: AtomicBool,
    pub pairs: Mutex<HashMap<String, String>>,
    pub global_secret: Mutex<String>,
    pub peer_secrets: Mutex<HashMap<String, String>>,
    pub new_pairs: Mutex<Vec<(String, String, String)>>,
    pub pair_target: Mutex<Option<PairTarget>>,
    pub pairing: Mutex<Option<PairStatus>>,
    pub policy: Mutex<Policy>,
    pub perms: Mutex<Perms>,
    pub requests: Mutex<Vec<Request>>,
    pub learned: Mutex<Vec<(String, String)>>,
    pub share_text: AtomicBool,
    pub metrics_share: AtomicBool,
    pub metrics_show: AtomicBool,
    pub metrics_local: Mutex<Option<crate::metrics::TimedSample>>,
    pub metrics_peer: Mutex<HashMap<String, crate::metrics::TimedSample>>,
    pub last_text: Mutex<String>,
    pub last_carry: Mutex<Vec<std::path::PathBuf>>,
    pub peer_prefs: Mutex<Option<u16>>,
    pub out_prefs: Mutex<Vec<(String, Pref, bool)>>,
    pub in_prefs: Mutex<Vec<(Pref, bool)>>,
    pub recent: Mutex<(Instant, Vec<std::path::PathBuf>)>,
    pub screen_want: AtomicBool,
    pub screen_sending: AtomicBool,
    pub screen: Arc<Mutex<screen::View>>,
    pub screen_opts: Mutex<screen::Opts>,
    pub screen_peer: Mutex<(bool, Vec<String>)>,
    pub usb_allowed: Mutex<Vec<String>>,
    pub usb_remote: Mutex<Option<Vec<usb::Dev>>>,
    pub usb_attached: Mutex<Vec<usb::Dev>>,
    pub usb_status: Mutex<HashMap<String, String>>,
    pub usb_cmds: Mutex<Vec<(u8, String)>>,
    pub usb_exported: Mutex<Vec<String>>,
    pub peer_version: Mutex<Option<String>>,
    pub links: Mutex<Vec<Link>>,
    pub remote_prefs: AtomicBool,
    pub devices: Mutex<HashMap<String, DeviceInfo>>,
    pub device_cmds: Mutex<Vec<(String, u8, Vec<u8>)>>,
    pub device_perms: Mutex<HashMap<String, u16>>,
    targets: Mutex<[String; 5]>,
    usb_holder: AtomicU64,
    owner: AtomicU64,
    primary: AtomicU64,
    cursor: Mutex<(u64, i32, i32)>,
    conns: Mutex<Vec<TcpStream>>,
    echo: bool,
}

impl Shared {
    pub fn new(echo: bool) -> Arc<Self> {
        Arc::new(Self {
            #[cfg(test)]
            test_transfer: crate::test_support::TransferSettings::default(),
            stop: AtomicBool::new(false),
            done: AtomicBool::new(false),
            state: Mutex::new(State::Stopped),
            log: Mutex::new(Vec::new()),
            want: [AtomicBool::new(false), AtomicBool::new(false)],
            sending: [AtomicBool::new(false), AtomicBool::new(false)],
            relay_last: [Arc::default(), Arc::default()],
            src: Mutex::default(),
            latency: AtomicU32::new(80),
            peer_ip: Mutex::new(None),
            transfers: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            carry_files: AtomicBool::new(true),
            pairs: Mutex::new(HashMap::new()),
            global_secret: Mutex::new(String::new()),
            peer_secrets: Mutex::new(HashMap::new()),
            new_pairs: Mutex::new(Vec::new()),
            pair_target: Mutex::new(None),
            pairing: Mutex::new(None),
            policy: Mutex::new(Policy::default()),
            perms: Mutex::new(Perms::default()),
            requests: Mutex::new(Vec::new()),
            learned: Mutex::new(Vec::new()),
            share_text: AtomicBool::new(true),
            metrics_share: AtomicBool::new(false),
            metrics_show: AtomicBool::new(false),
            metrics_local: Mutex::default(),
            metrics_peer: Mutex::default(),
            last_text: Mutex::new(String::new()),
            last_carry: Mutex::new(Vec::new()),
            peer_prefs: Mutex::new(None),
            out_prefs: Mutex::new(Vec::new()),
            in_prefs: Mutex::new(Vec::new()),
            recent: Mutex::new((Instant::now(), Vec::new())),
            screen_want: AtomicBool::new(false),
            screen_sending: AtomicBool::new(false),
            screen: Arc::default(),
            screen_opts: Mutex::new(screen::Opts::default()),
            screen_peer: Mutex::default(),
            usb_allowed: Mutex::default(),
            usb_remote: Mutex::default(),
            usb_attached: Mutex::default(),
            usb_status: Mutex::default(),
            usb_cmds: Mutex::default(),
            usb_exported: Mutex::default(),
            peer_version: Mutex::default(),
            links: Mutex::default(),
            remote_prefs: AtomicBool::new(true),
            devices: Mutex::default(),
            device_cmds: Mutex::default(),
            device_perms: Mutex::default(),
            targets: Mutex::default(),
            usb_holder: AtomicU64::new(0),
            owner: AtomicU64::new(0),
            primary: AtomicU64::new(0),
            cursor: Mutex::default(),
            conns: Mutex::new(Vec::new()),
            echo,
        })
    }

    pub fn perms_for(&self, id: &str) -> Perms {
        let p = *self.perms.lock().unwrap();
        match self.device_perms.lock().unwrap().get(id) {
            Some(bits) => p.with_bits(*bits),
            None => p,
        }
    }

    pub fn perms_for_ip(&self, ip: &str) -> Perms {
        let id = self
            .links
            .lock()
            .unwrap()
            .iter()
            .find(|l| l.ip == ip)
            .map(|l| l.id.clone())
            .unwrap_or_default();
        self.perms_for(&id)
    }

    fn shares_text(&self, id: &str) -> bool {
        match self.device_perms.lock().unwrap().get(id) {
            Some(bits) => bits & Pref::ShareText.bit() != 0,
            None => self.share_text.load(Ordering::Relaxed),
        }
    }

    fn prefs_with(&self, p: Perms, share_text: bool) -> u16 {
        let ld = Ordering::Relaxed;
        [
            (Pref::ControlIn, p.control_in),
            (Pref::AudioOut, p.audio_out),
            (Pref::MicOut, p.mic_out),
            (Pref::FilesIn, p.files_in),
            (Pref::ClipIn, p.clip_in),
            (Pref::ControlOut, p.control_out),
            (Pref::RelayIn, self.want[0].load(ld)),
            (Pref::MicIn, self.want[1].load(ld)),
            (Pref::ShareText, share_text),
        ]
        .into_iter()
        .filter(|(_, on)| *on)
        .fold(0, |b, (pref, _)| b | pref.bit())
    }

    fn device_base(&self, id: &str) -> u16 {
        self.prefs_with(self.perms_for(id), self.shares_text(id))
            | if self.perms_for(id).screen_out {
                flows::SCREEN_OUT
            } else {
                0
            }
    }

    fn device_off(&self, id: &str, pref: Pref) -> bool {
        if matches!(pref, Pref::RelayIn | Pref::MicIn) || id.is_empty() {
            return self.apply_remote_pref(pref, false);
        }
        let base = self.device_base(id);
        let mut map = self.device_perms.lock().unwrap();
        let bits = map.get(id).copied().unwrap_or(base);
        map.insert(id.to_string(), bits & !pref.bit());
        bits & pref.bit() != 0
    }

    fn device_on(&self, id: &str, pref: Pref) -> bool {
        if matches!(pref, Pref::RelayIn | Pref::MicIn) || id.is_empty() {
            if !id.is_empty() {
                let feat = if pref == Pref::RelayIn {
                    Feat::Audio
                } else {
                    Feat::Mic
                };
                self.set_target(feat, id);
            }
            self.apply_pref(pref, true);
            self.in_prefs.lock().unwrap().push((pref, true));
            return true;
        }
        let base = self.device_base(id);
        let mut map = self.device_perms.lock().unwrap();
        let bits = map.get(id).copied().unwrap_or(base);
        map.insert(id.to_string(), bits | pref.bit());
        bits & pref.bit() == 0
    }

    fn remote_set(&self, id: &str, pref: Pref, on: bool) -> bool {
        if !on {
            return self.device_off(id, pref);
        }
        self.remote_prefs.load(Ordering::Relaxed) && self.device_on(id, pref)
    }

    pub fn apply_pref(&self, pref: Pref, on: bool) {
        let ld = Ordering::Relaxed;
        let mut p = self.perms.lock().unwrap();
        match pref {
            Pref::ControlIn => p.control_in = on,
            Pref::AudioOut => p.audio_out = on,
            Pref::MicOut => p.mic_out = on,
            Pref::FilesIn => p.files_in = on,
            Pref::ClipIn => p.clip_in = on,
            Pref::ControlOut => p.control_out = on,
            Pref::RelayIn => self.want[0].store(on, ld),
            Pref::MicIn => self.want[1].store(on, ld),
            Pref::ShareText => self.share_text.store(on, ld),
        }
    }

    fn apply_remote_pref(&self, pref: Pref, on: bool) -> bool {
        if on {
            return false;
        }
        self.apply_pref(pref, false);
        self.in_prefs.lock().unwrap().push((pref, false));
        true
    }

    pub fn screen_note(&self, s: &str) {
        let mut v = self.screen.lock().unwrap();
        v.frame = None;
        v.note = Some(s.into());
        drop(v);
        self.log(s);
    }

    pub fn log(&self, s: impl Into<String>) {
        let s = s.into();
        if self.echo {
            eprintln!("{s}");
        }
        let mut l = self.log.lock().unwrap();
        l.push(s);
        let n = l.len();
        if n > 300 {
            l.drain(..n - 300);
        }
    }

    fn set(&self, st: State) {
        *self.state.lock().unwrap() = st;
    }

    fn refresh_state(&self) {
        let links = self.links.lock().unwrap();
        let primary = self.primary.load(Ordering::Relaxed);
        let owner = self.owner.load(Ordering::Relaxed);
        let st = match links
            .iter()
            .find(|l| l.session == primary)
            .or(links.first())
        {
            None => State::Waiting,
            Some(l) => State::Connected {
                peer: l.name.clone(),
                control: links
                    .iter()
                    .find(|o| o.session == owner)
                    .map_or(Control::Local, |o| o.control),
            },
        };
        drop(links);
        self.set(st);
    }

    pub fn target(&self, feat: Feat) -> String {
        self.targets.lock().unwrap()[feat as usize].clone()
    }

    pub fn set_target(&self, feat: Feat, id: &str) {
        self.targets.lock().unwrap()[feat as usize] = id.to_string();
        if feat == Feat::Files {
            let ip = self
                .links
                .lock()
                .unwrap()
                .iter()
                .find(|l| l.id == id)
                .map(|l| l.ip.clone());
            if ip.is_some() {
                *self.peer_ip.lock().unwrap() = ip;
            }
        }
    }

    pub fn serves(&self, feat: Feat, id: &str) -> bool {
        let links = self.links.lock().unwrap();
        let session = links.iter().find(|l| l.id == id).map(|l| l.session);
        drop(links);
        session.is_some_and(|session| self.target_is(feat, id, session))
    }

    fn target_is(&self, feat: Feat, id: &str, session: u64) -> bool {
        let t = self.target(feat);
        let present = !t.is_empty() && self.links.lock().unwrap().iter().any(|l| l.id == t);
        if present {
            t == id
        } else {
            self.primary.load(Ordering::Relaxed) == session
        }
    }

    fn local_prefs_for(&self, id: &str, session: u64) -> u16 {
        let mut bits = self.prefs_with(self.perms_for(id), self.shares_text(id));
        for (feat, pref) in [(Feat::Audio, Pref::RelayIn), (Feat::Mic, Pref::MicIn)] {
            if !self.target_is(feat, id, session) {
                bits &= !pref.bit();
            }
        }
        bits
    }

    fn update_link(&self, session: u64, change: impl FnOnce(&mut Link)) {
        let mut links = self.links.lock().unwrap();
        if let Some(l) = links.iter_mut().find(|l| l.session == session) {
            change(l);
        }
        for i in 0..2 {
            self.sending[i].store(links.iter().any(|l| l.sending[i]), Ordering::Relaxed);
        }
        self.screen_sending
            .store(links.iter().any(|l| l.screen_out), Ordering::Relaxed);
    }

    fn set_control(&self, session: u64, c: Control) {
        if let Some(l) = self
            .links
            .lock()
            .unwrap()
            .iter_mut()
            .find(|l| l.session == session)
        {
            l.control = c;
        }
        if c == Control::Local {
            let _ = self
                .owner
                .compare_exchange(session, 0, Ordering::AcqRel, Ordering::Relaxed);
        }
        self.refresh_state();
    }

    fn claim(&self, session: u64) -> bool {
        match self
            .owner
            .compare_exchange(0, session, Ordering::AcqRel, Ordering::Relaxed)
        {
            Ok(_) => true,
            Err(current) => current == session,
        }
    }

    fn idle_for(&self, session: u64) -> bool {
        let owner = self.owner.load(Ordering::Relaxed);
        owner == 0 || owner == session
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        for c in self.conns.lock().unwrap().drain(..) {
            let _ = c.shutdown(Shutdown::Both);
        }
    }

    pub fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn track(&self, s: &TcpStream) {
        if let Ok(c) = s.try_clone() {
            self.conns.lock().unwrap().push(c);
        }
    }
}

pub fn run(cfg: Config, sh: Arc<Shared>) {
    sh.want[0].store(cfg.relay_in, Ordering::Relaxed);
    sh.want[1].store(cfg.mic_in, Ordering::Relaxed);
    *sh.src.lock().unwrap() = [cfg.relay_src.clone(), cfg.mic_src.clone()];
    sh.latency.store(cfg.relay_latency, Ordering::Relaxed);
    sh.carry_files.store(cfg.carry_files, Ordering::Relaxed);
    sh.share_text.store(cfg.share_text, Ordering::Relaxed);
    sh.metrics_share.store(cfg.share_metrics, Ordering::Relaxed);
    sh.metrics_show.store(cfg.show_metrics, Ordering::Relaxed);
    crate::metrics::start(Arc::downgrade(&sh));
    *sh.policy.lock().unwrap() = Policy::from_cfg(&cfg);
    *sh.perms.lock().unwrap() = Perms::from_cfg(&cfg);
    sh.remote_prefs
        .store(cfg.allow_remote_prefs, Ordering::Relaxed);
    *sh.screen_opts.lock().unwrap() = cfg.screen;
    *sh.usb_allowed.lock().unwrap() = cfg.usb_shared.clone();
    *sh.device_perms.lock().unwrap() = cfg.device_perms.iter().cloned().collect();
    platform::set_excluded(cfg.excluded.clone());
    *sh.pairs.lock().unwrap() = cfg
        .paired
        .iter()
        .map(|(i, _, s)| (i.clone(), s.clone()))
        .collect();
    *sh.global_secret.lock().unwrap() = cfg.secret.clone();
    if let Err(e) = main_loop(&cfg, &sh) {
        sh.log(format!("Hata: {e}"));
    }
    sh.set(State::Stopped);
    sh.log("Durduruldu.");
    sh.done.store(true, Ordering::Relaxed);
}

enum Msg {
    Dev(Vec<Ev>),
    FromPeer(u64, Frame),
    Closed(u64, String),
    Input(Vec<Ev>),
    UsbOut(u64, u8, String),
    Takeover(u64, u16, Option<Side>),
}

pub fn screen_input(evs: Vec<Ev>) {
    let to = INPUT_TO.load(Ordering::Relaxed);
    if let Some((_, tx)) = SLOT.lock().unwrap().iter().find(|(id, _)| *id == to) {
        let _ = tx.send(Msg::Input(evs));
    }
}

static SLOT: Mutex<Vec<(u64, Sender<Msg>)>> = Mutex::new(Vec::new());
static INPUT_TO: AtomicU64 = AtomicU64::new(0);

fn resolve(addr: &str, port: &str) -> io::Result<SocketAddr> {
    format!("{addr}:{port}")
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::other("adres çözümlenemedi"))
}

fn secret_for(sh: &Shared, id: &str) -> Option<String> {
    if let Some(s) = sh.pairs.lock().unwrap().get(id) {
        return Some(s.clone());
    }
    let g = sh.global_secret.lock().unwrap().clone();
    (!g.is_empty()).then_some(g)
}

type Incoming = (Secure, u8, String, String, SocketAddr);

fn main_loop(cfg: &Config, sh: &Arc<Shared>) -> io::Result<()> {
    let inj = Arc::new(Mutex::new(Injector::new().map_err(|e| {
        io::Error::other(format!(
            "uinput açılamadı ({e}). /dev/uinput yazma izni gerekli"
        ))
    })?));
    if let Err(e) = platform::start_capture(on_dev) {
        sh.log(format!(
            "Uyarı: {e}. Bu PC karşıyı kontrol edemez, yalnızca kontrol edilebilir."
        ));
    }
    let l = TcpListener::bind(format!("0.0.0.0:{}", cfg.port))?;
    l.set_nonblocking(true)?;
    discover::announce(sh.clone(), cfg.name.clone(), cfg.port.clone());
    crate::transfer::serve(sh.clone());
    let manual = crate::config::valid_addr(&cfg.addr);
    sh.log(if manual {
        "Karşı PC'ye bağlanılıyor…"
    } else {
        "Eşleşmiş cihazlar aranıyor… (yeni cihaz için Düzen sayfasından bağlantı isteği gönderin)"
    });

    let (itx, irx) = mpsc::channel::<Incoming>();
    let mut session = 0u64;
    let mut warned = false;
    let mut denied: HashMap<String, Instant> = HashMap::new();
    let mut announced: HashSet<String> = HashSet::new();
    let mut last_try = Instant::now() - Duration::from_secs(10);
    let (done_tx, done_rx) = mpsc::channel::<String>();
    let mut workers: Vec<thread::JoinHandle<()>> = Vec::new();
    while !sh.stopping() {
        sh.refresh_state();
        workers.retain(|w| !w.is_finished());
        while let Ok(addr) = done_rx.try_recv() {
            denied.insert(addr, Instant::now() + Duration::from_secs(60));
        }
        match l.accept() {
            Ok((s, from)) => {
                s.set_nonblocking(false).ok();
                s.set_nodelay(true).ok();
                sh.track(&s);
                let (itx, sh2) = (itx.clone(), sh.clone());
                thread::spawn(move || {
                    let lookup = |mode: u8, id: &str| -> Option<String> {
                        if mode == net::MODE_PAIR {
                            Some(net::PAIR_SECRET.to_string())
                        } else {
                            secret_for(&sh2, id)
                        }
                    };
                    match net::accept_hello(s, &lookup) {
                        Ok((sec, mode, id, secret)) => {
                            let _ = itx.send((sec, mode, id, secret, from));
                        }
                        Err(e) => sh2.log(format!("Reddedildi: {} ({e})", from.ip())),
                    }
                });
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e),
        }
        if let Ok((sec, mode, _pid, secret, from)) = irx.try_recv() {
            session += 1;
            let ip = from.ip().to_string();
            if mode == net::MODE_PAIR {
                sh.log(format!("Eşleşme isteği geldi: {ip}"));
                let r = pairing_acceptor(sec, ip, cfg, sh);
                end_session(sh, &inj, r, session);
            } else {
                sh.log(format!("Doğrulanmış bağlantı: {ip}"));
                let start = Start {
                    initiator: false,
                    peer: ip,
                    secret,
                    side: cfg.side,
                    session,
                };
                workers.push(spawn_session(sec, start, cfg, sh, &inj, done_tx.clone()));
            }
            continue;
        }
        let target = sh.pair_target.lock().unwrap().take();
        if let Some(t) = target {
            session += 1;
            let status = |state: &str, sas: Option<String>, done: bool, failed: bool| {
                *sh.pairing.lock().unwrap() = Some(PairStatus {
                    name: t.name.clone(),
                    sas,
                    state: state.into(),
                    done,
                    failed,
                });
            };
            status("bağlanılıyor…", None, false, false);
            let conn = resolve(&t.ip, &t.port)
                .and_then(|a| TcpStream::connect_timeout(&a, Duration::from_secs(3)));
            match conn {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    sh.track(&s);
                    match net::connect_hello(s, &cfg.id, net::MODE_PAIR, net::PAIR_SECRET) {
                        Ok(sec) => {
                            let r = pairing_initiator(sec, t.ip.clone(), t.name.clone(), cfg, sh);
                            if let Err(e) = &r {
                                status(&format!("olmadı: {e}"), None, true, true);
                            }
                            end_session(sh, &inj, r, session);
                        }
                        Err(e) => status(&format!("olmadı: {e}"), None, true, true),
                    }
                }
                Err(e) => status(&format!("{} bulunamadı: {e}", t.ip), None, true, true),
            }
            continue;
        }
        if last_try.elapsed() > Duration::from_secs(2) {
            last_try = Instant::now();
            let linked: Vec<(String, String, Side, String)> = sh
                .links
                .lock()
                .unwrap()
                .iter()
                .map(|l| (l.id.clone(), l.ip.clone(), l.side, l.parent.clone()))
                .collect();
            let mut target: Option<(String, String, String, Side)> = None;
            if manual {
                let a = cfg.addr.trim().to_string();
                let g = sh.global_secret.lock().unwrap().clone();
                if !g.is_empty()
                    && !linked.iter().any(|(_, ip, ..)| *ip == a)
                    && denied.get(&a).is_none_or(|t| Instant::now() > *t)
                {
                    target = Some((a, cfg.port.clone(), g, cfg.side));
                }
            }
            if target.is_none() {
                for (ip, name, port, id) in discover::all() {
                    let side = cfg.side_of(&id).unwrap_or(cfg.side);
                    let parent = cfg.parent_of(&id).unwrap_or("");
                    if discover::self_id() >= id.as_str()
                        || sh.policy.lock().unwrap().blocked.contains(&id)
                        || denied.get(&ip).is_some_and(|t| Instant::now() <= *t)
                        || linked
                            .iter()
                            .any(|(i, a, s, p)| *i == id || *a == ip || (*s == side && p == parent))
                    {
                        continue;
                    }
                    if let Some(sec) = secret_for(sh, &id) {
                        if announced.insert(ip.clone()) {
                            sh.log(format!("Ağda bulundu: {name} ({ip})"));
                        }
                        target = Some((ip, port, sec, side));
                        break;
                    }
                }
            }
            if let Some((addr, port, secret, side)) = target {
                let conn = resolve(&addr, &port)
                    .and_then(|a| TcpStream::connect_timeout(&a, Duration::from_secs(2)));
                match conn {
                    Ok(s) => {
                        warned = false;
                        s.set_nodelay(true).ok();
                        sh.track(&s);
                        session += 1;
                        match net::connect_hello(s, &cfg.id, net::MODE_NORMAL, &secret) {
                            Ok(sec) => {
                                sh.log(format!("{addr} adresine bağlandı (doğrulandı, şifreli)."));
                                let start = Start {
                                    initiator: true,
                                    peer: addr,
                                    secret,
                                    side,
                                    session,
                                };
                                workers.push(spawn_session(
                                    sec,
                                    start,
                                    cfg,
                                    sh,
                                    &inj,
                                    done_tx.clone(),
                                ));
                            }
                            Err(e) => {
                                sh.log(format!("{addr}: {e}"));
                                denied.insert(addr, Instant::now() + Duration::from_secs(20));
                            }
                        }
                    }
                    Err(e) => {
                        if !warned {
                            sh.log(format!("{addr} bulunamadı ({e}); yeniden denenecek…"));
                            warned = true;
                        }
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    for w in workers {
        let _ = w.join();
    }
    Ok(())
}

const ECHO_NOTE: &str = "İki PC aynı anda birbirine sistem sesi gönderirse ses kendini besleyip parazit yapar; bu yüzden PC sesi tek yönde aktarılıyor.";

struct Start {
    initiator: bool,
    peer: String,
    secret: String,
    side: Side,
    session: u64,
}

fn spawn_session(
    sec: Secure,
    start: Start,
    cfg: &Config,
    sh: &Arc<Shared>,
    inj: &Arc<Mutex<Injector>>,
    done: Sender<String>,
) -> thread::JoinHandle<()> {
    let (cfg, sh, inj) = (cfg.clone(), sh.clone(), inj.clone());
    thread::spawn(move || {
        let (initiator, addr, session) = (start.initiator, start.peer.clone(), start.session);
        let r = run_session(sec, start, &cfg, &sh, &inj);
        let denied = r
            .as_ref()
            .is_err_and(|e| e.kind() == io::ErrorKind::PermissionDenied);
        end_session(&sh, &inj, r, session);
        if initiator && denied {
            let _ = done.send(addr);
        }
    })
}

fn parse_ident(i: &str) -> (String, String) {
    let mut it = i.splitn(3, '\n');
    (
        it.next().unwrap_or("").to_string(),
        it.next().unwrap_or("").to_string(),
    )
}

fn parse_flags(i: &str) -> u32 {
    i.splitn(3, '\n')
        .nth(2)
        .and_then(|f| f.trim().parse().ok())
        .unwrap_or(0xFF)
}

fn pairing_initiator(
    mut s: Secure,
    ip: String,
    name: String,
    cfg: &Config,
    sh: &Shared,
) -> io::Result<()> {
    let sas = net::sas(&s.hash);
    let set = |state: &str, done: bool, failed: bool| {
        *sh.pairing.lock().unwrap() = Some(PairStatus {
            name: name.clone(),
            sas: Some(sas.clone()),
            state: state.into(),
            done,
            failed,
        });
    };
    set(
        "Karşı PC'de onay bekleniyor. İki ekrandaki kod aynı olmalı",
        false,
        false,
    );
    write_frame(
        &mut s,
        &Frame::Hello(
            format!("{}\n{}", cfg.id, cfg.name),
            cfg.w as u16,
            cfg.h as u16,
            cfg.side.to_u8(),
        ),
    )?;
    match read_frame(&mut s)? {
        Frame::PairAccept(blob) => {
            let (secret, ident) = blob.split_once('\u{1}').unwrap_or(("", ""));
            let (pid, pname) = parse_ident(ident);
            if secret.len() < 16 || pid.is_empty() {
                set("geçersiz yanıt", true, true);
                return Err(io::Error::other("geçersiz eşleşme yanıtı"));
            }
            store_pair(
                sh,
                &pid,
                &if pname.is_empty() {
                    name.clone()
                } else {
                    pname
                },
                secret,
            );
            sh.log(format!("Eşleşti: {name} ({ip})"));
            set("eşleşti", true, false);
            Ok(())
        }
        Frame::Deny(r) => {
            set(&format!("karşı PC kabul etmedi ({r})"), true, true);
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("eşleşme reddedildi ({r})"),
            ))
        }
        _ => Err(io::Error::other("beklenmeyen yanıt")),
    }
}

fn pairing_acceptor(mut s: Secure, ip: String, cfg: &Config, sh: &Shared) -> io::Result<()> {
    let sas = net::sas(&s.hash);
    let (pid, pname) = match read_frame(&mut s)? {
        Frame::Hello(ident, ..) => parse_ident(&ident),
        _ => return Err(io::Error::other("el sıkışma başarısız")),
    };
    match authorize(sh, &pid, &pname, &ip, Some(&sas)) {
        Ok(()) => {
            let secret = crate::config::pair_secret();
            write_frame(
                &mut s,
                &Frame::PairAccept(format!("{secret}\u{1}{}\n{}", cfg.id, cfg.name)),
            )?;
            store_pair(sh, &pid, &pname, &secret);
            sh.log(format!("Eşleşti: {pname} ({ip})"));
            Ok(())
        }
        Err(reason) => {
            let _ = write_frame(&mut s, &Frame::Deny(reason.clone()));
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("eşleşme {reason}: {pname}"),
            ))
        }
    }
}

fn store_pair(sh: &Shared, id: &str, name: &str, secret: &str) {
    sh.pairs
        .lock()
        .unwrap()
        .insert(id.to_string(), secret.to_string());
    sh.policy.lock().unwrap().trusted.insert(id.to_string());
    sh.new_pairs
        .lock()
        .unwrap()
        .push((id.to_string(), name.to_string(), secret.to_string()));
}

fn authorize(
    sh: &Shared,
    dev_id: &str,
    name: &str,
    ip: &str,
    sas: Option<&str>,
) -> Result<(), String> {
    {
        let p = sh.policy.lock().unwrap();
        if p.blocked.contains(dev_id) {
            return Err("engellendi".into());
        }
        if sas.is_none() && (!p.require || p.trusted.contains(dev_id)) {
            return Ok(());
        }
    }
    let rid = sh.next_id.fetch_add(1, Ordering::Relaxed);
    sh.requests.lock().unwrap().push(Request {
        id: rid,
        sas: sas.map(|s| s.to_string()),
        dev_id: dev_id.to_string(),
        name: name.to_string(),
        ip: ip.to_string(),
        decision: None,
    });
    sh.log(format!(
        "{}: {name} ({ip}). Onayınız bekleniyor",
        if sas.is_some() {
            "Eşleşme isteği"
        } else {
            "Bağlantı isteği"
        }
    ));
    #[cfg(not(test))]
    platform::notify(
        "Mause: bağlantı isteği",
        &match sas {
            Some(c) => format!(
                "{name} ({ip}) eşleşmek istiyor. Kod: {c}. Mause penceresinden karar verin."
            ),
            None => format!("{name} ({ip}) bağlanmak istiyor. Mause penceresinden karar verin."),
        },
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let result = loop {
        let d = sh
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|r| r.id == rid)
            .and_then(|r| r.decision.clone());
        match d {
            Some(Decision::Accept) => break Ok(()),
            Some(Decision::Reject) => break Err("reddedildi".to_string()),
            Some(Decision::Block) => break Err("engellendi".to_string()),
            None => {}
        }
        if sh.stopping() || Instant::now() > deadline {
            break Err("yanıt gelmedi".to_string());
        }
        thread::sleep(Duration::from_millis(200));
    };
    sh.requests.lock().unwrap().retain(|r| r.id != rid);
    result
}

fn send_clip_text(s: &mut impl std::io::Write, share: bool, last: &mut String) -> io::Result<()> {
    if !share {
        return Ok(());
    }
    if let Some(t) = crate::clip::text_snapshot().filter(|t| t != last) {
        *last = t.clone();
        write_frame(s, &Frame::ClipText(t))?;
    }
    Ok(())
}

fn end_session(sh: &Shared, inj: &Mutex<Injector>, r: io::Result<()>, session: u64) {
    if let Err(e) = r {
        crate::connection::record(format!("session={session} ended ({:?}): {e}", e.kind()));
        if !sh.stopping() {
            sh.log(format!("Oturum bitti: {e}"));
        }
    }
    SLOT.lock().unwrap().retain(|(id, _)| *id != session);
    let _ = INPUT_TO.compare_exchange(session, 0, Ordering::AcqRel, Ordering::Relaxed);
    let gone: Vec<Link> = {
        let mut links = sh.links.lock().unwrap();
        let gone = links
            .iter()
            .filter(|l| l.session == session)
            .cloned()
            .collect();
        links.retain(|l| l.session != session);
        gone
    };
    for l in &gone {
        sh.metrics_peer.lock().unwrap().remove(&l.id);
        sh.devices.lock().unwrap().remove(&l.id);
        sh.peer_secrets.lock().unwrap().remove(&l.ip);
        let mut ip = sh.peer_ip.lock().unwrap();
        if ip.as_deref() == Some(l.ip.as_str()) {
            *ip = None;
        }
    }
    sh.update_link(0, |_| {});
    if sh
        .usb_holder
        .compare_exchange(session, 0, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        clear_usb(sh);
    }
    if sh
        .owner
        .compare_exchange(session, 0, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        platform::grab(false);
        inj.lock().unwrap().release_all();
    }
    if sh
        .primary
        .compare_exchange(session, 0, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        clear_primary(sh);
    }
    if sh.links.lock().unwrap().is_empty() {
        platform::grab(false);
        inj.lock().unwrap().release_all();
        sh.conns.lock().unwrap().clear();
    }
    sh.refresh_state();
}

fn clear_primary(sh: &Shared) {
    *sh.peer_prefs.lock().unwrap() = None;
    *sh.peer_version.lock().unwrap() = None;
}

fn clear_usb(sh: &Shared) {
    *sh.usb_remote.lock().unwrap() = None;
    sh.usb_status.lock().unwrap().clear();
    let attached: Vec<usb::Dev> = sh.usb_attached.lock().unwrap().drain(..).collect();
    let exported: Vec<String> = sh.usb_exported.lock().unwrap().drain(..).collect();
    if !attached.is_empty() || !exported.is_empty() {
        thread::spawn(move || {
            for d in attached {
                let _ = usb::detach(&d.busid);
            }
            for b in exported {
                let _ = usb::unbind(&b);
            }
        });
    }
}

fn beyond(x: i32, y: i32, w: i32, h: i32) -> Option<Side> {
    if x < 0 {
        Some(Side::Left)
    } else if x >= w {
        Some(Side::Right)
    } else if y < 0 {
        Some(Side::Top)
    } else if y >= h {
        Some(Side::Bottom)
    } else {
        None
    }
}

/// Gerçek imleç, geçiş yapılacak kenara dayanmış mı (2 piksel tolerans).
fn at_edge(x: i32, y: i32, edge: Side, w: i32, h: i32) -> bool {
    match edge {
        Side::Left => x <= 1,
        Side::Right => x >= w - 2,
        Side::Top => y <= 1,
        Side::Bottom => y >= h - 2,
    }
}

fn edge_along(x: i32, y: i32, edge: Side, w: i32, h: i32) -> u16 {
    if matches!(edge, Side::Left | Side::Right) {
        (y.clamp(0, h - 1) * 65535 / h) as u16
    } else {
        (x.clamp(0, w - 1) * 65535 / w) as u16
    }
}

fn edge_point(pos: u16, edge: Side, inset: i32, w: i32, h: i32) -> (i32, i32) {
    let ay = (pos as i32 * h / 65535).clamp(0, h - 1);
    let ax = (pos as i32 * w / 65535).clamp(0, w - 1);
    match edge {
        Side::Left => (inset, ay),
        Side::Right => (w - 1 - inset, ay),
        Side::Top => (ax, inset),
        Side::Bottom => (ax, h - 1 - inset),
    }
}

fn hand_off(sh: &Shared, from: u64, to: u64, pos: u16, push: Option<Side>) -> bool {
    let slot = SLOT.lock().unwrap();
    let Some((_, tx)) = slot.iter().find(|(id, _)| *id == to) else {
        return false;
    };
    if sh
        .owner
        .compare_exchange(from, to, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return false;
    }
    tx.send(Msg::Takeover(to, pos, push)).is_ok()
}

fn updated_peer_flags(flags: u32, preferences: u16) -> u32 {
    (flags & !0x1F) | (preferences as u32 & 0x1F) | flows::SUPPORTS_PREFS
}

fn not_for_us(f: &Frame, screen_mine: bool, usb_mine: bool) -> bool {
    match f {
        Frame::Usb(..) => !usb_mine,
        Frame::ScreenInfo(..) | Frame::Screen(screen::DENIED | screen::FAILED) => !screen_mine,
        _ => false,
    }
}

fn run_session(
    mut s: Secure,
    start: Start,
    cfg: &Config,
    sh: &Arc<Shared>,
    inj: &Mutex<Injector>,
) -> io::Result<()> {
    let Start {
        initiator,
        peer,
        secret,
        side,
        session,
    } = start;
    let (w, h) = (cfg.w, cfg.h);

    let my_ident = format!(
        "{}\n{}\n{}",
        cfg.id,
        cfg.name,
        sh.perms.lock().unwrap().flags()
            | flows::SUPPORTS_PREFS
            | flows::SUPPORTS_SCREEN
            | flows::SUPPORTS_SCREEN2
            | flows::SUPPORTS_USB
            | flows::SUPPORTS_VERSION
            | flows::SUPPORTS_HARDWARE
            | flows::SUPPORTS_METRICS
            | flows::SUPPORTS_USB2
    );
    let parse = parse_ident;
    let (eff, peer, mut pflags, pid, far_home, (pw, ph)) = if initiator {
        write_frame(
            &mut s,
            &Frame::Hello(my_ident, w as u16, h as u16, side.to_u8()),
        )?;
        match read_frame(&mut s)? {
            Frame::Size(pw, ph, ident) => {
                let (pid, pname) = parse(&ident);
                if sh.policy.lock().unwrap().blocked.contains(&pid) {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "bu cihaz engellenmiş",
                    ));
                }
                if !pid.is_empty() {
                    sh.policy.lock().unwrap().trusted.insert(pid.clone());
                    sh.learned
                        .lock()
                        .unwrap()
                        .push((pid.clone(), pname.clone()));
                }
                (
                    side,
                    if pname.is_empty() { peer } else { pname },
                    parse_flags(&ident),
                    pid,
                    side.opposite(),
                    (pw as i32, ph as i32),
                )
            }
            Frame::Deny(r) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("karşı PC bağlantıyı kabul etmedi ({r})"),
                ));
            }
            _ => return Err(io::Error::other("el sıkışma başarısız")),
        }
    } else {
        match read_frame(&mut s)? {
            Frame::Hello(ident, pw, ph, their_side) => {
                let (pid, pname) = parse(&ident);
                match authorize(sh, &pid, &pname, &peer, None) {
                    Ok(()) => write_frame(&mut s, &Frame::Size(w as u16, h as u16, my_ident))?,
                    Err(reason) => {
                        let _ = write_frame(&mut s, &Frame::Deny(reason.clone()));
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            format!("{reason}: {pname}"),
                        ));
                    }
                }
                (
                    cfg.side_of(&pid)
                        .unwrap_or(Side::from_u8(their_side).opposite()),
                    if pname.is_empty() { peer } else { pname },
                    parse_flags(&ident),
                    pid,
                    Side::from_u8(their_side),
                    (pw as i32, ph as i32),
                )
            }
            _ => return Err(io::Error::other("el sıkışma başarısız")),
        }
    };
    let (pw, ph) = (pw.max(1), ph.max(1));
    let parent = cfg.parent_of(&pid).unwrap_or("").to_string();
    let hash = s.hash;
    let screen_ok = pflags & flows::SUPPORTS_SCREEN != 0 && pflags != 0xFF;
    let screen2_ok = pflags & flows::SUPPORTS_SCREEN2 != 0 && pflags != 0xFF;
    let usb_ok = pflags & flows::SUPPORTS_USB != 0 && pflags != 0xFF;
    let hardware_ok = pflags & flows::SUPPORTS_HARDWARE != 0 && pflags != 0xFF;
    let primary = sh
        .primary
        .compare_exchange(0, session, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok();
    let knows_version = pflags & flows::SUPPORTS_VERSION != 0 && pflags != 0xFF;
    if knows_version {
        if primary {
            *sh.peer_version.lock().unwrap() = None;
        }
        write_frame(&mut s, &Frame::Version(crate::update::VERSION.into()))?;
    } else if primary {
        *sh.peer_version.lock().unwrap() = Some(String::new());
    }
    sh.log(format!(
        "{peer} bağlandı. Ekranın {} tarafında. Acil dönüş: Ctrl+Alt+Home",
        eff.tr()
    ));
    let peer_ip = s.peer_addr()?.ip().to_string();
    sh.links.lock().unwrap().push(Link {
        session,
        id: pid.clone(),
        name: peer.clone(),
        ip: peer_ip.clone(),
        side: eff,
        parent: parent.clone(),
        control: Control::Local,
        prefs: None,
        sending: [false; 2],
        screen_out: false,
        version: (!knows_version).then(String::new),
    });
    let set = |c: Control| sh.set_control(session, c);
    set(Control::Local);

    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = mpsc::channel();
    SLOT.lock().unwrap().push((session, tx.clone()));
    let usb_tx = tx.clone();
    let closed = tx.clone();
    let mut s = crate::connection::start(
        s,
        move |frame| tx.send(Msg::FromPeer(session, frame)).is_ok(),
        move |reason| {
            crate::connection::record(format!("session={session} transport: {reason}"));
            let _ = closed.send(Msg::Closed(session, reason));
        },
    )?;
    crate::connection::record(format!(
        "session={session} peer={peer} ip={peer_ip} connected; build={}",
        env!("CARGO_PKG_VERSION")
    ));

    sh.peer_secrets
        .lock()
        .unwrap()
        .insert(peer_ip.clone(), secret.clone());
    let mut recv: [Option<audio::Handle>; 2] = [None, None];
    let mut send: [Option<audio::Handle>; 2] = [None, None];
    let mut sent = [false; 2];
    let (mut _screen_rx, mut screen_tx): (Option<screen::Handle>, Option<screen::Sharing>) =
        (None, None);
    let mut screen_sent = false;
    let mut opts_sent: Option<screen::Opts> = None;
    let mut info_sent = String::new();
    let mut usb_server: Option<usb::Tunnel> = None;
    let mut audio_retry: [Option<Instant>; 2] = [None, None];
    // Ses isteği gönderildiği halde paket gelmiyorsa (istek karşıda henüz izin/
    // cihaz hazır değilken reddedildi, gönderici açılamadı vb.) isteği yineler.
    let mut audio_asked: [Option<Instant>; 2] = [None, None];
    let mut echo_warned = false;
    let mut screen_retry: Option<Instant> = None;
    let mut usb_client: Option<usb::Tunnel> = None;
    let metrics_ok = pflags & flows::SUPPORTS_METRICS != 0;
    let mut metrics_requested = false;
    let mut metrics_subscribed = None;
    let mut metrics_sent = Instant::now() - Duration::from_secs(2);
    let hit = |nx: i32, ny: i32| beyond(nx, ny, w, h) == Some(eff);
    let along = |x: i32, y: i32| edge_along(x, y, eff, w, h);
    let place = |pos: u16, inset: i32| edge_point(pos, eff, inset, w, h);
    let flash = |edge: Side, x: i32, y: i32| {
        platform::edge_flash(x, y, matches!(edge, Side::Left | Side::Right), w, h)
    };
    let child_on = |edge: Side| -> Option<u64> {
        sh.links
            .lock()
            .unwrap()
            .iter()
            .find(|l| l.parent == pid && l.side == edge && !pid.is_empty())
            .map(|l| l.session)
    };
    let parent_session = || -> Option<u64> {
        sh.links
            .lock()
            .unwrap()
            .iter()
            .find(|l| l.id == parent && !parent.is_empty())
            .map(|l| l.session)
    };
    let mut mirror = (pw / 2, ph / 2);
    let mut clip_sent = sh.last_text.lock().unwrap().clone();
    let (mut x, mut y) = (w / 2, h / 2);
    let mut epoch = 0u64;
    let mut mode = Control::Local;
    let mut down: HashSet<u16> = HashSet::new();
    let (ctrl, alt, home) = (ev::KEY_LEFTCTRL, ev::KEY_LEFTALT, ev::KEY_HOME);
    let (mut moved, mut moved_at) = (0i32, Instant::now());
    let mut gain = crate::pointer::Gain::new();
    let local_scale = platform::ui_scale();
    gain.set_local_scale(local_scale);
    let mut incoming = crate::pointer::Incoming::new(local_scale);
    let mut pending: Option<Msg> = None;

    let mut last_ping = Instant::now();
    let prefs_ok = pflags & flows::SUPPORTS_PREFS != 0 && pflags != 0xFF;
    if hardware_ok {
        write_frame(&mut s, &Frame::Device(hardware::LIST, Vec::new()))?;
    }
    let mut prefs_sent: Option<u16> = None;
    let mut screen_control_allowed = sh.perms_for(&pid).screen_control;
    while !sh.stopping() {
        let screen_mine = sh.target_is(Feat::Screen, &pid, session);
        let usb_mine = usb_ok && sh.target_is(Feat::Usb, &pid, session);
        let files_mine = sh.target_is(Feat::Files, &pid, session);
        {
            let mut ip = sh.peer_ip.lock().unwrap();
            if files_mine && ip.as_deref() != Some(peer_ip.as_str()) {
                *ip = Some(peer_ip.clone());
            } else if !files_mine && ip.as_deref() == Some(peer_ip.as_str()) {
                *ip = None;
            }
        }
        if usb_mine && usb_server.is_none() {
            let port = if pflags & flows::SUPPORTS_USB2 != 0 {
                usb::PORT
            } else {
                usb::LEGACY_PORT
            };
            usb_server = usb::serve(peer_ip.clone(), secret.clone(), port).ok();
            if usb_server.is_some() {
                sh.usb_holder.store(session, Ordering::Relaxed);
            }
        } else if !usb_mine && usb_server.take().is_some() {
            usb_client = None;
            if sh
                .usb_holder
                .compare_exchange(session, 0, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                clear_usb(sh);
            }
        }
        let mine_out = |sh: &Shared| -> Vec<(Pref, bool)> {
            let mut out = sh.out_prefs.lock().unwrap();
            let (here, rest): (Vec<_>, Vec<_>) = out
                .drain(..)
                .partition(|(id, ..)| *id == pid || (id.is_empty() && primary));
            *out = rest;
            here.into_iter().map(|(_, p, on)| (p, on)).collect()
        };
        if prefs_ok {
            let bits = sh.local_prefs_for(&pid, session);
            if prefs_sent != Some(bits) {
                write_frame(&mut s, &Frame::Prefs(bits))?;
                prefs_sent = Some(bits);
            }
            for (pref, on) in mine_out(sh) {
                write_frame(&mut s, &Frame::SetPref(pref as u8, on))?;
            }
        } else {
            mine_out(sh);
        }
        if hardware_ok {
            let mine: Vec<(u8, Vec<u8>)> = {
                let mut queue = sh.device_cmds.lock().unwrap();
                let (here, rest): (Vec<_>, Vec<_>) =
                    queue.drain(..).partition(|(id, ..)| *id == pid);
                *queue = rest;
                here.into_iter().map(|(_, op, data)| (op, data)).collect()
            };
            for (op, data) in mine {
                write_frame(&mut s, &Frame::Device(op, data))?;
            }
        }
        if last_ping.elapsed() >= Duration::from_secs(1) {
            if metrics_ok {
                let show = sh.metrics_show.load(Ordering::Relaxed);
                if metrics_subscribed != Some(show) {
                    write_frame(
                        &mut s,
                        &Frame::Device(hardware::METRICS_SUBSCRIBE, vec![show as u8]),
                    )?;
                    metrics_subscribed = Some(show);
                    if !show {
                        sh.metrics_peer.lock().unwrap().remove(&pid);
                    }
                }
                if metrics_requested && metrics_sent.elapsed() >= Duration::from_secs(2) {
                    let data = crate::metrics::payload(
                        sh.metrics_share.load(Ordering::Relaxed),
                        sh.metrics_local.lock().unwrap().as_ref(),
                    );
                    write_frame(&mut s, &Frame::Device(hardware::METRICS_SAMPLE, data))?;
                    metrics_sent = Instant::now();
                }
            }
            last_ping = Instant::now();
        }
        let local_perms = sh.perms_for(&pid);
        for (i, allowed) in [local_perms.audio_out, local_perms.mic_out]
            .into_iter()
            .enumerate()
        {
            if !allowed && send[i].take().is_some() {
                sh.update_link(session, |l| l.sending[i] = false);
            }
        }
        if mode == Control::Controlled && !local_perms.control_in {
            inj.lock().unwrap().release_all();
            mode = Control::Local;
            set(mode);
            write_frame(&mut s, &Frame::Reclaim)?;
        }
        if !local_perms.screen_out && screen_tx.take().is_some() {
            inj.lock().unwrap().release_all();
            write_frame(&mut s, &Frame::Screen(screen::DENIED))?;
            sh.log("Ekran paylaşımı izni kapatıldı; karşı PC'nin izlemesi durduruldu.");
        }
        if screen_control_allowed && !local_perms.screen_control && screen_tx.is_some() {
            inj.lock().unwrap().release_all();
        }
        screen_control_allowed = local_perms.screen_control;
        for i in 0..2 {
            let feat = if i == 0 { Feat::Audio } else { Feat::Mic };
            let want = sh.target_is(feat, &pid, session) && sh.want[i].load(Ordering::Relaxed);
            if want && !sent[i] && audio_retry[i].is_some_and(|t| Instant::now() < t) {
                continue;
            }
            if want != sent[i] {
                if want && pflags & (2 << i) == 0 {
                    sent[i] = want;
                    sh.log(if i == 0 {
                        "Karşı PC sesini paylaşmaya izin vermiyor."
                    } else {
                        "Karşı PC mikrofonunu paylaşmaya izin vermiyor."
                    });
                    continue;
                }
                recv[i] = None;
                if want {
                    recv[i] = audio::start_receiver(
                        i as u8,
                        &peer_ip,
                        sh.latency.load(Ordering::Relaxed),
                        sh.relay_last[i].clone(),
                        net::audio_key(&hash, i as u8, !initiator),
                    );
                    if recv[i].is_none() {
                        if audio_retry[i].is_none() {
                            sh.log(if cfg!(target_os = "linux") {
                                "Ses alıcısı başlatılamadı (pacat/pactl gerekli ya da port kullanımda); yeniden denenecek."
                            } else {
                                "Ses alıcısı başlatılamadı (ses çıkışı açılamadı ya da port kullanımda); yeniden denenecek."
                            });
                        }
                        audio_retry[i] = Some(Instant::now() + Duration::from_millis(500));
                        continue;
                    }
                    audio_retry[i] = None;
                }
                if want && i == 0 && send[0].is_some() {
                    // İki PC birbirine aynı anda sistem sesini gönderirse her biri
                    // diğerinin sesini yeniden yakalayıp geri yollar: kendini besleyen
                    // uğultu/parazit oluşur. Tek yön seçilir: bağlantıyı başlatan PC
                    // öncelikli; diğeri karşı onu dinlerken istek göndermez.
                    if !initiator {
                        if !echo_warned {
                            echo_warned = true;
                            sh.log(ECHO_NOTE);
                        }
                        continue;
                    }
                    send[0] = None;
                    sh.update_link(session, |l| l.sending[0] = false);
                    sh.log(ECHO_NOTE);
                }
                write_frame(&mut s, &Frame::AudioReq(i as u8, want))?;
                sent[i] = want;
                audio_asked[i] = want.then(Instant::now);
            } else if want
                && sent[i]
                && pflags & (2 << i) != 0
                && audio_asked[i].is_some_and(|t| t.elapsed() >= Duration::from_secs(3))
                && sh.relay_last[i]
                    .lock()
                    .unwrap()
                    .is_none_or(|t| t.elapsed() >= Duration::from_secs(3))
            {
                write_frame(&mut s, &Frame::AudioReq(i as u8, true))?;
                audio_asked[i] = Some(Instant::now());
            }
        }
        let want = screen_mine && sh.screen_want.load(Ordering::Relaxed);
        let waiting = want && !screen_sent && screen_retry.is_some_and(|t| Instant::now() < t);
        if want != screen_sent && !waiting {
            screen_sent = want;
            _screen_rx = None;
            let _ = INPUT_TO.compare_exchange(session, 0, Ordering::AcqRel, Ordering::Relaxed);
            if !want {
                if screen_ok {
                    write_frame(&mut s, &Frame::Screen(screen::STOP))?;
                }
            } else if !screen_ok {
                sh.screen_note("Karşı PC'deki Mause sürümü ekran paylaşımını desteklemiyor; onu da güncelleyin.");
            } else {
                _screen_rx =
                    screen::start_receiver(peer_ip.clone(), secret.clone(), sh.screen.clone());
                if _screen_rx.is_some() {
                    screen_retry = None;
                    INPUT_TO.store(session, Ordering::Relaxed);
                    *sh.screen_peer.lock().unwrap() = (false, Vec::new());
                    if screen2_ok {
                        let o = *sh.screen_opts.lock().unwrap();
                        write_frame(&mut s, &Frame::ScreenOpts(o.encode()))?;
                        opts_sent = Some(o);
                    } else {
                        write_frame(&mut s, &Frame::Screen(screen::START))?;
                    }
                } else {
                    if screen_retry.is_none() {
                        sh.screen_note(
                            "Ekran alıcısı başlatılamadı (24806 portu kullanımda olabilir); yeniden denenecek.",
                        );
                    }
                    screen_sent = false;
                    screen_retry = Some(Instant::now() + Duration::from_millis(500));
                }
            }
        }
        if screen_sent && screen2_ok && _screen_rx.is_some() {
            let o = *sh.screen_opts.lock().unwrap();
            if opts_sent != Some(o) {
                write_frame(&mut s, &Frame::ScreenOpts(o.encode()))?;
                opts_sent = Some(o);
            }
        }
        if screen_tx.is_some() && !sh.perms_for(&pid).screen_out {
            screen_tx = None;
            inj.lock().unwrap().release_all();
            sh.log("Ekran paylaşımı ayarlardan kapatıldı.");
        }
        if screen_tx.is_some() && screen2_ok {
            let info = screen::info(sh.perms_for(&pid).screen_control);
            if info != info_sent {
                write_frame(&mut s, &Frame::ScreenInfo(info.clone()))?;
                info_sent = info;
            }
        } else {
            info_sent.clear();
        }
        let cmds: Vec<(u8, String)> = if usb_mine {
            sh.usb_cmds.lock().unwrap().drain(..).collect()
        } else {
            Vec::new()
        };
        for (op, busid) in cmds {
            if !usb_ok {
                sh.log(
                    "Karşı PC'deki Mause sürümü USB paylaşımını desteklemiyor; onu da güncelleyin.",
                );
                continue;
            }
            match op {
                usb::ATTACH => {
                    sh.usb_status
                        .lock()
                        .unwrap()
                        .insert(busid.clone(), "karşı PC cihazı hazırlıyor…".into());
                    write_frame(&mut s, &Frame::Usb(usb::ATTACH, busid))?;
                }
                usb::DETACH => {
                    sh.usb_status
                        .lock()
                        .unwrap()
                        .insert(busid.clone(), "çıkarılıyor…".into());
                    let (sh2, tx2) = (sh.clone(), usb_tx.clone());
                    thread::spawn(move || {
                        let r = usb::detach(&busid);
                        let mut st = sh2.usb_status.lock().unwrap();
                        match r {
                            Ok(()) => {
                                st.remove(&busid);
                                sh2.usb_attached
                                    .lock()
                                    .unwrap()
                                    .retain(|d| d.busid != busid);
                                let _ = tx2.send(Msg::UsbOut(session, usb::DETACH, busid));
                            }
                            Err(e) => {
                                st.insert(busid, format!("çıkarılamadı: {e}"));
                            }
                        }
                    });
                }
                _ => write_frame(&mut s, &Frame::Usb(usb::LIST_REQ, String::new()))?,
            }
        }
        let sharing = screen_tx.is_some();
        sh.update_link(session, |l| l.screen_out = sharing);
        if mode == Control::Remote && !sh.perms_for(&pid).control_out {
            write_frame(&mut s, &Frame::Release)?;
            mode = Control::Local;
            platform::grab(false);
            inj.lock().unwrap().release_keys(&down)?;
            sh.log("Kontrol bu PC'ye alındı; karşıya geçiş kapalı.");
            set(mode);
        }
        let msg = match pending.take() {
            Some(m) => m,
            None => match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(m) => m,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => return Err(io::Error::other("kanal kapandı")),
            },
        };
        let owned = sh.owner.load(Ordering::Relaxed) == session;
        if !owned {
            let c = *sh.cursor.lock().unwrap();
            if c.0 != epoch {
                (epoch, x, y) = c;
            }
        }
        match msg {
            Msg::Closed(id, reason) if id == session => {
                return Err(io::Error::other(reason));
            }
            Msg::UsbOut(id, op, text) if id == session => {
                write_frame(&mut s, &Frame::Usb(op, text))?
            }
            Msg::Input(evs) if screen2_ok && _screen_rx.is_some() => {
                write_frame(&mut s, &Frame::ScreenInput(evs))?
            }
            Msg::FromPeer(id, f) if id == session && not_for_us(&f, screen_mine, usb_mine) => {}
            Msg::FromPeer(id, f) if id == session => match f {
                Frame::Enter(..) if !sh.perms_for(&pid).control_in => {
                    let _ = write_frame(&mut s, &Frame::Reclaim);
                    sh.log("Karşı PC bu PC'yi yönetmek istedi ama ayarlarda kapalı.");
                }
                Frame::Enter(yf, _) => {
                    if sh.claim(session) {
                        mode = Control::Controlled;
                        (x, y) = place(yf, 0);
                        inj.lock().unwrap().warp(x, y, w, h)?;
                        flash(eff, x, y);
                        set(mode);
                    } else {
                        write_frame(&mut s, &Frame::Reclaim)?;
                    }
                }
                Frame::Release if mode == Control::Controlled => {
                    mode = Control::Local;
                    inj.lock().unwrap().release_all();
                    set(mode);
                }
                Frame::Events(mut evs)
                    if mode == Control::Controlled && sh.perms_for(&pid).control_in =>
                {
                    incoming.apply(&mut evs);
                    let mut leave = false;
                    for &(t, c, v) in &evs {
                        if t == ev::EV_REL {
                            if c == ev::REL_X {
                                let nx = x + v;
                                leave |= hit(nx, y);
                                x = nx.clamp(0, w - 1);
                            } else if c == ev::REL_Y {
                                let ny = y + v;
                                leave |= hit(x, ny);
                                y = ny.clamp(0, h - 1);
                            }
                        }
                    }
                    inj.lock().unwrap().emit_pointer(&evs, x, y, w, h)?;
                    if leave {
                        inj.lock().unwrap().release_all();
                        mode = Control::Local;
                        if files_mine {
                            crate::clip::carry(sh.clone());
                        }
                        send_clip_text(&mut s, sh.shares_text(&pid), &mut clip_sent)?;
                        write_frame(&mut s, &Frame::Leave(along(x, y)))?;
                        set(mode);
                    }
                }
                Frame::Leave(yf) if mode == Control::Remote => {
                    mode = Control::Local;
                    let handed =
                        parent_session().is_some_and(|to| hand_off(sh, session, to, yf, Some(eff)));
                    if !handed {
                        platform::grab(false);
                        (x, y) = place(yf, 1);
                        inj.lock().unwrap().warp(x, y, w, h)?;
                        flash(eff, x, y);
                    }
                    set(mode);
                }
                Frame::Reclaim if mode == Control::Remote => {
                    mode = Control::Local;
                    platform::grab(false);
                    set(mode);
                }
                Frame::ClipText(t) if sh.shares_text(&pid) && sh.perms_for(&pid).clip_in => {
                    *sh.last_text.lock().unwrap() = t.clone();
                    clip_sent = t.clone();
                    thread::spawn(move || crate::clip::set_text(&t));
                }
                Frame::Prefs(bits) => {
                    if primary {
                        *sh.peer_prefs.lock().unwrap() = Some(bits);
                    }
                    sh.update_link(session, |l| l.prefs = Some(bits));
                    let old = pflags;
                    pflags = updated_peer_flags(pflags, bits);
                    for i in 0..2 {
                        let (now, before) = (pflags & (2 << i) != 0, old & (2 << i) != 0);
                        if !now && before {
                            recv[i] = None;
                        }
                        if now && !before {
                            sent[i] = false;
                        }
                    }
                }
                Frame::Device(op, data) if hardware_ok => {
                    if metrics_ok && op == hardware::METRICS_SUBSCRIBE {
                        metrics_requested = data.as_slice() == [1];
                        continue;
                    }
                    if metrics_ok && op == hardware::METRICS_SAMPLE {
                        let mut peers = sh.metrics_peer.lock().unwrap();
                        if sh.metrics_show.load(Ordering::Relaxed) {
                            if let Some(sample) = crate::metrics::Sample::decode(&data) {
                                peers.insert(
                                    pid.clone(),
                                    crate::metrics::TimedSample {
                                        at: Instant::now(),
                                        sample,
                                    },
                                );
                            } else {
                                peers.remove(&pid);
                            }
                        } else {
                            peers.remove(&pid);
                        }
                        continue;
                    }
                    let mut devices = sh.devices.lock().unwrap();
                    let entry = devices.entry(pid.clone()).or_default();
                    let text = String::from_utf8_lossy(&data).into_owned();
                    match op {
                        hardware::LIST => entry.caps = hardware::parse_caps(&text),
                        hardware::STATUS => entry.status = text,
                        _ => {}
                    }
                }
                Frame::SetPref(raw, on) => {
                    if let Some(pref) = Pref::from_u8(raw) {
                        if sh.remote_set(&pid, pref, on) {
                            sh.log(format!(
                                "{peer} {} akışını {}.",
                                pref.tr(),
                                if on { "açtı" } else { "kapattı" }
                            ));
                        }
                        let bits = sh.local_prefs_for(&pid, session);
                        write_frame(&mut s, &Frame::Prefs(bits))?;
                        prefs_sent = Some(bits);
                    }
                }
                Frame::Screen(screen::START) if !sh.perms_for(&pid).screen_out => {
                    write_frame(&mut s, &Frame::Screen(screen::DENIED))?;
                    sh.log("Karşı PC ekranınızı izlemek istedi ama ayarlarda kapalı; reddedildi.");
                }
                Frame::ScreenOpts(b) if screen_tx.is_some() => {
                    if let (Some(o), Some(tx)) = (screen::Opts::decode(&b), &screen_tx) {
                        *tx.opts.lock().unwrap() = o;
                        platform::screen_cursor(o.cursor);
                    }
                }
                Frame::ScreenOpts(_) | Frame::Screen(screen::START)
                    if !sh.perms_for(&pid).screen_out =>
                {
                    write_frame(&mut s, &Frame::Screen(screen::DENIED))?;
                    sh.log("Karşı PC ekranınızı izlemek istedi ama ayarlarda kapalı; reddedildi.");
                }
                Frame::ScreenOpts(b) => {
                    let o = screen::Opts::decode(&b).unwrap_or_default();
                    screen_tx = screen::start_sender(&peer_ip, secret.clone(), o);
                    if screen_tx.is_some() {
                        sh.log(if crate::platform::screen_capture_pending() {
                            "Karşı PC ekranınızı izlemek istiyor: açılan pencerede paylaşılacak ekranı seçip onaylayın."
                        } else {
                            "Karşı PC ekranınızı izliyor."
                        });
                    } else {
                        write_frame(&mut s, &Frame::Screen(screen::FAILED))?;
                        sh.log(SCREEN_FAILED_HERE);
                    }
                }
                Frame::Version(v) => {
                    if primary {
                        *sh.peer_version.lock().unwrap() = Some(v.clone());
                    }
                    sh.update_link(session, |l| l.version = Some(v));
                }
                Frame::ScreenInfo(t) => {
                    *sh.screen_peer.lock().unwrap() = screen::parse_info(&t);
                }
                Frame::ScreenInput(evs) => {
                    let allowed = sh.perms_for(&pid).screen_control;
                    if let (Some(tx), true) = (&screen_tx, allowed) {
                        let abs = |code: u16| {
                            evs.iter()
                                .find(|e| e.0 == screen::EV_ABS && e.1 == code)
                                .map(|e| e.2)
                        };
                        if let (Some(nx), Some(ny)) = (abs(0), abs(1)) {
                            if let Some((x, y)) = tx.to_desktop(nx, ny) {
                                inj.lock().unwrap().point(x, y)?;
                            }
                        }
                        let rest: Vec<Ev> =
                            evs.into_iter().filter(|e| e.0 != screen::EV_ABS).collect();
                        if !rest.is_empty() {
                            inj.lock().unwrap().emit(&rest)?;
                        }
                    }
                }
                Frame::Usb(op, text) => handle_usb(
                    op,
                    text,
                    sh,
                    &usb_tx,
                    session,
                    &peer_ip,
                    &secret,
                    if pflags & flows::SUPPORTS_USB2 != 0 {
                        usb::PORT
                    } else {
                        usb::LEGACY_PORT
                    },
                    &mut usb_client,
                    &mut s,
                )?,
                Frame::Screen(screen::START) => {
                    screen_tx =
                        screen::start_sender(&peer_ip, secret.clone(), screen::Opts::default());
                    if screen_tx.is_some() {
                        sh.log(if crate::platform::screen_capture_pending() {
                            "Karşı PC ekranınızı izlemek istiyor: açılan pencerede paylaşılacak ekranı seçip onaylayın."
                        } else {
                            "Karşı PC ekranınızı izliyor."
                        });
                    } else {
                        write_frame(&mut s, &Frame::Screen(screen::FAILED))?;
                        sh.log(SCREEN_FAILED_HERE);
                    }
                }
                Frame::Screen(screen::STOP) => {
                    if screen_tx.take().is_some() {
                        inj.lock().unwrap().release_all();
                        sh.log("Karşı PC ekranınızı izlemeyi bıraktı.");
                    }
                }
                Frame::Screen(screen::DENIED) => {
                    _screen_rx = None;
                    sh.screen_note("Karşı PC ekran paylaşımına izin vermiyor. Karşıda Paylaşım → Bu bilgisayardan karşıya → Ekran açılmalı.");
                }
                Frame::Screen(screen::FAILED) => {
                    _screen_rx = None;
                    sh.screen_note("Karşı PC ekranını yakalayamadı. macOS'ta Ekran Kaydı izni gerekir (karşıda sorulur); Ubuntu'da açılan paylaşım penceresi reddedilmiş olabilir.");
                }
                Frame::AudioReq(kind, true)
                    if kind < 2 && {
                        let p = sh.perms_for(&pid);
                        (kind == 0 && !p.audio_out) || (kind == 1 && !p.mic_out)
                    } =>
                {
                    sh.log("Karşı PC ses/mikrofon istedi ama ayarlarda izin kapalı; reddedildi.");
                }
                Frame::AudioReq(0, true) if initiator && recv[0].is_some() => {
                    if !echo_warned {
                        echo_warned = true;
                        sh.log(ECHO_NOTE);
                    }
                }
                Frame::AudioReq(kind, on) if kind < 2 => {
                    let dev = sh.src.lock().unwrap()[kind as usize].clone();
                    if kind == 0 && on && recv[0].take().is_some() {
                        // Karşı bizi dinlemek istiyor ve öncelik onda: bizim dinlememizi bırak.
                        sent[0] = false;
                        write_frame(&mut s, &Frame::AudioReq(0, false))?;
                        sh.log(ECHO_NOTE);
                    }
                    if !on {
                        echo_warned = false;
                    }
                    let already = on && send[kind as usize].is_some();
                    send[kind as usize] = None;
                    send[kind as usize] = if on {
                        audio::start_sender(
                            kind,
                            &peer_ip,
                            &dev,
                            net::audio_key(&hash, kind, initiator),
                        )
                    } else {
                        None
                    };
                    let on_air = send[kind as usize].is_some();
                    sh.update_link(session, |l| l.sending[kind as usize] = on_air);
                    if on && !already {
                        sh.log(if kind == 0 {
                            "Karşı PC sesinizi istiyor: aktarılıyor."
                        } else {
                            "Karşı PC mikrofonunuzu istiyor: aktarılıyor."
                        });
                    }
                }
                _ => {}
            },
            Msg::Takeover(id, pos, push) if id == session => {
                mode = Control::Remote;
                send_clip_text(&mut s, sh.shares_text(&pid), &mut clip_sent)?;
                write_frame(&mut s, &Frame::Enter(pos, eff.to_u8()))?;
                mirror = edge_point(pos, far_home, 0, pw, ph);
                if let Some(edge) = push {
                    let target = edge_point(pos, edge, 1, pw, ph);
                    write_frame(
                        &mut s,
                        &Frame::Events(vec![
                            (ev::EV_REL, ev::REL_X, target.0 - mirror.0),
                            (ev::EV_REL, ev::REL_Y, target.1 - mirror.1),
                        ]),
                    )?;
                    mirror = target;
                }
                set(mode);
            }
            Msg::Dev(evs) if !sh.idle_for(session) => {
                for &(t, c, v) in &evs {
                    if t == ev::EV_KEY {
                        if v == 1 {
                            down.insert(c);
                        } else if v == 0 {
                            down.remove(&c);
                        }
                    }
                }
            }
            Msg::Dev(evs) => {
                let (mut cross, mut strong, mut mv) = (false, false, 0i32);
                for &(t, c, v) in &evs {
                    if t == ev::EV_KEY {
                        if v == 1 {
                            down.insert(c);
                            strong = true;
                        } else if v == 0 {
                            down.remove(&c);
                        }
                    } else if t == ev::EV_REL && mode != Control::Remote {
                        if c == ev::REL_X {
                            let nx = x + v;
                            cross |= hit(nx, y);
                            x = nx.clamp(0, w - 1);
                            mv += v.abs();
                        } else if c == ev::REL_Y {
                            let ny = y + v;
                            cross |= hit(x, ny);
                            y = ny.clamp(0, h - 1);
                            mv += v.abs();
                        }
                    }
                }
                if mode == Control::Local && mv > 0 {
                    let (dx, dy) = crate::pointer::motion(&evs);
                    gain.observe(dx, dy, platform::cursor_pos);
                }
                if cross && mode == Control::Local {
                    // Konum ham fare hareketinden tahmin edilir; işletim sisteminin
                    // ivmelenmesi yüzünden gerçek imleçten kayabilir ve fare ekranın
                    // ortasındayken bile "kenara geldi" sanılırdı. Gerçek imleç
                    // okunabiliyorsa geçişi ona göre onayla ve tahmini düzelt.
                    if let Some((rx, ry)) = platform::cursor_pos() {
                        if !at_edge(rx, ry, eff, w, h) {
                            cross = false;
                        }
                        x = rx.clamp(0, w - 1);
                        y = ry.clamp(0, h - 1);
                    }
                }
                match mode {
                    Control::Remote => {
                        if down.contains(&ctrl) && down.contains(&alt) && down.contains(&home) {
                            write_frame(&mut s, &Frame::Release)?;
                            mode = Control::Local;
                            platform::grab(false);
                            inj.lock().unwrap().release_keys(&down)?;
                            sh.log("Acil dönüş yapıldı.");
                            set(mode);
                        } else {
                            let mut evs = evs;
                            while let Ok(next) = rx.try_recv() {
                                match next {
                                    Msg::Dev(more) if more.iter().all(|e| e.0 == ev::EV_REL) => {
                                        evs.extend(more)
                                    }
                                    other => {
                                        pending = Some(other);
                                        break;
                                    }
                                }
                            }
                            gain.scale(&mut evs);
                            let mut exit = None;
                            for &(t, c, v) in &evs {
                                if t != ev::EV_REL || (c != ev::REL_X && c != ev::REL_Y) {
                                    continue;
                                }
                                let (nx, ny) = if c == ev::REL_X {
                                    (mirror.0 + v, mirror.1)
                                } else {
                                    (mirror.0, mirror.1 + v)
                                };
                                if exit.is_none() {
                                    exit = beyond(nx, ny, pw, ph)
                                        .filter(|e| *e != far_home)
                                        .and_then(|edge| child_on(edge).map(|to| (to, edge)));
                                }
                                mirror = (nx.clamp(0, pw - 1), ny.clamp(0, ph - 1));
                            }
                            write_frame(&mut s, &Frame::Events(evs))?;
                            if let Some((to, edge)) = exit {
                                let pos = edge_along(mirror.0, mirror.1, edge, pw, ph);
                                if hand_off(sh, session, to, pos, None) {
                                    write_frame(&mut s, &Frame::Release)?;
                                    mode = Control::Local;
                                    set(mode);
                                }
                            }
                        }
                    }
                    Control::Local
                        if cross
                            && sh.perms_for(&pid).control_out
                            && pflags & 1 != 0
                            && sh.claim(session) =>
                    {
                        inj.lock().unwrap().release_keys(&down)?;
                        platform::grab(true);
                        mode = Control::Remote;
                        if files_mine {
                            crate::clip::carry(sh.clone());
                        }
                        send_clip_text(&mut s, sh.shares_text(&pid), &mut clip_sent)?;
                        let pos = along(x, y);
                        write_frame(&mut s, &Frame::Enter(pos, eff.to_u8()))?;
                        mirror = edge_point(pos, far_home, 0, pw, ph);
                        flash(eff, x, y);
                        sh.log(gain.describe());
                        set(mode);
                    }
                    Control::Controlled if strong || mv > 0 => {
                        if moved_at.elapsed() > Duration::from_millis(400) {
                            moved = 0;
                        }
                        moved_at = Instant::now();
                        moved += mv;
                        if strong || moved > 60 {
                            mode = Control::Local;
                            moved = 0;
                            inj.lock().unwrap().release_all();
                            write_frame(&mut s, &Frame::Reclaim)?;
                            set(mode);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        if owned || mode != Control::Local {
            let mut c = sh.cursor.lock().unwrap();
            *c = (c.0 + 1, x, y);
            epoch = c.0;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_usb(
    op: u8,
    text: String,
    sh: &Arc<Shared>,
    tx: &Sender<Msg>,
    session: u64,
    peer_ip: &str,
    secret: &str,
    port: u16,
    client: &mut Option<usb::Tunnel>,
    s: &mut impl std::io::Write,
) -> io::Result<()> {
    match op {
        usb::LIST_REQ => {
            if !sh.perms.lock().unwrap().usb_out {
                write_frame(s, &Frame::Usb(usb::FAIL, "\tKarşı PC USB paylaşımına izin vermiyor (karşıda USB sayfasından açılmalı).".into()))?;
                write_frame(s, &Frame::Usb(usb::LIST, String::new()))?;
                return Ok(());
            }
            let (shared, sender) = (sh.clone(), tx.clone());
            thread::spawn(move || {
                let allowed = shared.usb_allowed.lock().unwrap().clone();
                let permitted = shared.perms.lock().unwrap().usb_out;
                let devs = match usb::list_local() {
                    Ok(v) => v
                        .into_iter()
                        .filter(|d| permitted && allowed.contains(&d.id))
                        .collect(),
                    Err(e) => {
                        let _ = sender.send(Msg::UsbOut(
                            session,
                            usb::FAIL,
                            format!("\tKarşı PC: {e}"),
                        ));
                        Vec::new()
                    }
                };
                let _ = sender.send(Msg::UsbOut(session, usb::LIST, usb::encode_list(&devs)));
            });
        }
        usb::LIST => *sh.usb_remote.lock().unwrap() = Some(usb::decode_list(&text)),
        usb::ATTACH => {
            if !sh.perms.lock().unwrap().usb_out || !usb::valid_busid(&text) {
                write_frame(
                    s,
                    &Frame::Usb(usb::FAIL, format!("{text}\tbu cihaz paylaşıma açık değil")),
                )?;
                return Ok(());
            }
            let (sh, tx) = (sh.clone(), tx.clone());
            thread::spawn(move || {
                let dev = usb::list_local()
                    .ok()
                    .and_then(|v| v.into_iter().find(|d| d.busid == text));
                let permitted = sh.perms.lock().unwrap().usb_out;
                if !permitted
                    || !dev
                        .as_ref()
                        .is_some_and(|d| sh.usb_allowed.lock().unwrap().contains(&d.id))
                {
                    let _ = tx.send(Msg::UsbOut(
                        session,
                        usb::FAIL,
                        format!("{text}\tbu cihaz paylaşıma açık değil veya çıkarılmış"),
                    ));
                    return;
                }
                let name = dev.map(|d| d.name).unwrap_or_default();
                let (op, msg) = match usb::bind(&text) {
                    Ok(()) => {
                        if !sh
                            .links
                            .lock()
                            .unwrap()
                            .iter()
                            .any(|l| l.session == session)
                        {
                            let _ = usb::unbind(&text);
                            return;
                        }
                        sh.usb_exported.lock().unwrap().push(text.clone());
                        sh.log(format!(
                            "USB cihazı paylaşım için hazır: {name}; karşı PC'nin takması bekleniyor."
                        ));
                        (usb::READY, text)
                    }
                    Err(e) => {
                        sh.log(format!("USB cihazı verilemedi: {e}"));
                        (usb::FAIL, format!("{text}\t{e}"))
                    }
                };
                let _ = tx.send(Msg::UsbOut(session, op, msg));
            });
        }
        usb::READY if usb::valid_busid(&text) => {
            if client.is_none() {
                match usb::connect(peer_ip.to_owned(), secret.to_owned(), port) {
                    Ok(tunnel) => *client = Some(tunnel),
                    Err(e) => {
                        sh.usb_status
                            .lock()
                            .unwrap()
                            .insert(text.clone(), format!("USB tüneli açılamadı: {e}"));
                        sh.log(format!(
                            "USB tüneli açılamadı: {e}; PC bağlantısı korunuyor."
                        ));
                        write_frame(s, &Frame::Usb(usb::DETACH, text))?;
                        return Ok(());
                    }
                }
            }
            let local = client.as_ref().map(|c| c.local).unwrap();
            let dev = sh
                .usb_remote
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|v| v.iter().find(|d| d.busid == text).cloned())
                .unwrap_or(usb::Dev {
                    busid: text.clone(),
                    id: String::new(),
                    name: format!("USB {text}"),
                });
            sh.usb_status
                .lock()
                .unwrap()
                .insert(text.clone(), "bu bilgisayara takılıyor…".into());
            let (sh, tx) = (sh.clone(), tx.clone());
            thread::spawn(move || match usb::attach(local, &text) {
                Ok(()) => {
                    if !sh
                        .links
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|l| l.session == session)
                    {
                        let _ = usb::detach(&text);
                        return;
                    }
                    sh.usb_status.lock().unwrap().remove(&text);
                    sh.log(format!("USB cihazı bu bilgisayara takıldı: {}", dev.name));
                    sh.usb_attached.lock().unwrap().push(dev);
                    let _ = tx.send(Msg::UsbOut(session, usb::ATTACHED, text));
                }
                Err(e) => {
                    sh.usb_status
                        .lock()
                        .unwrap()
                        .insert(text.clone(), format!("takılamadı: {e}"));
                    sh.log(format!("USB cihazı takılamadı: {e}"));
                    let _ = tx.send(Msg::UsbOut(session, usb::DETACH, text));
                }
            });
        }
        usb::ATTACHED if sh.usb_exported.lock().unwrap().contains(&text) => {
            sh.log(format!("USB cihazı karşı PC'ye takıldı: {text}."));
        }
        usb::FAIL => {
            let (busid, msg) = text.split_once('\t').unwrap_or(("", &text));
            if !busid.is_empty() {
                sh.usb_status
                    .lock()
                    .unwrap()
                    .insert(busid.to_owned(), msg.to_owned());
            }
            sh.log(format!("USB: {msg}"));
        }
        usb::DETACH if sh.usb_exported.lock().unwrap().contains(&text) => {
            sh.usb_exported.lock().unwrap().retain(|b| *b != text);
            let sh = sh.clone();
            thread::spawn(move || match usb::unbind(&text) {
                Ok(()) => sh.log("USB cihazı karşıdan çıkarıldı, bu PC'de yeniden kullanılabilir."),
                Err(e) => sh.log(format!("USB cihazı geri alınamadı: {e}")),
            });
        }
        _ => {}
    }
    Ok(())
}

#[cfg(target_os = "macos")]
const SCREEN_FAILED_HERE: &str = "Karşı PC ekranınızı izlemek istedi ama ekran yakalanamadı: Sistem Ayarları → Gizlilik ve Güvenlik → Ekran Kaydı'nda Mause'a izin verin.";
#[cfg(target_os = "linux")]
const SCREEN_FAILED_HERE: &str = "Karşı PC ekranınızı izlemek istedi ama ekran yakalanamadı.";
#[cfg(target_os = "windows")]
const SCREEN_FAILED_HERE: &str = "Karşı PC ekranınızı izlemek istedi ama ekran yakalanamadı.";

fn on_dev(evs: Vec<Ev>) {
    for (_, tx) in SLOT.lock().unwrap().iter() {
        let _ = tx.send(Msg::Dev(evs.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_permissions_preserves_usb_and_metrics_capabilities() {
        let flags = flows::SUPPORTS_USB2 | flows::SUPPORTS_METRICS | flows::SUPPORTS_SCREEN2 | 31;
        let updated = updated_peer_flags(flags, 0);
        assert_eq!(updated & 31, 0);
        assert_ne!(updated & flows::SUPPORTS_USB2, 0);
        assert_ne!(updated & flows::SUPPORTS_METRICS, 0);
        assert_ne!(updated & flows::SUPPORTS_SCREEN2, 0);
    }

    #[test]
    fn usb_tunnel_failure_does_not_close_the_control_channel() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let reader = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut secure = net::handshake(tcp, false, "usb-control-test").unwrap();
            assert!(
                matches!(read_frame(&mut secure).unwrap(), Frame::Usb(usb::DETACH, b) if b == "2-1")
            );
            assert!(matches!(read_frame(&mut secure).unwrap(), Frame::Ping));
        });
        let mut secure =
            net::handshake(TcpStream::connect(addr).unwrap(), true, "usb-control-test").unwrap();
        let shared = Shared::new(false);
        let (tx, _) = mpsc::channel();
        let mut client = None;
        handle_usb(
            usb::READY,
            "2-1".into(),
            &shared,
            &tx,
            1,
            "invalid-ip",
            "usb-control-test",
            usb::PORT,
            &mut client,
            &mut secure,
        )
        .unwrap();
        assert!(client.is_none());
        assert!(shared.usb_status.lock().unwrap()["2-1"].contains("tüneli"));
        write_frame(&mut secure, &Frame::Ping).unwrap();
        reader.join().unwrap();
    }

    #[test]
    fn remote_preferences_never_enable_local_permissions() {
        let sh = Shared::new(false);
        for pref in Pref::ALL {
            sh.apply_pref(pref, false);
            assert!(!sh.apply_remote_pref(pref, true));
            assert_eq!(sh.local_prefs_for("", 0) & pref.bit(), 0, "{pref:?}");
        }
        assert!(sh.in_prefs.lock().unwrap().is_empty());
        for pref in Pref::ALL {
            sh.apply_pref(pref, true);
            assert!(sh.apply_remote_pref(pref, false));
            assert_eq!(sh.local_prefs_for("", 0) & pref.bit(), 0);
        }
        assert_eq!(sh.in_prefs.lock().unwrap().len(), Pref::ALL.len());
    }

    fn link(sh: &Shared, session: u64, name: &str, side: Side) {
        sh.links.lock().unwrap().push(Link {
            session,
            id: format!("id{session}"),
            name: name.into(),
            ip: format!("10.0.0.{session}"),
            side,
            parent: String::new(),
            control: Control::Local,
            prefs: None,
            sending: [false; 2],
            screen_out: false,
            version: None,
        });
    }

    #[test]
    fn ayni_anda_tek_oturum_kontrolu_alir() {
        let sh = Shared::new(false);
        link(&sh, 1, "A", Side::Right);
        link(&sh, 2, "B", Side::Left);
        sh.primary.store(1, Ordering::Relaxed);
        assert!(sh.idle_for(1) && sh.idle_for(2));
        assert!(sh.claim(2));
        sh.set_control(2, Control::Remote);
        assert!(!sh.claim(1), "B kontroldeyken A alamamalı");
        assert!(!sh.idle_for(1) && sh.idle_for(2));
        assert!(matches!(
            &*sh.state.lock().unwrap(),
            State::Connected { peer, control: Control::Remote } if peer == "A"
        ));
        sh.set_control(2, Control::Local);
        assert!(sh.idle_for(1));
        assert!(sh.claim(1));
        sh.set_control(1, Control::Controlled);
        assert!(matches!(
            &*sh.state.lock().unwrap(),
            State::Connected {
                control: Control::Controlled,
                ..
            }
        ));
    }

    #[test]
    fn her_ozellik_secilen_cihaza_gider() {
        let sh = Shared::new(false);
        link(&sh, 1, "A", Side::Right);
        link(&sh, 2, "B", Side::Left);
        sh.primary.store(1, Ordering::Relaxed);
        assert!(sh.serves(Feat::Audio, "id1") && !sh.serves(Feat::Audio, "id2"));
        sh.set_target(Feat::Audio, "id2");
        sh.set_target(Feat::Screen, "id1");
        assert!(sh.serves(Feat::Audio, "id2") && !sh.serves(Feat::Audio, "id1"));
        assert!(sh.serves(Feat::Screen, "id1") && !sh.serves(Feat::Screen, "id2"));
        sh.apply_pref(Pref::RelayIn, true);
        assert_eq!(sh.local_prefs_for("id1", 1) & Pref::RelayIn.bit(), 0);
        assert_ne!(sh.local_prefs_for("id2", 2) & Pref::RelayIn.bit(), 0);
        sh.set_target(Feat::Audio, "yok");
        assert!(
            sh.serves(Feat::Audio, "id1"),
            "seçili cihaz yoksa ilk cihaza düşer"
        );
    }

    #[test]
    fn karsi_tarafin_ac_istegi_ayara_baglidir() {
        let sh = Shared::new(false);
        link(&sh, 1, "A", Side::Right);
        assert!(sh.remote_set("id1", Pref::ControlOut, false));
        assert!(!sh.perms_for("id1").control_out);
        assert!(sh.remote_set("id1", Pref::ControlOut, true));
        assert!(sh.perms_for("id1").control_out);
        assert!(!sh.remote_set("id1", Pref::ControlOut, true), "değişmedi");
        sh.remote_prefs.store(false, Ordering::Relaxed);
        assert!(!sh.remote_set("id1", Pref::FilesIn, false));
        assert!(!sh.remote_set("id1", Pref::FilesIn, true));
        assert!(!sh.perms_for("id1").files_in);
        sh.remote_prefs.store(true, Ordering::Relaxed);
        assert!(sh.remote_set("id1", Pref::FilesIn, true));
        assert!(sh.perms_for("id1").files_in);
        assert!(sh.remote_set("id1", Pref::RelayIn, true));
        assert!(sh.want[0].load(Ordering::Relaxed));
        assert_eq!(sh.target(Feat::Audio), "id1");
        assert!(sh.in_prefs.lock().unwrap().contains(&(Pref::RelayIn, true)));
    }

    #[test]
    fn izinler_cihaz_basina_uygulanir() {
        let sh = Shared::new(false);
        link(&sh, 1, "A", Side::Right);
        link(&sh, 2, "B", Side::Left);
        sh.primary.store(1, Ordering::Relaxed);
        assert!(sh.perms_for("id1").control_out && sh.perms_for("id2").control_out);
        assert!(sh.device_off("id2", Pref::ControlOut));
        assert!(sh.perms_for("id1").control_out, "A etkilenmemeli");
        assert!(!sh.perms_for("id2").control_out);
        assert!(!sh.perms_for_ip("10.0.0.2").control_out);
        assert_eq!(sh.local_prefs_for("id2", 2) & Pref::ControlOut.bit(), 0);
        assert_ne!(sh.local_prefs_for("id1", 1) & Pref::ControlOut.bit(), 0);
        sh.device_perms
            .lock()
            .unwrap()
            .insert("id1".into(), flows::SCREEN_OUT);
        let a = sh.perms_for("id1");
        assert!(a.screen_out && !a.control_in && !a.audio_out);
        assert!(!sh.shares_text("id1") && sh.shares_text("id2"));
    }

    #[test]
    fn baglanti_kalmayinca_bekleme_durumuna_doner() {
        let sh = Shared::new(false);
        link(&sh, 7, "A", Side::Top);
        sh.refresh_state();
        assert!(matches!(&*sh.state.lock().unwrap(), State::Connected { peer, .. } if peer == "A"));
        sh.links.lock().unwrap().clear();
        sh.refresh_state();
        assert!(*sh.state.lock().unwrap() == State::Waiting);
    }

    #[test]
    fn microphone_is_not_shared_by_default() {
        assert!(!Config::default().allow_mic_out);
        assert!(!Perms::default().mic_out);
    }

    fn shared(require: bool) -> Arc<Shared> {
        let sh = Shared::new(false);
        sh.policy.lock().unwrap().require = require;
        sh
    }

    fn decide(sh: &Arc<Shared>, d: Decision) {
        for _ in 0..50 {
            if let Some(r) = sh.requests.lock().unwrap().first_mut() {
                r.decision = Some(d);
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("istek hiç oluşmadı");
    }

    #[test]
    fn engelli_cihaz_hemen_reddedilir() {
        let sh = shared(true);
        sh.policy.lock().unwrap().blocked.insert("kotu".into());
        assert_eq!(
            authorize(&sh, "kotu", "Kötü", "1.2.3.4", None),
            Err("engellendi".into())
        );
        assert!(
            sh.requests.lock().unwrap().is_empty(),
            "engelli cihaz için istek açılmamalı"
        );
    }

    #[test]
    fn guvenilen_ya_da_onay_kapaliysa_kabul() {
        let sh = shared(true);
        sh.policy.lock().unwrap().trusted.insert("dost".into());
        assert_eq!(authorize(&sh, "dost", "Dost", "1.2.3.4", None), Ok(()));
        assert_eq!(
            authorize(&shared(false), "yeni", "Yeni", "1.2.3.4", None),
            Ok(())
        );
    }

    #[test]
    fn yeni_cihaz_icin_kullanici_karar_verir() {
        for (d, beklenen) in [
            (Decision::Accept, Ok(())),
            (Decision::Reject, Err("reddedildi".to_string())),
            (Decision::Block, Err("engellendi".to_string())),
        ] {
            let sh = shared(true);
            let (sh2, d2) = (sh.clone(), d.clone());
            let t = thread::spawn(move || decide(&sh2, d2));
            assert_eq!(authorize(&sh, "yeni", "Yeni", "1.2.3.4", None), beklenen);
            t.join().unwrap();
            assert!(
                sh.requests.lock().unwrap().is_empty(),
                "karardan sonra istek temizlenmeli"
            );
        }
    }

    #[test]
    fn durdurulunca_bekleyen_istek_sonlanir() {
        let sh = shared(true);
        let sh2 = sh.clone();
        let t = thread::spawn(move || authorize(&sh2, "yeni", "Yeni", "1.2.3.4", None));
        thread::sleep(Duration::from_millis(300));
        sh.request_stop();
        assert_eq!(t.join().unwrap(), Err("yanıt gelmedi".to_string()));
    }

    fn cfg(id: &str, name: &str) -> Config {
        Config {
            id: id.into(),
            name: name.into(),
            w: 1920,
            h: 1080,
            ..Config::default()
        }
    }

    #[test]
    fn eslesme_ortak_parola_uretir_ve_sonra_parolasiz_baglanir() {
        use std::net::{TcpListener, TcpStream};
        let (sh_a, sh_b) = (Shared::new(false), Shared::new(false));
        sh_b.policy.lock().unwrap().require = true;
        let (cfg_a, cfg_b) = (cfg("aaaa0001", "PC-A"), cfg("bbbb0002", "PC-B"));

        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let (shb, cfgb) = (sh_b.clone(), cfg_b.clone());
        let srv = thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let lookup =
                |mode: u8, _: &str| (mode == net::MODE_PAIR).then(|| net::PAIR_SECRET.to_string());
            let (sec, mode, _, _) = net::accept_hello(s, &lookup).unwrap();
            assert_eq!(mode, net::MODE_PAIR);
            pairing_acceptor(sec, "127.0.0.1".into(), &cfgb, &shb)
        });
        let shb2 = sh_b.clone();
        let decider = thread::spawn(move || {
            for _ in 0..60 {
                if let Some(r) = shb2.requests.lock().unwrap().first_mut() {
                    assert!(r.sas.is_some(), "eşleşme isteğinde doğrulama kodu olmalı");
                    r.decision = Some(Decision::Accept);
                    return r.sas.clone().unwrap();
                }
                thread::sleep(Duration::from_millis(50));
            }
            panic!("istek gelmedi");
        });
        let s = TcpStream::connect(addr).unwrap();
        let sec = net::connect_hello(s, &cfg_a.id, net::MODE_PAIR, net::PAIR_SECRET).unwrap();
        pairing_initiator(sec, "127.0.0.1".into(), "PC-B".into(), &cfg_a, &sh_a).unwrap();
        srv.join().unwrap().unwrap();
        let sas_b = decider.join().unwrap();
        let sas_a = sh_a.pairing.lock().unwrap().clone().unwrap().sas.unwrap();
        assert_eq!(sas_a, sas_b, "iki ekranda aynı doğrulama kodu görünmeli");

        let sec_a = sh_a
            .pairs
            .lock()
            .unwrap()
            .get("bbbb0002")
            .cloned()
            .expect("A, B'nin parolasını almalı");
        let sec_b = sh_b
            .pairs
            .lock()
            .unwrap()
            .get("aaaa0001")
            .cloned()
            .expect("B, A için parola üretmeli");
        assert_eq!(sec_a, sec_b);
        assert!(sec_a.len() >= 32, "ortak parola güçlü olmalı");
        assert!(sh_a.policy.lock().unwrap().trusted.contains("bbbb0002"));

        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let shb3 = sh_b.clone();
        let srv = thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let lookup = |mode: u8, id: &str| {
                if mode == net::MODE_PAIR {
                    None
                } else {
                    secret_for(&shb3, id)
                }
            };
            net::accept_hello(s, &lookup).map(|(_, m, id, _)| (m, id))
        });
        let secret = secret_for(&sh_a, "bbbb0002").unwrap();
        net::connect_hello(
            TcpStream::connect(addr).unwrap(),
            "aaaa0001",
            net::MODE_NORMAL,
            &secret,
        )
        .unwrap();
        assert_eq!(
            srv.join().unwrap().unwrap(),
            (net::MODE_NORMAL, "aaaa0001".to_string())
        );

        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let shb4 = sh_b.clone();
        let srv = thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let lookup = |mode: u8, id: &str| {
                if mode == net::MODE_PAIR {
                    None
                } else {
                    secret_for(&shb4, id)
                }
            };
            net::accept_hello(s, &lookup).is_err()
        });
        let _ = net::connect_hello(
            TcpStream::connect(addr).unwrap(),
            "yabanci99",
            net::MODE_NORMAL,
            "tahmin",
        );
        assert!(srv.join().unwrap(), "eşleşmemiş cihaz reddedilmeli");
    }

    #[test]
    fn izin_bayraklari_karsiya_dogru_iletilir() {
        let mut p = Perms::default();
        assert_eq!(p.flags(), 0b10011);
        p.mic_out = true;
        p.files_in = true;
        assert_eq!(p.flags(), 0b11111);
        p.audio_out = false;
        p.files_in = false;
        assert_eq!(p.flags(), 0b10101);
        let ident = format!("abc123\nPC-A\n{}", p.flags());
        assert_eq!(
            parse_ident(&ident),
            ("abc123".to_string(), "PC-A".to_string())
        );
        assert_eq!(parse_flags(&ident), 0b10101);
        assert_eq!(parse_flags("abc123\nPC-A"), 0xFF);
        let mut c = cfg("x", "y");
        c.allow_control_out = false;
        c.allow_mic_out = false;
        let q = Perms::from_cfg(&c);
        assert!(!q.control_out && q.control_in && !q.mic_out && q.audio_out);
    }

    #[test]
    #[ignore = "hardware: gerçek giriş enjeksiyonu ve masaüstü oturumu gerekir"]
    fn sessiz_kalan_es_baglantiyi_dusurur() {
        use std::net::{TcpListener, TcpStream};
        let sh = Shared::new(false);
        sh.policy.lock().unwrap().require = false;
        let c = cfg("aaaa0001", "PC-A");
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let freezer = thread::spawn(move || {
            let s = TcpStream::connect(addr).unwrap();
            let mut sec =
                net::connect_hello(s, "bbbb0002", net::MODE_NORMAL, "ortak-parola-1234").unwrap();
            write_frame(
                &mut sec,
                &Frame::Hello("bbbb0002\nPC-B\n31".into(), 1920, 1080, 0),
            )
            .unwrap();
            let _ = read_frame(&mut sec).unwrap();
            thread::sleep(Duration::from_secs(20));
        });
        let (s, from) = l.accept().unwrap();
        let lookup = |_: u8, _: &str| Some("ortak-parola-1234".to_string());
        let (sec, ..) = net::accept_hello(s, &lookup).unwrap();
        let inj = Mutex::new(Injector::new().unwrap());
        let t0 = Instant::now();
        let start = Start {
            initiator: false,
            peer: from.ip().to_string(),
            secret: "ortak-parola-1234".into(),
            side: c.side,
            session: 1,
        };
        let r = run_session(sec, start, &c, &sh, &inj);
        let dt = t0.elapsed();
        assert!(r.is_err(), "oturum hata ile bitmeli");
        assert!(
            dt < Duration::from_secs(18),
            "15 sn sessizlikte düşmeli, {dt:?} sürdü"
        );
        assert!(dt >= Duration::from_secs(14), "çok erken düştü: {dt:?}");
        end_session(&sh, &inj, r, 1);
        freezer.join().unwrap();
    }
}
