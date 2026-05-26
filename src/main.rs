use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use bevy::camera::CameraUpdateSystems;
use bevy::camera::CameraOutputMode;
use bevy::camera::visibility::RenderLayers;
use bevy::render::render_resource::BlendState;
use bevy_egui::{
    EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext,
    input::write_egui_wants_input_system,
};

use red_black_knights::calibration::{
    CalibrationGate, UserMaxZoomOut, advance_calibration, calibration_overlay,
    handle_recalibrate_requests, setup_calibration,
};
use red_black_knights::camera::{self, camera_controls};
use red_black_knights::model::GameDefinition;
use red_black_knights::render::{
    RenderCache, draw_spiral_cells, setup_render_assets, sync_army_materials,
};
use red_black_knights::sim_worker::SimulationBridge;
use red_black_knights::ui::{UiState, board_camera_active, sim_catchup_overlay, ui_game_definition};
use red_black_knights::viewport::{
    ViewportState, sync_board_camera_viewport, sync_simulation_to_viewport, WINDOW_HEIGHT,
    WINDOW_WIDTH,
};

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Red & Black Knights".into(),
                        resolution: (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32).into(),
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
        .init_resource::<CalibrationGate>()
        .init_resource::<UserMaxZoomOut>()
        .add_systems(
            Startup,
            (
                disable_egui_auto_primary_context,
                setup_camera,
                setup_calibration,
                camera::apply_saved_camera_session.after(setup_calibration),
                setup_sim_worker,
                setup_render_assets,
                setup_smoke_test,
            )
                .chain(),
        )
        .add_systems(
            EguiPrimaryContextPass,
            (calibration_overlay, ui_game_definition, sim_catchup_overlay).chain(),
        )
        .add_systems(
            Update,
            (
                handle_recalibrate_requests,
                camera::apply_camera_actions,
                sync_army_materials,
                smoke_test_exit,
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            (
                sync_board_camera_viewport.before(CameraUpdateSystems),
                camera_controls.run_if(board_camera_active),
                camera::camera_zoom_controls
                    .after(write_egui_wants_input_system)
                    .run_if(board_camera_active),
                camera::persist_camera_session,
                sync_simulation_to_viewport,
                draw_spiral_cells,
                advance_calibration,
                camera::clamp_camera_zoom_to_texture_limit,
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

fn disable_egui_auto_primary_context(mut settings: ResMut<EguiGlobalSettings>) {
    settings.auto_create_primary_context = false;
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        camera::BoardCamera,
        camera::PanCamera { ..default() },
    ));
    commands.spawn((
        Camera2d,
        PrimaryEguiContext,
        RenderLayers::none(),
        Camera {
            order: 1,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
    ));
}

fn setup_sim_worker(mut commands: Commands, def: Res<GameDefinition>) {
    commands.insert_resource(SimulationBridge::spawn(def.clone()));
}
