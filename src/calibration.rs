use std::collections::VecDeque;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::calibration_config::{self, CalibrationSettings};
use crate::camera::BoardCamera;
use crate::camera::PendingCameraAction;
use crate::model::GameDefinition;
use crate::render::RenderCache;
use crate::sim_worker::SimulationBridge;
use crate::viewport::{self, ViewportState};

/// Side panel width used while benchmarking so limits match normal play.
pub const CALIBRATION_LEFT_INSET_PX: f32 = 320.0;

/// Steady-state frame budget (~45 fps mean; every sample frame must stay under spike cap).
const FRAME_BUDGET_SECS: f32 = 1.0 / 45.0;
const FRAME_SPIKE_BUDGET_SECS: f32 = 1.0 / 28.0;
const SAMPLE_HOLD_RAMP_SECS: f32 = 0.2;
const STEADY_WARMUP_RAMP_SECS: f32 = 0.2;
const STEADY_AFTER_LOAD_RAMP_SECS: f32 = 0.3;
const FRAME_SAMPLE_COUNT_RAMP: usize = 8;
const MIN_PROBE_SCALE: f32 = 1.0;
const SEARCH_PRECISION_FRAC: f32 = 0.05;
const SEARCH_PRECISION_MIN: f32 = 1.0;
const MIN_PROBE_DWELL_RAMP_SECS: f32 = 0.45;
const FLAT_TIMING_DT_RATIO: f32 = 1.12;
const FLAT_CEILING_EXPONENT: f32 = 0.8;
const CALIBRATION_PROBE_CEILING: f32 = 8192.0;
/// Fixed low zoom ladder — never jump straight to huge grids during ramp.
const RAMP_PROBE_LEVELS: [f32; 3] = [1.0, 2.5, 6.0];
const MAX_RAMP_LEVELS: usize = RAMP_PROBE_LEVELS.len();
const MAX_REFINE_PROBES: u32 = 1;
/// Skip testing another ramp level when prediction is already below this × next level.
const RAMP_EARLY_STOP_RATIO: f32 = 0.9;
const MAX_MEASURE_WALL_SECS: f32 = 1.25;

#[derive(Resource, Clone, Copy)]
pub struct UserMaxZoomOut {
    pub scale: f32,
}

impl Default for UserMaxZoomOut {
    fn default() -> Self {
        Self { scale: f32::MAX }
    }
}

#[derive(Resource)]
pub struct CalibrationGate {
    pub running: bool,
    pub best_so_far: Option<f32>,
    pub search_progress: f32,
    pub message: String,
    /// Max wall time per zoom probe (from `calibration.toml`, configurable in UI).
    pub probe_time_budget_secs: f32,
    pending_recalibrate: bool,
    phase: Phase,
}

