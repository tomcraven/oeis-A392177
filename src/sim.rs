use std::collections::HashMap;

use crate::model::{ArmyId, GameDefinition};
use crate::spiral::{spiral_step, xy_to_index};
use bevy::prelude::{FromWorld, Resource, World};

#[derive(Resource)]
pub struct Simulation {
    pub occupancy: HashMap<u32, ArmyId>,
    occupied_cells: ForbiddenSet,
    /// Per-army attacked cells by armies this army respects.
    forbidden: Vec<ForbiddenSet>,
    threatened_for: Vec<Vec<ArmyId>>,
    pub cursors: Vec<u32>,
    cursor_positions: Vec<(i32, i32)>,
    pub turn_step: usize,
    pub placements: Vec<(u32, ArmyId)>,
}

impl Simulation {
    pub fn new(def: &GameDefinition) -> Self {
        Self {
            occupancy: HashMap::new(),
            occupied_cells: ForbiddenSet::new(),
            forbidden: vec![ForbiddenSet::new(); def.armies.len()],
            threatened_for: threatened_for(def),
            cursors: vec![0; def.armies.len()],
            cursor_positions: vec![(0, 0); def.armies.len()],
            turn_step: 0,
            placements: Vec::new(),
        }
    }

    pub fn reset(&mut self, def: &GameDefinition) {
        self.occupancy.clear();
        self.occupied_cells = ForbiddenSet::new();
        self.forbidden = vec![ForbiddenSet::new(); def.armies.len()];
        self.threatened_for = threatened_for(def);
        self.cursors = vec![0; def.armies.len()];
        self.cursor_positions = vec![(0, 0); def.armies.len()];
        self.turn_step = 0;
        self.placements.clear();
    }

    fn place(&mut self, def: &GameDefinition, index: u32, xy: (i32, i32), army_id: ArmyId) {
        self.occupancy.insert(index, army_id);
        self.record_forbidden(def, index, xy, army_id);
        self.placements.push((index, army_id));
    }

    fn record_forbidden(
        &mut self,
        def: &GameDefinition,
        index: u32,
        xy: (i32, i32),
        army_id: ArmyId,
    ) {
        self.occupied_cells.insert(index);
        let moves = &def.army(army_id).piece.valid_moves;
        for &target_army in &self.threatened_for[army_id] {
            for &(dx, dy) in moves {
                self.forbidden[target_army].insert(xy_to_index(xy.0 + dx, xy.1 + dy));
            }
        }
    }

    /// One army takes a turn: scan from its cursor for the first legal square.
    pub fn step_turn(&mut self, def: &GameDefinition) -> bool {
        if def.turn_order.is_empty() {
            return false;
        }
        let army_id = def.turn_order[self.turn_step % def.turn_order.len()];
        self.turn_step += 1;

        loop {
            let index = self.cursors[army_id];
            let xy = self.cursor_positions[army_id];
            if !self.occupied_cells.contains_index(index)
                && !self.forbidden[army_id].contains_index(index)
            {
                self.place(def, index, xy, army_id);
                return true;
            }

            self.cursors[army_id] = self.cursors[army_id].saturating_add(1);
            self.cursor_positions[army_id] = spiral_step(xy);
            if self.cursors[army_id] == u32::MAX {
                return false;
            }
        }
    }

    pub fn needs_work(&self, target_index: u32) -> bool {
        self.cursors.iter().any(|&c| c <= target_index)
    }

    pub fn advance_budget(&mut self, def: &GameDefinition, target_index: u32, max_turns: u32) {
        let mut turns = 0u32;
        while self.needs_work(target_index) && turns < max_turns {
            self.step_turn(def);
            turns += 1;
        }
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
        self.words
            .get(word_index)
            .is_some_and(|word| word & (1u64 << (index & 63)) != 0)
    }
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
        assert!(sim.occupied_cells.contains_index(0));
        assert!(sim.occupied_cells.contains_index(1));
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
