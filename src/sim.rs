use crate::model::{ArmyId, GameDefinition};
use crate::spiral::{index_to_xy, spiral_step, xy_to_index};
use bevy::prelude::{FromWorld, Resource, World};
use std::time::Duration;

use bevy::platform::time::Instant;

const EMPTY_ARMY: ArmyId = usize::MAX;

fn resize_army_vectors<T: Clone>(vec: &mut Vec<T>, len: usize, fill: T) {
    if vec.len() == len {
        vec.fill(fill);
    } else {
        *vec = vec![fill; len];
    }
}

#[derive(Resource)]
pub struct Simulation {
    /// Dense by spiral index because simulation placement scans are numeric and monotonic.
    /// This avoids hashing on every occupied-cell check in the hot loop.
    pub occupancy: OccupancyGrid,
    /// Cumulative attacked cells from each army's placements (one bitset per attacker).
    attack_layers: Vec<ForbiddenSet>,
    /// For each defender army, attackers whose `attack_layers` are OR'd during its scan.
    respected_attackers: Vec<Vec<ArmyId>>,
    pub cursors: Vec<u32>,
    cursor_positions: Vec<(i32, i32)>,
    /// Rolling cursor into `turn_order`; avoids a modulo in every simulated turn.
    turn_order_index: usize,
    pub turn_step: usize,
    pub placements: Vec<(u32, ArmyId)>,
}

impl Simulation {
    pub fn new(def: &GameDefinition) -> Self {
        Self {
            occupancy: OccupancyGrid::new(),
            attack_layers: vec![ForbiddenSet::default(); def.armies.len()],
            respected_attackers: respected_attackers(def),
            cursors: vec![0; def.armies.len()],
            cursor_positions: vec![(0, 0); def.armies.len()],
            turn_order_index: 0,
            turn_step: 0,
            placements: Vec::new(),
        }
    }

    pub fn reset(&mut self, def: &GameDefinition) {
        self.occupancy.clear();
        let army_count = def.armies.len();
        if self.attack_layers.len() == army_count {
            for set in &mut self.attack_layers {
                set.clear();
            }
        } else {
            self.attack_layers = vec![ForbiddenSet::default(); army_count];
        }
        self.respected_attackers = respected_attackers(def);
        resize_army_vectors(&mut self.cursors, army_count, 0);
        resize_army_vectors(&mut self.cursor_positions, army_count, (0, 0));
        self.turn_order_index = 0;
        self.turn_step = 0;
        self.placements.clear();
    }

    fn place(&mut self, def: &GameDefinition, index: u32, xy: (i32, i32), army_id: ArmyId) {
        #[cfg(feature = "place_profile")]
        if crate::place_profile::profiling_active() {
            let moves = def.army(army_id).piece.valid_moves.len() as u64;
            crate::place_profile::note_placement_work(moves, 1);
            let place_start = crate::place_profile::timing_enabled_for_place()
                .then(Instant::now);
            crate::place_profile::time_occupancy_insert(|| {
                self.occupancy.insert(index, army_id);
            });
            crate::place_profile::time_record_forbidden(|| {
                self.record_forbidden(def, xy, army_id);
            });
            crate::place_profile::time_placements_push(|| {
                self.placements.push((index, army_id));
            });
            if let Some(place_start) = place_start {
                crate::place_profile::add_place_total_ns(place_start.elapsed().as_nanos() as u64);
            }
        } else {
            self.occupancy.insert(index, army_id);
            self.record_forbidden(def, xy, army_id);
            self.placements.push((index, army_id));
        }
        #[cfg(not(feature = "place_profile"))]
        {
            self.occupancy.insert(index, army_id);
            self.record_forbidden(def, xy, army_id);
            self.placements.push((index, army_id));
        }
    }

