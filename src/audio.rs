use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit};
use std::io::{Read, Write};
use std::net::UdpSocket;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const HAS_VOLUME: bool = true;

#[derive(Clone, PartialEq)]
pub struct Dev {
    pub name: String,
    pub desc: String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Sink,
    Source,
}

impl Kind {
    fn plural(self) -> &'static str {
        match self {
            Kind::Sink => "sinks",
            Kind::Source => "sources",
        }
    }
    fn special(self) -> &'static str {
        match self {
            Kind::Sink => "@DEFAULT_SINK@",
            Kind::Source => "@DEFAULT_SOURCE@",
        }
    }
    fn word(self) -> &'static str {
        match self {
            Kind::Sink => "sink",
            Kind::Source => "source",
        }
    }
}

fn pactl(args: &[&str]) -> Option<String> {
    let out = Command::new("pactl")
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn available() -> bool {
    pactl(&["info"]).is_some()
}

pub fn list(kind: Kind) -> Vec<Dev> {
    let Some(text) = pactl(&["list", kind.plural()]) else {
        return vec![];
    };
    let mut v = Vec::new();
    let mut name = String::new();
    for l in text.lines().map(str::trim) {
        if let Some(n) = l.strip_prefix("Name: ") {
            name = n.to_string();
        } else if let Some(d) = l.strip_prefix("Description: ") {
            if !name.is_empty() && !(kind == Kind::Source && name.ends_with(".monitor")) {
                v.push(Dev {
                    name: std::mem::take(&mut name),
                    desc: d.to_string(),
                });
            }
        }
    }
    v
}

pub fn default(kind: Kind) -> Option<String> {
    pactl(&[&format!("get-default-{}", kind.word())]).map(|s| s.trim().to_string())
}

pub fn set_default(kind: Kind, name: &str) {
    if pactl(&[&format!("set-default-{}", kind.word()), name]).is_none() {
        return;
    }
    let (list_cmd, move_cmd) = match kind {
        Kind::Sink => ("sink-inputs", "move-sink-input"),
        Kind::Source => ("source-outputs", "move-source-output"),
    };
    if let Some(t) = pactl(&["list", "short", list_cmd]) {
        for id in t.lines().filter_map(|l| l.split_whitespace().next()) {
            let _ = pactl(&[move_cmd, id, name]);
        }
    }
}

pub fn volume(kind: Kind) -> Option<(u32, bool)> {
    let v = pactl(&[&format!("get-{}-volume", kind.word()), kind.special()])?;
    let pct = v
        .split('/')
        .find_map(|p| p.trim().strip_suffix('%')?.trim().parse::<u32>().ok())?;
    let m = pactl(&[&format!("get-{}-mute", kind.word()), kind.special()])?;
    Some((pct, m.contains("yes")))
}

pub fn set_volume(kind: Kind, pct: u32) {
    let _ = pactl(&[
        &format!("set-{}-volume", kind.word()),
        kind.special(),
        &format!("{pct}%"),
    ]);
}

pub fn set_mute(kind: Kind, mute: bool) {
    let _ = pactl(&[
        &format!("set-{}-mute", kind.word()),
        kind.special(),
        if mute { "1" } else { "0" },
    ]);
}

const CHUNK: usize = 3840;
const PACKET: usize = 1280;
const PORT_BASE: u16 = 24802;
const MIC_SINK: &str = "mause_mic";

pub struct Handle {
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    modules: Vec<String>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        for m in self.modules.iter().rev() {
            let _ = pactl(&["unload-module", m]);
        }
    }
}

/// Göndericinin ilk paket sayacı. Saatten (mikrosaniye) türetilir; böylece aynı
/// oturumda gönderici yeniden başlatılsa bile sayaç hep ileri gider: alıcı yeni
/// paketleri "eski" sanıp atmaz ve aynı anahtarla nonce tekrar kullanılmaz.
fn first_counter() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

fn nonce(counter: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&counter.to_be_bytes());
    n
}

