//! Versioned base64 share codes for restoring the current view (rules, sim depth, camera, colouring).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::camera_config::CameraSessionConfig;
use crate::game_snapshot::SavedGameDefinition;
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
}

#[derive(Clone, Copy, Debug)]
pub struct ShareCapture<'a> {
    pub def: &'a GameDefinition,
    pub camera: CameraSessionConfig,
    pub target_index: u32,
    pub board_colour_mode: BoardColourMode,
}

pub fn capture_share_view(input: ShareCapture<'_>) -> ShareViewSnapshot {
    ShareViewSnapshot {
        version: CURRENT_SHARE_VERSION,
        game: SavedGameDefinition::from_game(input.def),
        camera: input.camera,
        target_index: input.target_index,
        board_colour_mode: input.board_colour_mode,
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
    Ok(format!("{SHARE_CODE_PREFIX}{CURRENT_SHARE_VERSION}:{payload}"))
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
            let snap: ShareViewSnapshot = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
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

    sim.request_reset(def.clone(), ui_state.visit_order);
    ui_state.visit_order_applied = ui_state.visit_order;
    apply_camera_to_query(&snapshot.camera, camera_q);
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
        });
        let code = encode_share_code(&snap).unwrap();
        assert!(code.starts_with("rbk:1:"));
        let loaded = decode_share_code(&code).unwrap();
        assert_eq!(loaded, snap);
    }

    #[test]
    fn rejects_unknown_future_version() {
        let err = decode_share_code("rbk:99:AAAA").unwrap_err();
        assert!(err.contains("newer than this app"));
    }
}
