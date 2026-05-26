use bevy::prelude::Color;
use rand::Rng;
use rand::seq::SliceRandom;

use crate::model::{Army, ArmyId, GameDefinition, PieceDef};

/// Mirror attack cells around the piece (vertical = left/right, horizontal = up/down).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AttackSymmetry {
    #[default]
    None,
    /// Mirror across the vertical axis: `(x, y)` ↔ `(-x, y)`.
    Vertical,
    /// Mirror across the horizontal axis: `(x, y)` ↔ `(x, -y)`.
    Horizontal,
    Both,
}

impl AttackSymmetry {
    pub const ALL: [Self; 4] = [Self::None, Self::Vertical, Self::Horizontal, Self::Both];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Vertical => "Vertical",
            Self::Horizontal => "Horizontal",
            Self::Both => "Both",
        }
    }
}

/// Settings for the “Generate random pieces” action in the UI.
#[derive(Clone, Debug)]
pub struct RandomGenConfig {
    pub army_count_min: u32,
    pub army_count_max: u32,
    /// Chebyshev distance from the piece: minimum ring included when sampling attack cells.
    pub attack_radius_min: i32,
    /// Chebyshev distance from the piece: maximum ring included when sampling attack cells.
    pub attack_radius_max: i32,
    /// Per eligible cell probability of being an attacked square (0..1).
    pub pattern_density: f32,
    /// Per other-piece probability that this piece is blocked by them (0..1).
    pub blocked_by_density: f32,
    pub attack_symmetry: AttackSymmetry,
}

impl Default for RandomGenConfig {
    fn default() -> Self {
        Self {
            army_count_min: 2,
            army_count_max: 6,
            attack_radius_min: 1,
            attack_radius_max: 4,
            pattern_density: 0.35,
            blocked_by_density: 0.5,
            attack_symmetry: AttackSymmetry::None,
        }
    }
}

impl RandomGenConfig {
    pub fn sanitize(&mut self) {
        if self.army_count_max < self.army_count_min {
            std::mem::swap(&mut self.army_count_min, &mut self.army_count_max);
        }
        self.army_count_min = self.army_count_min.max(1);
        self.army_count_max = self.army_count_max.max(self.army_count_min);
        if self.attack_radius_max < self.attack_radius_min {
            std::mem::swap(&mut self.attack_radius_min, &mut self.attack_radius_max);
        }
        self.attack_radius_min = self.attack_radius_min.max(1);
        self.attack_radius_max = self.attack_radius_max.max(self.attack_radius_min);
        self.pattern_density = self.pattern_density.clamp(0.0, 1.0);
        self.blocked_by_density = self.blocked_by_density.clamp(0.0, 1.0);
    }
}

pub fn generate_random_game(config: &RandomGenConfig, rng: &mut impl Rng) -> GameDefinition {
    let mut cfg = config.clone();
    cfg.sanitize();

    let n = if cfg.army_count_min == cfg.army_count_max {
        cfg.army_count_min as usize
    } else {
        rng.random_range(cfg.army_count_min..=cfg.army_count_max) as usize
    };

    let colors = random_army_palette(rng, n);

    let armies: Vec<Army> = (0..n)
        .map(|i| {
            let piece = PieceDef {
                valid_moves: random_attack_pattern(
                    rng,
                    cfg.attack_radius_min,
                    cfg.attack_radius_max,
                    cfg.pattern_density,
                    cfg.attack_symmetry,
                ),
            };
            let blocked_by = random_blocked_by(rng, i, n, cfg.blocked_by_density);
            Army {
                name: format!("Piece {i}"),
                color: colors[i],
                piece,
                blocked_by,
            }
        })
        .collect();

    let mut turn_order: Vec<ArmyId> = (0..n).collect();
    turn_order.shuffle(rng);

    GameDefinition {
        armies,
        turn_order,
    }
}

fn chebyshev(x: i32, y: i32) -> i32 {
    x.abs().max(y.abs())
}

