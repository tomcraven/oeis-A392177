pub const MIN_ZOOM_OUT: f32 = 0.5;
pub const MAX_ZOOM_OUT_BUDGET: f32 = 40.0;

/// Zoom-out ceiling: at most [`MAX_ZOOM_OUT_BUDGET`], and never above GPU texture limit.
pub fn zoom_out_budget_ceiling(gpu_safe_max: f32) -> f32 {
    if gpu_safe_max.is_finite() && gpu_safe_max > 0.0 {
        MAX_ZOOM_OUT_BUDGET.min(gpu_safe_max)
    } else {
        MAX_ZOOM_OUT_BUDGET
    }
}

pub fn effective_zoom_out_max(gpu_safe_max: f32, pan_max_scale: f32) -> f32 {
    pan_max_scale.min(zoom_out_budget_ceiling(gpu_safe_max))
}

#[cfg(not(target_family = "wasm"))]
pub fn smoke_test_mode() -> bool {
    std::env::args().any(|a| a == "--smoke-test")
}

#[cfg(target_family = "wasm")]
pub fn smoke_test_mode() -> bool {
    false
}