impl Default for CalibrationGate {
    fn default() -> Self {
        Self {
            running: false,
            best_so_far: None,
            search_progress: 0.0,
            message: String::new(),
            probe_time_budget_secs: calibration_config::DEFAULT_PROBE_TIME_BUDGET_SECS,
            pending_recalibrate: false,
            phase: Phase::Idle,
        }
    }
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Search {
        fixed_cap: f32,
        stage: SearchStage,
        probe_scale: f32,
        scale_held_since: f32,
        sample_started: f32,
        last_good: f32,
        fail_hi: f32,
        refine_lo: f32,
        refine_hi: f32,
        refine_bracket: f32,
        frame_times: VecDeque<f32>,
        timing_samples: Vec<(f32, f32)>,
        ramp_level: usize,
        probes_done: u32,
        refine_probes_done: u32,
        status: SearchStatus,
        load_ready_since: Option<f32>,
        /// Wall time when sim + grid first matched this probe (preset-style load complete).
        load_ready_at: Option<f32>,
        probe_started_at: f32,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SearchStage {
    Ramp,
    Refine,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchStatus {
    Settling,
    Measuring,
}

impl CalibrationGate {
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Hide the modal overlay while sampling FPS so it matches normal play.
    pub fn overlay_visible(&self) -> bool {
        if !self.running {
            return false;
        }
        !matches!(
            &self.phase,
            Phase::Search {
                status: SearchStatus::Measuring,
                ..
            }
        )
    }

    pub fn request_recalibrate(&mut self) {
        self.pending_recalibrate = true;
    }

    fn start_running(&mut self) {
        self.running = true;
        self.best_so_far = None;
        self.search_progress = 0.0;
        self.message = format!(
            "Finding max zoom (≤{:.1}s load per probe)…",
            self.probe_time_budget_secs
        );
        self.phase = Phase::Search {
            fixed_cap: 0.0,
            stage: SearchStage::Ramp,
            probe_scale: MIN_PROBE_SCALE,
            scale_held_since: 0.0,
            sample_started: 0.0,
            last_good: MIN_PROBE_SCALE,
            fail_hi: f32::MAX,
            refine_lo: MIN_PROBE_SCALE,
            refine_hi: MIN_PROBE_SCALE,
            refine_bracket: 0.0,
            frame_times: VecDeque::new(),
            timing_samples: Vec::new(),
            ramp_level: 0,
            probes_done: 0,
            refine_probes_done: 0,
            status: SearchStatus::Settling,
            load_ready_since: None,
            load_ready_at: None,
            probe_started_at: 0.0,
        };
        info!(
            "calibration: run started probe_time_budget_secs={:.2}",
            self.probe_time_budget_secs
        );
    }

    fn finish(&mut self, max_scale: f32, user_max: &mut UserMaxZoomOut) {
        let max_scale = max_scale.max(MIN_PROBE_SCALE);
        user_max.scale = max_scale;
        let _ = calibration_config::save(&CalibrationSettings {
            max_zoom_out_scale: max_scale,
            probe_time_budget_secs: self.probe_time_budget_secs,
        });
        self.running = false;
        self.best_so_far = Some(max_scale);
        self.search_progress = 1.0;
        self.message = format!("Calibration complete (max zoom scale {max_scale:.2}).");
        self.phase = Phase::Idle;
        info!("calibration: finished saved_max_zoom={max_scale:.2}");
    }
}

fn log_probe_start(stage: SearchStage, scale: f32, fixed_cap: f32) {
    info!(
        "calibration: probe start stage={stage:?} zoom={scale:.2} (cap={fixed_cap:.2})",
    );
}

fn log_sample_result(
    stage: SearchStage,
    scale: f32,
    passed: bool,
    mean_dt: f32,
    load_secs: f32,
    probe_budget: f32,
    timing_samples: &[(f32, f32)],
) {
    let fps = if mean_dt > 0.0 { 1.0 / mean_dt } else { 0.0 };
    info!(
        "calibration: sample stage={stage:?} zoom={scale:.2} pass={passed} load_s={load_secs:.2} budget_s={probe_budget:.2} mean_ms={:.1} fps={fps:.0} budget_ms={:.1} n_timing={}",
        mean_dt * 1000.0,
        FRAME_BUDGET_SECS * 1000.0,
        timing_samples.len()
    );
}

fn next_ramp_level_index(current_level: usize) -> Option<usize> {
    if current_level + 1 >= MAX_RAMP_LEVELS {
        return None;
    }
    Some(current_level + 1)
}

fn ramp_level_scale(level: usize, fixed_cap: f32) -> f32 {
    RAMP_PROBE_LEVELS
        .get(level)
        .copied()
        .unwrap_or(*RAMP_PROBE_LEVELS.last().unwrap())
        .min(fixed_cap)
}

fn virtual_fail_hi_after_ramp(last_good: f32, fixed_cap: f32, level: usize) -> f32 {
    if let Some(next) = next_ramp_level_index(level) {
        ramp_level_scale(next, fixed_cap).max(last_good)
    } else {
        (last_good * 2.0).min(fixed_cap).max(last_good + 1.0)
    }
}

fn smoke_test_mode() -> bool {
    std::env::args().any(|a| a == "--smoke-test")
}

pub fn setup_calibration(
    mut gate: ResMut<CalibrationGate>,
    mut user_max: ResMut<UserMaxZoomOut>,
) {
    if smoke_test_mode() {
        user_max.scale = f32::MAX;
        return;
    }
    if let Some(settings) = calibration_config::load() {
        user_max.scale = settings.max_zoom_out_scale;
        gate.probe_time_budget_secs = settings.probe_time_budget_secs;
        gate.message = format!(
            "Loaded calibration (max zoom {:.2}, probe budget {:.1}s).",
            settings.max_zoom_out_scale, settings.probe_time_budget_secs
        );
        return;
    }
    gate.probe_time_budget_secs = calibration_config::DEFAULT_PROBE_TIME_BUDGET_SECS;
    gate.start_running();
}

pub fn handle_recalibrate_requests(mut gate: ResMut<CalibrationGate>) {
    if !gate.pending_recalibrate {
        return;
    }
    gate.pending_recalibrate = false;
    gate.start_running();
}

pub fn calibration_inactive(gate: Res<CalibrationGate>) -> bool {
    !gate.is_running()
}

fn push_frame_sample(samples: &mut VecDeque<f32>, dt: f32, max_len: usize) {
    if dt > 0.0 {
        samples.push_back(dt);
    }
    while samples.len() > max_len {
        samples.pop_front();
    }
}

fn mean_frame_time(samples: &VecDeque<f32>) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().sum::<f32>() / samples.len() as f32
}

fn frames_within_budget(samples: &VecDeque<f32>, sample_count: usize) -> bool {
    if samples.len() < sample_count {
        return false;
    }
    let mean = mean_frame_time(samples);
    mean <= FRAME_BUDGET_SECS
        && samples
            .iter()
            .all(|&dt| dt > 0.0 && dt <= FRAME_SPIKE_BUDGET_SECS)
}

fn load_secs_at_probe(load_ready_at: Option<f32>, probe_started_at: f32) -> f32 {
    load_ready_at
        .map(|t| t - probe_started_at)
        .unwrap_or(f32::INFINITY)
}

/// Ramp levels match preset-style load time; refine still uses steady FPS.
fn probe_passed(
    stage: SearchStage,
    fps_passed: bool,
    load_secs: f32,
    probe_budget: f32,
) -> bool {
    match stage {
        SearchStage::Ramp => {
            let load_ok = load_secs <= probe_budget;
            load_ok && (fps_passed || load_secs <= probe_budget * 0.92)
        }
        SearchStage::Refine => fps_passed,
    }
}

#[derive(Clone, Copy)]
struct ProbeTiming {
    warmup_secs: f32,
    after_load_secs: f32,
    dwell_secs: f32,
    hold_secs: f32,
    sample_count: usize,
}

fn probe_timing(stage: SearchStage, wall_budget_secs: f32) -> ProbeTiming {
    let mut t = match stage {
        SearchStage::Ramp => ProbeTiming {
            warmup_secs: STEADY_WARMUP_RAMP_SECS,
            after_load_secs: STEADY_AFTER_LOAD_RAMP_SECS,
            dwell_secs: MIN_PROBE_DWELL_RAMP_SECS,
            hold_secs: SAMPLE_HOLD_RAMP_SECS,
            sample_count: FRAME_SAMPLE_COUNT_RAMP,
        },
        SearchStage::Refine => ProbeTiming {
            warmup_secs: STEADY_WARMUP_RAMP_SECS,
            after_load_secs: STEADY_AFTER_LOAD_RAMP_SECS,
            dwell_secs: MIN_PROBE_DWELL_RAMP_SECS,
            hold_secs: SAMPLE_HOLD_RAMP_SECS,
            sample_count: FRAME_SAMPLE_COUNT_RAMP,
        },
    };
    let measure = t.dwell_secs + t.hold_secs + 0.08;
    let setup = t.warmup_secs + t.after_load_secs;
    if setup + measure > wall_budget_secs {
        t.warmup_secs = (wall_budget_secs * 0.12).clamp(0.05, t.warmup_secs);
        t.after_load_secs = (wall_budget_secs * 0.18).clamp(0.05, t.after_load_secs);
        let remaining = (wall_budget_secs - t.warmup_secs - t.after_load_secs - 0.05).max(0.1);
        t.dwell_secs = (remaining * 0.65).min(t.dwell_secs);
        t.hold_secs = (remaining * 0.35).min(t.hold_secs);
        t.sample_count = if remaining < 0.35 {
            4
        } else {
            FRAME_SAMPLE_COUNT_RAMP
        };
    }
    t
}

fn measurement_ready(viewport: &ViewportState, cache: &RenderCache) -> bool {
    let Some(bounds) = viewport.bounds else {
        return false;
    };
    if viewport.simulation_pending {
        return false;
    }
    cache.rendered_bounds == Some(bounds)
}

enum SteadySample {
    Wait,
    Ready,
}

fn steady_sample_gate(
    viewport: &ViewportState,
    cache: &RenderCache,
    now: f32,
    scale_held_since: f32,
    load_ready_since: &mut Option<f32>,
    timing: ProbeTiming,
) -> SteadySample {
    if scale_held_since <= 0.0 || now - scale_held_since < timing.warmup_secs {
        *load_ready_since = None;
        return SteadySample::Wait;
    }
    if !measurement_ready(viewport, cache) {
        *load_ready_since = None;
        return SteadySample::Wait;
    }
    if load_ready_since.is_none() {
        *load_ready_since = Some(now);
    }
    let since_load = now - load_ready_since.unwrap_or(now);
    if since_load < timing.after_load_secs {
        return SteadySample::Wait;
    }
    SteadySample::Ready
}

fn search_upper_bound(gpu_cap: f32) -> f32 {
    gpu_cap.min(CALIBRATION_PROBE_CEILING).max(MIN_PROBE_SCALE)
}

fn timing_samples_flat(samples: &[(f32, f32)]) -> bool {
    samples.len() >= 2
        && samples
            .windows(2)
            .all(|w| w[1].1 <= w[0].1 * FLAT_TIMING_DT_RATIO)
}

/// Raise the saved cap when probes show flat FPS (zoom cost not showing up in steady samples yet).
fn compute_raw_max_zoom(verified_pass: f32, timing_samples: &[(f32, f32)], fixed_cap: f32) -> f32 {
    let mut raw = verified_pass.min(fixed_cap);
    if timing_samples.is_empty() {
        return raw;
    }
    if timing_samples_flat(timing_samples) {
        let (s_max, t_min) = timing_samples.iter().fold((0.0f32, f32::MAX), |acc, &(s, t)| {
            (acc.0.max(s), acc.1.min(t))
        });
        let headroom = (FRAME_BUDGET_SECS / t_min.max(1e-6)).max(1.0);
        let extrapolated = (s_max * headroom.powf(FLAT_CEILING_EXPONENT)).min(fixed_cap);
        if extrapolated > raw + 0.05 {
            info!(
                "calibration: flat steady FPS at zoom {s_max:.2} ({:.1} ms) → raw cap {extrapolated:.2} (gpu cap {fixed_cap:.2})",
                t_min * 1000.0
            );
        }
        raw = raw.max(extrapolated);
    } else {
        let predicted = predict_budget_scale(timing_samples, FRAME_BUDGET_SECS).min(fixed_cap);
        raw = raw.max(predicted);
    }
    raw.min(fixed_cap)
}

fn bracket_is_narrow(lo: f32, hi: f32, bracket_start: f32) -> bool {
    let width = (hi - lo).max(0.0);
    let abs_ok = width <= SEARCH_PRECISION_MIN;
    let rel_ok = bracket_start > 0.0 && width / bracket_start <= SEARCH_PRECISION_FRAC;
    abs_ok || rel_ok
}

/// Predict zoom scale where mean frame time would hit the budget, from ramp samples (scale, mean dt).
fn predict_budget_scale(samples: &[(f32, f32)], budget_secs: f32) -> f32 {
    let Some(&(s_last, t_last)) = samples.last() else {
        return MIN_PROBE_SCALE;
    };
    if samples.len() < 2 {
        return s_last;
    }
    let (s1, t1) = samples[samples.len() - 2];
    let (s2, t2) = (s_last, t_last);
    if s2 <= s1 + f32::EPSILON {
        return s2;
    }

    // Frame time flat or lower at higher zoom — cost hasn't caught up yet; extrapolate from headroom.
    if t2 <= t1 * 1.08 {
        let headroom = (budget_secs / t2.max(1e-6)).max(1.0);
        let scale_factor = headroom.powf(0.55);
        let estimate = s2 * scale_factor;
        info!(
            "calibration: predict headroom scale={s2:.2} dt_ms={:.1} → estimate={estimate:.2}",
            t2 * 1000.0
        );
        return estimate;
    }

    let ratio_s = s2 / s1;
    let ratio_t = t2 / t1;
    if ratio_s <= 1.0 || ratio_t <= 1.0 {
        return s2;
    }

    // mean dt ~ k * scale^p  →  p from last ramp step (often ~1.5–2 for grid fill).
    let p = ratio_t.ln() / ratio_s.ln();
    if p.is_finite() && p > 0.15 {
        let k = t1 / s1.powf(p);
        if k > 0.0 {
            let target = (budget_secs / k).powf(1.0 / p);
            if target.is_finite() && target > 0.0 {
                return target;
            }
        }
    }

    // Linear extrapolation fallback.
    let slope = (t2 - t1) / (s2 - s1);
    if slope > f32::EPSILON {
        return (s2 + (budget_secs - t2) / slope).max(s1);
    }
    s2
}

fn begin_probe_at(
    probe: f32,
    now: f32,
    probe_scale: &mut f32,
    scale_held_since: &mut f32,
    sample_started: &mut f32,
    frame_times: &mut VecDeque<f32>,
    status: &mut SearchStatus,
    load_ready_since: &mut Option<f32>,
    load_ready_at: &mut Option<f32>,
    probe_started_at: &mut f32,
) {
    *probe_scale = probe;
    *scale_held_since = now;
    *sample_started = now;
    *probe_started_at = now;
    frame_times.clear();
    *load_ready_since = None;
    *load_ready_at = None;
    *status = SearchStatus::Settling;
}

fn begin_probe_at_logged(
    stage: SearchStage,
    fixed_cap: f32,
    probe: f32,
    now: f32,
    probe_scale: &mut f32,
    scale_held_since: &mut f32,
    sample_started: &mut f32,
    frame_times: &mut VecDeque<f32>,
    status: &mut SearchStatus,
    load_ready_since: &mut Option<f32>,
    load_ready_at: &mut Option<f32>,
    probe_started_at: &mut f32,
) {
    log_probe_start(stage, probe.min(fixed_cap), fixed_cap);
    begin_probe_at(
        probe,
        now,
        probe_scale,
        scale_held_since,
        sample_started,
        frame_times,
        status,
        load_ready_since,
        load_ready_at,
        probe_started_at,
    );
}

fn apply_probe_scale(
    probe_scale: f32,
    max_scale: f32,
    now: f32,
    scale_held_since: &mut f32,
    camera_q: &mut Query<(&mut Transform, &mut Projection), With<BoardCamera>>,
    camera_actions: &mut PendingCameraAction,
    viewport: &mut ViewportState,
    cache: &mut RenderCache,
    sim: &mut SimulationBridge,
    def: &GameDefinition,
) {
    let scale = probe_scale.min(max_scale);
    let mut scale_changed = false;
    if let Ok((mut transform, mut projection)) = camera_q.single_mut() {
        if let Projection::Orthographic(ref mut ortho) = *projection {
            if (ortho.scale - scale).abs() > f32::EPSILON {
                scale_changed = true;
                ortho.scale = scale;
            }
        }
        transform.translation = viewport::grid_to_world(0, 0).extend(transform.translation.z);
    }
    camera_actions.center_view = false;
    viewport.left_inset_px = CALIBRATION_LEFT_INSET_PX;
    if scale_changed {
        *scale_held_since = now;
        sim.request_reset(def.clone());
        viewport.bounds = None;
        viewport.target_index = 0;
        viewport.allow_sim_catchup_immediately();
        viewport.simulation_pending = false;
        viewport.render_dirty = true;
        cache.rendered_bounds = None;
    }
    if *scale_held_since <= 0.0 {
        *scale_held_since = now;
    }
}

fn ramp_progress(ramp_level: usize) -> f32 {
    (ramp_level as f32 / MAX_RAMP_LEVELS as f32 * 0.55).clamp(0.0, 0.55)
}

fn refine_progress(lo: f32, hi: f32, bracket: f32) -> f32 {
    if bracket <= f32::EPSILON {
        return 1.0;
    }
    let remaining = (hi - lo).max(0.0);
    0.55 + (1.0 - remaining / bracket).clamp(0.0, 1.0) * 0.45
}

fn start_refine_from_ramp(
    last_good: f32,
    fail_hi: f32,
    timing_samples: &[(f32, f32)],
    fixed_cap: f32,
    now: f32,
    probe_scale: &mut f32,
    refine_lo: &mut f32,
    refine_hi: &mut f32,
    refine_bracket: &mut f32,
    refine_probes_done: &mut u32,
    stage: &mut SearchStage,
    scale_held_since: &mut f32,
    sample_started: &mut f32,
    frame_times: &mut VecDeque<f32>,
    status: &mut SearchStatus,
    load_ready_since: &mut Option<f32>,
    load_ready_at: &mut Option<f32>,
    probe_started_at: &mut f32,
) {
    let hi = fail_hi.min(fixed_cap).max(last_good);
    let lo = last_good;
    let guess = predict_budget_scale(timing_samples, FRAME_BUDGET_SECS).clamp(lo, hi);
    let probe = if guess <= lo * 1.08 || guess <= lo + SEARCH_PRECISION_MIN * 0.25 {
        (lo + hi) * 0.5
    } else {
        guess.min(lo * 8.0).min(48.0)
    };
    *refine_lo = lo;
    *refine_hi = hi;
    *refine_bracket = (hi - lo).max(SEARCH_PRECISION_MIN);
    *stage = SearchStage::Refine;
    *refine_probes_done = 0;
    info!(
        "calibration: refine start lo={lo:.2} hi={hi:.2} guess={probe:.2} (predict={guess:.2})"
    );
    begin_probe_at_logged(
        SearchStage::Refine,
        fixed_cap,
        probe,
        now,
        probe_scale,
        scale_held_since,
        sample_started,
        frame_times,
        status,
        load_ready_since,
        load_ready_at,
        probe_started_at,
    );
}

struct StepAdvance {
    finish: Option<f32>,
    enter_refine: bool,
    next_ramp_probe: Option<f32>,
    refine_next: Option<f32>,
}

fn advance_from_outcome(outcome: &ProbeOutcome, stage: SearchStage) -> StepAdvance {
    StepAdvance {
        finish: outcome.finish,
        enter_refine: outcome.finish.is_none()
            && outcome.next_probe.is_none()
            && stage == SearchStage::Ramp,
        next_ramp_probe: if stage == SearchStage::Ramp {
            outcome.next_probe
        } else {
            None
        },
        refine_next: if stage == SearchStage::Refine {
            outcome.next_probe
        } else {
            None
        },
    }
}

#[derive(Clone, Copy)]
struct ProbeOutcome {
    finish: Option<f32>,
    next_probe: Option<f32>,
}

enum RampStep {
    EnterRefine,
    Next(f32),
}

fn ramp_step_after_pass(
    effective_probe: f32,
    fixed_cap: f32,
    ramp_level: &mut usize,
    timing_samples: &[(f32, f32)],
    fail_hi: &mut f32,
) -> RampStep {
    if *ramp_level + 1 >= MAX_RAMP_LEVELS {
        let hi = virtual_fail_hi_after_ramp(effective_probe, fixed_cap, *ramp_level);
        info!(
            "calibration: ramp ladder complete at zoom {effective_probe:.2} — extrapolate (virtual hi={hi:.2})"
        );
        *fail_hi = hi;
        return RampStep::EnterRefine;
    }

    let next_idx = *ramp_level + 1;
    let next_scale = ramp_level_scale(next_idx, fixed_cap);
    if timing_samples.len() >= 2 {
        let (_, t_last) = timing_samples[timing_samples.len() - 1];
        let comfortable = t_last < FRAME_BUDGET_SECS * 0.65;
        let pred = predict_budget_scale(timing_samples, FRAME_BUDGET_SECS);
        if !comfortable && pred < next_scale * RAMP_EARLY_STOP_RATIO {
            info!(
                "calibration: ramp early stop predict={pred:.2} — skip testing zoom {next_scale:.2}"
            );
            *fail_hi = next_scale.max(effective_probe);
            return RampStep::EnterRefine;
        }
    }

    *ramp_level = next_idx;
    RampStep::Next(next_scale)
}

fn after_probe_sample(
    stage: SearchStage,
    effective_probe: f32,
    fps_passed: bool,
    mean_dt: f32,
    load_secs: f32,
    probe_budget: f32,
    fixed_cap: f32,
    last_good: &mut f32,
    fail_hi: &mut f32,
    refine_lo: &mut f32,
    refine_hi: &mut f32,
    refine_bracket: f32,
    refine_probes_done: &mut u32,
    ramp_level: &mut usize,
    timing_samples: &mut Vec<(f32, f32)>,
) -> ProbeOutcome {
    let passed = probe_passed(stage, fps_passed, load_secs, probe_budget);
    log_sample_result(
        stage,
        effective_probe,
        passed,
        mean_dt,
        load_secs,
        probe_budget,
        timing_samples,
    );

    if passed {
        *last_good = effective_probe;
        timing_samples.push((effective_probe, mean_dt));
    }

    match stage {
        SearchStage::Ramp => {
            if !passed {
                if effective_probe <= MIN_PROBE_SCALE + f32::EPSILON {
                    return ProbeOutcome {
                        finish: Some(MIN_PROBE_SCALE),
                        next_probe: None,
                    };
                }
                *fail_hi = effective_probe;
                info!(
                    "calibration: ramp fail at zoom {effective_probe:.2} — refine between {:.2} and {effective_probe:.2}",
                    *last_good
                );
                return ProbeOutcome {
                    finish: None,
                    next_probe: None,
                };
            }
            if effective_probe >= fixed_cap * 0.995 {
                return ProbeOutcome {
                    finish: Some(*last_good),
                    next_probe: None,
                };
            }
            match ramp_step_after_pass(
                effective_probe,
                fixed_cap,
                ramp_level,
                timing_samples,
                fail_hi,
            ) {
                RampStep::EnterRefine => ProbeOutcome {
                    finish: None,
                    next_probe: None,
                },
                RampStep::Next(next) => ProbeOutcome {
                    finish: None,
                    next_probe: Some(next),
                },
            }
        }
        SearchStage::Refine => {
            *refine_probes_done += 1;
            if passed {
                *refine_lo = effective_probe.max(*refine_lo);
            } else {
                *refine_hi = effective_probe.min(*refine_hi);
            }
            if *refine_lo > *refine_hi {
                *refine_hi = *refine_lo;
            }
            if bracket_is_narrow(*refine_lo, *refine_hi, refine_bracket)
                || *refine_probes_done >= MAX_REFINE_PROBES
            {
                return ProbeOutcome {
                    finish: Some(*last_good),
                    next_probe: None,
                };
            }
            let mid = (*refine_lo + *refine_hi) * 0.5;
            if (mid - effective_probe).abs() < SEARCH_PRECISION_MIN * 0.25 {
                return ProbeOutcome {
                    finish: Some(*last_good),
                    next_probe: None,
                };
            }
            ProbeOutcome {
                finish: None,
                next_probe: Some(mid),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_probe_outcome(
    outcome: &ProbeOutcome,
    stage: SearchStage,
    fixed_cap: f32,
    now: f32,
    finish_scale: &mut Option<f32>,
    enter_refine: &mut bool,
    probe_scale: &mut f32,
    scale_held_since: &mut f32,
    sample_started: &mut f32,
    frame_times: &mut VecDeque<f32>,
    status: &mut SearchStatus,
    load_ready_since: &mut Option<f32>,
    load_ready_at: &mut Option<f32>,
    probe_started_at: &mut f32,
) {
    let step = advance_from_outcome(outcome, stage);
    if let Some(scale) = step.finish {
        *finish_scale = Some(scale);
    } else if step.enter_refine {
        *enter_refine = true;
    } else if let Some(next) = step.next_ramp_probe {
        begin_probe_at_logged(
            SearchStage::Ramp,
            fixed_cap,
            next,
            now,
            probe_scale,
            scale_held_since,
            sample_started,
            frame_times,
            status,
            load_ready_since,
            load_ready_at,
            probe_started_at,
        );
    } else if let Some(next) = step.refine_next {
        begin_probe_at_logged(
            SearchStage::Refine,
            fixed_cap,
            next,
            now,
            probe_scale,
            scale_held_since,
            sample_started,
            frame_times,
            status,
            load_ready_since,
            load_ready_at,
            probe_started_at,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_probe_sample(
    stage: SearchStage,
    effective_probe: f32,
    fps_passed: bool,
    mean_dt: f32,
    load_secs: f32,
    probe_budget: f32,
    fixed_cap: f32,
    now: f32,
    last_good: &mut f32,
    fail_hi: &mut f32,
    refine_lo: &mut f32,
    refine_hi: &mut f32,
    refine_bracket: f32,
    refine_probes_done: &mut u32,
    ramp_level: &mut usize,
    timing_samples: &mut Vec<(f32, f32)>,
    probes_done: &mut u32,
    probe_scale: &mut f32,
    scale_held_since: &mut f32,
    sample_started: &mut f32,
    frame_times: &mut VecDeque<f32>,
    status: &mut SearchStatus,
    load_ready_since: &mut Option<f32>,
    load_ready_at: &mut Option<f32>,
    probe_started_at: &mut f32,
    finish_scale: &mut Option<f32>,
    enter_refine: &mut bool,
    best_update: &mut Option<f32>,
) {
    *probes_done += 1;
    let passed = probe_passed(stage, fps_passed, load_secs, probe_budget);
    if passed {
        *best_update = Some(effective_probe);
    }
    let outcome = after_probe_sample(
        stage,
        effective_probe,
        fps_passed,
        mean_dt,
        load_secs,
        probe_budget,
        fixed_cap,
        last_good,
        fail_hi,
        refine_lo,
        refine_hi,
        refine_bracket,
        refine_probes_done,
        ramp_level,
        timing_samples,
    );
    dispatch_probe_outcome(
        &outcome,
        stage,
        fixed_cap,
        now,
        finish_scale,
        enter_refine,
        probe_scale,
        scale_held_since,
        sample_started,
        frame_times,
        status,
        load_ready_since,
        load_ready_at,
        probe_started_at,
    );
}

pub fn advance_calibration(
    mut gate: ResMut<CalibrationGate>,
    mut user_max: ResMut<UserMaxZoomOut>,
    time: Res<Time>,
    def: Res<GameDefinition>,
    mut sim: ResMut<SimulationBridge>,
    mut viewport: ResMut<ViewportState>,
    mut cache: ResMut<RenderCache>,
    mut camera_actions: ResMut<PendingCameraAction>,
    mut camera_q: Query<(&mut Transform, &mut Projection), With<BoardCamera>>,
    window_q: Query<&Window>,
) {
    if !gate.running {
        return;
    }

    let Ok(window) = window_q.single() else {
        return;
    };
    let Ok((_, projection)) = camera_q.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let gpu_cap = viewport::max_safe_zoom_out_scale(ortho, window, CALIBRATION_LEFT_INSET_PX);
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let probe_budget = gate.probe_time_budget_secs;

    let mut finish_scale: Option<f32> = None;
    let mut status_message: Option<String> = None;
    let mut best_update: Option<f32> = None;
    let mut progress_update: Option<f32> = None;
    let mut enter_refine = false;

    match &mut gate.phase {
        Phase::Idle => {}
        Phase::Search {
            fixed_cap,
            stage,
            probe_scale,
            scale_held_since,
            sample_started,
            last_good,
            fail_hi,
            refine_lo,
            refine_hi,
            refine_bracket,
            frame_times,
            timing_samples,
            ramp_level,
            probes_done,
            refine_probes_done,
            status,
            load_ready_since,
            load_ready_at,
            probe_started_at,
        } => {
            let timing = probe_timing(*stage, probe_budget);
            if *fixed_cap <= 0.0 {
                *fixed_cap = search_upper_bound(gpu_cap);
                info!(
                    "calibration: begin gpu_cap={gpu_cap:.2} search_cap={:.2} ramp_levels={:?} probe_budget={probe_budget:.2}s (load per zoom)",
                    *fixed_cap, RAMP_PROBE_LEVELS
                );
                *ramp_level = 0;
                begin_probe_at_logged(
                    SearchStage::Ramp,
                    *fixed_cap,
                    ramp_level_scale(0, *fixed_cap),
                    now,
                    probe_scale,
                    scale_held_since,
                    sample_started,
                    frame_times,
                    status,
                    load_ready_since,
                    load_ready_at,
                    probe_started_at,
                );
            }

            apply_probe_scale(
                *probe_scale,
                *fixed_cap,
                now,
                scale_held_since,
                &mut camera_q,
                &mut camera_actions,
                &mut *viewport,
                &mut cache,
                &mut sim,
                def.as_ref(),
            );

            if load_ready_at.is_none() && measurement_ready(&viewport, &cache) {
                *load_ready_at = Some(now);
            }

            let effective_probe = (*probe_scale).min(*fixed_cap);
            let load_secs = load_secs_at_probe(*load_ready_at, *probe_started_at);

            let stage_label = match *stage {
                SearchStage::Ramp => format!(
                    "Quick ramp ({}/{MAX_RAMP_LEVELS} levels: {:?})",
                    *ramp_level + 1,
                    RAMP_PROBE_LEVELS
                ),
                SearchStage::Refine => "One refine probe".into(),
            };
            let phase_label = match *status {
                SearchStatus::Settling if !measurement_ready(&viewport, &cache) => {
                    "Waiting for sim + grid (preset-style load)"
                }
                SearchStatus::Settling => "Settling at zoom",
                SearchStatus::Measuring => "Measuring FPS at zoom",
            };
            status_message = Some(format!(
                "{stage_label}: {phase_label} {:.2} (best ≤ {:.2}, load {load_secs:.1}/{probe_budget:.1}s)…",
                effective_probe, *last_good
            ));

            progress_update = Some(match *stage {
                SearchStage::Ramp => ramp_progress(*ramp_level),
                SearchStage::Refine => refine_progress(*refine_lo, *refine_hi, *refine_bracket),
            });

            if *status == SearchStatus::Settling
                && load_ready_at.is_none()
                && now - *probe_started_at >= probe_budget
            {
                info!(
                    "calibration: load TIMEOUT zoom={effective_probe:.2} load>{probe_budget:.2}s — failing probe"
                );
                status_message = Some(format!(
                    "Load exceeded {probe_budget:.1}s at zoom {effective_probe:.2}…"
                ));
                finish_probe_sample(
                    *stage,
                    effective_probe,
                    false,
                    probe_budget,
                    load_secs,
                    probe_budget,
                    *fixed_cap,
                    now,
                    last_good,
                    fail_hi,
                    refine_lo,
                    refine_hi,
                    *refine_bracket,
                    refine_probes_done,
                    ramp_level,
                    timing_samples,
                    probes_done,
                    probe_scale,
                    scale_held_since,
                    sample_started,
                    frame_times,
                    status,
                    load_ready_since,
                    load_ready_at,
                    probe_started_at,
                    &mut finish_scale,
                    &mut enter_refine,
                    &mut best_update,
                );
            } else {
                match steady_sample_gate(
                    &viewport,
                    &cache,
                    now,
                    *scale_held_since,
                    load_ready_since,
                    timing,
                ) {
                    SteadySample::Wait => {}
                    SteadySample::Ready => {
                        if *status == SearchStatus::Settling {
                            *status = SearchStatus::Measuring;
                            *sample_started = now;
                            frame_times.clear();
                        }

                        push_frame_sample(frame_times, dt, timing.sample_count);
                        let dwell = now - *scale_held_since;
                        let sample_elapsed = now - *sample_started;
                        let measure_timed_out =
                            *status == SearchStatus::Measuring
                                && sample_elapsed >= MAX_MEASURE_WALL_SECS
                                && frame_times.len() >= 4;
                        let samples_ready = *status == SearchStatus::Measuring
                            && dwell >= timing.dwell_secs
                            && sample_elapsed >= timing.hold_secs
                            && frame_times.len() >= timing.sample_count;
                        if samples_ready || measure_timed_out {
                            let fps_passed =
                                frames_within_budget(frame_times, timing.sample_count);
                            let mean_dt = mean_frame_time(frame_times);
                            finish_probe_sample(
                                *stage,
                                effective_probe,
                                fps_passed,
                                mean_dt,
                                load_secs,
                                probe_budget,
                                *fixed_cap,
                                now,
                                last_good,
                                fail_hi,
                                refine_lo,
                                refine_hi,
                                *refine_bracket,
                                refine_probes_done,
                                ramp_level,
                                timing_samples,
                                probes_done,
                                probe_scale,
                                scale_held_since,
                                sample_started,
                                frame_times,
                                status,
                                load_ready_since,
                                load_ready_at,
                                probe_started_at,
                                &mut finish_scale,
                                &mut enter_refine,
                                &mut best_update,
                            );
                        }
                    }
                }
            }
        }
    }

    if enter_refine {
        if let Phase::Search {
            fixed_cap,
            stage,
            probe_scale,
            last_good,
            fail_hi,
            refine_lo,
            refine_hi,
            refine_bracket,
            timing_samples,
            scale_held_since,
            sample_started,
            frame_times,
            status,
            load_ready_since,
            load_ready_at,
            refine_probes_done,
            probe_started_at,
            ..
        } = &mut gate.phase
        {
            start_refine_from_ramp(
                *last_good,
                *fail_hi,
                timing_samples,
                *fixed_cap,
                now,
                probe_scale,
                refine_lo,
                refine_hi,
                refine_bracket,
                refine_probes_done,
                stage,
                scale_held_since,
                sample_started,
                frame_times,
                status,
                load_ready_since,
                load_ready_at,
                probe_started_at,
            );
        }
    }

    if let Some(msg) = status_message {
        gate.message = msg;
    }
    if let Some(v) = best_update {
        gate.best_so_far = Some(v);
    }
    if let Some(p) = progress_update {
        gate.search_progress = p;
    }
    if let Some(verified) = finish_scale {
        let raw = match &gate.phase {
            Phase::Search {
                timing_samples,
                fixed_cap,
                ..
            } => compute_raw_max_zoom(verified, timing_samples, *fixed_cap),
            _ => verified,
        };
        gate.finish(raw, &mut user_max);
    }
}

pub fn calibration_overlay(
    mut contexts: EguiContexts,
    gate: Res<CalibrationGate>,
) {
    if !gate.overlay_visible() {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::Area::new(egui::Id::new("calibration_overlay"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .inner_margin(egui::Margin::same(16))
                .show(ui, |ui| {
                    ui.heading("Calibrating");
                    ui.label(&gate.message);
                    if let Some(best) = gate.best_so_far {
                        ui.label(format!("Best smooth zoom so far: {best:.2}"));
                    }
                    ui.add(
                        egui::ProgressBar::new(gate.search_progress).text("Search progress"),
                    );
                });
        });
}
