use super::{Ev, ev};
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CGMouseButton, EventField, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::{CGPoint, CGRect};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;

const MARK: i64 = 0x4D41_5553;

const KEYS: &[(u16, u16)] = &[
    (0, 30),
    (1, 31),
    (2, 32),
    (3, 33),
    (4, 35),
    (5, 34),
    (6, 44),
    (7, 45),
    (8, 46),
    (9, 47),
    (10, 86),
    (11, 48),
    (12, 16),
    (13, 17),
    (14, 18),
    (15, 19),
    (16, 21),
    (17, 20),
    (18, 2),
    (19, 3),
    (20, 4),
    (21, 5),
    (22, 7),
    (23, 6),
    (24, 13),
    (25, 10),
    (26, 8),
    (27, 12),
    (28, 9),
    (29, 11),
    (30, 27),
    (31, 24),
    (32, 22),
    (33, 26),
    (34, 23),
    (35, 25),
    (36, 28),
    (37, 38),
    (38, 36),
    (39, 40),
    (40, 37),
    (41, 39),
    (42, 43),
    (43, 51),
    (44, 53),
    (45, 49),
    (46, 50),
    (47, 52),
    (48, 15),
    (49, 57),
    (50, 41),
    (51, 14),
    (53, 1),
    (54, 126),
    (55, 125),
    (56, 42),
    (57, 58),
    (58, 56),
    (59, 29),
    (60, 54),
    (61, 100),
    (62, 97),
    (65, 83),
    (67, 55),
    (69, 78),
    (71, 69),
    (75, 98),
    (76, 96),
    (78, 74),
    (81, 117),
    (82, 82),
    (83, 79),
    (84, 80),
    (85, 81),
    (86, 75),
    (87, 76),
    (88, 77),
    (89, 71),
    (91, 72),
    (92, 73),
    (96, 63),
    (97, 64),
    (98, 65),
    (99, 61),
    (100, 66),
    (101, 67),
    (103, 87),
    (109, 68),
    (111, 88),
    (114, 110),
    (115, 102),
    (116, 104),
    (117, 111),
    (118, 62),
    (119, 107),
    (120, 60),
    (121, 109),
    (122, 59),
    (123, 105),
    (124, 106),
    (125, 108),
    (126, 103),
];

fn mac_to_evdev(k: u16) -> Option<u16> {
    KEYS.iter().find(|(m, _)| *m == k).map(|(_, e)| *e)
}
fn evdev_to_mac(e: u16) -> Option<u16> {
    KEYS.iter().find(|(_, x)| *x == e).map(|(m, _)| *m)
}

fn modifier_mask(mac: u16) -> Option<u64> {
    match mac {
        56 | 60 => Some(0x0002_0000),
        59 | 62 => Some(0x0004_0000),
        58 | 61 => Some(0x0008_0000),
        55 | 54 => Some(0x0010_0000),
        57 => Some(0x0001_0000),
        _ => None,
    }
}

pub fn init() {}

pub(super) fn flash_window(x: i32, y: i32, w: i32, h: i32) {
    super::macos_flash::show(x, y, w, h);
}

pub struct Injector {
    down: HashSet<u16>,
    flags: CGEventFlags,
    buttons: u8,
}

fn source() -> io::Result<CGEventSource> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| io::Error::other("CGEventSource açılamadı"))
}

fn display_bounds() -> Vec<CGRect> {
    CGDisplay::active_displays()
        .map(|ids| {
            ids.into_iter()
                .map(|id| CGDisplay::new(id).bounds())
                .collect()
        })
        .unwrap_or_default()
}

fn contains(r: &CGRect, p: CGPoint) -> bool {
    p.x >= r.origin.x
        && p.x < r.origin.x + r.size.width
        && p.y >= r.origin.y
        && p.y < r.origin.y + r.size.height
}

fn keep_on_screen(screens: &[CGRect], from: CGPoint, to: CGPoint) -> CGPoint {
    if screens.is_empty() || screens.iter().any(|r| contains(r, to)) {
        return to;
    }
    let Some(r) = screens
        .iter()
        .find(|r| contains(r, from))
        .or(screens.first())
    else {
        return to;
    };
    CGPoint::new(
        to.x.clamp(r.origin.x, r.origin.x + r.size.width - 1.0),
        to.y.clamp(r.origin.y, r.origin.y + r.size.height - 1.0),
    )
}

fn cursor() -> CGPoint {
    source()
        .ok()
        .and_then(|s| CGEvent::new(s).ok())
        .map(|e| e.location())
        .unwrap_or(CGPoint::new(0.0, 0.0))
}

