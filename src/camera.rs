use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;

use crate::viewport::grid_to_world;

#[derive(Resource, Default)]
pub struct PendingCameraAction {
    pub center_view: bool,
}

#[derive(Component)]
pub struct PanCamera {
    pub pan_speed: f32,
    pub zoom_speed: f32,
    pub min_scale: f32,
    pub max_scale: f32,
}

impl Default for PanCamera {
    fn default() -> Self {
        Self {
            pan_speed: 400.0,
            zoom_speed: 0.12,
            min_scale: 0.05,
            max_scale: 8.0,
        }
    }
}

pub fn camera_controls(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&mut Transform, &PanCamera), With<Camera2d>>,
) {
    let Ok((mut transform, pan)) = query.single_mut() else {
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
        transform.translation +=
            (delta.normalize() * pan.pan_speed * time.delta_secs()).extend(0.0);
    }
}

/// Runs after egui updates [`EguiWantsInput`] so scroll over UI does not zoom the board.
pub fn camera_zoom_controls(
    egui_wants: Res<EguiWantsInput>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut query: Query<&mut Projection, With<Camera2d>>,
    pan_q: Query<&PanCamera, With<Camera2d>>,
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
            ortho.scale = (ortho.scale * factor).clamp(pan.min_scale, pan.max_scale);
        }
    }
}

pub fn apply_camera_actions(
    mut pending: ResMut<PendingCameraAction>,
    mut query: Query<&mut Transform, With<Camera2d>>,
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
