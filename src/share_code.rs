//! Versioned base64 share codes for restoring the current view (rules, sim depth, camera, colouring).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::camera_config::CameraSessionConfig;
use crate::game_snapshot::SavedGameDefinition;
use crate::index_order::VisitOrder;
use crate::model::GameDefinition;
use crate::render::RenderCache;
use crate::sim_worker::SimulationBridge;
use crate::ui::{BoardColourMode, UiState};
use crate::viewport::ViewportState;

/// Prefix for pasted codes (`rbk:<version>:<payload>`).
pub const SHARE_CODE_PREFIX: &str = "rbk:";

/// Newest format version [`encode_share_code`] writes.
pub const CURRENT_SHARE_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ShareViewSnapshot {
    pub version: u32,
    pub game: SavedGameDefinition,
    pub camera: CameraSessionConfig,
    pub target_index: u32,
    pub board_colour_mode: BoardColourMode,
    #[serde(default)]
    pub visit_order: VisitOrder,
}

#[derive(Clone, Copy, Debug)]
pub struct ShareCapture<'a> {
    pub def: &'a GameDefinition,
    pub camera: CameraSessionConfig,
    pub target_index: u32,
    pub board_colour_mode: BoardColourMode,
    pub visit_order: VisitOrder,
}

pub fn capture_share_view(input: ShareCapture<'_>) -> ShareViewSnapshot {
    ShareViewSnapshot {
        version: CURRENT_SHARE_VERSION,
        game: SavedGameDefinition::from_game(input.def),
        camera: input.camera,
        target_index: input.target_index,
        board_colour_mode: input.board_colour_mode,
        visit_order: input.visit_order,
    }
}

pub fn encode_share_code(snapshot: &ShareViewSnapshot) -> Result<String, String> {
    if snapshot.version != CURRENT_SHARE_VERSION {
        return Err(format!(
            "internal share version {} does not match encoder {}",
            snapshot.version, CURRENT_SHARE_VERSION
        ));
    }
    let json = serde_json::to_vec(snapshot).map_err(|e| e.to_string())?;
    let payload = STANDARD.encode(json);
    Ok(format!(
        "{SHARE_CODE_PREFIX}{CURRENT_SHARE_VERSION}:{payload}"
    ))
}

pub fn decode_share_code(code: &str) -> Result<ShareViewSnapshot, String> {
    let code = code.trim();
    let rest = code
        .strip_prefix(SHARE_CODE_PREFIX)
        .ok_or_else(|| format!("share code must start with {SHARE_CODE_PREFIX}"))?;
    let (version_str, payload) = rest
        .split_once(':')
        .ok_or_else(|| "share code missing version or payload".to_string())?;
    let version: u32 = version_str
        .parse()
        .map_err(|_| format!("invalid share version {version_str:?}"))?;
    let bytes = STANDARD
        .decode(payload.trim())
        .map_err(|e| format!("invalid base64: {e}"))?;
    decode_share_payload(&bytes, version)
}

fn decode_share_payload(bytes: &[u8], version: u32) -> Result<ShareViewSnapshot, String> {
    match version {
        1 => {
            let snap: ShareViewSnapshot =
                serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            if snap.version != 1 {
                return Err(format!(
                    "share payload version {} does not match envelope version 1",
                    snap.version
                ));
            }
            Ok(snap)
        }
        v if v > CURRENT_SHARE_VERSION => Err(format!(
            "share code version {v} is newer than this app (supports up to {CURRENT_SHARE_VERSION}); update the game"
        )),
        v => Err(format!("unsupported share code version {v}")),
    }
}