    fn record_forbidden(&mut self, def: &GameDefinition, xy: (i32, i32), army_id: ArmyId) {
        let moves = &def.army(army_id).piece.valid_moves;
        let (x, y) = xy;
        for &(dx, dy) in moves {
            let attacked = xy_to_index(x + dx, y + dy);
            #[cfg(feature = "place_profile")]
            if crate::place_profile::profiling_active() {
                crate::place_profile::push_forbidden_record(army_id, attacked);
            }
            self.attack_layers[army_id].insert(attacked);
        }
    }

    /// One army takes a turn: scan from its cursor for the first legal square.
    pub fn step_turn(&mut self, def: &GameDefinition) -> bool {
        self.step_turn_scan::<false>(def, &mut 0)
    }

    /// Like `step_turn`, but accumulates scan/place timings (requires feature `place_profile`).
    #[cfg(feature = "place_profile")]
    pub fn step_turn_profiled(&mut self, def: &GameDefinition) -> bool {
        crate::place_profile::time_step_turn(|| {
            let mut cells = 0u32;
            let ok = self.step_turn_scan::<true>(def, &mut cells);
            crate::place_profile::add_scan_cells(cells as u64);
            ok
        })
    }

    /// Re-run `place` for profiling replays (requires feature `place_profile`).
    #[cfg(feature = "place_profile")]
    pub fn replay_place_profiled(
        &mut self,
        def: &GameDefinition,
        index: u32,
        xy: (i32, i32),
        army_id: ArmyId,
    ) {
        self.place(def, index, xy, army_id);
    }

    fn step_turn_scan<const COUNT_CELLS: bool>(
        &mut self,
        def: &GameDefinition,
        cells_examined: &mut u32,
    ) -> bool {
        let turn_order_len = def.turn_order.len();
        if turn_order_len == 0 {
            return false;
        }
        let army_id = def.turn_order[self.turn_order_index];
        self.turn_order_index += 1;
        if self.turn_order_index == turn_order_len {
            self.turn_order_index = 0;
        }
        self.turn_step += 1;

        let occupancy = &self.occupancy;
        let attack_layers = &self.attack_layers;
        let respected = &self.respected_attackers[army_id];
        // Locals avoid re-indexing `cursors`/`cursor_positions` on every scanned cell.
        let mut cursor = self.cursors[army_id];
        let mut xy = self.cursor_positions[army_id];
        let mut forb_word =
            combined_forbidden_word(attack_layers, respected, cursor as usize >> 6);

        loop {
            if COUNT_CELLS {
                *cells_examined += 1;
            }
            let bit = 1u64 << (cursor & 63);
            let occupied = occupancy.contains_index(cursor);
            let forbidden_here = forb_word & bit != 0;
            if !occupied && !forbidden_here {
                self.cursors[army_id] = cursor;
                self.cursor_positions[army_id] = xy;
                self.place(def, cursor, xy, army_id);
                return true;
            }

            let next = cursor + 1;
            if next == 0 {
                self.cursors[army_id] = cursor;
                self.cursor_positions[army_id] = xy;
                return false;
            }

            let word_end = ((cursor >> 6) + 1) << 6;
            if next < word_end {
                let shift = next & 63;
                let len = word_end - next;
                let tail_mask = (1u64 << len) - 1;
                if ((forb_word >> shift) & tail_mask) == tail_mask {
                    cursor = word_end;
                    xy = index_to_xy(word_end);
                    if cursor == u32::MAX {
                        self.cursors[army_id] = cursor;
                        self.cursor_positions[army_id] = xy;
                        return false;
                    }
                    forb_word =
                        combined_forbidden_word(attack_layers, respected, cursor as usize >> 6);
                    continue;
                }
            }

            cursor = next;
            xy = spiral_step(xy);

            if cursor == u32::MAX {
                self.cursors[army_id] = cursor;
                self.cursor_positions[army_id] = xy;
                return false;
            }

            if (cursor & 63) == 0 {
                forb_word =
                    combined_forbidden_word(attack_layers, respected, cursor as usize >> 6);
            }
        }
    }

    pub fn needs_work(&self, target_index: u32) -> bool {
        if self.cursors.is_empty() {
            return false;
        }
        self.cursors.iter().any(|&c| c <= target_index)
    }

