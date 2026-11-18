use crate::core::Shared;
use crate::net::Secure;
use crate::{clip, transfer};
use blake2::{Blake2s256, Digest};
use image::{ImageFormat, ImageReader, RgbaImage};
use std::io::{self, Cursor, Read, Write};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;

pub(crate) const MAGIC: &[u8; 4] = b"MFC1";
pub(crate) const MAX_PNG: usize = 48 * 1024 * 1024;
const MAX_TEXT: usize = 1024 * 1024;
const MAX_PIXELS: u64 = 36_000_000;
const MAX_DIMENSION: u32 = 16_384;
type Fingerprint = [u8; 32];

pub(crate) struct Session {
    pub hash: [u8; 32],
    last: Mutex<Option<Fingerprint>>,
    receiving: Mutex<()>,
}

impl Session {
    pub fn new(hash: [u8; 32]) -> Self {
        Self {
            hash,
            last: Mutex::new(None),
            receiving: Mutex::new(()),
        }
    }
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum Kind {
    Text = 1,
    Png = 2,
}

impl Kind {
    fn limit(self) -> usize {
        match self {
            Self::Text => MAX_TEXT,
            Self::Png => MAX_PNG,
        }
    }
}

pub(crate) fn valid_dimensions(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= MAX_DIMENSION
        && height <= MAX_DIMENSION
        && u64::from(width) * u64::from(height) <= MAX_PIXELS
}

pub(crate) fn decode_png(bytes: &[u8]) -> Option<RgbaImage> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_PIXELS * 8 + 16 * 1024 * 1024);
    let reader = || {
        let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
        reader.limits(limits.clone());
        reader
    };
    let (width, height) = reader().into_dimensions().ok()?;
    if !valid_dimensions(width, height) {
        return None;
    }
    Some(reader().decode().ok()?.into_rgba8())
}

fn fingerprint(kind: Kind, bytes: &[u8], image: Option<&RgbaImage>) -> Fingerprint {
    let mut digest = Blake2s256::new();
    digest.update([kind as u8]);
    if let Some(img) = image {
        // Different clipboard backends may encode the same pixels as different PNGs.
        digest.update(img.width().to_le_bytes());
        digest.update(img.height().to_le_bytes());
        digest.update(img.as_raw());
    } else {
        digest.update(bytes);
    }
    digest.finalize().into()
}

fn snapshot() -> Option<(Kind, Vec<u8>, Fingerprint)> {
    // Leave file-list clipboard entries to the existing file-carry path.
    if clip::current_files().is_some() {
        return None;
    }
    if let Some(bytes) = clip::image_snapshot()
        && let Some(image) = decode_png(&bytes)
    {
        let id = fingerprint(Kind::Png, &bytes, Some(&image));
        return Some((Kind::Png, bytes, id));
    }
    let text = clip::text_snapshot()?;
    let bytes = text.into_bytes();
    let id = fingerprint(Kind::Text, &bytes, None);
    Some((Kind::Text, bytes, id))
}

pub(crate) fn worker(
    sh: Arc<Shared>,
    ip: String,
    secret: String,
    session: Arc<Session>,
) -> SyncSender<()> {
    // Keep at most one crossing pending while the current clipboard is in flight.
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        while rx.recv().is_ok() {
            if sh.clipboard_session(&ip, &session.hash, false).is_none() {
                continue;
            }
            let Some((kind, bytes, id)) = snapshot() else {
                *session.last.lock().unwrap() = None;
                continue;
            };
            let last = *session.last.lock().unwrap();
            if last == Some(id) || sh.clipboard_session(&ip, &session.hash, false).is_none() {
                continue;
            }
            if send(&sh, &ip, &secret, &session.hash, kind, &bytes).is_ok() {
                let mut synced = session.last.lock().unwrap();
                if *synced == last {
                    *synced = Some(id);
                }
            }
        }
    });
    tx
}

