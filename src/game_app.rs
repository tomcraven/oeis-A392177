//! Shared Bevy app wiring for the main binary and perf harness.

use bevy::asset::AssetMetaCheck;
use bevy::camera::CameraUpdateSystems;
use bevy::prelude::*;
use bevy_egui::{
    EguiGlobalSettings, EguiPlugin, EguiPostUpdateSet, EguiPrimaryContextPass, PrimaryEguiContext,
    input::write_egui_wants_input_system,
};

use crate::app_session::{self, AppSessionCache};
#[cfg(not(target_family = "wasm"))]
use crate::board_export::BoardExportDialogState;
#[cfg(target_family = "wasm")]
use crate::board_export::BoardExportWasmJob;
use crate::board_export::{BoardExportPending, run_board_export};
use crate::board_hover::{
    draw_hover_attack_squares, draw_hover_forbidden_skips, draw_hover_placement_paths,
};
use crate::camera::{self, camera_controls};
use crate::index_order::VisitOrder;
use crate::model::GameDefinition;
use crate::perf_harness::{
    self, perf_harness_advance_script, perf_harness_exit_when_done, setup_perf_harness,
};
use crate::render::{RenderCache, draw_spiral_cells, setup_render_assets, sync_piece_materials};
use crate::sim_worker::SimulationBridge;
use crate::ui::{UiState, ui_game_definition};
use crate::viewport::{
    ViewportState, WINDOW_HEIGHT, WINDOW_WIDTH, sync_board_camera_viewport,
    sync_simulation_to_viewport,
};

pub fn configure_app(app: &mut App) {
    app.add_plugins(
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
    .init_resource::<crate::bookmark_config::BookmarkStore>()
    .init_resource::<RenderCache>()
    .init_resource::<ViewportState>()
    .init_resource::<AppSessionCache>()
    .init_resource::<BoardExportPending>()
    .init_resource::<camera::BoardPointerState>()
    .init_resource::<camera::PendingCameraAction>();

    #[cfg(feature = "app_profile")]
    {
        app.init_resource::<crate::app_profile::AppProfileTotals>();
        app.init_resource::<crate::app_profile::AppProfileFrame>();
    }

    #[cfg(not(target_family = "wasm"))]
    app.insert_non_send_resource(BoardExportDialogState::default());
    #[cfg(target_family = "wasm")]
    app.init_resource::<BoardExportWasmJob>();

    #[cfg(target_family = "wasm")]
    app.init_resource::<crate::wasm_clipboard::WasmShareCodePaste>();

    app.add_systems(
        Startup,
        (
            disable_egui_auto_primary_context,
            setup_camera,
            load_bookmarks,
            app_session::apply_saved_app_session,
            setup_sim_worker,
            setup_render_assets,
            setup_smoke_test,
            setup_perf_harness,
        )
            .chain(),
    )
    .add_systems(EguiPrimaryContextPass, ui_game_definition);
    app.add_systems(
        EguiPrimaryContextPass,
        (
            draw_hover_placement_paths.after(ui_game_definition),
            draw_hover_attack_squares.after(ui_game_definition),
            draw_hover_forbidden_skips.after(ui_game_definition),
        ),
    );
    #[cfg(target_family = "wasm")]
    app.add_systems(Update, crate::wasm_clipboard::poll_wasm_share_code_paste);
    app.add_systems(
        PostUpdate,
        run_board_export.after(EguiPostUpdateSet::EndPass),
    )
    .add_systems(
        Update,
        (
            camera::apply_camera_actions,
            sync_piece_materials,
            smoke_test_exit,
            perf_harness_exit_when_done,
        )
            .chain(),
    );

    let post_chain = (
        sync_board_camera_viewport.before(CameraUpdateSystems),
        camera_controls.after(write_egui_wants_input_system),
        camera::camera_zoom_controls.after(write_egui_wants_input_system),
        camera::camera_pointer_controls.after(write_egui_wants_input_system),
        perf_harness_advance_script,
        sync_simulation_to_viewport.after(EguiPostUpdateSet::EndPass),
        app_session::persist_app_session.after(EguiPostUpdateSet::EndPass),
        draw_spiral_cells,
        camera::clamp_camera_zoom_to_texture_limit,
    )
        .chain();

    app.add_systems(PostUpdate, post_chain);

    #[cfg(feature = "app_profile")]
    app.add_systems(PostUpdate, app_profile_end_frame.after(draw_spiral_cells));
}

#[cfg(feature = "app_profile")]
fn app_profile_end_frame(
    mut totals: ResMut<crate::app_profile::AppProfileTotals>,
    mut frame: ResMut<crate::app_profile::AppProfileFrame>,
) {
    crate::app_profile::flush_frame(&mut totals, &mut frame);
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
        bevy::camera::visibility::RenderLayers::none(),
        Camera {
            order: 1,
            output_mode: bevy::camera::CameraOutputMode::Write {
                blend_state: Some(bevy::render::render_resource::BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
    ));
}

fn setup_sim_worker(mut commands: Commands, def: Res<GameDefinition>) {
    commands.insert_resource(SimulationBridge::spawn(def.clone(), VisitOrder::default()));
}

fn load_bookmarks(mut bookmarks: ResMut<crate::bookmark_config::BookmarkStore>) {
    bookmarks.reload();
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

fn smoke_test_exit(
    smoke: Option<Res<SmokeTest>>,
    harness: Option<Res<perf_harness::PerfHarnessRun>>,
    time: Res<Time>,
    mut exit: MessageWriter<AppExit>,
) {
    if harness.is_some() {
        return;
    }
    if smoke.is_none_or(|s| !s.enabled) {
        return;
    }
    if time.elapsed_secs() >= 2.0 {
        exit.write(AppExit::Success);
    }
}
