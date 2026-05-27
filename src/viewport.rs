use bevy::prelude::*;
use std::time::Duration;

use crate::CELL_SIZE;
use crate::camera::BoardCamera;
use crate::model::GameDefinition;
use crate::index_order::VisitOrder;
use crate::sim_worker::SimulationBridge;

/// Default window size (side panel + rectangular board region).
pub const WINDOW_WIDTH: f32 = 1440.0;
pub const WINDOW_HEIGHT: f32 = 900.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridBounds {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
}

#[derive(Resource, Default)]
pub struct ViewportState {
    pub bounds: Option<GridBounds>,
    pub target_index: u32,
    pub render_dirty: bool,
    pub left_inset_px: f32,
}

impl GridBounds {
    pub fn cell_width(&self) -> i32 {
        self.max_x - self.min_x + 1
    }

    pub fn cell_height(&self) -> i32 {
        self.max_y - self.min_y + 1
    }

    pub fn cell_count(&self) -> u64 {
        self.cell_width().max(0) as u64 * self.cell_height().max(0) as u64
    }
}

pub fn world_to_grid(world: Vec2) -> (i32, i32) {
    let x = (world.x / CELL_SIZE).floor() as i32;
    let y = (world.y / CELL_SIZE).floor() as i32;
    (x, y)
}

pub fn grid_to_world(x: i32, y: i32) -> Vec2 {
    Vec2::new((x as f32 + 0.5) * CELL_SIZE, (y as f32 + 0.5) * CELL_SIZE)
}

pub fn viewport_grid_bounds(
    camera_transform: &Transform,
    ortho: &OrthographicProjection,
    _window: &Window,
    _left_inset_px: f32,
) -> GridBounds {
    // `OrthographicProjection::area` is the visible world size (scale already applied).
    let half_w = ortho.area.width() * 0.5;
    let half_h = ortho.area.height() * 0.5;
    let center = camera_transform.translation.truncate();
    let min_world_x = center.x - half_w;
    let max_world_x = center.x + half_w;

    let corners = [
        Vec2::new(min_world_x, center.y - half_h),
        Vec2::new(max_world_x, center.y - half_h),
        Vec2::new(min_world_x, center.y + half_h),
        Vec2::new(max_world_x, center.y + half_h),
    ];

    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for c in corners {
        let (gx, gy) = world_to_grid(c);
        min_x = min_x.min(gx);
        max_x = max_x.max(gx);
        min_y = min_y.min(gy);
        max_y = max_y.max(gy);
    }

    let margin = 2;
    GridBounds {
        min_x: min_x - margin,
        max_x: max_x + margin,
        min_y: min_y - margin,
        max_y: max_y + margin,
    }
}

pub fn visit_target_index_for_bounds(bounds: GridBounds, order: VisitOrder) -> u32 {
    max_visible_index_for_order(bounds, order).saturating_add(INDEX_MARGIN)
}

pub fn spiral_target_index_for_bounds(bounds: GridBounds) -> u32 {
    visit_target_index_for_bounds(bounds, VisitOrder::SquareSpiral)
}

fn max_visible_index_for_order(bounds: GridBounds, order: VisitOrder) -> u32 {
    let mut max_index = 0;
    for x in bounds.min_x..=bounds.max_x {
        max_index = max_index.max(order.xy_to_index(x, bounds.min_y));
        max_index = max_index.max(order.xy_to_index(x, bounds.max_y));
    }
    for y in bounds.min_y..=bounds.max_y {
        max_index = max_index.max(order.xy_to_index(bounds.min_x, y));
        max_index = max_index.max(order.xy_to_index(bounds.max_x, y));
    }
    max_index
}

const INDEX_MARGIN: u32 = 24;
const SIM_FRAME_BUDGET: Duration = Duration::from_millis(16);
/// Cap at the usual 16384 GPU limit (grid texture is one texel per cell).
const GPU_MAX_GRID_TEXTURE_DIMENSION: f32 = 16_384.0;

