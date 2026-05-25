use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;

use crate::camera_config::{CameraSessionConfig, load, save};
use crate::viewport::grid_to_world;

#[derive(Resource, Default)]
pub struct LastSavedCamera(pub Option<CameraSessionConfig>);

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
            max_scale: 1024.0,
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

pub fn clamp_camera_zoom_to_texture_limit(
    viewport: Res<crate::viewport::ViewportState>,
    window_q: Query<&Window>,
    mut query: Query<(&PanCamera, &mut Projection), With<Camera2d>>,
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
    let effective_max = pan.max_scale.min(safe_max);
    ortho.scale = ortho.scale.clamp(pan.min_scale, effective_max);
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

pub fn apply_saved_camera_session(
    mut query: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
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
        ortho.scale = saved.zoom;
    }
}

pub fn persist_camera_session(
    query: Query<(&Transform, &Projection), With<Camera2d>>,
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
