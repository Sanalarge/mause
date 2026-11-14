use crate::config::Side;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq)]
pub enum Effect {
    Hold(f32),
    Leave,
    Arrive,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Spot {
    pub edge: Side,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub scale: f32,
}

impl Spot {
    fn vertical(self) -> bool {
        matches!(self.edge, Side::Left | Side::Right)
    }

    fn px(self, value: f32) -> i32 {
        (value * self.scale.clamp(0.75, 3.0)).round() as i32
    }

    fn k(self) -> f32 {
        self.scale.clamp(0.75, 3.0)
    }
}

const ALONG: f32 = 520.0;
const SMOKE_DEPTH: f32 = 150.0;
const BADGE_GAP: f32 = 40.0;
const BADGE_HEIGHT: f32 = 76.0;
const BADGE_RADIUS: f32 = 20.0;
const ACCENT: (f32, f32, f32) = (255.0, 149.0, 0.0);
const SMOKE_FAR: (f32, f32, f32) = (255.0, 220.0, 178.0);
const CARD: (f32, f32, f32) = (18.0, 24.0, 38.0);
const CARD_EDGE: (f32, f32, f32) = (49.0, 61.0, 84.0);
const INK: (f32, f32, f32) = (245.0, 247.0, 251.0);
const MUTED: (f32, f32, f32) = (167.0, 177.0, 194.0);
const ON_ACCENT: (f32, f32, f32) = (18.0, 18.0, 20.0);

pub const ARRIVE_TIME: Duration = Duration::from_millis(1500);
pub const LEAVE_TIME: Duration = Duration::from_millis(650);
pub const HOLD_LINGER: Duration = Duration::from_millis(380);

static NAME: Mutex<String> = Mutex::new(String::new());

pub fn set_name(name: &str) {
    *NAME.lock().unwrap() = name.to_owned();
}

fn name() -> String {
    let name = NAME.lock().unwrap().clone();
    if name.is_empty() {
        crate::platform::hostname()
    } else {
        name
    }
}

pub fn os_label() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    }
}

fn os_glyph() -> char {
    let logo = if cfg!(target_os = "windows") {
        crate::design::Icon::Windows
    } else if cfg!(target_os = "macos") {
        crate::design::Icon::Apple
    } else {
        crate::design::Icon::Linux
    };
    crate::design::brand_glyph(logo).unwrap_or(' ')
}