/// Largest orthographic scale before the visible grid would exceed GPU texture limits.
pub fn max_safe_zoom_out_scale(
    ortho: &OrthographicProjection,
    _window: &Window,
    _left_inset_px: f32,
) -> f32 {
    // Board camera viewport matches `ortho.area`; no side-panel inset in world space.
    let scale = ortho.scale.max(1e-6);
    let board_w = ortho.area.width() / scale;
    let board_h = ortho.area.height() / scale;
    let cell_budget = GPU_MAX_GRID_TEXTURE_DIMENSION - 4.0;

    let max_scale_w = if board_w > 0.0 {
        cell_budget * CELL_SIZE / board_w
    } else {
        f32::MAX
    };
    let max_scale_h = if board_h > 0.0 {
        cell_budget * CELL_SIZE / board_h
    } else {
        f32::MAX
    };
    max_scale_w.min(max_scale_h)
}

/// Restrict the game camera to the board rectangle (right of the egui side panel).
pub fn sync_board_camera_viewport(
    viewport: Res<ViewportState>,
    window_q: Query<&Window>,
    mut cameras: Query<&mut Camera, With<BoardCamera>>,
) {
    let Ok(window) = window_q.single() else {
        return;
    };
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };
    let scale = window.scale_factor();
    let left = (viewport.left_inset_px * scale).round().max(0.0) as u32;
    let phys_w = window.physical_width().saturating_sub(left).max(1);
    let phys_h = window.physical_height().max(1);
    camera.viewport = Some(bevy::camera::Viewport {
        physical_position: UVec2::new(left, 0),
        physical_size: UVec2::new(phys_w, phys_h),
        depth: 0.0..1.0,
    });
}

pub fn sync_simulation_to_viewport(
    mut sim: ResMut<SimulationBridge>,
    def: Res<GameDefinition>,
    ui_state: Res<crate::ui::UiState>,
    mut viewport: ResMut<ViewportState>,
    camera_q: Query<(&Transform, &Projection), With<BoardCamera>>,
    window_q: Query<&Window>,
    #[cfg(feature = "app_profile")] mut profile_frame: Option<
        ResMut<crate::app_profile::AppProfileFrame>,
    >,
) {
    #[cfg(feature = "app_profile")]
    if let Some(frame) = profile_frame.as_mut() {
        crate::app_profile::scope("sync_viewport", frame, || {
            sync_simulation_to_viewport_inner(
                &mut sim,
                def.as_ref(),
                ui_state.visit_order,
                &mut viewport,
                &camera_q,
                &window_q,
            );
        });
        return;
    }
    sync_simulation_to_viewport_inner(
        &mut sim,
        def.as_ref(),
        ui_state.visit_order,
        &mut viewport,
        &camera_q,
        &window_q,
    );
}

fn sync_simulation_to_viewport_inner(
    sim: &mut SimulationBridge,
    def: &GameDefinition,
    visit_order: VisitOrder,
    viewport: &mut ViewportState,
    camera_q: &Query<(&Transform, &Projection), With<BoardCamera>>,
    window_q: &Query<&Window>,
) {
    let Ok((transform, projection)) = camera_q.single() else {
        return;
    };
    let Ok(window) = window_q.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };

    let bounds = viewport_grid_bounds(transform, ortho, window, viewport.left_inset_px);
    if sim.poll_updates() {
        viewport.render_dirty = true;
    }

    if viewport.bounds != Some(bounds) {
        viewport.bounds = Some(bounds);
        viewport.render_dirty = true;
    }

    let new_target = visit_target_index_for_bounds(bounds, visit_order);
    if new_target != viewport.target_index {
        viewport.target_index = new_target;
        sim.reprioritize_advance(new_target, SIM_FRAME_BUDGET);
    } else if sim.needs_work(def, viewport.target_index) && !sim.is_busy() {
        sim.request_advance(viewport.target_index, SIM_FRAME_BUDGET);
    }
}
