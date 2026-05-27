use bevy::prelude::Color;
use serde::{Deserialize, Serialize};

use crate::model::{Army, ArmyId, GameDefinition, PieceDef};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct SavedColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SavedArmy {
    pub name: String,
    pub color: SavedColor,
    pub valid_moves: Vec<[i32; 2]>,
    pub blocked_by: Vec<ArmyId>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SavedGameDefinition {
    pub armies: Vec<SavedArmy>,
    pub turn_order: Vec<ArmyId>,
}

/// Authoritative discover preset: exact armies + sim depth (written to `config.toml`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DiscoverRunConfig {
    pub game: SavedGameDefinition,
    #[serde(default)]
    pub target_index: u32,
    #[serde(default)]
    pub turns: usize,
}

impl SavedGameDefinition {
    pub fn from_game(def: &GameDefinition) -> Self {
        Self {
            armies: def
                .armies
                .iter()
                .map(|a| SavedArmy {
                    name: a.name.clone(),
                    color: SavedColor::from_bevy(a.color),
                    valid_moves: a
                        .piece
                        .valid_moves
                        .iter()
                        .map(|&(x, y)| [x, y])
                        .collect(),
                    blocked_by: a.blocked_by.clone(),
                    enabled: a.enabled,
                })
                .collect(),
            turn_order: def.turn_order.clone(),
        }
    }
}

impl SavedColor {
    pub fn from_bevy(color: Color) -> Self {
        let c = color.to_srgba();
        Self {
            r: c.red,
            g: c.green,
            b: c.blue,
            a: c.alpha,
        }
    }

    pub fn to_bevy(self) -> Color {
        Color::srgba(self.r, self.g, self.b, self.a)
    }
}

impl From<SavedGameDefinition> for GameDefinition {
    fn from(saved: SavedGameDefinition) -> Self {
        GameDefinition {
            armies: saved
                .armies
                .into_iter()
                .map(|a| Army {
                    name: a.name,
                    color: a.color.to_bevy(),
                    piece: PieceDef {
                        valid_moves: a
                            .valid_moves
                            .into_iter()
                            .map(|[x, y]| (x, y))
                            .collect(),
                    },
                    blocked_by: a.blocked_by,
                    enabled: a.enabled,
                })
                .collect(),
            turn_order: saved.turn_order,
        }
    }
}

impl DiscoverRunConfig {
    pub fn from_game(def: &GameDefinition, target_index: u32, turns: usize) -> Self {
        Self {
            game: SavedGameDefinition::from_game(def),
            target_index,
            turns,
        }
    }

    pub fn to_game_definition(&self) -> GameDefinition {
        self.game.clone().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GameDefinition;

    #[test]
    fn knight_pairwise_round_trips_through_toml() {
        let def = GameDefinition::knight_2_pairwise();
        let cfg = DiscoverRunConfig::from_game(&def, 100, 0);
        let text = toml::to_string_pretty(&cfg).unwrap();
        let loaded: DiscoverRunConfig = toml::from_str(&text).unwrap();
        assert_eq!(loaded.game, cfg.game);
        let restored: GameDefinition = loaded.to_game_definition();
        assert!(def.same_applied_state(&restored));
    }
}
