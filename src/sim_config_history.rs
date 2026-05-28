use crate::camera_config::CameraSessionConfig;
use crate::model::GameDefinition;

const MAX_ENTRIES: usize = 256;

#[derive(Clone, Debug)]
pub struct SimConfigSnapshot {
    pub game: GameDefinition,
    pub camera: CameraSessionConfig,
}

/// Linear buffer of past simulation configurations ([`GameDefinition::same_sim_state`])
/// with the camera view at the time each entry was recorded. New edits append even when
/// browsing an earlier slot (later entries are kept).
#[derive(Clone, Debug, Default)]
pub struct SimConfigHistory {
    entries: Vec<SimConfigSnapshot>,
    index: usize,
}

impl SimConfigHistory {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn position(&self) -> usize {
        self.index
    }

    pub fn can_step_back(&self) -> bool {
        !self.entries.is_empty() && self.index > 0
    }

    pub fn can_step_forward(&self) -> bool {
        !self.entries.is_empty() && self.index + 1 < self.entries.len()
    }

    pub fn current(&self) -> Option<&SimConfigSnapshot> {
        self.entries.get(self.index)
    }

    /// Replace the buffer with a single entry (session load / cold start).
    pub fn reset_to(&mut self, snapshot: SimConfigSnapshot) {
        self.entries = vec![snapshot];
        self.index = 0;
    }

    /// Record a new simulation configuration after a user-driven change (not camera-only).
    pub fn commit(&mut self, game: GameDefinition, camera: CameraSessionConfig) {
        if self.entries.is_empty() {
            self.reset_to(SimConfigSnapshot { game, camera });
            return;
        }
        if self
            .entries
            .get(self.index)
            .is_some_and(|e| e.game.same_sim_state(&game))
        {
            return;
        }
        self.entries.push(SimConfigSnapshot { game, camera });
        while self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
            self.index = self.index.saturating_sub(1);
        }
        self.index = self.entries.len() - 1;
    }

    /// Move within the buffer; returns the snapshot at the new position.
    pub fn step(&mut self, delta: i32) -> Option<SimConfigSnapshot> {
        if self.entries.is_empty() {
            return None;
        }
        let new_index = self.index as i32 + delta;
        if new_index < 0 || new_index as usize >= self.entries.len() {
            return None;
        }
        self.index = new_index as usize;
        Some(self.entries[self.index].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GameDefinition;

    fn cam(zoom: f32) -> CameraSessionConfig {
        CameraSessionConfig {
            x: 1.0,
            y: 2.0,
            zoom,
        }
    }

    #[test]
    fn commit_appends_from_middle_without_dropping_later_entries() {
        let a = GameDefinition::knight_2_pairwise();
        let mut b = a.clone();
        b.turn_order.reverse();
        let mut c = a.clone();
        c.pieces[0].piece.valid_moves.push((3, 4));
        let mut d = a.clone();
        d.pieces[0].enabled = false;

        let mut h = SimConfigHistory::default();
        h.reset_to(SimConfigSnapshot {
            game: a.clone(),
            camera: cam(1.0),
        });
        h.commit(b.clone(), cam(2.0));
        h.commit(c.clone(), cam(3.0));
        assert_eq!(h.len(), 3);

        let back = h.step(-1).unwrap();
        assert!(back.game.same_sim_state(&b));

        h.commit(d.clone(), cam(4.0));
        assert_eq!(h.len(), 4);
        assert_eq!(h.position(), 3);
        assert!(h.current().unwrap().game.same_sim_state(&d));
        assert!(h.can_step_back());
        assert!(!h.can_step_forward());
    }

    #[test]
    fn commit_skips_when_only_camera_would_differ() {
        let game = GameDefinition::knight_2_pairwise();
        let mut h = SimConfigHistory::default();
        h.reset_to(SimConfigSnapshot {
            game: game.clone(),
            camera: cam(1.0),
        });
        h.commit(game.clone(), cam(9.0));
        assert_eq!(h.len(), 1);
        assert_eq!(h.current().unwrap().camera.zoom, 1.0);
    }

    #[test]
    fn step_skips_when_out_of_range() {
        let mut h = SimConfigHistory::default();
        h.reset_to(SimConfigSnapshot {
            game: GameDefinition::knight_2_pairwise(),
            camera: cam(1.0),
        });
        assert!(h.step(-1).is_none());
        assert!(h.step(1).is_none());
    }
}
