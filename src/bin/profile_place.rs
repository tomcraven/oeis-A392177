use std::collections::HashSet;
use std::hint::black_box;
use std::time::Instant;

use red_black_knights::index_order::VisitOrder;
use red_black_knights::model::GameDefinition;
use red_black_knights::place_profile::{
    self, ForbiddenInsertHarness, ForbiddenInsertStats, OccupancyInsertHarness, PlaceWorkStats,
};
use red_black_knights::sim::Simulation;
use red_black_knights::spiral::{index_to_xy, xy_to_index};

const TURNS: usize = 100_000;
const LATE_PLACEMENTS: usize = 1_000;

#[derive(Clone)]
struct PlaceEvent {
    piece_id: usize,
    index: u32,
    xy: (i32, i32),
    inserts_per_placement: usize,
}

fn main() {
    if cfg!(debug_assertions) {
        eprintln!(
            "warning: run with `cargo run --release --features place_profile,bevy/dynamic_linking --bin profile_place`"
        );
    }

    place_profile::set_timing_enabled(false);
    place_profile::set_profiling_enabled(false);

    let cases: [(&str, fn() -> GameDefinition); 5] = [
        ("knight_2_pairwise", GameDefinition::knight_2_pairwise),
        ("knight_3_clique", GameDefinition::knight_3_clique),
        (
            "leaper_4_mixed_clique",
            GameDefinition::leaper_4_mixed_clique,
        ),
        ("king_6_clique", GameDefinition::king_6_clique),
        ("chimera_3_clique", GameDefinition::chimera_3_clique),
    ];

    println!(
        "case\tturns\tchecksum\tstep_ms\tplace_replay_ms\tscan_est_ms\tscan_pct\tcells_per_place\tforb_ins_per_place\tforb_combines_per_cell"
    );

    for (name, def_fn) in cases {
        let def = def_fn();
        let (checksum, work, forb_stats, events, records) = collect_run(&def);
        let step_ms = bench_full_step_turn(&def);
        let place_ms = bench_place_replay(&def, &events);
        let micro = microbench_components(&def, &events, &records, def.pieces.len());
        print_summary_row(name, checksum, step_ms, place_ms, &work);
        print_fanout_report(
            name,
            &def,
            &work,
            &forb_stats,
            &events,
            &records,
            &micro,
            place_ms,
        );
        black_box((checksum, events.len()));
    }
}

struct MicrobenchMs {
    xy_to_index: f64,
    forbidden_single_set: f64,
    forbidden_multi_set: f64,
    occupancy: f64,
}

fn threatened_targets(def: &GameDefinition, attacker: usize) -> usize {
    def.pieces
        .iter()
        .filter(|piece| piece.blocked_by.contains(&attacker))
        .count()
}

fn collect_run(
    def: &GameDefinition,
) -> (
    u64,
    PlaceWorkStats,
    ForbiddenInsertStats,
    Vec<PlaceEvent>,
    Vec<(usize, u32)>,
) {
    place_profile::reset();
    place_profile::clear_forbidden_records();
    place_profile::set_profiling_enabled(true);
    let mut sim = Simulation::new(def, VisitOrder::default());
    let mut events = Vec::with_capacity(TURNS);

    for _ in 0..TURNS {
        assert!(sim.step_turn_profiled(def), "step failed");
        let (index, piece_id) = *sim.placements.last().expect("placement");
        let moves = def.piece(piece_id).piece.valid_moves.len();
        events.push(PlaceEvent {
            piece_id,
            index,
            xy: index_to_xy(index),
            inserts_per_placement: moves,
        });
    }

    let records = place_profile::take_forbidden_records();
    place_profile::set_profiling_enabled(false);
    let checksum = placement_checksum(&sim.placements);
    (
        checksum,
        place_profile::take_work(),
        place_profile::take_forbidden_insert_stats(),
        events,
        records,
    )
}

fn bench_full_step_turn(def: &GameDefinition) -> f64 {
    median_ms(5, || {
        let mut sim = Simulation::new(def, VisitOrder::default());
        let start = Instant::now();
        for _ in 0..TURNS {
            assert!(sim.step_turn(def));
        }
        black_box(sim.placements.len());
        start.elapsed().as_secs_f64() * 1_000.0
    })
}

fn bench_place_replay(def: &GameDefinition, events: &[PlaceEvent]) -> f64 {
    median_ms(5, || {
        let mut sim = Simulation::new(def, VisitOrder::default());
        let start = Instant::now();
        for event in events {
            sim.replay_place_profiled(def, event.index, event.xy, event.piece_id);
        }
        black_box(sim.placements.len());
        start.elapsed().as_secs_f64() * 1_000.0
    })
}

fn median_ms(iters: u32, mut sample: impl FnMut() -> f64) -> f64 {
    let mut samples: Vec<f64> = (0..iters).map(|_| sample()).collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}

