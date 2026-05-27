use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use serde::{Deserialize, Serialize};

use crate::board_export;
use crate::bookmark_config::{Bookmark, BookmarkStore};
use crate::share_code::{self, ShareCapture};
use crate::camera::{BoardCamera, PanCamera, PendingCameraAction};
use crate::camera_config::CameraSessionConfig;
use crate::calibration_config;
#[cfg(not(target_family = "wasm"))]
use crate::CELL_SIZE;
use crate::model::{Piece, GameDefinition, PieceDef};
use crate::mutate::{
    reflect_across_x_axis, reflect_across_y_axis, rotate_ccw, rotate_cw,
    shared_attack_extent_for_pieces, shift_attacks, toggle_random_attack_square,
    toggle_random_blocked_by,
};
use crate::random_gen::{
    AttackSymmetry, RandomGenConfig, RandomPieceSlot, RandomPiecesConfig,
    generate_random_game, generate_random_pieces_game,
};
use crate::render::RenderCache;
#[cfg(not(target_family = "wasm"))]
use crate::render::grid_texture_size;
use crate::sim_worker::SimulationBridge;
use crate::viewport::{self, ViewportState};

/// How occupied spiral cells are coloured on the board texture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum BoardColourMode {
    #[default]
    Piece,
}

impl<'de> Deserialize<'de> for BoardColourMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let _ = String::deserialize(deserializer)?;
        Ok(Self::Piece)
    }
}

/// Open/closed state for sidebar [`egui::CollapsingHeader`] sections (persisted in app session).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SidebarSections {
    #[serde(default)]
    pub view: bool,
    #[serde(default)]
    pub colouring: bool,
    /// Presets, custom bookmarks, and share controls.
    #[serde(default, alias = "bookmarks", alias = "share", alias = "presets")]
    pub library: bool,
    #[serde(default, alias = "random_generator")]
    pub random_attacks: bool,
    #[serde(default)]
    pub random_pieces: bool,
    #[serde(default)]
    pub mutate: bool,
    #[serde(default)]
    pub pieces: bool,
    #[serde(default)]
    pub pieces_summary: bool,
    #[serde(default)]
    pub edit_roster: bool,
    #[serde(default)]
    pub pieces_advanced: bool,
    #[serde(default)]
    pub debug: bool,
}

#[derive(Resource)]
pub struct UiState {
    pub draft: Option<GameDefinition>,
    pub board_colour_mode: BoardColourMode,
    pub random_gen: RandomGenConfig,
    pub random_pieces_config: RandomPiecesConfig,
    /// Piece index targeted by the Mutate section (when `mutate_all` is false).
    pub mutate_piece: usize,
    pub mutate_all: bool,
    pub preset_index: usize,
    /// Piece shown in the nested Advanced editor under Pieces.
    pub edit_piece: usize,
    /// When true, attack-square grid toggles apply to every piece in the set.
    pub sync_attack_squares: bool,
    /// Name for the next custom bookmark (Library section).
    pub bookmark_new_name: String,
    /// Selected entry in [`PieceDef::piece_catalog`] for roster add.
    pub add_piece_preset_index: usize,
    /// Piece index targeted by roster remove control.
    pub roster_remove_piece: usize,
    /// Colour applied when adding a piece from Edit roster.
    pub add_piece_color: Color,
    pub sidebar: SidebarSections,
    /// Short-lived status after exporting the board (sidebar View section).
    pub export_status: Option<String>,
    /// Buffer for the import share-code dialog.
    pub share_code_input: String,
    /// Import dialog submitted (WASM DOM path); decode/apply after the sidebar pass.
    pub share_code_import_pending: bool,
    /// Native: egui import dialog visible.
    pub share_code_import_dialog_open: bool,
    /// egui time (`Context::input`) when copy succeeded; drives brief “Copied!” button label.
    pub share_code_copied_at: Option<f64>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            draft: None,
            board_colour_mode: BoardColourMode::default(),
            random_gen: RandomGenConfig::default(),
            random_pieces_config: RandomPiecesConfig::default(),
            mutate_piece: 0,
            mutate_all: false,
            preset_index: 0,
            edit_piece: 0,
            sync_attack_squares: false,
            bookmark_new_name: String::new(),
            add_piece_preset_index: 0,
            roster_remove_piece: 0,
            add_piece_color: GameDefinition::default_piece_color(0),
            sidebar: SidebarSections::default(),
            export_status: None,
            share_code_input: String::new(),
            share_code_import_pending: false,
            share_code_import_dialog_open: false,
            share_code_copied_at: None,
        }
    }
}

fn sidebar_collapsing(
    ui: &mut egui::Ui,
    section_id: &str,
    title: &str,
    open: bool,
    force_open: bool,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    let want_open = force_open || open;
    let response = egui::CollapsingHeader::new(title)
        .id_salt(egui::Id::new(section_id))
        .default_open(want_open)
        .open(if force_open { Some(true) } else { None })
        .show(ui, body);
    force_open || response.body_returned.is_some()
}

fn apply_preset_index(index: usize) -> GameDefinition {
    let catalog = GameDefinition::preset_catalog();
    let i = index % catalog.len();
    (catalog[i].1)()
}

fn preset_index_for_def(def: &GameDefinition) -> Option<usize> {
    GameDefinition::preset_catalog()
        .iter()
        .enumerate()
        .find(|(_, (_, factory))| factory().same_sim_state(def))
        .map(|(i, _)| i)
}

fn import_share_code_from_text(
    code: &str,
    def: &mut GameDefinition,
    sim: &mut SimulationBridge,
    ui_state: &mut UiState,
    viewport: &mut ViewportState,
    cache: &mut RenderCache,
    camera_q: &mut Query<(&mut Transform, &mut Projection, &PanCamera), With<BoardCamera>>,
    draft: &mut GameDefinition,
    bookmarks: &mut BookmarkStore,
) {
    match share_code::decode_share_code(code.trim()) {
        Ok(snapshot) => {
            share_code::apply_share_snapshot(
                &snapshot,
                def,
                sim,
                ui_state,
                viewport,
                cache,
                camera_q,
                preset_index_for_def,
            );
            *draft = def.clone();
            bookmarks.selected = None;
        }
        Err(_) => {}
    }
}

#[cfg(not(target_family = "wasm"))]
fn share_code_import_egui_dialog(
    ctx: &egui::Context,
    ui_state: &mut UiState,
    def: &mut GameDefinition,
    sim: &mut SimulationBridge,
    viewport: &mut ViewportState,
    cache: &mut RenderCache,
    camera_q: &mut Query<(&mut Transform, &mut Projection, &PanCamera), With<BoardCamera>>,
    draft: &mut GameDefinition,
    bookmarks: &mut BookmarkStore,
) {
    if !ui_state.share_code_import_dialog_open {
        return;
    }

    let mut close_dialog = false;
    let mut import_clicked = false;
    let window = egui::Window::new("Import share code")
        .id(egui::Id::new("share_code_import_dialog"))
        .collapsible(false)
        .resizable(true)
        .default_width(480.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut ui_state.share_code_input)
                    .desired_rows(5)
                    .hint_text("rbk:…")
                    .font(egui::TextStyle::Monospace),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close_dialog = true;
                }
                if ui.button("Import").clicked() {
                    import_clicked = true;
                }
            });
        });

    if window.is_none() || close_dialog {
        ui_state.share_code_import_dialog_open = false;
    } else if import_clicked {
        ui_state.share_code_import_dialog_open = false;
        let code = ui_state.share_code_input.clone();
        import_share_code_from_text(
            &code,
            def,
            sim,
            ui_state,
            viewport,
            cache,
            camera_q,
            draft,
            bookmarks,
        );
    }
}

