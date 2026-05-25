use bevy::prelude::{Color, Resource};

pub type ArmyId = usize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PieceDef {
    pub valid_moves: Vec<(i32, i32)>,
}

impl PieceDef {
    pub fn knight() -> Self {
        Self {
            valid_moves: vec![
                (1, 2),
                (2, 1),
                (2, -1),
                (1, -2),
                (-1, -2),
                (-2, -1),
                (-2, 1),
                (-1, 2),
            ],
        }
    }

    /// Single-step orthogonal (one-square rook).
    pub fn wazir() -> Self {
        Self {
            valid_moves: vec![(1, 0), (-1, 0), (0, 1), (0, -1)],
        }
    }

    /// Two-step orthogonal jump (dabbaba).
    pub fn dabbaba() -> Self {
        Self {
            valid_moves: vec![(2, 0), (-2, 0), (0, 2), (0, -2)],
        }
    }

    /// Single-step diagonal (ferz).
    pub fn ferz() -> Self {
        Self {
            valid_moves: vec![(1, 1), (1, -1), (-1, 1), (-1, -1)],
        }
    }

    /// Two-step diagonal jump (alfil).
    pub fn alfil() -> Self {
        Self {
            valid_moves: vec![(2, 2), (2, -2), (-2, 2), (-2, -2)],
        }
    }

    pub fn king() -> Self {
        let mut m = Self::wazir().valid_moves;
        m.extend(Self::ferz().valid_moves);
        Self { valid_moves: m }
    }

    pub fn camel() -> Self {
        Self {
            valid_moves: vec![
                (1, 3),
                (3, 1),
                (3, -1),
                (1, -3),
                (-1, -3),
                (-3, -1),
                (-3, 1),
                (-1, 3),
            ],
        }
    }

    pub fn zebra() -> Self {
        Self {
            valid_moves: vec![
                (2, 3),
                (3, 2),
                (3, -2),
                (2, -3),
                (-2, -3),
                (-3, -2),
                (-3, 2),
                (-2, 3),
            ],
        }
    }

    /// Knight plus camel (fictional long leaper).
    pub fn hippogriff() -> Self {
        Self::merge(&[Self::knight(), Self::camel()])
    }

    /// Orthogonal and diagonal one-step (king without castling semantics).
    pub fn guard() -> Self {
        Self::king()
    }

    /// Wide but short leaper: (±3, ±1) and (±1, ±3) only.
    pub fn giraffe() -> Self {
        Self {
            valid_moves: vec![
                (3, 1),
                (1, 3),
                (3, -1),
                (1, -3),
                (-3, -1),
                (-1, -3),
                (-3, 1),
                (-1, 3),
            ],
        }
    }

    /// Jumps three orthogonally (fictional trebuchet).
    pub fn trebuchet() -> Self {
        Self {
            valid_moves: vec![(3, 0), (-3, 0), (0, 3), (0, -3)],
        }
    }

    /// Combines move sets and deduplicates.
    pub fn merge(pieces: &[Self]) -> Self {
        let mut valid_moves = Vec::new();
        for p in pieces {
            valid_moves.extend_from_slice(&p.valid_moves);
        }
        valid_moves.sort_by_key(|&(x, y)| (x, y));
        valid_moves.dedup();
        Self { valid_moves }
    }
}

#[derive(Clone, Debug)]
pub struct Army {
    pub name: String,
    pub color: Color,
    pub piece: PieceDef,
    /// Armies whose pieces block placement on squares they attack.
    pub blocked_by: Vec<ArmyId>,
}

#[derive(Clone, Debug, Resource)]
pub struct GameDefinition {
    pub armies: Vec<Army>,
    /// Round-robin turn order by army index.
    pub turn_order: Vec<ArmyId>,
}

impl Default for GameDefinition {
    fn default() -> Self {
        Self::knight_2_pairwise()
    }
}

impl GameDefinition {
    /// Whether two definitions would run the same simulation and army colors.
    pub fn same_applied_state(&self, other: &Self) -> bool {
        self.turn_order == other.turn_order
            && self.armies.len() == other.armies.len()
            && self.armies.iter().zip(&other.armies).all(|(a, b)| {
                a.piece == b.piece && a.blocked_by == b.blocked_by && a.color == b.color
            })
    }

    pub fn knight_2_pairwise() -> Self {
        pairwise(
            "knight_0",
            PieceDef::knight(),
            Color::srgb(0.15, 0.15, 0.2),
            "knight_1",
            PieceDef::knight(),
            Color::srgb(0.85, 0.12, 0.12),
        )
    }

