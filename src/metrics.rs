use std::sync::Weak;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use sysinfo::{Disks, Gpus, Networks, System};

pub const ALL: u8 = 31;
const FIELDS: usize = 12;
pub const CPU: usize = 0;
pub const RAM_USED: usize = 1;
pub const RAM_TOTAL: usize = 2;
pub const GPU: usize = 3;
pub const VRAM_USED: usize = 4;
pub const VRAM_TOTAL: usize = 5;
pub const DISK_USED: usize = 6;
pub const DISK_TOTAL: usize = 7;
pub const DISK_READ: usize = 8;
pub const DISK_WRITE: usize = 9;
pub const RX: usize = 10;
pub const TX: usize = 11;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    pub values: [Option<u64>; FIELDS],
}

#[derive(Clone)]
pub struct TimedSample {
    pub at: Instant,
    pub sample: Sample,
}

pub fn payload(enabled: bool, sample: Option<&TimedSample>) -> Vec<u8> {
    if !enabled {
        return Vec::new();
    }
    sample
        .filter(|s| s.at.elapsed() < Duration::from_secs(6))
        .map(|s| s.sample.encode())
        .unwrap_or_default()
}

impl Sample {
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![1];
        for value in self.values {
            bytes.extend(value.unwrap_or(u64::MAX).to_le_bytes());
        }
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 1 + FIELDS * 8 || bytes[0] != 1 {
            return None;
        }
        let mut sample = Self::default();
        for (slot, b) in sample.values.iter_mut().zip(bytes[1..].chunks_exact(8)) {
            let v = u64::from_le_bytes(b.try_into().ok()?);
            *slot = (v != u64::MAX).then_some(v);
        }
        if [CPU, GPU]
            .iter()
            .any(|&i| sample.values[i].is_some_and(|v| v > 10_000))
        {
            return None;
        }
        for (used, total) in [
            (RAM_USED, RAM_TOTAL),
            (VRAM_USED, VRAM_TOTAL),
            (DISK_USED, DISK_TOTAL),
        ] {
            if matches!((sample.values[used], sample.values[total]), (Some(a), Some(b)) if a > b) {
                return None;
            }
        }
        Some(sample)
    }

    pub fn ratio(&self, category: usize) -> Option<f32> {
        match category {
            0 => self.values[CPU].map(|v| v as f32 / 10000.0),
            1 => ratio(self.values[RAM_USED], self.values[RAM_TOTAL]),
            2 => self.values[GPU].map(|v| v as f32 / 10000.0),
            3 => ratio(self.values[DISK_USED], self.values[DISK_TOTAL]),
            _ => None,
        }
    }

    pub fn detail(&self, category: usize) -> String {
        let percent = |i: usize| {
            self.values[i].map_or("Ölçüm kullanılamıyor".into(), |v| {
                format!("%{:.0}", v as f64 / 100.0)
            })
        };
        let usage =
            |a: usize, b: usize| format!("{} / {}", bytes(self.values[a]), bytes(self.values[b]));
        match category {
            0 => format!("İşlemci kullanımı: {}", percent(CPU)),
            1 => format!("RAM: {}", usage(RAM_USED, RAM_TOTAL)),
            2 => format!(
                "GPU kullanımı: {}\nGPU belleği: {}",
                percent(GPU),
                usage(VRAM_USED, VRAM_TOTAL)
            ),
            3 => format!(
                "Disk doluluğu: {}\nOkuma: {}/sn · Yazma: {}/sn",
                usage(DISK_USED, DISK_TOTAL),
                bytes(self.values[DISK_READ]),
                bytes(self.values[DISK_WRITE])
            ),
            _ => format!(
                "Ağ trafiği\nİndirme: {}/sn · Yükleme: {}/sn\nAktif ağ arayüzlerinin toplamıdır; internet hız testi değildir.",
                bytes(self.values[RX]),
                bytes(self.values[TX])
            ),
        }
    }

    pub fn traffic_label(&self) -> String {
        match (self.values[RX], self.values[TX]) {
            (Some(rx), Some(tx)) => compact_rate(rx.saturating_add(tx)),
            _ => "—".into(),
        }
    }
}

fn compact_rate(bytes_per_second: u64) -> String {
    let mut n = bytes_per_second as f64;
    let units = ["B", "K", "M", "G"];
    let mut i = 0;
    while n >= 1024.0 && i < units.len() - 1 {
        n /= 1024.0;
        i += 1;
    }
    if n < 10.0 && i > 0 {
        format!("{n:.1}{}", units[i])
    } else {
        format!("{n:.0}{}", units[i])
    }
}

fn ratio(a: Option<u64>, b: Option<u64>) -> Option<f32> {
    let (a, b) = (a?, b?);
    (b > 0).then(|| (a as f64 / b as f64).clamp(0.0, 1.0) as f32)
}

fn bytes(value: Option<u64>) -> String {
    let Some(value) = value else {
        return "—".into();
    };
    let mut n = value as f64;
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut i = 0;
    while n >= 1024.0 && i < units.len() - 1 {
        n /= 1024.0;
        i += 1;
    }
    format!("{n:.1} {}", units[i])
}

struct Sampler {
    system: Option<System>,
    disks: Option<Disks>,
    networks: Option<Networks>,
    gpus: Option<Gpus>,
    last: Instant,
    warm: bool,
}

impl Sampler {
    fn new() -> Self {
        Self {
            system: System::new().ok(),
            disks: Disks::new_with_refreshed_list().ok(),
            networks: Networks::new_with_refreshed_list().ok(),
            gpus: Gpus::new_with_refreshed_list().ok(),
            last: Instant::now(),
            warm: false,
        }
    }

