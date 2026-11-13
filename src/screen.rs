use crate::net;
use crate::platform::{Capture, Monitor};
use crate::video;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const PORT: u16 = 24806;

pub const STOP: u8 = 0;
pub const START: u8 = 1;
pub const DENIED: u8 = 2;
pub const FAILED: u8 = 3;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Opts {
    pub quality: u8,
    pub fps: u8,
    pub max_w: u16,
    pub monitor: u8,
    pub cursor: bool,
    pub h264: bool,
    pub extend: Option<(u16, u16)>,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            quality: 70,
            fps: 15,
            max_w: 1920,
            monitor: 0,
            cursor: true,
            h264: true,
            extend: None,
        }
    }
}

impl Opts {
    pub fn encode(&self) -> Vec<u8> {
        let w = self.max_w.to_le_bytes();
        let mut bytes = vec![
            self.quality,
            self.fps,
            w[0],
            w[1],
            self.monitor,
            self.cursor as u8,
            self.h264 as u8,
        ];
        if let Some((width, height)) = self.extend {
            bytes.push(1);
            bytes.extend(width.to_le_bytes());
            bytes.extend(height.to_le_bytes());
        }
        bytes
    }

    pub fn decode(b: &[u8]) -> Option<Opts> {
        (b.len() >= 6).then(|| Opts {
            quality: b[0].clamp(10, 95),
            fps: b[1].clamp(1, 30),
            max_w: u16::from_le_bytes([b[2], b[3]]),
            monitor: b[4],
            cursor: b[5] != 0,
            h264: b.get(6).is_some_and(|codecs| codecs & 1 != 0),
            extend: extend_size(b),
        })
    }
}

fn extend_size(b: &[u8]) -> Option<(u16, u16)> {
    if b.len() < 12 || b[7] != 1 {
        return None;
    }
    let width = u16::from_le_bytes([b[8], b[9]]);
    let height = u16::from_le_bytes([b[10], b[11]]);
    (width >= 320 && height >= 240 && width <= 3840 && height <= 2160).then_some((width, height))
}

pub fn info(control: bool) -> String {
    let names: Vec<String> = if crate::platform::screen_capture_pending()
        || cfg!(target_os = "linux")
            && std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
    {
        vec!["Paylaşım penceresinde seçilen ekran".into()]
    } else {
        crate::platform::monitors()
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let name = if m.name.is_empty() {
                    format!("Ekran {}", i + 1)
                } else {
                    m.name.clone()
                };
                format!("{name} ({}×{})", m.w, m.h)
            })
            .collect()
    };
    format!("{}\n{}", control as u8, names.join("\n"))
}

pub fn parse_info(s: &str) -> (bool, Vec<String>) {
    let mut it = s.lines();
    let control = it.next() == Some("1");
    (
        control,
        it.filter(|l| !l.is_empty()).map(str::to_owned).collect(),
    )
}

pub const EV_ABS: u16 = 0x03;

const MAX_FRAME: usize = 16 << 20;
const FIRST_FRAME: Duration = Duration::from_secs(90);

pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
    pub seq: u64,
}

#[derive(Default)]
pub struct View {
    pub frame: Option<Frame>,
    pub note: Option<String>,
}

