use super::{Ev, ev};
use std::collections::HashSet;
use std::io;
use std::mem::{size_of, zeroed};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK,
    MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, SendInput,
};
use windows_sys::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT,
    RIDEV_INPUTSINK, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE, RegisterRawInputDevices,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
    GetSystemMetrics, HC_ACTION, HHOOK, HWND_MESSAGE, KBDLLHOOKSTRUCT, LLKHF_INJECTED,
    LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, RegisterClassW, SM_CXSCREEN, SM_CXVIRTUALSCREEN,
    SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SetCursorPos,
    SetProcessDPIAware, SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_INPUT,
    WNDCLASSW,
};

struct PhysicalPixels(windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT);
impl PhysicalPixels {
    fn enter() -> Self {
        use windows_sys::Win32::UI::HiDpi::*;
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}
impl Drop for PhysicalPixels {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows_sys::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(self.0);
            }
        }
    }
}

const XBUTTON1: u32 = 1;
const XBUTTON2: u32 = 2;

const EXT: [(u16, u16); 18] = [
    (96, 0x1C),
    (97, 0x1D),
    (98, 0x35),
    (99, 0x37),
    (100, 0x38),
    (102, 0x47),
    (103, 0x48),
    (104, 0x49),
    (105, 0x4B),
    (106, 0x4D),
    (107, 0x4F),
    (108, 0x50),
    (109, 0x51),
    (110, 0x52),
    (111, 0x53),
    (125, 0x5B),
    (126, 0x5C),
    (127, 0x5D),
];

fn scan_to_evdev(sc: u16, ext: bool) -> Option<u16> {
    if ext {
        EXT.iter().find(|(_, s)| *s == sc).map(|(e, _)| *e)
    } else if (1..=0x58).contains(&sc) {
        Some(sc)
    } else {
        None
    }
}

fn evdev_to_scan(code: u16) -> Option<(u16, bool)> {
    if let Some((_, s)) = EXT.iter().find(|(e, _)| *e == code) {
        Some((*s, true))
    } else if (1..=88).contains(&code) {
        Some((code, false))
    } else {
        None
    }
}

pub fn init() {
    unsafe {
        use windows_sys::Win32::UI::HiDpi::*;
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) == 0 {
            SetProcessDPIAware();
        }
    }
}

pub struct Injector {
    down: HashSet<u16>,
    posted: Option<((i32, i32), Instant)>,
}

const POSTED_FRESH: Duration = Duration::from_millis(120);

