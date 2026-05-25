use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::model::{Army, GameDefinition, PieceDef};
use crate::render::RenderCache;
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

    egui::SidePanel::left("game_config")
        .default_width(320.0)
        .show(ctx, |ui| {
            ui.heading("Red & Black Knights");
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
                            draft.armies[army_idx].color = Color::srgb(arr[0], arr[1], arr[2]);
                            config_dirty = true;
                        }

                        ui.label("Moves (dx, dy)");
                        let mut remove_move: Option<usize> = None;
                        for (mi, (dx, dy)) in draft.armies[army_idx]
                            .piece
                            .valid_moves
                            .iter_mut()
                            .enumerate()
                        {
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(dx).speed(1));
                                ui.add(egui::DragValue::new(dy).speed(1));
                                if ui.button("−").clicked() {
                                    remove_move = Some(mi);
                                }
                            });
                        }
                        if let Some(mi) = remove_move {
                            draft.armies[army_idx].piece.valid_moves.remove(mi);
                            config_dirty = true;
                        }
                        if ui.button("Add move").clicked() {
                            draft.armies[army_idx].piece.valid_moves.push((1, 2));
                            config_dirty = true;
                        }

                        ui.label("Blocked by");
                        for other in 0..draft.armies.len() {
                            if other == army_idx {
                                continue;
                            }
                            let mut blocked = draft.armies[army_idx].blocked_by.contains(&other);
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
                ui.colored_label(egui::Color32::YELLOW, "Config changed — apply to reset sim");
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
        });

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