fn microbench_components(
    def: &GameDefinition,
    events: &[PlaceEvent],
    records: &[(usize, u32)],
    piece_count: usize,
) -> MicrobenchMs {
    MicrobenchMs {
        xy_to_index: median_ms(5, || bench_xy_to_index(def, events) as f64 / 1e6),
        forbidden_single_set: median_ms(5, || bench_forbidden_single_set(records) as f64 / 1e6),
        forbidden_multi_set: median_ms(5, || {
            bench_forbidden_multi_set(records, piece_count) as f64 / 1e6
        }),
        occupancy: median_ms(5, || bench_occupancy_inserts(events) as f64 / 1e6),
    }
}

fn bench_xy_to_index(def: &GameDefinition, events: &[PlaceEvent]) -> u64 {
    let start = Instant::now();
    let mut acc = 0u32;
    for event in events {
        let (x, y) = event.xy;
        for &(dx, dy) in &def.piece(event.piece_id).piece.valid_moves {
            acc = acc.wrapping_add(xy_to_index(x + dx, y + dy));
        }
    }
    black_box(acc);
    start.elapsed().as_nanos() as u64
}

fn bench_forbidden_single_set(records: &[(usize, u32)]) -> u64 {
    let start = Instant::now();
    let mut set = ForbiddenInsertHarness::default();
    for &(_, index) in records {
        set.insert(index);
    }
    black_box(set);
    start.elapsed().as_nanos() as u64
}

fn bench_forbidden_multi_set(records: &[(usize, u32)], piece_count: usize) -> u64 {
    let start = Instant::now();
    let mut sets: Vec<ForbiddenInsertHarness> = (0..piece_count)
        .map(|_| ForbiddenInsertHarness::default())
        .collect();
    for &(target, index) in records {
        sets[target].insert(index);
    }
    black_box(sets);
    start.elapsed().as_nanos() as u64
}

fn bench_occupancy_inserts(events: &[PlaceEvent]) -> u64 {
    let start = Instant::now();
    let mut grid = OccupancyInsertHarness::new();
    for (i, event) in events.iter().enumerate() {
        grid.insert(i as u32, event.piece_id);
    }
    black_box(grid.len());
    start.elapsed().as_nanos() as u64
}

struct PlacementFanoutSample {
    unique_attacked_cells: usize,
    total_inserts: usize,
    cross_target_duplicates: usize,
}

fn analyze_placement_fanout(
    records: &[(usize, u32)],
    offset: usize,
    len: usize,
) -> PlacementFanoutSample {
    let slice = &records[offset..offset + len];
    let mut unique = HashSet::new();
    for &(_, idx) in slice {
        unique.insert(idx);
    }
    let cross_target = slice.len().saturating_sub(unique.len());
    PlacementFanoutSample {
        unique_attacked_cells: unique.len(),
        total_inserts: slice.len(),
        cross_target_duplicates: cross_target,
    }
}

fn print_summary_row(
    name: &str,
    checksum: u64,
    step_ms: f64,
    place_ms: f64,
    work: &PlaceWorkStats,
) {
    let scan_ms = (step_ms - place_ms).max(0.0);
    let scan_pct = 100.0 * scan_ms / step_ms;
    let cells_per_place = work.scan_cells_examined as f64 / work.placements as f64;
    let forb_ins_per_place = work.forbidden_bit_inserts as f64 / work.placements as f64;
    let forb_combines_per_cell =
        work.scan_forb_word_combines as f64 / work.scan_cells_examined.max(1) as f64;

    println!(
        "{name}\t{TURNS}\t{checksum}\t{step_ms:.3}\t{place_ms:.3}\t{scan_ms:.3}\t{scan_pct:.1}\t{cells_per_place:.2}\t{forb_ins_per_place:.1}\t{forb_combines_per_cell:.3}"
    );
}

