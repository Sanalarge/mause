use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use std::collections::VecDeque;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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

pub const HAS_VOLUME: bool = false;

const RATE: u32 = 48_000;
const FRAMES: usize = 960;
const PORT_BASE: u16 = 24802;
const VIRTUAL_MICS: [&str; 3] = ["BlackHole", "CABLE Input", "VB-Audio"];

static CHOSEN: Mutex<[String; 2]> = Mutex::new([String::new(), String::new()]);

fn slot(kind: Kind) -> usize {
    match kind {
        Kind::Sink => 0,
        Kind::Source => 1,
    }
}

fn name_of(d: &cpal::Device) -> String {
    d.description()
        .map(|x| x.name().to_owned())
        .unwrap_or_else(|_| d.to_string())
}

fn devices(kind: Kind) -> Vec<cpal::Device> {
    let host = cpal::default_host();
    let list = match kind {
        Kind::Sink => host.output_devices().map(|d| d.collect()),
        Kind::Source => host.input_devices().map(|d| d.collect()),
    };
    list.unwrap_or_default()
}

fn find(kind: Kind, name: &str) -> Option<cpal::Device> {
    if !name.is_empty() {
        if let Some(d) = devices(kind).into_iter().find(|d| name_of(d) == name) {
            return Some(d);
        }
    }
    let host = cpal::default_host();
    match kind {
        Kind::Sink => host.default_output_device(),
        Kind::Source => host.default_input_device(),
    }
}

pub fn available() -> bool {
    let host = cpal::default_host();
    host.default_output_device().is_some() || host.default_input_device().is_some()
}

pub fn list(kind: Kind) -> Vec<Dev> {
    devices(kind)
        .iter()
        .map(|d| {
            let name = name_of(d);
            Dev {
                desc: name.clone(),
                name,
            }
        })
        .collect()
}

pub fn default(kind: Kind) -> Option<String> {
    let chosen = CHOSEN.lock().unwrap()[slot(kind)].clone();
    if !chosen.is_empty() {
        return Some(chosen);
    }
    find(kind, "").map(|d| name_of(&d))
}

pub fn set_default(kind: Kind, name: &str) {
    CHOSEN.lock().unwrap()[slot(kind)] = name.to_owned();
}

pub fn volume(_: Kind) -> Option<(u32, bool)> {
    None
}
pub fn set_volume(_: Kind, _: u32) {}
pub fn set_mute(_: Kind, _: bool) {}

pub struct Handle {
    stop: Arc<AtomicBool>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

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

struct Resampler {
    step: f64,
    t: f64,
    prev: [f32; 2],
}

impl Resampler {
    fn new(from: u32, to: u32) -> Self {
        Self {
            step: from as f64 / to as f64,
            t: 0.0,
            prev: [0.0; 2],
        }
    }

    fn run(&mut self, input: &[[f32; 2]], out: &mut Vec<[f32; 2]>) {
        if input.is_empty() {
            return;
        }
        let at = |i: usize| if i == 0 { self.prev } else { input[i - 1] };
        let last = input.len();
        while self.t + 1.0 <= last as f64 {
            let i = self.t as usize;
            let f = (self.t - i as f64) as f32;
            let (a, b) = (at(i), at(i + 1));
            out.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]);
            self.t += self.step;
        }
        self.t -= last as f64;
        self.prev = input[last - 1];
    }
}

fn to_stereo<T: Sample>(data: &[T], channels: usize) -> Vec<[f32; 2]>
where
    f32: FromSample<T>,
{
    data.chunks_exact(channels.max(1))
        .map(|c| {
            let l = f32::from_sample(c[0]);
            [
                l,
                if channels > 1 {
                    f32::from_sample(c[1])
                } else {
                    l
                },
            ]
        })
        .collect()
}

fn input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: mpsc::Sender<Vec<[f32; 2]>>,
) -> Option<cpal::Stream>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let channels = config.channels as usize;
    device
        .build_input_stream::<T, _, _>(
            *config,
            move |data, _| drop(tx.send(to_stereo(data, channels))),
            |_| {},
            None,
        )
        .ok()
}

