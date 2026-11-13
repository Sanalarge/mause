use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Left, Side::Right, Side::Top, Side::Bottom];

    pub fn to_u8(self) -> u8 {
        self as u8
    }
    pub fn from_u8(v: u8) -> Side {
        Side::ALL.get(v as usize).copied().unwrap_or(Side::Left)
    }
    pub fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }
    pub fn key(self) -> &'static str {
        ["left", "right", "top", "bottom"][self as usize]
    }
    pub fn from_key(s: &str) -> Side {
        Side::ALL
            .into_iter()
            .find(|x| x.key() == s)
            .unwrap_or(Side::Left)
    }
    pub fn tr(self) -> &'static str {
        ["sol", "sağ", "üst", "alt"][self as usize]
    }
}

#[derive(Clone)]
pub struct Config {
    pub addr: String,
    pub port: String,
    pub secret: String,
    pub w: i32,
    pub h: i32,
    pub side: Side,
    pub name: String,
    pub audio_sink: String,
    pub audio_source: String,
    pub relay_in: bool,
    pub mic_in: bool,
    pub relay_src: String,
    pub mic_src: String,
    pub relay_latency: u32,
    pub carry_files: bool,
    pub auto_connect: bool,
    pub share_text: bool,
    pub share_metrics: bool,
    pub show_metrics: bool,
    pub metrics_mask: u8,
    pub id: String,
    pub require_approval: bool,
    pub trusted: Vec<(String, String)>,
    pub paired: Vec<(String, String, String)>,
    pub placed: Vec<(String, Side, String)>,
    pub active_peer: String,
    pub peer_profiles: Vec<(String, String)>,
    pub peer_defaults: String,
    pub device_perms: Vec<(String, u16)>,
    pub blocked: Vec<(String, String)>,
    pub held: Vec<String>,
    pub allow_control_out: bool,
    pub allow_control_in: bool,
    pub allow_audio_out: bool,
    pub allow_mic_out: bool,
    pub allow_files_in: bool,
    pub allow_clip_in: bool,
    pub allow_screen_out: bool,
    pub allow_screen_control: bool,
    pub allow_usb_out: bool,
    pub allow_remote_prefs: bool,
    pub usb_shared: Vec<String>,
    pub screen: crate::screen::Opts,
    pub excluded: Vec<String>,
    pub theme: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            addr: String::new(),
            port: "24800".into(),
            secret: String::new(),
            w: 0,
            h: 0,
            side: Side::Left,
            name: hostname(),
            audio_sink: String::new(),
            audio_source: String::new(),
            relay_in: false,
            mic_in: false,
            relay_src: String::new(),
            mic_src: String::new(),
            relay_latency: 80,
            carry_files: true,
            auto_connect: true,
            share_text: true,
            share_metrics: false,
            show_metrics: false,
            metrics_mask: crate::metrics::ALL,
            id: random_id(),
            require_approval: true,
            trusted: Vec::new(),
            paired: Vec::new(),
            placed: Vec::new(),
            active_peer: String::new(),
            peer_profiles: Vec::new(),
            peer_defaults: String::new(),
            device_perms: Vec::new(),
            blocked: Vec::new(),
            held: Vec::new(),
            allow_control_out: true,
            allow_control_in: true,
            allow_audio_out: true,
            allow_mic_out: false,
            allow_files_in: false,
            allow_clip_in: true,
            allow_screen_out: false,
            allow_screen_control: false,
            allow_usb_out: false,
            allow_remote_prefs: true,
            usb_shared: Vec::new(),
            screen: crate::screen::Opts::default(),
            excluded: Vec::new(),
            theme: 1,
        }
    }
}

pub fn hostname() -> String {
    let h = crate::platform::hostname();
    if !h.is_empty() {
        return h;
    }
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "pc".into())
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("sistem rastgele sayı üreticisi kullanılamıyor");
    b
}

pub fn random_secret() -> String {
    let b = random_bytes::<4>();
    let n = u32::from_le_bytes(b) % 1_000_000;
    format!("{n:06}")
}

pub fn valid_addr(a: &str) -> bool {
    let a = a.trim();
    !a.is_empty()
        && (a.parse::<std::net::IpAddr>().is_ok()
            || (a.chars().any(|c| c.is_alphabetic()) && !a.contains(' ')))
}