fn read_camera_session(
    camera_q: &Query<(&mut Transform, &mut Projection, &PanCamera), With<BoardCamera>>,
    window_q: &Query<&Window>,
    viewport: &ViewportState,
) -> Option<CameraSessionConfig> {
    let Ok((transform, projection, pan)) = camera_q.single() else {
        return None;
    };
    let Projection::Orthographic(ortho) = projection else {
        return None;
    };
    let effective_zoom_max = if let Ok(window) = window_q.single() {
        let safe = viewport::max_safe_zoom_out_scale(ortho, window, viewport.left_inset_px);
        calibration_config::effective_zoom_out_max(safe, pan.max_scale)
    } else {
        calibration_config::MAX_ZOOM_OUT_BUDGET.min(pan.max_scale)
    };
    Some(CameraSessionConfig {
        x: transform.translation.x,
        y: transform.translation.y,
        zoom: ortho.scale.clamp(pan.min_scale, effective_zoom_max),
    })
}

fn apply_bookmark_camera(
    camera: CameraSessionConfig,
    camera_q: &mut Query<(&mut Transform, &mut Projection, &PanCamera), With<BoardCamera>>,
) {
    let Ok((mut transform, mut projection, pan)) = camera_q.single_mut() else {
        return;
    };
    transform.translation.x = camera.x;
    transform.translation.y = camera.y;
    if let Projection::Orthographic(ref mut ortho) = *projection {
        let cap = calibration_config::MAX_ZOOM_OUT_BUDGET.min(pan.max_scale);
        ortho.scale = camera.zoom.clamp(pan.min_scale, cap);
    }
}

fn apply_bookmark(
    bookmark: &Bookmark,
    draft: &mut GameDefinition,
    ui_state: &mut UiState,
    viewport: &mut ViewportState,
    camera_q: &mut Query<(&mut Transform, &mut Projection, &PanCamera), With<BoardCamera>>,
) {
    *draft = bookmark.to_game_definition();
    ui_state.preset_index = preset_index_for_def(draft).unwrap_or(ui_state.preset_index);
    viewport.target_index = bookmark.target_index;
    apply_bookmark_camera(bookmark.camera, camera_q);
}

fn library_entry_count(custom_bookmarks: usize) -> usize {
    GameDefinition::preset_catalog().len() + custom_bookmarks
}

fn library_selected_index(bookmarks: &BookmarkStore, preset_index: usize) -> usize {
    let preset_count = GameDefinition::preset_catalog().len();
    if let Some(j) = bookmarks.selected {
        if j < bookmarks.bookmarks.len() {
            return preset_count + j;
        }
    }
    preset_index.min(preset_count.saturating_sub(1))
}

fn library_selected_label(bookmarks: &BookmarkStore, preset_index: usize) -> &str {
    let catalog = GameDefinition::preset_catalog();
    if let Some(j) = bookmarks.selected {
        if let Some(bm) = bookmarks.bookmarks.get(j) {
            return bm.name.as_str();
        }
    }
    let idx = preset_index.min(catalog.len().saturating_sub(1));
    catalog.get(idx).map(|(label, _)| *label).unwrap_or("—")
}

fn apply_library_index(
    index: usize,
    draft: &mut GameDefinition,
    ui_state: &mut UiState,
    bookmarks: &mut BookmarkStore,
    viewport: &mut ViewportState,
    camera_q: &mut Query<(&mut Transform, &mut Projection, &PanCamera), With<BoardCamera>>,
) {
    let preset_count = GameDefinition::preset_catalog().len();
    let total = library_entry_count(bookmarks.bookmarks.len());
    if total == 0 {
        return;
    }
    let index = index % total;
    if index < preset_count {
        bookmarks.selected = None;
        ui_state.preset_index = index;
        *draft = apply_preset_index(index);
    } else {
        let j = index - preset_count;
        if j < bookmarks.bookmarks.len() {
            bookmarks.selected = Some(j);
            let bm = bookmarks.bookmarks[j].clone();
            apply_bookmark(&bm, draft, ui_state, viewport, camera_q);
        }
    }
}

