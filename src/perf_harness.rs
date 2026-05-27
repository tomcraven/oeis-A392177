//! Deterministic camera scripts for perf / regression scenarios (no user input).

use bevy::camera::Projection;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::calibration_config;
use crate::camera::{BoardCamera, PanCamera};
use crate::camera_config::CameraSessionConfig;
use crate::model::GameDefinition;
use crate::sim_worker::SimulationBridge;
use crate::viewport::{self, ViewportState, WINDOW_HEIGHT, WINDOW_WIDTH};

/// Synthetic window metrics matching the default app window (logical px).
#[derive(Clone, Copy, Debug)]
pub struct PerfWindowMetrics {
    pub width: f32,
    pub height: f32,
    pub scale_factor: f32,
}

impl Default for PerfWindowMetrics {
    fn default() -> Self {
        Self {
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            scale_factor: 1.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum PerfCameraStep {
    Hold { frames: u32 },
    SetCamera { x: f32, y: f32, zoom: f32 },
    PanWorld { dx: f32, dy: f32 },
    ZoomTo { scale: f32 },
    ZoomFactor { factor: f32 },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PerfScenario {
    pub name: String,
    pub preset: String,
    pub initial_camera: CameraSessionConfig,
    #[serde(default)]
    pub left_inset_px: f32,
    pub script: Vec<PerfCameraStep>,
}

impl PerfScenario {
    pub fn total_script_frames(&self) -> u32 {
        self.script
            .iter()
            .map(|step| match step {
                PerfCameraStep::Hold { frames } => *frames,
                _ => 1,
            })
            .sum()
    }
}

#[derive(Resource)]
pub struct PerfHarnessRun {
    pub scenario: PerfScenario,
    pub step_index: usize,
    pub frames_left_in_step: u32,
    pub frames_elapsed: u32,
    pub finished: bool,
}

pub fn perf_harness_mode() -> bool {
    perf_scenario_name_from_args().is_some()
}

pub fn perf_scenario_name_from_args() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--perf-scenario" {
            return args.next();
        }
        if let Some(name) = arg.strip_prefix("--perf-scenario=") {
            return Some(name.to_string());
        }
    }
    None
}

pub fn load_scenario_by_name(name: &str) -> Option<PerfScenario> {
    builtin_scenarios()
        .into_iter()
        .find(|s| s.name == name)
        .or_else(|| load_scenario_from_path(name).ok())
}

fn load_scenario_from_path(path: &str) -> Result<PerfScenario, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str(&text).map_err(|e| e.to_string())
}

pub fn builtin_scenarios() -> Vec<PerfScenario> {
    vec![
        PerfScenario {
            name: "origin_settled".into(),
            preset: "knight_2_pairwise".into(),
            initial_camera: CameraSessionConfig {
                x: 0.0,
                y: 0.0,
                zoom: 2.0,
            },
            left_inset_px: 320.0,
            script: vec![PerfCameraStep::Hold { frames: 90 }],
        },
        PerfScenario {
            name: "pan_east".into(),
            preset: "knight_2_pairwise".into(),
            initial_camera: CameraSessionConfig {
                x: 0.0,
                y: 0.0,
                zoom: 2.0,
            },
            left_inset_px: 320.0,
            script: vec![
                PerfCameraStep::Hold { frames: 30 },
                PerfCameraStep::PanWorld {
                    dx: 800.0,
                    dy: 0.0,
                },
                PerfCameraStep::Hold { frames: 60 },
            ],
        },
        PerfScenario {
            name: "zoom_out_catchup".into(),
            preset: "knight_3_clique".into(),
            initial_camera: CameraSessionConfig {
                x: 0.0,
                y: 0.0,
                zoom: 2.5,
            },
            left_inset_px: 320.0,
            script: vec![
                PerfCameraStep::Hold { frames: 45 },
                PerfCameraStep::ZoomTo { scale: 0.55 },
                PerfCameraStep::Hold { frames: 120 },
            ],
        },
        PerfScenario {
            name: "pan_render_stress".into(),
            preset: "knight_2_pairwise".into(),
            initial_camera: CameraSessionConfig {
                x: 0.0,
                y: 0.0,
                zoom: 1.8,
            },
            left_inset_px: 320.0,
            script: vec![
                PerfCameraStep::Hold { frames: 20 },
                PerfCameraStep::PanWorld { dx: 400.0, dy: 0.0 },
                PerfCameraStep::Hold { frames: 15 },
                PerfCameraStep::PanWorld { dx: 400.0, dy: 0.0 },
                PerfCameraStep::Hold { frames: 15 },
                PerfCameraStep::PanWorld { dx: -800.0, dy: 200.0 },
                PerfCameraStep::Hold { frames: 30 },
            ],
        },
    ]
}

