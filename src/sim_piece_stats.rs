//! Per-piece simulation statistics for the Debug panel.
//!
//! Everything here is derived in O(pieces) from the worker snapshot ([`SimDisplay`]) plus the
//! game definition — no simulation replay and no per-frame scan of the placement log. A piece's
//! `cursor` is its monotonic count of examined spiral cells, so "scan work" and "skips" fall out
//! of data the worker already maintains; placement counts and spiral reach come from the cheap
//! per-placement [`PieceTally`](crate::sim::PieceTally). This keeps the panel free of any cap and
//! adds nothing to the simulation hot loop.

use bevy::prelude::Color;

use crate::model::GameDefinition;
use crate::model::PieceId;
use crate::sim_worker::SimDisplay;

/// One row in the piece stats UI.
#[derive(Clone, Debug)]
pub struct PieceStatLine {
    pub piece_id: PieceId,
    pub name: String,
    pub color: Color,
    pub enabled: bool,
    /// Placements committed by this piece.
    pub placements: u64,
    /// Share of all placements on the board (%).
    pub placement_share_pct: f64,
    /// Spiral cells this piece has examined (its monotonic cursor) — its scan workload.
    pub cells_scanned: u64,
    /// Share of total scan work across pieces (%).
    pub work_share_pct: f64,
    /// Average cells skipped (occupied or forbidden) before each successful placement.
    pub avg_skips_per_placement: Option<f64>,
    /// Largest spiral index this piece has placed on (how far out it has reached).
    pub spiral_reach: Option<u32>,
    /// Average spiral-index gap between consecutive placements of this piece.
    pub avg_spiral_gap: Option<f64>,
}

/// Build the per-piece stat rows from the latest worker snapshot. Cheap (O(pieces)); call inline.
pub fn piece_stat_lines(def: &GameDefinition, display: &SimDisplay) -> Vec<PieceStatLine> {
    let total_placements = display.placements.len().max(1) as f64;

    let total_cells_scanned: u64 = def
        .pieces
        .iter()
        .enumerate()
        .filter(|(id, p)| p.enabled || tally_placements(display, *id) > 0)
        .map(|(id, _)| display.cursors.get(id).copied().unwrap_or(0) as u64)
        .sum();
    let work_denom = total_cells_scanned.max(1) as f64;

    let mut lines: Vec<PieceStatLine> = def
        .pieces
        .iter()
        .enumerate()
        .filter_map(|(piece_id, piece)| {
            let tally = display.piece_tally.get(piece_id).copied().unwrap_or_default();
            let placements = tally.placements as u64;
            if !piece.enabled && placements == 0 {
                return None;
            }
            let cells_scanned = display.cursors.get(piece_id).copied().unwrap_or(0) as u64;

            let avg_skips = if placements > 0 {
                Some(cells_scanned.saturating_sub(placements) as f64 / placements as f64)
            } else {
                None
            };

            let spiral_reach = (placements > 0).then_some(tally.last_index);
            let avg_gap = if placements > 1 {
                Some(
                    tally.last_index.saturating_sub(tally.first_index) as f64
                        / (placements - 1) as f64,
                )
            } else {
                None
            };

            Some(PieceStatLine {
                piece_id,
                name: piece.name.clone(),
                color: piece.color,
                enabled: piece.enabled,
                placements,
                placement_share_pct: 100.0 * placements as f64 / total_placements,
                cells_scanned,
                work_share_pct: 100.0 * cells_scanned as f64 / work_denom,
                avg_skips_per_placement: avg_skips,
                spiral_reach,
                avg_spiral_gap: avg_gap,
            })
        })
        .collect();

    lines.sort_by(|a, b| {
        b.placements
            .cmp(&a.placements)
            .then_with(|| a.piece_id.cmp(&b.piece_id))
    });

    lines
}

fn tally_placements(display: &SimDisplay, piece_id: PieceId) -> u32 {
    display
        .piece_tally
        .get(piece_id)
        .map(|t| t.placements)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index_order::VisitOrder;
    use crate::model::GameDefinition;
    use crate::sim::Simulation;

    #[test]
    fn stats_match_a_forward_sim() {
        let def = GameDefinition::knight_2_pairwise();
        let mut sim = Simulation::new(&def, VisitOrder::default());
        for _ in 0..500 {
            if !sim.step_turn(&def) {
                break;
            }
        }

        // Independent ground truth from the placement log.
        let mut counts = [0u64; 2];
        let mut first = [None; 2];
        let mut last = [0u32; 2];
        for &(idx, pid) in sim.placements.as_slice() {
            counts[pid] += 1;
            first[pid].get_or_insert(idx);
            last[pid] = idx;
        }

        let display = SimDisplay {
            cursors: sim.cursors.clone(),
            piece_tally: sim.piece_tally().to_vec(),
            placements: sim.placements.arc(),
            turn_step: sim.turn_step,
            ..Default::default()
        };

        let lines = piece_stat_lines(&def, &display);
        assert_eq!(lines.len(), 2);
        let total: u64 = counts.iter().sum();
        for line in &lines {
            let pid = line.piece_id;
            assert_eq!(line.placements, counts[pid]);
            assert_eq!(line.spiral_reach, Some(last[pid]));
            assert!(
                (line.placement_share_pct - 100.0 * counts[pid] as f64 / total as f64).abs()
                    < 1e-6
            );
            let cursor = display.cursors[pid] as u64;
            let expected_avg = cursor.saturating_sub(counts[pid]) as f64 / counts[pid] as f64;
            assert!((line.avg_skips_per_placement.unwrap() - expected_avg).abs() < 1e-6);
        }
    }
}
