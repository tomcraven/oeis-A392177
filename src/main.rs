mod camera;
mod model;
mod render;
mod sim;
mod spiral;
mod ui;
mod viewport;

use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};

use camera::camera_controls;
use model::GameDefinition;
use render::{draw_spiral_cells, setup_render_assets, sync_army_materials, RenderCache};
use sim::Simulation;
use ui::{ui_game_definition, UiState};
use viewport::{sync_simulation_to_viewport, ViewportState};

pub const CELL_SIZE: f32 = 16.0;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Red & Black Knights".into(),
                        resolution: (1280, 720).into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                })
                .set(ImagePlugin::default_nearest()),
        )
        .add_plugins(EguiPlugin::default())
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.05)))
        .init_resource::<GameDefinition>()
        .init_resource::<Simulation>()
        .init_resource::<UiState>()
        .init_resource::<RenderCache>()
        .init_resource::<ViewportState>()
        .add_systems(Startup, (setup_camera, setup_render_assets, setup_smoke_test))
        .add_systems(EguiPrimaryContextPass, ui_game_definition)
        .add_systems(
            Update,
            (
                camera_controls,
                sync_simulation_to_viewport,
                sync_army_materials,
                draw_spiral_cells,
                smoke_test_exit,
            )
                .chain(),
        )
        .run();
}

#[derive(Resource, Default)]
struct SmokeTest {
    enabled: bool,
}

fn setup_smoke_test(mut commands: Commands) {
    if std::env::args().any(|a| a == "--smoke-test") {
        commands.insert_resource(SmokeTest { enabled: true });
    }
}

/// With `--smoke-test`, exit cleanly after a couple of seconds (for log-based CI checks).
fn smoke_test_exit(
    smoke: Option<Res<SmokeTest>>,
    time: Res<Time>,
    mut exit: MessageWriter<AppExit>,
) {
    if smoke.is_none_or(|s| !s.enabled) {
        return;
    }
    if time.elapsed_secs() >= 2.0 {
        exit.write(AppExit::Success);
    }
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        camera::PanCamera {
            ..default()
        },
    ));
}