    fn sample(&mut self) -> Sample {
        let elapsed = self.last.elapsed().as_secs_f64().max(0.001);
        self.last = Instant::now();
        let mut sample = Sample::default();
        let v = &mut sample.values;
        if let Some(s) = &mut self.system {
            s.refresh_cpu_usage();
            s.refresh_memory();
            if self.warm && !s.cpus().is_empty() {
                v[CPU] = percentage(s.global_cpu_usage());
            }
            if s.total_memory() > 0 {
                v[RAM_TOTAL] = Some(s.total_memory());
                v[RAM_USED] = Some(s.used_memory().min(s.total_memory()));
            }
        }
        if let Some(gpus) = &mut self.gpus {
            gpus.refresh(true);
            if self.warm {
                v[GPU] = gpus
                    .iter()
                    .filter_map(|g| g.usage())
                    .filter_map(percentage)
                    .max();
            }
            let totals: Vec<_> = gpus
                .iter()
                .filter_map(|g| Some((g.used_memory()?, g.total_memory()?)))
                .collect();
            if !totals.is_empty() {
                v[VRAM_USED] = Some(totals.iter().map(|(u, _)| *u).sum());
                v[VRAM_TOTAL] = Some(totals.iter().map(|(_, t)| *t).sum());
            }
        }
        if let Some(disks) = &mut self.disks {
            disks.refresh(true);
            if !disks.is_empty() {
                v[DISK_TOTAL] = Some(disks.iter().map(|d| d.total_space()).sum());
                v[DISK_USED] = Some(
                    disks
                        .iter()
                        .map(|d| d.total_space().saturating_sub(d.available_space()))
                        .sum(),
                );
                if self.warm {
                    v[DISK_READ] = Some(rate(
                        disks.iter().map(|d| d.usage().read_bytes).sum(),
                        elapsed,
                    ));
                    v[DISK_WRITE] = Some(rate(
                        disks.iter().map(|d| d.usage().written_bytes).sum(),
                        elapsed,
                    ));
                }
            }
        }
        if let Some(networks) = &mut self.networks {
            networks.refresh(true);
            if self.warm {
                let active: Vec<_> = networks
                    .iter()
                    .filter(|(_, n)| n.ip_networks().iter().any(|ip| !ip.addr.is_loopback()))
                    .collect();
                if !active.is_empty() {
                    v[RX] = Some(rate(
                        active.iter().map(|(_, n)| n.received()).sum(),
                        elapsed,
                    ));
                    v[TX] = Some(rate(
                        active.iter().map(|(_, n)| n.transmitted()).sum(),
                        elapsed,
                    ));
                }
            }
        }
        self.warm = true;
        sample
    }
}

fn percentage(value: f32) -> Option<u64> {
    value
        .is_finite()
        .then(|| (value.clamp(0.0, 100.0) * 100.0).round() as u64)
}
fn rate(bytes: u64, seconds: f64) -> u64 {
    (bytes as f64 / seconds) as u64
}

pub fn start(shared: Weak<crate::core::Shared>) {
    std::thread::spawn(move || {
        let mut sampler = None;
        loop {
            let Some(sh) = shared.upgrade() else {
                return;
            };
            if sh.stop.load(Ordering::Relaxed) {
                return;
            }
            if sh.metrics_share.load(Ordering::Relaxed) || sh.metrics_show.load(Ordering::Relaxed) {
                let s = sampler.get_or_insert_with(Sampler::new);
                let sample = s.sample();
                *sh.metrics_local.lock().unwrap() = Some(TimedSample {
                    at: Instant::now(),
                    sample,
                });
            } else {
                sampler = None;
                *sh.metrics_local.lock().unwrap() = None;
            }
            drop(sh);
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traffic_rate_fits_under_the_icon() {
        assert_eq!(compact_rate(512), "512B");
        assert_eq!(compact_rate(1536), "1.5K");
        assert_eq!(compact_rate(20 * 1024 * 1024), "20M");
    }

    #[test]
    fn wire_roundtrip_and_invalid_values() {
        let mut s = Sample::default();
        s.values[CPU] = Some(1234);
        s.values[RAM_USED] = Some(4);
        s.values[RAM_TOTAL] = Some(8);
        assert_eq!(Sample::decode(&s.encode()), Some(s.clone()));
        assert_eq!(s.ratio(1), Some(0.5));
        assert!(Sample::decode(&s.encode()[..10]).is_none());
        s.values[CPU] = Some(10001);
        assert!(Sample::decode(&s.encode()).is_none());
        s.values[CPU] = None;
        s.values[RAM_USED] = Some(9);
        assert!(Sample::decode(&s.encode()).is_none());
        assert_eq!(percentage(f32::NAN), None);
        assert_eq!(rate(4000, 2.0), 2000);
    }

    #[test]
    fn cached_measurements_are_not_sent_without_permission_or_when_stale() {
        let mut sample = TimedSample {
            at: Instant::now(),
            sample: Sample::default(),
        };
        sample.sample.values[CPU] = Some(4200);
        assert!(payload(false, Some(&sample)).is_empty());
        assert_eq!(payload(true, Some(&sample)), sample.sample.encode());
        sample.at = Instant::now() - Duration::from_secs(7);
        assert!(payload(true, Some(&sample)).is_empty());
    }

    #[test]
    #[ignore = "reads real performance counters"]
    fn live_sample() {
        let mut sampler = Sampler::new();
        sampler.sample();
        std::thread::sleep(Duration::from_secs(2));
        let sample = sampler.sample();
        println!("{sample:?}");
        assert!(Sample::decode(&sample.encode()).is_some());
        assert!(sample.values[RAM_TOTAL].is_some_and(|v| v > 0));
    }
}