pub fn start_sender(kind: u8, ip: &str, dev: &str, key: [u8; 32]) -> Option<Handle> {
    let device = if kind == 0 {
        find(Kind::Sink, dev)?
    } else {
        find(Kind::Source, dev)?
    };
    let supported = if kind == 0 {
        device.default_output_config()
    } else {
        device.default_input_config()
    }
    .ok()?;
    let config = supported.config();
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    let target = format!("{ip}:{}", PORT_BASE + kind as u16);
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel();
    let st = stop.clone();
    thread::spawn(move || {
        let (tx, rx) = mpsc::channel::<Vec<[f32; 2]>>();
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => input_stream::<f32>(&device, &config, tx),
            cpal::SampleFormat::I16 => input_stream::<i16>(&device, &config, tx),
            cpal::SampleFormat::I32 => input_stream::<i32>(&device, &config, tx),
            _ => None,
        };
        let Some(stream) = stream.filter(|s| s.play().is_ok()) else {
            let _ = ready_tx.send(false);
            return;
        };
        let _ = ready_tx.send(true);
        let cipher = ChaCha20Poly1305::new(&key.into());
        let mut resampler = Resampler::new(config.sample_rate, RATE);
        let (mut frames, mut counter) = (Vec::<[f32; 2]>::new(), first_counter());
        while !st.load(Ordering::Relaxed) {
            let Ok(chunk) = rx.recv_timeout(Duration::from_millis(200)) else {
                continue;
            };
            resampler.run(&chunk, &mut frames);
            while frames.len() >= FRAMES {
                let pcm: Vec<u8> = frames
                    .drain(..FRAMES)
                    .flat_map(|[l, r]| [i16::from_sample(l), i16::from_sample(r)])
                    .flat_map(i16::to_le_bytes)
                    .collect();
                if let Ok(ct) = cipher.encrypt(&nonce(counter).into(), pcm.as_ref()) {
                    let mut pkt = counter.to_be_bytes().to_vec();
                    pkt.extend(ct);
                    let _ = sock.send_to(&pkt, &target);
                }
                counter += 1;
            }
        }
        drop(stream);
    });
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .ok()
        .filter(|ok| *ok)
        .map(|_| Handle { stop })
}

fn output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buf: Arc<Mutex<Jitter>>,
) -> Option<cpal::Stream>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let channels = config.channels as usize;
    device
        .build_output_stream::<T, _, _>(
            *config,
            move |out: &mut [T], _| {
                let mut b = buf.lock().unwrap();
                for frame in out.chunks_exact_mut(channels.max(1)) {
                    let [l, r] = b.pop();
                    for (c, s) in frame.iter_mut().enumerate() {
                        *s = T::from_sample(if c % 2 == 0 { l } else { r });
                    }
                }
            },
            |_| {},
            None,
        )
        .ok()
}

struct Jitter {
    frames: VecDeque<[f32; 2]>,
    target: usize,
    playing: bool,
}

impl Jitter {
    fn push(&mut self, input: &[[f32; 2]]) {
        self.frames.extend(input);
        if self.frames.len() > self.target * 3 {
            let extra = self.frames.len() - self.target;
            self.frames.drain(..extra);
        }
    }

    fn pop(&mut self) -> [f32; 2] {
        if !self.playing {
            if self.frames.len() < self.target {
                return [0.0; 2];
            }
            self.playing = true;
        }
        match self.frames.pop_front() {
            Some(f) => f,
            None => {
                self.playing = false;
                [0.0; 2]
            }
        }
    }
}

