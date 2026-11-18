use std::io::Read;
use std::path::PathBuf;
use wl_clipboard_rs::copy::{MimeSource, MimeType as CMime, Options, Source};
use wl_clipboard_rs::paste::{self, ClipboardType, MimeType as PMime, Seat};

pub const AVAILABLE: bool = true;

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn pct_encode(p: &str) -> String {
    let mut o = String::new();
    for c in p.bytes() {
        match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                o.push(c as char)
            }
            _ => o.push_str(&format!("%{c:02X}")),
        }
    }
    o
}

pub fn current_files() -> Option<Vec<PathBuf>> {
    let (mut r, _) = paste::get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        PMime::Specific("text/uri-list"),
    )
    .ok()?;
    let mut s = String::new();
    r.read_to_string(&mut s).ok()?;
    let v: Vec<PathBuf> = s
        .lines()
        .filter_map(|l| l.trim().strip_prefix("file://"))
        .map(|p| PathBuf::from(pct_decode(p.strip_prefix("localhost").unwrap_or(p))))
        .filter(|p| p.is_file())
        .collect();
    (!v.is_empty()).then_some(v)
}

pub fn set_files(files: &[PathBuf]) {
    let uris: Vec<String> = files
        .iter()
        .map(|p| format!("file://{}", pct_encode(&p.to_string_lossy())))
        .collect();
    let list = uris.join("\r\n") + "\r\n";
    let gnome = format!("copy\n{}", uris.join("\n"));
    let mut o = Options::new();
    o.foreground(true);
    let _ = o.copy_multi(vec![
        MimeSource {
            source: Source::Bytes(list.into_bytes().into()),
            mime_type: CMime::Specific("text/uri-list".into()),
        },
        MimeSource {
            source: Source::Bytes(gnome.into_bytes().into()),
            mime_type: CMime::Specific("x-special/gnome-copied-files".into()),
        },
    ]);
}

pub fn text_snapshot() -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = paste::get_contents(ClipboardType::Regular, Seat::Unspecified, PMime::Text)
            .ok()
            .and_then(|(mut r, _)| {
                let mut s = String::new();
                r.by_ref().take(1 << 20).read_to_string(&mut s).ok()?;
                (!s.is_empty()).then_some(s)
            });
        let _ = tx.send(r);
    });
    rx.recv_timeout(std::time::Duration::from_millis(400))
        .ok()
        .flatten()
}

pub fn set_text(t: &str) -> bool {
    Options::new()
        .copy(Source::Bytes(t.as_bytes().to_vec().into()), CMime::Text)
        .is_ok()
}

pub fn image_snapshot() -> Option<Vec<u8>> {
    let (reader, _) = paste::get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        PMime::Specific("image/png"),
    )
    .ok()?;
    let mut bytes = Vec::new();
    reader
        .take(crate::clipboard_sync::MAX_PNG as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= crate::clipboard_sync::MAX_PNG).then_some(bytes)
}

pub fn set_image(bytes: Vec<u8>, _: image::RgbaImage) -> bool {
    Options::new()
        .copy(
            Source::Bytes(bytes.into()),
            CMime::Specific("image/png".into()),
        )
        .is_ok()
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
    #[ignore = "desktop: gerçek Wayland panosunu değiştirir"]
    fn pano_dosya_gidis_donus() {
        let p = std::env::temp_dir().join("mause pano test ğ.txt");
        std::fs::write(&p, b"x").unwrap();
        let pc = p.clone();
        std::thread::spawn(move || set_files(&[pc]));
        std::thread::sleep(std::time::Duration::from_millis(800));
        let got = current_files();
        println!("okunan: {got:?}");
        assert_eq!(got, Some(vec![p.clone()]));
        std::fs::remove_file(p).ok();
    }

    #[test]
    #[ignore = "desktop: gerçek Wayland panosunu değiştirir"]
    fn metin_panosu() {
        std::thread::spawn(|| set_text("merhaba dünya ğüşiöç"));
        std::thread::sleep(std::time::Duration::from_millis(600));
        assert_eq!(text_snapshot().as_deref(), Some("merhaba dünya ğüşiöç"));
    }

    #[test]
    #[ignore = "desktop: gerçek Wayland panosunu değiştirir"]
    fn gecista_tasima() {
        use crate::core::Shared;
        use std::sync::atomic::Ordering;
        use std::time::Duration;
        let (a, b) = (Shared::new(false), Shared::new(false));
        *a.peer_ip.lock().unwrap() = Some("127.0.0.1".into());
        *b.peer_ip.lock().unwrap() = Some("127.0.0.1".into());
        for sh in [&a, &b] {
            sh.peer_secrets
                .lock()
                .unwrap()
                .insert("127.0.0.1".into(), "s".into());
        }
        let server = crate::test_support::TransferServer::new(b.clone());
        a.test_transfer
            .port
            .store(server.addr.port(), Ordering::Relaxed);
        let source = tempfile::tempdir().unwrap();
        let src = source.path().join("mause-tasima-ornek.txt");
        std::fs::write(&src, b"merhaba").unwrap();
        let s2 = src.clone();
        std::thread::spawn(move || set_files(&[s2]));
        std::thread::sleep(Duration::from_millis(600));
        carry(a.clone());
        std::thread::sleep(Duration::from_millis(2500));
        let got = current_files().expect("pano boş");
        println!("karşı panosu: {got:?}");
        assert_ne!(got[0], src, "pano hâlâ kaynak dosyada");
        assert!(got[0].starts_with(b.test_transfer.inbox.path()));
        assert_eq!(std::fs::read(&got[0]).unwrap(), b"merhaba");
        let s3 = src.clone();
        std::thread::spawn(move || set_files(&[s3]));
        std::thread::sleep(Duration::from_millis(600));
        let before = a.transfers.lock().unwrap().len();
        carry(a.clone());
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(a.transfers.lock().unwrap().len(), before);
        std::fs::remove_file(&got[0]).ok();
        std::fs::remove_file(src).ok();
        a.request_stop();
        b.request_stop();
        std::thread::sleep(Duration::from_millis(400));
    }
}