pub fn start_sender(kind: u8, ip: &str, dev: &str, key: [u8; 32]) -> Option<Handle> {
    let device = if kind == 0 {
        let sink = if dev.is_empty() {
            default(Kind::Sink)?
        } else {
            dev.to_string()
        };
        format!("{sink}.monitor")
    } else if dev.is_empty() {
        default(Kind::Source)?
    } else {
        dev.to_string()
    };
    let mut child = Command::new("parec")
        .args([
            "-d",
            &device,
            "--raw",
            "--format=s16le",
            "--rate=48000",
            "--channels=2",
            "--latency-msec=20",
            "--client-name=mause",
            "--stream-name=mause-relay",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    let target = format!("{ip}:{}", PORT_BASE + kind as u16);
    let stop = Arc::new(AtomicBool::new(false));
    let st = stop.clone();
    thread::spawn(move || {
        let cipher = ChaCha20Poly1305::new(&key.into());
        let mut buf = [0u8; CHUNK];
        let mut counter = first_counter();
        while !st.load(Ordering::Relaxed) && out.read_exact(&mut buf).is_ok() {
            for part in buf.chunks(PACKET) {
                if let Ok(ct) = cipher.encrypt(&nonce(counter).into(), part) {
                    let mut pkt = counter.to_be_bytes().to_vec();
                    pkt.extend(ct);
                    let _ = sock.send_to(&pkt, &target);
                }
                counter += 1;
            }
        }
    });
    Some(Handle {
        stop,
        child: Arc::new(Mutex::new(Some(child))),
        modules: vec![],
    })
}

pub fn start_receiver(
    kind: u8,
    ip: &str,
    latency_ms: u32,
    last: Arc<Mutex<Option<Instant>>>,
    key: [u8; 32],
) -> Option<Handle> {
    let mut modules = Vec::new();
    let mut args: Vec<String> = [
        "--playback",
        "--raw",
        "--format=s16le",
        "--rate=48000",
        "--channels=2",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(format!("--latency-msec={latency_ms}"));
    args.push("--client-name=mause".into());
    args.push(format!("--stream-name=mause-relay-{kind}"));
    if kind == 1 {
        let m1 = pactl(&[
            "load-module",
            "module-null-sink",
            &format!("sink_name={MIC_SINK}"),
            "sink_properties=device.description=Mause-Mikrofon",
        ])?;
        modules.push(m1.trim().to_string());
        if let Some(m2) = pactl(&[
            "load-module",
            "module-remap-source",
            &format!("master={MIC_SINK}.monitor"),
            "source_name=mause_mic_in",
            "source_properties=device.description=Mause-Mikrofon",
        ]) {
            modules.push(m2.trim().to_string());
        }
        args.push(format!("--device={MIC_SINK}"));
    }
    let child = Command::new("pacat")
        .args(&args)
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(_) => {
            for m in modules.iter().rev() {
                let _ = pactl(&["unload-module", m]);
            }
            return None;
        }
    };
    let mut stdin = child.stdin.take()?;
    let sock = UdpSocket::bind(("0.0.0.0", PORT_BASE + kind as u16)).ok()?;
    sock.set_read_timeout(Some(Duration::from_millis(300))).ok();
    let (stop, allowed) = (Arc::new(AtomicBool::new(false)), ip.to_string());
    let st = stop.clone();
    thread::spawn(move || {
        let cipher = ChaCha20Poly1305::new(&key.into());
        let mut buf = [0u8; 8192];
        let mut newest: Option<u64> = None;
        while !st.load(Ordering::Relaxed) {
            match sock.recv_from(&mut buf) {
                Ok((n, from)) if from.ip().to_string() == allowed && n > 8 + 16 => {
                    let mut c = [0u8; 8];
                    c.copy_from_slice(&buf[..8]);
                    let counter = u64::from_be_bytes(c);
                    if newest.is_some_and(|m| counter <= m) {
                        continue;
                    }
                    let Ok(pcm) = cipher.decrypt(&nonce(counter).into(), &buf[8..n]) else {
                        continue;
                    };
                    newest = Some(counter);
                    *last.lock().unwrap() = Some(Instant::now());
                    if stdin.write_all(&pcm).is_err() {
                        return;
                    }
                }
                _ => {}
            }
        }
    });
    Some(Handle {
        stop,
        child: Arc::new(Mutex::new(Some(child))),
        modules,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(kind: u8) -> bool {
        let last: Arc<Mutex<Option<Instant>>> = Arc::default();
        let rx = start_receiver(kind, "127.0.0.1", 60, last.clone(), [7u8; 32]).expect("alıcı");
        if kind == 1 {
            let srcs: Vec<_> = list(Kind::Source).iter().map(|d| d.desc.clone()).collect();
            println!("mikrofon açıkken kaynaklar: {srcs:?}");
            assert!(srcs.iter().any(|d| d.contains("Mause-Mikrofon")));
        }
        let tx = start_sender(kind, "127.0.0.1", "", [7u8; 32]).expect("gönderici");
        thread::sleep(Duration::from_millis(1500));
        let got = last
            .lock()
            .unwrap()
            .is_some_and(|t| t.elapsed() < Duration::from_secs(1));
        drop(tx);
        drop(rx);
        got
    }

    #[test]
    #[ignore = "hardware: gerçek PulseAudio/PipeWire ve ses cihazları gerekir"]
    fn ses_ve_mikrofon_aktarimi() {
        assert!(run(0), "PC sesi paketi gelmedi");
        assert!(run(1), "mikrofon paketi gelmedi");
        let srcs: Vec<_> = list(Kind::Source).iter().map(|d| d.desc.clone()).collect();
        println!("kapandıktan sonra kaynaklar: {srcs:?}");
        assert!(
            !srcs.iter().any(|d| d.contains("Mause-Mikrofon")),
            "sanal mikrofon temizlenmedi"
        );
    }
}