fn print_fanout_report(
    name: &str,
    def: &GameDefinition,
    work: &PlaceWorkStats,
    forb_stats: &ForbiddenInsertStats,
    events: &[PlaceEvent],
    records: &[(usize, u32)],
    micro: &MicrobenchMs,
    place_ms: f64,
) {
    let late = analyze_window(
        events,
        records,
        events.len().saturating_sub(LATE_PLACEMENTS),
        LATE_PLACEMENTS,
    );
    let early = analyze_window(events, records, 0, 1_000.min(events.len()));

    eprintln!("\n=== forbidden fanout: {name} ===");
    eprintln!(
        "  layer inserts/placement: {} moves (attack_layers; no cross-target fanout)",
        work.forbidden_bit_inserts as f64 / work.placements.max(1) as f64
    );
    let places = work.placements.max(1) as f64;
    eprintln!(
        "  scan: cells/placement {:.2}; combined_forbidden_word calls/placement {:.2} ({:.3} per cell examined)",
        work.scan_cells_examined as f64 / places,
        work.scan_forb_word_combines as f64 / places,
        work.scan_forb_word_combines as f64 / work.scan_cells_examined.max(1) as f64
    );
    eprintln!(
        "  scan rejects: tail_skip/placement {:.3}; single_step/placement {:.3} (remainder ≈ cells − 1 − tail − single)",
        work.scan_forbidden_tail_skips as f64 / places,
        work.scan_single_step_rejects as f64 / places
    );
    eprintln!(
        "  graph: {} pieces; defenders respecting piece0: {}",
        def.pieces.len(),
        threatened_targets(def, 0)
    );

    eprintln!("  insert path (full 100k run):");
    let calls = forb_stats.calls.max(1) as f64;
    eprintln!(
        "    existing-word OR: {} ({:.1}%)",
        forb_stats.or_into_existing_word,
        100.0 * forb_stats.or_into_existing_word as f64 / calls
    );
    eprintln!(
        "    new-word resize:  {} ({:.1}%)",
        forb_stats.grow_new_word,
        100.0 * forb_stats.grow_new_word as f64 / calls
    );
    eprintln!(
        "    bit already set:  {} ({:.1}%) — idempotent OR, no correctness skip today",
        forb_stats.bit_already_set,
        100.0 * forb_stats.bit_already_set as f64 / calls
    );

    eprintln!("  per-placement attacked cells (unique spiral index per placement):");
    eprintln!(
        "    first 1k placements: unique/cell {:.2}, cross-target duplicate inserts/placement {:.2}",
        early.mean_unique_attacked, early.mean_cross_target_dupes
    );
    eprintln!(
        "    last 1k placements:  unique/cell {:.2}, cross-target duplicate inserts/placement {:.2}",
        late.mean_unique_attacked, late.mean_cross_target_dupes
    );
    eprintln!(
        "    fanout multiplier (inserts ÷ unique attacked): early {:.2}×, late {:.2}× (theoretical max = targets per move batch)",
        early.mean_fanout_multiplier, late.mean_fanout_multiplier
    );

    eprintln!("  isolated replay (median ms, fresh state):");
    eprintln!(
        "    xy_to_index:           {:.3} ({:.1}% of place replay)",
        micro.xy_to_index,
        100.0 * micro.xy_to_index / place_ms
    );
    eprintln!(
        "    forbidden OR (1 set):    {:.3} ({:.1}% of place) — lower bound; single bitset",
        micro.forbidden_single_set,
        100.0 * micro.forbidden_single_set / place_ms
    );
    eprintln!(
        "    forbidden OR ({} sets): {:.3} ({:.1}% of place) — matches sim layout",
        def.pieces.len(),
        micro.forbidden_multi_set,
        100.0 * micro.forbidden_multi_set / place_ms
    );
    eprintln!(
        "    multi vs single set:   +{:.1}% (separate `ForbiddenSet` / cache working set)",
        100.0 * (micro.forbidden_multi_set - micro.forbidden_single_set)
            / micro.forbidden_single_set.max(0.001)
    );
    eprintln!(
        "    occupancy insert:      {:.3} ({:.1}% of place)",
        micro.occupancy,
        100.0 * micro.occupancy / place_ms
    );
}

struct LateFanoutAgg {
    mean_unique_attacked: f64,
    mean_cross_target_dupes: f64,
    mean_fanout_multiplier: f64,
}

fn analyze_window(
    events: &[PlaceEvent],
    records: &[(usize, u32)],
    placement_start: usize,
    placement_count: usize,
) -> LateFanoutAgg {
    let mut record_offset = 0usize;
    for event in &events[..placement_start] {
        record_offset += event.inserts_per_placement;
    }

    let end = (placement_start + placement_count).min(events.len());
    let mut sum_unique = 0u64;
    let mut sum_cross = 0u64;
    let mut sum_mult = 0f64;
    let mut count = 0u64;

    for event in &events[placement_start..end] {
        let sample = analyze_placement_fanout(records, record_offset, event.inserts_per_placement);
        record_offset += event.inserts_per_placement;

        sum_unique += sample.unique_attacked_cells as u64;
        sum_cross += sample.cross_target_duplicates as u64;
        if sample.unique_attacked_cells > 0 {
            sum_mult += sample.total_inserts as f64 / sample.unique_attacked_cells as f64;
        }
        count += 1;
    }

    let n = count.max(1) as f64;
    LateFanoutAgg {
        mean_unique_attacked: sum_unique as f64 / n,
        mean_cross_target_dupes: sum_cross as f64 / n,
        mean_fanout_multiplier: sum_mult / n,
    }
}

fn placement_checksum(placements: &[(u32, usize)]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &(index, piece_id) in placements {
        let value = ((index as u64) << 8) ^ piece_id as u64;
        hash ^= value;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