/// Restore simulation, camera, and colouring from a decoded snapshot.
pub fn apply_share_snapshot(
    snapshot: &ShareViewSnapshot,
    def: &mut GameDefinition,
    sim: &mut SimulationBridge,
    ui_state: &mut UiState,
    viewport: &mut ViewportState,
    cache: &mut RenderCache,
    camera_q: &mut Query<
        (&mut Transform, &mut Projection, &crate::camera::PanCamera),
        With<crate::camera::BoardCamera>,
    >,
    preset_index_for: impl FnOnce(&GameDefinition) -> Option<usize>,
) {
    let restored: GameDefinition = snapshot.game.clone().into();
    *def = restored.clone();
    ui_state.draft = Some(restored.clone());
    ui_state.board_colour_mode = snapshot.board_colour_mode;
    if let Some(idx) = preset_index_for(&restored) {
        ui_state.preset_index = idx;
    }
    ui_state.export_status = None;

    viewport.target_index = snapshot.target_index;
    viewport.bounds = None;
    viewport.render_dirty = true;
    cache.rendered_bounds = None;

    sim.request_reset(def.clone(), snapshot.visit_order);
    apply_camera_to_query(&snapshot.camera, camera_q);
    ui_state
        .sim_config_history
        .commit(restored, snapshot.camera);
}