impl Injector {
    pub fn new() -> io::Result<Self> {
        require_control_permission()?;
        source()?;
        Ok(Self {
            down: HashSet::new(),
            flags: CGEventFlags::empty(),
            buttons: 0,
        })
    }

    fn post(e: CGEvent) {
        e.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, MARK);
        e.post(CGEventTapLocation::HID);
    }

    pub fn emit_pointer(
        &mut self,
        evs: &[Ev],
        _x: i32,
        _y: i32,
        _w: i32,
        _h: i32,
    ) -> io::Result<()> {
        self.emit(evs)
    }

    pub fn emit(&mut self, evs: &[Ev]) -> io::Result<()> {
        require_control_permission()?;
        let src = source()?;
        let (mut dx, mut dy) = (0i32, 0i32);
        let (mut wv, mut wh) = (0i32, 0i32);
        let mut moves = |this: &Self, dx: &mut i32, dy: &mut i32| {
            if *dx == 0 && *dy == 0 {
                return;
            }
            let p = cursor();
            let np = keep_on_screen(
                &display_bounds(),
                p,
                CGPoint::new(p.x + *dx as f64, p.y + *dy as f64),
            );
            let (ty, btn) = if this.buttons & 1 != 0 {
                (CGEventType::LeftMouseDragged, CGMouseButton::Left)
            } else if this.buttons & 2 != 0 {
                (CGEventType::RightMouseDragged, CGMouseButton::Right)
            } else {
                (CGEventType::MouseMoved, CGMouseButton::Left)
            };
            if let Ok(e) = CGEvent::new_mouse_event(src.clone(), ty, np, btn) {
                e.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_X, *dx as i64);
                e.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y, *dy as i64);
                Self::post(e);
            }
            (*dx, *dy) = (0, 0);
        };
        for &(t, c, v) in evs {
            if t == ev::EV_REL {
                match c {
                    ev::REL_X => dx += v,
                    ev::REL_Y => dy += v,
                    ev::REL_WHEEL_HI_RES => wv += v,
                    ev::REL_HWHEEL_HI_RES => wh += v,
                    _ => {}
                }
            } else if t == ev::EV_KEY && v != 2 {
                moves(self, &mut dx, &mut dy);
                let down = v != 0;
                if down {
                    self.down.insert(c);
                } else {
                    self.down.remove(&c);
                }
                if (ev::BTN_LEFT..=ev::BTN_EXTRA).contains(&c) {
                    let (dn, up, btn, bit) = match c {
                        ev::BTN_LEFT => (
                            CGEventType::LeftMouseDown,
                            CGEventType::LeftMouseUp,
                            CGMouseButton::Left,
                            1u8,
                        ),
                        ev::BTN_RIGHT => (
                            CGEventType::RightMouseDown,
                            CGEventType::RightMouseUp,
                            CGMouseButton::Right,
                            2,
                        ),
                        _ => (
                            CGEventType::OtherMouseDown,
                            CGEventType::OtherMouseUp,
                            CGMouseButton::Center,
                            4,
                        ),
                    };
                    if down {
                        self.buttons |= bit
                    } else {
                        self.buttons &= !bit
                    }
                    if let Ok(e) = CGEvent::new_mouse_event(
                        src.clone(),
                        if down { dn } else { up },
                        cursor(),
                        btn,
                    ) {
                        if c == ev::BTN_SIDE || c == ev::BTN_EXTRA {
                            e.set_integer_value_field(
                                EventField::MOUSE_EVENT_BUTTON_NUMBER,
                                if c == ev::BTN_SIDE { 3 } else { 4 },
                            );
                        }
                        Self::post(e);
                    }
                } else if let Some(m) = evdev_to_mac(c) {
                    if let Some(mask) = modifier_mask(m) {
                        let f = CGEventFlags::from_bits_retain(mask);
                        if down {
                            self.flags |= f
                        } else {
                            self.flags &= !f
                        }
                    }
                    if let Ok(e) = CGEvent::new_keyboard_event(src.clone(), m, down) {
                        e.set_flags(self.flags);
                        Self::post(e);
                    }
                }
            }
        }
        moves(self, &mut dx, &mut dy);
        if wv != 0 || wh != 0 {
            if let Ok(e) =
                CGEvent::new_scroll_event(src, ScrollEventUnit::LINE, 2, wv / 120, wh / 120, 0)
            {
                Self::post(e);
            }
        }
        Ok(())
    }

    pub fn warp(&mut self, x: i32, y: i32, w: i32, h: i32) -> io::Result<()> {
        let p = CGPoint::new(x.clamp(0, w - 1) as f64, y.clamp(0, h - 1) as f64);
        CGDisplay::warp_mouse_cursor_position(p)
            .map_err(|_| io::Error::other("imleç taşınamadı"))?;
        let _ = CGDisplay::associate_mouse_and_mouse_cursor_position(true);
        Ok(())
    }

    pub fn point(&mut self, x: i32, y: i32) -> io::Result<()> {
        require_control_permission()?;
        CGDisplay::warp_mouse_cursor_position(CGPoint::new(x as f64, y as f64))
            .map_err(|_| io::Error::other("imleç taşınamadı"))?;
        let _ = CGDisplay::associate_mouse_and_mouse_cursor_position(true);
        Ok(())
    }

    pub fn release_all(&mut self) {
        let evs: Vec<Ev> = self.down.iter().map(|&c| (ev::EV_KEY, c, 0)).collect();
        if !evs.is_empty() {
            let _ = self.emit(&evs);
        }
    }

    pub fn release_keys(&mut self, keys: &HashSet<u16>) -> io::Result<()> {
        let evs: Vec<Ev> = keys.iter().map(|&c| (ev::EV_KEY, c, 0)).collect();
        self.emit(&evs)
    }
}