pub struct Handle {
    stop: Arc<AtomicBool>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub struct Sharing {
    _handle: Handle,
    pub opts: Arc<Mutex<Opts>>,
    pub area: Arc<Mutex<Option<Monitor>>>,
}

impl Sharing {
    pub fn to_desktop(&self, nx: i32, ny: i32) -> Option<(i32, i32)> {
        let a = self.area.lock().unwrap().clone()?;
        Some((
            a.x + nx.clamp(0, 65535) * (a.w - 1).max(0) / 65535,
            a.y + ny.clamp(0, 65535) * (a.h - 1).max(0) / 65535,
        ))
    }
}

pub fn start_receiver(peer_ip: String, secret: String, view: Arc<Mutex<View>>) -> Option<Handle> {
    start_receiver_port(PORT, peer_ip, secret, view)
}

pub fn start_receiver_port(
    port: u16,
    peer_ip: String,
    secret: String,
    view: Arc<Mutex<View>>,
) -> Option<Handle> {
    let l = TcpListener::bind(("0.0.0.0", port)).ok()?;
    start_receiver_on(l, peer_ip, secret, view)
}

fn start_receiver_on(
    l: TcpListener,
    peer_ip: String,
    secret: String,
    view: Arc<Mutex<View>>,
) -> Option<Handle> {
    l.set_nonblocking(true).ok()?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    *view.lock().unwrap() = View {
        frame: None,
        note: None,
    };
    thread::spawn(move || {
        let note = |s: &str| view.lock().unwrap().note = Some(s.into());
        let Some(s) = accept_peer(&l, &peer_ip, &flag) else {
            if !flag.load(Ordering::Relaxed) {
                note(
                    "Karşı PC görüntü göndermedi. Güvenlik duvarı 24806 portunu engelliyor olabilir.",
                );
            }
            return;
        };
        drop(l);
        let r = net::handshake(s, false, &secret).and_then(|mut s| {
            s.set_read_timeout(Some(FIRST_FRAME))?;
            receive(&mut s, &view, &flag)
        });
        if !flag.load(Ordering::Relaxed) {
            view.lock().unwrap().frame = None;
            note(&match r {
                Ok(()) => "Ekran paylaşımı durdu.".to_owned(),
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    "Ekran paylaşımı kesildi: karşı PC bağlantıyı kapattı (ekran yakalanamamış olabilir).".to_owned()
                }
                Err(e) => format!("Ekran paylaşımı kesildi: {e}"),
            });
        }
    });
    Some(Handle { stop })
}

