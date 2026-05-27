use crate::model::{PieceId, GameDefinition};

pub use crate::index_order::{IndexOrder, SquareSpiral, VisitOrder};
use bevy::prelude::{FromWorld, Resource, World};
use std::sync::Arc;
use std::time::Duration;

use bevy::platform::time::Instant;

const EMPTY_ARMY: PieceId = usize::MAX;
/// Sentinel for unoccupied spiral indices (shared with render).
pub(crate) const EMPTY_ARMY_SLOT: PieceId = EMPTY_ARMY;

fn resize_piece_vectors<T: Clone>(vec: &mut Vec<T>, len: usize, fill: T) {
    if vec.len() == len {
        vec.fill(fill);
    } else {
        *vec = vec![fill; len];
    }
}

#[derive(Resource)]
pub struct Simulation {
    pub visit_order: VisitOrder,
    /// Dense by spiral index because simulation placement scans are numeric and monotonic.
    /// This avoids hashing on every occupied-cell check in the hot loop.
    pub occupancy: OccupancyGrid,
    /// Cumulative attacked cells from each piece's placements (one bitset per attacker).
    attack_layers: Vec<ForbiddenSet>,
    /// For each defender piece, attackers whose `attack_layers` are OR'd during its scan.
    respected_attackers: Vec<Vec<PieceId>>,
    /// Enabled turn order captured with the definition-derived simulation metadata.
    active_turn_order: Vec<PieceId>,
    pub cursors: Vec<u32>,
    cursor_positions: Vec<(i32, i32)>,
    /// Rolling cursor into `turn_order`; avoids a modulo in every simulated turn.
    turn_order_index: usize,
    pub turn_step: usize,
    pub placements: Vec<(u32, PieceId)>,
}

impl Simulation {
    pub fn new(def: &GameDefinition, visit_order: VisitOrder) -> Self {
        Self {
            visit_order,
            occupancy: OccupancyGrid::new(),
            attack_layers: vec![ForbiddenSet::default(); def.pieces.len()],
            respected_attackers: respected_attackers(def),
            active_turn_order: active_turn_order(def),
            cursors: vec![0; def.pieces.len()],
            cursor_positions: vec![(0, 0); def.pieces.len()],
            turn_order_index: 0,
            turn_step: 0,
            placements: Vec::new(),
        }
    }

    pub fn reset(&mut self, def: &GameDefinition) {
        self.occupancy.clear();
        let piece_count = def.pieces.len();
        if self.attack_layers.len() == piece_count {
            for set in &mut self.attack_layers {
                set.clear();
            }
        } else {
            self.attack_layers = vec![ForbiddenSet::default(); piece_count];
        }
        self.respected_attackers = respected_attackers(def);
        self.active_turn_order = active_turn_order(def);
        resize_piece_vectors(&mut self.cursors, piece_count, 0);
        resize_piece_vectors(&mut self.cursor_positions, piece_count, (0, 0));
        self.turn_order_index = 0;
        self.turn_step = 0;
        self.placements.clear();
    }

    fn place(&mut self, def: &GameDefinition, index: u32, xy: (i32, i32), piece_id: PieceId) {
        #[cfg(feature = "place_profile")]
        if crate::place_profile::profiling_active() {
            let moves = def.piece(piece_id).piece.valid_moves.len() as u64;
            crate::place_profile::note_placement_work(moves, 1);
            let place_start = crate::place_profile::timing_enabled_for_place()
                .then(Instant::now);
            crate::place_profile::time_occupancy_insert(|| {
                self.occupancy.insert(index, piece_id);
            });
            crate::place_profile::time_record_forbidden(|| {
                self.record_forbidden(def, xy, piece_id);
            });
            crate::place_profile::time_placements_push(|| {
                self.placements.push((index, piece_id));
            });
            if let Some(place_start) = place_start {
                crate::place_profile::add_place_total_ns(place_start.elapsed().as_nanos() as u64);
            }
        } else {
            self.occupancy.insert(index, piece_id);
            self.record_forbidden(def, xy, piece_id);
            self.placements.push((index, piece_id));
        }
        #[cfg(not(feature = "place_profile"))]
        {
            self.occupancy.insert(index, piece_id);
            self.record_forbidden(def, xy, piece_id);
            self.placements.push((index, piece_id));
        }
    }