    pub fn knight_3_clique() -> Self {
        clique("knight", PieceDef::knight(), 3)
    }

    pub fn knight_4_clique() -> Self {
        clique("knight", PieceDef::knight(), 4)
    }

    pub fn knight_5_clique() -> Self {
        clique("knight", PieceDef::knight(), 5)
    }

    pub fn knight_6_clique() -> Self {
        clique("knight", PieceDef::knight(), 6)
    }

    pub fn wazir_ferz_2_pairwise() -> Self {
        pairwise(
            "wazir_0",
            PieceDef::wazir(),
            Color::srgb(0.25, 0.25, 0.35),
            "ferz_1",
            PieceDef::ferz(),
            Color::srgb(0.9, 0.5, 0.15),
        )
    }

    pub fn king_knight_2_pairwise() -> Self {
        pairwise(
            "king_0",
            PieceDef::king(),
            Color::srgb(0.85, 0.8, 0.2),
            "knight_1",
            PieceDef::knight(),
            Color::srgb(0.2, 0.2, 0.25),
        )
    }

    pub fn wazir_ferz_knight_3_clique() -> Self {
        Self {
            armies: vec![
                army(
                    "wazir_0",
                    Color::srgb(0.35, 0.4, 0.55),
                    PieceDef::wazir(),
                    vec![1, 2],
                ),
                army(
                    "ferz_1",
                    Color::srgb(0.75, 0.35, 0.85),
                    PieceDef::ferz(),
                    vec![0, 2],
                ),
                army(
                    "knight_2",
                    Color::srgb(0.15, 0.55, 0.35),
                    PieceDef::knight(),
                    vec![0, 1],
                ),
            ],
            turn_order: vec![0, 1, 2],
        }
    }

    pub fn leaper_4_mixed_clique() -> Self {
        Self {
            armies: vec![
                army(
                    "knight_0",
                    Color::srgb(0.2, 0.25, 0.3),
                    PieceDef::knight(),
                    vec![1, 2, 3],
                ),
                army(
                    "camel_1",
                    Color::srgb(0.75, 0.45, 0.2),
                    PieceDef::camel(),
                    vec![0, 2, 3],
                ),
                army(
                    "zebra_2",
                    Color::srgb(0.25, 0.6, 0.75),
                    PieceDef::zebra(),
                    vec![0, 1, 3],
                ),
                army(
                    "giraffe_3",
                    Color::srgb(0.55, 0.75, 0.25),
                    PieceDef::giraffe(),
                    vec![0, 1, 2],
                ),
            ],
            turn_order: vec![0, 1, 2, 3],
        }
    }

    pub fn hippogriff_2_pairwise() -> Self {
        pairwise(
            "hippogriff_0",
            PieceDef::hippogriff(),
            Color::srgb(0.5, 0.15, 0.65),
            "hippogriff_1",
            PieceDef::hippogriff(),
            Color::srgb(0.15, 0.55, 0.5),
        )
    }

    pub fn hippogriff_3_clique() -> Self {
        clique("hippogriff", PieceDef::hippogriff(), 3)
    }

    pub fn trebuchet_dabbaba_2_pairwise() -> Self {
        pairwise(
            "trebuchet_0",
            PieceDef::trebuchet(),
            Color::srgb(0.6, 0.25, 0.2),
            "dabbaba_1",
            PieceDef::dabbaba(),
            Color::srgb(0.2, 0.45, 0.65),
        )
    }

    pub fn orthogonal_3_clique() -> Self {
        Self {
            armies: vec![
                army(
                    "wazir_0",
                    Color::srgb(0.4, 0.4, 0.5),
                    PieceDef::wazir(),
                    vec![1, 2],
                ),
                army(
                    "dabbaba_1",
                    Color::srgb(0.85, 0.35, 0.3),
                    PieceDef::dabbaba(),
                    vec![0, 2],
                ),
                army(
                    "trebuchet_2",
                    Color::srgb(0.3, 0.7, 0.45),
                    PieceDef::trebuchet(),
                    vec![0, 1],
                ),
            ],
            turn_order: vec![0, 1, 2],
        }
    }

    pub fn ferz_alfil_2_pairwise() -> Self {
        pairwise(
            "ferz_0",
            PieceDef::ferz(),
            Color::srgb(0.7, 0.2, 0.55),
            "alfil_1",
            PieceDef::alfil(),
            Color::srgb(0.2, 0.65, 0.75),
        )
    }

