use crate::platform::{Ev, ev};
use std::time::{Duration, Instant};

const BUCKETS: usize = 8;
const WINDOW: Duration = Duration::from_millis(40);
const STALE: Duration = Duration::from_millis(150);
const IDLE: Duration = Duration::from_millis(100);
const WALL_MARGIN: i32 = 3;

pub type Screen = (i32, i32, i32, i32);

pub struct Gain {
    table: [f32; BUCKETS],
    seen: [bool; BUCKETS],
    raw: (i32, i32),
    window_start: Instant,
    cursor_start: Option<(i32, i32)>,
    speed: f32,
    last_send: Option<Instant>,
    remainder: (f32, f32),
    out_scale: f32,
    screens: Vec<Screen>,
}

impl Default for Gain {
    fn default() -> Self {
        Self::new()
    }
}

impl Gain {
    pub fn new() -> Self {
        Self {
            table: [1.0; BUCKETS],
            seen: [false; BUCKETS],
            raw: (0, 0),
            window_start: Instant::now(),
            cursor_start: None,
            speed: 0.0,
            last_send: None,
            remainder: (0.0, 0.0),
            out_scale: 1.0,
            screens: Vec::new(),
        }
    }

    pub fn set_screens(&mut self, screens: Vec<Screen>) {
        self.screens = screens;
    }

    fn against_wall(&self, (x, y): (i32, i32)) -> bool {
        if self.screens.is_empty() {
            return false;
        }
        let inside = |px: i32, py: i32| {
            self.screens
                .iter()
                .any(|&(sx, sy, sw, sh)| px >= sx && px < sx + sw && py >= sy && py < sy + sh)
        };
        let reach = WALL_MARGIN + 1;
        [(-reach, 0), (reach, 0), (0, -reach), (0, reach)]
            .iter()
            .any(|&(dx, dy)| !inside(x + dx, y + dy))
    }

    pub fn observe(&mut self, dx: i32, dy: i32, cursor: impl FnOnce() -> Option<(i32, i32)>) {
        self.observe_at(dx, dy, Instant::now(), cursor);
    }

    fn observe_at(
        &mut self,
        dx: i32,
        dy: i32,
        now: Instant,
        cursor: impl FnOnce() -> Option<(i32, i32)>,
    ) {
        let elapsed = now.saturating_duration_since(self.window_start);
        if elapsed > STALE || self.cursor_start.is_none() {
            self.restart(now, cursor());
            self.raw = (dx, dy);
            return;
        }
        self.raw.0 += dx;
        self.raw.1 += dy;
        if elapsed < WINDOW {
            return;
        }
        let end = cursor();
        if let (Some(start), Some(end)) = (self.cursor_start, end) {
            let raw = length(self.raw.0, self.raw.1);
            let moved = length(end.0 - start.0, end.1 - start.1);
            let free = !self.against_wall(start) && !self.against_wall(end);
            if raw >= 4.0 && moved > 0.0 && free {
                let millis = elapsed.as_secs_f32() * 1000.0;
                self.learn(bucket(raw * 10.0 / millis), moved / raw);
            }
        }
        self.restart(now, end);
    }

    fn restart(&mut self, now: Instant, cursor: Option<(i32, i32)>) {
        self.window_start = now;
        self.cursor_start = cursor;
        self.raw = (0, 0);
    }

    fn learn(&mut self, index: usize, ratio: f32) {
        let ratio = ratio.clamp(0.1, 20.0);
        self.table[index] = if self.seen[index] {
            self.table[index] * 0.8 + ratio * 0.2
        } else {
            ratio
        };
        self.seen[index] = true;
    }

    fn gain(&self, index: usize) -> f32 {
        (0..BUCKETS)
            .filter(|&i| self.seen[i])
            .min_by_key(|&i| i.abs_diff(index))
            .map_or_else(|| fallback(index), |i| self.table[i])
    }

    pub fn set_local_scale(&mut self, scale: f32) {
        self.out_scale = 1.0 / scale.clamp(0.5, 4.0);
    }

