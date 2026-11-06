use crate::hardware;
use crate::screen::View;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const LABEL: &str = "Mause Telefon Kamerasi";

pub fn rgb_to_yuyv(width: usize, height: usize, rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(width * height * 2);
    let luma = |r: i32, g: i32, b: i32| ((299 * r + 587 * g + 114 * b) / 1000).clamp(0, 255) as u8;
    for row in 0..height {
        for pair in 0..width / 2 {
            let at = (row * width + pair * 2) * 3;
            let (r0, g0, b0) = (rgb[at] as i32, rgb[at + 1] as i32, rgb[at + 2] as i32);
            let (r1, g1, b1) = (rgb[at + 3] as i32, rgb[at + 4] as i32, rgb[at + 5] as i32);
            let (r, g, b) = ((r0 + r1) / 2, (g0 + g1) / 2, (b0 + b1) / 2);
            let u = ((-169 * r - 331 * g + 500 * b) / 1000 + 128).clamp(0, 255) as u8;
            let v = ((500 * r - 419 * g - 81 * b) / 1000 + 128).clamp(0, 255) as u8;
            out.extend([luma(r0, g0, b0), u, luma(r1, g1, b1), v]);
        }
    }
    out
}

pub fn even_rgb(width: usize, height: usize, rgb: Vec<u8>) -> (usize, usize, Vec<u8>) {
    let (even_width, even_height) = (width & !1, height & !1);
    if (even_width, even_height) == (width, height) {
        return (width, height, rgb);
    }
    let mut out = Vec::with_capacity(even_width * even_height * 3);
    for row in 0..even_height {
        let start = row * width * 3;
        out.extend_from_slice(&rgb[start..start + even_width * 3]);
    }
    (even_width, even_height, out)
}

#[cfg(target_os = "linux")]
mod device {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::path::{Path, PathBuf};

    #[repr(C)]
    struct Pix {
        width: u32,
        height: u32,
        pixelformat: u32,
        field: u32,
        bytesperline: u32,
        sizeimage: u32,
        colorspace: u32,
        private: u32,
        flags: u32,
        enc: u32,
        quantization: u32,
        xfer: u32,
    }

    #[repr(C)]
    struct Format {
        kind: u32,
        pad: u32,
        pix: Pix,
        rest: [u8; 152],
    }

    const SET_FORMAT: libc::c_ulong = 0xC0D0_5605;
    const OUTPUT: u32 = 2;
    const YUYV: u32 = 0x5659_5559;

    pub fn find() -> Option<PathBuf> {
        for entry in std::fs::read_dir("/sys/class/video4linux").ok()?.flatten() {
            let name = std::fs::read_to_string(entry.path().join("name")).unwrap_or_default();
            if name.trim() == LABEL {
                return Some(Path::new("/dev").join(entry.file_name()));
            }
        }
        None
    }

    pub fn open(path: &Path, width: usize, height: usize) -> io::Result<File> {
        let file = OpenOptions::new().write(true).open(path)?;
        let mut format = Format {
            kind: OUTPUT,
            pad: 0,
            pix: Pix {
                width: width as u32,
                height: height as u32,
                pixelformat: YUYV,
                field: 1,
                bytesperline: width as u32 * 2,
                sizeimage: (width * height * 2) as u32,
                colorspace: 8,
                private: 0,
                flags: 0,
                enc: 0,
                quantization: 0,
                xfer: 0,
            },
            rest: [0; 152],
        };
        let result = unsafe { libc::ioctl(file.as_raw_fd(), SET_FORMAT, &mut format) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(file)
    }

    pub fn write(file: &mut File, frame: &[u8]) -> io::Result<()> {
        file.write_all(frame)
    }

    const _: () = assert!(std::mem::size_of::<Format>() == 208);
}

#[cfg(not(target_os = "linux"))]
mod device {
    use super::*;
    use std::path::{Path, PathBuf};

    pub struct Output;

