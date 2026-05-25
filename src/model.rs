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
        Self::red_black_knights()
    }
}

impl GameDefinition {
    pub fn red_black_knights() -> Self {
        let knight = PieceDef::knight();
        Self {
            armies: vec![
                Army {
                    name: "Black".into(),
                    color: Color::srgb(0.15, 0.15, 0.2),
                    piece: knight.clone(),
                    blocked_by: vec![1],
                },
                Army {
                    name: "Red".into(),
                    color: Color::srgb(0.85, 0.12, 0.12),
                    piece: knight,
                    blocked_by: vec![0],
                },
            ],
            turn_order: vec![0, 1],
        }
    }

    pub fn three_knights() -> Self {
        let k = PieceDef::knight();
        Self {
            armies: vec![
                army("Violet", Color::srgb(0.55, 0.2, 0.85), k.clone(), vec![1, 2]),
                army("Amber", Color::srgb(0.95, 0.65, 0.1), k.clone(), vec![0, 2]),
                army("Teal", Color::srgb(0.1, 0.75, 0.7), k, vec![0, 1]),
            ],
            turn_order: vec![0, 1, 2],
        }
    }

    pub fn four_knights() -> Self {
        let k = PieceDef::knight();
        Self {
            armies: vec![
                army("North", Color::srgb(0.2, 0.35, 0.9), k.clone(), all_but(0, 4)),
                army("East", Color::srgb(0.9, 0.25, 0.2), k.clone(), all_but(1, 4)),
                army("South", Color::srgb(0.2, 0.75, 0.35), k.clone(), all_but(2, 4)),
                army("West", Color::srgb(0.85, 0.75, 0.15), k, all_but(3, 4)),
            ],
            turn_order: vec![0, 1, 2, 3],
        }
    }

    pub fn five_knights_ring() -> Self {
        let k = PieceDef::knight();
        let n = 5;
        Self {
            armies: (0..n)
                .map(|i| {
                    army(
                        &format!("Knight {i}"),
                        hue(i, n),
                        k.clone(),
                        (0..n).filter(|&j| j != i).collect(),
                    )
                })
                .collect(),
            turn_order: (0..n).collect(),
        }
    }

    pub fn rook_vs_bishop() -> Self {
        Self {
            armies: vec![
                army("Rook", Color::srgb(0.25, 0.25, 0.35), PieceDef::wazir(), vec![1]),
                army("Bishop", Color::srgb(0.9, 0.5, 0.15), PieceDef::ferz(), vec![0]),
            ],
            turn_order: vec![0, 1],
        }
    }

    pub fn king_vs_knight() -> Self {
        Self {
            armies: vec![
                army("King", Color::srgb(0.85, 0.8, 0.2), PieceDef::king(), vec![1]),
                army("Knight", Color::srgb(0.2, 0.2, 0.25), PieceDef::knight(), vec![0]),
            ],
            turn_order: vec![0, 1],
        }
    }

    pub fn rook_bishop_knight() -> Self {
        Self {
            armies: vec![
                army("Rook", Color::srgb(0.35, 0.4, 0.55), PieceDef::wazir(), vec![1, 2]),
                army("Bishop", Color::srgb(0.75, 0.35, 0.85), PieceDef::ferz(), vec![0, 2]),
                army("Knight", Color::srgb(0.15, 0.55, 0.35), PieceDef::knight(), vec![0, 1]),
            ],
            turn_order: vec![0, 1, 2],
        }
    }

    pub fn four_classic_leapers() -> Self {
        Self {
            armies: vec![
                army("Knight", Color::srgb(0.2, 0.25, 0.3), PieceDef::knight(), vec![1, 2, 3]),
                army("Camel", Color::srgb(0.75, 0.45, 0.2), PieceDef::camel(), vec![0, 2, 3]),
                army("Zebra", Color::srgb(0.25, 0.6, 0.75), PieceDef::zebra(), vec![0, 1, 3]),
                army(
                    "Giraffe",
                    Color::srgb(0.55, 0.75, 0.25),
                    PieceDef::giraffe(),
                    vec![0, 1, 2],
                ),
            ],
            turn_order: vec![0, 1, 2, 3],
        }
    }

    pub fn hippogriff_duel() -> Self {
        let h = PieceDef::hippogriff();
        Self {
            armies: vec![
                army("Hippogriff A", Color::srgb(0.5, 0.15, 0.65), h.clone(), vec![1]),
                army("Hippogriff B", Color::srgb(0.15, 0.55, 0.5), h, vec![0]),
            ],
            turn_order: vec![0, 1],
        }
    }

