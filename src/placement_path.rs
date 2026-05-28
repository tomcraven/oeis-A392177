use crate::index_order::VisitOrder;
use crate::model::{GameDefinition, PieceId};
use crate::sim::{ScanSkips, Simulation};

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

/// Next same-piece placement after `(index, piece_id)` in turn order, if any.
pub fn next_same_piece_after(
    placements: &[(u32, PieceId)],
    index: u32,
    piece_id: PieceId,
) -> Option<u32> {
    let mut after_current = false;
    for &(idx, pid) in placements {
        if pid != piece_id {
            continue;
        }
        if after_current {
            return Some(idx);
        }
        if idx == index {
            after_current = true;
        }
    }
    None
}

fn turn_index_for_placement(
    placements: &[(u32, PieceId)],
    index: u32,
    piece_id: PieceId,
) -> Option<usize> {
    placements
        .iter()
        .position(|&(idx, pid)| idx == index && pid == piece_id)
}

/// Sim state immediately before the turn that placed at `(index, piece_id)`.
pub fn replay_sim_before_placement(
    def: &GameDefinition,
    visit_order: VisitOrder,
    placements: &[(u32, PieceId)],
    index: u32,
    piece_id: PieceId,
) -> Option<Simulation> {
    let turn_at = turn_index_for_placement(placements, index, piece_id)?;
    let mut sim = Simulation::new(def, visit_order);
    for _ in 0..turn_at {
        if !sim.step_turn(def) {
            return None;
        }
    }
    Some(sim)
}

/// Skips on the scan that placed at `(index, piece_id)`, plus sim state for attribution lines.
pub fn scan_skips_to_placement(
    def: &GameDefinition,
    visit_order: VisitOrder,
    placements: &[(u32, PieceId)],
    index: u32,
    piece_id: PieceId,
) -> Option<(ScanSkips, Simulation)> {
    let sim = replay_sim_before_placement(def, visit_order, placements, index, piece_id)?;
    let skips = sim.scan_skips_on_next_scan(def);
    Some((skips, sim))
}

/// Forbidden (attacked, empty) skips only — see [`scan_skips_to_placement`] for the full split.
pub fn forbidden_skips_to_placement(
    def: &GameDefinition,
    visit_order: VisitOrder,
    placements: &[(u32, PieceId)],
    index: u32,
    piece_id: PieceId,
) -> Vec<u32> {
    scan_skips_to_placement(def, visit_order, placements, index, piece_id)
        .map(|(skips, _)| skips.forbidden)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index_order::VisitOrder;
    use crate::model::GameDefinition;
    use crate::sim::Simulation;

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
        assert_eq!(next_same_piece_after(&sim.placements, 2, 0), Some(5));
        assert_eq!(next_same_piece_after(&sim.placements, 20, 0), None);
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

    #[test]
    fn scan_skips_replay_matches_scan_preview() {
        let def = GameDefinition::knight_2_pairwise();
        let order = VisitOrder::default();
        let mut sim = Simulation::new(&def, order);
        for turn in 0..32 {
            let expected = sim.scan_skips_on_next_scan(&def);
            assert!(sim.step_turn(&def), "turn {turn}");
            let (idx, pid) = sim.placements[turn];
            let (replay, _) = scan_skips_to_placement(
                &def,
                order,
                &sim.placements[..=turn],
                idx,
                pid,
            )
            .expect("replay");
            assert_eq!(replay, expected, "turn {turn} placement ({idx}, {pid})");
        }
    }
}