    fn record_forbidden(&mut self, def: &GameDefinition, xy: (i32, i32), piece_id: PieceId) {
        let moves = &def.piece(piece_id).piece.valid_moves;
        let (x, y) = xy;
        for &(dx, dy) in moves {
            let attacked = self.visit_order.xy_to_index(x + dx, y + dy);
            #[cfg(feature = "place_profile")]
            if crate::place_profile::profiling_active() {
                crate::place_profile::push_forbidden_record(piece_id, attacked);
            }
            self.attack_layers[piece_id].insert(attacked);
        }
    }

    /// One piece takes a turn: scan from its cursor for the first legal square.
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
        piece_id: PieceId,
    ) {
        self.place(def, index, xy, piece_id);
    }

    fn step_turn_scan<const COUNT_CELLS: bool>(
        &mut self,
        def: &GameDefinition,
        cells_examined: &mut u32,
    ) -> bool {
        let turn_order_len = self.active_turn_order.len();
        if turn_order_len == 0 {
            return false;
        }
        let piece_id = self.active_turn_order[self.turn_order_index];
        self.turn_order_index += 1;
        if self.turn_order_index == turn_order_len {
            self.turn_order_index = 0;
        }
        self.turn_step += 1;

        let occupancy = &self.occupancy;
        let attack_layers = &self.attack_layers;
        let respected = &self.respected_attackers[piece_id];
        // Locals avoid re-indexing `cursors`/`cursor_positions` on every scanned cell.
        let mut cursor = self.cursors[piece_id];
        let mut xy = self.cursor_positions[piece_id];
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
                self.cursors[piece_id] = cursor;
                self.cursor_positions[piece_id] = xy;
                self.place(def, cursor, xy, piece_id);
                return true;
            }

            let next = cursor + 1;
            if next == 0 {
                self.cursors[piece_id] = cursor;
                self.cursor_positions[piece_id] = xy;
                return false;
            }

            let word_end = ((cursor >> 6) + 1) << 6;
            if next < word_end {
                let shift = next & 63;
                let len = word_end - next;
                let tail_mask = (1u64 << len) - 1;
                if ((forb_word >> shift) & tail_mask) == tail_mask {
                    #[cfg(feature = "place_profile")]
                    crate::place_profile::note_scan_forbidden_tail_skip();
                    cursor = word_end;
                    xy = self.visit_order.index_to_xy(word_end);
                    if cursor == u32::MAX {
                        self.cursors[piece_id] = cursor;
                        self.cursor_positions[piece_id] = xy;
                        return false;
                    }
                    forb_word =
                        combined_forbidden_word(attack_layers, respected, cursor as usize >> 6);
                    continue;
                }
            }

            cursor = next;
            xy = self.visit_order.scan_step_xy(cursor - 1, xy);
            #[cfg(feature = "place_profile")]
            crate::place_profile::note_scan_single_step_reject();

            if cursor == u32::MAX {
                self.cursors[piece_id] = cursor;
                self.cursor_positions[piece_id] = xy;
                return false;
            }

            if (cursor & 63) == 0 {
                forb_word =
                    combined_forbidden_word(attack_layers, respected, cursor as usize >> 6);
            }
        }
    }

    pub fn needs_work(&self, def: &GameDefinition, target_index: u32) -> bool {
        if self.cursors.is_empty() {
            return false;
        }
        self.cursors.iter().enumerate().any(|(id, &c)| {
            def.pieces
                .get(id)
                .is_some_and(|a| a.enabled && c <= target_index)
        })
    }

    pub fn advance_to_target(&mut self, def: &GameDefinition, target_index: u32) {
        if def.pieces.is_empty() || self.active_turn_order.is_empty() {
            return;
        }
        while self.needs_work(def, target_index) {
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
        if def.pieces.is_empty() || self.active_turn_order.is_empty() {
            return;
        }
        self.occupancy.ensure_unique_for_mutation();
        let start = Instant::now();
        let mut turns_since_check = 0u32;
        while self.needs_work(def, target_index) {
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
    cells: Arc<Vec<PieceId>>,
}

impl OccupancyGrid {
    fn new() -> Self {
        Self {
            cells: Arc::new(Vec::new()),
        }
    }

    /// Split shared backing storage so the sim can mutate while UI holds a snapshot `Arc`.
    pub fn ensure_unique_for_mutation(&mut self) {
        if Arc::strong_count(&self.cells) > 1 {
            self.cells = Arc::new(self.cells.as_ref().clone());
        }
    }

    fn clear(&mut self) {
        Arc::make_mut(&mut self.cells).clear();
    }

    fn insert(&mut self, index: u32, piece_id: PieceId) {
        let cells = Arc::make_mut(&mut self.cells);
        let index = index as usize;
        if index >= cells.len() {
            #[cfg(feature = "place_profile")]
            crate::place_profile::note_occupancy_grow();
            cells.resize(index + 1, EMPTY_ARMY);
        }
        cells[index] = piece_id;
    }

    pub fn get(&self, index: &u32) -> Option<&PieceId> {
        let piece_id = self.cells.get(*index as usize)?;
        (*piece_id != EMPTY_ARMY).then_some(piece_id)
    }

    /// Hot-path lookup for rendering (spiral index → piece).
    pub fn piece_id_at(&self, index: u32) -> Option<PieceId> {
        let piece_id = *self.cells.get(index as usize)?;
        (piece_id != EMPTY_ARMY).then_some(piece_id)
    }

    pub fn cells_slice(&self) -> &[PieceId] {
        self.cells.as_ref()
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

fn respected_attackers(def: &GameDefinition) -> Vec<Vec<PieceId>> {
    let piece_count = def.pieces.len();
    let mut respected = vec![Vec::new(); piece_count];
    for defender in 0..piece_count {
        for attacker in 0..piece_count {
            if def.piece(defender).blocked_by.contains(&attacker) {
                respected[defender].push(attacker);
            }
        }
    }
    respected
}

fn active_turn_order(def: &GameDefinition) -> Vec<PieceId> {
    def.active_turn_order().iter().collect()
}

fn combined_forbidden_word(
    layers: &[ForbiddenSet],
    respected: &[PieceId],
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
fn threatened_for(def: &GameDefinition) -> Vec<Vec<PieceId>> {
    let mut threatened_for = vec![Vec::new(); def.pieces.len()];
    for target_piece in 0..def.pieces.len() {
        for &attacker in &def.piece(target_piece).blocked_by {
            if attacker < threatened_for.len() {
                threatened_for[attacker].push(target_piece);
            }
        }
    }
    threatened_for
}

impl FromWorld for Simulation {
    fn from_world(world: &mut World) -> Self {
        let def = world.resource::<GameDefinition>();
        Simulation::new(def, VisitOrder::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GameDefinition;
    use crate::spiral::{index_to_xy, xy_to_index};
    use std::collections::HashSet;

    const GOLDEN_TURNS: [usize; 3] = [64, 1_024, 10_000];

    struct GoldenCase {
        name: &'static str,
        def: fn() -> GameDefinition,
        checksums: [u64; 3],
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

    fn run_turns(def: &GameDefinition, turns: usize) -> Simulation {
        let mut sim = Simulation::new(def, VisitOrder::default());
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
        let mut sim = Simulation::new(def, VisitOrder::default());
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
                    piece_count_min: 4,
                    piece_count_max: 6,
                    attack_radius_min: 1,
                    attack_radius_max: 3,
                    pattern_density: 0.55,
                    attack_symmetry: AttackSymmetry::Both,
                    identical_pieces: false,
                },
            ),
            (
                "random_sparse",
                RandomGenConfig {
                    piece_count_min: 2,
                    piece_count_max: 4,
                    attack_radius_min: 2,
                    attack_radius_max: 4,
                    pattern_density: 0.15,
                    attack_symmetry: AttackSymmetry::None,
                    identical_pieces: false,
                },
            ),
            (
                "random_wide_attacks",
                RandomGenConfig {
                    piece_count_min: 3,
                    piece_count_max: 5,
                    attack_radius_min: 3,
                    attack_radius_max: 5,
                    pattern_density: 0.45,
                    attack_symmetry: AttackSymmetry::Vertical,
                    identical_pieces: false,
                },
            ),
        ];

        for (cfg_name, mut cfg) in random_configs {
            cfg.sanitize();
            for seed in 0..3u64 {
                let mut rng = StdRng::seed_from_u64(seed);
                let def = generate_random_game(&cfg, &mut rng);
                let label = format!("{cfg_name}_seed{seed}_pieces{}", def.pieces.len());
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

    fn assert_valid_placements(def: &GameDefinition, placements: &[(u32, PieceId)]) {
        let threatened_for = threatened_for(def);
        let mut occupied = HashSet::new();
        let mut forbidden = vec![HashSet::new(); def.pieces.len()];

        for &(index, piece_id) in placements {
            assert!(piece_id < def.pieces.len(), "invalid piece id {piece_id}");
            assert!(occupied.insert(index), "duplicate placement at {index}");
            assert!(
                !forbidden[piece_id].contains(&index),
                "piece {piece_id} placed on forbidden square {index}"
            );

            let xy = index_to_xy(index);
            for target_piece in 0..forbidden.len() {
                forbidden[target_piece].insert(index);
            }
            for &target_piece in &threatened_for[piece_id] {
                for &(dx, dy) in &def.piece(piece_id).piece.valid_moves {
                    forbidden[target_piece].insert(xy_to_index(xy.0 + dx, xy.1 + dy));
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
        let mut sim = Simulation::new(&def, VisitOrder::default());
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
        let mut sim = Simulation::new(&def, VisitOrder::default());
        for _ in 0..6 {
            sim.step_turn(&def);
        }
        assert_eq!(sim.occupancy.get(&0), Some(&0));
        assert_eq!(sim.occupancy.get(&1), Some(&1));
        assert_eq!(sim.occupancy.get(&2), Some(&0));
        assert_eq!(sim.occupancy.get(&3), Some(&1));
    }

    #[test]
    fn forbidden_cells_are_cached_per_piece() {
        let def = GameDefinition::knight_2_pairwise();
        let mut sim = Simulation::new(&def, VisitOrder::default());
        sim.step_turn(&def);
        sim.step_turn(&def);

        let red_xy = sim.cursor_positions[1];
        for &(dx, dy) in &def.pieces[1].piece.valid_moves {
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

    /// Survey whether cheaper than `xy_to_index` per attack is plausible (run with `--nocapture`).
    #[test]
    fn attack_indexing_survey() {
        use crate::spiral::{index_to_ring_offset, xy_to_ring_offset};
        use std::collections::HashMap;

        struct Survey {
            placements: u64,
            max_abs_xy: i32,
            same_ring_attacks: u64,
            ring_delta_le1: u64,
            /// Distinct `(placement_index, move_slot)` → spiral index delta.
            index_delta_keys: HashMap<(u32, u8), i32>,
            /// Distinct `(placement_ring, move_slot)` → spiral index delta.
            ring_delta_keys: HashMap<(u32, u8), i32>,
            /// Distinct `(placement_ring, perimeter_offset, move_slot)` → delta.
            ring_offset_delta_keys: HashMap<(u32, u32, u8), i32>,
            index_delta_conflicts: u64,
            ring_delta_conflicts: u64,
            ring_offset_delta_conflicts: u64,
            total_attacks: u64,
        }

        fn survey_preset(name: &str, def: &GameDefinition, turns: usize) -> Survey {
            let mut sim = Simulation::new(def, VisitOrder::default());
            let mut s = Survey {
                placements: 0,
                max_abs_xy: 0,
                same_ring_attacks: 0,
                ring_delta_le1: 0,
                index_delta_keys: HashMap::new(),
                ring_delta_keys: HashMap::new(),
                ring_offset_delta_keys: HashMap::new(),
                index_delta_conflicts: 0,
                ring_delta_conflicts: 0,
                ring_offset_delta_conflicts: 0,
                total_attacks: 0,
            };

            for _ in 0..turns {
                assert!(sim.step_turn(def));
                let (place_index, piece_id) = *sim.placements.last().unwrap();
                let (x, y) = index_to_xy(place_index);
                s.max_abs_xy = s.max_abs_xy.max(x.abs()).max(y.abs());
                let place_ro = index_to_ring_offset(place_index);
                let moves = &def.piece(piece_id).piece.valid_moves;
                s.placements += 1;

                for (slot, &(dx, dy)) in moves.iter().enumerate() {
                    s.total_attacks += 1;
                    let attacked = xy_to_index(x + dx, y + dy);
                    let delta = attacked as i64 - place_index as i64;
                    let attack_ro = xy_to_ring_offset(x + dx, y + dy);
                    if attack_ro.ring == place_ro.ring {
                        s.same_ring_attacks += 1;
                    }
                    if attack_ro.ring.abs_diff(place_ro.ring) <= 1 {
                        s.ring_delta_le1 += 1;
                    }
                    let d = delta as i32;
                    match s.index_delta_keys.get(&(place_index, slot as u8)) {
                        Some(prev) if *prev != d => s.index_delta_conflicts += 1,
                        None => {
                            s.index_delta_keys.insert((place_index, slot as u8), d);
                        }
                        _ => {}
                    }
                    match s.ring_delta_keys.get(&(place_ro.ring, slot as u8)) {
                        Some(prev) if *prev != d => s.ring_delta_conflicts += 1,
                        None => {
                            s.ring_delta_keys.insert((place_ro.ring, slot as u8), d);
                        }
                        _ => {}
                    }
                    match s
                        .ring_offset_delta_keys
                        .get(&(place_ro.ring, place_ro.offset, slot as u8))
                    {
                        Some(prev) if *prev != d => s.ring_offset_delta_conflicts += 1,
                        None => {
                            s.ring_offset_delta_keys
                                .insert((place_ro.ring, place_ro.offset, slot as u8), d);
                        }
                        _ => {}
                    }
                }
            }
            eprintln!("\n=== attack indexing survey: {name} ({turns} placements) ===");
            eprintln!(
                "  max |x|,|y| at placement: {}; total attacks: {}",
                s.max_abs_xy, s.total_attacks
            );
            let attacks = s.total_attacks;
            eprintln!(
                "  attack ring == placement ring: {:.1}%",
                100.0 * s.same_ring_attacks as f64 / attacks as f64
            );
            eprintln!(
                "  |Δring| <= 1: {:.1}%",
                100.0 * s.ring_delta_le1 as f64 / attacks as f64
            );
            eprintln!(
                "  unique (placement_index, move) → index Δ: {}",
                s.index_delta_keys.len()
            );
            eprintln!(
                "  unique (placement_ring, move) → index Δ: {}",
                s.ring_delta_keys.len()
            );
            eprintln!(
                "  unique (ring, perimeter_offset, move) → index Δ: {} (conflicts {})",
                s.ring_offset_delta_keys.len(),
                s.ring_offset_delta_conflicts
            );
            eprintln!(
                "  index-key conflicts: {}; ring-key conflicts: {}",
                s.index_delta_conflicts, s.ring_delta_conflicts
            );
            s
        }

        let king = GameDefinition::king_6_clique();
        let chimera = GameDefinition::chimera_3_clique();
        survey_preset("knight_2_pairwise", &GameDefinition::knight_2_pairwise(), 100_000);
        survey_preset("king_6_clique", &king, 100_000);
        survey_preset("chimera_3_clique", &chimera, 100_000);

        // Replay 100k knight placements: precomputed index→attacks table vs live xy_to_index.
        use crate::model::PieceDef;
        use std::hint::black_box;
        use std::time::Instant;

        let moves: &[(i32, i32)] = &PieceDef::knight().valid_moves;
        let def = GameDefinition::knight_2_pairwise();
        let mut sim = Simulation::new(&def, VisitOrder::default());
        let mut placements_xy: Vec<(u32, (i32, i32))> = Vec::with_capacity(100_000);
        for _ in 0..100_000 {
            assert!(sim.step_turn(&def));
            let (index, _) = *sim.placements.last().unwrap();
            placements_xy.push((index, index_to_xy(index)));
        }
        let max_index = placements_xy.iter().map(|(i, _)| *i).max().unwrap();
        eprintln!("\n=== attack table vs xy_to_index (knight, 100k placements, max_index={max_index}) ===");

        let mut table = vec![[0u32; 8]; max_index as usize + 1];
        let t0 = Instant::now();
        for index in 0..=max_index {
            let (x, y) = index_to_xy(index);
            for (i, &(dx, dy)) in moves.iter().enumerate() {
                table[index as usize][i] = xy_to_index(x + dx, y + dy);
            }
        }
        let build_ms = t0.elapsed().as_secs_f64() * 1e3;
        eprintln!("  build table [0..={max_index}]: {build_ms:.1} ms");

        let t1 = Instant::now();
        let mut acc = 0u64;
        for &(index, _) in &placements_xy {
            for &a in &table[index as usize] {
                acc = acc.wrapping_add(a as u64);
            }
        }
        black_box(acc);
        let lut_ms = t1.elapsed().as_secs_f64() * 1e3;

        let t2 = Instant::now();
        let mut acc2 = 0u64;
        for &(_, (x, y)) in &placements_xy {
            for &(dx, dy) in moves {
                acc2 = acc2.wrapping_add(xy_to_index(x + dx, y + dy) as u64);
            }
        }
        black_box(acc2);
        let live_ms = t2.elapsed().as_secs_f64() * 1e3;

        eprintln!(
            "  replay 800k attacks — LUT: {lut_ms:.1} ms, live xy_to_index: {live_ms:.1} ms (build amortized +{:.1} ms/placement if spread over 100k)",
            build_ms / 100_000.0
        );
    }
}
