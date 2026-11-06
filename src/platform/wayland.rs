use super::Capture;
use ashpd::desktop::PersistMode;
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::param::video::VideoFormat;
use std::os::fd::OwnedFd;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const IDLE: Duration = Duration::from_secs(5);
static FPS: AtomicU8 = AtomicU8::new(30);

pub fn set_fps(fps: u8) {
    FPS.store(fps.clamp(1, 60), Ordering::Relaxed);
}

fn store_interval(fps: u8) -> Duration {
    Duration::from_nanos(1_000_000_000 / fps.clamp(1, 60) as u64)
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static FRAME: Mutex<Option<Capture>> = Mutex::new(None);
static LAST_USE: Mutex<Option<Instant>> = Mutex::new(None);
static ERROR: Mutex<Option<String>> = Mutex::new(None);
static GEOMETRY: Mutex<Option<super::Monitor>> = Mutex::new(None);
static CURSOR: AtomicBool = AtomicBool::new(true);
static VIRTUAL: Mutex<Option<(u16, u16)>> = Mutex::new(None);

pub fn set_cursor(on: bool) {
    CURSOR.store(on, Ordering::Relaxed);
}

pub fn geometry() -> Option<super::Monitor> {
    GEOMETRY.lock().unwrap().clone()
}

pub fn active() -> bool {
    std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
}

pub fn start() {
    begin(None);
}

pub fn start_virtual(size: (u16, u16)) {
    begin(Some(size));
}

fn begin(virtual_size: Option<(u16, u16)>) {
    if RUNNING.load(Ordering::SeqCst) && *VIRTUAL.lock().unwrap() != virtual_size {
        *LAST_USE.lock().unwrap() = None;
        let deadline = Instant::now() + Duration::from_secs(4);
        while RUNNING.load(Ordering::SeqCst) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
    }
    *VIRTUAL.lock().unwrap() = virtual_size;
    *LAST_USE.lock().unwrap() = Some(Instant::now());
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    *ERROR.lock().unwrap() = None;
    thread::spawn(|| {
        if let Err(e) = run() {
            eprintln!("ekran yakalama: {e}");
            *ERROR.lock().unwrap() = Some(e);
        }
        *FRAME.lock().unwrap() = None;
        RUNNING.store(false, Ordering::SeqCst);
    });
}

pub fn latest() -> Option<Capture> {
    *LAST_USE.lock().unwrap() = Some(Instant::now());
    FRAME.lock().unwrap().as_ref().map(|c| Capture {
        w: c.w,
        h: c.h,
        stride: c.stride,
        bgra: c.bgra.clone(),
    })
}

pub fn error() -> Option<String> {
    ERROR.lock().unwrap().clone()
}

pub fn pending() -> bool {
    RUNNING.load(Ordering::SeqCst) && ERROR.lock().unwrap().is_none()
}

fn token_path() -> std::path::PathBuf {
    super::config_dir().join("ekran-izni")
}

fn virtual_geometry() -> Option<super::Monitor> {
    let output = std::process::Command::new("kscreen-doctor")
        .arg("-o")
        .output()
        .ok()?;
    parse_virtual_geometry(&String::from_utf8_lossy(&output.stdout))
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_virtual_geometry(text: &str) -> Option<super::Monitor> {
    let mut name: Option<String> = None;
    for line in strip_ansi(text).lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("Output:") {
            name = rest
                .split_whitespace()
                .nth(1)
                .filter(|n| n.starts_with("Virtual"))
                .map(str::to_owned);
        } else if let (Some(found), Some(rest)) = (&name, line.strip_prefix("Geometry:")) {
            let (position, size) = rest.trim().split_once(' ')?;
            let (x, y) = position.split_once(',')?;
            let (w, h) = size.split_once('x')?;
            return Some(super::Monitor {
                name: found.clone(),
                x: x.parse().ok()?,
                y: y.parse().ok()?,
                w: w.parse().ok()?,
                h: h.parse().ok()?,
            });
        }
    }
    None
}

fn run() -> Result<(), String> {
    let virtual_size = *VIRTUAL.lock().unwrap();
    let token = std::fs::read_to_string(token_path())
        .ok()
        .filter(|t| !t.trim().is_empty())
        .filter(|_| virtual_size.is_none());
    let (proxy, session, node, fd) = async_io::block_on(async {
        let proxy = Screencast::new().await?;
        let session = proxy.create_session(Default::default()).await?;
        proxy
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(if CURSOR.load(Ordering::Relaxed) { CursorMode::Embedded } else { CursorMode::Hidden })
                    .set_sources(ashpd::enumflags2::BitFlags::from(if virtual_size.is_some() {
                        SourceType::Virtual
                    } else {
                        SourceType::Monitor
                    }))
                    .set_multiple(false)
                    .set_persist_mode(if virtual_size.is_some() {
                        PersistMode::DoNot
                    } else {
                        PersistMode::ExplicitlyRevoked
                    })
                    .set_restore_token(token.as_deref().map(str::trim)),
            )
            .await?;
        let streams = proxy.start(&session, None, Default::default()).await?.response()?;
        if let Some(t) = streams.restore_token() {
            let _ = std::fs::create_dir_all(super::config_dir());
            let _ = std::fs::write(token_path(), t);
        }
        let first = streams.streams().first();
        let mut geometry = first.and_then(|s| {
            let (x, y) = s.position()?;
            let (w, h) = s.size()?;
            Some(super::Monitor { name: String::new(), x, y, w, h })
        });
        if virtual_size.is_some() && geometry.is_none() {
            for _ in 0..30 {
                geometry = virtual_geometry();
                if geometry.is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        *GEOMETRY.lock().unwrap() = geometry;
        let node = first.map(|s| s.pipe_wire_node_id());
        let fd = proxy.open_pipe_wire_remote(&session, Default::default()).await?;
        Ok::<_, ashpd::Error>((proxy, session, node, fd))
    })
    .map_err(|e| {
        if unsafe { libc::geteuid() } == 0 || e.to_string().contains("/proc/") {
            format!(
                "ekran paylaşımı izni alınamadı: Mause yönetici olarak, sudo/sg ile ya da bir yapay zekâ \
                 asistanının yalıtılmış terminalinden başlatılmış olabilir. Masaüstü portalı buna izin \
                 vermez; Mause'u kapatıp uygulama menüsünden açın ({e})"
            )
        } else {
            format!("ekran paylaşımı izni alınamadı: {e}")
        }
    })?;
    let r = match node {
        Some(node) => stream(fd, node),
        None => Err("paylaşılacak ekran seçilmedi".into()),
    };
    let _ = async_io::block_on(session.close());
    drop(proxy);
    r
}

struct Frames {
    info: spa::param::video::VideoInfoRaw,
    stored: Instant,
}

fn stream(fd: OwnedFd, node: u32) -> Result<(), String> {
    pw::init();
    let err = |e: pw::Error| format!("PipeWire: {e}");
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(err)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(err)?;
    let core = context.connect_fd_rc(fd, None).map_err(err)?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "mause-ekran",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(err)?;
    let quit = mainloop.clone();
    let _listener = stream
        .add_local_listener_with_user_data(Frames {
            info: Default::default(),
            stored: Instant::now() - Duration::from_secs(1),
        })
        .state_changed(move |_, _, _, new| {
            if matches!(
                new,
                pw::stream::StreamState::Error(_) | pw::stream::StreamState::Unconnected
            ) {
                quit.quit();
            }
        })
        .param_changed(|_, frames, id, param| {
            let Some(param) = param else { return };
            if id == spa::param::ParamType::Format.as_raw() {
                let _ = frames.info.parse(param);
            }
        })
        .process(|stream, frames| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            if frames.stored.elapsed() < store_interval(FPS.load(Ordering::Relaxed)) {
                return;
            }
            let Some(d) = buffer.datas_mut().first_mut() else {
                return;
            };
            let (w, h) = (
                frames.info.size().width as usize,
                frames.info.size().height as usize,
            );
            let (stride, offset) = (
                d.chunk().stride().max(0) as usize,
                d.chunk().offset() as usize,
            );
            let stride = if stride == 0 { w * 4 } else { stride };
            let Some(bytes) = d.data() else { return };
            let swap_rb = matches!(frames.info.format(), VideoFormat::RGBx | VideoFormat::RGBA);
            if !matches!(
                frames.info.format(),
                VideoFormat::RGBx | VideoFormat::RGBA | VideoFormat::BGRx | VideoFormat::BGRA
            ) {
                return;
            }
            let Some(frame) = Capture::from_bytes(w, h, stride, offset, bytes, swap_rb) else {
                return;
            };
            frames.stored = Instant::now();
            *FRAME.lock().unwrap() = Some(frame);
        })
        .register()
        .map_err(err)?;

    let format = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 30, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 240, denom: 1 }
        ),
    );
    let bytes = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(format),
    )
    .map_err(|e| format!("PipeWire biçimi: {e:?}"))?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&bytes).ok_or("PipeWire biçimi oluşturulamadı")?];
    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(err)?;

    let quit = mainloop.clone();
    let timer = mainloop.loop_().add_timer(move |_| {
        if LAST_USE.lock().unwrap().is_none_or(|t| t.elapsed() > IDLE) {
            quit.quit();
        }
    });
    timer.update_timer(Some(Duration::from_secs(1)), Some(Duration::from_secs(1)));
    mainloop.run();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanal_cikis_konumu_kscreen_ciktisindan_okunur() {
        let text = "\u{1b}[01;32mOutput: \u{1b}[0;0m1 HDMI-A-2 a49a\n\t\u{1b}[01;33mGeometry: \u{1b}[0;0m0,0 2560x1440\n\u{1b}[01;32mOutput: \u{1b}[0;0m2 Virtual-xdp-1 b2c3\n\tenabled\n\t\u{1b}[01;33mGeometry: \u{1b}[0;0m2560,0 1920x864\n";
        let monitor = parse_virtual_geometry(text).unwrap();
        assert_eq!(
            (
                monitor.name.as_str(),
                monitor.x,
                monitor.y,
                monitor.w,
                monitor.h
            ),
            ("Virtual-xdp-1", 2560, 0, 1920, 864)
        );
        assert!(
            parse_virtual_geometry("Output: 1 HDMI-A-2 u\n\tGeometry: 0,0 1920x1080\n").is_none()
        );
    }

    #[test]
    fn capture_cadence_tracks_the_requested_fps() {
        assert!(store_interval(60) < Duration::from_millis(17));
        assert!(store_interval(30) < Duration::from_millis(34));
        assert_eq!(store_interval(0), Duration::from_secs(1));
        assert_eq!(store_interval(255), store_interval(60));
    }
}
