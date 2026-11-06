use std::sync::{Arc, Mutex};
use std::time::Instant;

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

pub struct Handle;

pub const HAS_VOLUME: bool = false;

pub fn available() -> bool {
    false
}
pub fn list(_: Kind) -> Vec<Dev> {
    vec![]
}
pub fn default(_: Kind) -> Option<String> {
    None
}
pub fn set_default(_: Kind, _: &str) {}
pub fn volume(_: Kind) -> Option<(u32, bool)> {
    None
}
pub fn set_volume(_: Kind, _: u32) {}
pub fn set_mute(_: Kind, _: bool) {}
pub fn start_sender(_: u8, _: &str, _: &str, _: [u8; 32]) -> Option<Handle> {
    None
}
pub fn start_receiver(
    _: u8,
    _: &str,
    _: u32,
    _: Arc<Mutex<Option<Instant>>>,
    _: [u8; 32],
) -> Option<Handle> {
    None
}