pub fn show(effect: Effect, spot: Spot) {
    if cfg!(test) || spot.w <= 0 || spot.h <= 0 {
        return;
    }
    #[cfg(target_os = "windows")]
    overlay::send(effect, spot);
    #[cfg(not(target_os = "windows"))]
    if !matches!(effect, Effect::Hold(_)) {
        crate::platform::edge_flash(spot.x, spot.y, spot.vertical(), spot.w, spot.h);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn frame_for(spot: Spot, with_badge: bool, badge_width: i32) -> Frame {
    let smoke = spot.px(SMOKE_DEPTH);
    let margin = spot.px(BADGE_GAP + 28.0);
    let depth = if with_badge {
        smoke.max(margin + badge_width)
    } else {
        smoke
    };
    let depth_v = if with_badge {
        smoke.max(margin + spot.px(BADGE_HEIGHT))
    } else {
        smoke
    };
    let along = spot.px(ALONG);
    if spot.vertical() {
        let width = depth.min(spot.w);
        let height = along.min(spot.h);
        let y = (spot.y - height / 2).clamp(0, spot.h - height);
        let x = if spot.edge == Side::Left {
            0
        } else {
            spot.w - width
        };
        Frame {
            x,
            y,
            width,
            height,
        }
    } else {
        let width = along.min(spot.w);
        let height = depth_v.min(spot.h);
        let x = (spot.x - width / 2).clamp(0, spot.w - width);
        let y = if spot.edge == Side::Top {
            0
        } else {
            spot.h - height
        };
        Frame {
            x,
            y,
            width,
            height,
        }
    }
}

fn local(spot: Spot, frame: Frame, px: i32, py: i32) -> (f32, f32) {
    let (gx, gy) = (frame.x + px, frame.y + py);
    match spot.edge {
        Side::Left => (gx as f32, (gy - spot.y) as f32),
        Side::Right => ((spot.w - 1 - gx) as f32, (gy - spot.y) as f32),
        Side::Top => (gy as f32, (gx - spot.x) as f32),
        Side::Bottom => ((spot.h - 1 - gy) as f32, (gx - spot.x) as f32),
    }
}

fn premultiplied(color: (f32, f32, f32), alpha: f32) -> u32 {
    let a = alpha.clamp(0.0, 1.0);
    let channel = |c: f32| (c * a).round().clamp(0.0, 255.0) as u32;
    ((a * 255.0).round() as u32) << 24
        | channel(color.0) << 16
        | channel(color.1) << 8
        | channel(color.2)
}

fn over(below: u32, above: u32) -> u32 {
    let aa = (above >> 24) as f32 / 255.0;
    if aa <= 0.0 {
        return below;
    }
    let mix = |shift: u32| {
        let b = ((below >> shift) & 0xFF) as f32;
        let a = ((above >> shift) & 0xFF) as f32;
        (a + b * (1.0 - aa)).round().clamp(0.0, 255.0) as u32
    };
    mix(24) << 24 | mix(16) << 16 | mix(8) << 8 | mix(0)
}

fn lerp(a: (f32, f32, f32), b: (f32, f32, f32), t: f32) -> (f32, f32, f32) {
    let t = t.clamp(0.0, 1.0);
    (
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
    )
}

pub fn paint_smoke(pixels: &mut [u32], spot: Spot, frame: Frame, time: f32, strength: f32) {
    if strength <= 0.001 {
        return;
    }
    let k = spot.k();
    let reach_max = SMOKE_DEPTH * k;
    let spread = ALONG * k * 0.3;
    for py in 0..frame.height {
        for px in 0..frame.width {
            let (depth, along) = local(spot, frame, px, py);
            if depth > reach_max {
                continue;
            }
            let (depth_n, along_n) = (depth / k, along / k);
            let envelope = (-(along / spread).powi(2)).exp();
            if envelope < 0.01 {
                continue;
            }
            let wave = 0.5
                + 0.25 * (along_n * 0.045 + time * 7.0).sin()
                + 0.25 * (along_n * 0.017 - time * 4.3 + depth_n * 0.05).sin();
            let reach = SMOKE_DEPTH * (0.35 + 0.65 * wave) * (0.55 + 0.45 * strength);
            let fall = (-depth_n / (reach * 0.42).max(1.0)).exp();
            let wisp = 0.72 + 0.28 * (depth_n * 0.09 - time * 9.0 + along_n * 0.021).sin();
            let alpha = strength * envelope * fall * wisp * 0.92;
            if alpha < 0.004 {
                continue;
            }
            let color = lerp(ACCENT, SMOKE_FAR, depth_n / SMOKE_DEPTH);
            let i = (py * frame.width + px) as usize;
            pixels[i] = over(pixels[i], premultiplied(color, alpha));
        }
    }
}

pub struct Badge {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u32>,
}

fn rounded_coverage(x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let qx = (x - w / 2.0).abs() - (w / 2.0 - r);
    let qy = (y - h / 2.0).abs() - (h / 2.0 - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
    (0.5 - outside).clamp(0.0, 1.0)
}

struct Text<'a> {
    font: ab_glyph::FontRef<'a>,
    size: f32,
}

impl Text<'_> {
    fn width(&self, text: &str) -> f32 {
        use ab_glyph::{Font, ScaleFont};
        let scaled = self.font.as_scaled(self.size);
        text.chars()
            .map(|c| scaled.h_advance(scaled.glyph_id(c)))
            .sum()
    }

    fn draw(&self, text: &str, x: f32, baseline: f32, mut plot: impl FnMut(i32, i32, f32)) {
        use ab_glyph::{Font, ScaleFont, point};
        let scaled = self.font.as_scaled(self.size);
        let mut pen = x;
        for c in text.chars() {
            let id = scaled.glyph_id(c);
            let glyph = id.with_scale_and_position(self.size, point(pen, baseline));
            pen += scaled.h_advance(id);
            if let Some(outline) = self.font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, coverage| {
                    plot(
                        bounds.min.x as i32 + gx as i32,
                        bounds.min.y as i32 + gy as i32,
                        coverage,
                    )
                });
            }
        }
    }
}

