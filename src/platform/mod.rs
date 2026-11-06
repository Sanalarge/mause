pub struct Capture {
    pub w: usize,
    pub h: usize,
    pub stride: usize,
    pub bgra: Vec<u8>,
}

impl Capture {
    #[cfg(any(target_os = "linux", target_os = "macos", test))]
    pub fn from_bytes(
        w: usize,
        h: usize,
        stride: usize,
        offset: usize,
        bytes: &[u8],
        swap_rb: bool,
    ) -> Option<Self> {
        let packed = capture_len(w, h)?;
        let row = w.checked_mul(4)?;
        if stride < row || stride > 1 << 20 {
            return None;
        }
        let end = offset
            .checked_add((h - 1).checked_mul(stride)?)?
            .checked_add(row)?;
        if end > bytes.len() {
            return None;
        }
        let mut bgra = Vec::with_capacity(packed);
        for y in 0..h {
            let begin = offset + y * stride;
            bgra.extend_from_slice(&bytes[begin..begin + row]);
        }
        if swap_rb {
            for pixel in bgra.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        Some(Self {
            w,
            h,
            stride: row,
            bgra,
        })
    }
}

pub fn capture_len(w: usize, h: usize) -> Option<usize> {
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return None;
    }
    let pixels = w.checked_mul(h)?;
    (pixels <= 16_777_216).then(|| pixels * 4)
}

pub fn capture_fps(fps: u8) {
    #[cfg(target_os = "linux")]
    wayland::set_fps(fps);
    #[cfg(not(target_os = "linux"))]
    let _ = fps;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[cfg(target_os = "linux")]
pub fn desktop() -> Monitor {
    let ms = monitors();
    let (x0, y0) = (
        ms.iter().map(|m| m.x).min().unwrap_or(0),
        ms.iter().map(|m| m.y).min().unwrap_or(0),
    );
    let (x1, y1) = (
        ms.iter().map(|m| m.x + m.w).max().unwrap_or(1),
        ms.iter().map(|m| m.y + m.h).max().unwrap_or(1),
    );
    Monitor {
        name: String::new(),
        x: x0,
        y: y0,
        w: (x1 - x0).max(1),
        h: (y1 - y0).max(1),
    }
}

pub type Ev = (u16, u16, i32);

#[allow(dead_code)]
pub mod ev {
    pub const EV_KEY: u16 = 0x01;
    pub const EV_REL: u16 = 0x02;
    pub const REL_X: u16 = 0x00;
    pub const REL_Y: u16 = 0x01;
    pub const REL_HWHEEL: u16 = 0x06;
    pub const REL_WHEEL: u16 = 0x08;
    pub const REL_WHEEL_HI_RES: u16 = 0x0b;
    pub const REL_HWHEEL_HI_RES: u16 = 0x0c;
    pub const KEY_LEFTCTRL: u16 = 29;
    pub const KEY_LEFTALT: u16 = 56;
    pub const KEY_HOME: u16 = 102;
    pub const BTN_LEFT: u16 = 0x110;
    pub const BTN_RIGHT: u16 = 0x111;
    pub const BTN_MIDDLE: u16 = 0x112;
    pub const BTN_SIDE: u16 = 0x113;
    pub const BTN_EXTRA: u16 = 0x114;
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(test)]
mod capture_tests {
    use super::*;
    #[test]
    fn native_frame_padding_offset_and_color_are_normalized() {
        let bytes = [9, 9, 10, 20, 30, 255, 8, 8, 40, 50, 60, 255];
        let frame = Capture::from_bytes(1, 2, 6, 2, &bytes, true).unwrap();
        assert_eq!(frame.stride, 4);
        assert_eq!(frame.bgra, [30, 20, 10, 255, 60, 50, 40, 255]);
    }
    #[test]
    fn malformed_native_frame_layouts_are_rejected() {
        assert!(capture_len(8192, 8192).is_none());
        assert!(capture_len(usize::MAX, 2).is_none());
        assert!(Capture::from_bytes(2, 2, 7, 0, &[0; 32], false).is_none());
        assert!(Capture::from_bytes(2, 2, 8, usize::MAX, &[0; 32], false).is_none());
        assert!(Capture::from_bytes(2, 2, usize::MAX, 0, &[0; 32], false).is_none());
        assert!(Capture::from_bytes(0, 2, 8, 0, &[0; 32], false).is_none());
        assert!(Capture::from_bytes(2, 2, 8, 0, &[0; 15], false).is_none());
    }
}
