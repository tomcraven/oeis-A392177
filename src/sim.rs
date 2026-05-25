use crate::model::{ArmyId, GameDefinition};
use crate::spiral::{index_to_xy, spiral_step, xy_to_index};
use bevy::prelude::{FromWorld, Resource, World};
use std::time::{Duration, Instant};

const EMPTY_ARMY: ArmyId = usize::MAX;

#[derive(Resource)]
pub struct Simulation {
    /// Dense by spiral index because simulation placement scans are numeric and monotonic.
    /// This avoids hashing on every occupied-cell check in the hot loop.
    pub occupancy: OccupancyGrid,
    /// Per-army attacked cells by armies this army respects.
    forbidden: Vec<ForbiddenSet>,
    threatened_for: Vec<Vec<ArmyId>>,
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
            forbidden: vec![ForbiddenSet::new(); def.armies.len()],
            threatened_for: threatened_for(def),
            cursors: vec![0; def.armies.len()],
            cursor_positions: vec![(0, 0); def.armies.len()],
            turn_order_index: 0,
            turn_step: 0,
            placements: Vec::new(),
        }
    }

    pub fn reset(&mut self, def: &GameDefinition) {
        self.occupancy.clear();
        self.forbidden = vec![ForbiddenSet::new(); def.armies.len()];
        self.threatened_for = threatened_for(def);
        self.cursors = vec![0; def.armies.len()];
        self.cursor_positions = vec![(0, 0); def.armies.len()];
        self.turn_order_index = 0;
        self.turn_step = 0;
        self.placements.clear();
    }

    fn place(&mut self, def: &GameDefinition, index: u32, xy: (i32, i32), army_id: ArmyId) {
        self.occupancy.insert(index, army_id);
        self.record_forbidden(def, xy, army_id);
        self.placements.push((index, army_id));
    }

    fn record_forbidden(&mut self, def: &GameDefinition, xy: (i32, i32), army_id: ArmyId) {
        let moves = &def.army(army_id).piece.valid_moves;
        let targets = &self.threatened_for[army_id];
        // Most presets fan out to 1-5 target armies. Specializing those sizes avoids
        // re-running xy_to_index per target and removes the tiny inner iterator overhead.
        match targets[..] {
            [] => {}
            [target_army] => {
                for &(dx, dy) in moves {
                    self.forbidden[target_army].insert(xy_to_index(xy.0 + dx, xy.1 + dy));
                }
            }
            [first, second] => {
                for &(dx, dy) in moves {
                    let attacked = xy_to_index(xy.0 + dx, xy.1 + dy);
                    self.forbidden[first].insert(attacked);
                    self.forbidden[second].insert(attacked);
                }
            }
            [first, second, third] => {
                for &(dx, dy) in moves {
                    let attacked = xy_to_index(xy.0 + dx, xy.1 + dy);
                    self.forbidden[first].insert(attacked);
                    self.forbidden[second].insert(attacked);
                    self.forbidden[third].insert(attacked);
                }
            }
            [first, second, third, fourth] => {
                for &(dx, dy) in moves {
                    let attacked = xy_to_index(xy.0 + dx, xy.1 + dy);
                    self.forbidden[first].insert(attacked);
                    self.forbidden[second].insert(attacked);
                    self.forbidden[third].insert(attacked);
                    self.forbidden[fourth].insert(attacked);
                }
            }
            [first, second, third, fourth, fifth] => {
                for &(dx, dy) in moves {
                    let attacked = xy_to_index(xy.0 + dx, xy.1 + dy);
                    self.forbidden[first].insert(attacked);
                    self.forbidden[second].insert(attacked);
                    self.forbidden[third].insert(attacked);
                    self.forbidden[fourth].insert(attacked);
                    self.forbidden[fifth].insert(attacked);
                }
            }
            _ => {
                for &(dx, dy) in moves {
                    let attacked = xy_to_index(xy.0 + dx, xy.1 + dy);
                    for &target_army in targets {
                        self.forbidden[target_army].insert(attacked);
                    }
                }
            }
        }
    }

    /// One army takes a turn: scan from its cursor for the first legal square.
    pub fn step_turn(&mut self, def: &GameDefinition) -> bool {
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
        let forbidden = &self.forbidden[army_id];
        // Locals avoid re-indexing `cursors`/`cursor_positions` on every scanned cell.
        let mut cursor = self.cursors[army_id];
        let mut xy = self.cursor_positions[army_id];

        loop {
            if !occupancy.contains_index(cursor) && !forbidden.contains_index(cursor) {
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
            if next < word_end && forbidden.forbidden_bits_all_set(next, word_end) {
                if word_end == 0 {
                    self.cursors[army_id] = cursor;
                    self.cursor_positions[army_id] = xy;
                    return false;
                }
                cursor = word_end;
                xy = index_to_xy(word_end);
                if cursor == u32::MAX {
                    self.cursors[army_id] = cursor;
                    self.cursor_positions[army_id] = xy;
                    return false;
                }
                continue;
            }

            cursor = next;
            xy = spiral_step(xy);

            if cursor == u32::MAX {
                self.cursors[army_id] = cursor;
                self.cursor_positions[army_id] = xy;
                return false;
            }
        }
    }

    pub fn needs_work(&self, target_index: u32) -> bool {
        self.cursors.iter().any(|&c| c <= target_index)
    }

    pub fn advance_to_target(&mut self, def: &GameDefinition, target_index: u32) {
        while self.needs_work(target_index) {
            self.step_turn(def);
        }
    }

    pub fn advance_for_duration(
        &mut self,
        def: &GameDefinition,
        target_index: u32,
        max_duration: Duration,
    ) {
        let start = Instant::now();
        let mut turns_since_check = 0u32;
        while self.needs_work(target_index) {
            self.step_turn(def);
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
            self.cells.resize(index + 1, EMPTY_ARMY);
        }
        self.cells[index] = army_id;
    }

    pub fn get(&self, index: &u32) -> Option<&ArmyId> {
        let army_id = self.cells.get(*index as usize)?;
        (*army_id != EMPTY_ARMY).then_some(army_id)
    }

    fn contains_index(&self, index: u32) -> bool {
        let index = index as usize;
        index < self.cells.len() && self.cells[index] != EMPTY_ARMY
    }
}

