//! Hover overlay on the board: placement paths and attack squares.

use bevy::prelude::*;
use bevy_egui::EguiContexts;
use bevy_egui::egui::{self, Color32, Id, LayerId, Order, Pos2, Rect, Stroke};

use crate::CELL_SIZE;
use crate::camera::BoardCamera;
use crate::index_order::VisitOrder;
use crate::model::{GameDefinition, PieceId};
use crate::perf_harness::PerfHarnessRun;
use crate::placement_path::{next_same_piece_after, scan_skips_to_placement, same_piece_path_to};
use crate::sim_worker::SimulationBridge;
use crate::ui::UiState;
use crate::viewport::{
    ViewportState, board_panel_size, cursor_on_board_panel, grid_to_world, spiral_index_at_cursor,
    world_to_screen_on_board,
};

const HOT_PINK: Color32 = Color32::from_rgb(255, 105, 180);
const HOT_PINK_FILL: Color32 = Color32::from_rgba_premultiplied(255, 105, 180, 72);
const HOT_PINK_SOURCE_FILL: Color32 = Color32::from_rgba_premultiplied(255, 105, 180, 140);
const AMBER: Color32 = Color32::from_rgb(255, 180, 0);
const AMBER_FILL: Color32 = Color32::from_rgba_premultiplied(255, 180, 0, 100);
const OCCUPIED_SKIP: Color32 = Color32::from_rgb(80, 200, 255);
const OCCUPIED_SKIP_FILL: Color32 = Color32::from_rgba_premultiplied(80, 200, 255, 110);
const HOVERED_CELL: Color32 = Color32::from_rgb(70, 220, 90);
const HOVERED_CELL_FILL: Color32 = Color32::from_rgba_premultiplied(70, 220, 90, 120);
const SUCCEEDING_CELL: Color32 = Color32::from_rgb(255, 140, 60);
const SUCCEEDING_CELL_FILL: Color32 = Color32::from_rgba_premultiplied(255, 140, 60, 130);
const SKIP_LINE_WIDTH: f32 = 1.75;

struct ScanAnchorStyle {
    fill: Color32,
    stroke: Color32,
}

fn draw_placement_scan_context(
    painter: &mut egui::Painter,
    hover: &HoverBoardContext<'_>,
    def: &GameDefinition,
    placements: &[(u32, PieceId)],
    placement_index: u32,
    piece_id: PieceId,
    anchor: ScanAnchorStyle,
) {
    let path = same_piece_path_to(placements, placement_index, piece_id);
    let previous_same_piece = path.len().checked_sub(2).map(|i| path[i]);

    let skips_replay = scan_skips_to_placement(
        def,
        hover.visit_order,
        placements,
        placement_index,
        piece_id,
    );

    if let Some((skips, replay)) = &skips_replay {
        for idx in &skips.occupied {
            draw_cell_rect(
                painter,
                *idx,
                hover,
                OCCUPIED_SKIP_FILL,
                Stroke::new(1.5, OCCUPIED_SKIP),
            );
        }

        let forbidden_stroke = Stroke::new(1.5, AMBER);
        for idx in &skips.forbidden {
            draw_cell_rect(painter, *idx, hover, AMBER_FILL, forbidden_stroke);
            let from = index_center_screen(*idx, hover);
            for attacker in replay.respected_forbidden_attackers(piece_id, *idx) {
                let Some(attacker_index) =
                    replay.placement_blocking_attacker(def, attacker, *idx)
                else {
                    continue;
                };
                let to = index_center_screen(attacker_index, hover);
                let line = Stroke::new(SKIP_LINE_WIDTH, HOT_PINK);
                painter.line_segment([from, to], line);
            }
        }
    }

    if let Some(prev) = previous_same_piece {
        draw_cell_rect(
            painter,
            prev,
            hover,
            HOT_PINK_SOURCE_FILL,
            Stroke::new(2.0, HOT_PINK),
        );
    }

    draw_cell_rect(
        painter,
        placement_index,
        hover,
        anchor.fill,
        Stroke::new(2.0, anchor.stroke),
    );
}

struct HoverBoardContext<'a> {
    left: f32,
    board_w: f32,
    board_h: f32,
    visit_order: VisitOrder,
    transform: &'a Transform,
    ortho: &'a OrthographicProjection,
}

