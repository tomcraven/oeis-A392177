use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

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
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut query: Query<(&mut Transform, &mut Projection, &PanCamera), With<Camera2d>>,
) {
    let Ok((mut transform, mut projection, pan)) = query.single_mut() else {
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
        transform.translation += (delta.normalize() * pan.pan_speed * time.delta_secs()).extend(0.0);
    }

    for wheel in mouse_wheel.read() {
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
