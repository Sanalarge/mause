use super::{CALLBACKS, Callbacks, LINK, Link, PREFS, Secure, connect_transfer};
use jni::JNIEnv;
use jni::objects::{JByteArray, JObject, JValue};
use std::io::{self, Read, Write};
use std::sync::Mutex;
use std::sync::atomic::Ordering;

pub const SUPPORTED: u32 = 1 << 9;
pub const MAGIC: &[u8; 4] = b"MFC1";
const CLIP_IN: u16 = 1 << 4;
const SHARE: u16 = 1 << 8;
const MAX_TEXT: usize = 1024 * 1024;
const MAX_PNG: usize = 48 * 1024 * 1024;
const PNG: u8 = 2;
const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
static RECEIVING: Mutex<()> = Mutex::new(());

pub fn can_receive() -> bool {
    PREFS.load(Ordering::Relaxed) & (CLIP_IN | SHARE) == CLIP_IN | SHARE
}

fn active(link: &Link) -> bool {
    link.clipboard_images
        && LINK
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|current| current.hash == link.hash)
}

pub fn receive(s: &mut Secure, link: &Link, callbacks: &Callbacks) -> io::Result<()> {
    let mut hash = [0; 32];
    let mut kind = [0];
    let mut size = [0; 4];
    s.read_exact(&mut hash)?;
    s.read_exact(&mut kind)?;
    s.read_exact(&mut size)?;
    let size = u32::from_le_bytes(size) as usize;
    let limit = match kind[0] {
        1 => MAX_TEXT,
        PNG => MAX_PNG,
        _ => 0,
    };
    if size == 0 || size > limit || hash != link.hash || !active(link) || !can_receive() {
        return s.write_all(&[0]);
    }
    let Ok(_receiving) = RECEIVING.try_lock() else {
        return s.write_all(&[0]);
    };
    s.write_all(&[1])?;
    let mut bytes = vec![0; size];
    s.read_exact(&mut bytes)?;
    if !active(link) || !can_receive() {
        return s.write_all(&[0]);
    }
    let applied = match kind[0] {
        1 => std::str::from_utf8(&bytes).is_ok_and(|text| {
            callbacks.clipboard(text);
            true
        }),
        PNG => bytes.starts_with(SIGNATURE) && deliver_image(callbacks, &bytes),
        _ => false,
    };
    s.write_all(&[u8::from(applied)])
}

fn deliver_image(callbacks: &Callbacks, bytes: &[u8]) -> bool {
    let mut applied = false;
    callbacks.call(|env| {
        let data = JObject::from(env.byte_array_from_slice(bytes)?);
        applied = env
            .call_method(
                callbacks.target.as_obj(),
                "nativeClipboardImage",
                "([B)Z",
                &[JValue::Object(&data)],
            )?
            .z()?;
        Ok(())
    });
    applied
}

fn send_image(link: &Link, bytes: &[u8]) -> io::Result<()> {
    if !active(link) || PREFS.load(Ordering::Relaxed) & SHARE == 0 {
        return Err(io::Error::other(
            "bağlı bilgisayarda güncel Mause ve pano paylaşımı gerekir",
        ));
    }
    if bytes.len() > MAX_PNG || !bytes.starts_with(SIGNATURE) {
        return Err(io::Error::other("geçersiz veya çok büyük pano görseli"));
    }
    let mut s = connect_transfer(link)?;
    s.write_all(MAGIC)?;
    s.write_all(&link.hash)?;
    s.write_all(&[PNG])?;
    s.write_all(&(bytes.len() as u32).to_le_bytes())?;
    let mut status = [0];
    s.read_exact(&mut status)?;
    if status[0] != 1 || !active(link) || PREFS.load(Ordering::Relaxed) & SHARE == 0 {
        return Err(io::Error::other(
            "karşı bilgisayar pano aktarımına izin vermiyor",
        ));
    }
    s.write_all(bytes)?;
    s.read_exact(&mut status)?;
    if status[0] != 1 {
        return Err(io::Error::other("karşı pano güncellenemedi"));
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_sendClipboardImage(
    env: JNIEnv,
    _this: JObject,
    data: JByteArray,
) {
    let Some(callbacks) = CALLBACKS.lock().unwrap().clone() else {
        return;
    };
    let link = LINK.lock().unwrap().clone();
    let result = (|| {
        let link = link.ok_or_else(|| io::Error::other("bilgisayarla bağlantı yok"))?;
        let len = env.get_array_length(&data).map_err(io::Error::other)? as usize;
        if len > MAX_PNG {
            return Err(io::Error::other("pano görseli çok büyük"));
        }
        let bytes = env.convert_byte_array(data).map_err(io::Error::other)?;
        send_image(&link, &bytes)
    })();
    match result {
        Ok(()) => callbacks.activity("Görsel bilgisayarın panosuna gönderildi"),
        Err(e) => callbacks.activity(&format!("Görsel gönderilemedi: {e}")),
    }
}
