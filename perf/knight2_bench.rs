//! Release benchmark / checksum for knight_2_pairwise vs perf/python_reference.py.

use std::env;
use std::io::{self, Write};
use std::time::Instant;

use red_black_knights::index_order::VisitOrder;
use red_black_knights::model::GameDefinition;
use red_black_knights::sim::Simulation;

fn placement_checksum(placements: &[(u32, usize)]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &(index, army_id) in placements {
        let value = ((index as u64) << 8) ^ army_id as u64;
        hash ^= value;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn main() {
    if cfg!(debug_assertions) {
        eprintln!("warning: use `cargo run --release --bin perf_knight2` for timing");
    }

    let turns: usize = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(10_000);

    let def = GameDefinition::knight_2_pairwise();
    let start = Instant::now();
    let mut sim = Simulation::new(&def, VisitOrder::default());
    for _ in 0..turns {
        if !sim.step_turn(&def) {
            eprintln!(
                "simulation stopped early at {} placements",
                sim.placements.len()
            );
            std::process::exit(1);
        }
    }
    let elapsed_s = start.elapsed().as_secs_f64();
    let checksum = placement_checksum(
        &sim.placements
            .iter()
            .map(|&(i, a)| (i, a))
            .collect::<Vec<_>>(),
    );

    let black_last = sim
        .placements
        .iter()
        .rev()
        .find_map(|&(i, a)| (a == 0).then_some(i));
    let red_last = sim
        .placements
        .iter()
        .rev()
        .find_map(|&(i, a)| (a == 1).then_some(i));

    let json = format!(
        r#"{{"engine":"rust","preset":"knight_2_pairwise","turns":{turns},"placements":{turns},"checksum":{checksum},"elapsed_s":{elapsed_s:.9},"black_last_index":{black_last},"red_last_index":{red_last}}}"#,
        black_last = black_last.unwrap_or(0),
        red_last = red_last.unwrap_or(0),
    );
    io::stdout().write_all(json.as_bytes()).unwrap();
    io::stdout().write_all(b"\n").unwrap();
}