pub fn pair_secret() -> String {
    random_bytes::<16>()
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

pub fn random_id() -> String {
    random_bytes::<4>()
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

pub fn secret_ok(s: &str) -> bool {
    s.trim().chars().count() >= 4
}

pub fn path() -> PathBuf {
    crate::platform::config_dir()
        .join("mause")
        .join("config.conf")
}

impl Config {
    pub fn side_of(&self, id: &str) -> Option<Side> {
        self.placed
            .iter()
            .find(|(i, ..)| i == id)
            .map(|(_, s, _)| *s)
    }

    pub fn parent_of(&self, id: &str) -> Option<&str> {
        self.placed
            .iter()
            .find(|(i, ..)| i == id)
            .map(|(_, _, p)| p.as_str())
    }

    pub fn ancestry(&self, id: &str) -> Vec<String> {
        let mut chain = Vec::new();
        let mut at = id.to_string();
        while !at.is_empty() && !chain.contains(&at) {
            chain.push(at.clone());
            at = self.parent_of(&at).unwrap_or("").to_string();
        }
        chain
    }

    pub fn place(&mut self, id: &str, side: Side, parent: &str) {
        if self.ancestry(parent).iter().any(|a| a == id) {
            return;
        }
        self.placed.retain(|(i, ..)| i != id);
        self.placed.push((id.to_string(), side, parent.to_string()));
    }

    pub fn unplace(&mut self, id: &str) {
        let mut gone = vec![id.to_string()];
        while let Some(at) = gone.pop() {
            let children: Vec<String> = self
                .placed
                .iter()
                .filter(|(_, _, p)| *p == at)
                .map(|(i, ..)| i.clone())
                .collect();
            self.placed.retain(|(i, ..)| *i != at);
            gone.extend(children);
        }
    }

    pub fn load() -> Self {
        let mut c = Config::default();
        let Ok(text) = fs::read_to_string(path()) else {
            return c;
        };
        c.parse(&text);
        c
    }

    fn parse(&mut self, text: &str) {
        let c = self;
        let mut browse = None;
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim().to_string();
            match k.trim() {
                "addr" => c.addr = if valid_addr(&v) { v } else { String::new() },
                "port" => c.port = v,
                "secret" if !v.is_empty() => c.secret = v,
                "w" => c.w = v.parse().unwrap_or(0),
                "h" => c.h = v.parse().unwrap_or(0),
                "side" => c.side = Side::from_key(&v),
                "name" if !v.is_empty() => c.name = v,
                "active_peer" => c.active_peer = v,
                "peer_defaults" => {
                    if let Some(settings) = decode_settings(&v) {
                        c.peer_defaults = settings;
                    }
                }
                "peer_profile" => {
                    if let Some((id, settings)) = v.split_once('|').filter(|(id, _)| !id.is_empty())
                    {
                        if let Some(settings) = decode_settings(settings) {
                            c.peer_profiles.retain(|(i, _)| i != id);
                            c.peer_profiles.push((id.to_owned(), settings));
                        }
                    }
                }
                "audio_sink" => c.audio_sink = v,
                "audio_source" => c.audio_source = v,
                "carry_files" => c.carry_files = v != "0",
                "auto_connect" => c.auto_connect = v != "0",
                "share_text" => c.share_text = v != "0",
                "id" if !v.is_empty() => c.id = v,
                "require_approval" => c.require_approval = v != "0",
                "allow_control_out" => c.allow_control_out = v != "0",
                "allow_control_in" => c.allow_control_in = v != "0",
                "allow_audio_out" => c.allow_audio_out = v != "0",
                "allow_mic_out" => c.allow_mic_out = v != "0",
                "allow_files_in" => c.allow_files_in = v != "0",
                "allow_clip_in" => c.allow_clip_in = v != "0",
                "allow_screen_out" => c.allow_screen_out = v != "0",
                "allow_browse" => browse = Some(v != "0"),
                "allow_screen_control" => c.allow_screen_control = v != "0",
                "allow_usb_out" => c.allow_usb_out = v != "0",
                "allow_remote_prefs" => c.allow_remote_prefs = v != "0",
                "usb_share" if !v.is_empty() => c.usb_shared.push(v),
                "screen_quality" => c.screen.quality = v.parse().unwrap_or(70u8).clamp(10, 95),
                "screen_fps" => c.screen.fps = v.parse().unwrap_or(15u8).clamp(1, 30),
                "screen_max_w" => c.screen.max_w = v.parse().unwrap_or(1920),
                "screen_monitor" => c.screen.monitor = v.parse().unwrap_or(0),
                "screen_cursor" => c.screen.cursor = v != "0",
                "screen_h264" => c.screen.h264 = v != "0",
                "share_metrics" => c.share_metrics = v == "1",
                "show_metrics" => c.show_metrics = v == "1",
                "metrics_mask" => {
                    c.metrics_mask =
                        v.parse::<u8>().unwrap_or(crate::metrics::ALL) & crate::metrics::ALL
                }
                "screen_extend" => {
                    c.screen.extend = v.split_once('x').and_then(|(w, h)| {
                        let size = (w.parse().ok()?, h.parse().ok()?);
                        matches!(
                            size,
                            (1280, 720) | (1920, 1080) | (2560, 1440) | (3840, 2160)
                        )
                        .then_some(size)
                    })
                }
                "exclude" if !v.is_empty() => c.excluded.push(v),
                "pair" => {
                    let mut it = v.splitn(3, '|');
                    if let (Some(i), Some(n), Some(s)) = (it.next(), it.next(), it.next()) {
                        c.paired.push((i.to_string(), n.to_string(), s.to_string()));
                    }
                }
                "place" => {
                    let mut it = v.splitn(3, '|');
                    if let (Some(i), Some(side)) = (it.next().filter(|i| !i.is_empty()), it.next())
                    {
                        c.place(i, Side::from_key(side), it.next().unwrap_or(""));
                    }
                }
                "perm" => {
                    let entry = v
                        .split_once('|')
                        .filter(|(i, _)| !i.is_empty())
                        .and_then(|(i, bits)| Some((i, bits.parse::<u16>().ok()?)));
                    if let Some((i, bits)) = entry {
                        c.device_perms.retain(|(d, _)| d != i);
                        c.device_perms.push((i.to_string(), bits));
                    }
                }
                "trust" | "block" => {
                    if let Some((i, n)) = v.split_once('|') {
                        let e = (i.to_string(), n.to_string());
                        if k.trim() == "trust" {
                            c.trusted.push(e)
                        } else {
                            c.blocked.push(e)
                        }
                    }
                }
                "theme" => c.theme = v.parse().unwrap_or(1).min(2),
                "relay_in" => c.relay_in = v == "1",
                "mic_in" => c.mic_in = v == "1",
                "relay_src" => c.relay_src = v,
                "mic_src" => c.mic_src = v,
                "relay_latency" => c.relay_latency = v.parse().unwrap_or(80).clamp(20, 500),
                _ => {}
            }
        }
        if browse == Some(false) {
            c.allow_files_in = false;
        }
    }

    pub fn save(&self) {
        let p = path();
        if let Some(d) = p.parent() {
            let _ = fs::create_dir_all(d);
        }
        let _ = fs::write(p, self.serialize());
    }

    fn serialize_base(&self) -> String {
        let s = format!(
            "addr={}\nport={}\nsecret={}\nw={}\nh={}\nside={}\nname={}\naudio_sink={}\naudio_source={}\nrelay_in={}\nmic_in={}\nrelay_src={}\nmic_src={}\nrelay_latency={}\ncarry_files={}\nauto_connect={}\nshare_text={}\ntheme={}\n",
            self.addr,
            self.port,
            self.secret,
            self.w,
            self.h,
            self.side.key(),
            self.name,
            self.audio_sink,
            self.audio_source,
            self.relay_in as u8,
            self.mic_in as u8,
            self.relay_src,
            self.mic_src,
            self.relay_latency,
            self.carry_files as u8,
            self.auto_connect as u8,
            self.share_text as u8,
            self.theme
        );
        let mut s = s;
        s.push_str(&format!(
            "id={}\nrequire_approval={}\n",
            self.id, self.require_approval as u8
        ));
        for (i, n) in &self.trusted {
            s.push_str(&format!("trust={}|{}\n", i, n.replace(['|', '\n'], " ")));
        }
        s.push_str(&format!(
            "allow_control_out={}\nallow_control_in={}\nallow_audio_out={}\nallow_mic_out={}\nallow_files_in={}\nallow_clip_in={}\nallow_screen_out={}\n",
            self.allow_control_out as u8, self.allow_control_in as u8, self.allow_audio_out as u8,
            self.allow_mic_out as u8, self.allow_files_in as u8, self.allow_clip_in as u8,
            self.allow_screen_out as u8
        ));
        s.push_str(&format!(
            "allow_screen_control={}\nallow_usb_out={}\nscreen_quality={}\nscreen_fps={}\nscreen_max_w={}\nscreen_monitor={}\nscreen_cursor={}\n",
            self.allow_screen_control as u8, self.allow_usb_out as u8, self.screen.quality, self.screen.fps,
            self.screen.max_w, self.screen.monitor, self.screen.cursor as u8
        ));
        s.push_str(&format!(
            "allow_remote_prefs={}\n",
            self.allow_remote_prefs as u8
        ));
        s.push_str(&format!(
            "share_metrics={}\nshow_metrics={}\nmetrics_mask={}\n",
            self.share_metrics as u8, self.show_metrics as u8, self.metrics_mask
        ));
        if let Some((w, h)) = self.screen.extend {
            s.push_str(&format!("screen_extend={w}x{h}\n"));
        }
        for d in &self.usb_shared {
            s.push_str(&format!("usb_share={}\n", d.replace('\n', " ")));
        }
        for d in &self.excluded {
            s.push_str(&format!("exclude={}\n", d.replace('\n', " ")));
        }
        for (i, n, sec) in &self.paired {
            s.push_str(&format!(
                "pair={}|{}|{}\n",
                i,
                n.replace(['|', '\n'], " "),
                sec
            ));
        }
        for (i, bits) in &self.device_perms {
            s.push_str(&format!("perm={i}|{bits}\n"));
        }
        for (i, side, parent) in &self.placed {
            s.push_str(&format!("place={}|{}|{}\n", i, side.key(), parent));
        }
        for (i, n) in &self.blocked {
            s.push_str(&format!("block={}|{}\n", i, n.replace(['|', '\n'], " ")));
        }
        s.push_str(&format!("screen_h264={}\n", self.screen.h264 as u8));
        s
    }

    fn peer_settings(&self) -> String {
        let mut text = self
            .serialize_base()
            .lines()
            .filter(|line| {
                line.split_once('=')
                    .is_some_and(|(key, _)| peer_setting_key(key))
            })
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        if self.screen.extend.is_none() {
            text.push_str("screen_extend=\n");
        }
        text
    }

    fn apply_peer_settings(&mut self, settings: &str) {
        let text = settings
            .lines()
            .filter(|line| {
                line.split_once('=')
                    .is_some_and(|(key, _)| peer_setting_key(key))
            })
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        self.parse(&text);
    }

    pub fn for_peer(&self, id: &str) -> Self {
        let mut cfg = self.clone();
        let settings = self
            .peer_profiles
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, settings)| settings.as_str())
            .unwrap_or(if self.active_peer.is_empty() {
                ""
            } else {
                &self.peer_defaults
            });
        if id != self.active_peer && !settings.is_empty() {
            cfg.apply_peer_settings(settings);
        }
        if let Some((_, bits)) = self.device_perms.iter().find(|(i, _)| i == id) {
            for pref in crate::flows::Pref::ALL {
                crate::flows::set(&mut cfg, pref, bits & pref.bit() != 0);
            }
            cfg.allow_screen_out = bits & crate::flows::SCREEN_OUT != 0;
        }
        cfg
    }

    pub fn activate_peer(&mut self, id: &str) -> bool {
        if self.active_peer == id {
            return false;
        }
        if self.active_peer.is_empty() || self.peer_defaults.is_empty() {
            self.peer_defaults = self.peer_settings();
        }
        if !self.active_peer.is_empty() {
            let settings = self.peer_settings();
            self.peer_profiles.retain(|(i, _)| *i != self.active_peer);
            self.peer_profiles
                .push((self.active_peer.clone(), settings));
        }
        let cfg = self.for_peer(id);
        self.apply_peer_settings(&cfg.peer_settings());
        self.active_peer = id.to_owned();
        true
    }

    fn serialize(&self) -> String {
        let mut text = self.serialize_base();
        text.push_str(&format!(
            "active_peer={}\npeer_defaults={}\n",
            self.active_peer,
            encode_settings(&self.peer_defaults)
        ));
        for (id, settings) in &self.peer_profiles {
            if *id != self.active_peer {
                text.push_str(&format!(
                    "peer_profile={id}|{}\n",
                    encode_settings(settings)
                ));
            }
        }
        if !self.active_peer.is_empty() {
            text.push_str(&format!(
                "peer_profile={}|{}\n",
                self.active_peer,
                encode_settings(&self.peer_settings())
            ));
        }
        text
    }
}