pub fn ui_game_definition(
    mut contexts: EguiContexts,
    mut def: ResMut<GameDefinition>,
    mut sim: ResMut<SimulationBridge>,
    mut ui_state: ResMut<UiState>,
    mut bookmarks: ResMut<BookmarkStore>,
    mut cache: ResMut<RenderCache>,
    mut viewport: ResMut<ViewportState>,
    mut camera_actions: ResMut<PendingCameraAction>,
    mut board_export_pending: ResMut<board_export::BoardExportPending>,
    #[cfg(not(target_family = "wasm"))]
    board_export_dialog: NonSend<board_export::BoardExportDialogState>,
    #[cfg(target_family = "wasm")]
    board_export_wasm: Res<board_export::BoardExportWasmJob>,
    #[cfg(target_family = "wasm")]
    share_code_paste: Res<crate::wasm_clipboard::WasmShareCodePaste>,
    mut camera_q: Query<(&mut Transform, &mut Projection, &PanCamera), With<BoardCamera>>,
    window_q: Query<&Window>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let mut draft = ui_state
        .draft
        .take()
        .unwrap_or_else(|| def.as_ref().clone());
    let panel_response = egui::SidePanel::left("game_config")
        .default_width(SIDEBAR_PANEL_WIDTH)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("sidebar_scroll")
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.heading("Red & Black Knights");
                    ui_state.sidebar.view = sidebar_collapsing(ui, "view", "View", ui_state.sidebar.view, false, |ui| {
                        if ui.button("Center view").clicked() {
                            camera_actions.center_view = true;
                        }

                        ui.add_space(4.0);
                        if ui.button("Export PNG").clicked() {
                            ui_state.export_status = if let Some(bounds) = viewport.bounds {
                                let queued = {
                                    #[cfg(not(target_family = "wasm"))]
                                    {
                                        board_export::queue_board_png_export(
                                            &mut board_export_pending,
                                            bounds,
                                            &board_export_dialog,
                                        )
                                    }
                                    #[cfg(target_family = "wasm")]
                                    {
                                        board_export::queue_board_png_export(
                                            &mut board_export_pending,
                                            bounds,
                                            &board_export_wasm,
                                        )
                                    }
                                };
                                match queued {
                                    Ok(()) => Some(
                                        if cfg!(target_family = "wasm") {
                                            "Exporting…".into()
                                        } else {
                                            "Choose save location…".into()
                                        },
                                    ),
                                    Err(err) => Some(err),
                                }
                            } else {
                                Some("Board not ready yet".into())
                            };
                        }
                        if let Some(status) = &ui_state.export_status {
                            ui.label(status);
                        }

                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);

                        if let Ok((mut transform, mut projection, pan)) = camera_q.single_mut()
                        {
                            if let Projection::Orthographic(ref mut ortho) = *projection {
                                sidebar_field_label(ui, "Position");
                                ui.columns(2, |cols| {
                                    let row_h = cols[0].spacing().interact_size.y;
                                    let w0 = cols[0].available_width();
                                    let w1 = cols[1].available_width();
                                    cols[0].add_sized(
                                        egui::vec2(w0, row_h),
                                        egui::DragValue::new(&mut transform.translation.x)
                                            .speed(4.0),
                                    );
                                    cols[1].add_sized(
                                        egui::vec2(w1, row_h),
                                        egui::DragValue::new(&mut transform.translation.y)
                                            .speed(4.0),
                                    );
                                });
                                ui.add_space(4.0);

                                let effective_zoom_max =
                                    if let Ok(window) = window_q.single() {
                                        let safe = viewport::max_safe_zoom_out_scale(
                                            ortho,
                                            window,
                                            viewport.left_inset_px,
                                        );
                                        calibration_config::effective_zoom_out_max(safe, pan.max_scale)
                                    } else {
                                        calibration_config::MAX_ZOOM_OUT_BUDGET
                                            .min(pan.max_scale)
                                    };
                                ortho.scale = ortho
                                    .scale
                                    .clamp(pan.min_scale, effective_zoom_max);

                                sidebar_field_label(ui, "Zoom");
                                ui.add(
                                    egui::DragValue::new(&mut ortho.scale)
                                        .range(pan.min_scale..=effective_zoom_max)
                                        .speed(0.02)
                                        .fixed_decimals(3),
                                );
                                ui.add_space(SIDEBAR_FIELD_GAP);
                            }
                        }
                    });
                    ui_state.sidebar.library = sidebar_collapsing(
                        ui,
                        "library",
                        "Library",
                        ui_state.sidebar.library,
                        false,
                        |ui| {
                            let catalog = GameDefinition::preset_catalog();
                            let custom_n = bookmarks.bookmarks.len();
                            if let Some(sel) = bookmarks.selected {
                                if sel >= custom_n {
                                    bookmarks.selected = None;
                                }
                            }
                            let total = library_entry_count(custom_n);
                            let selected_idx =
                                library_selected_index(&bookmarks, ui_state.preset_index);
                            let selected_label =
                                library_selected_label(&bookmarks, ui_state.preset_index);

                            if total > 0 {
                                egui::ComboBox::from_id_salt("library_pick")
                                    .selected_text(selected_label)
                                    .show_ui(ui, |ui| {
                                        let mut pick: Option<usize> = None;
                                        for (i, (label, _)) in catalog.iter().enumerate() {
                                            let is_sel = bookmarks.selected.is_none()
                                                && ui_state.preset_index == i;
                                            if ui.selectable_label(is_sel, *label).clicked() {
                                                pick = Some(i);
                                            }
                                        }
                                        for (j, bm) in bookmarks.bookmarks.iter().enumerate() {
                                            if ui
                                                .selectable_label(
                                                    bookmarks.selected == Some(j),
                                                    &bm.name,
                                                )
                                                .clicked()
                                            {
                                                pick = Some(catalog.len() + j);
                                            }
                                        }
                                        if let Some(i) = pick {
                                            apply_library_index(
                                                i,
                                                &mut draft,
                                                &mut ui_state,
                                                &mut bookmarks,
                                                &mut viewport,
                                                &mut camera_q,
                                            );
                                        }
                                    });
                                ui.horizontal(|ui| {
                                    if ui.button("◀ Previous").clicked() {
                                        apply_library_index(
                                            selected_idx + total - 1,
                                            &mut draft,
                                            &mut ui_state,
                                            &mut bookmarks,
                                            &mut viewport,
                                            &mut camera_q,
                                        );
                                    }
                                    if ui.button("Next ▶").clicked() {
                                        apply_library_index(
                                            selected_idx + 1,
                                            &mut draft,
                                            &mut ui_state,
                                            &mut bookmarks,
                                            &mut viewport,
                                            &mut camera_q,
                                        );
                                    }
                                });
                                ui.add_space(4.0);
                            }

                            ui.text_edit_singleline(&mut ui_state.bookmark_new_name);
                            ui.horizontal(|ui| {
                                let custom_selected = bookmarks.selected;
                                let can_delete =
                                    custom_selected.is_some() && custom_n > 0;
                                if ui
                                    .add_enabled(can_delete, egui::Button::new("Delete bookmark"))
                                    .clicked()
                                {
                                    bookmarks.remove_selected();
                                }
                                if ui.button("Add bookmark").clicked() {
                                    let name = if ui_state.bookmark_new_name.trim().is_empty() {
                                        format!("Bookmark {}", custom_n + 1)
                                    } else {
                                        ui_state.bookmark_new_name.trim().to_string()
                                    };
                                    ui_state.bookmark_new_name.clear();
                                    if let Some(camera) =
                                        read_camera_session(&camera_q, &window_q, &viewport)
                                    {
                                        let bm = Bookmark::capture(
                                            name,
                                            &draft,
                                            camera,
                                            viewport.target_index,
                                        );
                                        bookmarks.add(bm);
                                    }
                                }
                            });

                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(4.0);

                            const SHARE_COPIED_FLASH_SECS: f64 = 1.0;
                            let now = ctx.input(|i| i.time);
                            if ui_state
                                .share_code_copied_at
                                .is_some_and(|t| now - t >= SHARE_COPIED_FLASH_SECS)
                            {
                                ui_state.share_code_copied_at = None;
                            }
                            let copy_share_label = if ui_state.share_code_copied_at.is_some() {
                                "Copied!"
                            } else {
                                "Copy share code"
                            };
                            if ui.button(copy_share_label).clicked() {
                                match read_camera_session(&camera_q, &window_q, &viewport) {
                                    Some(camera) => {
                                        let snap = share_code::capture_share_view(ShareCapture {
                                            def: &draft,
                                            camera,
                                            target_index: viewport.target_index,
                                            board_colour_mode: ui_state.board_colour_mode,
                                        });
                                        match share_code::encode_share_code(&snap) {
                                            Ok(code) => {
                                                #[cfg(target_family = "wasm")]
                                                {
                                                    use crate::wasm_clipboard::{
                                                        ShareCodeCopyOutcome,
                                                        publish_share_code_for_copy,
                                                    };
                                                    match publish_share_code_for_copy(&code) {
                                                        Ok(
                                                            ShareCodeCopyOutcome::SystemClipboard,
                                                        ) => {
                                                            ui_state.share_code_copied_at =
                                                                Some(ctx.input(|i| i.time));
                                                        }
                                                        Ok(
                                                            ShareCodeCopyOutcome::ManualCopyDialog,
                                                        ) => {}
                                                        Err(_) => {}
                                                    }
                                                }
                                                #[cfg(not(target_family = "wasm"))]
                                                {
                                                    ctx.copy_text(code);
                                                    ui_state.share_code_copied_at =
                                                        Some(ctx.input(|i| i.time));
                                                }
                                            }
                                            Err(_) => {}
                                        }
                                    }
                                    None => {}
                                }
                            }
                            if ui.button("Import share code").clicked() {
                                ui_state.share_code_input.clear();
                                #[cfg(target_family = "wasm")]
                                share_code_paste.open_paste_dialog("");
                                #[cfg(not(target_family = "wasm"))]
                                {
                                    ui_state.share_code_import_dialog_open = true;
                                }
                            }
                        },
                    );
                    ui_state.sidebar.colouring = sidebar_collapsing(
                        ui,
                        "colouring",
                        "Colouring",
                        ui_state.sidebar.colouring,
                        false,
                        |ui| {
                            let prev = ui_state.board_colour_mode;
                            ui.radio_value(
                                &mut ui_state.board_colour_mode,
                                BoardColourMode::Piece,
                                "Piece colour",
                            );
                            if ui_state.board_colour_mode != prev {
                                viewport.render_dirty = true;
                                cache.rendered_bounds = None;
                            }
                        },
                    );
                    ui_state.sidebar.random_attacks = sidebar_collapsing(
                        ui,
                        "random_attacks",
                        "Random attacks",
                        ui_state.sidebar.random_attacks,
                        false,
                        |ui| {
                        let rg = &mut ui_state.random_gen;
                        ui.scope(|ui| {
                            sidebar_u32_range(
                                ui,
                                "Piece count",
                                &mut rg.piece_count_min,
                                &mut rg.piece_count_max,
                                1..=32,
                                None,
                            );
                            sidebar_i32_range(
                                ui,
                                "Attack radius",
                                &mut rg.attack_radius_min,
                                &mut rg.attack_radius_max,
                                1..=12,
                                Some(
                                    "Chebyshev distance from each piece when sampling attack cells",
                                ),
                            );

                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(4.0);

                            sidebar_f32_slider(
                                ui,
                                "Pattern density",
                                &mut rg.pattern_density,
                                0.0..=1.0,
                                false,
                                None,
                            );
                            sidebar_field_label(ui, "Attack symmetry");
                            egui::ComboBox::from_id_salt("random_attack_symmetry")
                                .selected_text(rg.attack_symmetry.label())
                                .show_ui(ui, |ui| {
                                    for mode in AttackSymmetry::ALL {
                                        if ui
                                            .selectable_label(
                                                rg.attack_symmetry == mode,
                                                mode.label(),
                                            )
                                            .clicked()
                                        {
                                            rg.attack_symmetry = mode;
                                        }
                                    }
                                });
                            ui.checkbox(
                                &mut rg.identical_pieces,
                                "Identical attack patterns",
                            );
                            ui.add_space(SIDEBAR_FIELD_GAP);

                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(6.0);
                            if ui.button("Generate random attacks").clicked() {
                                rg.sanitize();
                                let mut rng = rand::rng();
                                draft = generate_random_game(rg, &mut rng);
                            }
                        });
                    });

                    ui_state.sidebar.random_pieces = sidebar_collapsing(
                        ui,
                        "random_pieces",
                        "Random pieces",
                        ui_state.sidebar.random_pieces,
                        false,
                        |ui| {
                        let catalog = PieceDef::piece_catalog();
                        {
                        let rpc = &mut ui_state.random_pieces_config;

                        let slot_count = rpc.slots.len();
                        let mut remove_at: Option<usize> = None;
                        for slot_i in 0..slot_count {
                            let mut locked = rpc.slots[slot_i].locked;
                            let mut random_attack = rpc.slots[slot_i].random_attack;
                            let mut catalog_index = rpc.slots[slot_i]
                                .catalog_index
                                .min(catalog.len().saturating_sub(1));
                            let mut color_rgb = {
                                let c = rpc.slots[slot_i].color.to_bevy().to_srgba();
                                [c.red, c.green, c.blue]
                            };
                            let selected_label = if locked {
                                catalog
                                    .get(catalog_index)
                                    .map(|(name, _)| *name)
                                    .unwrap_or("?")
                            } else if random_attack {
                                "random attack"
                            } else {
                                "random preset"
                            };
                            ui.horizontal(|ui| {
                                if slot_count > 1 {
                                    let remove = ui
                                        .add_sized(
                                            egui::vec2(22.0, 22.0),
                                            egui::Button::new("X"),
                                        )
                                        .on_hover_text("Remove slot")
                                        .clicked();
                                    if remove {
                                        remove_at = Some(slot_i);
                                    }
                                }
                                egui::ComboBox::from_id_salt(format!(
                                    "random_piece_slot_{slot_i}"
                                ))
                                .selected_text(selected_label)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(
                                            !locked && !random_attack,
                                            "random preset",
                                        )
                                        .clicked()
                                    {
                                        locked = false;
                                        random_attack = false;
                                    }
                                    if ui
                                        .selectable_label(
                                            !locked && random_attack,
                                            "random attack",
                                        )
                                        .clicked()
                                    {
                                        locked = false;
                                        random_attack = true;
                                    }
                                    for (ci, (name, _)) in catalog.iter().enumerate() {
                                        let picked =
                                            locked && catalog_index == ci;
                                        if ui.selectable_label(picked, *name).clicked() {
                                            locked = true;
                                            random_attack = false;
                                            catalog_index = ci;
                                        }
                                    }
                                });
                                if ui.color_edit_button_rgb(&mut color_rgb).changed() {
                                    rpc.slots[slot_i].color =
                                        crate::game_snapshot::SavedColor::from_bevy(Color::srgb(
                                            color_rgb[0],
                                            color_rgb[1],
                                            color_rgb[2],
                                        ));
                                }
                            });
                            rpc.slots[slot_i].locked = locked;
                            rpc.slots[slot_i].random_attack = random_attack;
                            rpc.slots[slot_i].catalog_index = catalog_index;
                        }
                        if let Some(i) = remove_at {
                            rpc.slots.remove(i);
                        }

                        if rpc.slots.len() < 32 {
                            if ui
                                .add_sized(
                                    egui::vec2(22.0, 22.0),
                                    egui::Button::new("+"),
                                )
                                .on_hover_text("Add slot")
                                .clicked()
                            {
                                let i = rpc.slots.len();
                                rpc.slots.push(RandomPieceSlot::with_default_color(i));
                            }
                        }
                        }

                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(6.0);
                        if ui.button("Generate random pieces").clicked() {
                            ui_state.random_pieces_config.sanitize();
                            let pieces_cfg = ui_state.random_pieces_config.clone();
                            let mut attack_cfg = ui_state.random_gen.clone();
                            attack_cfg.sanitize();
                            let mut rng = rand::rng();
                            draft = generate_random_pieces_game(
                                &pieces_cfg,
                                &attack_cfg,
                                &mut rng,
                            );
                        }
                    });

                    ui_state.sidebar.mutate = sidebar_collapsing(
                        ui,
                        "mutate",
                        "Mutate",
                        ui_state.sidebar.mutate,
                        false,
                        |ui| {
                        if draft.pieces.is_empty() {
                            ui.label("No pieces");
                        } else {
                            if !ui_state.mutate_all
                                && ui_state.mutate_piece >= draft.pieces.len()
                            {
                                ui_state.mutate_piece = draft.pieces.len().saturating_sub(1);
                            }
                            let selected = if ui_state.mutate_all {
                                "All".to_string()
                            } else {
                                let a = ui_state.mutate_piece;
                                format!("{}: {}", a, draft.pieces[a].name)
                            };
                            egui::ComboBox::from_id_salt("mutate_piece_pick")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(ui_state.mutate_all, "All")
                                        .clicked()
                                    {
                                        ui_state.mutate_all = true;
                                    }
                                    for (aid, piece) in draft.pieces.iter().enumerate() {
                                        let picked = !ui_state.mutate_all
                                            && ui_state.mutate_piece == aid;
                                        if ui
                                            .selectable_label(
                                                picked,
                                                format!("{aid}: {}", piece.name),
                                            )
                                            .clicked()
                                        {
                                            ui_state.mutate_all = false;
                                            ui_state.mutate_piece = aid;
                                        }
                                    }
                                });

                            let targets: Vec<usize> = if ui_state.mutate_all {
                                (0..draft.pieces.len()).collect()
                            } else {
                                vec![ui_state.mutate_piece]
                            };

                            let mut rng = rand::rng();
                            let piece_count = draft.pieces.len();
                            let shared_r = if ui_state.mutate_all && targets.len() > 1 {
                                Some(shared_attack_extent_for_pieces(&draft.pieces, &targets))
                            } else {
                                None
                            };

                            ui.add_space(4.0);
                            ui.scope(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 5.0);

                                if mutate_panel_button(ui, "Toggle attack square") {
                                        for &aid in &targets {
                                            toggle_random_attack_square(
                                                &mut draft.pieces[aid].piece.valid_moves,
                                                &mut rng,
                                            );
                                        }
                                    }

                                    let (shift_px, shift_mx) =
                                        mutate_panel_pair(ui, "+X", "−X");
                                    if shift_px {
                                        for &aid in &targets {
                                            shift_attacks(
                                                &mut draft.pieces[aid].piece.valid_moves,
                                                1,
                                                0,
                                                shared_r,
                                            );
                                        }
                                    }
                                    if shift_mx {
                                        for &aid in &targets {
                                            shift_attacks(
                                                &mut draft.pieces[aid].piece.valid_moves,
                                                -1,
                                                0,
                                                shared_r,
                                            );
                                        }
                                    }
                                    let (shift_py, shift_my) =
                                        mutate_panel_pair(ui, "+Y", "−Y");
                                    if shift_py {
                                        for &aid in &targets {
                                            shift_attacks(
                                                &mut draft.pieces[aid].piece.valid_moves,
                                                0,
                                                1,
                                                shared_r,
                                            );
                                        }
                                    }
                                    if shift_my {
                                        for &aid in &targets {
                                            shift_attacks(
                                                &mut draft.pieces[aid].piece.valid_moves,
                                                0,
                                                -1,
                                                shared_r,
                                            );
                                        }
                                    }

                                    let (flip_y, flip_x) =
                                        mutate_panel_pair(ui, "Flip Y", "Flip X");
                                    if flip_y {
                                        for &aid in &targets {
                                            reflect_across_x_axis(
                                                &mut draft.pieces[aid].piece.valid_moves,
                                            );
                                        }
                                    }
                                    if flip_x {
                                        for &aid in &targets {
                                            reflect_across_y_axis(
                                                &mut draft.pieces[aid].piece.valid_moves,
                                            );
                                        }
                                    }

                                    let (rot_ccw, rot_cw) =
                                        mutate_panel_pair(ui, "↺ CCW", "↻ CW");
                                    if rot_ccw {
                                        for &aid in &targets {
                                            rotate_ccw(
                                                &mut draft.pieces[aid].piece.valid_moves,
                                            );
                                        }
                                    }
                                    if rot_cw {
                                        for &aid in &targets {
                                            rotate_cw(
                                                &mut draft.pieces[aid].piece.valid_moves,
                                            );
                                        }
                                    }

                                    if mutate_panel_button(ui, "Toggle blocked-by") {
                                        for &aid in &targets {
                                            toggle_random_blocked_by(
                                                &mut draft.pieces[aid],
                                                aid,
                                                piece_count,
                                                &mut rng,
                                            );
                                        }
                                    }
                                });
                        }
                    });

                    ui_state.sidebar.pieces = sidebar_collapsing(
                        ui,
                        "pieces",
                        "Pieces",
                        ui_state.sidebar.pieces,
                        false,
                        |ui| {
                        let mut open_advanced = false;
                        ui_state.sidebar.pieces_summary = sidebar_collapsing(
                            ui,
                            "pieces_summary",
                            "Summary",
                            ui_state.sidebar.pieces_summary,
                            false,
                            |ui| {
                                if draft.pieces.is_empty() {
                                    ui.label("No pieces in set");
                                } else {
                                    let advanced_was_open = ui_state.sidebar.pieces_advanced;
                                    let row_h = summary_strip_row_height(&draft);
                                    egui::ScrollArea::horizontal()
                                        .min_scrolled_height(0.0)
                                        .max_height(row_h)
                                        .show(ui, |ui| {
                                            ui.horizontal_top(|ui| {
                                                ui.spacing_mut().item_spacing =
                                                    egui::vec2(2.0, 0.0);
                                                for (piece_idx, piece) in
                                                    draft.pieces.iter().enumerate()
                                                {
                                                    let blocked: Vec<_> = piece
                                                        .blocked_by
                                                        .iter()
                                                        .filter_map(|&id| {
                                                            draft
                                                                .pieces
                                                                .get(id)
                                                                .map(|a| a.name.as_str())
                                                        })
                                                        .collect();
                                                    let blocked_line = if blocked.is_empty() {
                                                        "blocked by: —".to_string()
                                                    } else {
                                                        format!(
                                                            "blocked by: {}",
                                                            blocked.join(", ")
                                                        )
                                                    };
                                                    let hover = format!(
                                                        "{piece_idx}: {}\n{blocked_line}\nClick to edit",
                                                        piece.name
                                                    );
                                                    let selected = piece_idx
                                                        == ui_state.edit_piece
                                                        && (advanced_was_open
                                                            || open_advanced);

                                                    let preview = move_grid_preview_ui(
                                                        ui,
                                                        piece_idx,
                                                        &piece.piece.valid_moves,
                                                        piece.color,
                                                        selected,
                                                    )
                                                    .on_hover_text(hover);

                                                    if preview.clicked() {
                                                        ui_state.edit_piece = piece_idx;
                                                        open_advanced = true;
                                                    }
                                                }
                                            });
                                        });
                                }
                            },
                        );

                        pieces_roster_editor_ui(ui, &mut draft, &mut ui_state);

                        if !draft.pieces.is_empty() {
                            ui_state.sidebar.pieces_advanced = sidebar_collapsing(
                                ui,
                                "pieces_advanced",
                                "Advanced",
                                ui_state.sidebar.pieces_advanced,
                                open_advanced,
                                |ui| {
                                if ui_state.edit_piece >= draft.pieces.len() {
                                    ui_state.edit_piece =
                                        draft.pieces.len().saturating_sub(1);
                                }
                                let piece_idx = ui_state.edit_piece;

                                ui.horizontal(|ui| {
                                    ui.label("Name");
                                    ui.text_edit_singleline(&mut draft.pieces[piece_idx].name);
                                });

                                ui.horizontal(|ui| {
                                    ui.checkbox(
                                        &mut draft.pieces[piece_idx].enabled,
                                        "Enabled",
                                    )
                                    .on_hover_text(
                                        "Disabled pieces stay in the set but do not take placement turns",
                                    );
                                    if ui.button("Duplicate").clicked() {
                                        duplicate_draft_piece(
                                            &mut draft,
                                            &mut ui_state,
                                            piece_idx,
                                        );
                                    }
                                });

                                let rgb = draft.pieces[piece_idx].color.to_srgba();
                                let mut arr = [rgb.red, rgb.green, rgb.blue];
                                if ui.color_edit_button_rgb(&mut arr).changed() {
                                    draft.pieces[piece_idx].color =
                                        Color::srgb(arr[0], arr[1], arr[2]);
                                }

                                ui.horizontal(|ui| {
                                    ui.label("Attacked squares");
                                    ui.checkbox(
                                        &mut ui_state.sync_attack_squares,
                                        "Sync all pieces",
                                    )
                                    .on_hover_text(
                                        "When enabled, each square toggle applies to every piece",
                                    );
                                });
                                move_grid_ui(
                                    ui,
                                    piece_idx,
                                    &mut draft.pieces,
                                    ui_state.sync_attack_squares,
                                );
                                if ui.button("Clear").clicked() {
                                    clear_attack_squares(
                                        &mut draft.pieces,
                                        piece_idx,
                                        ui_state.sync_attack_squares,
                                    );
                                }

                                ui.label("Blocked by");
                                for other in 0..draft.pieces.len() {
                                    if other == piece_idx {
                                        continue;
                                    }
                                    let mut blocked =
                                        draft.pieces[piece_idx].blocked_by.contains(&other);
                                    let label = draft.pieces[other].name.clone();
                                    if ui.checkbox(&mut blocked, label).changed() {
                                        if blocked {
                                            if !draft.pieces[piece_idx]
                                                .blocked_by
                                                .contains(&other)
                                            {
                                                draft.pieces[piece_idx].blocked_by.push(other);
                                            }
                                        } else {
                                            draft.pieces[piece_idx]
                                                .blocked_by
                                                .retain(|&id| id != other);
                                        }
                                    }
                                }

                                if ui.button("Remove piece").clicked() {
                                    remove_draft_piece(
                                        &mut draft,
                                        &mut ui_state,
                                        piece_idx,
                                    );
                                }
                            },
                            );
                        }
                    });

                    if draft.turn_order.is_empty() && !draft.pieces.is_empty() {
                        draft.turn_order = (0..draft.pieces.len()).collect();
                    }

                    #[cfg(not(target_family = "wasm"))]
                    {
                        ui.separator();
                        ui_state.sidebar.debug = sidebar_collapsing(
                            ui,
                            "debug",
                            "Debug",
                            ui_state.sidebar.debug,
                            false,
                            |ui| {
                                if let Some(bounds) = viewport.bounds {
                                    let grid_size = grid_texture_size(bounds);
                                    ui.label(format!(
                                        "Grid cells: {} x {}",
                                        grid_size.x, grid_size.y
                                    ));
                                    ui.label(format!(
                                        "Render texels: {}",
                                        grid_size.x as u64 * grid_size.y as u64
                                    ));
                                    ui.label(format!("Target index: {}", viewport.target_index));
                                } else {
                                    ui.label("Grid cells: pending");
                                }

                                if let (Ok((_, Projection::Orthographic(ortho), _)), Ok(window)) =
                                    (camera_q.single(), window_q.single())
                                {
                                    let board_width_px = (window.width() - viewport.left_inset_px)
                                        .ceil()
                                        .max(1.0);
                                    let world_per_screen_px =
                                        ortho.area.width() / board_width_px.max(1.0);
                                    let cells_per_screen_px =
                                        world_per_screen_px / CELL_SIZE;
                                    let board_pixels = board_width_px as u64
                                        * window.height().ceil().max(1.0) as u64;
                                    ui.label(format!("Zoom scale: {:.3}", ortho.scale));
                                    ui.label(format!("Cells per px: {:.3}", cells_per_screen_px));
                                    ui.label(format!("Board pixels: {board_pixels}"));
                                    ui.label(format!(
                                        "Left inset px: {:.0}",
                                        viewport.left_inset_px
                                    ));
                                }
                            },
                        );
                    }
                });
        });
    viewport.left_inset_px = panel_response.response.rect.width();

    if ui_state.share_code_import_pending {
        ui_state.share_code_import_pending = false;
        let code = ui_state.share_code_input.clone();
        import_share_code_from_text(
            &code,
            def.as_mut(),
            sim.as_mut(),
            &mut ui_state,
            &mut viewport,
            &mut cache,
            &mut camera_q,
            &mut draft,
            &mut bookmarks,
        );
    }

    #[cfg(not(target_family = "wasm"))]
    share_code_import_egui_dialog(
        ctx,
        &mut ui_state,
        def.as_mut(),
        sim.as_mut(),
        &mut viewport,
        &mut cache,
        &mut camera_q,
        &mut draft,
        &mut bookmarks,
    );

    if !draft.same_sim_state(def.as_ref()) {
        dedupe_moves(&mut draft);
        *def = draft.clone();
        sim.request_reset(def.clone());
        cache.rendered_bounds = None;
        viewport.bounds = None;
        viewport.target_index = 0;
        viewport.render_dirty = true;
    } else if !draft.same_applied_state(def.as_ref()) {
        *def = draft.clone();
        viewport.render_dirty = true;
    }

    ui_state.draft = Some(draft);
}