    pub fn advance_to_target(&mut self, def: &GameDefinition, target_index: u32) {
        if def.armies.is_empty() || def.turn_order.is_empty() {
            return;
        }
        while self.needs_work(target_index) {
            if !self.step_turn(def) {
                break;
            }
        }
    }

    pub fn advance_for_duration(
        &mut self,
        def: &GameDefinition,
        target_index: u32,
        max_duration: Duration,
    ) {
        if def.armies.is_empty() || def.turn_order.is_empty() {
            return;
        }
        let start = Instant::now();
        let mut turns_since_check = 0u32;
        while self.needs_work(target_index) {
            if !self.step_turn(def) {
                break;
            }
            turns_since_check += 1;

            // Checking the clock every turn costs too much in this hot loop.
            // Batch the check so the UI still updates while simulation uses most of
            // the allotted frame time.
            if turns_since_check == 4_096 {
                if start.elapsed() >= max_duration {
                    break;
                }
                turns_since_check = 0;
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct OccupancyGrid {
    cells: Vec<ArmyId>,
}

impl OccupancyGrid {
    fn new() -> Self {
        Self { cells: Vec::new() }
    }

    fn clear(&mut self) {
        self.cells.clear();
    }

    fn insert(&mut self, index: u32, army_id: ArmyId) {
        let index = index as usize;
        if index >= self.cells.len() {
            #[cfg(feature = "place_profile")]
            crate::place_profile::note_occupancy_grow();
            self.cells.resize(index + 1, EMPTY_ARMY);
        }
        self.cells[index] = army_id;
    }

    pub fn get(&self, index: &u32) -> Option<&ArmyId> {
        let army_id = self.cells.get(*index as usize)?;
        (*army_id != EMPTY_ARMY).then_some(army_id)
    }

    fn contains_index(&self, index: u32) -> bool {
        self.cells
            .get(index as usize)
            .copied()
            .unwrap_or(EMPTY_ARMY)
            != EMPTY_ARMY
    }
}

#[derive(Clone, Debug, Default)]
struct ForbiddenSet {
    words: Vec<u64>,
}

impl ForbiddenSet {
    fn clear(&mut self) {
        self.words.clear();
    }

    fn insert(&mut self, index: u32) {
        let word_index = index as usize >> 6;
        let bit = 1u64 << (index & 63);
        if word_index < self.words.len() {
            #[cfg(feature = "place_profile")]
            {
                let already_set = self.words[word_index] & bit != 0;
                crate::place_profile::note_forbidden_or_existing_word(already_set);
            }
            self.words[word_index] |= bit;
        } else {
            #[cfg(feature = "place_profile")]
            crate::place_profile::note_forbidden_or_new_word();
            self.words.resize(word_index + 1, 0);
            self.words[word_index] |= bit;
        }
    }

    #[cfg(test)]
    fn contains_index(&self, index: u32) -> bool {
        let bit = 1u64 << (index & 63);
        self.words
            .get(index as usize >> 6)
            .copied()
            .unwrap_or(0)
            & bit
            != 0
    }

    fn word_bits(&self, word_index: usize) -> u64 {
        self.words.get(word_index).copied().unwrap_or(0)
    }

    /// Every index in `[from, to)` has its forbidden bit set.
    #[cfg(test)]
    fn forbidden_bits_all_set(&self, from: u32, to: u32) -> bool {
        range_bits_all_set(|word_index| self.word_bits(word_index), from, to)
    }
}

#[cfg(test)]
fn range_bits_all_set(word_bits: impl Fn(usize) -> u64, from: u32, to: u32) -> bool {
    debug_assert!(from < to);
    let mut index = from;
    while index < to {
        let segment_end = (((index >> 6) + 1) << 6).min(to);
        let shift = index & 63;
        let len = segment_end - index;
        let mask = if len >= 64 {
            u64::MAX
        } else {
            (1u64 << len) - 1
        };
        let bits = word_bits(index as usize >> 6) >> shift;
        if (bits & mask) != mask {
            return false;
        }
        index = segment_end;
    }
    true
}

fn respected_attackers(def: &GameDefinition) -> Vec<Vec<ArmyId>> {
    let army_count = def.armies.len();
    let mut respected = vec![Vec::new(); army_count];
    for defender in 0..army_count {
        for attacker in 0..army_count {
            if def.army(defender).blocked_by.contains(&attacker) {
                respected[defender].push(attacker);
            }
        }
    }
    respected
}

fn combined_forbidden_word(
    layers: &[ForbiddenSet],
    respected: &[ArmyId],
    word_index: usize,
) -> u64 {
    #[cfg(feature = "place_profile")]
    if crate::place_profile::profiling_active() {
        crate::place_profile::note_scan_forb_word_combine();
    }
    match respected {
        [] => 0,
        [a] => layers[*a].word_bits(word_index),
        [a, b] => layers[*a].word_bits(word_index) | layers[*b].word_bits(word_index),
        [a, b, c] => {
            layers[*a].word_bits(word_index)
                | layers[*b].word_bits(word_index)
                | layers[*c].word_bits(word_index)
        }
        [a, b, c, d] => {
            layers[*a].word_bits(word_index)
                | layers[*b].word_bits(word_index)
                | layers[*c].word_bits(word_index)
                | layers[*d].word_bits(word_index)
        }
        [a, b, c, d, e] => {
            layers[*a].word_bits(word_index)
                | layers[*b].word_bits(word_index)
                | layers[*c].word_bits(word_index)
                | layers[*d].word_bits(word_index)
                | layers[*e].word_bits(word_index)
        }
        _ => respected
            .iter()
            .fold(0u64, |acc, &a| acc | layers[a].word_bits(word_index)),
    }
}

#[cfg(test)]
fn threatened_for(def: &GameDefinition) -> Vec<Vec<ArmyId>> {
    let mut threatened_for = vec![Vec::new(); def.armies.len()];
    for target_army in 0..def.armies.len() {
        for &attacker in &def.army(target_army).blocked_by {
            if attacker < threatened_for.len() {
                threatened_for[attacker].push(target_army);
            }
        }
    }
    threatened_for
}

impl FromWorld for Simulation {
    fn from_world(world: &mut World) -> Self {
        let def = world.resource::<GameDefinition>();
        Simulation::new(def)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GameDefinition;
    use crate::spiral::index_to_xy;
    use std::collections::HashSet;

    const GOLDEN_TURNS: [usize; 3] = [64, 1_024, 10_000];

    struct GoldenCase {
        name: &'static str,
        def: fn() -> GameDefinition,
        checksums: [u64; 3],
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

    fn run_turns(def: &GameDefinition, turns: usize) -> Simulation {
        let mut sim = Simulation::new(def);
        for _ in 0..turns {
            assert!(sim.step_turn(def));
        }
        sim
    }

    /// Rejected cells before placement on a successful turn (`cells_examined - 1`).
    fn rejections_on_success(cells_examined: u32) -> u32 {
        cells_examined.saturating_sub(1)
    }

    #[derive(Debug)]
    struct RejectionStats {
        turns_sampled: usize,
        max_rejections: u32,
        max_rejections_turn: usize,
        p50_rejections: u32,
        p99_rejections: u32,
        mean_rejections: f64,
    }

    fn collect_rejection_stats(def: &GameDefinition, total_turns: usize, late_window: usize) -> RejectionStats {
        let mut sim = Simulation::new(def);
        let mut late_rejections = Vec::with_capacity(late_window.min(total_turns));
        let mut max_rejections = 0u32;
        let mut max_rejections_turn = 0usize;
        let mut sum = 0u64;

        for turn in 0..total_turns {
            let mut cells_examined = 0u32;
            let placed = sim.step_turn_scan::<true>(def, &mut cells_examined);
            assert!(placed, "turn {turn} failed to place");
            let examined = cells_examined;
            let rejections = rejections_on_success(examined);
            sum += rejections as u64;
            if rejections > max_rejections {
                max_rejections = rejections;
                max_rejections_turn = turn + 1;
            }
            if turn + late_window >= total_turns {
                late_rejections.push(rejections);
            }
        }

        late_rejections.sort_unstable();
        let p50 = late_rejections[late_rejections.len() / 2];
        let p99_idx = ((late_rejections.len() as f64) * 0.99).floor() as usize;
        let p99 = late_rejections[p99_idx.min(late_rejections.len().saturating_sub(1))];

        RejectionStats {
            turns_sampled: total_turns,
            max_rejections,
            max_rejections_turn,
            p50_rejections: p50,
            p99_rejections: p99,
            mean_rejections: sum as f64 / total_turns as f64,
        }
    }

    fn format_rejection_stats(label: &str, stats: &RejectionStats, late_window: usize) -> String {
        format!(
            "{label}: turns={} late_last={} max_rej={} @turn{} late_p50={} late_p99={} mean_rej={:.1}",
            stats.turns_sampled,
            late_window,
            stats.max_rejections,
            stats.max_rejections_turn,
            stats.p50_rejections,
            stats.p99_rejections,
            stats.mean_rejections,
        )
    }

    /// How many spiral cells are rejected before each placement in late game?
    /// Run with `cargo testd scan_rejection -- --nocapture` to print the report.
    #[test]
    fn scan_rejection_late_game_presets_and_random() {
        use crate::random_gen::{AttackSymmetry, RandomGenConfig, generate_random_game};
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        const PRESET_TURNS: usize = 100_000;
        const RANDOM_TURNS: usize = 20_000;
        const LATE_WINDOW: usize = 1_000;
        const THOUSAND: u32 = 1_000;

        let mut global_max = 0u32;
        let mut global_max_label = String::new();

        let preset_cases: [(&str, fn() -> GameDefinition); 5] = [
            ("knight_2_pairwise", GameDefinition::knight_2_pairwise),
            ("knight_3_clique", GameDefinition::knight_3_clique),
            ("leaper_4_mixed_clique", GameDefinition::leaper_4_mixed_clique),
            ("king_6_clique", GameDefinition::king_6_clique),
            ("chimera_3_clique", GameDefinition::chimera_3_clique),
        ];

        eprintln!("\n=== scan rejections (rejections = cells examined - 1 on success) ===");
        for (name, def_fn) in preset_cases {
            let def = def_fn();
            let stats = collect_rejection_stats(&def, PRESET_TURNS, LATE_WINDOW);
            eprintln!("{}", format_rejection_stats(name, &stats, LATE_WINDOW));
            if stats.max_rejections > global_max {
                global_max = stats.max_rejections;
                global_max_label = name.to_string();
            }
        }

        let random_configs: [(&str, RandomGenConfig); 4] = [
            (
                "random_default",
                RandomGenConfig::default(),
            ),
            (
                "random_dense_clique_like",
                RandomGenConfig {
                    army_count_min: 4,
                    army_count_max: 6,
                    attack_radius_min: 1,
                    attack_radius_max: 3,
                    pattern_density: 0.55,
                    attack_symmetry: AttackSymmetry::Both,
                },
            ),
            (
                "random_sparse",
                RandomGenConfig {
                    army_count_min: 2,
                    army_count_max: 4,
                    attack_radius_min: 2,
                    attack_radius_max: 4,
                    pattern_density: 0.15,
                    attack_symmetry: AttackSymmetry::None,
                },
            ),
            (
                "random_wide_attacks",
                RandomGenConfig {
                    army_count_min: 3,
                    army_count_max: 5,
                    attack_radius_min: 3,
                    attack_radius_max: 5,
                    pattern_density: 0.45,
                    attack_symmetry: AttackSymmetry::Vertical,
                },
            ),
        ];

        for (cfg_name, mut cfg) in random_configs {
            cfg.sanitize();
            for seed in 0..3u64 {
                let mut rng = StdRng::seed_from_u64(seed);
                let def = generate_random_game(&cfg, &mut rng);
                let label = format!("{cfg_name}_seed{seed}_armies{}", def.armies.len());
                let stats = collect_rejection_stats(&def, RANDOM_TURNS, LATE_WINDOW.min(RANDOM_TURNS));
                eprintln!("{}", format_rejection_stats(&label, &stats, LATE_WINDOW.min(RANDOM_TURNS)));
                if stats.max_rejections > global_max {
                    global_max = stats.max_rejections;
                    global_max_label = label;
                }
            }
        }

        eprintln!(
            "=== overall max rejections: {global_max} ({global_max_label}); \
             1000+ rejections/turn: {}",
            if global_max >= THOUSAND { "YES" } else { "NO" }
        );

        assert!(
            global_max < THOUSAND,
            "did not expect >=1000 rejections per turn in this survey; \
             got max {global_max} on {global_max_label}"
        );
    }

    fn assert_valid_placements(def: &GameDefinition, placements: &[(u32, ArmyId)]) {
        let threatened_for = threatened_for(def);
        let mut occupied = HashSet::new();
        let mut forbidden = vec![HashSet::new(); def.armies.len()];

        for &(index, army_id) in placements {
            assert!(army_id < def.armies.len(), "invalid army id {army_id}");
            assert!(occupied.insert(index), "duplicate placement at {index}");
            assert!(
                !forbidden[army_id].contains(&index),
                "army {army_id} placed on forbidden square {index}"
            );

            let xy = index_to_xy(index);
            for target_army in 0..forbidden.len() {
                forbidden[target_army].insert(index);
            }
            for &target_army in &threatened_for[army_id] {
                for &(dx, dy) in &def.army(army_id).piece.valid_moves {
                    forbidden[target_army].insert(xy_to_index(xy.0 + dx, xy.1 + dy));
                }
            }
        }
    }

    fn golden_cases() -> [GoldenCase; 5] {
        [
            GoldenCase {
                name: "knight_2_pairwise",
                def: GameDefinition::knight_2_pairwise,
                checksums: [
                    15_737_156_276_822_775_461,
                    5_149_276_635_673_381_925,
                    561_431_110_996_648_581,
                ],
            },
            GoldenCase {
                name: "knight_3_clique",
                def: GameDefinition::knight_3_clique,
                checksums: [
                    16_115_999_991_126_781_684,
                    10_088_445_098_850_287_540,
                    7_584_768_825_753_057_092,
                ],
            },
            GoldenCase {
                name: "leaper_4_mixed_clique",
                def: GameDefinition::leaper_4_mixed_clique,
                checksums: [
                    5_964_283_847_930_621_157,
                    6_946_720_379_821_596_453,
                    6_370_614_915_775_779_925,
                ],
            },
            GoldenCase {
                name: "king_6_clique",
                def: GameDefinition::king_6_clique,
                checksums: [
                    6_480_521_862_097_834_845,
                    15_603_942_777_120_392_349,
                    1_643_601_116_650_407_053,
                ],
            },
            GoldenCase {
                name: "chimera_3_clique",
                def: GameDefinition::chimera_3_clique,
                checksums: [
                    8_459_319_956_822_578_164,
                    6_307_119_068_148_425_140,
                    12_399_277_720_126_721_092,
                ],
            },
        ]
    }

    fn backing_capacities(sim: &Simulation) -> (usize, usize, usize) {
        let forb: usize = sim
            .attack_layers
            .iter()
            .map(|f| f.words.capacity())
            .sum();
        (sim.occupancy.cells.capacity(), sim.placements.capacity(), forb)
    }

    #[test]
    fn hot_path_vec_capacity_growth_is_bounded() {
        let def = GameDefinition::king_6_clique();
        let mut sim = Simulation::new(&def);
        let mut capacity_events = 0usize;

        for _ in 0..100_000 {
            let before = backing_capacities(&sim);
            assert!(sim.step_turn(&def));
            let after = backing_capacities(&sim);
            if after != before {
                capacity_events += 1;
            }
        }
        eprintln!("backing capacity changes during first 100k turns: {capacity_events}");

        let caps = backing_capacities(&sim);
        for _ in 0..5_000 {
            let before = backing_capacities(&sim);
            assert!(sim.step_turn(&def));
            assert_eq!(backing_capacities(&sim), before);
        }

        sim.reset(&def);
        for _ in 0..1_000 {
            let before = backing_capacities(&sim);
            assert!(sim.step_turn(&def));
            assert_eq!(backing_capacities(&sim), before);
        }

        assert_eq!(backing_capacities(&sim), caps);
    }

    #[test]
    fn forbidden_bits_all_set_covers_word_tail() {
        let mut set = ForbiddenSet::default();
        for index in 0..64 {
            set.insert(index);
        }
        assert!(set.forbidden_bits_all_set(0, 64));
        assert!(set.forbidden_bits_all_set(40, 64));
        assert!(!set.forbidden_bits_all_set(40, 65));
        for index in 64..128 {
            set.insert(index);
        }
        assert!(set.forbidden_bits_all_set(64, 128));
    }

    #[test]
    fn early_red_black_placements() {
        let def = GameDefinition::knight_2_pairwise();
        let mut sim = Simulation::new(&def);
        for _ in 0..6 {
            sim.step_turn(&def);
        }
        assert_eq!(sim.occupancy.get(&0), Some(&0));
        assert_eq!(sim.occupancy.get(&1), Some(&1));
        assert_eq!(sim.occupancy.get(&2), Some(&0));
        assert_eq!(sim.occupancy.get(&3), Some(&1));
    }

    #[test]
    fn forbidden_cells_are_cached_per_army() {
        let def = GameDefinition::knight_2_pairwise();
        let mut sim = Simulation::new(&def);
        sim.step_turn(&def);
        sim.step_turn(&def);

        let red_xy = sim.cursor_positions[1];
        for &(dx, dy) in &def.armies[1].piece.valid_moves {
            let attacked = xy_to_index(red_xy.0 + dx, red_xy.1 + dy);
            assert!(sim.attack_layers[1].contains_index(attacked));
        }

        let black_xy = (0, 0);
        let attacked = xy_to_index(black_xy.0 + 1, black_xy.1 + 2);
        assert!(sim.attack_layers[0].contains_index(attacked));
        assert!(sim.occupancy.contains_index(0));
        assert!(sim.occupancy.contains_index(1));
    }

    #[test]
    fn red_black_first_sixteen_placements_are_stable() {
        let def = GameDefinition::knight_2_pairwise();
        let sim = run_turns(&def, 16);

        assert_eq!(
            sim.placements,
            vec![
                (0, 0),
                (1, 1),
                (2, 0),
                (3, 1),
                (5, 0),
                (4, 1),
                (9, 0),
                (6, 1),
                (11, 0),
                (10, 1),
                (15, 0),
                (12, 1),
                (20, 0),
                (24, 1),
                (21, 0),
                (25, 1),
            ]
        );
    }

    #[test]
    fn representative_preset_checksums_are_stable() {
        for case in golden_cases() {
            let def = (case.def)();
            for (turns, expected_checksum) in GOLDEN_TURNS.into_iter().zip(case.checksums) {
                let sim = run_turns(&def, turns);
                assert_eq!(
                    placement_checksum(&sim.placements),
                    expected_checksum,
                    "{} after {turns} turns",
                    case.name
                );
            }
        }
    }

    #[test]
    fn representative_preset_placements_remain_legal() {
        for case in golden_cases() {
            let def = (case.def)();
            let sim = run_turns(&def, 10_000);

            assert_valid_placements(&def, &sim.placements);
            for (&cursor, &xy) in sim.cursors.iter().zip(&sim.cursor_positions) {
                assert_eq!(index_to_xy(cursor), xy, "{} cursor {cursor}", case.name);
            }
        }
    }
}