    pub fn describe(&self) -> String {
        let learned: Vec<String> = (0..BUCKETS)
            .filter(|&i| self.seen[i])
            .map(|i| format!("{:.1}", self.table[i]))
            .collect();
        if learned.is_empty() {
            "Fare hızı henüz öğrenilmedi; hareket olduğu gibi gönderiliyor.".to_owned()
        } else {
            format!("Fare hız oranı (yavaştan hızlıya): {}", learned.join(" · "))
        }
    }

    pub fn scale(&mut self, evs: &mut Vec<Ev>) {
        self.scale_at(evs, Instant::now());
    }

    fn scale_at(&mut self, evs: &mut Vec<Ev>, now: Instant) {
        let (dx, dy) = motion(evs);
        if dx == 0 && dy == 0 {
            return;
        }
        let millis = self
            .last_send
            .map(|t| now.saturating_duration_since(t))
            .filter(|d| *d < IDLE)
            .map(|d| (d.as_secs_f32() * 1000.0).max(1.0));
        self.last_send = Some(now);
        let instant = length(dx, dy) * 10.0 / millis.unwrap_or(8.0);
        self.speed = match millis {
            Some(_) => self.speed * 0.6 + instant * 0.4,
            None => {
                self.remainder = (0.0, 0.0);
                instant
            }
        };
        let gain = self.gain(bucket(self.speed)) * self.out_scale;
        let fx = dx as f32 * gain + self.remainder.0;
        let fy = dy as f32 * gain + self.remainder.1;
        let (sx, sy) = (fx.round(), fy.round());
        self.remainder = (fx - sx, fy - sy);
        replace_motion(evs, sx as i32, sy as i32);
    }
}

fn fallback(index: usize) -> f32 {
    const CURVE: [f32; BUCKETS] = [1.0, 1.0, 1.15, 1.4, 1.75, 2.1, 2.4, 2.6];
    CURVE[index]
}

pub struct Incoming {
    scale: f32,
    remainder: (f32, f32),
}

impl Incoming {
    pub fn new(scale: f32) -> Self {
        Self {
            scale: scale.clamp(0.5, 4.0),
            remainder: (0.0, 0.0),
        }
    }

    pub fn apply(&mut self, evs: &mut Vec<Ev>) {
        let (dx, dy) = motion(evs);
        if (dx == 0 && dy == 0) || self.scale == 1.0 {
            return;
        }
        let fx = dx as f32 * self.scale + self.remainder.0;
        let fy = dy as f32 * self.scale + self.remainder.1;
        let (sx, sy) = (fx.round(), fy.round());
        self.remainder = (fx - sx, fy - sy);
        replace_motion(evs, sx as i32, sy as i32);
    }
}

fn length(dx: i32, dy: i32) -> f32 {
    ((dx as f32).powi(2) + (dy as f32).powi(2)).sqrt()
}

fn bucket(counts_per_10ms: f32) -> usize {
    ((counts_per_10ms + 1.0).log2().floor().max(0.0) as usize).min(BUCKETS - 1)
}

pub fn motion(evs: &[Ev]) -> (i32, i32) {
    evs.iter()
        .filter(|e| e.0 == ev::EV_REL)
        .fold((0, 0), |(x, y), &(_, c, v)| match c {
            ev::REL_X => (x + v, y),
            ev::REL_Y => (x, y + v),
            _ => (x, y),
        })
}