fn hover_board_context<'a>(
    window: &'a Window,
    viewport: &ViewportState,
    transform: &'a Transform,
    ortho: &'a OrthographicProjection,
    visit_order: VisitOrder,
    mouse_buttons: &ButtonInput<MouseButton>,
    harness: Option<&PerfHarnessRun>,
) -> Option<HoverBoardContext<'a>> {
    if harness.is_some() {
        return None;
    }
    if mouse_buttons.pressed(MouseButton::Left) {
        return None;
    }
    let cursor = window.cursor_position()?;
    let left = viewport.left_inset_px;
    let (board_w, board_h) = board_panel_size(window, left);
    if !cursor_on_board_panel(cursor, left, board_h) {
        return None;
    }
    Some(HoverBoardContext {
        left,
        board_w,
        board_h,
        visit_order,
        transform,
        ortho,
    })
}

fn hovered_occupied_cell(
    ctx: &HoverBoardContext<'_>,
    cursor: Vec2,
    sim: &SimulationBridge,
) -> Option<(u32, PieceId)> {
    let index = spiral_index_at_cursor(
        cursor,
        ctx.transform,
        ctx.ortho,
        ctx.left,
        ctx.board_w,
        ctx.board_h,
        ctx.visit_order,
    )?;
    let piece_id = sim.display.occupancy.piece_id_at(index)?;
    Some((index, piece_id))
}

fn board_clip_rect(ctx: &HoverBoardContext<'_>) -> Rect {
    Rect::from_min_max(
        Pos2::new(ctx.left, 0.0),
        Pos2::new(ctx.left + ctx.board_w, ctx.board_h),
    )
}

