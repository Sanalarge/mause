use super::{Ev, ev};
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, Device, EventType, InputEvent, KeyCode,
    RelativeAxisCode, UinputAbsSetup, uinput::VirtualDevice,
};
use std::collections::HashSet;
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

const EVIOCGRAB: libc::c_ulong = 0x4004_4590;

pub fn init() {
    if unsafe { libc::geteuid() } != 0 {
        return;
    }
    let uid = ["SUDO_UID", "PKEXEC_UID"]
        .iter()
        .find_map(|k| std::env::var(k).ok()?.parse::<u32>().ok())
        .filter(|u| *u != 0);
    let Some(uid) = uid else {
        eprintln!(
            "Uyarı: Mause root olarak çalışıyor; ekran paylaşımı, pano ve ses çalışmayabilir. Normal kullanıcı olarak başlatın."
        );
        return;
    };
    let pw = unsafe { libc::getpwuid(uid) };
    if pw.is_null() {
        return;
    }
    let (name, gid, home) = unsafe {
        let s = |p: *const libc::c_char| std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned();
        (s((*pw).pw_name), (*pw).pw_gid, s((*pw).pw_dir))
    };
    let _ = Command::new("usermod")
        .args(["-aG", "input", &name])
        .status();
    let _ = Command::new("sh")
        .args([
            "-c",
            "setfacl -m \"u:$1:rw\" /dev/input/event* /dev/uinput",
            "sh",
            &name,
        ])
        .status();

    let mut cmd = Command::new("/proc/self/exe");
    cmd.args(std::env::args_os().skip(1));
    for k in [
        "SUDO_UID",
        "SUDO_GID",
        "SUDO_USER",
        "SUDO_COMMAND",
        "PKEXEC_UID",
    ] {
        cmd.env_remove(k);
    }
    cmd.env("HOME", &home)
        .env("USER", &name)
        .env("LOGNAME", &name);
    for (k, v) in session_env(uid) {
        cmd.env(k, v);
    }
    use std::os::unix::process::CommandExt;
    let cname = std::ffi::CString::new(name.clone()).unwrap_or_default();
    unsafe {
        if libc::initgroups(cname.as_ptr(), gid) != 0
            || libc::setgid(gid) != 0
            || libc::setuid(uid) != 0
        {
            eprintln!("Uyarı: {name} kullanıcısına geçilemedi; Mause root olarak devam ediyor.");
            return;
        }
    }
    let e = cmd.exec();
    eprintln!("Mause {name} olarak yeniden başlatılamadı: {e}");
    std::process::exit(1);
}

fn session_env(uid: u32) -> Vec<(String, String)> {
    use std::os::unix::fs::MetadataExt;
    const KEYS: [&str; 9] = [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_SESSION_TYPE",
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
        "XAUTHORITY",
    ];
    let from_proc = std::fs::read_dir("/proc").ok().and_then(|rd| {
        rd.flatten()
            .filter(|e| e.metadata().is_ok_and(|m| m.uid() == uid))
            .filter_map(|e| std::fs::read(e.path().join("environ")).ok())
            .map(|raw| {
                raw.split(|b| *b == 0)
                    .filter_map(|kv| std::str::from_utf8(kv).ok()?.split_once('='))
                    .filter(|(k, _)| KEYS.contains(k))
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect::<Vec<_>>()
            })
            .find(|env| {
                let has = |k: &str| env.iter().any(|(x, _)| x == k);
                has("DBUS_SESSION_BUS_ADDRESS") && (has("WAYLAND_DISPLAY") || has("DISPLAY"))
            })
    });
    if let Some(env) = from_proc {
        return env;
    }
    let run = format!("/run/user/{uid}");
    let mut env = vec![
        ("XDG_RUNTIME_DIR".to_owned(), run.clone()),
        (
            "DBUS_SESSION_BUS_ADDRESS".to_owned(),
            format!("unix:path={run}/bus"),
        ),
    ];
    let wayland = std::fs::read_dir(&run).ok().and_then(|rd| {
        rd.flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|n| n.starts_with("wayland-") && !n.ends_with(".lock"))
    });
    match wayland {
        Some(w) => {
            env.push(("WAYLAND_DISPLAY".to_owned(), w));
            env.push(("XDG_SESSION_TYPE".to_owned(), "wayland".to_owned()));
        }
        None => env.push((
            "DISPLAY".to_owned(),
            std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into()),
        )),
    }
    env
}