    pub fn find() -> Option<PathBuf> {
        None
    }

    pub fn open(_: &Path, _: usize, _: usize) -> io::Result<Output> {
        Err(io::Error::other("sanal kamera bu sistemde desteklenmiyor"))
    }

    pub fn write(_: &mut Output, _: &[u8]) -> io::Result<()> {
        Ok(())
    }
}

pub fn available() -> bool {
    device::find().is_some()
}

pub fn setup() -> Result<(), String> {
    let script = format!(
        "printf 'options v4l2loopback devices=1 video_nr=42 card_label=\"{LABEL}\" exclusive_caps=1\\n' > /etc/modprobe.d/mause-v4l2loopback.conf && printf 'v4l2loopback\\n' > /etc/modules-load.d/mause-v4l2loopback.conf && {{ modprobe -r v4l2loopback 2>/dev/null; modprobe v4l2loopback; }}"
    );
    let output = std::process::Command::new("pkexec")
        .args(["sh", "-c", &script])
        .output()
        .map_err(|error| format!("pkexec çalıştırılamadı: {error}"))?;
    if output.status.success() && available() {
        Ok(())
    } else {
        let text = String::from_utf8_lossy(&output.stderr);
        Err(if text.trim().is_empty() {
            "Sanal kamera modülü yüklenemedi (v4l2loopback kurulu mu? sudo apt install v4l2loopback-dkms)".to_owned()
        } else {
            text.trim().to_owned()
        })
    }
}

pub struct Feeder {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for Feeder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn start(view: Arc<Mutex<View>>, turns: Arc<AtomicU16>) -> io::Result<Feeder> {
    let path = device::find().ok_or_else(|| io::Error::other("sanal kamera kurulu değil"))?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let worker = thread::spawn(move || {
        let mut output = None;
        let mut size = (0, 0);
        let mut last = 0u64;
        while !flag.load(Ordering::Relaxed) {
            let frame = view
                .lock()
                .unwrap()
                .frame
                .as_ref()
                .filter(|frame| frame.seq != last)
                .map(|frame| (frame.w, frame.h, frame.seq, frame.rgb.clone()));
            let Some((width, height, seq, rgb)) = frame else {
                thread::sleep(Duration::from_millis(8));
                continue;
            };
            last = seq;
            let (width, height, rgb) =
                hardware::rotate_rgb(width, height, &rgb, turns.load(Ordering::Relaxed));
            let (width, height, rgb) = even_rgb(width, height, rgb);
            if width < 2 || height < 2 {
                continue;
            }
            if output.is_none() || size != (width, height) {
                output = device::open(&path, width, height).ok();
                size = (width, height);
            }
            let failed = output.as_mut().is_some_and(|file| {
                device::write(file, &rgb_to_yuyv(width, height, &rgb)).is_err()
            });
            if failed {
                output = None;
            }
        }
    });
    Ok(Feeder {
        stop,
        worker: Some(worker),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gri_piksel_yuyv_olarak_dogru_cikar() {
        let rgb = vec![100u8; 4 * 2 * 3];
        let yuyv = rgb_to_yuyv(4, 2, &rgb);
        assert_eq!(yuyv.len(), 4 * 2 * 2);
        for quad in yuyv.chunks(4) {
            assert_eq!(quad[0], 100);
            assert_eq!(quad[2], 100);
            assert!((quad[1] as i32 - 128).abs() <= 1 && (quad[3] as i32 - 128).abs() <= 1);
        }
    }

    #[test]
    fn tek_sayili_goruntu_ciftlenir() {
        let (width, height, rgb) = even_rgb(3, 3, vec![7u8; 3 * 3 * 3]);
        assert_eq!((width, height, rgb.len()), (2, 2, 2 * 2 * 3));
        let (width, height, rgb) = even_rgb(4, 2, vec![1u8; 4 * 2 * 3]);
        assert_eq!((width, height, rgb.len()), (4, 2, 24));
    }
}
