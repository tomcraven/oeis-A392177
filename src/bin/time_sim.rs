use std::hint::black_box;
use std::time::{Duration, Instant};

use red_black_knights::model::{ArmyId, GameDefinition};
use red_black_knights::sim::Simulation;

const WARMUP_ITERS: usize = 1;
const MEASURED_ITERS: usize = 5;
const TURNS: usize = 100_000;

struct BenchCase {
    name: &'static str,
    def: fn() -> GameDefinition,
    turns: usize,
}

fn main() {
    if cfg!(debug_assertions) {
        eprintln!(
            "warning: timing harness should be run with `cargo run --release --bin time_sim`"
        );
    }

    println!(
        "mode\tcase\tturns\tplacements\tchecksum\twarmup_iters\tmeasured_iters\tbest_ms\tavg_ms"
    );

    for case in bench_cases() {
        for _ in 0..WARMUP_ITERS {
            let (placements, checksum) = run_case(&case);
            black_box((placements, checksum));
        }

        let mut total = Duration::ZERO;
        let mut best = Duration::MAX;
        let mut last_placements = 0usize;
        let mut last_checksum = 0u64;

        for _ in 0..MEASURED_ITERS {
            let start = Instant::now();
            let (placements, checksum) = run_case(&case);
            let elapsed = start.elapsed();

            total += elapsed;
            best = best.min(elapsed);
            last_placements = placements;
            last_checksum = checksum;
            black_box((placements, checksum));
        }

        let avg = total.as_secs_f64() * 1_000.0 / MEASURED_ITERS as f64;
        let best_ms = best.as_secs_f64() * 1_000.0;
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{best_ms:.3}\t{avg:.3}",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            case.name,
            case.turns,
            last_placements,
            last_checksum,
            WARMUP_ITERS,
            MEASURED_ITERS,
        );
    }
}

fn bench_cases() -> [BenchCase; 5] {
    [
        BenchCase {
            name: "red_black_knights",
            def: GameDefinition::red_black_knights,
            turns: TURNS,
        },
        BenchCase {
            name: "three_knights",
            def: GameDefinition::three_knights,
            turns: TURNS,
        },
        BenchCase {
            name: "four_classic_leapers",
            def: GameDefinition::four_classic_leapers,
            turns: TURNS,
        },
        BenchCase {
            name: "six_guards",
            def: GameDefinition::six_guards,
            turns: TURNS,
        },
        BenchCase {
            name: "fusion_piece_freeforall",
            def: GameDefinition::fusion_piece_freeforall,
            turns: TURNS,
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

fn placement_checksum(placements: &[(u32, ArmyId)]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &(index, army_id) in placements {
        let value = ((index as u64) << 8) ^ army_id as u64;
        hash ^= value;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