fn dedupe_moves(def: &mut GameDefinition) {
    for piece in &mut def.pieces {
        piece.piece.valid_moves.sort_by_key(|&(x, y)| (x, y));
        piece.piece.valid_moves.dedup();
    }
}

const SIDEBAR_PANEL_WIDTH: f32 = 320.0;
const MUTATE_BTN_HEIGHT: f32 = 24.0;

fn mutate_panel_button(ui: &mut egui::Ui, label: &str) -> bool {
    let w = ui.available_width();
    ui.add_sized(
        egui::vec2(w, MUTATE_BTN_HEIGHT),
        egui::Button::new(label),
    )
    .clicked()
}

fn mutate_panel_pair(ui: &mut egui::Ui, left: &str, right: &str) -> (bool, bool) {
    ui.columns(2, |cols| {
        (
            mutate_panel_button(&mut cols[0], left),
            mutate_panel_button(&mut cols[1], right),
        )
    })
}

const SIDEBAR_FIELD_GAP: f32 = 12.0;

fn sidebar_field_label(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(text)
}

fn sidebar_f32_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    logarithmic: bool,
    hover: Option<&str>,
) {
    let caption = sidebar_field_label(ui, label);
    if let Some(text) = hover {
        caption.on_hover_text(text);
    }
    let row_h = ui.spacing().interact_size.y;
    ui.columns(2, |cols| {
        let w0 = cols[0].available_width();
        let w1 = cols[1].available_width();
        let prev_slider_width = cols[0].style().spacing.slider_width;
        cols[0].style_mut().spacing.slider_width = w0;
        let mut slider = egui::Slider::new(value, range.clone())
            .show_value(false)
            .clamping(egui::SliderClamping::Always);
        if logarithmic {
            slider = slider.logarithmic(true);
        }
        cols[0].add(slider);
        cols[0].style_mut().spacing.slider_width = prev_slider_width;
        let speed = if logarithmic { 0.05 } else { 0.01 };
        cols[1].add_sized(
            egui::vec2(w1, row_h),
            egui::DragValue::new(value)
                .range(range)
                .speed(speed)
                .fixed_decimals(2),
        );
    });
    ui.add_space(SIDEBAR_FIELD_GAP);
}