fn peer_setting_key(key: &str) -> bool {
    matches!(
        key,
        "side"
            | "audio_sink"
            | "audio_source"
            | "relay_in"
            | "mic_in"
            | "relay_src"
            | "mic_src"
            | "relay_latency"
            | "carry_files"
            | "share_text"
            | "allow_control_out"
            | "allow_control_in"
            | "allow_audio_out"
            | "allow_mic_out"
            | "allow_files_in"
            | "allow_clip_in"
            | "allow_screen_out"
            | "allow_screen_control"
            | "allow_usb_out"
            | "allow_remote_prefs"
            | "screen_quality"
            | "screen_fps"
            | "screen_max_w"
            | "screen_monitor"
            | "screen_cursor"
            | "screen_h264"
            | "screen_extend"
    )
}

fn encode_settings(text: &str) -> String {
    text.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_settings(hex: &str) -> Option<String> {
    if hex.len() > 65536 || !hex.len().is_multiple_of(2) || !hex.is_ascii() {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn performance_is_opt_in_and_categories_are_bounded() {
        let mut c = Config::default();
        assert!(!c.share_metrics && !c.show_metrics);
        c.parse("share_metrics=1\nshow_metrics=1\nmetrics_mask=255\n");
        assert!(c.share_metrics && c.show_metrics);
        assert_eq!(c.metrics_mask, crate::metrics::ALL);
    }

    #[test]
    fn virtual_display_preference_accepts_supported_sizes_only() {
        let mut c = Config::default();
        c.parse("screen_extend=1920x1080\n");
        assert_eq!(c.screen.extend, Some((1920, 1080)));
        for bad in ["0x0", "9999x9999", "1920", "abc", "1920x0"] {
            c.parse(&format!("screen_extend={bad}\n"));
            assert_eq!(c.screen.extend, None);
        }
    }

    #[test]
    fn eski_klasor_izni_kapaliysa_dosya_izni_kapali_baslar() {
        let load = |text: &str| {
            let mut c = Config::default();
            c.parse(text);
            c.allow_files_in
        };
        assert!(!load("allow_files_in=1\nallow_browse=0\n"));
        assert!(load("allow_files_in=1\nallow_browse=1\n"));
        assert!(load("allow_files_in=1\n"));
        assert!(!load(""));
    }

    #[test]
    fn ayni_kenari_farkli_zamanlarda_kullanan_cihazlar_hatirlanir() {
        let mut c = Config::default();
        c.parse("place=aa|right\nplace=bb|left\nplace=|top\nplace=cc|right\n");
        assert!(c.side_of("aa") == Some(Side::Right));
        assert!(c.side_of("bb") == Some(Side::Left));
        assert!(c.side_of("cc") == Some(Side::Right));
        assert_eq!(c.placed.len(), 3);
        c.place("bb", Side::Top, "");
        assert!(c.side_of("bb") == Some(Side::Top));
        assert_eq!(c.placed.len(), 3);
        c.unplace("cc");
        assert!(c.side_of("cc").is_none());
    }

    #[test]
    fn pc_profiles_survive_switching_restart_and_new_devices_without_changing_identity() {
        let mut c = Config::default();
        let identity = (c.id.clone(), c.name.clone());
        c.secret = "local-secret".into();
        c.place("A", Side::Right, "");
        c.activate_peer("A");
        c.relay_in = true;
        c.allow_files_in = true;
        c.relay_latency = 140;
        c.screen.quality = 90;
        c.screen.extend = Some((1920, 1080));
        c.relay_src = "音声|A".into();
        c.theme = 2;
        c.activate_peer("B");
        c.place("B", Side::Right, "");
        assert!(!c.relay_in && !c.allow_files_in);
        assert_eq!(c.screen.extend, None);
        assert_eq!(c.relay_latency, 80);
        c.relay_latency = 60;
        c.allow_control_out = false;
        let mut loaded = Config::default();
        loaded.parse(&c.serialize());
        assert_eq!(loaded.active_peer, "B");
        assert!(!loaded.allow_control_out);
        loaded.activate_peer("A");
        assert!(loaded.relay_in && loaded.allow_files_in && loaded.allow_control_out);
        assert_eq!(loaded.screen.extend, Some((1920, 1080)));
        assert_eq!(loaded.screen.quality, 90);
        assert_eq!(loaded.relay_latency, 140);
        assert_eq!(loaded.relay_src, "音声|A");
        assert_eq!((loaded.id.clone(), loaded.name.clone()), identity);
        assert_eq!(loaded.secret, "local-secret");
        assert_eq!(loaded.theme, 2);
        assert!(loaded.side_of("A") == Some(Side::Right));
        assert!(loaded.side_of("B") == Some(Side::Right));
        loaded.activate_peer("C");
        assert!(!loaded.relay_in && !loaded.allow_files_in);
        assert_eq!(loaded.screen.extend, None);
        loaded.activate_peer("B");
        assert!(!loaded.allow_control_out);
        assert_eq!(loaded.relay_latency, 60);
        loaded.activate_peer("");
        assert_eq!(loaded.relay_latency, 80);
        loaded.relay_latency = 100;
        assert_eq!(loaded.for_peer("new").relay_latency, 100);
        loaded.activate_peer("new");
        assert_eq!(loaded.relay_latency, 100);
    }

    #[test]
    fn device_permissions_and_legacy_config_migrate_without_crossing_profiles() {
        let mut c = Config::default();
        c.parse("relay_latency=120\nplace=A|top\nperm=A|1\n");
        c.activate_peer("A");
        assert_eq!(c.relay_latency, 120);
        assert!(c.allow_control_in && !c.allow_control_out);
        c.activate_peer("B");
        assert!(c.allow_control_out);
        assert_eq!(c.relay_latency, 120);
        c.peer_profiles.push((
            "malicious".into(),
            "id=wrong\nsecret=wrong\nname=wrong\nplace=X|left\nrelay_latency=55\n".into(),
        ));
        let identity = c.id.clone();
        c.activate_peer("malicious");
        assert_eq!(c.id, identity);
        assert!(c.side_of("X").is_none());
        assert_eq!(c.relay_latency, 55);
        for invalid in ["x", "zz", "é", "ff"] {
            assert!(decode_settings(invalid).is_none());
        }
    }

    #[test]
    fn zincirli_yerlesim_agac_olarak_kalir() {
        let mut c = Config::default();
        c.parse("place=aa|right\nplace=bb|right|aa\nplace=cc|top|bb\n");
        assert_eq!(c.parent_of("aa"), Some(""));
        assert_eq!(c.parent_of("cc"), Some("bb"));
        assert_eq!(c.ancestry("cc"), vec!["cc", "bb", "aa"]);
        c.place("aa", Side::Bottom, "cc");
        assert!(c.side_of("aa") == Some(Side::Right), "döngü kurulmamalı");
        c.place("dd", Side::Top, "aa");
        c.unplace("bb");
        assert!(c.side_of("bb").is_none() && c.side_of("cc").is_none());
        assert!(c.side_of("aa").is_some() && c.side_of("dd").is_some());
    }

    #[test]
    fn gecersiz_elle_adres_yok_sayilir() {
        for ok in [
            "192.168.1.136",
            "pc-ofis",
            "taymur-work.local",
            "::1",
            " 10.0.0.2 ",
        ] {
            assert!(valid_addr(ok), "{ok} geçerli sayılmalı");
        }
        for bad in ["", "109957", "123456", "a b", "   "] {
            assert!(!valid_addr(bad), "{bad:?} geçersiz sayılmalı");
        }
    }
}
