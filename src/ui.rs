use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::CELL_SIZE;
use crate::camera::PendingCameraAction;
use crate::model::{Army, GameDefinition, PieceDef};
use crate::mutate::{
    reflect_across_x_axis, reflect_across_y_axis, rotate_ccw, rotate_cw,
    shared_attack_extent_for_armies, shift_attacks, toggle_random_attack_square,
    toggle_random_blocked_by,
};
use crate::random_gen::{AttackSymmetry, RandomGenConfig, generate_random_game};
use crate::render::{RenderCache, grid_texture_size};
use crate::sim_worker::SimulationBridge;
use crate::viewport::ViewportState;

#[derive(Resource)]
pub struct UiState {
    pub draft: Option<GameDefinition>,
    /// When true, any draft change that affects the sim is applied automatically.
    pub auto_update: bool,
    pub random_gen: RandomGenConfig,
    /// Army index targeted by the Mutate section (when `mutate_all` is false).
    pub mutate_army: usize,
    pub mutate_all: bool,
    pub preset_index: usize,
    /// Army shown in the Armies editor dropdown.
    pub edit_army: usize,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            draft: None,
            auto_update: true,
            random_gen: RandomGenConfig::default(),
            mutate_army: 0,
            mutate_all: false,
            preset_index: 0,
            edit_army: 0,
        }
    }
}

fn apply_preset_index(index: usize) -> GameDefinition {
    let catalog = GameDefinition::preset_catalog();
    let i = index % catalog.len();
    (catalog[i].1)()
}

