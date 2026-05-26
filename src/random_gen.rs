use bevy::prelude::Color;
use rand::Rng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use crate::model::{Army, ArmyId, GameDefinition, PieceDef};

/// Mirror attack cells around the piece (vertical = left/right, horizontal = up/down).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RandomGenConfig {
    pub army_count_min: u32,
    pub army_count_max: u32,
    /// Chebyshev distance from the piece: minimum ring included when sampling attack cells.
    pub attack_radius_min: i32,
    /// Chebyshev distance from the piece: maximum ring included when sampling attack cells.
    pub attack_radius_max: i32,
    /// Per eligible cell probability of being an attacked square (0..1).
    pub pattern_density: f32,
    pub attack_symmetry: AttackSymmetry,
    /// When true, every generated piece uses the same attack pattern.
    #[serde(default)]
    pub identical_pieces: bool,
}

impl Default for RandomGenConfig {
    fn default() -> Self {
        Self {
            army_count_min: 2,
            army_count_max: 3,
            attack_radius_min: 2,
            attack_radius_max: 3,
            pattern_density: 0.17,
            attack_symmetry: AttackSymmetry::Both,
            identical_pieces: false,
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

    let shared_moves = if cfg.identical_pieces {
        Some(random_attack_pattern(
            rng,
            cfg.attack_radius_min,
            cfg.attack_radius_max,
            cfg.pattern_density,
            cfg.attack_symmetry,
        ))
    } else {
        None
    };

    let armies: Vec<Army> = (0..n)
        .map(|i| {
            let valid_moves = if let Some(moves) = &shared_moves {
                moves.clone()
            } else {
                random_attack_pattern(
                    rng,
                    cfg.attack_radius_min,
                    cfg.attack_radius_max,
                    cfg.pattern_density,
                    cfg.attack_symmetry,
                )
            };
            let piece = PieceDef { valid_moves };
            let blocked_by = all_other_armies(i, n);
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

/// One representative per symmetry orbit so `pattern_density` is the per-cell inclusion rate
/// after mirroring, not before (independent rolls on mirrored copies overshoot badly).
fn is_canonical_attack_seed(x: i32, y: i32, symmetry: AttackSymmetry) -> bool {
    match symmetry {
        AttackSymmetry::None => true,
        AttackSymmetry::Vertical => x > 0 || (x == 0 && y > 0),
        AttackSymmetry::Horizontal => y > 0 || (y == 0 && x != 0),
        AttackSymmetry::Both => (x > 0 && y >= 0) || (x == 0 && y > 0),
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
            if !is_canonical_attack_seed(x, y, symmetry) {
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

fn all_other_armies(army: ArmyId, n: usize) -> Vec<ArmyId> {
    (0..n).filter(|&other| other != army).collect()
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
            let h: f32 = anchors[i % anchors.len()];
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
            attack_symmetry: AttackSymmetry::Both,
            identical_pieces: false,
        };
        cfg.sanitize();
        assert_eq!(cfg.army_count_min, 2);
        assert_eq!(cfg.army_count_max, 8);
        assert_eq!(cfg.attack_radius_min, 1);
        assert_eq!(cfg.attack_radius_max, 5);
        assert_eq!(cfg.pattern_density, 1.0);
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
    fn canonical_seeds_cover_each_orbit_once() {
        assert!(is_canonical_attack_seed(2, 1, AttackSymmetry::Both));
        assert!(!is_canonical_attack_seed(-2, 1, AttackSymmetry::Both));
        assert!(is_canonical_attack_seed(2, 0, AttackSymmetry::Vertical));
        assert!(!is_canonical_attack_seed(-2, 0, AttackSymmetry::Vertical));
        assert!(is_canonical_attack_seed(0, 3, AttackSymmetry::Horizontal));
        assert!(!is_canonical_attack_seed(0, -3, AttackSymmetry::Horizontal));
    }

    #[test]
    fn pattern_density_matches_eligible_fraction_with_symmetry() {
        fn eligible_count(rmin: i32, rmax: i32) -> usize {
            let mut n = 0usize;
            for y in -rmax..=rmax {
                for x in -rmax..=rmax {
                    if x == 0 && y == 0 {
                        continue;
                    }
                    let d = chebyshev(x, y);
                    if d >= rmin && d <= rmax {
                        n += 1;
                    }
                }
            }
            n
        }

        let cfg = RandomGenConfig {
            army_count_min: 1,
            army_count_max: 1,
            attack_radius_min: 1,
            attack_radius_max: 4,
            pattern_density: 0.2,
            attack_symmetry: AttackSymmetry::Both,
            identical_pieces: false,
        };
        let eligible = eligible_count(cfg.attack_radius_min, cfg.attack_radius_max);
        let mut rng = StdRng::seed_from_u64(2026);
        let mut filled = 0usize;
        let trials = 400usize;
        for _ in 0..trials {
            let def = generate_random_game(&cfg, &mut rng);
            filled += def.armies[0].piece.valid_moves.len();
        }
        let ratio = filled as f32 / (trials * eligible) as f32;
        assert!(
            (0.14..=0.26).contains(&ratio),
            "expected ~0.2 fill ratio, got {ratio}"
        );
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
    fn identical_pieces_share_attack_pattern() {
        let cfg = RandomGenConfig {
            army_count_min: 4,
            army_count_max: 4,
            attack_radius_min: 2,
            attack_radius_max: 3,
            pattern_density: 0.25,
            attack_symmetry: AttackSymmetry::Both,
            identical_pieces: true,
        };
        let mut rng = StdRng::seed_from_u64(123);
        let def = generate_random_game(&cfg, &mut rng);
        assert_eq!(def.armies.len(), 4);
        let first = &def.armies[0].piece.valid_moves;
        for army in &def.armies[1..] {
            assert_eq!(&army.piece.valid_moves, first);
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
            attack_symmetry: AttackSymmetry::None,
            identical_pieces: false,
        };
        let mut rng = StdRng::seed_from_u64(42);
        let def = generate_random_game(&cfg, &mut rng);
        assert_eq!(def.armies.len(), 3);
        for (i, army) in def.armies.iter().enumerate() {
            assert!(!army.piece.valid_moves.is_empty());
            for &(x, y) in &army.piece.valid_moves {
                assert_eq!(chebyshev(x, y), 2);
            }
            assert_eq!(army.blocked_by, all_other_armies(i, 3));
        }
    }
}