pub fn badge(name: &str, os: &str, glyph: char, scale: f32) -> Option<Badge> {
    let k = scale.clamp(0.75, 3.0);
    let title = Text {
        font: ab_glyph::FontRef::try_from_slice(crate::design::POPPINS_SEMIBOLD).ok()?,
        size: 20.0 * k,
    };
    let subtitle = Text {
        font: ab_glyph::FontRef::try_from_slice(crate::design::POPPINS_SEMIBOLD).ok()?,
        size: 14.0 * k,
    };
    let icon = Text {
        font: ab_glyph::FontRef::try_from_slice(crate::design::BRANDS).ok()?,
        size: 28.0 * k,
    };
    let text_width = title.width(name).max(subtitle.width(os));
    let width = (80.0 * k + text_width + 26.0 * k)
        .ceil()
        .clamp(190.0 * k, 460.0 * k) as i32;
    let height = (BADGE_HEIGHT * k).round() as i32;
    let radius = BADGE_RADIUS * k;
    let center_x = 40.0 * k;
    let circle_radius = 24.0 * k;
    let mut pixels = vec![0u32; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let outer = rounded_coverage(fx, fy, width as f32, height as f32, radius);
            if outer <= 0.0 {
                continue;
            }
            let inner = rounded_coverage(
                fx - 1.0,
                fy - 1.0,
                width as f32 - 2.0,
                height as f32 - 2.0,
                radius - 1.0,
            );
            let border = premultiplied(CARD_EDGE, outer);
            pixels[(y * width + x) as usize] = over(border, premultiplied(CARD, inner * 0.97));
            let dx = fx - center_x;
            let dy = fy - height as f32 / 2.0;
            let circle = (circle_radius + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            if circle > 0.0 {
                let i = (y * width + x) as usize;
                pixels[i] = over(pixels[i], premultiplied(ACCENT, circle));
            }
        }
    }
    let glyph_text = glyph.to_string();
    let glyph_width = icon.width(&glyph_text);
    icon.draw(
        &glyph_text,
        center_x - glyph_width / 2.0,
        height as f32 / 2.0 + 10.0 * k,
        |x, y, c| blend_at(&mut pixels, width, height, x, y, ON_ACCENT, c),
    );
    title.draw(name, 78.0 * k, 35.0 * k, |x, y, c| {
        blend_at(&mut pixels, width, height, x, y, INK, c)
    });
    subtitle.draw(os, 78.0 * k, 57.0 * k, |x, y, c| {
        blend_at(&mut pixels, width, height, x, y, MUTED, c)
    });
    Some(Badge {
        width,
        height,
        pixels,
    })
}

fn blend_at(
    pixels: &mut [u32],
    width: i32,
    height: i32,
    x: i32,
    y: i32,
    color: (f32, f32, f32),
    coverage: f32,
) {
    if x < 0 || y < 0 || x >= width || y >= height {
        return;
    }
    let i = (y * width + x) as usize;
    pixels[i] = over(pixels[i], premultiplied(color, coverage));
}

pub fn paint_badge(
    pixels: &mut [u32],
    spot: Spot,
    frame: Frame,
    badge: &Badge,
    alpha: f32,
    slide: f32,
) {
    if alpha <= 0.001 {
        return;
    }
    let inset = (BADGE_GAP - slide) * spot.k();
    let (left, top) = match spot.edge {
        Side::Left => (inset as i32, spot.y - frame.y - badge.height / 2),
        Side::Right => (
            frame.width - inset as i32 - badge.width,
            spot.y - frame.y - badge.height / 2,
        ),
        Side::Top => (spot.x - frame.x - badge.width / 2, inset as i32),
        Side::Bottom => (
            spot.x - frame.x - badge.width / 2,
            frame.height - inset as i32 - badge.height,
        ),
    };
    let left = left.clamp(0, (frame.width - badge.width).max(0));
    let top = top.clamp(0, (frame.height - badge.height).max(0));
    for by in 0..badge.height {
        let y = top + by;
        if y < 0 || y >= frame.height {
            continue;
        }
        for bx in 0..badge.width {
            let x = left + bx;
            if x < 0 || x >= frame.width {
                continue;
            }
            let source = badge.pixels[(by * badge.width + bx) as usize];
            let scaled = scale_alpha(source, alpha);
            let i = (y * frame.width + x) as usize;
            pixels[i] = over(pixels[i], scaled);
        }
    }
}

