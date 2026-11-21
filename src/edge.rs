use crate::config::Side;
use std::time::{Duration, Instant};

pub const PUSH_NEEDED: i32 = 180;
pub const HOLD_FLAG: u8 = 0x80;

pub fn enter_flags(edge: Side, hold: bool) -> u8 {
    edge.to_u8() | if hold { HOLD_FLAG } else { 0 }
}

pub fn wants_hold(flags: u8) -> bool {
    flags & HOLD_FLAG != 0
}
pub const PUSH_GAP: Duration = Duration::from_millis(320);

pub fn outward(edge: Side, horizontal: bool, delta: i32) -> i32 {
    let toward = match (edge, horizontal) {
        (Side::Right, true) | (Side::Bottom, false) => delta,
        (Side::Left, true) | (Side::Top, false) => -delta,
        _ => 0,
    };
    toward.max(0)
}

#[derive(Default)]
pub struct EdgePush {
    pushed: i32,
    last: Option<Instant>,
}

impl EdgePush {
    pub fn push(&mut self, amount: i32, now: Instant) -> f32 {
        if self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) > PUSH_GAP)
        {
            self.pushed = 0;
        }
        self.pushed = self.pushed.saturating_add(amount.max(0));
        self.last = Some(now);
        self.progress()
    }

    pub fn progress(&self) -> f32 {
        (self.pushed as f32 / PUSH_NEEDED as f32).clamp(0.0, 1.0)
    }

    pub fn done(&self) -> bool {
        self.pushed >= PUSH_NEEDED
    }

    pub fn active(&self) -> bool {
        self.pushed > 0
    }

    pub fn reset(&mut self) {
        self.pushed = 0;
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_movement_toward_the_edge_counts() {
        assert_eq!(outward(Side::Right, true, 12), 12);
        assert_eq!(outward(Side::Right, true, -12), 0);
        assert_eq!(outward(Side::Left, true, -7), 7);
        assert_eq!(outward(Side::Top, false, -5), 5);
        assert_eq!(outward(Side::Bottom, false, 9), 9);
        assert_eq!(outward(Side::Right, false, 30), 0);
    }

    #[test]
    fn hold_choice_travels_with_the_enter_message() {
        for edge in Side::ALL {
            let flags = enter_flags(edge, true);
            assert!(wants_hold(flags));
            assert!(Side::from_u8(flags & !HOLD_FLAG) == edge);
            assert!(!wants_hold(enter_flags(edge, false)));
        }
    }

    #[test]
    fn steady_pushing_fills_the_edge() {
        let start = Instant::now();
        let mut push = EdgePush::default();
        let mut now = start;
        while !push.done() {
            now += Duration::from_millis(16);
            push.push(20, now);
        }
        assert!(now - start < Duration::from_millis(200));
        assert_eq!(push.progress(), 1.0);
    }

    #[test]
    fn pausing_at_the_edge_starts_over() {
        let start = Instant::now();
        let mut push = EdgePush::default();
        push.push(120, start);
        assert!(push.progress() > 0.5);
        push.push(10, start + PUSH_GAP + Duration::from_millis(1));
        assert!(push.progress() < 0.1);
    }
}
