use bevy::prelude::Color;

use crate::model::{Army, ArmyId, GameDefinition, PieceDef};

const N_PIECES: usize = 16;
const N_PAIRWISE: usize = N_PIECES * N_PIECES;
const N_CLIQUE_SIZES: usize = 4; // armies 2..=5
const N_SAME_CLIQUES: usize = N_PIECES * N_CLIQUE_SIZES;
const N_MIXED: usize = 12;

const SIMPLE_PIECES: [(&str, fn() -> PieceDef); N_PIECES] = [
    ("wazir", PieceDef::wazir),
    ("ferz", PieceDef::ferz),
    ("dabbaba", PieceDef::dabbaba),
    ("alfil", PieceDef::alfil),
    ("knight", PieceDef::knight),
    ("king", PieceDef::king),
    ("camel", PieceDef::camel),
    ("zebra", PieceDef::zebra),
    ("giraffe", PieceDef::giraffe),
    ("trebuchet", PieceDef::trebuchet),
    ("antelope", PieceDef::antelope),
    ("tripper", PieceDef::tripper),
    ("fourleaper", PieceDef::fourleaper),
    ("gnu", PieceDef::gnu),
    ("hippogriff", PieceDef::hippogriff),
    ("chimera", PieceDef::chimera),
];

const MIXED: [(&str, &str, [&'static str; 3]); N_MIXED] = [
    ("mixed_wazir_ferz_knight", "wazir + ferz + knight", ["wazir", "ferz", "knight"]),
    (
        "mixed_wazir_ferz_dabbaba",
        "wazir + ferz + dabbaba",
        ["wazir", "ferz", "dabbaba"],
    ),
    (
        "mixed_knight_camel_zebra",
        "knight + camel + zebra",
        ["knight", "camel", "zebra"],
    ),
    (
        "mixed_king_knight_wazir",
        "king + knight + wazir",
        ["king", "knight", "wazir"],
    ),
    (
        "mixed_ferz_alfil_dabbaba",
        "ferz + alfil + dabbaba",
        ["ferz", "alfil", "dabbaba"],
    ),
    (
        "mixed_knight_king_camel",
        "knight + king + camel",
        ["knight", "king", "camel"],
    ),
    (
        "mixed_wazir_knight_dabbaba",
        "wazir + knight + dabbaba",
        ["wazir", "knight", "dabbaba"],
    ),
    (
        "mixed_ferz_knight_camel",
        "ferz + knight + camel",
        ["ferz", "knight", "camel"],
    ),
    (
        "mixed_dabbaba_alfil_knight",
        "dabbaba + alfil + knight",
        ["dabbaba", "alfil", "knight"],
    ),
    (
        "mixed_zebra_giraffe_knight",
        "zebra + giraffe + knight",
        ["zebra", "giraffe", "knight"],
    ),
    (
        "mixed_king_ferz_knight",
        "king + ferz + knight",
        ["king", "ferz", "knight"],
    ),
    (
        "mixed_trebuchet_wazir_ferz",
        "trebuchet + wazir + ferz",
        ["trebuchet", "wazir", "ferz"],
    ),
];

pub fn catalog_len() -> usize {
    N_PAIRWISE + N_SAME_CLIQUES + N_MIXED
}

pub fn recipe_meta(index: usize) -> Option<(String, String)> {
    if index >= catalog_len() {
        return None;
    }
    let id = match section(index) {
        Section::Pairwise { a, b } => format!(
            "pairwise_{}_vs_{}",
            SIMPLE_PIECES[a].0,
            SIMPLE_PIECES[b].0
        ),
        Section::SameClique { piece, size } => {
            format!("clique_{size}_{}", SIMPLE_PIECES[piece].0)
        }
        Section::Mixed { m } => MIXED[m].0.to_string(),
    };
    let label = recipe_label(index)?;
    Some((id, label))
}

pub fn recipe_id(index: usize) -> Option<&'static str> {
    if index >= catalog_len() {
        return None;
    }
    match section(index) {
        Section::Mixed { m } => Some(MIXED[m].0),
        _ => None,
    }
}

pub fn recipe_label(index: usize) -> Option<String> {
    if index >= catalog_len() {
        return None;
    }
    Some(match section(index) {
        Section::Pairwise { a, b } => format!(
            "{} vs {} (pairwise)",
            SIMPLE_PIECES[a].0, SIMPLE_PIECES[b].0
        ),
        Section::SameClique { piece, size } => {
            format!("{size}× {} clique", SIMPLE_PIECES[piece].0)
        }
        Section::Mixed { m } => format!("{} clique", MIXED[m].1),
    })
}

pub fn game_at(index: usize) -> Option<GameDefinition> {
    if index >= catalog_len() {
        return None;
    }
    Some(match section(index) {
        Section::Pairwise { a, b } => pairwise_pieces(
            SIMPLE_PIECES[a].0,
            (SIMPLE_PIECES[a].1)(),
            SIMPLE_PIECES[b].0,
            (SIMPLE_PIECES[b].1)(),
        ),
        Section::SameClique { piece, size } => {
            clique_pieces(SIMPLE_PIECES[piece].0, (SIMPLE_PIECES[piece].1)(), size)
        }
        Section::Mixed { m } => mixed_clique(MIXED[m].2),
    })
}

pub fn recipe_for_iteration(iteration: u64) -> usize {
    (iteration as usize) % catalog_len().max(1)
}

enum Section {
    Pairwise { a: usize, b: usize },
    SameClique { piece: usize, size: usize },
    Mixed { m: usize },
}

fn section(index: usize) -> Section {
    if index < N_PAIRWISE {
        return Section::Pairwise {
            a: index / N_PIECES,
            b: index % N_PIECES,
        };
    }
    let i = index - N_PAIRWISE;
    if i < N_SAME_CLIQUES {
        let piece = i / N_CLIQUE_SIZES;
        let size = i % N_CLIQUE_SIZES + 2;
        return Section::SameClique { piece, size };
    }
    Section::Mixed {
        m: i - N_SAME_CLIQUES,
    }
}

fn pairwise_pieces(
    label_a: &str,
    piece_a: PieceDef,
    label_b: &str,
    piece_b: PieceDef,
) -> GameDefinition {
    GameDefinition {
        armies: vec![
            army(
                &format!("{label_a}_0"),
                Color::srgb(0.18, 0.2, 0.28),
                piece_a,
                vec![1],
            ),
            army(
                &format!("{label_b}_1"),
                Color::srgb(0.88, 0.14, 0.12),
                piece_b,
                vec![0],
            ),
        ],
        turn_order: vec![0, 1],
    }
}

fn clique_pieces(label: &str, piece: PieceDef, n: usize) -> GameDefinition {
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

fn mixed_clique(names: [&'static str; 3]) -> GameDefinition {
    let pieces: [PieceDef; 3] = [
        piece_by_name(names[0]).1(),
        piece_by_name(names[1]).1(),
        piece_by_name(names[2]).1(),
    ];
    GameDefinition {
        armies: (0..3)
            .map(|i| {
                army(
                    &format!("{}_{i}", names[i]),
                    hue(i, 3),
                    pieces[i].clone(),
                    all_but(i, 3),
                )
            })
            .collect(),
        turn_order: vec![0, 1, 2],
    }
}

fn piece_by_name(name: &str) -> (&'static str, fn() -> PieceDef) {
    SIMPLE_PIECES
        .iter()
        .copied()
        .find(|(n, _)| *n == name)
        .unwrap_or(("knight", PieceDef::knight))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_includes_knight_pairwise() {
        let idx = (0..catalog_len())
            .find(|&i| {
                recipe_meta(i)
                    .map(|(id, _)| id == "pairwise_knight_vs_knight")
                    .unwrap_or(false)
            })
            .unwrap();
        let def = game_at(idx).unwrap();
        assert_eq!(def.armies.len(), 2);
        assert_eq!(def.armies[0].piece, PieceDef::knight());
    }

    #[test]
    fn catalog_len_matches_sections() {
        assert_eq!(catalog_len(), 256 + 64 + 12);
    }
}
