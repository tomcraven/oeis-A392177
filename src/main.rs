use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass, input::write_egui_wants_input_system};

use red_black_knights::camera::{self, camera_controls};
use red_black_knights::model::GameDefinition;
use red_black_knights::render::{
    RenderCache, draw_spiral_cells, setup_render_assets, sync_army_materials,
};
use red_black_knights::sim_worker::SimulationBridge;
use red_black_knights::ui::{UiState, ui_game_definition};
use red_black_knights::viewport::{ViewportState, sync_simulation_to_viewport};

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
        .init_resource::<UiState>()
        .init_resource::<RenderCache>()
        .init_resource::<ViewportState>()
        .init_resource::<camera::PendingCameraAction>()
        .init_resource::<camera::LastSavedCamera>()
        .add_systems(
            Startup,
            (
                setup_camera,
                camera::apply_saved_camera_session.after(setup_camera),
                setup_sim_worker,
                setup_render_assets,
                setup_smoke_test,
            ),
        )
        .add_systems(EguiPrimaryContextPass, ui_game_definition)
        .add_systems(
            Update,
            (
                camera::apply_camera_actions,
                sync_army_materials,
                smoke_test_exit,
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            (
                camera_controls,
                camera::camera_zoom_controls.after(write_egui_wants_input_system),
                camera::clamp_camera_zoom_to_texture_limit,
                camera::persist_camera_session,
                sync_simulation_to_viewport,
                draw_spiral_cells,
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
    commands.spawn((Camera2d, camera::PanCamera { ..default() }));
}

fn setup_sim_worker(mut commands: Commands, def: Res<GameDefinition>) {
    commands.insert_resource(SimulationBridge::spawn(def.clone()));
}