pub fn game_definition_for_preset(name: &str) -> Option<GameDefinition> {
    Some(match name {
        "knight_2_pairwise" => GameDefinition::knight_2_pairwise(),
        "knight_3_clique" => GameDefinition::knight_3_clique(),
        "leaper_4_mixed_clique" => GameDefinition::leaper_4_mixed_clique(),
        "king_6_clique" => GameDefinition::king_6_clique(),
        "chimera_3_clique" => GameDefinition::chimera_3_clique(),
        _ => return None,
    })
}

pub fn apply_camera_session(
    saved: &CameraSessionConfig,
    query: &mut Query<(&mut Transform, &mut Projection), With<BoardCamera>>,
) {
    let Ok((mut transform, mut projection)) = query.single_mut() else {
        return;
    };
    transform.translation.x = saved.x;
    transform.translation.y = saved.y;
    if let Projection::Orthographic(ref mut ortho) = *projection {
        let cap = calibration_config::MAX_ZOOM_OUT_BUDGET;
        ortho.scale = saved.zoom.clamp(calibration_config::MIN_ZOOM_OUT, cap);
    }
}

pub fn apply_camera_step(
    step: &PerfCameraStep,
    query: &mut Query<(&mut Transform, &PanCamera, &mut Projection), With<BoardCamera>>,
) {
    let Ok((mut transform, pan, mut projection)) = query.single_mut() else {
        return;
    };
    let Projection::Orthographic(ref mut ortho) = *projection else {
        return;
    };
    match step {
        PerfCameraStep::Hold { .. } => {}
        PerfCameraStep::SetCamera { x, y, zoom } => {
            transform.translation.x = *x;
            transform.translation.y = *y;
            ortho.scale = zoom.clamp(pan.min_scale, pan.max_scale);
        }
        PerfCameraStep::PanWorld { dx, dy } => {
            transform.translation.x += dx;
            transform.translation.y += dy;
        }
        PerfCameraStep::ZoomTo { scale } => {
            ortho.scale = scale.clamp(pan.min_scale, pan.max_scale);
        }
        PerfCameraStep::ZoomFactor { factor } => {
            ortho.scale = (ortho.scale * factor).clamp(pan.min_scale, pan.max_scale);
        }
    }
}

/// Visible spiral target index for a board camera pose (same rule as the live app).
pub fn spiral_target_index_for_view(
    transform: &Transform,
    ortho: &OrthographicProjection,
    window: &PerfWindowMetrics,
    left_inset_px: f32,
) -> u32 {
    let bounds = viewport_grid_bounds_for_metrics(transform, ortho, window, left_inset_px);
    viewport::spiral_target_index_for_bounds(bounds)
}

fn viewport_grid_bounds_for_metrics(
    camera_transform: &Transform,
    ortho: &OrthographicProjection,
    _window: &PerfWindowMetrics,
    _left_inset_px: f32,
) -> viewport::GridBounds {
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
        let (gx, gy) = viewport::world_to_grid(c);
        min_x = min_x.min(gx);
        max_x = max_x.max(gx);
        min_y = min_y.min(gy);
        max_y = max_y.max(gy);
    }

    let margin = 2;
    viewport::GridBounds {
        min_x: min_x - margin,
        max_x: max_x + margin,
        min_y: min_y - margin,
        max_y: max_y + margin,
    }
}

pub fn setup_perf_harness(
    mut commands: Commands,
    mut def: ResMut<GameDefinition>,
    mut viewport: ResMut<ViewportState>,
    mut sim: ResMut<SimulationBridge>,
    mut query: Query<(&mut Transform, &mut Projection), With<BoardCamera>>,
) {
    let Some(name) = perf_scenario_name_from_args() else {
        return;
    };
    let Some(scenario) = load_scenario_by_name(&name) else {
        panic!("unknown --perf-scenario {name:?} (built-in: origin_settled, pan_east, zoom_out_catchup, pan_render_stress, or path to .toml)");
    };
    let Some(game) = game_definition_for_preset(&scenario.preset) else {
        panic!("unknown preset {:?} in perf scenario", scenario.preset);
    };
    *def = game.clone();
    sim.request_reset(game);
    viewport.left_inset_px = scenario.left_inset_px;
    viewport.render_dirty = true;
    viewport.target_index = 0;
    apply_camera_session(&scenario.initial_camera, &mut query);
    commands.insert_resource(PerfHarnessRun {
        scenario,
        step_index: 0,
        frames_left_in_step: 0,
        frames_elapsed: 0,
        finished: false,
    });
}

