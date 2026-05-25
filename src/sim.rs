use std::collections::{HashMap, HashSet};

use crate::model::{ArmyId, GameDefinition};
use crate::spiral::{spiral_step, xy_to_index};
use bevy::prelude::{FromWorld, Resource, World};

#[derive(Resource)]
pub struct Simulation {
    pub occupancy: HashMap<u32, ArmyId>,
    /// Per-army forbidden cells: occupied cells plus attacks by armies this army respects.
    forbidden: Vec<HashSet<u32>>,
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
            forbidden: vec![HashSet::new(); def.armies.len()],
            threatened_for: threatened_for(def),
            cursors: vec![0; def.armies.len()],
            cursor_positions: vec![(0, 0); def.armies.len()],
            turn_step: 0,
            placements: Vec::new(),
        }
    }

    pub fn reset(&mut self, def: &GameDefinition) {
        self.occupancy.clear();
        self.forbidden = vec![HashSet::new(); def.armies.len()];
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
        let mut attacked_indices = Vec::with_capacity(def.army(army_id).piece.valid_moves.len());
        for &(dx, dy) in &def.army(army_id).piece.valid_moves {
            attacked_indices.push(xy_to_index(xy.0 + dx, xy.1 + dy));
        }

        for target_army in 0..self.forbidden.len() {
            self.forbidden[target_army].insert(index);
        }

        for &target_army in &self.threatened_for[army_id] {
            self.forbidden[target_army].extend(attacked_indices.iter().copied());
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
            if !self.forbidden[army_id].contains(&index) {
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
            assert!(sim.forbidden[0].contains(&attacked));
        }

        let black_xy = (0, 0);
        let attacked = xy_to_index(black_xy.0 + 1, black_xy.1 + 2);
        assert!(sim.forbidden[1].contains(&attacked));
        assert!(sim.forbidden[0].contains(&0));
        assert!(sim.forbidden[1].contains(&0));
    }
}