pub fn apply_attack_symmetry(moves: &mut Vec<(i32, i32)>, symmetry: AttackSymmetry) {
    if symmetry == AttackSymmetry::None {
        return;
    }
    let seeds: Vec<(i32, i32)> = moves
        .iter()
        .copied()
        .filter(|&(x, y)| x != 0 || y != 0)
        .collect();
    for (x, y) in seeds {
        match symmetry {
            AttackSymmetry::None => {}
            AttackSymmetry::Vertical => push_if_legal(moves, -x, y),
            AttackSymmetry::Horizontal => push_if_legal(moves, x, -y),
            AttackSymmetry::Both => {
                push_if_legal(moves, -x, y);
                push_if_legal(moves, x, -y);
                push_if_legal(moves, -x, -y);
            }
        }
    }
    normalize_moves(moves);
}

fn push_if_legal(moves: &mut Vec<(i32, i32)>, x: i32, y: i32) {
    if x != 0 || y != 0 {
        moves.push((x, y));
    }
}

fn random_attack_pattern(
    rng: &mut impl Rng,
    radius_min: i32,
    radius_max: i32,
    density: f32,
    symmetry: AttackSymmetry,
) -> Vec<(i32, i32)> {
    let mut moves = Vec::new();
    for y in -radius_max..=radius_max {
        for x in -radius_max..=radius_max {
            if x == 0 && y == 0 {
                continue;
            }
            let d = chebyshev(x, y);
            if d < radius_min || d > radius_max {
                continue;
            }
            if rng.random::<f32>() < density {
                moves.push((x, y));
            }
        }
    }
    if moves.is_empty() {
        moves.push((radius_min, 0));
    }
    apply_attack_symmetry(&mut moves, symmetry);
    normalize_moves(&mut moves);
    if moves.is_empty() {
        moves.push((radius_min, 0));
        apply_attack_symmetry(&mut moves, symmetry);
        normalize_moves(&mut moves);
    }
    moves
}

fn normalize_moves(moves: &mut Vec<(i32, i32)>) {
    moves.retain(|&(x, y)| x != 0 || y != 0);
    moves.sort_by_key(|&(x, y)| (x, y));
    moves.dedup();
}

fn random_blocked_by(
    rng: &mut impl Rng,
    army: ArmyId,
    n: usize,
    density: f32,
) -> Vec<ArmyId> {
    (0..n)
        .filter(|&other| other != army && rng.random::<f32>() < density)
        .collect()
}

fn random_army_palette(rng: &mut impl Rng, n: usize) -> Vec<Color> {
    if n == 0 {
        return Vec::new();
    }
    let base = rng.random_range(0.0f32..360.0);
    let mut colors = match rng.random_range(0..6) {
        0 => palette_analogous(rng, base, n),
        1 => palette_triadic(rng, base, n),
        2 => palette_complementary(rng, base, n),
        3 => palette_split_complementary(rng, base, n),
        4 => palette_tetradic(rng, base, n),
        _ => palette_accents(rng, base, n),
    };
    colors.shuffle(rng);
    colors
}

fn palette_color(rng: &mut impl Rng, hue: f32, slot: usize, slots: usize) -> Color {
    let hue_jitter = rng.random_range(-6.0..6.0);
    let s = rng.random_range(0.58..0.86);
    let l_center = 0.48 + (slot as f32 / slots.max(1) as f32) * 0.14;
    let l = (l_center + rng.random_range(-0.05..0.05)).clamp(0.38, 0.68);
    hsl(hue + hue_jitter, s, l)
}

fn colors_from_anchors(rng: &mut impl Rng, anchors: &[f32], n: usize) -> Vec<Color> {
    (0..n)
        .map(|i| {
            let h = anchors[i % anchors.len()];
            palette_color(rng, h, i, n)
        })
        .collect()
}

/// Neighbouring hues on the wheel — cohesive but not monochrome.
fn palette_analogous(rng: &mut impl Rng, base: f32, n: usize) -> Vec<Color> {
    let spread = rng.random_range(35.0..85.0);
    let start = base - spread * 0.5;
    (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1).max(1) as f32;
            palette_color(rng, start + t * spread, i, n)
        })
        .collect()
}