fn apply_camera_to_query(
    saved: &CameraSessionConfig,
    query: &mut Query<
        (&mut Transform, &mut Projection, &crate::camera::PanCamera),
        With<crate::camera::BoardCamera>,
    >,
) {
    let Ok((mut transform, mut projection, pan)) = query.single_mut() else {
        return;
    };
    transform.translation.x = saved.x;
    transform.translation.y = saved.y;
    if let Projection::Orthographic(ref mut ortho) = *projection {
        let cap = crate::calibration_config::MAX_ZOOM_OUT_BUDGET.min(pan.max_scale);
        ortho.scale = saved
            .zoom
            .clamp(crate::calibration_config::MIN_ZOOM_OUT, cap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index_order::VisitOrder;
    use crate::model::GameDefinition;

    #[test]
    fn share_code_round_trips_v1() {
        let def = GameDefinition::knight_2_pairwise();
        let snap = capture_share_view(ShareCapture {
            def: &def,
            camera: CameraSessionConfig {
                x: 1.0,
                y: 2.0,
                zoom: 3.0,
            },
            target_index: 99_999,
            board_colour_mode: BoardColourMode::Piece,
            visit_order: VisitOrder::MortonZOrder,
        });
        let code = encode_share_code(&snap).unwrap();
        assert!(code.starts_with("rbk:1:"));
        let loaded = decode_share_code(&code).unwrap();
        assert_eq!(loaded, snap);
    }

    #[test]
    fn share_code_v1_without_visit_order_defaults_to_spiral() {
        let def = GameDefinition::knight_2_pairwise();
        let snap = capture_share_view(ShareCapture {
            def: &def,
            camera: CameraSessionConfig {
                x: 0.0,
                y: 0.0,
                zoom: 1.0,
            },
            target_index: 0,
            board_colour_mode: BoardColourMode::default(),
            visit_order: VisitOrder::MortonZOrder,
        });
        let mut value = serde_json::to_value(&snap).unwrap();
        value.as_object_mut().unwrap().remove("visit_order");
        let bytes = serde_json::to_vec(&value).unwrap();
        let loaded = decode_share_payload(&bytes, 1).unwrap();
        assert_eq!(loaded.visit_order, VisitOrder::default());
        assert_eq!(loaded.target_index, snap.target_index);
    }

    #[test]
    fn rejects_unknown_future_version() {
        let err = decode_share_code("rbk:99:AAAA").unwrap_err();
        assert!(err.contains("newer than this app"));
    }

    /// Share codes in `test-data/screenshot_sharecodes.json` (JSON string array).
    const SCREENSHOT_SHARE_CODES_JSON: &str = include_str!("test-data/screenshot_sharecodes.json");

    fn screenshot_share_codes() -> Vec<String> {
        serde_json::from_str(SCREENSHOT_SHARE_CODES_JSON)
            .expect("valid test-data/screenshot_sharecodes.json")
    }

    fn screenshot_golden_png_path(index: usize) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/test-data")
            .join(format!("screenshot_sharecodes_{index}.png"))
    }

    fn rgba8(color: bevy::prelude::Color) -> [u8; 4] {
        let c = color.to_srgba();
        [
            (c.red.clamp(0.0, 1.0) * 255.0).round() as u8,
            (c.green.clamp(0.0, 1.0) * 255.0).round() as u8,
            (c.blue.clamp(0.0, 1.0) * 255.0).round() as u8,
            (c.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
        ]
    }

    fn share_code_screenshot_rgba(code: &str) -> (u32, u32, Vec<u8>) {
        use bevy::prelude::Color;
        use crate::render::{grid_texture_size, raster_spiral_grid};
        use crate::sim::Simulation;

        let snap = decode_share_code(code.trim()).unwrap();
        let def: GameDefinition = snap.game.into();
        let mut sim = Simulation::new(&def, snap.visit_order);
        sim.advance_to_target(&def, snap.target_index);

        let bounds = crate::viewport::grid_bounds_for_share_screenshot(
            snap.camera,
            &sim.placements,
            snap.visit_order,
        );
        let size = grid_texture_size(bounds);
        let empty = rgba8(Color::srgba(0.12, 0.12, 0.16, 1.0));
        let colors: Vec<[u8; 4]> = def.pieces.iter().map(|p| rgba8(p.color)).collect();
        let raster = raster_spiral_grid(
            bounds,
            size.x,
            size.y,
            &sim.occupancy,
            &colors,
            empty,
            snap.board_colour_mode,
        );
        (size.x, size.y, raster)
    }

    fn encode_rgba_png(data: &[u8], width: u32, height: u32) -> Vec<u8> {
        use png::{BitDepth, ColorType, Encoder};
        use std::io::Cursor;

        let mut buf = Vec::new();
        let mut encoder = Encoder::new(Cursor::new(&mut buf), width, height);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(data).expect("png pixels");
        writer.finish().expect("png finish");
        buf
    }

    fn share_code_screenshot_png(code: &str) -> Vec<u8> {
        let (width, height, rgba) = share_code_screenshot_rgba(code);
        encode_rgba_png(&rgba, width, height)
    }

    #[test]
    fn legacy_v1_share_codes_decode() {
        let codes = screenshot_share_codes();
        assert!(
            !codes.is_empty(),
            "expected share codes in test-data/screenshot_sharecodes.json"
        );
        for code in &codes {
            let snap = decode_share_code(code).expect("fixture share code must decode");
            assert_eq!(snap.version, 1);
            assert_eq!(snap.visit_order, VisitOrder::default());
            assert_eq!(snap.board_colour_mode, BoardColourMode::Piece);
            assert!(!snap.game.pieces.is_empty());
        }
    }

    #[test]
    #[ignore = "writes src/test-data/screenshot_sharecodes_N.png — run with --ignored"]
    fn write_screenshot_share_code_golden_pngs() {
        for (index, code) in screenshot_share_codes().into_iter().enumerate() {
            let png = share_code_screenshot_png(&code);
            let path = screenshot_golden_png_path(index);
            std::fs::write(&path, png).unwrap_or_else(|e| {
                panic!("failed to write {}: {e}", path.display());
            });
            eprintln!("wrote {}", path.display());
        }
    }

    #[test]
    fn screenshot_share_codes_match_golden_pngs() {
        for (index, code) in screenshot_share_codes().into_iter().enumerate() {
            let path = screenshot_golden_png_path(index);
            let expected = std::fs::read(&path).unwrap_or_else(|e| {
                panic!(
                    "missing golden {} (run write_screenshot_share_code_golden_pngs with --ignored): {e}",
                    path.display()
                );
            });
            let actual = share_code_screenshot_png(&code);
            assert_eq!(
                actual, expected,
                "PNG mismatch for screenshot_sharecodes.json entry {index} — inspect {} before updating golden",
                path.display()
            );
        }
    }
}
