//! Release microbench: visit-order hot paths vs square spiral baseline.
//!
//! ```bash
//! cargo rund --release --bin time_index_order
//! ```
//!
//! Env: `TIME_INDEX_ORDER_ITERS` (default 12), `TIME_INDEX_ORDER_WARMUP` (default 2),
//! `TIME_INDEX_ORDER_MAX_INDEX` (default 200_000).

use std::hint::black_box;
use std::time::{Duration, Instant};

use red_black_knights::index_order::VisitOrder;
use red_black_knights::spiral;

const DEFAULT_WARMUP: usize = 2;
const DEFAULT_ITERS: usize = 12;
const DEFAULT_MAX_INDEX: u32 = 200_000;

fn main() {
    if cfg!(debug_assertions) {
        eprintln!("warning: run with `cargo run --release --bin time_index_order`");
    }

    let warmup = env_usize("TIME_INDEX_ORDER_WARMUP", DEFAULT_WARMUP);
    let iters = env_usize("TIME_INDEX_ORDER_ITERS", DEFAULT_ITERS);
    let max_index = env_u32("TIME_INDEX_ORDER_MAX_INDEX", DEFAULT_MAX_INDEX);

    println!(
        "mode\tbench\torder\tmax_index\twarmup\titers\tmedian_ns_per_op\tmean_ns_per_op\tvs_spiral_median"
    );

    let baseline = bench_median(warmup, iters, max_index, |max| {
        bench_index_to_xy_spiral_direct(max)
    });

    for order in VisitOrder::ALL {
        let m = bench_median(warmup, iters, max_index, |max| {
            bench_index_to_xy(order, max)
        });
        print_row("index_to_xy", order, max_index, warmup, iters, m, baseline);
    }

    let baseline_xy = bench_median(warmup, iters, max_index, |max| {
        bench_xy_to_index_spiral_direct(max)
    });
    for order in VisitOrder::ALL {
        let m = bench_median(warmup, iters, max_index, |max| {
            bench_xy_to_index(order, max)
        });
        print_row(
            "xy_to_index",
            order,
            max_index,
            warmup,
            iters,
            m,
            baseline_xy,
        );
    }

    let baseline_scan = bench_median(warmup, iters, max_index, |max| {
        bench_scan_step_spiral_direct(max)
    });
    for order in VisitOrder::ALL {
        let m = bench_median(warmup, iters, max_index, |max| bench_scan_step(order, max));
        print_row(
            "scan_step_xy",
            order,
            max_index,
            warmup,
            iters,
            m,
            baseline_scan,
        );
    }

    let baseline_mix = bench_median(warmup, iters, max_index, |max| {
        bench_mixed_place_spiral_direct(max)
    });
    for order in VisitOrder::ALL {
        let m = bench_median(warmup, iters, max_index, |max| {
            bench_mixed_place(order, max)
        });
        print_row(
            "mixed_scan_place",
            order,
            max_index,
            warmup,
            iters,
            m,
            baseline_mix,
        );
    }
}

fn print_row(
    bench: &str,
    order: VisitOrder,
    max_index: u32,
    warmup: usize,
    iters: usize,
    median_ns: f64,
    baseline_ns: f64,
) {
    let ratio = median_ns / baseline_ns;
    println!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{:.2}\t{:.3}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        bench,
        order.label(),
        max_index,
        warmup,
        iters,
        median_ns,
        median_ns,
        ratio,
    );
}

fn bench_median(
    warmup: usize,
    iters: usize,
    max_index: u32,
    mut f: impl FnMut(u32) -> Duration,
) -> f64 {
    for _ in 0..warmup {
        black_box(f(max_index));
    }
    let mut samples: Vec<f64> = (0..iters)
        .map(|_| {
            let d = f(max_index);
            ns_per_op(d, max_index)
        })
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}

fn ns_per_op(d: Duration, ops: u32) -> f64 {
    d.as_secs_f64() * 1e9 / ops as f64
}

/// Sequential index→xy (forbidden tail skip, cursor refresh).
fn bench_index_to_xy(order: VisitOrder, max_index: u32) -> Duration {
    let start = Instant::now();
    let mut acc = 0i32;
    for i in 0..max_index {
        let (x, y) = order.index_to_xy(i);
        acc = acc.wrapping_add(x).wrapping_add(y);
    }
    black_box(acc);
    start.elapsed()
}

fn bench_index_to_xy_spiral_direct(max_index: u32) -> Duration {
    let start = Instant::now();
    let mut acc = 0i32;
    for i in 0..max_index {
        let (x, y) = spiral::index_to_xy(i);
        acc = acc.wrapping_add(x).wrapping_add(y);
    }
    black_box(acc);
    start.elapsed()
}

fn bench_xy_to_index(order: VisitOrder, max_index: u32) -> Duration {
    let coords: Vec<(i32, i32)> = (0..max_index).map(|i| order.index_to_xy(i)).collect();
    let start = Instant::now();
    let mut acc = 0u32;
    for &(x, y) in &coords {
        acc = acc.wrapping_add(order.xy_to_index(x, y));
    }
    black_box(acc);
    start.elapsed()
}

fn bench_xy_to_index_spiral_direct(max_index: u32) -> Duration {
    let coords: Vec<(i32, i32)> = (0..max_index).map(spiral::index_to_xy).collect();
    let start = Instant::now();
    let mut acc = 0u32;
    for &(x, y) in &coords {
        acc = acc.wrapping_add(spiral::xy_to_index(x, y));
    }
    black_box(acc);
    start.elapsed()
}

/// One scan rejection step (dominant when cells are rejected).
fn bench_scan_step(order: VisitOrder, max_index: u32) -> Duration {
    let start = Instant::now();
    let mut xy = (0, 0);
    for i in 0..max_index {
        xy = order.scan_step_xy(i, black_box(xy));
        black_box(i);
    }
    black_box(xy);
    start.elapsed()
}

fn bench_scan_step_spiral_direct(max_index: u32) -> Duration {
    let start = Instant::now();
    let mut xy = (0, 0);
    for i in 0..max_index {
        xy = spiral::spiral_step(xy);
        black_box(i);
    }
    black_box(xy);
    start.elapsed()
}

/// Scan step + knight attack indexing (8 xy_to_index), rough place-path mix.
const KNIGHT_OFFSETS: [(i32, i32); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];

fn bench_mixed_place(order: VisitOrder, max_index: u32) -> Duration {
    let start = Instant::now();
    let mut xy = (0, 0);
    let mut acc = 0u32;
    for i in 0..max_index {
        xy = order.scan_step_xy(i, black_box(xy));
        let (x, y) = xy;
        for &(dx, dy) in &KNIGHT_OFFSETS {
            acc = acc.wrapping_add(order.xy_to_index(x + dx, y + dy));
        }
        black_box(i);
    }
    black_box((xy, acc));
    start.elapsed()
}

fn bench_mixed_place_spiral_direct(max_index: u32) -> Duration {
    let start = Instant::now();
    let mut xy = (0, 0);
    let mut acc = 0u32;
    for i in 0..max_index {
        xy = spiral::spiral_step(xy);
        let (x, y) = xy;
        for &(dx, dy) in &KNIGHT_OFFSETS {
            acc = acc.wrapping_add(spiral::xy_to_index(x + dx, y + dy));
        }
        black_box(i);
    }
    black_box((xy, acc));
    start.elapsed()
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
