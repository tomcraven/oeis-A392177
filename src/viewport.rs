use bevy::prelude::*;

use crate::CELL_SIZE;
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
        for y in bounds.min_y..=bounds.max_y {
            max_index = max_index.max(xy_to_index(x, y));
        }
    }
    max_index
}

const INDEX_MARGIN: u32 = 64;

pub fn sync_simulation_to_viewport(
    mut sim: ResMut<crate::sim::Simulation>,
    def: Res<crate::model::GameDefinition>,
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
    let bounds_changed = viewport.bounds != Some(bounds);
    if bounds_changed {
        viewport.bounds = Some(bounds);
        viewport.target_index = max_visible_spiral_index(bounds).saturating_add(INDEX_MARGIN);
        viewport.simulation_pending = true;
    }

    if !bounds_changed && sim.needs_work(viewport.target_index) {
        sim.advance_budget(&def, viewport.target_index, 50_000);
    }

    let still_pending = sim.needs_work(viewport.target_index);
    if (bounds_changed || viewport.simulation_pending) && !still_pending {
        viewport.render_dirty = true;
    }
    viewport.simulation_pending = still_pending;
}