fn send(
    sh: &Shared,
    ip: &str,
    secret: &str,
    hash: &[u8; 32],
    kind: Kind,
    bytes: &[u8],
) -> io::Result<()> {
    let mut s = transfer::connect_to(sh, ip, secret)?;
    s.write_all(MAGIC)?;
    s.write_all(hash)?;
    s.write_all(&[kind as u8])?;
    s.write_all(&(bytes.len() as u32).to_le_bytes())?;
    let mut status = [0];
    s.read_exact(&mut status)?;
    if status[0] != 1 || sh.clipboard_session(ip, hash, false).is_none() {
        return Err(io::Error::other("pano aktarımı reddedildi"));
    }
    s.write_all(bytes)?;
    s.read_exact(&mut status)?;
    if status[0] != 1 {
        return Err(io::Error::other("karşı pano güncellenemedi"));
    }
    Ok(())
}

fn header(s: &mut impl Read) -> io::Result<([u8; 32], Kind, usize)> {
    let mut hash = [0; 32];
    let mut kind = [0];
    let mut len = [0; 4];
    s.read_exact(&mut hash)?;
    s.read_exact(&mut kind)?;
    s.read_exact(&mut len)?;
    let kind = match kind[0] {
        1 => Kind::Text,
        2 => Kind::Png,
        _ => return Err(io::Error::other("bilinmeyen pano içeriği")),
    };
    let len = u32::from_le_bytes(len) as usize;
    if len == 0 || len > kind.limit() {
        return Err(io::Error::other("pano içeriği boyut sınırı dışında"));
    }
    Ok((hash, kind, len))
}

pub(crate) fn receive(sh: &Shared, ip: &str, s: &mut Secure) {
    let _ = receive_snapshot(sh, ip, s);
}

fn receive_snapshot(sh: &Shared, ip: &str, s: &mut Secure) -> io::Result<()> {
    let (hash, kind, len) = header(s)?;
    let Some(session) = sh.clipboard_session(ip, &hash, true) else {
        return s.write_all(&[0]);
    };
    let Ok(_receiving) = session.receiving.try_lock() else {
        return s.write_all(&[0]);
    };
    s.write_all(&[1])?;
    let mut bytes = vec![0; len];
    s.read_exact(&mut bytes)?;
    let image = match kind {
        Kind::Text => None,
        Kind::Png => match decode_png(&bytes) {
            Some(image) => Some(image),
            None => return s.write_all(&[0]),
        },
    };
    let id = fingerprint(kind, &bytes, image.as_ref());
    let mut last = session.last.lock().unwrap();
    if sh.clipboard_session(ip, &hash, true).is_none() {
        return s.write_all(&[0]);
    }
    let applied = match image {
        Some(img) => clip::set_image(bytes, img),
        None => std::str::from_utf8(&bytes).is_ok_and(clip::set_text),
    };
    if applied {
        *last = Some(id);
    }
    drop(last);
    s.write_all(&[u8::from(applied)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};

    #[test]
    fn png_preserves_pixels_and_ignores_encoding_in_fingerprint() {
        let pixels = [1, 2, 3, 255, 7, 8, 9, 128];
        let encode = |compression| {
            let mut bytes = Vec::new();
            PngEncoder::new_with_quality(&mut bytes, compression, FilterType::NoFilter)
                .write_image(&pixels, 2, 1, image::ExtendedColorType::Rgba8)
                .unwrap();
            bytes
        };
        let fast = encode(CompressionType::Fast);
        let best = encode(CompressionType::Best);
        let first = decode_png(&fast).unwrap();
        let second = decode_png(&best).unwrap();
        assert_eq!(first.dimensions(), (2, 1));
        assert_eq!(first.as_raw().as_slice(), &pixels);
        assert_eq!(
            fingerprint(Kind::Png, &fast, Some(&first)),
            fingerprint(Kind::Png, &best, Some(&second)),
        );
        assert!(decode_png(&[]).is_none());
    }

    #[test]
    fn header_rejects_invalid_kinds_and_lengths_before_allocation() {
        let make = |kind, len: usize| {
            let mut data = vec![0; 32];
            data.push(kind);
            data.extend_from_slice(&(len as u32).to_le_bytes());
            Cursor::new(data)
        };
        assert!(matches!(
            header(&mut make(2, MAX_PNG)),
            Ok((_, Kind::Png, MAX_PNG))
        ));
        assert!(header(&mut make(2, MAX_PNG + 1)).is_err());
        assert!(header(&mut make(1, MAX_TEXT + 1)).is_err());
        assert!(header(&mut make(1, 0)).is_err());
        assert!(header(&mut make(3, 1)).is_err());
    }
}