static GRABBED: AtomicBool = AtomicBool::new(false);
static CALLBACK: OnceLock<fn(Vec<Ev>)> = OnceLock::new();
static STARTED: Mutex<bool> = Mutex::new(false);
static MODS: AtomicU32 = AtomicU32::new(0);

pub fn grab(on: bool) {
    GRABBED.store(on, Ordering::Relaxed);
}

fn translate(t: CGEventType, e: &CGEvent) -> Vec<Ev> {
    let mut v = Vec::new();
    match t {
        CGEventType::MouseMoved
        | CGEventType::LeftMouseDragged
        | CGEventType::RightMouseDragged
        | CGEventType::OtherMouseDragged => {
            let dx = e.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_X) as i32;
            let dy = e.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y) as i32;
            if dx != 0 {
                v.push((ev::EV_REL, ev::REL_X, dx));
            }
            if dy != 0 {
                v.push((ev::EV_REL, ev::REL_Y, dy));
            }
        }
        CGEventType::LeftMouseDown => v.push((ev::EV_KEY, ev::BTN_LEFT, 1)),
        CGEventType::LeftMouseUp => v.push((ev::EV_KEY, ev::BTN_LEFT, 0)),
        CGEventType::RightMouseDown => v.push((ev::EV_KEY, ev::BTN_RIGHT, 1)),
        CGEventType::RightMouseUp => v.push((ev::EV_KEY, ev::BTN_RIGHT, 0)),
        CGEventType::OtherMouseDown | CGEventType::OtherMouseUp => {
            let down = matches!(t, CGEventType::OtherMouseDown) as i32;
            let code = match e.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER) {
                2 => ev::BTN_MIDDLE,
                3 => ev::BTN_SIDE,
                _ => ev::BTN_EXTRA,
            };
            v.push((ev::EV_KEY, code, down));
        }
        CGEventType::ScrollWheel => {
            let a1 = e.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1) as i32;
            let a2 = e.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2) as i32;
            if a1 != 0 {
                v.push((ev::EV_REL, ev::REL_WHEEL, a1));
                v.push((ev::EV_REL, ev::REL_WHEEL_HI_RES, a1 * 120));
            }
            if a2 != 0 {
                v.push((ev::EV_REL, ev::REL_HWHEEL, a2));
                v.push((ev::EV_REL, ev::REL_HWHEEL_HI_RES, a2 * 120));
            }
        }
        CGEventType::KeyDown | CGEventType::KeyUp => {
            let k = e.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
            if let Some(code) = mac_to_evdev(k) {
                v.push((
                    ev::EV_KEY,
                    code,
                    (t as u32 == CGEventType::KeyDown as u32) as i32,
                ));
            }
        }
        CGEventType::FlagsChanged => {
            let k = e.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
            if let (Some(code), Some(mask)) = (mac_to_evdev(k), modifier_mask(k)) {
                let down = e.get_flags().bits() & mask != 0;
                v.push((ev::EV_KEY, code, down as i32));
            }
        }
        _ => {}
    }
    v
}