fn scale_alpha(color: u32, alpha: f32) -> u32 {
    let a = alpha.clamp(0.0, 1.0);
    let part = |shift: u32| ((((color >> shift) & 0xFF) as f32) * a).round() as u32;
    part(24) << 24 | part(16) << 16 | part(8) << 8 | part(0)
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn arrive_levels(elapsed: f32) -> (f32, f32, f32) {
    let smoke = if elapsed < 0.12 {
        ease(elapsed / 0.12)
    } else {
        1.0 - ease((elapsed - 0.3) / 0.6)
    };
    let badge = if elapsed < 0.18 {
        ease(elapsed / 0.18)
    } else {
        1.0 - ease((elapsed - 1.15) / 0.35)
    };
    let slide = 10.0 * (1.0 - ease(elapsed / 0.25));
    (smoke.clamp(0.0, 1.0), badge.clamp(0.0, 1.0), slide)
}

pub fn leave_levels(elapsed: f32) -> f32 {
    if elapsed < 0.08 {
        ease(elapsed / 0.08)
    } else {
        (1.0 - ease((elapsed - 0.12) / 0.5)).clamp(0.0, 1.0)
    }
}

pub fn hold_level(progress: f32) -> f32 {
    0.22 + 0.78 * progress.clamp(0.0, 1.0)
}

#[cfg(target_os = "windows")]
mod overlay {
    use super::*;
    use std::mem::{size_of, zeroed};
    use std::ptr::{null, null_mut};
    use std::sync::OnceLock;
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
    use windows_sys::Win32::Graphics::Gdi::{
        AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
        CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC,
        ReleaseDC, SelectObject,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetSystemMetrics, MSG, PM_REMOVE,
        PeekMessageW, RegisterClassW, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE,
        SW_SHOWNOACTIVATE, ShowWindow, TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WNDCLASSW,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
        WS_POPUP,
    };

    const TICK: Duration = Duration::from_millis(16);

    static CHANNEL: OnceLock<Mutex<Sender<(Effect, Spot)>>> = OnceLock::new();

    pub fn send(effect: Effect, spot: Spot) {
        let sender = CHANNEL.get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || run(rx));
            Mutex::new(tx)
        });
        let _ = sender.lock().unwrap().send((effect, spot));
    }

    struct Window(HWND);

    impl Window {
        fn create() -> Option<Self> {
            let class: Vec<u16> = "MauseEdgeEffect\0".encode_utf16().collect();
            unsafe {
                let instance = GetModuleHandleW(null());
                let mut wc: WNDCLASSW = zeroed();
                wc.lpfnWndProc = Some(DefWindowProcW);
                wc.hInstance = instance;
                wc.lpszClassName = class.as_ptr();
                RegisterClassW(&wc);
                let hwnd = CreateWindowExW(
                    WS_EX_LAYERED
                        | WS_EX_TRANSPARENT
                        | WS_EX_TOPMOST
                        | WS_EX_TOOLWINDOW
                        | WS_EX_NOACTIVATE,
                    class.as_ptr(),
                    null(),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    null_mut(),
                    null_mut(),
                    instance,
                    null(),
                );
                (!hwnd.is_null()).then_some(Self(hwnd))
            }
        }

        fn present(&self, frame: Frame, pixels: &[u32]) {
            unsafe {
                let screen = GetDC(null_mut());
                let memory = CreateCompatibleDC(screen);
                let mut info: BITMAPINFO = zeroed();
                info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
                info.bmiHeader.biWidth = frame.width;
                info.bmiHeader.biHeight = -frame.height;
                info.bmiHeader.biPlanes = 1;
                info.bmiHeader.biBitCount = 32;
                info.bmiHeader.biCompression = BI_RGB;
                let mut bits: *mut core::ffi::c_void = null_mut();
                let bitmap =
                    CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
                if !bitmap.is_null() && !bits.is_null() {
                    std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u32, pixels.len());
                    let previous = SelectObject(memory, bitmap);
                    let destination = POINT {
                        x: frame.x + GetSystemMetrics(SM_XVIRTUALSCREEN),
                        y: frame.y + GetSystemMetrics(SM_YVIRTUALSCREEN),
                    };
                    let size = SIZE {
                        cx: frame.width,
                        cy: frame.height,
                    };
                    let source = POINT { x: 0, y: 0 };
                    let blend = BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: 255,
                        AlphaFormat: AC_SRC_ALPHA as u8,
                    };
                    UpdateLayeredWindow(
                        self.0,
                        screen,
                        &destination,
                        &size,
                        memory,
                        &source,
                        0,
                        &blend,
                        ULW_ALPHA,
                    );
                    ShowWindow(self.0, SW_SHOWNOACTIVATE);
                    SelectObject(memory, previous);
                    DeleteObject(bitmap);
                }
                DeleteDC(memory);
                ReleaseDC(null_mut(), screen);
                self.pump();
            }
        }

        fn hide(&self) {
            unsafe {
                ShowWindow(self.0, SW_HIDE);
            }
            self.pump();
        }

        fn pump(&self) {
            unsafe {
                let mut msg: MSG = zeroed();
                while PeekMessageW(&mut msg, self.0, 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }

    struct Animation {
        effect: Effect,
        spot: Spot,
        started: Instant,
        updated: Instant,
        badge: Option<Badge>,
    }

    impl Animation {
        fn new(effect: Effect, spot: Spot) -> Self {
            let badge = (effect == Effect::Arrive)
                .then(|| super::badge(&name(), os_label(), os_glyph(), spot.scale))
                .flatten();
            let now = Instant::now();
            Self {
                effect,
                spot,
                started: now,
                updated: now,
                badge,
            }
        }

        fn render(&self, now: Instant) -> Option<(Frame, Vec<u32>)> {
            let elapsed = now.saturating_duration_since(self.started);
            let time = elapsed.as_secs_f32();
            let badge_width = self.badge.as_ref().map_or(0, |b| b.width);
            let frame = frame_for(self.spot, self.badge.is_some(), badge_width);
            let mut pixels = vec![0u32; (frame.width * frame.height) as usize];
            match self.effect {
                Effect::Hold(progress) => {
                    let idle = now.saturating_duration_since(self.updated);
                    if idle > HOLD_LINGER + Duration::from_millis(260) {
                        return None;
                    }
                    let fade = if idle > HOLD_LINGER {
                        1.0 - (idle - HOLD_LINGER).as_secs_f32() / 0.26
                    } else {
                        1.0
                    };
                    paint_smoke(
                        &mut pixels,
                        self.spot,
                        frame,
                        time,
                        hold_level(progress) * fade,
                    );
                }
                Effect::Leave => {
                    if elapsed > LEAVE_TIME {
                        return None;
                    }
                    paint_smoke(&mut pixels, self.spot, frame, time, leave_levels(time));
                }
                Effect::Arrive => {
                    if elapsed > ARRIVE_TIME {
                        return None;
                    }
                    let (smoke, badge_alpha, slide) = arrive_levels(time);
                    paint_smoke(&mut pixels, self.spot, frame, time, smoke);
                    if let Some(badge) = &self.badge {
                        paint_badge(&mut pixels, self.spot, frame, badge, badge_alpha, slide);
                    }
                }
            }
            Some((frame, pixels))
        }
    }

    fn run(rx: Receiver<(Effect, Spot)>) {
        let Some(window) = Window::create() else {
            return;
        };
        let mut current: Option<Animation> = None;
        loop {
            let message = if current.is_some() {
                match rx.recv_timeout(TICK) {
                    Ok(message) => Some(message),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            } else {
                match rx.recv() {
                    Ok(message) => Some(message),
                    Err(_) => return,
                }
            };
            if let Some((effect, spot)) = message {
                match (&mut current, effect) {
                    (Some(animation), Effect::Hold(_))
                        if matches!(animation.effect, Effect::Hold(_))
                            && animation.spot.edge == spot.edge =>
                    {
                        animation.effect = effect;
                        animation.spot = spot;
                        animation.updated = Instant::now();
                    }
                    _ => current = Some(Animation::new(effect, spot)),
                }
            }
            let rendered = current
                .as_ref()
                .and_then(|animation| animation.render(Instant::now()));
            match rendered {
                Some((frame, pixels)) => window.present(frame, &pixels),
                None => {
                    if current.take().is_some() {
                        window.hide();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(edge: Side) -> Spot {
        Spot {
            edge,
            x: 960,
            y: 540,
            w: 1920,
            h: 1080,
            scale: 1.0,
        }
    }

    #[test]
    fn effect_frame_hugs_the_crossed_edge() {
        let right = frame_for(spot(Side::Right), false, 0);
        assert_eq!(right.x + right.width, 1920);
        assert_eq!(right.height, ALONG as i32);
        let top = frame_for(spot(Side::Top), false, 0);
        assert_eq!(top.y, 0);
        assert_eq!(top.width, ALONG as i32);
        let left = frame_for(
            Spot {
                y: 5,
                ..spot(Side::Left)
            },
            false,
            0,
        );
        assert_eq!((left.x, left.y), (0, 0));
    }

    #[test]
    fn smoke_is_strongest_at_the_edge() {
        let s = spot(Side::Left);
        let frame = frame_for(s, false, 0);
        let mut pixels = vec![0u32; (frame.width * frame.height) as usize];
        paint_smoke(&mut pixels, s, frame, 0.0, 1.0);
        let row = (s.y - frame.y) * frame.width;
        let alpha = |x: i32| pixels[(row + x) as usize] >> 24;
        assert!(alpha(0) > alpha(60));
        assert!(alpha(60) >= alpha(frame.width - 1));
    }

    #[test]
    fn badge_shows_name_and_fades_after_arrival() {
        let card = badge("DEROOT", "Windows", '\u{f17a}', 1.0).unwrap();
        assert!(card.width >= 190 && card.height == BADGE_HEIGHT as i32);
        let large = badge("DEROOT", "Windows", '\u{f17a}', 1.5).unwrap();
        assert!(large.height > card.height && large.width > card.width);
        assert!(card.pixels.iter().any(|p| p >> 24 == 255));
        let (_, start, _) = arrive_levels(0.0);
        let (_, middle, _) = arrive_levels(0.6);
        let (_, end, _) = arrive_levels(ARRIVE_TIME.as_secs_f32());
        assert!(start < 0.05 && middle > 0.95 && end < 0.05);
    }

    #[test]
    fn hold_glow_grows_with_progress() {
        assert!(hold_level(0.0) < hold_level(0.5));
        assert!(hold_level(0.5) < hold_level(1.0));
    }
}

#[cfg(test)]
mod preview {
    use super::*;

    fn save(path: &str, frame: Frame, pixels: &[u32]) {
        let mut rgb = Vec::with_capacity(pixels.len() * 3);
        for p in pixels {
            let a = (p >> 24) as f32 / 255.0;
            let bg = [13.0, 17.0, 28.0];
            for (i, shift) in [16, 8, 0].into_iter().enumerate() {
                let c = ((p >> shift) & 0xFF) as f32 + bg[i] * (1.0 - a);
                rgb.push(c.round().clamp(0.0, 255.0) as u8);
            }
        }
        let mut jpeg = Vec::new();
        jpeg_encoder::Encoder::new(&mut jpeg, 92)
            .encode(
                &rgb,
                frame.width as u16,
                frame.height as u16,
                jpeg_encoder::ColorType::Rgb,
            )
            .unwrap();
        std::fs::write(path, jpeg).unwrap();
    }

    #[test]
    #[ignore]
    fn write_preview_frames() {
        let dir = std::env::var("MAUSE_FX_PREVIEW").unwrap();
        let spot = Spot {
            edge: Side::Left,
            x: 0,
            y: 540,
            w: 1920,
            h: 1080,
            scale: 1.0,
        };
        let card = badge("SERDAR BUSSINENS", "macOS", '\u{f179}', 1.0).unwrap();
        for (label, time) in [("arrive1", 0.1f32), ("arrive2", 0.45), ("arrive3", 1.0)] {
            let frame = frame_for(spot, true, card.width);
            let mut pixels = vec![0u32; (frame.width * frame.height) as usize];
            let (smoke, alpha, slide) = arrive_levels(time);
            paint_smoke(&mut pixels, spot, frame, time, smoke);
            paint_badge(&mut pixels, spot, frame, &card, alpha, slide);
            save(&format!("{dir}/{label}.jpg"), frame, &pixels);
        }
        for (label, name, os, glyph) in [
            ("card-windows", "DEROOT", "Windows", '\u{f17a}'),
            ("card-linux", "UBUNTU-PC", "Linux", '\u{f17c}'),
            ("card-android", "Pixel 8", "Android", '\u{f17b}'),
        ] {
            let card = badge(name, os, glyph, 1.0).unwrap();
            let frame = Frame {
                x: 0,
                y: 0,
                width: card.width,
                height: card.height,
            };
            save(&format!("{dir}/{label}.jpg"), frame, &card.pixels);
        }
        for (label, progress) in [("hold30", 0.3f32), ("hold90", 0.9)] {
            let right = Spot {
                edge: Side::Right,
                x: 1919,
                ..spot
            };
            let frame = frame_for(right, false, 0);
            let mut pixels = vec![0u32; (frame.width * frame.height) as usize];
            paint_smoke(&mut pixels, right, frame, 0.7, hold_level(progress));
            save(&format!("{dir}/{label}.jpg"), frame, &pixels);
        }
    }
}