fn palette_triadic(rng: &mut impl Rng, base: f32, n: usize) -> Vec<Color> {
    colors_from_anchors(rng, &[base, base + 120.0, base + 240.0], n)
}

fn palette_complementary(rng: &mut impl Rng, base: f32, n: usize) -> Vec<Color> {
    colors_from_anchors(rng, &[base, base + 180.0], n)
}

fn palette_split_complementary(rng: &mut impl Rng, base: f32, n: usize) -> Vec<Color> {
    colors_from_anchors(rng, &[base, base + 150.0, base + 210.0], n)
}

fn palette_tetradic(rng: &mut impl Rng, base: f32, n: usize) -> Vec<Color> {
    colors_from_anchors(rng, &[base, base + 90.0, base + 180.0, base + 270.0], n)
}

/// One vivid hue plus muted neighbours — good contrast on a dark board.
fn palette_accents(rng: &mut impl Rng, base: f32, n: usize) -> Vec<Color> {
    let mut out = Vec::with_capacity(n);
    let accent_idx = rng.random_range(0..n);
    for i in 0..n {
        let h = base + (i as f32 - accent_idx as f32) * rng.random_range(18.0..32.0);
        if i == accent_idx {
            out.push(hsl(
                h + rng.random_range(-4.0..4.0),
                rng.random_range(0.72..0.92),
                rng.random_range(0.50..0.62),
            ));
        } else {
            out.push(hsl(
                h + rng.random_range(-8.0..8.0),
                rng.random_range(0.45..0.62),
                rng.random_range(0.42..0.56),
            ));
        }
    }
    out
}

fn hsl(h: f32, s: f32, l: f32) -> Color {
    Color::hsl(h.rem_euclid(360.0), s.clamp(0.0, 1.0), l.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn sanitized_config_produces_stable_army_count_bounds() {
        let mut cfg = RandomGenConfig {
            army_count_min: 8,
            army_count_max: 2,
            attack_radius_min: 5,
            attack_radius_max: 1,
            pattern_density: 2.0,
            blocked_by_density: -1.0,
            attack_symmetry: AttackSymmetry::Both,
        };
        cfg.sanitize();
        assert_eq!(cfg.army_count_min, 2);
        assert_eq!(cfg.army_count_max, 8);
        assert_eq!(cfg.attack_radius_min, 1);
        assert_eq!(cfg.attack_radius_max, 5);
        assert_eq!(cfg.pattern_density, 1.0);
        assert_eq!(cfg.blocked_by_density, 0.0);
    }

    #[test]
    fn vertical_symmetry_pairs_moves() {
        let mut moves = vec![(2, 1)];
        apply_attack_symmetry(&mut moves, AttackSymmetry::Vertical);
        assert!(moves.contains(&(-2, 1)));
    }

    #[test]
    fn both_symmetry_fills_quadrants() {
        let mut moves = vec![(2, 1)];
        apply_attack_symmetry(&mut moves, AttackSymmetry::Both);
        assert!(moves.contains(&(-2, 1)));
        assert!(moves.contains(&(2, -1)));
        assert!(moves.contains(&(-2, -1)));
    }

    #[test]
    fn random_palette_has_one_color_per_army() {
        let mut rng = StdRng::seed_from_u64(99);
        for n in 2..=8 {
            let colors = random_army_palette(&mut rng, n);
            assert_eq!(colors.len(), n);
        }
    }

    #[test]
    fn generate_respects_count_and_move_radius() {
        let cfg = RandomGenConfig {
            army_count_min: 3,
            army_count_max: 3,
            attack_radius_min: 2,
            attack_radius_max: 2,
            pattern_density: 1.0,
            blocked_by_density: 0.0,
            attack_symmetry: AttackSymmetry::None,
        };
        let mut rng = StdRng::seed_from_u64(42);
        let def = generate_random_game(&cfg, &mut rng);
        assert_eq!(def.armies.len(), 3);
        for army in &def.armies {
            assert!(!army.piece.valid_moves.is_empty());
            for &(x, y) in &army.piece.valid_moves {
                assert_eq!(chebyshev(x, y), 2);
            }
        }
    }
}
