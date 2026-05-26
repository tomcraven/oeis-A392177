use bevy::input::gestures::PinchGesture;
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;

use crate::calibration_config;
use crate::camera_config::{CameraSessionConfig, load, save};
use crate::viewport::grid_to_world;

#[derive(Resource, Default)]
pub struct LastSavedCamera(pub Option<CameraSessionConfig>);

#[derive(Resource, Default)]
pub struct PendingCameraAction {
    pub center_view: bool,
}

#[derive(Component)]
pub struct BoardCamera;

#[derive(Component)]
pub struct PanCamera {
    /// How many viewport widths per second to pan at full WASD input (zoom-invariant on screen).
    pub pan_screen_widths_per_sec: f32,
    pub zoom_speed: f32,
    pub min_scale: f32,
    pub max_scale: f32,
}

impl Default for PanCamera {
    fn default() -> Self {
        Self {
            pan_screen_widths_per_sec: 0.65,
            zoom_speed: 0.12,
            min_scale: calibration_config::MIN_ZOOM_OUT,
            max_scale: 8192.0,
        }
    }
}

#[derive(Resource, Default)]
pub struct BoardPointerState {
    mouse_pan: bool,
    pan_touch_id: Option<u64>,
    last_pinch_span: Option<f32>,
}

fn pointer_in_board(pos: Vec2, window: &Window, left_inset_px: f32) -> bool {
    pos.x >= left_inset_px
        && pos.x <= window.width()
        && pos.y >= 0.0
        && pos.y <= window.height()
}

fn screen_delta_to_world(
    delta_px: Vec2,
    ortho: &OrthographicProjection,
    board_width_px: f32,
    board_height_px: f32,
) -> Vec2 {
    let sx = ortho.area.width() / board_width_px.max(1.0);
    let sy = ortho.area.height() / board_height_px.max(1.0);
    Vec2::new(-delta_px.x * sx, delta_px.y * sy)
}

fn apply_board_pan(
    transform: &mut Transform,
    delta_px: Vec2,
    ortho: &OrthographicProjection,
    board_width_px: f32,
    board_height_px: f32,
) {
    let world = screen_delta_to_world(delta_px, ortho, board_width_px, board_height_px);
    transform.translation += world.extend(0.0);
}

fn apply_zoom_factor(ortho: &mut OrthographicProjection, factor: f32, pan: &PanCamera) {
    ortho.scale = (ortho.scale * factor).clamp(pan.min_scale, pan.max_scale);
}

/// Runs after egui updates [`EguiWantsInput`] so typing in the sidebar does not pan the board.
pub fn camera_controls(
    egui_wants: Res<EguiWantsInput>,
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&mut Transform, &PanCamera, &Projection), With<BoardCamera>>,
) {
    if egui_wants.wants_keyboard_input() {
        return;
    }
    let Ok((mut transform, pan, projection)) = query.single_mut() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };

    let mut delta = Vec2::ZERO;
    if keyboard.pressed(KeyCode::KeyW) || keyboard.pressed(KeyCode::ArrowUp) {
        delta.y += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyS) || keyboard.pressed(KeyCode::ArrowDown) {
        delta.y -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyA) || keyboard.pressed(KeyCode::ArrowLeft) {
        delta.x -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) || keyboard.pressed(KeyCode::ArrowRight) {
        delta.x += 1.0;
    }
    if delta != Vec2::ZERO {
        // `OrthographicProjection::area` already includes `scale` (see Bevy camera update).
        let visible_world_width = ortho.area.width();
        let world_per_sec = pan.pan_screen_widths_per_sec * visible_world_width;
        transform.translation +=
            (delta.normalize() * world_per_sec * time.delta_secs()).extend(0.0);
    }
}

/// Runs after egui updates [`EguiWantsInput`] so scroll over UI does not zoom the board.
pub fn camera_zoom_controls(
    egui_wants: Res<EguiWantsInput>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut query: Query<&mut Projection, With<BoardCamera>>,
    pan_q: Query<&PanCamera, With<BoardCamera>>,
) {
    let Ok(mut projection) = query.single_mut() else {
        return;
    };
    let Ok(pan) = pan_q.single() else {
        return;
    };

    for wheel in mouse_wheel.read() {
        if egui_wants.wants_any_pointer_input() {
            continue;
        }
        let scroll = match wheel.unit {
            MouseScrollUnit::Line => wheel.y,
            MouseScrollUnit::Pixel => wheel.y * 0.05,
        };
        if let Projection::Orthographic(ref mut ortho) = *projection {
            let factor = 1.0 - scroll * pan.zoom_speed;
            apply_zoom_factor(ortho, factor, pan);
        }
    }
}