pub fn ui_game_definition(
    mut contexts: EguiContexts,
    mut def: ResMut<GameDefinition>,
    mut sim: ResMut<SimulationBridge>,
    mut ui_state: ResMut<UiState>,
    mut cache: ResMut<RenderCache>,
    mut viewport: ResMut<ViewportState>,
    mut camera_actions: ResMut<PendingCameraAction>,
    camera_q: Query<&Projection, With<Camera2d>>,
    window_q: Query<&Window>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let mut draft = ui_state
        .draft
        .take()
        .unwrap_or_else(|| def.as_ref().clone());
    let mut apply_clicked = false;
    let auto_update = ui_state.auto_update;

    let panel_response = egui::SidePanel::left("game_config")
        .default_width(320.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.heading("Red & Black Knights");
                    if ui.button("Center view").clicked() {
                        camera_actions.center_view = true;
                    }
                    ui.checkbox(&mut ui_state.auto_update, "Auto update simulation");
                    ui.separator();

                    ui.collapsing("Presets", |ui| {
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
                        ui.horizontal(|ui| {
                            if ui.button("◀").on_hover_text("Previous preset").clicked() {
                                load_preset(idx + n - 1);
                            }
                            egui::ComboBox::from_id_salt("preset_pick")
                                .selected_text(catalog[idx].0)
                                .width(ui.available_width() - 56.0)
                                .show_ui(ui, |ui| {
                                    for (i, (label, _)) in catalog.iter().enumerate() {
                                        if ui.selectable_label(idx == i, *label).clicked() {
                                            load_preset(i);
                                        }
                                    }
                                });
                            if ui.button("▶").on_hover_text("Next preset").clicked() {
                                load_preset(idx + 1);
                            }
                        });
                    });

                    ui.collapsing("Random generator", |ui| {
                        let rg = &mut ui_state.random_gen;
                        ui.label("Army count range");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut rg.army_count_min)
                                    .range(1..=32)
                                    .prefix("min: "),
                            );
                            ui.add(
                                egui::DragValue::new(&mut rg.army_count_max)
                                    .range(1..=32)
                                    .prefix("max: "),
                            );
                        });

                        ui.label("Attack pattern (from piece center)");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut rg.attack_radius_min)
                                    .range(1..=12)
                                    .prefix("radius min: "),
                            );
                            ui.add(
                                egui::DragValue::new(&mut rg.attack_radius_max)
                                    .range(1..=12)
                                    .prefix("radius max: "),
                            );
                        });
                        ui.add(
                            egui::Slider::new(&mut rg.pattern_density, 0.0..=1.0)
                                .text("pattern density"),
                        );
                        ui.label("Attack symmetry");
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
                        ui.add(
                            egui::Slider::new(&mut rg.blocked_by_density, 0.0..=1.0)
                                .text("blocked-by density"),
                        );

                        if ui.button("Generate random armies").clicked() {
                            rg.sanitize();
                            let mut rng = rand::rng();
                            draft = generate_random_game(rg, &mut rng);
                        }
                    });

                    ui.separator();

                    ui.collapsing("Army summary", |ui| {
                        if draft.armies.is_empty() {
                            ui.label("No armies");
                        } else {
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
                                        for (army_idx, army) in draft.armies.iter().enumerate() {
                                            let blocked: Vec<_> = army
                                                .blocked_by
                                                .iter()
                                                .filter_map(|&id| {
                                                    draft.armies.get(id).map(|a| a.name.as_str())
                                                })
                                                .collect();
                                            let blocked_line = if blocked.is_empty() {
                                                "blocked by: —".to_string()
                                            } else {
                                                format!("blocked by: {}", blocked.join(", "))
                                            };
                                            let hover = format!(
                                                "{army_idx}: {}\n{blocked_line}",
                                                army.name
                                            );

                                            move_grid_preview_ui(
                                                ui,
                                                army_idx,
                                                &army.piece.valid_moves,
                                                army.color,
                                            )
                                            .on_hover_text(hover);
                                        }
                                    });
                                });
                        }
                    });

                    ui.collapsing("Mutate", |ui| {
                        if draft.armies.is_empty() {
                            ui.label("No armies");
                        } else {
                            if !ui_state.mutate_all
                                && ui_state.mutate_army >= draft.armies.len()
                            {
                                ui_state.mutate_army = draft.armies.len() - 1;
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

                            if ui.button("Toggle random attack square").clicked() {
                                for &aid in &targets {
                                    toggle_random_attack_square(
                                        &mut draft.armies[aid].piece.valid_moves,
                                        &mut rng,
                                    );
                                }
                            }
                            ui.horizontal(|ui| {
                                if ui.button("Shift +X").clicked() {
                                    for &aid in &targets {
                                        shift_attacks(
                                            &mut draft.armies[aid].piece.valid_moves,
                                            1,
                                            0,
                                            shared_r,
                                        );
                                    }
                                }
                                if ui.button("Shift −X").clicked() {
                                    for &aid in &targets {
                                        shift_attacks(
                                            &mut draft.armies[aid].piece.valid_moves,
                                            -1,
                                            0,
                                            shared_r,
                                        );
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                if ui.button("Shift +Y").clicked() {
                                    for &aid in &targets {
                                        shift_attacks(
                                            &mut draft.armies[aid].piece.valid_moves,
                                            0,
                                            1,
                                            shared_r,
                                        );
                                    }
                                }
                                if ui.button("Shift −Y").clicked() {
                                    for &aid in &targets {
                                        shift_attacks(
                                            &mut draft.armies[aid].piece.valid_moves,
                                            0,
                                            -1,
                                            shared_r,
                                        );
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                if ui.button("Flip Y").clicked() {
                                    for &aid in &targets {
                                        reflect_across_x_axis(
                                            &mut draft.armies[aid].piece.valid_moves,
                                        );
                                    }
                                }
                                if ui.button("Flip X").clicked() {
                                    for &aid in &targets {
                                        reflect_across_y_axis(
                                            &mut draft.armies[aid].piece.valid_moves,
                                        );
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                if ui.button("Rotate ↻").clicked() {
                                    for &aid in &targets {
                                        rotate_cw(&mut draft.armies[aid].piece.valid_moves);
                                    }
                                }
                                if ui.button("Rotate ↺").clicked() {
                                    for &aid in &targets {
                                        rotate_ccw(&mut draft.armies[aid].piece.valid_moves);
                                    }
                                }
                            });

                            if ui.button("Toggle random blocked-by").clicked() {
                                for &aid in &targets {
                                    toggle_random_blocked_by(
                                        &mut draft.armies[aid],
                                        aid,
                                        army_count,
                                        &mut rng,
                                    );
                                }
                            }
                        }
                    });

                    ui.collapsing("Armies", |ui| {
                        if draft.armies.is_empty() {
                            ui.label("No armies");
                        } else {
                            if ui_state.edit_army >= draft.armies.len() {
                                ui_state.edit_army = draft.armies.len() - 1;
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
                            ui.small("Click cells to toggle moves relative to the piece.");
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
                                        if !draft.armies[army_idx].blocked_by.contains(&other)
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

                            ui.horizontal(|ui| {
                                if ui.button("Add army").clicked() {
                                    let id = draft.armies.len();
                                    draft.armies.push(Army {
                                        name: format!("Army {id}"),
                                        color: Color::srgb(0.5, 0.5, 0.5),
                                        piece: PieceDef::knight(),
                                        blocked_by: vec![],
                                    });
                                    draft.turn_order.push(id);
                                    ui_state.edit_army = id;
                                }
                                if draft.armies.len() > 1
                                    && ui.button("Remove army").clicked()
                                {
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
                                    if ui_state.edit_army >= draft.armies.len() {
                                        ui_state.edit_army = draft.armies.len().saturating_sub(1);
                                    }
                                }
                            });
                        }
                    });

                    if draft.turn_order.is_empty() && !draft.armies.is_empty() {
                        draft.turn_order = (0..draft.armies.len()).collect();
                    }

                    ui.separator();
                    let draft_pending = !draft.same_applied_state(def.as_ref());
                    if draft_pending && !ui_state.auto_update {
                        ui.colored_label(
                            egui::Color32::YELLOW,
                            "Config changed — apply to reset sim",
                        );
                    }
                    if ui
                        .add_enabled(
                            draft_pending,
                            egui::Button::new("Apply & reset simulation"),
                        )
                        .clicked()
                    {
                        apply_clicked = true;
                    }

                    let min_cursor = sim
                        .display
                        .cursors
                        .iter()
                        .copied()
                        .min()
                        .unwrap_or(0);
                    ui.separator();
                    ui.label(format!("Placements: {}", sim.display.placements_len));
                    ui.label(format!("Min cursor: {min_cursor}"));
                    if viewport.simulation_pending || sim.is_busy() {
                        ui.label(format!("Simulating to index {}", viewport.target_index));
                    }
                    ui.separator();
                    ui.label("Debug");
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

                    if let (Ok(Projection::Orthographic(ortho)), Ok(window)) =
                        (camera_q.single(), window_q.single())
                    {
                        let world_per_screen_px =
                            (ortho.area.width() * ortho.scale) / window.width().max(1.0);
                        let cells_per_screen_px = world_per_screen_px / CELL_SIZE;
                        let board_width_px =
                            (window.width() - viewport.left_inset_px).ceil().max(1.0);
                        let board_pixels =
                            board_width_px as u64 * window.height().ceil().max(1.0) as u64;
                        ui.label(format!("Zoom scale: {:.3}", ortho.scale));
                        ui.label(format!("Cells per px: {:.3}", cells_per_screen_px));
                        ui.label(format!("Board pixels: {board_pixels}"));
                        ui.label(format!("Left inset px: {:.0}", viewport.left_inset_px));
                    }
                });
        });
    viewport.left_inset_px = panel_response.response.rect.width();

    let should_apply =
        apply_clicked || (auto_update && !draft.same_applied_state(def.as_ref()));
    if should_apply {
        dedupe_moves(&mut draft);
        *def = draft.clone();
        sim.request_reset(def.clone());
        cache.rendered_bounds = None;
        viewport.bounds = None;
        viewport.target_index = 0;
        viewport.allow_sim_catchup_immediately();
        viewport.simulation_pending = false;
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

fn summary_preview_side_px(moves: &[(i32, i32)]) -> f32 {
    let radius = move_grid_radius(moves, 1);
    let cell_px = 8.0;
    let side = (2 * radius as usize + 1) as f32;
    side * cell_px
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
    army_idx: usize,
    moves: &[(i32, i32)],
    army_color: Color,
) -> egui::Response {
    let radius = move_grid_radius(moves, 1);
    let cell_px = 8.0;
    let gap = 0.0;
    let rgb = army_color.to_srgba();
    let attack_fill = egui::Color32::from_rgba_premultiplied(
        (rgb.red * 255.0) as u8,
        (rgb.green * 255.0) as u8,
        (rgb.blue * 255.0) as u8,
        220,
    );
    let empty_fill = egui::Color32::from_gray(28);
    let piece_fill = egui::Color32::from_gray(45);

    ui.push_id(("move_grid_preview", army_idx), |ui| {
        let side = (2 * radius as usize + 1) as f32;
        let grid_side = side * cell_px + (side - 1.0).max(0.0) * gap;
        let (grid_rect, response) =
            ui.allocate_exact_size(egui::vec2(grid_side, grid_side), egui::Sense::hover());

        for y in (-radius..=radius).rev() {
            for x in -radius..=radius {
                let col = (x + radius) as f32;
                let row = (radius - y) as f32;
                let min = egui::pos2(
                    grid_rect.min.x + col * (cell_px + gap),
                    grid_rect.min.y + row * (cell_px + gap),
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
                ui.painter().rect_filled(cell_rect, 1.0, fill);
                if x == 0 && y == 0 {
                    ui.painter().text(
                        cell_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "P",
                        egui::FontId::proportional(6.0),
                        egui::Color32::LIGHT_GRAY,
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
