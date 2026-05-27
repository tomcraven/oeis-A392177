use std::hint::black_box;
use std::time::{Duration, Instant};

use red_black_knights::model::{PieceId, GameDefinition};
use red_black_knights::sim::Simulation;

const DEFAULT_WARMUP_ITERS: usize = 2;
const DEFAULT_MEASURED_ITERS: usize = 15;
const TURNS: usize = 100_000;

#[derive(Copy, Clone)]
struct BenchCase {
    name: &'static str,
    def: fn() -> GameDefinition,
    turns: usize,
}

struct TimingStats {
    best_ms: f64,
    mean_ms: f64,
    median_ms: f64,
    stdev_ms: f64,
    max_ms: f64,
}

fn main() {
    if cfg!(debug_assertions) {
        eprintln!(
            "warning: timing harness should be run with `cargo run --release --bin time_sim`"
        );
    }

    let turns = env_usize("TIME_SIM_TURNS", TURNS);
    let warmup_iters = env_usize("TIME_SIM_WARMUP", DEFAULT_WARMUP_ITERS);
    let measured_iters = env_usize("TIME_SIM_ITERS", DEFAULT_MEASURED_ITERS);

    println!(
        "mode\tcase\tturns\tplacements\tchecksum\twarmup_iters\tmeasured_iters\tbest_ms\tmean_ms\tmedian_ms\tstdev_ms\tmax_ms"
    );

    let cases: Vec<BenchCase> = bench_cases(turns);
    for case in &cases {
        run_case_timed(case, warmup_iters, measured_iters);
    }
}

fn run_case_timed(case: &BenchCase, warmup_iters: usize, measured_iters: usize) {
    for _ in 0..warmup_iters {
        let (placements, checksum) = run_case(case);
        black_box((placements, checksum));
    }

    let mut samples = Vec::with_capacity(measured_iters);
    let mut last_placements = 0usize;
    let mut last_checksum = 0u64;

    for _ in 0..measured_iters {
        let start = Instant::now();
        let (placements, checksum) = run_case(case);
        let elapsed = start.elapsed();

        samples.push(elapsed);
        last_placements = placements;
        last_checksum = checksum;
        black_box((placements, checksum));
    }

    let stats = timing_stats(&samples);

    println!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        case.name,
        case.turns,
        last_placements,
        last_checksum,
        warmup_iters,
        measured_iters,
        stats.best_ms,
        stats.mean_ms,
        stats.median_ms,
        stats.stdev_ms,
        stats.max_ms,
    );
}

fn timing_stats(samples: &[Duration]) -> TimingStats {
    let n = samples.len() as f64;
    let mut ms: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1_000.0).collect();
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let mean_ms = ms.iter().sum::<f64>() / n;
    let median_ms = if ms.len() % 2 == 0 {
        (ms[ms.len() / 2 - 1] + ms[ms.len() / 2]) / 2.0
    } else {
        ms[ms.len() / 2]
    };
    let variance = ms.iter().map(|t| (t - mean_ms).powi(2)).sum::<f64>() / n;
    let stdev_ms = variance.sqrt();

    TimingStats {
        best_ms: ms[0],
        mean_ms,
        median_ms,
        stdev_ms,
        max_ms: ms[ms.len() - 1],
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}

fn bench_cases(turns: usize) -> Vec<BenchCase> {
    vec![
        BenchCase {
            name: "knight_2_pairwise",
            def: GameDefinition::knight_2_pairwise,
            turns,
        },
        BenchCase {
            name: "knight_3_clique",
            def: GameDefinition::knight_3_clique,
            turns,
        },
        BenchCase {
            name: "leaper_4_mixed_clique",
            def: GameDefinition::leaper_4_mixed_clique,
            turns,
        },
        BenchCase {
            name: "king_6_clique",
            def: GameDefinition::king_6_clique,
            turns,
        },
        BenchCase {
            name: "chimera_3_clique",
            def: GameDefinition::chimera_3_clique,
            turns,
        },
    ]
}

fn run_case(case: &BenchCase) -> (usize, u64) {
    let def = (case.def)();
    let mut sim = Simulation::new(&def);
    for _ in 0..case.turns {
        assert!(sim.step_turn(&def), "{} failed to step", case.name);
    }
    (sim.placements.len(), placement_checksum(&sim.placements))
}

fn placement_checksum(placements: &[(u32, PieceId)]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &(index, piece_id) in placements {
        let value = ((index as u64) << 8) ^ piece_id as u64;
        hash ^= value;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