/// Drag-to-pan (mouse / single touch) and pinch-to-zoom on the board region.
pub fn camera_pointer_controls(
    egui_wants: Res<EguiWantsInput>,
    viewport: Res<crate::viewport::ViewportState>,
    window_q: Query<&Window>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    mut pointer: ResMut<BoardPointerState>,
    mut mouse_button_events: MessageReader<MouseButtonInput>,
    mut mouse_motion_events: MessageReader<MouseMotion>,
    mut pinch_events: MessageReader<PinchGesture>,
    mut query: Query<(&mut Transform, &PanCamera, &mut Projection), With<BoardCamera>>,
) {
    let Ok(window) = window_q.single() else {
        return;
    };
    let Ok((mut transform, pan, mut projection)) = query.single_mut() else {
        return;
    };
    let Projection::Orthographic(ref mut ortho) = *projection else {
        return;
    };

    let left = viewport.left_inset_px;
    let board_w = (window.width() - left).max(1.0);
    let board_h = window.height().max(1.0);
    let block_new_pointer = egui_wants.wants_any_pointer_input();

    for press in mouse_button_events.read() {
        if press.button != MouseButton::Left {
            continue;
        }
        match press.state {
            bevy::input::ButtonState::Pressed => {
                if !block_new_pointer
                    && window
                        .cursor_position()
                        .is_some_and(|p| pointer_in_board(p, window, left))
                {
                    pointer.mouse_pan = true;
                }
            }
            bevy::input::ButtonState::Released => {
                pointer.mouse_pan = false;
            }
        }
    }

    if !mouse_buttons.pressed(MouseButton::Left) {
        pointer.mouse_pan = false;
    }

    if pointer.mouse_pan {
        for motion in mouse_motion_events.read() {
            apply_board_pan(&mut transform, motion.delta, ortho, board_w, board_h);
        }
    } else {
        mouse_motion_events.clear();
    }

    let touch_count = touches.iter().count();
    if touch_count >= 2 {
        pointer.pan_touch_id = None;
        let mut touch_iter = touches.iter();
        let a = touch_iter.next().expect("touch_count >= 2");
        let b = touch_iter.next().expect("touch_count >= 2");
        let span = a.position().distance(b.position());
        if let Some(prev) = pointer.last_pinch_span {
            let factor = (prev / span).clamp(0.2, 5.0);
            apply_zoom_factor(ortho, factor, pan);
        }
        pointer.last_pinch_span = Some(span);
    } else {
        pointer.last_pinch_span = None;

        if touch_count == 1 {
            let touch = touches.iter().next().expect("touch_count == 1");
            if pointer.pan_touch_id.is_none() {
                if touches.just_pressed(touch.id())
                    && pointer_in_board(touch.position(), window, left)
                    && !block_new_pointer
                {
                    pointer.pan_touch_id = Some(touch.id());
                }
            }
            if pointer.pan_touch_id == Some(touch.id()) {
                let delta = touch.delta();
                if delta != Vec2::ZERO {
                    apply_board_pan(&mut transform, delta, ortho, board_w, board_h);
                }
            }
        } else {
            pointer.pan_touch_id = None;
        }
    }

    for pinch in pinch_events.read() {
        if block_new_pointer {
            continue;
        }
        let factor = 1.0 - pinch.0 * pan.zoom_speed;
        apply_zoom_factor(ortho, factor, pan);
    }
}

pub fn clamp_camera_zoom_to_texture_limit(
    viewport: Res<crate::viewport::ViewportState>,
    window_q: Query<&Window>,
    mut query: Query<(&PanCamera, &mut Projection), With<BoardCamera>>,
) {
    let Ok(window) = window_q.single() else {
        return;
    };
    let Ok((pan, mut projection)) = query.single_mut() else {
        return;
    };
    let Projection::Orthographic(ref mut ortho) = *projection else {
        return;
    };
    let safe_max =
        crate::viewport::max_safe_zoom_out_scale(ortho, window, viewport.left_inset_px);
    let budget_cap = if calibration_config::smoke_test_mode() {
        f32::MAX
    } else {
        calibration_config::zoom_out_budget_ceiling(safe_max)
    };
    let effective_max = pan.max_scale.min(safe_max).min(budget_cap);
    ortho.scale = ortho.scale.clamp(pan.min_scale, effective_max);
}

pub fn apply_camera_actions(
    mut pending: ResMut<PendingCameraAction>,
    mut query: Query<&mut Transform, With<BoardCamera>>,
) {
    if !pending.center_view {
        return;
    }
    pending.center_view = false;
    let Ok(mut transform) = query.single_mut() else {
        return;
    };
    let origin = grid_to_world(0, 0);
    transform.translation.x = origin.x;
    transform.translation.y = origin.y;
}

pub fn apply_saved_camera_session(
    mut query: Query<(&mut Transform, &mut Projection), With<BoardCamera>>,
) {
    let Some(saved) = load() else {
        return;
    };
    let Ok((mut transform, mut projection)) = query.single_mut() else {
        return;
    };
    transform.translation.x = saved.x;
    transform.translation.y = saved.y;
    if let Projection::Orthographic(ref mut ortho) = *projection {
        let cap = if calibration_config::smoke_test_mode() {
            f32::MAX
        } else {
            calibration_config::MAX_ZOOM_OUT_BUDGET
        };
        ortho.scale = saved
            .zoom
            .clamp(calibration_config::MIN_ZOOM_OUT, cap);
    }
}

pub fn persist_camera_session(
    query: Query<(&Transform, &Projection), With<BoardCamera>>,
    mut last: ResMut<LastSavedCamera>,
) {
    let Ok((transform, projection)) = query.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let current = CameraSessionConfig {
        x: transform.translation.x,
        y: transform.translation.y,
        zoom: ortho.scale,
    };
    if last.0 == Some(current) {
        return;
    }
    if save(&current).is_ok() {
        last.0 = Some(current);
    }
}
