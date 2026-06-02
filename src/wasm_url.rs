//! URL query-param share codes for WASM builds.
//!
//! On load, a `?s=rbk:…` query param (if present) overrides the locally persisted session so a
//! pasted link reproduces exactly what the author saw. While running, the param is kept in sync
//! with the live view via `history.replaceState`, so copying the address bar always yields an
//! up-to-date share code.

use bevy::prelude::*;

use crate::bookmark_config::BookmarkStore;
use crate::camera::{BoardCamera, PanCamera};
use crate::camera_config::CameraSessionConfig;
use crate::model::GameDefinition;
use crate::render::RenderCache;
use crate::share_code::{self, ShareCapture, ShareViewSnapshot};
use crate::sim_worker::SimulationBridge;
use crate::ui::{self, UiState};
use crate::viewport::ViewportState;

/// Query-param key that carries the `rbk:` share code.
const QUERY_PARAM: &str = "s";

/// Remembers the last snapshot mirrored to the URL so we only call `replaceState` on real changes.
#[derive(Resource, Default)]
pub struct ShareUrlCache {
    pub last: Option<ShareViewSnapshot>,
}

/// Reads the `?s=` share code from the current document URL, if any.
fn read_share_code_from_url() -> Option<String> {
    let window = web_sys::window()?;
    let href = window.location().href().ok()?;
    let url = web_sys::Url::new(&href).ok()?;
    url.search_params()
        .get(QUERY_PARAM)
        .filter(|code| !code.trim().is_empty())
}

/// Writes `code` into the `?s=` query param using `replaceState` (no new history entry).
fn write_share_code_to_url(code: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(href) = window.location().href() else {
        return;
    };
    let Ok(url) = web_sys::Url::new(&href) else {
        return;
    };
    url.search_params().set(QUERY_PARAM, code);
    let new_href = url.href();
    if let Ok(history) = window.history() {
        let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&new_href));
    }
}

/// Startup system: if the URL carries a share code, apply it over any restored local session.
pub fn apply_url_share_code(
    mut def: ResMut<GameDefinition>,
    mut sim: ResMut<SimulationBridge>,
    mut ui_state: ResMut<UiState>,
    mut viewport: ResMut<ViewportState>,
    mut cache: ResMut<RenderCache>,
    mut bookmarks: ResMut<BookmarkStore>,
    mut url_cache: ResMut<ShareUrlCache>,
    mut camera_q: Query<(&mut Transform, &mut Projection, &PanCamera), With<BoardCamera>>,
) {
    let Some(code) = read_share_code_from_url() else {
        return;
    };
    let Ok(snapshot) = share_code::decode_share_code(&code) else {
        return;
    };
    share_code::apply_share_snapshot(
        &snapshot,
        &mut def,
        &mut sim,
        &mut ui_state,
        &mut viewport,
        &mut cache,
        &mut camera_q,
        ui::preset_index_for_def,
    );
    bookmarks.selected = None;
    url_cache.last = Some(snapshot);
}

/// PostUpdate system: mirror the live view into the `?s=` query param whenever it changes.
pub fn sync_share_code_to_url(
    def: Res<GameDefinition>,
    ui_state: Res<UiState>,
    sim: Res<SimulationBridge>,
    viewport: Res<ViewportState>,
    camera_q: Query<(&Transform, &Projection), With<BoardCamera>>,
    mut url_cache: ResMut<ShareUrlCache>,
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
    let snapshot = share_code::capture_share_view(ShareCapture {
        def: def.as_ref(),
        camera,
        target_index: viewport.target_index,
        board_colour_mode: ui_state.board_colour_mode,
        visit_order: sim.visit_order(),
    });
    if url_cache.last.as_ref() == Some(&snapshot) {
        return;
    }
    if let Ok(code) = share_code::encode_share_code(&snapshot) {
        write_share_code_to_url(&code);
        url_cache.last = Some(snapshot);
    }
}
