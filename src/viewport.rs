use bevy::prelude::*;
use std::time::Duration;

use crate::CELL_SIZE;
use crate::camera::BoardCamera;
use crate::camera_config::CameraSessionConfig;
use crate::index_order::VisitOrder;
use crate::model::{GameDefinition, PieceId};
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

/// Typical egui sidebar width when computing share-code screenshot aspect (board panel is a rect).
pub const DEFAULT_BOARD_LEFT_INSET_PX: f32 = 320.0;

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

    /// Axis-aligned overlap of two grid rectangles (None if disjoint).
    pub fn intersect(self, other: Self) -> Option<Self> {
        let min_x = self.min_x.max(other.min_x);
        let max_x = self.max_x.min(other.max_x);
        let min_y = self.min_y.max(other.min_y);
        let max_y = self.max_y.min(other.max_y);
        if min_x > max_x || min_y > max_y {
            return None;
        }
        Some(Self {
            min_x,
            max_x,
            min_y,
            max_y,
        })
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

/// Window cursor position → board world (board camera, logical pixels).
pub fn screen_to_board_world(
    cursor: Vec2,
    camera_transform: &Transform,
    ortho: &OrthographicProjection,
    left_inset_px: f32,
    board_width_px: f32,
    board_height_px: f32,
) -> Vec2 {
    let half_w = ortho.area.width() * 0.5;
    let half_h = ortho.area.height() * 0.5;
    let center = camera_transform.translation.truncate();
    let u = ((cursor.x - left_inset_px) / board_width_px.max(1.0)).clamp(0.0, 1.0);
    let v = (cursor.y / board_height_px.max(1.0)).clamp(0.0, 1.0);
    Vec2::new(
        center.x - half_w + u * (2.0 * half_w),
        center.y + half_h - v * (2.0 * half_h),
    )
}

pub fn world_to_screen_on_board(
    world: Vec2,
    camera_transform: &Transform,
    ortho: &OrthographicProjection,
    left_inset_px: f32,
    board_width_px: f32,
    board_height_px: f32,
) -> bevy_egui::egui::Pos2 {
    let half_w = ortho.area.width() * 0.5;
    let half_h = ortho.area.height() * 0.5;
    let center = camera_transform.translation.truncate();
    let u = (world.x - (center.x - half_w)) / (2.0 * half_w);
    let v = (center.y + half_h - world.y) / (2.0 * half_h);
    bevy_egui::egui::Pos2::new(
        left_inset_px + u * board_width_px,
        v * board_height_px,
    )
}

pub fn board_panel_size(window: &Window, left_inset_px: f32) -> (f32, f32) {
    let board_w = (window.width() - left_inset_px).max(1.0);
    let board_h = window.height().max(1.0);
    (board_w, board_h)
}

pub fn cursor_on_board_panel(cursor: Vec2, left_inset_px: f32, board_height_px: f32) -> bool {
    cursor.x >= left_inset_px && cursor.y >= 0.0 && cursor.y <= board_height_px
}

/// Spiral index under the window cursor on the board panel, if the cursor is over the panel.
pub fn spiral_index_at_cursor(
    cursor: Vec2,
    camera_transform: &Transform,
    ortho: &OrthographicProjection,
    left_inset_px: f32,
    board_width_px: f32,
    board_height_px: f32,
    visit_order: VisitOrder,
) -> Option<u32> {
    if !cursor_on_board_panel(cursor, left_inset_px, board_height_px) {
        return None;
    }
    let world = screen_to_board_world(
        cursor,
        camera_transform,
        ortho,
        left_inset_px,
        board_width_px,
        board_height_px,
    );
    let (gx, gy) = world_to_grid(world);
    Some(visit_order.xy_to_index(gx, gy))
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

/// Tight axis-aligned rectangle around occupied cells (visit order defines index → xy).
pub fn grid_bounds_from_placements(
    placements: &[(u32, PieceId)],
    order: VisitOrder,
    padding: i32,
) -> GridBounds {
    if placements.is_empty() {
        return GridBounds {
            min_x: -padding,
            max_x: padding,
            min_y: -padding,
            max_y: padding,
        };
    }
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for &(index, _) in placements {
        let (x, y) = order.index_to_xy(index);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    GridBounds {
        min_x: min_x - padding,
        max_x: max_x + padding,
        min_y: min_y - padding,
        max_y: max_y + padding,
    }
}

/// Grid rectangle visible on the board panel for a saved camera pose.
pub fn grid_bounds_for_camera_session(camera: CameraSessionConfig) -> GridBounds {
    grid_bounds_for_camera_session_with_inset(camera, DEFAULT_BOARD_LEFT_INSET_PX)
}

pub fn grid_bounds_for_camera_session_with_inset(
    camera: CameraSessionConfig,
    left_inset_px: f32,
) -> GridBounds {
    use bevy::camera::{CameraProjection, OrthographicProjection, ScalingMode};

    let transform = Transform::from_xyz(camera.x, camera.y, 0.0);
    let scale = camera
        .zoom
        .clamp(
            crate::calibration_config::MIN_ZOOM_OUT,
            crate::calibration_config::MAX_ZOOM_OUT_BUDGET,
        );
    let mut ortho = OrthographicProjection {
        scaling_mode: ScalingMode::FixedVertical {
            viewport_height: WINDOW_HEIGHT,
        },
        scale,
        ..OrthographicProjection::default_2d()
    };
    ortho.update(
        (WINDOW_WIDTH - left_inset_px).max(1.0),
        WINDOW_HEIGHT,
    );
    viewport_grid_bounds(&transform, &ortho, &Window::default(), left_inset_px)
}

/// Share-code screenshot: simulation placement rect ∩ camera viewport (both axis-aligned rects).
pub fn grid_bounds_for_share_screenshot(
    camera: CameraSessionConfig,
    placements: &[(u32, PieceId)],
    visit_order: VisitOrder,
) -> GridBounds {
    const PADDING: i32 = 2;
    let sim = grid_bounds_from_placements(placements, visit_order, PADDING);
    let view = grid_bounds_for_camera_session(camera);
    sim.intersect(view).unwrap_or(view)
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
    mut viewport: ResMut<ViewportState>,
    camera_q: Query<(&Transform, &Projection), With<BoardCamera>>,
    window_q: Query<&Window>,
    #[cfg(feature = "app_profile")] mut profile_frame: Option<
        ResMut<crate::app_profile::AppProfileFrame>,
    >,
) {
    #[cfg(feature = "app_profile")]
    if let Some(frame) = profile_frame.as_mut() {
        let visit_order = sim.visit_order();
        crate::app_profile::scope("sync_viewport", frame, || {
            sync_simulation_to_viewport_inner(
                &mut sim,
                def.as_ref(),
                visit_order,
                &mut viewport,
                &camera_q,
                &window_q,
            );
        });
        return;
    }
    let visit_order = sim.visit_order();
    sync_simulation_to_viewport_inner(
        &mut sim,
        def.as_ref(),
        visit_order,
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
    viewport.target_index = new_target;
    // Never interrupt a running advance job for camera movement. The sim fills in monotonic
    // spiral-index order, so a changed view never invalidates work already done: a larger target
    // just means "keep going further" and a smaller one is already covered. The worker picks up
    // the latest target at its next budget boundary via `request_advance` (fires only when idle).
    // Interrupting mid-job forced a `snapshot` + full copy-on-write clone of the (tens-of-MB)
    // occupancy grid and placements log every frame during sustained panning, which froze the
    // fill frontier (~512 turns/frame instead of ~500k).
    if !sim.is_saturated() && !sim.is_busy() && sim.needs_work(def, new_target) {
        sim.request_advance(new_target, SIM_FRAME_BUDGET);
    }
}