    pub fn guard_4_clique() -> Self {
        clique("guard", PieceDef::guard(), 4)
    }

    pub fn guard_6_clique() -> Self {
        clique("guard", PieceDef::guard(), 6)
    }

    pub fn king_3_clique() -> Self {
        clique("king", PieceDef::king(), 3)
    }

    pub fn camel_2_pairwise() -> Self {
        pairwise(
            "camel_0",
            PieceDef::camel(),
            Color::srgb(0.75, 0.45, 0.2),
            "camel_1",
            PieceDef::camel(),
            Color::srgb(0.25, 0.6, 0.75),
        )
    }

    pub fn camel_3_clique() -> Self {
        clique("camel", PieceDef::camel(), 3)
    }

    pub fn zebra_2_pairwise() -> Self {
        pairwise(
            "zebra_0",
            PieceDef::zebra(),
            Color::srgb(0.25, 0.6, 0.75),
            "zebra_1",
            PieceDef::zebra(),
            Color::srgb(0.85, 0.35, 0.3),
        )
    }

    pub fn zebra_4_clique() -> Self {
        clique("zebra", PieceDef::zebra(), 4)
    }

    pub fn dabbaba_2_pairwise() -> Self {
        pairwise(
            "dabbaba_0",
            PieceDef::dabbaba(),
            Color::srgb(0.85, 0.35, 0.3),
            "dabbaba_1",
            PieceDef::dabbaba(),
            Color::srgb(0.3, 0.7, 0.45),
        )
    }

    pub fn dabbaba_3_clique() -> Self {
        clique("dabbaba", PieceDef::dabbaba(), 3)
    }

    pub fn alfil_3_clique() -> Self {
        clique("alfil", PieceDef::alfil(), 3)
    }

    pub fn knight_camel_2_pairwise() -> Self {
        pairwise(
            "knight_0",
            PieceDef::knight(),
            Color::srgb(0.2, 0.25, 0.3),
            "camel_1",
            PieceDef::camel(),
            Color::srgb(0.75, 0.45, 0.2),
        )
    }

    pub fn king_knight_camel_3_weighted_turns() -> Self {
        Self {
            armies: vec![
                army(
                    "king_0",
                    Color::srgb(0.75, 0.7, 0.25),
                    PieceDef::king(),
                    vec![1, 2],
                ),
                army(
                    "knight_1",
                    Color::srgb(0.2, 0.3, 0.85),
                    PieceDef::knight(),
                    vec![0],
                ),
                army(
                    "camel_2",
                    Color::srgb(0.85, 0.3, 0.35),
                    PieceDef::camel(),
                    vec![0],
                ),
            ],
            turn_order: vec![0, 1, 2, 1, 2],
        }
    }

    pub fn chimera_3_clique() -> Self {
        let chimera = PieceDef::merge(&[PieceDef::knight(), PieceDef::wazir(), PieceDef::alfil()]);
        clique("chimera", chimera, 3)
    }

    pub fn chimera_4_clique() -> Self {
        let chimera = PieceDef::merge(&[PieceDef::knight(), PieceDef::wazir(), PieceDef::alfil()]);
        clique("chimera", chimera, 4)
    }

    pub fn giraffe_2_pairwise() -> Self {
        pairwise(
            "giraffe_0",
            PieceDef::giraffe(),
            Color::srgb(0.55, 0.75, 0.25),
            "giraffe_1",
            PieceDef::giraffe(),
            Color::srgb(0.75, 0.35, 0.85),
        )
    }

    pub fn leaper_5_mixed_clique() -> Self {
        Self {
            armies: vec![
                army(
                    "knight_0",
                    Color::srgb(0.2, 0.25, 0.3),
                    PieceDef::knight(),
                    all_but(0, 5),
                ),
                army(
                    "camel_1",
                    Color::srgb(0.75, 0.45, 0.2),
                    PieceDef::camel(),
                    all_but(1, 5),
                ),
                army(
                    "zebra_2",
                    Color::srgb(0.25, 0.6, 0.75),
                    PieceDef::zebra(),
                    all_but(2, 5),
                ),
                army(
                    "giraffe_3",
                    Color::srgb(0.55, 0.75, 0.25),
                    PieceDef::giraffe(),
                    all_but(3, 5),
                ),
                army(
                    "hippogriff_4",
                    Color::srgb(0.5, 0.15, 0.65),
                    PieceDef::hippogriff(),
                    all_but(4, 5),
                ),
            ],
            turn_order: vec![0, 1, 2, 3, 4],
        }
    }

