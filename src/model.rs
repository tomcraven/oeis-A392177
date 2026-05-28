use bevy::prelude::{Color, Resource};

pub type PieceId = usize;

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

    /// Knight plus camel (standard fairy-chess **gnu**).
    pub fn gnu() -> Self {
        Self::merge(&[Self::knight(), Self::camel()])
    }

    /// Knight plus zebra (distinct compound leaper).
    pub fn hippogriff() -> Self {
        Self::merge(&[Self::knight(), Self::zebra()])
    }

    /// Knight plus wazir plus alfil (used in chimera presets).
    pub fn chimera() -> Self {
        Self::merge(&[Self::knight(), Self::wazir(), Self::alfil()])
    }

    /// (3, 4) leaper.
    pub fn antelope() -> Self {
        Self {
            valid_moves: vec![
                (3, 4),
                (4, 3),
                (4, -3),
                (3, -4),
                (-3, -4),
                (-4, -3),
                (-4, 3),
                (-3, 4),
            ],
        }
    }

    /// (3, 3) diagonal leaper.
    pub fn tripper() -> Self {
        Self {
            valid_moves: vec![(3, 3), (3, -3), (-3, 3), (-3, -3)],
        }
    }

    /// (4, 0) orthogonal leaper.
    pub fn fourleaper() -> Self {
        Self {
            valid_moves: vec![(4, 0), (-4, 0), (0, 4), (0, -4)],
        }
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

    /// Named constructors for the piece picker (attack pattern only).
    pub fn piece_catalog() -> &'static [(&'static str, fn() -> PieceDef)] {
        &[
            ("knight", PieceDef::knight),
            ("wazir", PieceDef::wazir),
            ("dabbaba", PieceDef::dabbaba),
            ("ferz", PieceDef::ferz),
            ("alfil", PieceDef::alfil),
            ("king", PieceDef::king),
            ("camel", PieceDef::camel),
            ("zebra", PieceDef::zebra),
            ("antelope", PieceDef::antelope),
            ("gnu", PieceDef::gnu),
            ("hippogriff", PieceDef::hippogriff),
            ("chimera", PieceDef::chimera),
            ("giraffe", PieceDef::giraffe),
            ("trebuchet", PieceDef::trebuchet),
            ("tripper", PieceDef::tripper),
            ("fourleaper", PieceDef::fourleaper),
        ]
    }
}

#[derive(Clone, Debug)]
pub struct Piece {
    pub name: String,
    pub color: Color,
    pub piece: PieceDef,
    /// Armies whose pieces block placement on squares they attack.
    pub blocked_by: Vec<PieceId>,
    /// When false, this piece does not take placement turns (definition kept in roster).
    pub enabled: bool,
}

#[derive(Clone, Debug, Resource)]
pub struct GameDefinition {
    pub pieces: Vec<Piece>,
    /// Round-robin turn order by piece index.
    pub turn_order: Vec<PieceId>,
}

/// Enabled entries from [`GameDefinition::turn_order`] without building a `Vec`.
pub struct ActiveTurnOrder<'a> {
    def: &'a GameDefinition,
    /// Every `turn_order` id is an enabled piece — index directly into `turn_order`.
    dense: bool,
}

impl<'a> ActiveTurnOrder<'a> {
    fn new(def: &'a GameDefinition) -> Self {
        let dense = !def.turn_order.is_empty()
            && def
                .turn_order
                .iter()
                .all(|&id| def.pieces.get(id).is_some_and(|a| a.enabled));
        Self { def, dense }
    }

    pub fn is_empty(&self) -> bool {
        if self.def.turn_order.is_empty() {
            return true;
        }
        if self.dense {
            return false;
        }
        self.iter().next().is_none()
    }

    pub fn len(&self) -> usize {
        if self.dense {
            self.def.turn_order.len()
        } else {
            self.iter().count()
        }
    }

