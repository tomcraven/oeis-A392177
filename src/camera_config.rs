use serde::{Deserialize, Serialize};

#[cfg(not(target_family = "wasm"))]
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct CameraSessionConfig {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
}

#[cfg(not(target_family = "wasm"))]
pub fn config_file_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("red_black_knights").join("camera.toml")
}

#[cfg(not(target_family = "wasm"))]
pub fn load() -> Option<CameraSessionConfig> {
    if smoke_test_mode() {
        return None;
    }
    let path = config_file_path();
    let text = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&text).ok()
}

#[cfg(not(target_family = "wasm"))]
pub fn save(config: &CameraSessionConfig) -> std::io::Result<()> {
    if smoke_test_mode() {
        return Ok(());
    }
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)
}

#[cfg(target_family = "wasm")]
pub fn load() -> Option<CameraSessionConfig> {
    None
}

#[cfg(target_family = "wasm")]
pub fn save(_config: &CameraSessionConfig) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
fn smoke_test_mode() -> bool {
    std::env::args().any(|a| a == "--smoke-test")
}
