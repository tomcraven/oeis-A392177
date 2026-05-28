//! Hover overlay on the board: placement paths and attack squares.

use bevy::prelude::*;
use bevy_egui::EguiContexts;
use bevy_egui::egui::{self, Color32, Id, LayerId, Order, Pos2, Rect, Stroke, vec2};

use crate::camera::BoardCamera;
use crate::index_order::VisitOrder;
use crate::model::{GameDefinition, PieceId};
use crate::perf_harness::PerfHarnessRun;
use crate::placement_path::{forbidden_skips_to_placement, same_piece_path_to};
use crate::sim_worker::SimulationBridge;
use crate::ui::UiState;
use crate::viewport::{
    ViewportState, board_panel_size, cursor_on_board_panel, grid_to_world, spiral_index_at_cursor,
    world_to_screen_on_board,
};

const HOT_PINK: Color32 = Color32::from_rgb(255, 105, 180);
const HOT_PINK_FILL: Color32 = Color32::from_rgba_premultiplied(255, 105, 180, 72);
const AMBER: Color32 = Color32::from_rgb(255, 180, 0);
const AMBER_FILL: Color32 = Color32::from_rgba_premultiplied(255, 180, 0, 100);

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

pub fn draw_hover_forbidden_skips(
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
    if !ui_state.show_hover_forbidden_skips {
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

    let skips = forbidden_skips_to_placement(
        def.as_ref(),
        hover.visit_order,
        sim.display.placements.as_ref(),
        index,
        piece_id,
    );
    if skips.is_empty() {
        return;
    }

    let layer = LayerId::new(Order::Foreground, Id::new("forbidden_skips_hover"));
    let mut painter = ctx.layer_painter(layer);
    painter.set_clip_rect(board_clip_rect(&hover));
    let stroke = Stroke::new(1.5, AMBER);
    for idx in skips {
        draw_cell_rect(&mut painter, idx, &hover, AMBER_FILL, stroke);
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
    let center = world_to_screen_on_board(
        grid_to_world(x, y),
        ctx.transform,
        ctx.ortho,
        ctx.left,
        ctx.board_w,
        ctx.board_h,
    );
    let east = world_to_screen_on_board(
        grid_to_world(x + 1, y),
        ctx.transform,
        ctx.ortho,
        ctx.left,
        ctx.board_w,
        ctx.board_h,
    );
    let south = world_to_screen_on_board(
        grid_to_world(x, y + 1),
        ctx.transform,
        ctx.ortho,
        ctx.left,
        ctx.board_w,
        ctx.board_h,
    );
    let half_w = (east.x - center.x).abs().max(1.0);
    let half_h = (south.y - center.y).abs().max(1.0);
    Rect::from_center_size(center, vec2(half_w * 2.0, half_h * 2.0))
}