fn sidebar_u32_range(
    ui: &mut egui::Ui,
    title: &str,
    min: &mut u32,
    max: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    hover: Option<&str>,
) {
    let caption = sidebar_field_label(ui, title);
    if let Some(text) = hover {
        caption.on_hover_text(text);
    }
    sidebar_min_max_drag_row(ui, min, max, range);
}

fn sidebar_i32_range(
    ui: &mut egui::Ui,
    title: &str,
    min: &mut i32,
    max: &mut i32,
    range: std::ops::RangeInclusive<i32>,
    hover: Option<&str>,
) {
    let caption = sidebar_field_label(ui, title);
    if let Some(text) = hover {
        caption.on_hover_text(text);
    }
    sidebar_min_max_drag_row(ui, min, max, range);
}

fn sidebar_min_max_drag_row<T>(
    ui: &mut egui::Ui,
    min: &mut T,
    max: &mut T,
    range: std::ops::RangeInclusive<T>,
) where
    T: egui::emath::Numeric,
{
    let row_h = ui.spacing().interact_size.y;
    ui.columns(2, |cols| {
        let w0 = cols[0].available_width();
        let w1 = cols[1].available_width();
        cols[0].add_sized(
            egui::vec2(w0, row_h),
            egui::DragValue::new(min)
                .prefix("Min ")
                .range(range.clone()),
        );
        cols[1].add_sized(
            egui::vec2(w1, row_h),
            egui::DragValue::new(max).prefix("Max ").range(range),
        );
    });
    ui.add_space(SIDEBAR_FIELD_GAP);
}

