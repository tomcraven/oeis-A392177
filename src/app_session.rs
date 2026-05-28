use bevy::camera::Projection;
use bevy::prelude::{Resource, *};
use serde::{Deserialize, Serialize};

use crate::bookmark_config::BookmarkStore;
use crate::calibration_config;
use crate::camera::BoardCamera;
use crate::camera_config::CameraSessionConfig;
use crate::game_snapshot::{SavedColor, SavedGameDefinition};
use crate::model::GameDefinition;
use crate::random_gen::{RandomGenConfig, RandomPiecesConfig};
use crate::ui::{BoardColourMode, SidebarSections, UiState};
use crate::viewport::ViewportState;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SavedUiState {
    pub random_gen: RandomGenConfig,
    #[serde(default)]
    pub random_pieces_config: RandomPiecesConfig,
    pub mutate_piece: usize,
    pub mutate_all: bool,
    pub preset_index: usize,
    pub edit_piece: usize,
    #[serde(default)]
    pub sync_attack_squares: bool,
    pub bookmark_new_name: String,
    pub add_piece_preset_index: usize,
    pub roster_remove_piece: usize,
    pub add_piece_color: SavedColor,
    #[serde(default)]
    pub bookmark_selected: Option<usize>,
    #[serde(default)]
    pub sidebar: SidebarSections,
    #[serde(default)]
    pub board_colour_mode: BoardColourMode,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AppSession {
    pub game: SavedGameDefinition,
    pub camera: CameraSessionConfig,
    pub target_index: u32,
    pub ui: SavedUiState,
}

#[derive(Resource, Default)]
pub struct AppSessionCache {
    pub last: Option<AppSession>,
}

pub fn capture_session(
    def: &GameDefinition,
    camera: CameraSessionConfig,
    target_index: u32,
    ui_state: &UiState,
    bookmark_selected: Option<usize>,
) -> AppSession {
    AppSession {
        game: SavedGameDefinition::from_game(def),
        camera,
        target_index,
        ui: SavedUiState {
            random_gen: ui_state.random_gen.clone(),
            random_pieces_config: ui_state.random_pieces_config.clone(),
            mutate_piece: ui_state.mutate_piece,
            mutate_all: ui_state.mutate_all,
            preset_index: ui_state.preset_index,
            edit_piece: ui_state.edit_piece,
            sync_attack_squares: ui_state.sync_attack_squares,
            bookmark_new_name: ui_state.bookmark_new_name.clone(),
            add_piece_preset_index: ui_state.add_piece_preset_index,
            roster_remove_piece: ui_state.roster_remove_piece,
            add_piece_color: SavedColor::from_bevy(ui_state.add_piece_color),
            board_colour_mode: ui_state.board_colour_mode,
            bookmark_selected,
            sidebar: ui_state.sidebar.clone(),
        },
    }
}

pub fn apply_session_to_ui(ui_state: &mut UiState, def: &GameDefinition, saved: &SavedUiState) {
    let n = def.pieces.len();
    ui_state.random_gen = saved.random_gen.clone();
    ui_state.random_pieces_config = saved.random_pieces_config.clone();
    ui_state.mutate_all = saved.mutate_all;
    ui_state.mutate_piece = saved.mutate_piece.min(n.saturating_sub(1));
    ui_state.preset_index = saved.preset_index;
    ui_state.edit_piece = saved.edit_piece.min(n.saturating_sub(1));
    ui_state.sync_attack_squares = saved.sync_attack_squares;
    ui_state.bookmark_new_name = saved.bookmark_new_name.clone();
    ui_state.add_piece_preset_index = saved.add_piece_preset_index;
    ui_state.roster_remove_piece = saved.roster_remove_piece.min(n.saturating_sub(1));
    ui_state.add_piece_color = saved.add_piece_color.to_bevy();
    ui_state.board_colour_mode = saved.board_colour_mode;
    ui_state.sidebar = saved.sidebar.clone();
    ui_state.draft = Some(def.clone());
}

#[cfg(not(target_family = "wasm"))]
use std::path::PathBuf;

#[cfg(not(target_family = "wasm"))]
pub fn config_file_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("red_black_knights").join("session.toml")
}

#[cfg(not(target_family = "wasm"))]
pub fn load_session() -> Option<AppSession> {
    if calibration_config::smoke_test_mode() || crate::perf_harness::perf_harness_mode() {
        return None;
    }
    let path = config_file_path();
    let text = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&text).ok()
}

#[cfg(not(target_family = "wasm"))]
pub fn save_session(session: &AppSession) -> std::io::Result<()> {
    if calibration_config::smoke_test_mode() || crate::perf_harness::perf_harness_mode() {
        return Ok(());
    }
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(session)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)
}

#[cfg(target_family = "wasm")]
const STORAGE_KEY: &str = "red_black_knights_session";

