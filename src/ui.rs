use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use serde::{Deserialize, Serialize};

use crate::bookmark_config::{Bookmark, BookmarkStore};
use crate::camera::{BoardCamera, PanCamera, PendingCameraAction};
use crate::camera_config::CameraSessionConfig;
use crate::calibration_config;
use crate::CELL_SIZE;
use crate::model::{GameDefinition, PieceDef};
use crate::mutate::{
    reflect_across_x_axis, reflect_across_y_axis, rotate_ccw, rotate_cw,
    shared_attack_extent_for_armies, shift_attacks, toggle_random_attack_square,
    toggle_random_blocked_by,
};
use crate::random_gen::{AttackSymmetry, RandomGenConfig, generate_random_game};
use crate::render::{RenderCache, grid_texture_size};
use crate::sim_worker::SimulationBridge;
use crate::viewport::{self, ViewportState};

/// How occupied spiral cells are coloured on the board texture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoardColourMode {
    #[default]
    Army,
    ScanSkips,
}

/// Open/closed state for sidebar [`egui::CollapsingHeader`] sections (persisted in app session).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SidebarSections {
    #[serde(default)]
    pub view: bool,
    #[serde(default)]
    pub colouring: bool,
    #[serde(default)]
    pub presets: bool,
    #[serde(default)]
    pub bookmarks: bool,
    #[serde(default)]
    pub random_generator: bool,
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
    /// Piece index targeted by the Mutate section (when `mutate_all` is false).
    pub mutate_army: usize,
    pub mutate_all: bool,
    pub preset_index: usize,
    /// Piece shown in the nested Advanced editor under Pieces.
    pub edit_army: usize,
    /// Name for the next bookmark (sidebar Bookmarks section).
    pub bookmark_new_name: String,
    /// Selected entry in [`PieceDef::piece_catalog`] for roster add.
    pub add_piece_preset_index: usize,
    /// Army index targeted by roster remove control.
    pub roster_remove_army: usize,
    /// Colour applied when adding a piece from Edit roster.
    pub add_piece_color: Color,
    pub sidebar: SidebarSections,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            draft: None,
            board_colour_mode: BoardColourMode::default(),
            random_gen: RandomGenConfig::default(),
            mutate_army: 0,
            mutate_all: false,
            preset_index: 0,
            edit_army: 0,
            bookmark_new_name: String::new(),
            add_piece_preset_index: 0,
            roster_remove_army: 0,
            add_piece_color: GameDefinition::default_army_color(0),
            sidebar: SidebarSections::default(),
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