pub struct Injector {
    rel: VirtualDevice,
    abs: VirtualDevice,
    down: HashSet<u16>,
}

impl Injector {
    pub fn new() -> io::Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for c in 1..0x2ff {
            keys.insert(KeyCode::new(c));
        }
        let mut rel = AttributeSet::<RelativeAxisCode>::new();
        for a in [
            RelativeAxisCode::REL_X,
            RelativeAxisCode::REL_Y,
            RelativeAxisCode::REL_WHEEL,
            RelativeAxisCode::REL_HWHEEL,
            RelativeAxisCode::REL_WHEEL_HI_RES,
            RelativeAxisCode::REL_HWHEEL_HI_RES,
        ] {
            rel.insert(a);
        }
        let rel_dev = VirtualDevice::builder()?
            .name("mause-input")
            .with_keys(&keys)?
            .with_relative_axes(&rel)?
            .build()?;

        let mut btn = AttributeSet::<KeyCode>::new();
        btn.insert(KeyCode::BTN_LEFT);
        let info = AbsInfo::new(0, 0, 65535, 0, 0, 0);
        let abs_dev = VirtualDevice::builder()?
            .name("mause-warp")
            .with_keys(&btn)?
            .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, info))?
            .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, info))?
            .build()?;
        Ok(Self {
            rel: rel_dev,
            abs: abs_dev,
            down: HashSet::new(),
        })
    }

    pub fn emit(&mut self, evs: &[Ev]) -> io::Result<()> {
        let mut out = Vec::with_capacity(evs.len());
        for &(t, c, v) in evs {
            if t == ev::EV_KEY {
                if v == 2 {
                    continue;
                }
                if v == 1 {
                    self.down.insert(c);
                } else {
                    self.down.remove(&c);
                }
            }
            out.push(InputEvent::new(t, c, v));
        }
        self.rel.emit(&out)
    }

    pub fn emit_pointer(&mut self, evs: &[Ev], x: i32, y: i32, w: i32, h: i32) -> io::Result<()> {
        let is_move = |e: &&Ev| e.0 == ev::EV_REL && (e.1 == ev::REL_X || e.1 == ev::REL_Y);
        let moved = evs.iter().any(|e| is_move(&e));
        let rest: Vec<Ev> = evs.iter().filter(|e| !is_move(e)).copied().collect();
        if moved {
            self.warp(x, y, w, h)?;
        }
        if rest.is_empty() {
            return Ok(());
        }
        self.emit(&rest)
    }

    pub fn warp(&mut self, x: i32, y: i32, w: i32, h: i32) -> io::Result<()> {
        let ax = x.clamp(0, w - 1) * 65535 / (w - 1).max(1);
        let ay = y.clamp(0, h - 1) * 65535 / (h - 1).max(1);
        self.abs.emit(&[
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, ax),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, ay),
        ])
    }

    pub fn point(&mut self, x: i32, y: i32) -> io::Result<()> {
        let d = super::desktop();
        self.warp(x - d.x, y - d.y, d.w, d.h)
    }

    pub fn release_all(&mut self) {
        let evs: Vec<Ev> = self.down.iter().map(|&c| (ev::EV_KEY, c, 0)).collect();
        if !evs.is_empty() {
            let _ = self.emit(&evs);
        }
    }

    pub fn release_keys(&mut self, keys: &HashSet<u16>) -> io::Result<()> {
        let evs: Vec<InputEvent> = keys
            .iter()
            .map(|&c| InputEvent::new(ev::EV_KEY, c, 0))
            .collect();
        if evs.is_empty() {
            return Ok(());
        }
        self.rel.emit(&evs)
    }
}