fn key_input(sc: u16, ext: bool, up: bool) -> INPUT {
    let mut flags = KEYEVENTF_SCANCODE;
    if ext {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: sc,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn mouse_input(dx: i32, dy: i32, data: u32, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

struct Pointer {
    pos: Option<(i32, i32)>,
    desk: (i32, i32, i32, i32),
    moved: bool,
}

impl Pointer {
    fn at(pos: Option<(i32, i32)>) -> Self {
        let desk = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        Self {
            pos,
            desk,
            moved: false,
        }
    }

    fn move_by(&mut self, dx: i32, dy: i32) -> INPUT {
        let (left, top, width, height) = self.desk;
        let Some((x, y)) = self.pos.filter(|_| width > 1 && height > 1) else {
            return mouse_input(dx, dy, 0, MOUSEEVENTF_MOVE);
        };
        let x = (x + dx).clamp(left, left + width - 1);
        let y = (y + dy).clamp(top, top + height - 1);
        self.pos = Some((x, y));
        self.moved = true;
        let normalize = |v: i32, start: i32, size: i32| {
            ((v - start) as i64 * 65536 / size as i64 + 1).min(65535) as i32
        };
        mouse_input(
            normalize(x, left, width),
            normalize(y, top, height),
            0,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        )
    }
}

fn send(inputs: &[INPUT]) -> io::Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    let n = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if n as usize == inputs.len() {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

impl Injector {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            down: HashSet::new(),
            posted: None,
        })
    }

    fn button(code: u16, down: bool) -> Option<INPUT> {
        let (flags, data) = match (code, down) {
            (ev::BTN_LEFT, true) => (MOUSEEVENTF_LEFTDOWN, 0),
            (ev::BTN_LEFT, false) => (MOUSEEVENTF_LEFTUP, 0),
            (ev::BTN_RIGHT, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
            (ev::BTN_RIGHT, false) => (MOUSEEVENTF_RIGHTUP, 0),
            (ev::BTN_MIDDLE, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
            (ev::BTN_MIDDLE, false) => (MOUSEEVENTF_MIDDLEUP, 0),
            (ev::BTN_SIDE, true) => (MOUSEEVENTF_XDOWN, XBUTTON1),
            (ev::BTN_SIDE, false) => (MOUSEEVENTF_XUP, XBUTTON1),
            (ev::BTN_EXTRA, true) => (MOUSEEVENTF_XDOWN, XBUTTON2),
            (ev::BTN_EXTRA, false) => (MOUSEEVENTF_XUP, XBUTTON2),
            _ => return None,
        };
        Some(mouse_input(0, 0, data, flags))
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
        let _dpi = PhysicalPixels::enter();
        let mut out: Vec<INPUT> = Vec::new();
        let (mut dx, mut dy) = (0i32, 0i32);
        let base = match self.posted {
            Some((p, at)) if at.elapsed() < POSTED_FRESH => Some(p),
            _ => cursor_pos(),
        };
        let mut pointer = Pointer::at(base);
        let mut flush = |out: &mut Vec<INPUT>, dx: &mut i32, dy: &mut i32| {
            if *dx != 0 || *dy != 0 {
                out.push(pointer.move_by(*dx, *dy));
                (*dx, *dy) = (0, 0);
            }
        };
        for &(t, c, v) in evs {
            if t == ev::EV_REL {
                match c {
                    ev::REL_X => dx += v,
                    ev::REL_Y => dy += v,
                    ev::REL_WHEEL_HI_RES => {
                        flush(&mut out, &mut dx, &mut dy);
                        out.push(mouse_input(0, 0, v as u32, MOUSEEVENTF_WHEEL));
                    }
                    ev::REL_HWHEEL_HI_RES => {
                        flush(&mut out, &mut dx, &mut dy);
                        out.push(mouse_input(0, 0, v as u32, MOUSEEVENTF_HWHEEL));
                    }
                    _ => {}
                }
            } else if t == ev::EV_KEY && v != 2 {
                flush(&mut out, &mut dx, &mut dy);
                let down = v != 0;
                if down {
                    self.down.insert(c);
                } else {
                    self.down.remove(&c);
                }
                if (ev::BTN_LEFT..=ev::BTN_EXTRA).contains(&c) {
                    out.extend(Self::button(c, down));
                } else if let Some((sc, ext)) = evdev_to_scan(c) {
                    out.push(key_input(sc, ext, !down));
                }
            }
        }
        flush(&mut out, &mut dx, &mut dy);
        if pointer.moved {
            self.posted = pointer.pos.map(|p| (p, Instant::now()));
        }
        send(&out)
    }

    pub fn warp(&mut self, x: i32, y: i32, w: i32, h: i32) -> io::Result<()> {
        self.point(x.clamp(0, (w - 1).max(0)), y.clamp(0, (h - 1).max(0)))
    }

    pub fn point(&mut self, x: i32, y: i32) -> io::Result<()> {
        self.posted = None;
        let _dpi = PhysicalPixels::enter();
        if unsafe { SetCursorPos(x, y) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
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

pub fn grab(on: bool) {
    GRABBED.store(on, Ordering::Relaxed);
}

unsafe extern "system" fn kb_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && GRABBED.load(Ordering::Relaxed) {
        let k = unsafe { &*(l as *const KBDLLHOOKSTRUCT) };
        if k.flags & LLKHF_INJECTED == 0 {
            return 1;
        }
    }
    unsafe { CallNextHookEx(null_mut(), code, w, l) }
}

unsafe extern "system" fn mouse_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && GRABBED.load(Ordering::Relaxed) {
        let m = unsafe { &*(l as *const MSLLHOOKSTRUCT) };
        if m.flags & LLMHF_INJECTED == 0 {
            return 1;
        }
    }
    unsafe { CallNextHookEx(null_mut(), code, w, l) }
}

fn handle_raw(l: LPARAM) {
    let mut size: u32 = 0;
    let hdr = size_of::<RAWINPUTHEADER>() as u32;
    unsafe { GetRawInputData(l as HRAWINPUT, RID_INPUT, null_mut(), &mut size, hdr) };
    if size == 0 {
        return;
    }
    let mut buf = vec![0u8; size as usize];
    let got = unsafe {
        GetRawInputData(
            l as HRAWINPUT,
            RID_INPUT,
            buf.as_mut_ptr().cast(),
            &mut size,
            hdr,
        )
    };
    if got == u32::MAX || (got as usize) < size_of::<RAWINPUTHEADER>() {
        return;
    }
    let raw = unsafe { &*(buf.as_ptr() as *const RAWINPUT) };
    if raw.header.hDevice.is_null() {
        return;
    }
    let mut evs: Vec<Ev> = Vec::new();
    if raw.header.dwType == RIM_TYPEMOUSE {
        let m = unsafe { &raw.data.mouse };
        if m.usFlags & 1 == 0 {
            if m.lLastX != 0 {
                evs.push((ev::EV_REL, ev::REL_X, m.lLastX));
            }
            if m.lLastY != 0 {
                evs.push((ev::EV_REL, ev::REL_Y, m.lLastY));
            }
        }
        let (flags, data) = unsafe {
            (
                m.Anonymous.Anonymous.usButtonFlags,
                m.Anonymous.Anonymous.usButtonData,
            )
        };
        const BTNS: [(u16, u16, u16); 5] = [
            (0x0001, 0x0002, ev::BTN_LEFT),
            (0x0004, 0x0008, ev::BTN_RIGHT),
            (0x0010, 0x0020, ev::BTN_MIDDLE),
            (0x0040, 0x0080, ev::BTN_SIDE),
            (0x0100, 0x0200, ev::BTN_EXTRA),
        ];
        for (down, up, code) in BTNS {
            if flags & down != 0 {
                evs.push((ev::EV_KEY, code, 1));
            }
            if flags & up != 0 {
                evs.push((ev::EV_KEY, code, 0));
            }
        }
        let delta = data as i16 as i32;
        if flags & 0x0400 != 0 {
            let notch = if delta == 0 {
                0
            } else if delta.abs() < 120 {
                delta.signum()
            } else {
                delta / 120
            };
            evs.push((ev::EV_REL, ev::REL_WHEEL, notch));
            evs.push((ev::EV_REL, ev::REL_WHEEL_HI_RES, delta));
        }
        if flags & 0x0800 != 0 {
            evs.push((ev::EV_REL, ev::REL_HWHEEL, delta.signum()));
            evs.push((ev::EV_REL, ev::REL_HWHEEL_HI_RES, delta));
        }
    } else if raw.header.dwType == RIM_TYPEKEYBOARD {
        let k = unsafe { &raw.data.keyboard };
        if k.MakeCode == 0 || k.MakeCode == 0xFF || k.Flags & 4 != 0 {
            return;
        }
        let ext = k.Flags & 2 != 0;
        if ext && k.MakeCode == 0x2A {
            return;
        }
        if let Some(code) = scan_to_evdev(k.MakeCode, ext) {
            let up = k.Flags & 1 != 0;
            evs.push((ev::EV_KEY, code, if up { 0 } else { 1 }));
        }
    }
    if !evs.is_empty() {
        if let Some(cb) = CALLBACK.get() {
            cb(evs);
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_INPUT {
        handle_raw(l);
    }
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn start_capture(cb: fn(Vec<Ev>)) -> io::Result<()> {
    let mut started = STARTED.lock().unwrap();
    if *started {
        return Ok(());
    }
    let _ = CALLBACK.set(cb);
    let (tx, rx) = std::sync::mpsc::channel::<io::Result<()>>();
    thread::spawn(move || unsafe {
        let hinst = GetModuleHandleW(null());
        let class = wide("MauseRawInput");
        let mut wc: WNDCLASSW = zeroed();
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = hinst;
        wc.lpszClassName = class.as_ptr();
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            hinst,
            null(),
        );
        if hwnd.is_null() {
            let _ = tx.send(Err(io::Error::last_os_error()));
            return;
        }
        let devs = [
            RAWINPUTDEVICE {
                usUsagePage: 1,
                usUsage: 2,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
            RAWINPUTDEVICE {
                usUsagePage: 1,
                usUsage: 6,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
        ];
        if RegisterRawInputDevices(devs.as_ptr(), 2, size_of::<RAWINPUTDEVICE>() as u32) == 0 {
            let _ = tx.send(Err(io::Error::last_os_error()));
            return;
        }
        let _kb: HHOOK = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_hook), hinst, 0);
        let _ms: HHOOK = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), hinst, 0);
        let _ = tx.send(Ok(()));
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
    let r = rx
        .recv()
        .unwrap_or_else(|_| Err(io::Error::other("yakalama thread'i başlamadı")));
    if r.is_ok() {
        *started = true;
    }
    r
}

pub fn check_permissions() -> (bool, bool) {
    (true, true)
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
        "Windows'ta ek izin gerekmez. Yönetici olarak çalışan pencerelere giriş gönderebilmek için Mause'u da yönetici olarak çalıştırın.",
        "",
    )
}

pub fn screen_capture_begin() -> bool {
    capture_screen().is_some()
}

pub fn screen_capture_begin_virtual(_: (u16, u16)) -> bool {
    false
}

pub fn screen_capture_error() -> Option<String> {
    Some("Windows masaüstü yakalanamadı. Açık kullanıcı oturumunu ve ekranı kontrol edin; UAC güvenli masaüstü ve oturum açma ekranı bu modda desteklenmiyor.".into())
}

pub fn screen_capture_pending() -> bool {
    false
}

pub fn screen_cursor(_: bool) {}

pub fn monitors() -> Vec<super::Monitor> {
    let _dpi = PhysicalPixels::enter();
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
    };
    use windows_sys::core::BOOL;
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let out = unsafe { &mut *(data as *mut Vec<(bool, super::Monitor)>) };
        let mut info: MONITORINFOEXW = unsafe { zeroed() };
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(m, &mut info as *mut _ as *mut _) } != 0 {
            let r = info.monitorInfo.rcMonitor;
            let n = info
                .szDevice
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(info.szDevice.len());
            let name = String::from_utf16_lossy(&info.szDevice[..n])
                .trim_start_matches(r"\\.\")
                .to_string();
            out.push((
                info.monitorInfo.dwFlags & 1 != 0,
                super::Monitor {
                    name,
                    x: r.left,
                    y: r.top,
                    w: r.right - r.left,
                    h: r.bottom - r.top,
                },
            ));
        }
        1
    }
    let mut v: Vec<(bool, super::Monitor)> = Vec::new();
    unsafe {
        EnumDisplayMonitors(null_mut(), null(), Some(each), &mut v as *mut _ as LPARAM);
    }
    v.sort_by_key(|(primary, _)| !primary);
    let mut out: Vec<super::Monitor> = v.into_iter().map(|(_, m)| m).collect();
    if out.is_empty() {
        let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        out.push(super::Monitor {
            name: String::new(),
            x: 0,
            y: 0,
            w,
            h,
        });
    }
    out
}

pub fn cursor_pos() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x, p.y))
}

