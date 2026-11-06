use std::path::PathBuf;
use std::sync::Arc;

pub const AVAILABLE: bool = false;

pub fn current_files() -> Option<Vec<PathBuf>> {
    None
}

pub fn set_files(_: &[PathBuf]) {}

pub fn text_snapshot() -> Option<String> {
    None
}

pub fn set_text(_: &str) {}

pub fn carry(_: Arc<crate::core::Shared>) {}