static DEVS: Mutex<Vec<(PathBuf, i32, String)>> = Mutex::new(Vec::new());
static GRAB_ON: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);
static EXCLUDED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn set_excluded(names: Vec<String>) {
    *EXCLUDED.lock().unwrap() = names;
    grab(GRAB_ON.load(Ordering::Relaxed));
}

fn is_excluded(name: &str) -> bool {
    EXCLUDED.lock().unwrap().iter().any(|n| n == name)
}

pub fn input_devices() -> Vec<(String, &'static str)> {
    let mut v: Vec<(String, &'static str)> = Vec::new();
    for (_p, dev) in evdev::enumerate() {
        let name = dev.name().unwrap_or("?").to_string();
        if name.starts_with("mause-") {
            continue;
        }
        let (m, k) = classify(&dev);
        if (m || k) && !v.iter().any(|(n, _)| *n == name) {
            v.push((name, if m { "mouse" } else { "klavye" }));
        }
    }
    v
}

fn classify(dev: &Device) -> (bool, bool) {
    let keys = dev.supported_keys();
    let is_mouse = dev
        .supported_relative_axes()
        .is_some_and(|r| r.contains(RelativeAxisCode::REL_X))
        && keys.is_some_and(|k| k.contains(KeyCode::BTN_LEFT));
    let is_kbd = !is_mouse
        && keys
            .is_some_and(|k| k.contains(KeyCode::KEY_ENTER) || k.contains(KeyCode::KEY_VOLUMEUP));
    (is_mouse, is_kbd)
}

fn scan(cb: fn(Vec<Ev>)) {
    for (path, dev) in evdev::enumerate() {
        if dev.name().unwrap_or("").starts_with("mause-")
            || DEVS.lock().unwrap().iter().any(|(p, _, _)| *p == path)
        {
            continue;
        }
        let (is_mouse, is_kbd) = classify(&dev);
        if !is_mouse && !is_kbd {
            continue;
        }
        let fd = dev.as_raw_fd();
        if GRAB_ON.load(Ordering::Relaxed) && !is_excluded(dev.name().unwrap_or("")) {
            unsafe { libc::ioctl(fd, EVIOCGRAB, 1 as libc::c_int) };
        }
        DEVS.lock()
            .unwrap()
            .push((path.clone(), fd, dev.name().unwrap_or("").to_string()));
        let mut dev: Device = dev;
        thread::spawn(move || {
            let dname = dev.name().unwrap_or("").to_string();
            while let Ok(it) = dev.fetch_events() {
                if is_excluded(&dname) {
                    continue;
                }
                let evs: Vec<Ev> = it
                    .filter(|e| {
                        e.event_type() == EventType::KEY || e.event_type() == EventType::RELATIVE
                    })
                    .map(|e| (e.event_type().0, e.code(), e.value()))
                    .collect();
                if !evs.is_empty() {
                    cb(evs);
                }
            }
            DEVS.lock().unwrap().retain(|(p, _, _)| *p != path);
        });
    }
}

pub fn start_capture(cb: fn(Vec<Ev>)) -> io::Result<()> {
    if STARTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    scan(cb);
    thread::spawn(move || {
        loop {
            thread::sleep(std::time::Duration::from_secs(3));
            scan(cb);
        }
    });
    let missing = unreadable_devices();
    if DEVS.lock().unwrap().is_empty() || !missing.is_empty() {
        return Err(io::Error::other(format!(
            "Okunamayan giriş cihazı: {}. Mause penceresindeki \"İzinleri ayarla\" düğmesiyle izin verin",
            if missing.is_empty() {
                "hiçbiri okunabilir değil".to_string()
            } else {
                missing.join(", ")
            }
        )));
    }
    Ok(())
}

pub fn grab(on: bool) {
    GRAB_ON.store(on, Ordering::Relaxed);
    for (_, fd, name) in DEVS.lock().unwrap().iter() {
        let want = on && !is_excluded(name);
        unsafe { libc::ioctl(*fd, EVIOCGRAB, want as libc::c_int) };
    }
}

fn key_bit(caps: &str, code: usize) -> bool {
    let words: Vec<&str> = caps.split_whitespace().collect();
    let w = code / 64;
    if w >= words.len() {
        return false;
    }
    u64::from_str_radix(words[words.len() - 1 - w], 16).is_ok_and(|v| v >> (code % 64) & 1 == 1)
}

pub fn unreadable_devices() -> Vec<String> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/sys/class/input") else {
        return out;
    };
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if !n.starts_with("event") {
            continue;
        }
        let dir = e.path().join("device");
        let name = std::fs::read_to_string(dir.join("name"))
            .unwrap_or_default()
            .trim()
            .to_string();
        if name.starts_with("mause-") {
            continue;
        }
        let caps = std::fs::read_to_string(dir.join("capabilities/key")).unwrap_or_default();
        let relevant = key_bit(&caps, 28) || key_bit(&caps, 0x110) || key_bit(&caps, 115);
        if relevant && std::fs::File::open(format!("/dev/input/{n}")).is_err() {
            out.push(name);
        }
    }
    out
}

