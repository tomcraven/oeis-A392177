//! Frame-level timings for the interactive app (feature `app_profile` only).

use bevy::prelude::Resource;

#[cfg(feature = "app_profile")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "app_profile")]
static DISPLAY_CLONE_NS: AtomicU64 = AtomicU64::new(0);

#[derive(Resource, Clone, Debug, Default)]
pub struct AppProfileTotals {
    pub frames: u64,
    /// Nanoseconds per label (same order as [`LABELS`]).
    pub ns: [u64; LABELS.len()],
}

const LABELS: [&str; 5] = [
    "sync_viewport",
    "render_raster",
    "render_image_write",
    "render_sprite_layout",
    "display_clone",
];

#[cfg(feature = "app_profile")]
mod enabled {
    use super::*;
    use bevy::platform::time::Instant;

    #[derive(Resource, Default)]
    pub struct AppProfileFrame {
        pub ns: [u64; LABELS.len()],
    }

    pub fn label_index(label: &'static str) -> Option<usize> {
        LABELS.iter().position(|&l| l == label)
    }

    pub fn scope<T>(label: &'static str, frame: &mut AppProfileFrame, f: impl FnOnce() -> T) -> T {
        let Some(idx) = label_index(label) else {
            return f();
        };
        let start = Instant::now();
        let out = f();
        frame.ns[idx] += start.elapsed().as_nanos() as u64;
        out
    }

    pub fn flush_frame(totals: &mut AppProfileTotals, frame: &mut AppProfileFrame) {
        totals.frames += 1;
        for (acc, &v) in totals.ns.iter_mut().zip(frame.ns.iter()) {
            *acc += v;
        }
        frame.ns = [0; LABELS.len()];
        let clone_ns = DISPLAY_CLONE_NS.swap(0, Ordering::Relaxed);
        totals.ns[label_index("display_clone").unwrap()] += clone_ns;
    }

    pub fn note_display_clone_ns(ns: u64) {
        DISPLAY_CLONE_NS.fetch_add(ns, Ordering::Relaxed);
    }

    pub fn print_report(totals: &AppProfileTotals) {
        if totals.frames == 0 {
            eprintln!("app_profile: no frames recorded");
            return;
        }
        eprintln!(
            "app_profile\tframes\t{}\t(total_ms per label, avg_ms per frame)",
            totals.frames
        );
        for (label, &ns) in LABELS.iter().zip(totals.ns.iter()) {
            let total_ms = ns as f64 / 1e6;
            let avg_ms = total_ms / totals.frames as f64;
            eprintln!("app_profile\t{label}\t{total_ms:.3}\t{avg_ms:.4}");
        }
    }
}

#[cfg(not(feature = "app_profile"))]
mod enabled {
    use super::*;

    #[derive(Resource, Default)]
    pub struct AppProfileFrame;

    pub fn scope<T>(_label: &'static str, _frame: &mut AppProfileFrame, f: impl FnOnce() -> T) -> T {
        f()
    }

    pub fn flush_frame(_totals: &mut AppProfileTotals, _frame: &mut AppProfileFrame) {}

    pub fn note_display_clone_ns(_ns: u64) {}

    pub fn print_report(_totals: &AppProfileTotals) {}
}

pub use enabled::{flush_frame, note_display_clone_ns, print_report, scope, AppProfileFrame};

pub fn labels() -> &'static [&'static str] {
    &LABELS
}