fn duplicate_draft_piece(draft: &mut GameDefinition, ui_state: &mut UiState, piece_idx: usize) {
    if piece_idx >= draft.pieces.len() {
        return;
    }
    let mut copy = draft.pieces[piece_idx].clone();
    copy.name = format!("{} copy", copy.name);
    let new_id = draft.pieces.len();
    for piece in &mut draft.pieces {
        if piece.blocked_by.contains(&piece_idx) && !piece.blocked_by.contains(&new_id) {
            piece.blocked_by.push(new_id);
        }
    }
    draft.pieces.push(copy);
    draft.turn_order.push(new_id);
    ui_state.edit_piece = new_id;
    ui_state.roster_remove_piece = ui_state
        .roster_remove_piece
        .min(draft.pieces.len().saturating_sub(1));
}

fn remove_draft_piece(draft: &mut GameDefinition, ui_state: &mut UiState, piece_idx: usize) {
    if piece_idx >= draft.pieces.len() {
        return;
    }
    draft.pieces.remove(piece_idx);
    for piece in &mut draft.pieces {
        piece.blocked_by.retain(|&id| id != piece_idx);
        for b in &mut piece.blocked_by {
            if *b > piece_idx {
                *b -= 1;
            }
        }
    }
    draft.turn_order.retain(|&id| id != piece_idx);
    for t in &mut draft.turn_order {
        if *t > piece_idx {
            *t -= 1;
        }
    }
    if draft.pieces.is_empty() {
        draft.turn_order.clear();
    }
    if ui_state.edit_piece >= draft.pieces.len() {
        ui_state.edit_piece = draft.pieces.len().saturating_sub(1);
    }
    if !ui_state.mutate_all && ui_state.mutate_piece >= draft.pieces.len() {
        ui_state.mutate_piece = draft.pieces.len().saturating_sub(1);
    }
    ui_state.roster_remove_piece = ui_state
        .roster_remove_piece
        .min(draft.pieces.len().saturating_sub(1));
}