pub fn capture_report() -> Vec<String> {
    let mut out = Vec::new();
    for (path, dev) in evdev::enumerate() {
        let name = dev.name().unwrap_or("?").to_string();
        let keys = dev.supported_keys();
        let is_mouse = dev
            .supported_relative_axes()
            .is_some_and(|r| r.contains(RelativeAxisCode::REL_X))
            && keys.is_some_and(|k| k.contains(KeyCode::BTN_LEFT));
        let has_letters = keys.is_some_and(|k| {
            k.contains(KeyCode::KEY_A)
                && k.contains(KeyCode::KEY_V)
                && k.contains(KeyCode::KEY_LEFTCTRL)
        });
        let is_kbd = !is_mouse
            && keys.is_some_and(|k| {
                k.contains(KeyCode::KEY_ENTER) || k.contains(KeyCode::KEY_VOLUMEUP)
            });
        let kind = if is_mouse {
            "mouse"
        } else if is_kbd {
            "klavye/medya"
        } else {
            continue;
        };
        let fd = dev.as_raw_fd();
        let on = unsafe { libc::ioctl(fd, EVIOCGRAB, 1 as libc::c_int) };
        let err = if on != 0 {
            Some(io::Error::last_os_error().to_string())
        } else {
            None
        };
        unsafe { libc::ioctl(fd, EVIOCGRAB, 0 as libc::c_int) };
        out.push(format!(
            "{:<14} {:<34} {}{}{}",
            kind,
            name,
            path.display(),
            if has_letters {
                "  [harfler+Ctrl var]"
            } else {
                ""
            },
            match err {
                None => "  kilitlenebiliyor ✔".to_string(),
                Some(e) => format!("  KİLİTLENEMEDİ: {e}"),
            },
        ));
    }
    out
}

pub fn check_permissions() -> (bool, bool) {
    let uinput = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
        .is_ok();
    (uinput, unreadable_devices().is_empty())
}

pub fn permission_help() -> (&'static str, &'static str) {
    (
        "Mouse/klavye cihazlarını okumak ve /dev/uinput'e yazmak için izin gerekli.",
        "sudo usermod -aG input $USER",
    )
}

pub fn screen_capture_begin() -> bool {
    if super::wayland::active() {
        super::wayland::start();
        true
    } else {
        capture_screen().is_some()
    }
}

pub fn screen_capture_begin_virtual(size: (u16, u16)) -> bool {
    if super::wayland::active() {
        super::wayland::start_virtual(size);
        true
    } else {
        false
    }
}

pub fn screen_capture_pending() -> bool {
    super::wayland::active() && super::wayland::pending()
}

pub fn screen_capture_error() -> Option<String> {
    super::wayland::active()
        .then(super::wayland::error)
        .flatten()
}