    pub fn get(&self, index: usize) -> Option<PieceId> {
        if self.dense {
            self.def.turn_order.get(index).copied()
        } else {
            self.iter().nth(index)
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = PieceId> + '_ {
        self.def
            .turn_order
            .iter()
            .copied()
            .filter(|&id| self.def.pieces.get(id).is_some_and(|a| a.enabled))
    }
}

impl Default for GameDefinition {
    fn default() -> Self {
        Self::knight_2_pairwise()
    }
}

impl GameDefinition {
    /// Whether two definitions would produce the same placement simulation.
    pub fn same_sim_state(&self, other: &Self) -> bool {
        self.turn_order == other.turn_order
            && self.pieces.len() == other.pieces.len()
            && self.pieces.iter().zip(&other.pieces).all(|(a, b)| {
                a.piece == b.piece && a.blocked_by == b.blocked_by && a.enabled == b.enabled
            })
    }

    /// Enabled pieces in turn order (lazy filter; no allocation on the hot path).
    pub fn active_turn_order(&self) -> ActiveTurnOrder<'_> {
        ActiveTurnOrder::new(self)
    }

    /// Whether sim state and per-piece colours match (ignores piece names).
    pub fn same_applied_state(&self, other: &Self) -> bool {
        self.same_sim_state(other)
            && self
                .pieces
                .iter()
                .zip(&other.pieces)
                .all(|(a, b)| a.color == b.color)
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
            pieces: vec![
                piece(
                    "wazir_0",
                    Color::srgb(0.35, 0.4, 0.55),
                    PieceDef::wazir(),
                    vec![1, 2],
                ),
                piece(
                    "ferz_1",
                    Color::srgb(0.75, 0.35, 0.85),
                    PieceDef::ferz(),
                    vec![0, 2],
                ),
                piece(
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
            pieces: vec![
                piece(
                    "knight_0",
                    Color::srgb(0.2, 0.25, 0.3),
                    PieceDef::knight(),
                    vec![1, 2, 3],
                ),
                piece(
                    "camel_1",
                    Color::srgb(0.75, 0.45, 0.2),
                    PieceDef::camel(),
                    vec![0, 2, 3],
                ),
                piece(
                    "zebra_2",
                    Color::srgb(0.25, 0.6, 0.75),
                    PieceDef::zebra(),
                    vec![0, 1, 3],
                ),
                piece(
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
            pieces: vec![
                piece(
                    "wazir_0",
                    Color::srgb(0.4, 0.4, 0.5),
                    PieceDef::wazir(),
                    vec![1, 2],
                ),
                piece(
                    "dabbaba_1",
                    Color::srgb(0.85, 0.35, 0.3),
                    PieceDef::dabbaba(),
                    vec![0, 2],
                ),
                piece(
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

    pub fn king_3_clique() -> Self {
        clique("king", PieceDef::king(), 3)
    }

    pub fn king_4_clique() -> Self {
        clique("king", PieceDef::king(), 4)
    }

    pub fn king_6_clique() -> Self {
        clique("king", PieceDef::king(), 6)
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
            pieces: vec![
                piece(
                    "king_0",
                    Color::srgb(0.75, 0.7, 0.25),
                    PieceDef::king(),
                    vec![1, 2],
                ),
                piece(
                    "knight_1",
                    Color::srgb(0.2, 0.3, 0.85),
                    PieceDef::knight(),
                    vec![0],
                ),
                piece(
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
        clique("chimera", PieceDef::chimera(), 3)
    }

    pub fn chimera_4_clique() -> Self {
        clique("chimera", PieceDef::chimera(), 4)
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
            pieces: vec![
                piece(
                    "knight_0",
                    Color::srgb(0.2, 0.25, 0.3),
                    PieceDef::knight(),
                    all_but(0, 5),
                ),
                piece(
                    "camel_1",
                    Color::srgb(0.75, 0.45, 0.2),
                    PieceDef::camel(),
                    all_but(1, 5),
                ),
                piece(
                    "zebra_2",
                    Color::srgb(0.25, 0.6, 0.75),
                    PieceDef::zebra(),
                    all_but(2, 5),
                ),
                piece(
                    "giraffe_3",
                    Color::srgb(0.55, 0.75, 0.25),
                    PieceDef::giraffe(),
                    all_but(3, 5),
                ),
                piece(
                    "hippogriff_4",
                    Color::srgb(0.5, 0.15, 0.65),
                    PieceDef::hippogriff(),
                    all_but(4, 5),
                ),
            ],
            turn_order: vec![0, 1, 2, 3, 4],
        }
    }

    pub fn piece(&self, id: PieceId) -> &Piece {
        &self.pieces[id]
    }

    /// Append a new piece using a catalog piece; default name `{label}_{id}`.
    /// `blocked_by` is wired to every other piece (same as clique presets).
    pub fn push_piece_from_piece_preset(
        &mut self,
        preset_label: &str,
        piece_def: PieceDef,
        color: Color,
    ) {
        let id = self.pieces.len();
        let n = id + 1;
        for piece in &mut self.pieces {
            if !piece.blocked_by.contains(&id) {
                piece.blocked_by.push(id);
            }
        }
        self.pieces.push(piece(
            &format!("{preset_label}_{id}"),
            color,
            piece_def,
            all_but(id, n),
        ));
        self.turn_order.push(id);
    }

    /// Distinct piece colour for roster slot `index` (same palette as clique presets).
    pub fn default_piece_color(index: usize) -> Color {
        hue(index, index + 1)
    }

    /// Preset label and constructor for the UI.
    pub fn preset_catalog() -> &'static [(&'static str, fn() -> GameDefinition)] {
        &[
            ("knight_2_pairwise", GameDefinition::knight_2_pairwise),
            ("knight_3_clique", GameDefinition::knight_3_clique),
            ("knight_4_clique", GameDefinition::knight_4_clique),
            ("knight_5_clique", GameDefinition::knight_5_clique),
            ("knight_6_clique", GameDefinition::knight_6_clique),
            (
                "wazir_ferz_2_pairwise",
                GameDefinition::wazir_ferz_2_pairwise,
            ),
            (
                "king_knight_2_pairwise",
                GameDefinition::king_knight_2_pairwise,
            ),
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
            (
                "hippogriff_2_pairwise",
                GameDefinition::hippogriff_2_pairwise,
            ),
            ("hippogriff_3_clique", GameDefinition::hippogriff_3_clique),
            (
                "trebuchet_dabbaba_2_pairwise",
                GameDefinition::trebuchet_dabbaba_2_pairwise,
            ),
            ("orthogonal_3_clique", GameDefinition::orthogonal_3_clique),
            (
                "ferz_alfil_2_pairwise",
                GameDefinition::ferz_alfil_2_pairwise,
            ),
            ("king_3_clique", GameDefinition::king_3_clique),
            ("king_4_clique", GameDefinition::king_4_clique),
            ("king_6_clique", GameDefinition::king_6_clique),
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

fn clique(label: &str, piece_def: PieceDef, n: usize) -> GameDefinition {
    GameDefinition {
        pieces: (0..n)
            .map(|i| {
                piece(
                    &format!("{label}_{i}"),
                    hue(i, n),
                    piece_def.clone(),
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
        pieces: vec![
            piece(label_a, color_a, piece_a, vec![1]),
            piece(label_b, color_b, piece_b, vec![0]),
        ],
        turn_order: vec![0, 1],
    }
}

fn piece(name: &str, color: Color, piece_def: PieceDef, blocked_by: Vec<PieceId>) -> Piece {
    Piece {
        name: name.into(),
        color,
        piece: piece_def,
        blocked_by,
        enabled: true,
    }
}

fn all_but(i: PieceId, n: usize) -> Vec<PieceId> {
    (0..n).filter(|&j| j != i).collect()
}

fn hue(i: usize, n: usize) -> Color {
    let t = i as f32 / n.max(1) as f32;
    Color::hsl(t * 360.0, 0.65, 0.5)
}
