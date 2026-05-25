use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::CELL_SIZE;
use crate::camera::PendingCameraAction;
use crate::model::{Army, GameDefinition, PieceDef};
use crate::render::{RenderCache, grid_texture_size};
use crate::sim::Simulation;
use crate::viewport::ViewportState;

#[derive(Resource, Default)]
pub struct UiState {
    pub config_dirty: bool,
    pub draft: Option<GameDefinition>,
}

pub fn ui_game_definition(
    mut contexts: EguiContexts,
    mut def: ResMut<GameDefinition>,
    mut sim: ResMut<Simulation>,
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
    let mut config_dirty = ui_state.config_dirty;
    let mut apply_clicked = false;

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
                    ui.separator();

                    ui.collapsing("Presets", |ui| {
                        for (label, build) in GameDefinition::preset_catalog() {
                            if ui.button(*label).clicked() {
                                draft = build();
                                config_dirty = true;
                            }
                        }
                    });

                    ui.separator();
                    ui.label("Armies");

                    let mut remove_army: Option<usize> = None;
                    for army_idx in 0..draft.armies.len() {
                        ui.collapsing(
                            format!("Army {}: {}", army_idx, draft.armies[army_idx].name),
                            |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Name");
                                    if ui
                                        .text_edit_singleline(&mut draft.armies[army_idx].name)
                                        .changed()
                                    {
                                        config_dirty = true;
                                    }
                                });

                                let rgb = draft.armies[army_idx].color.to_srgba();
                                let mut arr = [rgb.red, rgb.green, rgb.blue];
                                if ui.color_edit_button_rgb(&mut arr).changed() {
                                    draft.armies[army_idx].color =
                                        Color::srgb(arr[0], arr[1], arr[2]);
                                    config_dirty = true;
                                }

                                ui.label("Attacked squares");
                                ui.small("Click cells to toggle moves relative to the piece.");
                                if move_grid_ui(
                                    ui,
                                    army_idx,
                                    &mut draft.armies[army_idx].piece.valid_moves,
                                ) {
                                    config_dirty = true;
                                }

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
                                            if !draft.armies[army_idx].blocked_by.contains(&other) {
                                                draft.armies[army_idx].blocked_by.push(other);
                                            }
                                        } else {
                                            draft.armies[army_idx]
                                                .blocked_by
                                                .retain(|&id| id != other);
                                        }
                                        config_dirty = true;
                                    }
                                }

                                if draft.armies.len() > 1 && ui.button("Remove army").clicked() {
                                    remove_army = Some(army_idx);
                                }
                            },
                        );
                    }

                    if let Some(idx) = remove_army {
                        draft.armies.remove(idx);
                        for army in &mut draft.armies {
                            army.blocked_by.retain(|&id| id != idx);
                            for b in &mut army.blocked_by {
                                if *b > idx {
                                    *b -= 1;
                                }
                            }
                        }
                        draft.turn_order.retain(|&id| id != idx);
                        for t in &mut draft.turn_order {
                            if *t > idx {
                                *t -= 1;
                            }
                        }
                        config_dirty = true;
                    }

                    if ui.button("Add army").clicked() {
                        let id = draft.armies.len();
                        draft.armies.push(Army {
                            name: format!("Army {id}"),
                            color: Color::srgb(0.5, 0.5, 0.5),
                            piece: PieceDef::knight(),
                            blocked_by: vec![],
                        });
                        draft.turn_order.push(id);
                        config_dirty = true;
                    }

                    ui.separator();
                    ui.label("Turn order (round-robin)");
                    let mut new_order = draft.turn_order.clone();
                    egui::ComboBox::from_id_salt("turn_order_editor")
                        .selected_text(format!("{} steps", new_order.len()))
                        .show_ui(ui, |ui| {
                            for step in 0..new_order.len() {
                                let current = new_order[step];
                                egui::ComboBox::from_id_salt(format!("turn_{step}"))
                                    .selected_text(
                                        draft
                                            .armies
                                            .get(current)
                                            .map(|a| a.name.as_str())
                                            .unwrap_or("?"),
                                    )
                                    .show_ui(ui, |ui| {
                                        for (aid, army) in draft.armies.iter().enumerate() {
                                            if ui
                                                .selectable_label(current == aid, &army.name)
                                                .clicked()
                                            {
                                                new_order[step] = aid;
                                                config_dirty = true;
                                            }
                                        }
                                    });
                            }
                        });
                    if new_order != draft.turn_order {
                        draft.turn_order = new_order;
                        config_dirty = true;
                    }

                    if draft.turn_order.is_empty() && !draft.armies.is_empty() {
                        draft.turn_order = (0..draft.armies.len()).collect();
                    }

                    ui.separator();
                    if config_dirty {
                        ui.colored_label(
                            egui::Color32::YELLOW,
                            "Config changed — apply to reset sim",
                        );
                    }
                    if ui.button("Apply & reset simulation").clicked() {
                        apply_clicked = true;
                    }

                    let min_cursor = sim.cursors.iter().copied().min().unwrap_or(0);
                    ui.separator();
                    ui.label(format!("Placements: {}", sim.placements.len()));
                    ui.label(format!("Min cursor: {min_cursor}"));
                    if viewport.simulation_pending {
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

    if apply_clicked {
        dedupe_moves(&mut draft);
        *def = draft.clone();
        sim.reset(&def);
        cache.rendered_bounds = None;
        viewport.bounds = None;
        viewport.target_index = 0;
        viewport.simulation_pending = false;
        viewport.render_dirty = true;
        config_dirty = false;
    }

    ui_state.config_dirty = config_dirty;
    ui_state.draft = Some(draft);
}

fn dedupe_moves(def: &mut GameDefinition) {
    for army in &mut def.armies {
        army.piece.valid_moves.sort_by_key(|&(x, y)| (x, y));
        army.piece.valid_moves.dedup();
    }
}

fn move_grid_ui(ui: &mut egui::Ui, army_idx: usize, moves: &mut Vec<(i32, i32)>) -> bool {
    let radius = moves
        .iter()
        .map(|&(dx, dy)| dx.abs().max(dy.abs()))
        .max()
        .unwrap_or(1)
        .max(4);
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