pub fn capture_screen() -> Option<super::Capture> {
    capture_monitor(0).map(|(c, _)| c)
}

pub fn capture_monitor(i: usize) -> Option<(super::Capture, super::Monitor)> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};
    if super::wayland::active() {
        let c = super::wayland::latest()?;
        let m = super::wayland::geometry().unwrap_or(super::Monitor {
            name: String::new(),
            x: 0,
            y: 0,
            w: c.w as i32,
            h: c.h as i32,
        });
        return Some((c, m));
    }
    let ms = monitors();
    let m = ms.get(i).or(ms.first())?.clone();
    let (conn, n) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[n].root;
    let img = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            root,
            m.x as i16,
            m.y as i16,
            m.w as u16,
            m.h as u16,
            !0,
        )
        .ok()?
        .reply()
        .ok()?;
    let (w, h) = (m.w as usize, m.h as usize);
    (img.data.len() == w * h * 4).then_some((
        super::Capture {
            w,
            h,
            stride: w * 4,
            bgra: img.data,
        },
        m,
    ))
}

pub fn monitors() -> Vec<super::Monitor> {
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xproto::ConnectionExt as _;
    let Ok((conn, n)) = x11rb::connect(None) else {
        return Vec::new();
    };
    let root = &conn.setup().roots[n];
    let mut out: Vec<super::Monitor> = conn
        .randr_get_monitors(root.root, true)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| {
            let mut ms = r.monitors;
            ms.sort_by_key(|m| !m.primary);
            ms.into_iter()
                .map(|m| super::Monitor {
                    name: conn
                        .get_atom_name(m.name)
                        .ok()
                        .and_then(|c| c.reply().ok())
                        .map(|a| String::from_utf8_lossy(&a.name).into_owned())
                        .unwrap_or_default(),
                    x: m.x as i32,
                    y: m.y as i32,
                    w: m.width as i32,
                    h: m.height as i32,
                })
                .collect()
        })
        .unwrap_or_default();
    if out.is_empty() {
        out.push(super::Monitor {
            name: String::new(),
            x: 0,
            y: 0,
            w: root.width_in_pixels as i32,
            h: root.height_in_pixels as i32,
        });
    }
    out
}

pub fn screen_cursor(on: bool) {
    super::wayland::set_cursor(on);
}

pub fn cursor_pos() -> Option<(i32, i32)> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;
    if super::wayland::active() {
        return None;
    }
    let (conn, n) = x11rb::connect(None).ok()?;
    let p = conn
        .query_pointer(conn.setup().roots[n].root)
        .ok()?
        .reply()
        .ok()?;
    Some((p.root_x as i32, p.root_y as i32))
}

pub fn mounts() -> Vec<(String, PathBuf)> {
    let mut out = vec![("Sistem diski".to_string(), PathBuf::from("/"))];
    let text = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(dev), Some(dir)) = (it.next(), it.next()) else {
            continue;
        };
        let dir = dir.replace("\\040", " ");
        let user_mount = ["/media/", "/run/media/", "/mnt/"]
            .iter()
            .any(|p| dir.starts_with(p))
            || dir == "/home";
        if dev.starts_with("/dev/") && user_mount {
            let name = Path::new(&dir)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(dir.clone());
            out.push((name, PathBuf::from(dir)));
        }
    }
    out
}

pub fn permission_panes() -> Vec<&'static str> {
    Vec::new()
}

pub fn open_permission_pane(_: usize) {}