#[derive(Clone, Debug, Default)]
struct ForbiddenSet {
    words: Vec<u64>,
}

impl ForbiddenSet {
    fn new() -> Self {
        Self { words: Vec::new() }
    }

    fn insert(&mut self, index: u32) {
        let word_index = index as usize >> 6;
        if word_index >= self.words.len() {
            self.words.resize(word_index + 1, 0);
        }
        self.words[word_index] |= 1u64 << (index & 63);
    }

    fn contains_index(&self, index: u32) -> bool {
        let word_index = index as usize >> 6;
        word_index < self.words.len() && self.words[word_index] & (1u64 << (index & 63)) != 0
    }

    fn word_bits(&self, word_index: usize) -> u64 {
        self.words.get(word_index).copied().unwrap_or(0)
    }

    /// Every index in `[from, to)` has its forbidden bit set.
    fn forbidden_bits_all_set(&self, from: u32, to: u32) -> bool {
        range_bits_all_set(|word_index| self.word_bits(word_index), from, to)
    }
}

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
                name: "red_black_knights",
                def: GameDefinition::red_black_knights,
                checksums: [
                    15_737_156_276_822_775_461,
                    5_149_276_635_673_381_925,
                    561_431_110_996_648_581,
                ],
            },
            GoldenCase {
                name: "three_knights",
                def: GameDefinition::three_knights,
                checksums: [
                    16_115_999_991_126_781_684,
                    10_088_445_098_850_287_540,
                    7_584_768_825_753_057_092,
                ],
            },
            GoldenCase {
                name: "four_classic_leapers",
                def: GameDefinition::four_classic_leapers,
                checksums: [
                    5_964_283_847_930_621_157,
                    6_946_720_379_821_596_453,
                    6_370_614_915_775_779_925,
                ],
            },
            GoldenCase {
                name: "six_guards",
                def: GameDefinition::six_guards,
                checksums: [
                    6_480_521_862_097_834_845,
                    15_603_942_777_120_392_349,
                    1_643_601_116_650_407_053,
                ],
            },
            GoldenCase {
                name: "fusion_piece_freeforall",
                def: GameDefinition::fusion_piece_freeforall,
                checksums: [
                    8_459_319_956_822_578_164,
                    6_307_119_068_148_425_140,
                    12_399_277_720_126_721_092,
                ],
            },
        ]
    }

    #[test]
    fn forbidden_bits_all_set_covers_word_tail() {
        let mut set = ForbiddenSet::new();
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
        let def = GameDefinition::red_black_knights();
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
        let def = GameDefinition::red_black_knights();
        let mut sim = Simulation::new(&def);
        sim.step_turn(&def);
        sim.step_turn(&def);

        let red_xy = sim.cursor_positions[1];
        for &(dx, dy) in &def.armies[1].piece.valid_moves {
            let attacked = xy_to_index(red_xy.0 + dx, red_xy.1 + dy);
            assert!(sim.forbidden[0].contains_index(attacked));
        }

        let black_xy = (0, 0);
        let attacked = xy_to_index(black_xy.0 + 1, black_xy.1 + 2);
        assert!(sim.forbidden[1].contains_index(attacked));
        assert!(sim.occupancy.contains_index(0));
        assert!(sim.occupancy.contains_index(1));
    }

    #[test]
    fn red_black_first_sixteen_placements_are_stable() {
        let def = GameDefinition::red_black_knights();
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