pub fn start_capture(cb: fn(Vec<Ev>)) -> io::Result<()> {
    let mut started = STARTED.lock().unwrap();
    if *started {
        return Ok(());
    }
    let _ = CALLBACK.set(cb);
    let _ = &MODS;
    let (tx, rx) = std::sync::mpsc::channel::<io::Result<()>>();
    thread::spawn(move || {
        let types = vec![
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
            CGEventType::MouseMoved,
            CGEventType::LeftMouseDown,
            CGEventType::LeftMouseUp,
            CGEventType::RightMouseDown,
            CGEventType::RightMouseUp,
            CGEventType::OtherMouseDown,
            CGEventType::OtherMouseUp,
            CGEventType::LeftMouseDragged,
            CGEventType::RightMouseDragged,
            CGEventType::OtherMouseDragged,
            CGEventType::ScrollWheel,
        ];
        let tap = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            types,
            |_proxy, t, e| {
                if e.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA) == MARK {
                    return Some(e.clone());
                }
                let evs = translate(t, e);
                if !evs.is_empty() {
                    if let Some(cb) = CALLBACK.get() {
                        cb(evs);
                    }
                }
                if GRABBED.load(Ordering::Relaxed) {
                    None
                } else {
                    Some(e.clone())
                }
            },
        );
        let Ok(tap) = tap else {
            let _ = tx.send(Err(io::Error::other(
                "olay yakalanamadı: Sistem Ayarları → Gizlilik ve Güvenlik → Erişilebilirlik ve Giriş İzleme'ye Mause'u ekleyin",
            )));
            return;
        };
        let Ok(src) = tap.mach_port.create_runloop_source(0) else {
            let _ = tx.send(Err(io::Error::other("run loop kaynağı oluşturulamadı")));
            return;
        };
        let rl = CFRunLoop::get_current();
        rl.add_source(&src, unsafe { kCFRunLoopCommonModes });
        tap.enable();
        let _ = tx.send(Ok(()));
        CFRunLoop::run_current();
    });
    let r = rx
        .recv()
        .unwrap_or_else(|_| Err(io::Error::other("yakalama thread'i başlamadı")));
    if r.is_ok() {
        *started = true;
    }
    r
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGPreflightListenEventAccess() -> bool;
}

pub fn check_permissions() -> (bool, bool) {
    let ok = unsafe { AXIsProcessTrusted() };
    (ok, unsafe { CGPreflightListenEventAccess() })
}

pub fn can_grant() -> bool {
    false
}

pub fn grant_permissions() -> io::Result<()> {
    Err(io::Error::other("bu platformda gerekmez"))
}

pub fn set_excluded(_: Vec<String>) {}

pub fn input_devices() -> Vec<(String, &'static str)> {
    Vec::new()
}

pub fn unreadable_devices() -> Vec<String> {
    Vec::new()
}

pub fn permission_help() -> (&'static str, &'static str) {
    (
        "Sistem Ayarları → Gizlilik ve Güvenlik → Erişilebilirlik ve Giriş İzleme bölümlerine Mause'u ekleyip açın, sonra Mause'u yeniden başlatın.",
        "",
    )
}

const PERMISSION_PANES: [(&str, &str); 3] = [
    (
        "Ekran Kaydı’nı aç",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
    ),
    (
        "Erişilebilirlik'i aç",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
    ),
    (
        "Giriş İzleme'yi aç",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent",
    ),
];

pub fn permission_panes() -> Vec<&'static str> {
    PERMISSION_PANES.iter().map(|(name, _)| *name).collect()
}

pub fn open_permission_pane(i: usize) {
    if let Some((_, url)) = PERMISSION_PANES.get(i) {
        let _ = Command::new("open").arg(url).spawn();
    }
}

pub fn screen_capture_begin() -> bool {
    if unsafe { CGPreflightScreenCaptureAccess() } {
        return true;
    }
    unsafe { CGRequestScreenCaptureAccess() }
}

pub fn screen_capture_begin_virtual(_: (u16, u16)) -> bool {
    crate::vdisplay::supported() && screen_capture_begin()
}

pub fn screen_capture_error() -> Option<String> {
    Some(if unsafe { CGPreflightScreenCaptureAccess() } {
        "macOS ekranı yakalayamadı. Masaüstü oturumunu ve ekranın kullanılabilirliğini kontrol edin."
    } else {
        "macOS Ekran Kaydı izni eksik. Sistem Ayarları → Gizlilik ve Güvenlik → Ekran Kaydı bölümünde Mause'a izin verip bağlantıyı yeniden başlatın."
    }.into())
}

fn require_control_permission() -> io::Result<()> {
    if unsafe { AXIsProcessTrusted() } {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "macOS Erişilebilirlik izni eksik veya kaldırıldı",
        ))
    }
}

pub fn screen_capture_pending() -> bool {
    false
}

