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

pub fn set_text(_: &str) -> bool {
    false
}

pub fn image_snapshot() -> Option<Vec<u8>> {
    None
}

pub fn set_image(_: Vec<u8>, _: image::RgbaImage) -> bool {
    false
}

pub fn carry(_: Arc<crate::core::Shared>) {}