pub fn perf_harness_advance_script(
    harness: Option<ResMut<PerfHarnessRun>>,
    mut query: Query<(&mut Transform, &PanCamera, &mut Projection), With<BoardCamera>>,
) {
    let Some(mut harness) = harness else {
        return;
    };
    if harness.finished {
        return;
    }

    if harness.frames_left_in_step == 0 {
        if harness.step_index >= harness.scenario.script.len() {
            harness.finished = true;
            return;
        }
        let step = harness.scenario.script[harness.step_index].clone();
        harness.step_index += 1;
        harness.frames_left_in_step = match &step {
            PerfCameraStep::Hold { frames } => *frames,
            _ => {
                apply_camera_step(&step, &mut query);
                1
            }
        };
        if matches!(step, PerfCameraStep::Hold { .. }) {
            // Hold steps do not mutate the camera on entry.
        }
    }

    harness.frames_left_in_step = harness.frames_left_in_step.saturating_sub(1);
    harness.frames_elapsed += 1;
    if harness.frames_left_in_step == 0 && harness.step_index >= harness.scenario.script.len() {
        harness.finished = true;
    }
}

pub fn perf_harness_exit_when_done(
    harness: Option<Res<PerfHarnessRun>>,
    #[cfg(feature = "app_profile")] totals: Option<Res<crate::app_profile::AppProfileTotals>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(harness) = harness else {
        return;
    };
    if !harness.finished {
        return;
    }
    #[cfg(feature = "app_profile")]
    if let Some(totals) = totals {
        eprintln!("app_profile\tscenario\t{}", harness.scenario.name);
        crate::app_profile::print_report(totals.as_ref());
    }
    exit.write(AppExit::Success);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::{OrthographicProjection, ScalingMode};

    fn test_ortho(scale: f32) -> OrthographicProjection {
        OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: WINDOW_HEIGHT,
            },
            scale,
            ..OrthographicProjection::default_2d()
        }
    }

    #[test]
    fn zoom_out_raises_spiral_target_index() {
        let tight = viewport::GridBounds {
            min_x: -4,
            max_x: 4,
            min_y: -4,
            max_y: 4,
        };
        let wide = viewport::GridBounds {
            min_x: -40,
            max_x: 40,
            min_y: -40,
            max_y: 40,
        };
        let tight_idx = viewport::spiral_target_index_for_bounds(tight);
        let wide_idx = viewport::spiral_target_index_for_bounds(wide);
        assert!(
            wide_idx > tight_idx,
            "expected wider bounds to need more spiral history (tight={tight_idx}, wide={wide_idx})"
        );
    }

    #[test]
    fn pan_script_changes_target_index_deterministically() {
        let mut transform = Transform::from_xyz(0.0, 0.0, 0.0);
        let ortho = test_ortho(2.0);
        let window = PerfWindowMetrics::default();
        let inset = 320.0;
        let before = spiral_target_index_for_view(&transform, &ortho, &window, inset);
        transform.translation.x += 600.0;
        let after = spiral_target_index_for_view(&transform, &ortho, &window, inset);
        assert_ne!(before, after);
    }

    #[test]
    fn builtin_scenarios_parse_and_have_frames() {
        for scenario in builtin_scenarios() {
            assert!(game_definition_for_preset(&scenario.preset).is_some());
            assert!(scenario.total_script_frames() > 0);
            let text = toml::to_string_pretty(&scenario).unwrap();
            let back: PerfScenario = toml::from_str(&text).unwrap();
            assert_eq!(back, scenario);
        }
    }

    #[test]
    fn perf_camera_step_script_advances_frame_counts() {
        let scenario = PerfScenario {
            name: "test".into(),
            preset: "knight_2_pairwise".into(),
            initial_camera: CameraSessionConfig {
                x: 0.0,
                y: 0.0,
                zoom: 1.0,
            },
            left_inset_px: 0.0,
            script: vec![
                PerfCameraStep::Hold { frames: 3 },
                PerfCameraStep::PanWorld { dx: 1.0, dy: 0.0 },
                PerfCameraStep::Hold { frames: 2 },
            ],
        };
        assert_eq!(scenario.total_script_frames(), 6);
    }

    /// Simulates harness script stepping (no Bevy) for CI determinism.
    #[test]
    fn script_runner_matches_total_frame_count() {
        let scenario = builtin_scenarios()
            .into_iter()
            .find(|s| s.name == "pan_render_stress")
            .unwrap();
        let mut step_index = 0usize;
        let mut frames_left = 0u32;
        let mut elapsed = 0u32;
        while step_index < scenario.script.len() || frames_left > 0 {
            if frames_left == 0 {
                if step_index >= scenario.script.len() {
                    break;
                }
                let step = &scenario.script[step_index];
                step_index += 1;
                frames_left = match step {
                    PerfCameraStep::Hold { frames } => *frames,
                    _ => 1,
                };
            }
            frames_left -= 1;
            elapsed += 1;
        }
        assert_eq!(elapsed, scenario.total_script_frames());
    }

    #[test]
    fn all_builtin_scenarios_listed_for_perf_app() {
        let scenarios = builtin_scenarios();
        let names: Vec<_> = scenarios.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"origin_settled"));
        assert!(names.contains(&"pan_east"));
        assert!(names.contains(&"zoom_out_catchup"));
        assert!(names.contains(&"pan_render_stress"));
    }
}
