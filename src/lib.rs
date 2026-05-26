pub mod app_session;
pub mod board_export;
pub mod bookmark_config;
pub mod calibration_config;
pub mod camera;
pub mod camera_config;
pub mod discover;
pub mod discover_catalog;
pub mod game_snapshot;
#[cfg(feature = "place_profile")]
pub mod place_profile;
pub mod model;
pub mod mutate;
pub mod random_gen;
pub mod render;
pub mod share_code;
pub mod sim;
pub mod sim_worker;
pub mod spiral;
pub mod ui;
pub mod viewport;
#[cfg(target_family = "wasm")]
pub mod wasm_clipboard;

pub const CELL_SIZE: f32 = 16.0;