fn pieces_roster_editor_ui(
    ui: &mut egui::Ui,
    draft: &mut GameDefinition,
    ui_state: &mut UiState,
) {
    ui_state.sidebar.edit_roster = sidebar_collapsing(
        ui,
        "edit_roster",
        "Edit roster",
        ui_state.sidebar.edit_roster,
        false,
        |ui| {
            pieces_roster_editor_body(ui, draft, ui_state);
        },
    );
}

fn pieces_roster_editor_body(
    ui: &mut egui::Ui,
    draft: &mut GameDefinition,
    ui_state: &mut UiState,
) {
    ui.with_layout(
        egui::Layout::left_to_right(egui::Align::TOP).with_main_wrap(false),
        |ui| {
            let catalog = PieceDef::piece_catalog();
            if ui_state.add_piece_preset_index >= catalog.len() {
                ui_state.add_piece_preset_index = 0;
            }
            let preset_i = ui_state.add_piece_preset_index;
            let (preset_label, preset_factory) = catalog[preset_i];
            let preview_piece = preset_factory();

            ui.vertical(|ui| {
                egui::ComboBox::from_id_salt("add_piece_preset")
                    .selected_text(preset_label)
                    .show_ui(ui, |ui| {
                        for (i, (label, _)) in catalog.iter().enumerate() {
                            if ui.selectable_label(preset_i == i, *label).clicked() {
                                ui_state.add_piece_preset_index = i;
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label("Colour");
                    let rgb = ui_state.add_piece_color.to_srgba();
                    let mut arr = [rgb.red, rgb.green, rgb.blue];
                    if ui.color_edit_button_rgb(&mut arr).changed() {
                        ui_state.add_piece_color =
                            Color::srgb(arr[0], arr[1], arr[2]);
                    }
                });
                move_grid_preview_ui(
                    ui,
                    900_000 + preset_i,
                    &preview_piece.valid_moves,
                    ui_state.add_piece_color,
                    false,
                );
                if ui.button("Add").clicked() {
                    let color = ui_state.add_piece_color;
                    draft.push_piece_from_piece_preset(preset_label, preview_piece, color);
                    ui_state.add_piece_color =
                        GameDefinition::default_piece_color(draft.pieces.len());
                    ui_state.edit_piece = draft.pieces.len().saturating_sub(1);
                    ui_state.roster_remove_piece = ui_state.edit_piece;
                    if !ui_state.mutate_all && ui_state.mutate_piece >= draft.pieces.len() {
                        ui_state.mutate_piece = draft.pieces.len().saturating_sub(1);
                    }
                }
            });

            if !draft.pieces.is_empty() {
                ui.add_space(12.0);

                ui.vertical(|ui| {
                    ui.label("Remove");
                    if ui_state.roster_remove_piece >= draft.pieces.len() {
                        ui_state.roster_remove_piece = draft.pieces.len().saturating_sub(1);
                    }
                    let remove_idx = ui_state.roster_remove_piece;
                    egui::ComboBox::from_id_salt("roster_remove_pick")
                        .selected_text(format!(
                            "{}: {}",
                            remove_idx, draft.pieces[remove_idx].name
                        ))
                        .show_ui(ui, |ui| {
                            for (aid, piece) in draft.pieces.iter().enumerate() {
                                if ui
                                    .selectable_label(
                                        remove_idx == aid,
                                        format!("{aid}: {}", piece.name),
                                    )
                                    .clicked()
                                {
                                    ui_state.roster_remove_piece = aid;
                                }
                            }
                        });
                    let can_remove = !draft.pieces.is_empty();
                    if ui
                        .add_enabled(can_remove, egui::Button::new("Remove"))
                        .clicked()
                    {
                        remove_draft_piece(draft, ui_state, remove_idx);
                        ui_state.roster_remove_piece = ui_state
                            .edit_piece
                            .min(draft.pieces.len().saturating_sub(1));
                    }
                });
            }
        },
    );
}

fn summary_preview_side_px(moves: &[(i32, i32)]) -> f32 {
    let radius = move_grid_radius(moves, 1);
    let cells = (2 * radius + 1) as f32;
    cells * MOVE_PREVIEW_CELL_PX + 2.0 * MOVE_PREVIEW_PAD_PX
}

fn summary_strip_row_height(draft: &GameDefinition) -> f32 {
    draft
        .pieces
        .iter()
        .map(|a| summary_preview_side_px(&a.piece.valid_moves))
        .fold(0.0_f32, f32::max)
}

const MOVE_PREVIEW_CELL_PX: f32 = 8.0;
const MOVE_PREVIEW_PAD_PX: f32 = 4.0;
const ATTACK_GRID_MIN_RADIUS: i32 = 4;
const ATTACK_GRID_LAYOUT_RADIUS: i32 = 4;
const ATTACK_GRID_CELL_PX: f32 = 22.0;
const ATTACK_GRID_CELL_GAP: f32 = 1.0;

fn attack_grid_viewport_side_px() -> f32 {
    let cells = (2 * ATTACK_GRID_LAYOUT_RADIUS + 1) as f32;
    cells * ATTACK_GRID_CELL_PX + (cells - 1.0).max(0.0) * ATTACK_GRID_CELL_GAP
}

fn move_grid_radius(moves: &[(i32, i32)], min_radius: i32) -> i32 {
    moves
        .iter()
        .map(|&(dx, dy)| dx.abs().max(dy.abs()))
        .max()
        .unwrap_or(1)
        .max(min_radius)
}

fn move_grid_preview_ui(
    ui: &mut egui::Ui,
    piece_idx: usize,
    moves: &[(i32, i32)],
    piece_color: Color,
    selected: bool,
) -> egui::Response {
    let radius = move_grid_radius(moves, 1);
    let cell_px = MOVE_PREVIEW_CELL_PX;
    let gap = 0.0;
    let rgb = piece_color.to_srgba();
    let attack_fill = egui::Color32::from_rgba_unmultiplied(
        (rgb.red * 255.0) as u8,
        (rgb.green * 255.0) as u8,
        (rgb.blue * 255.0) as u8,
        255,
    );
    let panel_bg = egui::Color32::from_rgb(112, 114, 122);
    let empty_fill = egui::Color32::from_rgb(126, 128, 136);
    let piece_fill = egui::Color32::from_rgb(162, 164, 172);
    let cell_outline = egui::Stroke::new(1.0, egui::Color32::from_rgb(58, 60, 68));
    let piece_label = egui::Color32::from_rgb(32, 34, 40);

    ui.push_id(("move_grid_preview", piece_idx), |ui| {
        let side = (2 * radius as usize + 1) as f32;
        let grid_side = side * cell_px + (side - 1.0).max(0.0) * gap;
        let outer_side = grid_side + 2.0 * MOVE_PREVIEW_PAD_PX;
        let (outer_rect, response) =
            ui.allocate_exact_size(egui::vec2(outer_side, outer_side), egui::Sense::click());

        let painter = ui.painter();
        painter.rect_filled(outer_rect, 5.0, panel_bg);
        if selected {
            painter.rect_stroke(
                outer_rect,
                5.0,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(72, 118, 196)),
                egui::StrokeKind::Outside,
            );
        }
        let grid_origin = outer_rect.min + egui::vec2(MOVE_PREVIEW_PAD_PX, MOVE_PREVIEW_PAD_PX);

        for y in (-radius..=radius).rev() {
            for x in -radius..=radius {
                let col = (x + radius) as f32;
                let row = (radius - y) as f32;
                let min = egui::pos2(
                    grid_origin.x + col * (cell_px + gap),
                    grid_origin.y + row * (cell_px + gap),
                );
                let cell_rect =
                    egui::Rect::from_min_size(min, egui::vec2(cell_px, cell_px));

                let attack = moves.iter().any(|&m| m == (x, y));
                let fill = if x == 0 && y == 0 {
                    piece_fill
                } else if attack {
                    attack_fill
                } else {
                    empty_fill
                };
                painter.rect_filled(cell_rect, 1.5, fill);
                painter.rect_stroke(
                    cell_rect,
                    1.5,
                    cell_outline,
                    egui::StrokeKind::Inside,
                );
                if x == 0 && y == 0 {
                    painter.text(
                        cell_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "P",
                        egui::FontId::proportional(6.0),
                        piece_label,
                    );
                }
            }
        }
        response
    })
    .inner
}

fn set_attack_square(moves: &mut Vec<(i32, i32)>, x: i32, y: i32, on: bool) {
    if on {
        if !moves.iter().any(|&m| m == (x, y)) {
            moves.push((x, y));
        }
    } else {
        moves.retain(|&m| m != (x, y));
    }
    moves.sort_by_key(|&(x, y)| (x, y));
    moves.dedup();
}

fn attack_square_on(pieces: &[Piece], piece_idx: usize, x: i32, y: i32) -> bool {
    pieces[piece_idx]
        .piece
        .valid_moves
        .iter()
        .any(|&m| m == (x, y))
}

fn apply_attack_square(
    pieces: &mut [Piece],
    piece_idx: usize,
    x: i32,
    y: i32,
    on: bool,
    sync_all: bool,
) -> bool {
    if attack_square_on(pieces, piece_idx, x, y) == on {
        return false;
    }
    if sync_all {
        for piece in pieces.iter_mut() {
            set_attack_square(&mut piece.piece.valid_moves, x, y, on);
        }
    } else {
        set_attack_square(&mut pieces[piece_idx].piece.valid_moves, x, y, on);
    }
    true
}

fn clear_attack_squares(pieces: &mut [Piece], piece_idx: usize, sync_all: bool) {
    if sync_all {
        for piece in pieces.iter_mut() {
            piece.piece.valid_moves.clear();
        }
    } else {
        pieces[piece_idx].piece.valid_moves.clear();
    }
}

fn move_grid_ui(
    ui: &mut egui::Ui,
    piece_idx: usize,
    pieces: &mut [Piece],
    sync_all: bool,
) -> bool {
    let radius = move_grid_radius(&pieces[piece_idx].piece.valid_moves, ATTACK_GRID_MIN_RADIUS);
    let viewport = attack_grid_viewport_side_px();
    let mut changed = false;

    ui.push_id(("move_grid", piece_idx), |ui| {
        egui::ScrollArea::both()
            .id_salt("viewport")
            .min_scrolled_width(viewport)
            .min_scrolled_height(viewport)
            .max_width(viewport)
            .max_height(viewport)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                changed = attack_grid_editor_ui(ui, piece_idx, pieces, sync_all, radius);
            });
    });

    changed
}

