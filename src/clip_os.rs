use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

pub const AVAILABLE: bool = true;

const MAX_TEXT: usize = 1 << 20;

static LOCK: Mutex<()> = Mutex::new(());

fn board() -> Option<(MutexGuard<'static, ()>, arboard::Clipboard)> {
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Some((guard, arboard::Clipboard::new().ok()?))
}

pub fn current_files() -> Option<Vec<PathBuf>> {
    let (_lock, mut b) = board()?;
    let v: Vec<PathBuf> = b
        .get()
        .file_list()
        .ok()?
        .into_iter()
        .filter(|p| p.is_file())
        .collect();
    (!v.is_empty()).then_some(v)
}

pub fn set_files(files: &[PathBuf]) {
    if let Some((_lock, mut b)) = board() {
        let _ = b.set().file_list(files);
    }
}

pub fn text_snapshot() -> Option<String> {
    let (_lock, mut b) = board()?;
    let s = b.get_text().ok()?;
    (!s.is_empty() && s.len() <= MAX_TEXT).then_some(s)
}

pub fn set_text(t: &str) -> bool {
    board().is_some_and(|(_lock, mut b)| b.set_text(t).is_ok())
}

pub fn image_snapshot() -> Option<Vec<u8>> {
    use image::ImageEncoder;

    let img = {
        let (_lock, mut b) = board()?;
        b.get_image().ok()?
    };
    let width = u32::try_from(img.width).ok()?;
    let height = u32::try_from(img.height).ok()?;
    if !crate::clipboard_sync::valid_dimensions(width, height) {
        return None;
    }
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&img.bytes, width, height, image::ExtendedColorType::Rgba8)
        .ok()?;
    (bytes.len() <= crate::clipboard_sync::MAX_PNG).then_some(bytes)
}

pub fn set_image(_: Vec<u8>, img: image::RgbaImage) -> bool {
    use std::borrow::Cow;

    board().is_some_and(|(_lock, mut b)| {
        b.set_image(arboard::ImageData {
            width: img.width() as usize,
            height: img.height() as usize,
            bytes: Cow::Owned(img.into_raw()),
        })
        .is_ok()
    })
}

pub fn carry(sh: std::sync::Arc<crate::core::Shared>) {
    use std::sync::atomic::Ordering;
    if !sh.carry_files.load(Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let Some(files) = current_files() else { return };
        {
            let mut last = sh.last_carry.lock().unwrap();
            if *last == files {
                return;
            }
            *last = files.clone();
        }
        sh.log(format!(
            "Panodaki {} dosya karşı PC'ye taşınıyor…",
            files.len()
        ));
        for f in files {
            crate::transfer::send_file(sh.clone(), f, true);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "desktop: gerçek sistem panosunu değiştirir"]
    fn pano_metin_ve_dosya_gidis_donus() {
        let before = text_snapshot();
        set_text("merhaba dünya ğüşiöç");
        assert_eq!(text_snapshot().as_deref(), Some("merhaba dünya ğüşiöç"));

        let p = std::env::temp_dir().join("mause pano test ğ.txt");
        std::fs::write(&p, b"x").unwrap();
        let p = p.canonicalize().unwrap();
        set_files(std::slice::from_ref(&p));
        let got = current_files().map(|v| {
            v.into_iter()
                .map(|f| f.canonicalize().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(got, Some(vec![p.clone()]));
        std::fs::remove_file(p).ok();

        let readers: Vec<_> = (0..8)
            .map(|i| {
                std::thread::spawn(move || {
                    if i % 2 == 0 {
                        current_files().is_some()
                    } else {
                        text_snapshot().is_some()
                    }
                })
            })
            .collect();
        for r in readers {
            r.join().expect("eşzamanlı pano okuması çöktü");
        }
        if let Some(t) = before {
            set_text(&t);
        }
    }
}