pub fn start_receiver(
    kind: u8,
    ip: &str,
    latency_ms: u32,
    last: Arc<Mutex<Option<Instant>>>,
    key: [u8; 32],
) -> Option<Handle> {
    let chosen = CHOSEN.lock().unwrap()[0].clone();
    let device = if kind == 1 {
        devices(Kind::Sink)
            .into_iter()
            .find(|d| VIRTUAL_MICS.iter().any(|v| name_of(d).contains(v)))
            .or_else(|| find(Kind::Sink, &chosen))?
    } else {
        find(Kind::Sink, &chosen)?
    };
    let supported = device.default_output_config().ok()?;
    let config = supported.config();
    let sock = UdpSocket::bind(("0.0.0.0", PORT_BASE + kind as u16)).ok()?;
    sock.set_read_timeout(Some(Duration::from_millis(300))).ok();
    let target = (config.sample_rate as usize * latency_ms.max(20) as usize / 1000).max(FRAMES);
    let buf = Arc::new(Mutex::new(Jitter {
        frames: VecDeque::new(),
        target,
        playing: false,
    }));
    let (stop, allowed) = (Arc::new(AtomicBool::new(false)), ip.to_string());
    let (ready_tx, ready_rx) = mpsc::channel();
    let st = stop.clone();
    thread::spawn(move || {
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => output_stream::<f32>(&device, &config, buf.clone()),
            cpal::SampleFormat::I16 => output_stream::<i16>(&device, &config, buf.clone()),
            cpal::SampleFormat::I32 => output_stream::<i32>(&device, &config, buf.clone()),
            _ => None,
        };
        let Some(stream) = stream.filter(|s| s.play().is_ok()) else {
            let _ = ready_tx.send(false);
            return;
        };
        let _ = ready_tx.send(true);
        let cipher = ChaCha20Poly1305::new(&key.into());
        let mut resampler = Resampler::new(RATE, config.sample_rate);
        let (mut packet, mut out) = ([0u8; 8192], Vec::new());
        let mut newest: Option<u64> = None;
        while !st.load(Ordering::Relaxed) {
            match sock.recv_from(&mut packet) {
                Ok((n, from)) if from.ip().to_string() == allowed && n > 8 + 16 => {
                    let counter = u64::from_be_bytes(packet[..8].try_into().unwrap());
                    if newest.is_some_and(|m| counter <= m) {
                        continue;
                    }
                    let Ok(pcm) = cipher.decrypt(&nonce(counter).into(), &packet[8..n]) else {
                        continue;
                    };
                    newest = Some(counter);
                    *last.lock().unwrap() = Some(Instant::now());
                    let frames: Vec<[f32; 2]> = pcm
                        .chunks_exact(4)
                        .map(|b| {
                            let s =
                                |i: usize| f32::from_sample(i16::from_le_bytes([b[i], b[i + 1]]));
                            [s(0), s(2)]
                        })
                        .collect();
                    out.clear();
                    resampler.run(&frames, &mut out);
                    buf.lock().unwrap().push(&out);
                }
                _ => {}
            }
        }
        drop(stream);
    });
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .ok()
        .filter(|ok| *ok)
        .map(|_| Handle { stop })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hiz_donusumu_uzunlugu_korur_ve_parcalar_arasinda_kopmaz() {
        let input: Vec<[f32; 2]> = (0..44_100).map(|i| [i as f32 / 44_100.0; 2]).collect();
        let mut r = Resampler::new(44_100, RATE);
        let mut out = Vec::new();
        for part in input.chunks(441 * 7) {
            r.run(part, &mut out);
        }
        assert!((out.len() as i64 - 48_000).abs() <= 2, "{}", out.len());
        assert!(
            out.windows(2)
                .all(|w| w[1][0] >= w[0][0] - 1e-6 && w[1][0] - w[0][0] < 1e-3)
        );
    }

    #[test]
    fn jitter_preserves_samples_until_playback_and_after_underrun() {
        let mut j = Jitter {
            frames: VecDeque::new(),
            target: 4,
            playing: false,
        };
        for _ in 0..2 {
            for value in 1..4 {
                j.push(&[[value as f32; 2]]);
                for _ in 0..8 {
                    assert_eq!(j.pop(), [0.0; 2]);
                }
                assert_eq!(j.frames.len(), value);
            }
            j.push(&[[4.0; 2]]);
            for value in 1..=4 {
                assert_eq!(j.pop(), [value as f32; 2]);
            }
            assert_eq!(j.pop(), [0.0; 2]);
            assert!(!j.playing);
        }
    }

    #[test]
    fn tampon_dolmadan_calmaz_ve_tasarsa_eskiyi_atar() {
        let mut j = Jitter {
            frames: VecDeque::new(),
            target: 4,
            playing: false,
        };
        j.push(&[[0.5; 2]; 3]);
        assert_eq!(j.pop(), [0.0; 2], "hedef dolmadan sessiz");
        j.push(&[[0.5; 2]; 20]);
        assert!(j.frames.len() <= 12);
        assert_eq!(j.pop(), [0.5; 2]);
    }

    #[test]
    #[ignore]
    fn alici_gercek_cihazda_sifreli_paketi_calar() {
        let key = [9u8; 32];
        let last: Arc<Mutex<Option<Instant>>> = Arc::default();
        let _rx =
            start_receiver(0, "127.0.0.1", 60, last.clone(), key).expect("çıkış cihazı açılamadı");
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let cipher = ChaCha20Poly1305::new(&key.into());
        for counter in 0..10u64 {
            let ct = cipher
                .encrypt(&nonce(counter).into(), [0u8; 3840].as_ref())
                .unwrap();
            let mut pkt = counter.to_be_bytes().to_vec();
            pkt.extend(ct);
            sock.send_to(&pkt, ("127.0.0.1", PORT_BASE)).unwrap();
            thread::sleep(Duration::from_millis(20));
        }
        thread::sleep(Duration::from_millis(100));
        assert!(last.lock().unwrap().is_some(), "paket alınmadı");
    }

    #[test]
    fn mac_ve_linux_ayni_paketi_anlar() {
        let mut r = Resampler::new(RATE, RATE);
        let mut out = Vec::new();
        r.run(&[[0.25, -0.25]; FRAMES + 1], &mut out);
        assert!(out.len() >= FRAMES);
        let pcm: Vec<u8> = out[..FRAMES]
            .iter()
            .flat_map(|[l, r]| [i16::from_sample(*l), i16::from_sample(*r)])
            .flat_map(i16::to_le_bytes)
            .collect();
        assert_eq!(pcm.len(), 3840);
    }
}