    pub fn army(&self, id: ArmyId) -> &Army {
        &self.armies[id]
    }

    /// Preset label and constructor for the UI.
    pub fn preset_catalog() -> &'static [(&'static str, fn() -> GameDefinition)] {
        &[
            ("knight_2_pairwise", GameDefinition::knight_2_pairwise),
            ("knight_3_clique", GameDefinition::knight_3_clique),
            ("knight_4_clique", GameDefinition::knight_4_clique),
            ("knight_5_clique", GameDefinition::knight_5_clique),
            ("knight_6_clique", GameDefinition::knight_6_clique),
            ("wazir_ferz_2_pairwise", GameDefinition::wazir_ferz_2_pairwise),
            ("king_knight_2_pairwise", GameDefinition::king_knight_2_pairwise),
            (
                "wazir_ferz_knight_3_clique",
                GameDefinition::wazir_ferz_knight_3_clique,
            ),
            (
                "leaper_4_mixed_clique",
                GameDefinition::leaper_4_mixed_clique,
            ),
            (
                "leaper_5_mixed_clique",
                GameDefinition::leaper_5_mixed_clique,
            ),
            ("hippogriff_2_pairwise", GameDefinition::hippogriff_2_pairwise),
            ("hippogriff_3_clique", GameDefinition::hippogriff_3_clique),
            (
                "trebuchet_dabbaba_2_pairwise",
                GameDefinition::trebuchet_dabbaba_2_pairwise,
            ),
            ("orthogonal_3_clique", GameDefinition::orthogonal_3_clique),
            ("ferz_alfil_2_pairwise", GameDefinition::ferz_alfil_2_pairwise),
            ("guard_4_clique", GameDefinition::guard_4_clique),
            ("guard_6_clique", GameDefinition::guard_6_clique),
            ("king_3_clique", GameDefinition::king_3_clique),
            ("camel_2_pairwise", GameDefinition::camel_2_pairwise),
            ("camel_3_clique", GameDefinition::camel_3_clique),
            ("zebra_2_pairwise", GameDefinition::zebra_2_pairwise),
            ("zebra_4_clique", GameDefinition::zebra_4_clique),
            ("dabbaba_2_pairwise", GameDefinition::dabbaba_2_pairwise),
            ("dabbaba_3_clique", GameDefinition::dabbaba_3_clique),
            ("alfil_3_clique", GameDefinition::alfil_3_clique),
            (
                "knight_camel_2_pairwise",
                GameDefinition::knight_camel_2_pairwise,
            ),
            ("giraffe_2_pairwise", GameDefinition::giraffe_2_pairwise),
            (
                "king_knight_camel_3_weighted_turns",
                GameDefinition::king_knight_camel_3_weighted_turns,
            ),
            ("chimera_3_clique", GameDefinition::chimera_3_clique),
            ("chimera_4_clique", GameDefinition::chimera_4_clique),
        ]
    }
}

fn clique(label: &str, piece: PieceDef, n: usize) -> GameDefinition {
    GameDefinition {
        armies: (0..n)
            .map(|i| {
                army(
                    &format!("{label}_{i}"),
                    hue(i, n),
                    piece.clone(),
                    all_but(i, n),
                )
            })
            .collect(),
        turn_order: (0..n).collect(),
    }
}

fn pairwise(
    label_a: &str,
    piece_a: PieceDef,
    color_a: Color,
    label_b: &str,
    piece_b: PieceDef,
    color_b: Color,
) -> GameDefinition {
    GameDefinition {
        armies: vec![
            army(label_a, color_a, piece_a, vec![1]),
            army(label_b, color_b, piece_b, vec![0]),
        ],
        turn_order: vec![0, 1],
    }
}

fn army(name: &str, color: Color, piece: PieceDef, blocked_by: Vec<ArmyId>) -> Army {
    Army {
        name: name.into(),
        color,
        piece,
        blocked_by,
    }
}

fn all_but(i: ArmyId, n: usize) -> Vec<ArmyId> {
    (0..n).filter(|&j| j != i).collect()
}

fn hue(i: usize, n: usize) -> Color {
    let t = i as f32 / n.max(1) as f32;
    Color::hsl(t * 360.0, 0.65, 0.5)
}