    pub fn trebuchet_vs_dabbaba() -> Self {
        Self {
            armies: vec![
                army(
                    "Trebuchet",
                    Color::srgb(0.6, 0.25, 0.2),
                    PieceDef::trebuchet(),
                    vec![1],
                ),
                army(
                    "Dabbaba",
                    Color::srgb(0.2, 0.45, 0.65),
                    PieceDef::dabbaba(),
                    vec![0],
                ),
            ],
            turn_order: vec![0, 1],
        }
    }

    pub fn orthogonal_pack() -> Self {
        Self {
            armies: vec![
                army("Wazir", Color::srgb(0.4, 0.4, 0.5), PieceDef::wazir(), vec![1, 2]),
                army(
                    "Dabbaba",
                    Color::srgb(0.85, 0.35, 0.3),
                    PieceDef::dabbaba(),
                    vec![0, 2],
                ),
                army(
                    "Trebuchet",
                    Color::srgb(0.3, 0.7, 0.45),
                    PieceDef::trebuchet(),
                    vec![0, 1],
                ),
            ],
            turn_order: vec![0, 1, 2],
        }
    }

    pub fn diagonal_pack() -> Self {
        Self {
            armies: vec![
                army("Ferz", Color::srgb(0.7, 0.2, 0.55), PieceDef::ferz(), vec![1]),
                army("Alfil", Color::srgb(0.2, 0.65, 0.75), PieceDef::alfil(), vec![0]),
            ],
            turn_order: vec![0, 1],
        }
    }

    pub fn six_guards() -> Self {
        let g = PieceDef::guard();
        let n = 6;
        Self {
            armies: (0..n)
                .map(|i| army(&format!("Guard {i}"), hue(i, n), g.clone(), all_but(i, n)))
                .collect(),
            turn_order: (0..n).collect(),
        }
    }

    pub fn asymmetric_melee() -> Self {
        Self {
            armies: vec![
                army(
                    "Heavy (King)",
                    Color::srgb(0.75, 0.7, 0.25),
                    PieceDef::king(),
                    vec![1, 2],
                ),
                army(
                    "Swift (Knight)",
                    Color::srgb(0.2, 0.3, 0.85),
                    PieceDef::knight(),
                    vec![0],
                ),
                army(
                    "Sniper (Camel)",
                    Color::srgb(0.85, 0.3, 0.35),
                    PieceDef::camel(),
                    vec![0],
                ),
            ],
            turn_order: vec![0, 1, 2, 1, 2],
        }
    }

    pub fn zebra_chaos_four() -> Self {
        let z = PieceDef::zebra();
        Self {
            armies: (0..4)
                .map(|i| {
                    army(
                        &format!("Zebra {i}"),
                        hue(i, 4),
                        z.clone(),
                        all_but(i, 4),
                    )
                })
                .collect(),
            turn_order: vec![0, 1, 2, 3],
        }
    }

    pub fn fusion_piece_freeforall() -> Self {
        let chimera = PieceDef::merge(&[
            PieceDef::knight(),
            PieceDef::wazir(),
            PieceDef::alfil(),
        ]);
        let n = 3;
        Self {
            armies: (0..n)
                .map(|i| {
                    army(
                        &format!("Chimera {i}"),
                        hue(i, n),
                        chimera.clone(),
                        all_but(i, n),
                    )
                })
                .collect(),
            turn_order: vec![0, 1, 2],
        }
    }

    pub fn army(&self, id: ArmyId) -> &Army {
        &self.armies[id]
    }

    /// Preset label and constructor for the UI.
    pub fn preset_catalog() -> &'static [(&'static str, fn() -> GameDefinition)] {
        &[
            ("Red & Black Knights", GameDefinition::red_black_knights),
            ("3 Knights (mutual threat)", GameDefinition::three_knights),
            ("4 Knights (cardinal)", GameDefinition::four_knights),
            ("5 Knights (all block all)", GameDefinition::five_knights_ring),
            ("Rook vs Bishop (1-step)", GameDefinition::rook_vs_bishop),
            ("King vs Knight", GameDefinition::king_vs_knight),
            ("Rook + Bishop + Knight", GameDefinition::rook_bishop_knight),
            ("Knight / Camel / Zebra / Giraffe", GameDefinition::four_classic_leapers),
            ("Hippogriff duel (knight+camel)", GameDefinition::hippogriff_duel),
            ("Trebuchet vs Dabbaba", GameDefinition::trebuchet_vs_dabbaba),
            ("Orthogonal pack (3)", GameDefinition::orthogonal_pack),
            ("Diagonal pack (ferz + alfil)", GameDefinition::diagonal_pack),
            ("6 Guards (king moves)", GameDefinition::six_guards),
            ("Asymmetric melee (3)", GameDefinition::asymmetric_melee),
            ("4 Zebras (chaos)", GameDefinition::zebra_chaos_four),
            ("3 Chimeras (fusion leaper)", GameDefinition::fusion_piece_freeforall),
        ]
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
