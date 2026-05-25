use bevy::prelude::*;
use std::time::Duration;

use crate::CELL_SIZE;
use crate::sim_worker::SimulationBridge;
use crate::spiral::xy_to_index;

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
    pub simulation_pending: bool,
    pub left_inset_px: f32,
    /// Frames since visible grid bounds last changed (zoom/pan). Sim catch-up waits until settled.
    bounds_stable_frames: u32,
    needs_target_refresh: bool,
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

impl ViewportState {
    /// Allow simulation to run immediately after a preset reset (no zoom settle wait).
    pub fn allow_sim_catchup_immediately(&mut self) {
        self.bounds_stable_frames = BOUNDS_SETTLE_FRAMES;
        self.needs_target_refresh = true;
    }

    pub fn is_interactively_moving(&self) -> bool {
        self.bounds_stable_frames < BOUNDS_SETTLE_FRAMES
    }
}

/// Wait this many frames after bounds stop changing before requesting more simulation.
pub const BOUNDS_SETTLE_FRAMES: u32 = 4;

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
    window: &Window,
    left_inset_px: f32,
) -> GridBounds {
    let world_w = ortho.area.width() * ortho.scale;
    let half_w = world_w * 0.5;
    let half_h = ortho.area.height() * ortho.scale * 0.5;
    let center = camera_transform.translation.truncate();
    let left_inset_fraction = (left_inset_px / window.width().max(1.0)).clamp(0.0, 0.95);
    let min_world_x = center.x - half_w + world_w * left_inset_fraction;
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

fn max_visible_spiral_index(bounds: GridBounds) -> u32 {
    let mut max_index = 0;
    for x in bounds.min_x..=bounds.max_x {
        max_index = max_index.max(xy_to_index(x, bounds.min_y));
        max_index = max_index.max(xy_to_index(x, bounds.max_y));
    }
    for y in bounds.min_y..=bounds.max_y {
        max_index = max_index.max(xy_to_index(bounds.min_x, y));
        max_index = max_index.max(xy_to_index(bounds.max_x, y));
    }
    max_index
}

const INDEX_MARGIN: u32 = 64;
const SIM_FRAME_BUDGET: Duration = Duration::from_millis(16);
/// Conservative cap under the usual 16384 GPU limit (grid texture is one texel per cell).
const GPU_MAX_GRID_TEXTURE_DIMENSION: f32 = 16_000.0;

/// Largest orthographic scale before the visible grid would exceed GPU texture limits.
pub fn max_safe_zoom_out_scale(
    ortho: &OrthographicProjection,
    window: &Window,
    left_inset_px: f32,
) -> f32 {
    let inset = (left_inset_px / window.width().max(1.0)).clamp(0.0, 0.95);
    let board_w = ortho.area.width() * (1.0 - inset);
    let board_h = ortho.area.height();
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

pub fn sync_simulation_to_viewport(
    mut sim: ResMut<SimulationBridge>,
    mut viewport: ResMut<ViewportState>,
    camera_q: Query<(&Transform, &Projection), With<Camera2d>>,
    window_q: Query<&Window>,
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

    let bounds_changed = viewport.bounds != Some(bounds);
    if bounds_changed {
        viewport.bounds = Some(bounds);
        viewport.render_dirty = true;
        viewport.bounds_stable_frames = 0;
        viewport.needs_target_refresh = true;

        let new_target = max_visible_spiral_index(bounds).saturating_add(INDEX_MARGIN);
        if new_target != viewport.target_index {
            viewport.target_index = new_target;
            sim.reprioritize_advance(new_target, SIM_FRAME_BUDGET);
        }
    } else {
        viewport.bounds_stable_frames = viewport.bounds_stable_frames.saturating_add(1);
    }

    let bounds_settled = viewport.bounds_stable_frames >= BOUNDS_SETTLE_FRAMES;
    if bounds_settled && viewport.needs_target_refresh {
        let new_target = max_visible_spiral_index(bounds).saturating_add(INDEX_MARGIN);
        if new_target != viewport.target_index {
            viewport.target_index = new_target;
            sim.reprioritize_advance(new_target, SIM_FRAME_BUDGET);
        }
        viewport.needs_target_refresh = false;
    }
    if bounds_settled && sim.needs_work(viewport.target_index) && !sim.is_busy() {
        sim.request_advance(viewport.target_index, SIM_FRAME_BUDGET);
    }

    viewport.simulation_pending =
        sim.needs_work(viewport.target_index) || sim.is_busy() || !bounds_settled;
}