#[cfg(target_family = "wasm")]
pub fn load_session() -> Option<AppSession> {
    let text = read_local_storage()?;
    toml::from_str(&text).ok()
}

#[cfg(target_family = "wasm")]
pub fn save_session(session: &AppSession) -> std::io::Result<()> {
    let text = toml::to_string_pretty(session)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_local_storage(&text)
}

#[cfg(target_family = "wasm")]
fn read_local_storage() -> Option<String> {
    let window = web_sys::window()?;
    let storage = window.local_storage().ok()??;
    storage.get_item(STORAGE_KEY).ok()?
}

#[cfg(target_family = "wasm")]
fn write_local_storage(text: &str) -> std::io::Result<()> {
    let window = web_sys::window()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no window"))?;
    let storage = window
        .local_storage()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "no localStorage"))?
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "localStorage disabled")
        })?;
    storage
        .set_item(STORAGE_KEY, text)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "localStorage set failed"))
}

pub fn apply_saved_app_session(
    mut def: ResMut<GameDefinition>,
    mut ui_state: ResMut<UiState>,
    mut viewport: ResMut<ViewportState>,
    mut bookmarks: ResMut<BookmarkStore>,
    mut cache: ResMut<AppSessionCache>,
    mut query: Query<(&mut Transform, &mut Projection), With<BoardCamera>>,
) {
    let Some(session) = load_session() else {
        apply_legacy_camera_only(&mut query);
        return;
    };

    let restored: GameDefinition = session.game.clone().into();
    *def = restored.clone();
    apply_session_to_ui(&mut ui_state, &restored, &session.ui);
    if let Some(sel) = session.ui.bookmark_selected {
        bookmarks.selected = (sel < bookmarks.bookmarks.len()).then_some(sel);
    }
    viewport.target_index = session.target_index;
    viewport.render_dirty = true;

    apply_camera_to_query(&session.camera, &mut query);
    ui_state
        .sim_config_history
        .reset_to(crate::sim_config_history::SimConfigSnapshot {
            game: restored,
            camera: session.camera,
        });
    cache.last = Some(session);
}

fn apply_legacy_camera_only(
    query: &mut Query<(&mut Transform, &mut Projection), With<BoardCamera>>,
) {
    let Some(saved) = crate::camera_config::load() else {
        return;
    };
    apply_camera_to_query(&saved, query);
}

fn apply_camera_to_query(
    saved: &CameraSessionConfig,
    query: &mut Query<(&mut Transform, &mut Projection), With<BoardCamera>>,
) {
    let Ok((mut transform, mut projection)) = query.single_mut() else {
        return;
    };
    transform.translation.x = saved.x;
    transform.translation.y = saved.y;
    if let Projection::Orthographic(ref mut ortho) = *projection {
        let cap = if calibration_config::smoke_test_mode() {
            f32::MAX
        } else {
            calibration_config::MAX_ZOOM_OUT_BUDGET
        };
        ortho.scale = saved.zoom.clamp(calibration_config::MIN_ZOOM_OUT, cap);
    }
}

pub fn persist_app_session(
    def: Res<GameDefinition>,
    ui_state: Res<UiState>,
    viewport: Res<ViewportState>,
    bookmarks: Res<BookmarkStore>,
    camera_q: Query<(&Transform, &Projection), With<BoardCamera>>,
    mut cache: ResMut<AppSessionCache>,
) {
    let Ok((transform, projection)) = camera_q.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let camera = CameraSessionConfig {
        x: transform.translation.x,
        y: transform.translation.y,
        zoom: ortho.scale,
    };
    let session = capture_session(
        def.as_ref(),
        camera,
        viewport.target_index,
        ui_state.as_ref(),
        bookmarks.selected,
    );
    if cache.last.as_ref() == Some(&session) {
        return;
    }
    if save_session(&session).is_ok() {
        cache.last = Some(session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GameDefinition;

    #[test]
    fn app_session_round_trips_through_toml() {
        let def = GameDefinition::knight_3_clique();
        let mut ui = UiState::default();
        ui.sidebar.view = true;
        ui.sidebar.random_generation = true;
        ui.sidebar.pieces = true;
        ui.sidebar.pieces_summary = true;
        ui.sync_attack_squares = true;
        let session = capture_session(
            &def,
            CameraSessionConfig {
                x: 1.0,
                y: 2.0,
                zoom: 3.0,
            },
            120,
            &ui,
            Some(0),
        );
        let text = toml::to_string_pretty(&session).unwrap();
        let loaded: AppSession = toml::from_str(&text).unwrap();
        assert_eq!(loaded, session);
        assert!(loaded.ui.sidebar.view);
        assert!(loaded.ui.sidebar.random_generation);
        assert!(loaded.ui.sidebar.pieces);
        assert!(loaded.ui.sidebar.pieces_summary);
        assert!(loaded.ui.sync_attack_squares);
    }
}
