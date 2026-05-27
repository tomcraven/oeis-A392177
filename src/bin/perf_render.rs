//! Headless CPU raster benchmarks (no window). Use with `app_profile` when comparing app paths.

use std::time::Instant;

use red_black_knights::model::GameDefinition;
use red_black_knights::render::{grid_texture_size, raster_checksum, raster_spiral_grid};
use red_black_knights::index_order::VisitOrder;
use red_black_knights::sim::Simulation;
use red_black_knights::ui::BoardColourMode;
use red_black_knights::viewport::GridBounds;

const DEFAULT_ITERS: u32 = 15;
const WARMUP: u32 = 3;

fn main() {
    let iters = env_u32("PERF_RENDER_ITERS", DEFAULT_ITERS);
    let turns = env_usize("PERF_RENDER_TURNS", 10_000);

    println!("mode\tcase\tturns\tcells\titers\tchecksum\tmedian_ms\tmean_ms");

    bench_case(
        "knight_2_tight",
        GameDefinition::knight_2_pairwise,
        turns,
        GridBounds {
            min_x: -24,
            max_x: 24,
            min_y: -24,
            max_y: 24,
        },
        iters,
    );
    bench_case(
        "knight_3_wide",
        GameDefinition::knight_3_clique,
        turns,
        GridBounds {
            min_x: -72,
            max_x: 72,
            min_y: -72,
            max_y: 72,
        },
        iters,
    );
    bench_case(
        "king_6_wide",
        GameDefinition::king_6_clique,
        turns,
        GridBounds {
            min_x: -64,
            max_x: 64,
            min_y: -64,
            max_y: 64,
        },
        iters,
    );

    // Near GPU grid-texture limit (~16k×16k texels when zoomed out fully).
    let half = env_i32("PERF_RENDER_MAX_ZOOM_HALF", 4_096);
    bench_case(
        "max_zoom_grid",
        GameDefinition::knight_2_pairwise,
        turns,
        GridBounds {
            min_x: -half,
            max_x: half,
            min_y: -half,
            max_y: half,
        },
        iters,
    );
}

fn bench_case(
    name: &str,
    preset: fn() -> GameDefinition,
    turns: usize,
    bounds: GridBounds,
    measured_iters: u32,
) {
    let def = preset();
    let mut sim = Simulation::new(&def, VisitOrder::default());
    for _ in 0..turns {
        assert!(sim.step_turn(&def), "{name}: step failed");
    }
    let size = grid_texture_size(bounds);
    let empty = [
        31,
        31,
        41,
        255,
    ];
    let colors: Vec<[u8; 4]> = def.pieces.iter().map(|a| rgba8_for_tests_from(a.color)).collect();

    for _ in 0..WARMUP {
        let data = raster_spiral_grid(
            bounds,
            size.x,
            size.y,
            &sim.occupancy,
            &colors,
            empty,
            BoardColourMode::Piece,
        );
        std::hint::black_box(data);
    }

    let mut samples = Vec::with_capacity(measured_iters as usize);
    let mut last_checksum = 0u64;
    for _ in 0..measured_iters {
        let start = Instant::now();
        let data = raster_spiral_grid(
            bounds,
            size.x,
            size.y,
            &sim.occupancy,
            &colors,
            empty,
            BoardColourMode::Piece,
        );
        samples.push(start.elapsed());
        last_checksum = raster_checksum(&data);
    }

    let median = median_duration(&samples);
    let mean: f64 = samples.iter().map(|d| d.as_secs_f64()).sum::<f64>() / samples.len() as f64;

    println!(
        "headless\t{name}\t{turns}\t{}\t{measured_iters}\t{last_checksum}\t{:.3}\t{:.3}",
        bounds.cell_count(),
        median.as_secs_f64() * 1e3,
        mean * 1e3,
    );
}

fn median_duration(samples: &[std::time::Duration]) -> std::time::Duration {
    let mut ns: Vec<u128> = samples.iter().map(|d| d.as_nanos()).collect();
    ns.sort_unstable();
    std::time::Duration::from_nanos(ns[ns.len() / 2] as u64)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_i32(key: &str, default: i32) -> i32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn rgba8_for_tests_from(color: bevy::prelude::Color) -> [u8; 4] {
    let c = color.to_srgba();
    [
        (c.red.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.green.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.blue.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}