fn attack_grid_editor_ui(
    ui: &mut egui::Ui,
    piece_idx: usize,
    pieces: &mut [Piece],
    sync_all: bool,
    radius: i32,
) -> bool {
    let cell_size = egui::Vec2::splat(ATTACK_GRID_CELL_PX);
    let mut changed = false;

    egui::Grid::new("cells")
        .min_col_width(cell_size.x)
        .min_row_height(cell_size.y)
        .spacing(egui::Vec2::splat(ATTACK_GRID_CELL_GAP))
        .show(ui, |ui| {
            for y in (-radius..=radius).rev() {
                for x in -radius..=radius {
                    if x == 0 && y == 0 {
                        ui.add_enabled(false, egui::Button::new("P").min_size(cell_size));
                        continue;
                    }

                    let selected = attack_square_on(pieces, piece_idx, x, y);
                    let label = if selected { "x" } else { "" };
                    let mut button = egui::Button::new(label).min_size(cell_size);
                    if selected {
                        button = button.fill(egui::Color32::from_rgb(90, 40, 40));
                    }

                    if ui
                        .add(button)
                        .on_hover_text(format!("({x}, {y})"))
                        .clicked()
                    {
                        if apply_attack_square(
                            pieces,
                            piece_idx,
                            x,
                            y,
                            !selected,
                            sync_all,
                        ) {
                            changed = true;
                        }
                    }
                }
                ui.end_row();
            }
        });

    changed
}