pub fn ui_game_definition(
    mut contexts: EguiContexts,
    mut def: ResMut<GameDefinition>,
    mut sim: ResMut<SimulationBridge>,
    mut ui_state: ResMut<UiState>,
    mut bookmarks: ResMut<BookmarkStore>,
    mut cache: ResMut<RenderCache>,
    mut viewport: ResMut<ViewportState>,
    mut camera_actions: ResMut<PendingCameraAction>,
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
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.heading("Red & Black Knights");
                    ui_state.sidebar.view = sidebar_collapsing(ui, "view", "View", ui_state.sidebar.view, false, |ui| {
                        if ui.button("Center view").clicked() {
                            camera_actions.center_view = true;
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
                    ui_state.sidebar.colouring = sidebar_collapsing(
                        ui,
                        "colouring",
                        "Colouring",
                        ui_state.sidebar.colouring,
                        false,
                        |ui| {
                            let prev = ui_state.board_colour_mode;
                            ui.vertical(|ui| {
                                ui.radio_value(
                                    &mut ui_state.board_colour_mode,
                                    BoardColourMode::Army,
                                    "Piece colour",
                                );
                                ui.radio_value(
                                    &mut ui_state.board_colour_mode,
                                    BoardColourMode::ScanSkips,
                                    "Scan skips",
                                );
                            });
                            if ui_state.board_colour_mode == BoardColourMode::ScanSkips {
                                ui.add_space(4.0);
                                ui.label(
                                    "Heat by cells rejected along the spiral before each placement.",
                                );
                            }
                            if ui_state.board_colour_mode != prev {
                                viewport.render_dirty = true;
                                cache.rendered_bounds = None;
                            }
                        },
                    );
                    ui_state.sidebar.bookmarks = sidebar_collapsing(
                        ui,
                        "bookmarks",
                        "Bookmarks",
                        ui_state.sidebar.bookmarks,
                        false,
                        |ui| {
                        let n = bookmarks.bookmarks.len();
                        if let Some(sel) = bookmarks.selected {
                            if sel >= n {
                                bookmarks.selected = None;
                            }
                        }
                        let selected = bookmarks.selected;

                        if n > 0 {
                            let idx = selected.unwrap_or(0);
                            let label = bookmarks.bookmarks[idx].name.as_str();
                            egui::ComboBox::from_id_salt("bookmark_pick")
                                .selected_text(label)
                                .show_ui(ui, |ui| {
                                    let mut pick: Option<usize> = None;
                                    for (i, bm) in bookmarks.bookmarks.iter().enumerate() {
                                        if ui
                                            .selectable_label(selected == Some(i), &bm.name)
                                            .clicked()
                                        {
                                            pick = Some(i);
                                        }
                                    }
                                    if let Some(i) = pick {
                                        bookmarks.selected = Some(i);
                                        let bm = bookmarks.bookmarks[i].clone();
                                        apply_bookmark(
                                            &bm,
                                            &mut draft,
                                            &mut ui_state,
                                            &mut viewport,
                                            &mut camera_q,
                                        );
                                    }
                                });
                            ui.add_space(4.0);
                        }

                        ui.text_edit_singleline(&mut ui_state.bookmark_new_name);

                        ui.horizontal(|ui| {
                            let can_delete = selected.is_some() && n > 0;
                            if ui
                                .add_enabled(can_delete, egui::Button::new("Delete bookmark"))
                                .clicked()
                            {
                                bookmarks.remove_selected();
                            }
                            if ui.button("Add bookmark").clicked() {
                                let name = if ui_state.bookmark_new_name.trim().is_empty() {
                                    format!("Bookmark {}", n + 1)
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
                    });

                    ui_state.sidebar.random_generator = sidebar_collapsing(
                        ui,
                        "random_generator",
                        "Random generator",
                        ui_state.sidebar.random_generator,
                        false,
                        |ui| {
                        let rg = &mut ui_state.random_gen;
                        ui.scope(|ui| {
                            sidebar_u32_range(
                                ui,
                                "Piece count",
                                &mut rg.army_count_min,
                                &mut rg.army_count_max,
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
                            ui.add_space(SIDEBAR_FIELD_GAP);

                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(6.0);
                            if ui.button("Generate random pieces").clicked() {
                                rg.sanitize();
                                let mut rng = rand::rng();
                                draft = generate_random_game(rg, &mut rng);
                            }
                        });
                    });

                    ui_state.sidebar.mutate = sidebar_collapsing(
                        ui,
                        "mutate",
                        "Mutate",
                        ui_state.sidebar.mutate,
                        false,
                        |ui| {
                        if draft.armies.is_empty() {
                            ui.label("No pieces");
                        } else {
                            if !ui_state.mutate_all
                                && ui_state.mutate_army >= draft.armies.len()
                            {
                                ui_state.mutate_army = draft.armies.len().saturating_sub(1);
                            }
                            let selected = if ui_state.mutate_all {
                                "All".to_string()
                            } else {
                                let a = ui_state.mutate_army;
                                format!("{}: {}", a, draft.armies[a].name)
                            };
                            egui::ComboBox::from_id_salt("mutate_army_pick")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(ui_state.mutate_all, "All")
                                        .clicked()
                                    {
                                        ui_state.mutate_all = true;
                                    }
                                    for (aid, army) in draft.armies.iter().enumerate() {
                                        let picked = !ui_state.mutate_all
                                            && ui_state.mutate_army == aid;
                                        if ui
                                            .selectable_label(
                                                picked,
                                                format!("{aid}: {}", army.name),
                                            )
                                            .clicked()
                                        {
                                            ui_state.mutate_all = false;
                                            ui_state.mutate_army = aid;
                                        }
                                    }
                                });

                            let targets: Vec<usize> = if ui_state.mutate_all {
                                (0..draft.armies.len()).collect()
                            } else {
                                vec![ui_state.mutate_army]
                            };

                            let mut rng = rand::rng();
                            let army_count = draft.armies.len();
                            let shared_r = if ui_state.mutate_all && targets.len() > 1 {
                                Some(shared_attack_extent_for_armies(&draft.armies, &targets))
                            } else {
                                None
                            };

                            ui.add_space(4.0);
                            ui.scope(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 5.0);

                                if mutate_panel_button(ui, "Toggle attack square") {
                                        for &aid in &targets {
                                            toggle_random_attack_square(
                                                &mut draft.armies[aid].piece.valid_moves,
                                                &mut rng,
                                            );
                                        }
                                    }

                                    let (shift_px, shift_mx) =
                                        mutate_panel_pair(ui, "+X", "−X");
                                    if shift_px {
                                        for &aid in &targets {
                                            shift_attacks(
                                                &mut draft.armies[aid].piece.valid_moves,
                                                1,
                                                0,
                                                shared_r,
                                            );
                                        }
                                    }
                                    if shift_mx {
                                        for &aid in &targets {
                                            shift_attacks(
                                                &mut draft.armies[aid].piece.valid_moves,
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
                                                &mut draft.armies[aid].piece.valid_moves,
                                                0,
                                                1,
                                                shared_r,
                                            );
                                        }
                                    }
                                    if shift_my {
                                        for &aid in &targets {
                                            shift_attacks(
                                                &mut draft.armies[aid].piece.valid_moves,
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
                                                &mut draft.armies[aid].piece.valid_moves,
                                            );
                                        }
                                    }
                                    if flip_x {
                                        for &aid in &targets {
                                            reflect_across_y_axis(
                                                &mut draft.armies[aid].piece.valid_moves,
                                            );
                                        }
                                    }

                                    let (rot_ccw, rot_cw) =
                                        mutate_panel_pair(ui, "↺ CCW", "↻ CW");
                                    if rot_ccw {
                                        for &aid in &targets {
                                            rotate_ccw(
                                                &mut draft.armies[aid].piece.valid_moves,
                                            );
                                        }
                                    }
                                    if rot_cw {
                                        for &aid in &targets {
                                            rotate_cw(
                                                &mut draft.armies[aid].piece.valid_moves,
                                            );
                                        }
                                    }

                                    if mutate_panel_button(ui, "Toggle blocked-by") {
                                        for &aid in &targets {
                                            toggle_random_blocked_by(
                                                &mut draft.armies[aid],
                                                aid,
                                                army_count,
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
                        ui_state.sidebar.presets = sidebar_collapsing(
                            ui,
                            "pieces_presets",
                            "Presets",
                            ui_state.sidebar.presets,
                            false,
                            |ui| {
                                let catalog = GameDefinition::preset_catalog();
                                let n = catalog.len().max(1);
                                if ui_state.preset_index >= n {
                                    ui_state.preset_index = 0;
                                }
                                let idx = ui_state.preset_index;
                                let mut load_preset = |new_idx: usize| {
                                    ui_state.preset_index = new_idx % n;
                                    draft = apply_preset_index(ui_state.preset_index);
                                };
                                egui::ComboBox::from_id_salt("preset_pick")
                                    .selected_text(catalog[idx].0)
                                    .show_ui(ui, |ui| {
                                        for (i, (label, _)) in catalog.iter().enumerate() {
                                            if ui
                                                .selectable_label(idx == i, *label)
                                                .clicked()
                                            {
                                                load_preset(i);
                                            }
                                        }
                                    });
                                ui.horizontal(|ui| {
                                    if ui.button("◀ Previous").clicked() {
                                        load_preset(idx + n - 1);
                                    }
                                    if ui.button("Next ▶").clicked() {
                                        load_preset(idx + 1);
                                    }
                                });
                            },
                        );

                        let mut open_advanced = false;
                        ui_state.sidebar.pieces_summary = sidebar_collapsing(
                            ui,
                            "pieces_summary",
                            "Summary",
                            ui_state.sidebar.pieces_summary,
                            false,
                            |ui| {
                                if draft.armies.is_empty() {
                                    ui.label("No pieces in set");
                                } else {
                                    let advanced_was_open = ui_state.sidebar.pieces_advanced;
                                    let row_h = draft
                                        .armies
                                        .iter()
                                        .map(|a| summary_preview_side_px(&a.piece.valid_moves))
                                        .fold(0.0_f32, f32::max);
                                    egui::ScrollArea::horizontal()
                                        .min_scrolled_height(0.0)
                                        .max_height(row_h)
                                        .show(ui, |ui| {
                                            ui.horizontal_top(|ui| {
                                                ui.spacing_mut().item_spacing =
                                                    egui::vec2(2.0, 0.0);
                                                for (army_idx, army) in
                                                    draft.armies.iter().enumerate()
                                                {
                                                    let blocked: Vec<_> = army
                                                        .blocked_by
                                                        .iter()
                                                        .filter_map(|&id| {
                                                            draft
                                                                .armies
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
                                                        "{army_idx}: {}\n{blocked_line}\nClick to edit",
                                                        army.name
                                                    );
                                                    let selected = army_idx
                                                        == ui_state.edit_army
                                                        && (advanced_was_open
                                                            || open_advanced);

                                                    let preview = move_grid_preview_ui(
                                                        ui,
                                                        army_idx,
                                                        &army.piece.valid_moves,
                                                        army.color,
                                                        selected,
                                                    )
                                                    .on_hover_text(hover);

                                                    if preview.clicked() {
                                                        ui_state.edit_army = army_idx;
                                                        open_advanced = true;
                                                    }
                                                }
                                            });
                                        });
                                }
                            },
                        );

                        pieces_roster_editor_ui(ui, &mut draft, &mut ui_state);

                        if !draft.armies.is_empty() {
                            ui_state.sidebar.pieces_advanced = sidebar_collapsing(
                                ui,
                                "pieces_advanced",
                                "Advanced",
                                ui_state.sidebar.pieces_advanced,
                                open_advanced,
                                |ui| {
                                if ui_state.edit_army >= draft.armies.len() {
                                    ui_state.edit_army =
                                        draft.armies.len().saturating_sub(1);
                                }
                                let army_idx = ui_state.edit_army;
                                egui::ComboBox::from_id_salt("army_edit_pick")
                                    .selected_text(format!(
                                        "{}: {}",
                                        army_idx, draft.armies[army_idx].name
                                    ))
                                    .show_ui(ui, |ui| {
                                        for (aid, army) in draft.armies.iter().enumerate() {
                                            if ui
                                                .selectable_label(
                                                    army_idx == aid,
                                                    format!("{aid}: {}", army.name),
                                                )
                                                .clicked()
                                            {
                                                ui_state.edit_army = aid;
                                            }
                                        }
                                    });

                                ui.horizontal(|ui| {
                                    ui.label("Name");
                                    ui.text_edit_singleline(&mut draft.armies[army_idx].name);
                                });

                                let rgb = draft.armies[army_idx].color.to_srgba();
                                let mut arr = [rgb.red, rgb.green, rgb.blue];
                                if ui.color_edit_button_rgb(&mut arr).changed() {
                                    draft.armies[army_idx].color =
                                        Color::srgb(arr[0], arr[1], arr[2]);
                                }

                                ui.label("Attacked squares");
                                move_grid_ui(
                                    ui,
                                    army_idx,
                                    &mut draft.armies[army_idx].piece.valid_moves,
                                );

                                ui.label("Blocked by");
                                for other in 0..draft.armies.len() {
                                    if other == army_idx {
                                        continue;
                                    }
                                    let mut blocked =
                                        draft.armies[army_idx].blocked_by.contains(&other);
                                    let label = draft.armies[other].name.clone();
                                    if ui.checkbox(&mut blocked, label).changed() {
                                        if blocked {
                                            if !draft.armies[army_idx]
                                                .blocked_by
                                                .contains(&other)
                                            {
                                                draft.armies[army_idx].blocked_by.push(other);
                                            }
                                        } else {
                                            draft.armies[army_idx]
                                                .blocked_by
                                                .retain(|&id| id != other);
                                        }
                                    }
                                }

                                if ui.button("Remove piece").clicked() {
                                    remove_draft_army(
                                        &mut draft,
                                        &mut ui_state,
                                        army_idx,
                                    );
                                }
                            },
                            );
                        }
                    });

                    if draft.turn_order.is_empty() && !draft.armies.is_empty() {
                        draft.turn_order = (0..draft.armies.len()).collect();
                    }

                    ui.separator();
                    ui_state.sidebar.debug = sidebar_collapsing(ui, "debug", "Debug", ui_state.sidebar.debug, false, |ui| {
                    if let Some(bounds) = viewport.bounds {
                        let grid_size = grid_texture_size(bounds);
                        ui.label(format!("Grid cells: {} x {}", grid_size.x, grid_size.y));
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
                        let board_width_px =
                            (window.width() - viewport.left_inset_px).ceil().max(1.0);
                        let world_per_screen_px =
                            ortho.area.width() / board_width_px.max(1.0);
                        let cells_per_screen_px = world_per_screen_px / CELL_SIZE;
                        let board_pixels =
                            board_width_px as u64 * window.height().ceil().max(1.0) as u64;
                        ui.label(format!("Zoom scale: {:.3}", ortho.scale));
                        ui.label(format!("Cells per px: {:.3}", cells_per_screen_px));
                        ui.label(format!("Board pixels: {board_pixels}"));
                        ui.label(format!("Left inset px: {:.0}", viewport.left_inset_px));
                    }
                    });
                });
        });
    viewport.left_inset_px = panel_response.response.rect.width();

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
    for army in &mut def.armies {
        army.piece.valid_moves.sort_by_key(|&(x, y)| (x, y));
        army.piece.valid_moves.dedup();
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

fn remove_draft_army(draft: &mut GameDefinition, ui_state: &mut UiState, army_idx: usize) {
    if army_idx >= draft.armies.len() {
        return;
    }
    draft.armies.remove(army_idx);
    for army in &mut draft.armies {
        army.blocked_by.retain(|&id| id != army_idx);
        for b in &mut army.blocked_by {
            if *b > army_idx {
                *b -= 1;
            }
        }
    }
    draft.turn_order.retain(|&id| id != army_idx);
    for t in &mut draft.turn_order {
        if *t > army_idx {
            *t -= 1;
        }
    }
    if draft.armies.is_empty() {
        draft.turn_order.clear();
    }
    if ui_state.edit_army >= draft.armies.len() {
        ui_state.edit_army = draft.armies.len().saturating_sub(1);
    }
    if !ui_state.mutate_all && ui_state.mutate_army >= draft.armies.len() {
        ui_state.mutate_army = draft.armies.len().saturating_sub(1);
    }
    ui_state.roster_remove_army = ui_state
        .roster_remove_army
        .min(draft.armies.len().saturating_sub(1));
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
                    draft.push_army_from_piece_preset(preset_label, preview_piece, color);
                    ui_state.add_piece_color =
                        GameDefinition::default_army_color(draft.armies.len());
                    ui_state.edit_army = draft.armies.len().saturating_sub(1);
                    ui_state.roster_remove_army = ui_state.edit_army;
                    if !ui_state.mutate_all && ui_state.mutate_army >= draft.armies.len() {
                        ui_state.mutate_army = draft.armies.len().saturating_sub(1);
                    }
                }
            });

            if !draft.armies.is_empty() {
                ui.add_space(12.0);

                ui.vertical(|ui| {
                    ui.label("Remove");
                    if ui_state.roster_remove_army >= draft.armies.len() {
                        ui_state.roster_remove_army = draft.armies.len().saturating_sub(1);
                    }
                    let remove_idx = ui_state.roster_remove_army;
                    egui::ComboBox::from_id_salt("roster_remove_pick")
                        .selected_text(format!(
                            "{}: {}",
                            remove_idx, draft.armies[remove_idx].name
                        ))
                        .show_ui(ui, |ui| {
                            for (aid, army) in draft.armies.iter().enumerate() {
                                if ui
                                    .selectable_label(
                                        remove_idx == aid,
                                        format!("{aid}: {}", army.name),
                                    )
                                    .clicked()
                                {
                                    ui_state.roster_remove_army = aid;
                                }
                            }
                        });
                    let can_remove = !draft.armies.is_empty();
                    if ui
                        .add_enabled(can_remove, egui::Button::new("Remove"))
                        .clicked()
                    {
                        remove_draft_army(draft, ui_state, remove_idx);
                        ui_state.roster_remove_army = ui_state
                            .edit_army
                            .min(draft.armies.len().saturating_sub(1));
                    }
                });
            }
        },
    );
}

fn summary_preview_side_px(moves: &[(i32, i32)]) -> f32 {
    let radius = move_grid_radius(moves, 1);
    let cell_px = MOVE_PREVIEW_CELL_PX;
    let side = (2 * radius as usize + 1) as f32;
    side * cell_px + 2.0 * MOVE_PREVIEW_PAD_PX
}

const MOVE_PREVIEW_CELL_PX: f32 = 8.0;
const MOVE_PREVIEW_PAD_PX: f32 = 4.0;

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
    army_idx: usize,
    moves: &[(i32, i32)],
    army_color: Color,
    selected: bool,
) -> egui::Response {
    let radius = move_grid_radius(moves, 1);
    let cell_px = MOVE_PREVIEW_CELL_PX;
    let gap = 0.0;
    let rgb = army_color.to_srgba();
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

    ui.push_id(("move_grid_preview", army_idx), |ui| {
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

                let selected = moves.iter().any(|&m| m == (x, y));
                let fill = if x == 0 && y == 0 {
                    piece_fill
                } else if selected {
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

fn move_grid_ui(ui: &mut egui::Ui, army_idx: usize, moves: &mut Vec<(i32, i32)>) -> bool {
    let radius = move_grid_radius(moves, 4);
    let cell_size = egui::Vec2::splat(22.0);
    let mut changed = false;

    ui.push_id(("move_grid", army_idx), |ui| {
        egui::Grid::new("cells")
            .min_col_width(cell_size.x)
            .min_row_height(cell_size.y)
            .spacing(egui::Vec2::splat(1.0))
            .show(ui, |ui| {
                for y in (-radius..=radius).rev() {
                    for x in -radius..=radius {
                        if x == 0 && y == 0 {
                            ui.add_enabled(false, egui::Button::new("P").min_size(cell_size));
                            continue;
                        }

                        let move_idx = moves.iter().position(|&m| m == (x, y));
                        let selected = move_idx.is_some();
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
                            if let Some(idx) = move_idx {
                                moves.remove(idx);
                            } else {
                                moves.push((x, y));
                            }
                            changed = true;
                        }
                    }
                    ui.end_row();
                }
            });
    });

    if changed {
        moves.sort_by_key(|&(x, y)| (x, y));
        moves.dedup();
    }
    changed
}