pub fn mounts() -> Vec<(String, PathBuf)> {
    (b'A'..=b'Z')
        .map(|c| format!("{}:\\", c as char))
        .filter(|d| Path::new(d).exists())
        .map(|d| (format!("Disk {}", &d[..2]), PathBuf::from(d)))
        .collect()
}

pub fn capture_screen() -> Option<super::Capture> {
    capture_monitor(0).map(|(c, _)| c)
}

pub fn capture_monitor(i: usize) -> Option<(super::Capture, super::Monitor)> {
    let ms = monitors();
    let m = ms.get(i).or(ms.first())?.clone();
    capture_rect(&m).map(|c| (c, m))
}

fn capture_rect(m: &super::Monitor) -> Option<super::Capture> {
    let _dpi = PhysicalPixels::enter();
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
        CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
        SRCCOPY, SelectObject,
    };
    unsafe {
        let (w, h) = (m.w, m.h);
        if w <= 0 || h <= 0 {
            return None;
        }
        let len = super::capture_len(w as usize, h as usize)?;
        let screen = GetDC(null_mut());
        if screen.is_null() {
            return None;
        }
        let mem = CreateCompatibleDC(screen);
        if mem.is_null() {
            ReleaseDC(null_mut(), screen);
            return None;
        }
        let bmp = CreateCompatibleBitmap(screen, w, h);
        if bmp.is_null() {
            DeleteDC(mem);
            ReleaseDC(null_mut(), screen);
            return None;
        }
        let old = SelectObject(mem, bmp);
        if old.is_null() || old as isize == -1 {
            DeleteObject(bmp);
            DeleteDC(mem);
            ReleaseDC(null_mut(), screen);
            return None;
        }
        let copied = BitBlt(mem, 0, 0, w, h, screen, m.x, m.y, SRCCOPY | CAPTUREBLT) != 0;
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = w;
        info.bmiHeader.biHeight = -h;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bgra = vec![0u8; len];
        SelectObject(mem, old);
        let lines = GetDIBits(
            mem,
            bmp,
            0,
            h as u32,
            bgra.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );
        DeleteObject(bmp);
        DeleteDC(mem);
        ReleaseDC(null_mut(), screen);
        let (w, h) = (w as usize, h as usize);
        (copied && lines as usize == h).then_some(super::Capture {
            w,
            h,
            stride: w * 4,
            bgra,
        })
    }
}

