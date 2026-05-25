use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct CameraSessionConfig {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
}

pub fn config_file_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("red_black_knights").join("camera.toml")
}

pub fn load() -> Option<CameraSessionConfig> {
    if smoke_test_mode() {
        return None;
    }
    let path = config_file_path();
    let text = fs::read_to_string(&path).ok()?;
    toml::from_str(&text).ok()
}

pub fn save(config: &CameraSessionConfig) -> std::io::Result<()> {
    if smoke_test_mode() {
        return Ok(());
    }
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(path, text)
}

fn smoke_test_mode() -> bool {
    std::env::args().any(|a| a == "--smoke-test")
}