fn replace_motion(evs: &mut Vec<Ev>, dx: i32, dy: i32) {
    let is_move = |e: &Ev| e.0 == ev::EV_REL && (e.1 == ev::REL_X || e.1 == ev::REL_Y);
    let at = evs.iter().position(is_move).unwrap_or(0);
    evs.retain(|e| !is_move(e));
    let at = at.min(evs.len());
    let mut moves = Vec::with_capacity(2);
    if dx != 0 {
        moves.push((ev::EV_REL, ev::REL_X, dx));
    }
    if dy != 0 {
        moves.push((ev::EV_REL, ev::REL_Y, dy));
    }
    evs.splice(at..at, moves);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moves(dx: i32, dy: i32) -> Vec<Ev> {
        vec![(ev::EV_REL, ev::REL_X, dx), (ev::EV_REL, ev::REL_Y, dy)]
    }

    fn teach(gain: &mut Gain, step: i32, ratio: i32, rounds: usize) -> Instant {
        let mut now = Instant::now();
        let mut cursor = (500, 500);
        gain.observe_at(0, 0, now, || Some(cursor));
        for _ in 0..rounds {
            for _ in 0..5 {
                now += Duration::from_millis(10);
                cursor.0 += step * ratio;
                gain.observe_at(step, 0, now, || Some(cursor));
            }
        }
        now
    }

    #[test]
    fn pushing_against_a_wall_does_not_slow_the_remote_pointer() {
        let mut gain = Gain::new();
        gain.set_screens(vec![(0, 0, 1920, 1080)]);
        let mut now = teach(&mut gain, 6, 3, 6);
        let learned = gain.gain(bucket(6.0));
        for _ in 0..8 {
            now += Duration::from_millis(400);
            let mut cursor = (1800, 500);
            gain.observe_at(0, 0, now, || Some(cursor));
            for _ in 0..12 {
                now += Duration::from_millis(10);
                cursor.0 = (cursor.0 + 18).min(1919);
                gain.observe_at(6, 0, now, || Some(cursor));
            }
        }
        assert!(
            gain.gain(bucket(6.0)) >= learned * 0.95,
            "{} < {learned}",
            gain.gain(bucket(6.0))
        );
    }

    #[test]
    fn only_outer_edges_count_as_walls() {
        let mut gain = Gain::new();
        gain.set_screens(vec![(0, 0, 1920, 1080), (1920, 0, 2560, 1440)]);
        assert!(!gain.against_wall((1919, 500)));
        assert!(!gain.against_wall((1920, 500)));
        assert!(gain.against_wall((0, 500)));
        assert!(gain.against_wall((1000, 1079)));
        assert!(gain.against_wall((4479, 700)));
        assert!(!gain.against_wall((960, 540)));
    }

    #[test]
    fn ogrenilmemis_yavas_hareket_degismez() {
        let mut gain = Gain::new();
        let mut evs = moves(1, -1);
        gain.scale(&mut evs);
        assert_eq!(motion(&evs), (1, -1));
    }

    #[test]
    fn olcek_farki_hizi_esitler() {
        let mut gain = Gain::new();
        gain.learn(0, 1.0);
        gain.set_local_scale(2.0);
        let mut evs = moves(10, 0);
        gain.scale_at(&mut evs, Instant::now());
        assert_eq!(motion(&evs), (5, 0));
        let mut inc = Incoming::new(1.5);
        let mut total = 0;
        for _ in 0..4 {
            let mut e = moves(1, 0);
            inc.apply(&mut e);
            total += motion(&e).0;
        }
        assert_eq!(total, 6);
    }

    #[test]
    fn yerel_imlec_hizi_karsiya_tasinir() {
        let mut gain = Gain::new();
        let now = teach(&mut gain, 6, 3, 6);
        let mut total = 0;
        for i in 1..=10 {
            let mut evs = moves(6, 0);
            gain.scale_at(&mut evs, now + Duration::from_millis(100 + 10 * i));
            total += motion(&evs).0;
        }
        assert!((170..=190).contains(&total), "toplam {total}");
    }

    #[test]
    fn kesirli_kazanc_kaybolmaz() {
        let mut gain = Gain::new();
        gain.learn(0, 0.5);
        let mut total = 0;
        let start = Instant::now();
        for i in 0..20 {
            let mut evs = vec![(ev::EV_REL, ev::REL_X, 1)];
            gain.scale_at(&mut evs, start + Duration::from_millis(30 * i));
            total += motion(&evs).0;
        }
        assert_eq!(total, 10);
    }

    #[test]
    fn tus_olaylari_korunur() {
        let mut gain = Gain::new();
        gain.learn(bucket(5.0), 2.0);
        let mut evs = vec![
            (ev::EV_KEY, ev::BTN_LEFT, 1),
            (ev::EV_REL, ev::REL_X, 5),
            (ev::EV_REL, ev::REL_WHEEL_HI_RES, 120),
        ];
        gain.scale(&mut evs);
        assert_eq!(evs[0], (ev::EV_KEY, ev::BTN_LEFT, 1));
        assert_eq!(evs[1], (ev::EV_REL, ev::REL_X, 10));
        assert_eq!(evs[2], (ev::EV_REL, ev::REL_WHEEL_HI_RES, 120));
    }
}