pub fn detect_screen() -> Option<(i32, i32)> {
    let out = Command::new("kscreen-doctor").arg("-o").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut best: Option<(i32, (i32, i32))> = None;
    for block in text.split("Output:").skip(1) {
        let clean: String = {
            let mut s = String::new();
            let mut it = block.chars().peekable();
            while let Some(ch) = it.next() {
                if ch == '\x1b' {
                    for n in it.by_ref() {
                        if n.is_ascii_alphabetic() {
                            break;
                        }
                    }
                } else {
                    s.push(ch);
                }
            }
            s
        };
        if !clean.lines().any(|l| l.trim() == "enabled") {
            continue;
        }
        let prio = clean
            .lines()
            .find_map(|l| {
                l.trim()
                    .strip_prefix("priority ")
                    .and_then(|p| p.trim().parse::<i32>().ok())
            })
            .unwrap_or(99);
        let geo = clean.lines().find_map(|l| {
            let g = l.trim().strip_prefix("Geometry:")?.trim();
            let (_, size) = g.split_once(' ')?;
            let (w, h) = size.split_once('x')?;
            Some((w.parse::<i32>().ok()?, h.parse::<i32>().ok()?))
        });
        if let Some(g) = geo {
            if best.is_none_or(|(p, _)| prio < p) {
                best = Some((prio, g));
            }
        }
    }
    best.map(|(_, g)| g)
}

pub fn can_grant() -> bool {
    Command::new("pkexec").arg("--version").output().is_ok()
}

pub fn grant_permissions() -> io::Result<()> {
    let user = std::env::var("USER").map_err(|_| io::Error::other("kullanıcı adı bulunamadı"))?;
    if user.is_empty()
        || !user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(io::Error::other("geçersiz kullanıcı adı"));
    }
    let st = Command::new("pkexec")
        .args([
            "sh",
            "-c",
            "usermod -aG input \"$1\" && setfacl -m \"u:$1:rw\" /dev/input/event* /dev/uinput",
            "sh",
            &user,
        ])
        .status()?;
    if st.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            "izin verilmedi (parola penceresi iptal edildi ya da hata)",
        ))
    }
}

#[cfg(not(test))]
pub fn notify(title: &str, body: &str) {
    let _ = Command::new("notify-send")
        .args(["-a", "Mause", "-i", "mause", title, body])
        .spawn();
}

pub fn open_path(p: &Path) {
    let _ = Command::new("xdg-open").arg(p).spawn();
}

pub fn downloads_dir() -> PathBuf {
    Command::new("xdg-user-dir")
        .arg("DOWNLOAD")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Downloads")
        })
}

pub fn config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
        })
}

pub fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tus_bitmap_okuma() {
        assert!(key_bit("0 0 0 0 10000000", 28));
        assert!(!key_bit("0 0 0 0 10000000", 29));
        assert!(key_bit("0 0 1 0 0", 0x110 - 192 + 192) || key_bit("1 0 0 0 0", 256));
        assert!(!key_bit("", 28));
    }

    #[test]
    fn okunamayan_cihazlar_listelenir() {
        let missing = unreadable_devices();
        println!("okunamayan: {missing:?}");
    }
}

pub(super) fn flash_window(x: i32, y: i32, w: i32, h: i32) {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        ChangeWindowAttributesAux, ConnectionExt, CreateWindowAux, WindowClass,
    };
    if super::wayland::active() || w <= 0 || h <= 0 {
        return;
    }
    let Ok((conn, n)) = x11rb::connect(None) else {
        return;
    };
    let screen = &conn.setup().roots[n];
    let Ok(win) = conn.generate_id() else {
        return;
    };
    let aux = CreateWindowAux::new()
        .override_redirect(1)
        .background_pixel(super::FLASH_COLORS[0]);
    let made = conn.create_window(
        screen.root_depth,
        win,
        screen.root,
        x as i16,
        y as i16,
        w as u16,
        h as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &aux,
    );
    if made.is_err() || conn.map_window(win).is_err() {
        return;
    }
    for step in 0..super::FLASH_STEPS {
        let color = super::FLASH_COLORS[step as usize % 2];
        let _ = conn.change_window_attributes(
            win,
            &ChangeWindowAttributesAux::new().background_pixel(color),
        );
        let _ = conn.clear_area(false, win, 0, 0, 0, 0);
        let _ = conn.flush();
        std::thread::sleep(std::time::Duration::from_millis(super::FLASH_STEP_MS));
    }
    let _ = conn.destroy_window(win);
    let _ = conn.flush();
}