pub fn permission_panes() -> Vec<&'static str> {
    Vec::new()
}

pub fn open_permission_pane(_: usize) {}

pub fn detect_screen() -> Option<(i32, i32)> {
    let _dpi = PhysicalPixels::enter();
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    (w > 0 && h > 0).then_some((w, h))
}

#[cfg(not(test))]
pub fn notify(_title: &str, _body: &str) {}

pub fn open_path(p: &Path) {
    let _ = Command::new("explorer.exe").arg(p).spawn();
}

pub fn downloads_dir() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads")
}

pub fn config_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

pub(super) fn flash_window(x: i32, y: i32, w: i32, h: i32) {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateSolidBrush, DeleteObject, InvalidateRect, UpdateWindow,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyWindow, GCLP_HBRBACKGROUND, LWA_ALPHA, MSG, PM_REMOVE, PeekMessageW,
        SW_SHOWNOACTIVATE, SetClassLongPtrW, SetLayeredWindowAttributes, ShowWindow, WNDCLASSW,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
        WS_POPUP,
    };
    if w <= 0 || h <= 0 {
        return;
    }
    let bgr = |c: u32| ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF);
    let class: Vec<u16> = "MauseEdgeFlash\0".encode_utf16().collect();
    unsafe {
        let inst = GetModuleHandleW(null());
        let brushes = [
            CreateSolidBrush(bgr(super::FLASH_COLORS[0])),
            CreateSolidBrush(bgr(super::FLASH_COLORS[1])),
        ];
        let mut wc: WNDCLASSW = zeroed();
        wc.lpfnWndProc = Some(DefWindowProcW);
        wc.hInstance = inst;
        wc.lpszClassName = class.as_ptr();
        wc.hbrBackground = brushes[0];
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED,
            class.as_ptr(),
            null(),
            WS_POPUP,
            x + GetSystemMetrics(SM_XVIRTUALSCREEN),
            y + GetSystemMetrics(SM_YVIRTUALSCREEN),
            w,
            h,
            null_mut(),
            null_mut(),
            inst,
            null(),
        );
        if !hwnd.is_null() {
            SetLayeredWindowAttributes(hwnd, 0, 230, LWA_ALPHA);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            for step in 0..super::FLASH_STEPS {
                SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, brushes[step as usize % 2] as _);
                InvalidateRect(hwnd, null(), 1);
                UpdateWindow(hwnd);
                let until = Instant::now() + Duration::from_millis(super::FLASH_STEP_MS);
                while Instant::now() < until {
                    let mut msg: MSG = zeroed();
                    while PeekMessageW(&mut msg, hwnd, 0, 0, PM_REMOVE) != 0 {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
            DestroyWindow(hwnd);
        }
        for b in brushes {
            DeleteObject(b as _);
        }
    }
}
