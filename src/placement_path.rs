use crate::index_order::VisitOrder;
use crate::model::{GameDefinition, PieceId};
use crate::sim::Simulation;

/// Same-piece placement chain in turn order through `index` (inclusive).
///
/// Derived from the placement log only — no extra simulation state.
pub fn same_piece_path_to(
    placements: &[(u32, PieceId)],
    index: u32,
    piece_id: PieceId,
) -> Vec<u32> {
    let mut path = Vec::new();
    for &(idx, pid) in placements {
        if pid != piece_id {
            continue;
        }
        path.push(idx);
        if idx == index {
            return path;
        }
    }
    path
}

/// Forbidden cells the upcoming piece skips (not occupied) while scanning to place at `index`.
pub fn forbidden_skips_to_placement(
    def: &GameDefinition,
    visit_order: VisitOrder,
    placements: &[(u32, PieceId)],
    index: u32,
    piece_id: PieceId,
) -> Vec<u32> {
    let Some(turn_at) = placements
        .iter()
        .position(|&(idx, pid)| idx == index && pid == piece_id)
    else {
        return Vec::new();
    };
    let mut sim = Simulation::new(def, visit_order);
    for _ in 0..turn_at {
        if !sim.step_turn(def) {
            return Vec::new();
        }
    }
    sim.forbidden_skips_on_next_scan(def)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GameDefinition;
    use crate::sim::Simulation;
    use crate::index_order::VisitOrder;

    #[test]
    fn path_matches_interactive_red_black_early_placements() {
        let def = GameDefinition::knight_2_pairwise();
        let mut sim = Simulation::new(&def, VisitOrder::default());
        for _ in 0..6 {
            sim.step_turn(&def);
        }
        assert_eq!(
            same_piece_path_to(&sim.placements, 3, 1),
            vec![1, 3]
        );
        assert_eq!(
            same_piece_path_to(&sim.placements, 2, 0),
            vec![0, 2]
        );
    }

    #[test]
    fn forbidden_skips_replay_matches_scan_preview() {
        let def = GameDefinition::knight_2_pairwise();
        let order = VisitOrder::default();
        let mut sim = Simulation::new(&def, order);
        for turn in 0..32 {
            let expected = sim.forbidden_skips_on_next_scan(&def);
            assert!(sim.step_turn(&def), "turn {turn}");
            let (idx, pid) = sim.placements[turn];
            let replay = forbidden_skips_to_placement(
                &def,
                order,
                &sim.placements[..=turn],
                idx,
                pid,
            );
            assert_eq!(replay, expected, "turn {turn} placement ({idx}, {pid})");
        }
    }
}