fn accept_peer(l: &TcpListener, peer_ip: &str, stop: &AtomicBool) -> Option<TcpStream> {
    let until = Instant::now() + Duration::from_secs(10);
    while !stop.load(Ordering::Relaxed) && Instant::now() < until {
        match l.accept() {
            Ok((s, a)) if a.ip().to_string() == peer_ip => {
                s.set_nonblocking(false).ok()?;
                return Some(s);
            }
            Ok(_) => {}
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
    None
}

pub(crate) fn receive(s: &mut impl Read, view: &Mutex<View>, stop: &AtomicBool) -> io::Result<()> {
    let mut seq = 0;
    let mut decoder: Option<video::H264Decoder> = None;
    while !stop.load(Ordering::Relaxed) {
        let mut l = [0u8; 4];
        s.read_exact(&mut l)?;
        let n = u32::from_le_bytes(l) as usize;
        if !(5..=MAX_FRAME).contains(&n) {
            return Err(io::Error::other("geçersiz kare"));
        }
        let mut b = vec![0u8; n];
        s.read_exact(&mut b)?;
        let (w, h) = (
            u16::from_le_bytes([b[0], b[1]]) as usize,
            u16::from_le_bytes([b[2], b[3]]) as usize,
        );
        if w == 0 && h == 0 {
            let why = String::from_utf8_lossy(&b[4..]).into_owned();
            if why.contains("Unable to open /proc/") {
                return Err(io::Error::other(
                    "Karşı PC'deki Mause yönetici olarak açılmış; Ubuntu bu durumda ekran paylaşımına izin vermiyor.\n\
                     Karşı PC'de Mause'dan çıkın (görev çubuğundaki simge → Çık) ve uygulama menüsünden yeniden açın.\n\
                     Karşıdaki Mause'u güncellerseniz bu durumu kendisi düzeltir.",
                ));
            }
            return Err(io::Error::other(format!("karşı PC: {why}")));
        }
        seq += 1;
        if let Some(frame) = decode_payload(&b, seq, &mut decoder)? {
            *view.lock().unwrap() = View {
                frame: Some(frame),
                note: None,
            };
        }
    }
    Ok(())
}

fn decode_payload(
    b: &[u8],
    seq: u64,
    decoder: &mut Option<video::H264Decoder>,
) -> io::Result<Option<Frame>> {
    if b.len() > 4 && video::is_h264(&b[4..]) {
        if decoder.is_none() {
            *decoder = Some(video::H264Decoder::new()?);
        }
        let decoded = decoder.as_mut().unwrap().decode(&b[4..])?;
        return Ok(decoded.map(|(w, h, rgb)| Frame { w, h, rgb, seq }));
    }
    decode_image(b, seq).map(Some)
}

fn frame_packet(w: usize, h: usize, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend(((payload.len() + 4) as u32).to_le_bytes());
    out.extend((w as u16).to_le_bytes());
    out.extend((h as u16).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

type Encoding = Option<(video::H264Encoder, (usize, usize, u8, u8))>;

fn h264_packet(
    encoder: &mut Encoding,
    w: usize,
    h: usize,
    bgra: &[u8],
    fps: u8,
    quality: u8,
) -> io::Result<Option<(usize, usize, Vec<u8>)>> {
    let (even_w, even_h, pixels) = video::even(w, h, bgra);
    let key = (even_w, even_h, fps, quality);
    if encoder.as_ref().is_none_or(|(_, current)| *current != key) {
        let bitrate = video::bitrate(even_w, even_h, fps, quality);
        *encoder = Some((video::H264Encoder::new(even_w, even_h, fps, bitrate)?, key));
    }
    let payload = encoder.as_mut().unwrap().0.encode(&pixels)?;
    Ok((!payload.is_empty()).then_some((even_w, even_h, payload)))
}

fn stream_fps(requested: u8) -> u8 {
    (requested as u32 * 2).clamp(15, 30) as u8
}

pub(crate) fn decode_image(b: &[u8], seq: u64) -> io::Result<Frame> {
    if !(5..=MAX_FRAME).contains(&b.len()) {
        return Err(io::Error::other("geçersiz kare uzunluğu"));
    }
    let w = u16::from_le_bytes([b[0], b[1]]) as usize;
    let h = u16::from_le_bytes([b[2], b[3]]) as usize;
    if w == 0 || h == 0 || w > 8192 || h > 8192 || w * h > 16_777_216 {
        return Err(io::Error::other("ekran boyutu sınırı aşıldı"));
    }
    let mut d = jpeg_decoder::Decoder::new(&b[4..]);
    d.set_max_decoding_buffer_size(16_777_216 * 3);
    d.read_info().map_err(io::Error::other)?;
    let info = d
        .info()
        .ok_or_else(|| io::Error::other("JPEG bilgisi yok"))?;
    if info.width as usize != w
        || info.height as usize != h
        || info.pixel_format != jpeg_decoder::PixelFormat::RGB24
    {
        return Err(io::Error::other("JPEG boyutu veya renk biçimi uyuşmuyor"));
    }
    let rgb = d.decode().map_err(io::Error::other)?;
    if rgb.len() != w * h * 3 {
        return Err(io::Error::other("kare boyutu tutmuyor"));
    }
    Ok(Frame { w, h, rgb, seq })
}

pub fn start_sender(peer_ip: &str, secret: String, opts: Opts) -> Option<Sharing> {
    crate::platform::screen_cursor(opts.cursor);
    let begun = match opts.extend {
        Some(size) => crate::platform::screen_capture_begin_virtual(size),
        None => crate::platform::screen_capture_begin(),
    };
    if !begun {
        return None;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let opts = Arc::new(Mutex::new(opts));
    let area: Arc<Mutex<Option<Monitor>>> = Arc::default();
    let (o2, a2) = (opts.clone(), area.clone());
    let addr = format!("{peer_ip}:{PORT}");
    thread::spawn(move || {
        let r = addr
            .parse()
            .map_err(io::Error::other)
            .and_then(|a| TcpStream::connect_timeout(&a, Duration::from_secs(3)))
            .and_then(|s| {
                s.set_nodelay(true)?;
                net::handshake(s, true, &secret)
            })
            .and_then(|mut s| {
                let r = send(&mut s, &flag, &o2, &a2);
                if let Err(e) = &r {
                    let _ = s.write_all(&error_frame(&e.to_string()));
                }
                r
            });
        if let Err(e) = r {
            eprintln!("ekran paylaşımı: {e}");
        }
    });
    Some(Sharing {
        _handle: Handle { stop },
        opts,
        area,
    })
}

pub(crate) fn send(
    s: &mut impl Write,
    stop: &AtomicBool,
    opts: &Mutex<Opts>,
    area: &Mutex<Option<Monitor>>,
) -> io::Result<()> {
    let started = Instant::now();
    let mut encoder: Encoding = None;
    let mut h264_failed = false;
    #[cfg(target_os = "windows")]
    let (mut virtual_size, mut virtual_display) = (None, None::<crate::vdisplay::Lease>);
    while !stop.load(Ordering::Relaxed) {
        let t0 = Instant::now();
        let o = *opts.lock().unwrap();
        #[cfg(target_os = "windows")]
        if o.extend != virtual_size {
            drop(virtual_display.take());
            virtual_display = o.extend.map(crate::vdisplay::Lease::begin).transpose()?;
            virtual_size = o.extend;
        }
        let fps = if o.h264 && !h264_failed {
            stream_fps(o.fps)
        } else {
            o.fps
        };
        crate::platform::capture_fps(fps);
        let tick = Duration::from_millis(1000 / fps.max(1) as u64);
        #[cfg(target_os = "windows")]
        let monitor = match &virtual_display {
            Some(display) => display
                .monitor_index()
                .ok_or_else(|| io::Error::other("Sanal ekran bağlantısı kesildi."))?,
            None => o.monitor as usize,
        };
        #[cfg(not(target_os = "windows"))]
        let monitor = o.monitor as usize;
        let Some((mut cap, mon)) = crate::platform::capture_monitor(monitor) else {
            if crate::platform::screen_capture_pending() && started.elapsed() < FIRST_FRAME {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
            return Err(io::Error::other(
                match crate::platform::screen_capture_error() {
                    Some(e) => format!("ekran yakalanamadı: {e}"),
                    None => "ekran yakalanamadı".to_owned(),
                },
            ));
        };
        if o.cursor {
            if let Some(p) = crate::platform::cursor_pos() {
                draw_cursor(&mut cap, &mon, p);
            }
        }
        *area.lock().unwrap() = Some(mon);
        let (w, h, bgra) = shrink(
            &cap,
            if o.max_w == 0 {
                usize::MAX
            } else {
                o.max_w as usize
            },
        );
        if o.h264 && !h264_failed {
            match h264_packet(&mut encoder, w, h, &bgra, fps, o.quality) {
                Ok(Some((even_w, even_h, payload))) => {
                    s.write_all(&frame_packet(even_w, even_h, &payload))?;
                }
                Ok(None) => {}
                Err(_) => {
                    h264_failed = true;
                    encoder = None;
                    continue;
                }
            }
            if let Some(rest) = tick.checked_sub(t0.elapsed()) {
                thread::sleep(rest);
            }
            continue;
        }
        let mut jpeg = Vec::with_capacity(w * h / 4);
        jpeg_encoder::Encoder::new(&mut jpeg, o.quality)
            .encode(&bgra, w as u16, h as u16, jpeg_encoder::ColorType::Bgra)
            .map_err(|e| io::Error::other(format!("{e}")))?;
        s.write_all(&frame_packet(w, h, &jpeg))?;
        if let Some(rest) = tick.checked_sub(t0.elapsed()) {
            thread::sleep(rest);
        }
    }
    Ok(())
}

fn error_frame(msg: &str) -> Vec<u8> {
    let mut out = ((msg.len() + 4) as u32).to_le_bytes().to_vec();
    out.extend([0u8; 4]);
    out.extend(msg.as_bytes());
    out
}

const ARROW: [&str; 17] = [
    "X",
    "XX",
    "X.X",
    "X..X",
    "X...X",
    "X....X",
    "X.....X",
    "X......X",
    "X.......X",
    "X........X",
    "X.....XXXXX",
    "X..X..X",
    "X.X X..X",
    "XX  X..X",
    "X    X..X",
    "     X..X",
    "      XX",
];

pub(crate) fn draw_cursor(c: &mut Capture, mon: &Monitor, p: (i32, i32)) {
    if mon.w <= 0 || mon.h <= 0 {
        return;
    }
    let scale = c.w as f32 / mon.w as f32;
    let (px, py) = (
        ((p.0 - mon.x) as f32 * scale) as i64,
        ((p.1 - mon.y) as f32 * scale) as i64,
    );
    let k = scale.round().max(1.0) as i64;
    for (dy, row) in ARROW.iter().enumerate() {
        for (dx, ch) in row.bytes().enumerate() {
            let color = match ch {
                b'X' => 0u8,
                b'.' => 255,
                _ => continue,
            };
            for sy in 0..k {
                for sx in 0..k {
                    let (x, y) = (px + dx as i64 * k + sx, py + dy as i64 * k + sy);
                    if x < 0 || y < 0 || x >= c.w as i64 || y >= c.h as i64 {
                        continue;
                    }
                    let o = y as usize * c.stride + x as usize * 4;
                    if let Some(px) = c.bgra.get_mut(o..o + 3) {
                        px.fill(color);
                    }
                }
            }
        }
    }
}

pub(crate) fn shrink(c: &Capture, max_w: usize) -> (usize, usize, Vec<u8>) {
    let f = c.w.div_ceil(max_w).max(1);
    let (w, h) = (c.w / f, c.h / f);
    if f == 1 && c.stride == c.w * 4 {
        return (w, h, c.bgra[..w * h * 4].to_vec());
    }
    let mut out = vec![0u8; w * h * 4];
    let area = (f * f) as u32;
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for dy in 0..f {
                let row = (y * f + dy) * c.stride;
                for dx in 0..f {
                    let p = row + (x * f + dx) * 4;
                    for (k, s) in sum.iter_mut().enumerate() {
                        *s += c.bgra[p + k] as u32;
                    }
                }
            }
            let o = (y * w + x) * 4;
            for k in 0..4 {
                out[o + k] = (sum[k] / area) as u8;
            }
        }
    }
    (w, h, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_bayragi_eski_surumlerle_uyumlu() {
        let bytes = Opts::default().encode();
        assert_eq!(bytes.len(), 7);
        assert!(Opts::decode(&bytes).unwrap().h264);
        assert!(!Opts::decode(&bytes[..6]).unwrap().h264);
        assert!(Opts::decode(&bytes).unwrap().extend.is_none());
    }

    #[test]
    fn ikinci_monitor_boyutu_tasinir_ve_sinirlari_denetlenir() {
        let opts = Opts {
            extend: Some((1920, 864)),
            ..Opts::default()
        };
        let bytes = opts.encode();
        assert_eq!(bytes.len(), 12);
        assert_eq!(Opts::decode(&bytes).unwrap().extend, Some((1920, 864)));
        let mut huge = bytes.clone();
        huge[8..10].copy_from_slice(&9000u16.to_le_bytes());
        assert_eq!(Opts::decode(&huge).unwrap().extend, None);
        let mut tiny = bytes;
        tiny[10..12].copy_from_slice(&10u16.to_le_bytes());
        assert_eq!(Opts::decode(&tiny).unwrap().extend, None);
    }

    #[test]
    fn h264_kareleri_alicida_gorunur() {
        let (width, height) = (320, 180);
        let mut encoder = video::H264Encoder::new(width, height, 30, 2_000_000).unwrap();
        let mut decoder = None;
        let mut shown = 0;
        for index in 0..6usize {
            let mut bgra = vec![200u8; width * height * 4];
            for pixel in bgra
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .skip(index * 40)
                .take(2000)
            {
                pixel[..3].copy_from_slice(&[20, 90, 240]);
            }
            let payload = encoder.encode(&bgra).unwrap();
            if payload.is_empty() {
                continue;
            }
            let packet = frame_packet(width, height, &payload);
            let frame = decode_payload(&packet[4..], index as u64 + 1, &mut decoder).unwrap();
            if let Some(frame) = frame {
                assert_eq!((frame.w, frame.h), (width, height));
                assert_eq!(frame.rgb.len(), width * height * 3);
                shown += 1;
            }
        }
        assert!(shown >= 3, "yalnız {shown} kare görüntülendi");
    }

    #[test]
    fn jpeg_dimensions_are_checked_before_decoding_pixels() {
        let mut jpeg = Vec::new();
        jpeg_encoder::Encoder::new(&mut jpeg, 70)
            .encode(&[0; 12], 2, 2, jpeg_encoder::ColorType::Rgb)
            .unwrap();
        let mut frame = vec![2, 0, 2, 0];
        frame.extend(jpeg);
        assert!(decode_image(&frame, 1).is_ok());
        frame[0] = 1;
        assert!(decode_image(&frame, 1).is_err());
        frame[..4].copy_from_slice(&[255, 255, 255, 255]);
        assert!(decode_image(&frame, 1).is_err());
        assert!(decode_image(&[0; 4], 1).is_err());
    }

    fn capture(w: usize, h: usize) -> Capture {
        let bgra = (0..w * h)
            .flat_map(|i| [(i % 256) as u8, 100, 200, 255])
            .collect();
        Capture {
            w,
            h,
            stride: w * 4,
            bgra,
        }
    }

    #[test]
    fn genis_ekran_yariya_kuculur() {
        let (w, h, b) = shrink(&capture(3024, 1964), 1920);
        assert_eq!((w, h), (1512, 982));
        assert_eq!(b.len(), w * h * 4);
        assert_eq!((b[1], b[2]), (100, 200));
    }

    #[test]
    fn kare_gonderilir_ve_cozulur() {
        let (w, h, bgra) = shrink(&capture(64, 48), 1920);
        let mut jpeg = Vec::new();
        jpeg_encoder::Encoder::new(&mut jpeg, 70)
            .encode(&bgra, w as u16, h as u16, jpeg_encoder::ColorType::Bgra)
            .unwrap();
        let mut wire = ((jpeg.len() + 4) as u32).to_le_bytes().to_vec();
        wire.extend((w as u16).to_le_bytes());
        wire.extend((h as u16).to_le_bytes());
        wire.extend(jpeg);
        let view = Mutex::new(View::default());
        let _ = receive(&mut &wire[..], &view, &AtomicBool::new(false));
        let v = view.lock().unwrap();
        let f = v.frame.as_ref().expect("kare çözülmedi");
        assert_eq!((f.w, f.h, f.seq), (64, 48, 1));
        let (r, g) = (f.rgb[3 * 10] as i32, f.rgb[3 * 10 + 1] as i32);
        assert!(
            (r - 200).abs() < 12 && (g - 100).abs() < 12,
            "renk kaydı: {r} {g}"
        );
    }

    #[test]
    fn alici_parolayla_baglanan_karsidan_kare_alir() {
        let view: Arc<Mutex<View>> = Arc::default();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let _rx = start_receiver_on(
            listener,
            "127.0.0.1".into(),
            "ortak-parola".into(),
            view.clone(),
        )
        .expect("alıcı açılamadı");
        let s = TcpStream::connect(addr).unwrap();
        let mut s = net::handshake(s, true, "ortak-parola").unwrap();
        let (w, h, bgra) = shrink(&capture(32, 16), 1920);
        let mut jpeg = Vec::new();
        jpeg_encoder::Encoder::new(&mut jpeg, 70)
            .encode(&bgra, w as u16, h as u16, jpeg_encoder::ColorType::Bgra)
            .unwrap();
        let mut wire = ((jpeg.len() + 4) as u32).to_le_bytes().to_vec();
        wire.extend((w as u16).to_le_bytes());
        wire.extend((h as u16).to_le_bytes());
        wire.extend(jpeg);
        s.write_all(&wire).unwrap();
        for _ in 0..50 {
            if let Some(f) = &view.lock().unwrap().frame {
                assert_eq!((f.w, f.h), (32, 16));
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("kare gelmedi");
    }
}