pub fn screen_cursor(_: bool) {}

fn displays() -> Vec<CGDisplay> {
    let mut ids = CGDisplay::active_displays().unwrap_or_default();
    let main = CGDisplay::main().id;
    ids.sort_by_key(|id| *id != main);
    if ids.is_empty() {
        ids.push(main);
    }
    ids.into_iter().map(CGDisplay::new).collect()
}

pub fn display_index(id: u32) -> Option<usize> {
    displays().iter().position(|display| display.id == id)
}

pub fn monitors() -> Vec<super::Monitor> {
    displays()
        .into_iter()
        .enumerate()
        .map(|(i, d)| {
            let b = d.bounds();
            super::Monitor {
                name: if d.is_builtin() {
                    "Dahili ekran".into()
                } else {
                    format!("Ekran {}", i + 1)
                },
                x: b.origin.x as i32,
                y: b.origin.y as i32,
                w: b.size.width as i32,
                h: b.size.height as i32,
            }
        })
        .collect()
}

pub fn cursor_pos() -> Option<(i32, i32)> {
    let p = cursor();
    Some((p.x as i32, p.y as i32))
}

pub fn mounts() -> Vec<(String, PathBuf)> {
    let mut out = vec![("Sistem diski".to_string(), PathBuf::from("/"))];
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        for e in rd.flatten() {
            let p = e.path();
            if std::fs::read_link(&p).is_ok_and(|t| t == Path::new("/")) {
                continue;
            }
            out.push((e.file_name().to_string_lossy().into_owned(), p));
        }
    }
    out
}

pub fn capture_screen() -> Option<super::Capture> {
    capture_monitor(0).map(|(c, _)| c)
}

pub fn capture_monitor(i: usize) -> Option<(super::Capture, super::Monitor)> {
    if !unsafe { CGPreflightScreenCaptureAccess() } {
        return None;
    }
    let ds = displays();
    let i = if i < ds.len() { i } else { 0 };
    let m = monitors().get(i)?.clone();
    let img = ds.get(i)?.image()?;
    if img.bits_per_pixel() != 32 {
        return None;
    }
    let (w, h, stride) = (img.width(), img.height(), img.bytes_per_row());
    let bgra = img.data().bytes().to_vec();
    super::Capture::from_bytes(w, h, stride, 0, &bgra, false).map(|c| (c, m))
}

pub fn detect_screen() -> Option<(i32, i32)> {
    let b = CGDisplay::main().bounds();
    let (w, h) = (b.size.width as i32, b.size.height as i32);
    (w > 0 && h > 0).then_some((w, h))
}

#[cfg(not(test))]
pub fn notify(title: &str, body: &str) {
    let script = format!("display notification {:?} with title {:?}", body, title);
    let _ = Command::new("osascript").args(["-e", &script]).spawn();
}

pub fn open_path(p: &Path) {
    let _ = Command::new("open").arg(p).spawn();
}

pub fn downloads_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Downloads")
}

pub fn config_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library")
        .join("Application Support")
}

pub fn hostname() -> String {
    Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_graphics::geometry::CGSize;

    fn screen(x: f64, y: f64, w: f64, h: f64) -> CGRect {
        CGRect::new(&CGPoint::new(x, y), &CGSize::new(w, h))
    }

    #[test]
    fn alt_kenara_itilen_imlec_ekranin_son_satirinda_kalir() {
        let main = [screen(0.0, 0.0, 1710.0, 1107.0)];
        let p = keep_on_screen(
            &main,
            CGPoint::new(800.0, 1106.0),
            CGPoint::new(800.0, 1114.0),
        );
        assert_eq!((p.x, p.y), (800.0, 1106.0));
    }

    #[test]
    fn ikinci_monitore_gecis_engellenmez() {
        let screens = [
            screen(0.0, 0.0, 1710.0, 1107.0),
            screen(1710.0, 0.0, 1920.0, 1080.0),
        ];
        let p = keep_on_screen(
            &screens,
            CGPoint::new(1705.0, 500.0),
            CGPoint::new(1720.0, 500.0),
        );
        assert_eq!((p.x, p.y), (1720.0, 500.0));
    }

    #[test]
    fn ekran_icindeki_konum_degismez() {
        let main = [screen(0.0, 0.0, 1710.0, 1107.0)];
        let p = keep_on_screen(&main, CGPoint::new(10.0, 10.0), CGPoint::new(15.0, 12.0));
        assert_eq!((p.x, p.y), (15.0, 12.0));
    }
}

pub fn ui_scale() -> f32 {
    1.0
}