pub fn draw_hover_placement_paths(
    mut contexts: EguiContexts,
    ui_state: Res<UiState>,
    viewport: Res<ViewportState>,
    sim: Res<SimulationBridge>,
    window_q: Query<&Window>,
    camera_q: Query<(&Transform, &Projection), With<BoardCamera>>,
    harness: Option<Res<PerfHarnessRun>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
) {
    if !ui_state.show_hover_placement_path {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let Ok(window) = window_q.single() else {
        return;
    };
    let Ok((transform, projection)) = camera_q.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let Some(hover) = hover_board_context(
        window,
        &viewport,
        transform,
        ortho,
        sim.visit_order(),
        &mouse_buttons,
        harness.as_deref(),
    ) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Some((index, piece_id)) = hovered_occupied_cell(&hover, cursor, &sim) else {
        return;
    };

    let path = same_piece_path_to(sim.display.placements.as_ref(), index, piece_id);
    if path.len() < 2 {
        return;
    }

    let stroke = Stroke::new(2.0, HOT_PINK);
    let layer = LayerId::new(Order::Foreground, Id::new("placement_path_hover"));
    let mut painter = ctx.layer_painter(layer);
    painter.set_clip_rect(board_clip_rect(&hover));

    for window in path.windows(2) {
        let a = index_center_screen(
            window[0],
            &hover,
        );
        let b = index_center_screen(window[1], &hover);
        painter.line_segment([a, b], stroke);
    }

    for &idx in &path {
        let center = index_center_screen(idx, &hover);
        painter.circle_filled(center, 3.0, HOT_PINK);
    }
}

pub fn draw_hover_attack_squares(
    mut contexts: EguiContexts,
    ui_state: Res<UiState>,
    def: Res<GameDefinition>,
    viewport: Res<ViewportState>,
    sim: Res<SimulationBridge>,
    window_q: Query<&Window>,
    camera_q: Query<(&Transform, &Projection), With<BoardCamera>>,
    harness: Option<Res<PerfHarnessRun>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
) {
    if !ui_state.show_hover_attack_squares {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let Ok(window) = window_q.single() else {
        return;
    };
    let Ok((transform, projection)) = camera_q.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let Some(hover) = hover_board_context(
        window,
        &viewport,
        transform,
        ortho,
        sim.visit_order(),
        &mouse_buttons,
        harness.as_deref(),
    ) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Some((index, piece_id)) = hovered_occupied_cell(&hover, cursor, &sim) else {
        return;
    };
    let Some(army) = def.pieces.get(piece_id) else {
        return;
    };
    if !army.enabled {
        return;
    }

    let (px, py) = hover.visit_order.index_to_xy(index);
    let moves = &army.piece.valid_moves;
    if moves.is_empty() {
        return;
    }

    let layer = LayerId::new(Order::Foreground, Id::new("attack_squares_hover"));
    let mut painter = ctx.layer_painter(layer);
    painter.set_clip_rect(board_clip_rect(&hover));

    for &(dx, dy) in moves {
        let attacked = hover.visit_order.xy_to_index(px + dx, py + dy);
        draw_cell_rect(&mut painter, attacked, &hover, HOT_PINK_FILL, Stroke::new(1.5, HOT_PINK));
    }
}

pub fn draw_hover_neighbor_placement_scan(
    mut contexts: EguiContexts,
    ui_state: Res<UiState>,
    def: Res<GameDefinition>,
    viewport: Res<ViewportState>,
    sim: Res<SimulationBridge>,
    window_q: Query<&Window>,
    camera_q: Query<(&Transform, &Projection), With<BoardCamera>>,
    harness: Option<Res<PerfHarnessRun>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
) {
    if !ui_state.show_hover_forbidden_skips && !ui_state.show_hover_succeeding_cell_info {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let Ok(window) = window_q.single() else {
        return;
    };
    let Ok((transform, projection)) = camera_q.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let Some(hover) = hover_board_context(
        window,
        &viewport,
        transform,
        ortho,
        sim.visit_order(),
        &mouse_buttons,
        harness.as_deref(),
    ) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Some((index, piece_id)) = hovered_occupied_cell(&hover, cursor, &sim) else {
        return;
    };

    let layer = LayerId::new(Order::Foreground, Id::new("placement_scan_hover"));
    let mut painter = ctx.layer_painter(layer);
    painter.set_clip_rect(board_clip_rect(&hover));

    let placements = sim.display.placements.as_ref();
    let def = def.as_ref();

    if ui_state.show_hover_forbidden_skips {
        draw_placement_scan_context(
            &mut painter,
            &hover,
            def,
            placements,
            index,
            piece_id,
            ScanAnchorStyle {
                fill: HOVERED_CELL_FILL,
                stroke: HOVERED_CELL,
            },
        );
    }

    if ui_state.show_hover_succeeding_cell_info {
        if let Some(next) = next_same_piece_after(placements, index, piece_id) {
            draw_placement_scan_context(
                &mut painter,
                &hover,
                def,
                placements,
                next,
                piece_id,
                ScanAnchorStyle {
                    fill: SUCCEEDING_CELL_FILL,
                    stroke: SUCCEEDING_CELL,
                },
            );
        }
    }
}

fn draw_cell_rect(
    painter: &mut egui::Painter,
    index: u32,
    ctx: &HoverBoardContext<'_>,
    fill: Color32,
    stroke: Stroke,
) {
    let rect = cell_screen_rect(index, ctx);
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Inside);
}

fn index_center_screen(index: u32, ctx: &HoverBoardContext<'_>) -> Pos2 {
    let (x, y) = ctx.visit_order.index_to_xy(index);
    let world = grid_to_world(x, y);
    world_to_screen_on_board(
        world,
        ctx.transform,
        ctx.ortho,
        ctx.left,
        ctx.board_w,
        ctx.board_h,
    )
}

fn cell_screen_rect(index: u32, ctx: &HoverBoardContext<'_>) -> Rect {
    let (x, y) = ctx.visit_order.index_to_xy(index);
    let min_world = Vec2::new(x as f32 * CELL_SIZE, y as f32 * CELL_SIZE);
    let max_world = Vec2::new((x + 1) as f32 * CELL_SIZE, (y + 1) as f32 * CELL_SIZE);
    let top_left = world_to_screen_on_board(
        Vec2::new(min_world.x, max_world.y),
        ctx.transform,
        ctx.ortho,
        ctx.left,
        ctx.board_w,
        ctx.board_h,
    );
    let bottom_right = world_to_screen_on_board(
        Vec2::new(max_world.x, min_world.y),
        ctx.transform,
        ctx.ortho,
        ctx.left,
        ctx.board_w,
        ctx.board_h,
    );
    Rect::from_min_max(
        Pos2::new(
            top_left.x.min(bottom_right.x),
            top_left.y.min(bottom_right.y),
        ),
        Pos2::new(
            top_left.x.max(bottom_right.x),
            top_left.y.max(bottom_right.y),
        ),
    )
}
