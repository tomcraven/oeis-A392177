use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::camera_config::CameraSessionConfig;
use crate::game_snapshot::SavedGameDefinition;
use crate::model::GameDefinition;

#[cfg(not(target_family = "wasm"))]
use crate::calibration_config;

#[cfg(not(target_family = "wasm"))]
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bookmark {
    pub name: String,
    pub game: SavedGameDefinition,
    pub camera: CameraSessionConfig,
    pub target_index: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct BookmarkCollection {
    pub bookmarks: Vec<Bookmark>,
}

#[derive(Resource, Default)]
pub struct BookmarkStore {
    pub bookmarks: Vec<Bookmark>,
    pub selected: Option<usize>,
}

impl Bookmark {
    pub fn capture(
        name: String,
        game: &GameDefinition,
        camera: CameraSessionConfig,
        target_index: u32,
    ) -> Self {
        Self {
            name,
            game: SavedGameDefinition::from_game(game),
            camera,
            target_index,
        }
    }

    pub fn to_game_definition(&self) -> GameDefinition {
        self.game.clone().into()
    }
}

impl BookmarkStore {
    pub fn reload(&mut self) {
        if let Some(collection) = load_collection() {
            self.bookmarks = collection.bookmarks;
            self.selected = self.selected.filter(|&i| i < self.bookmarks.len());
        }
    }

    pub fn persist(&self) -> std::io::Result<()> {
        save_collection(&BookmarkCollection {
            bookmarks: self.bookmarks.clone(),
        })
    }

    pub fn add(&mut self, bookmark: Bookmark) -> usize {
        self.bookmarks.push(bookmark);
        let idx = self.bookmarks.len() - 1;
        self.selected = Some(idx);
        let _ = self.persist();
        idx
    }

    pub fn remove_selected(&mut self) -> bool {
        let Some(idx) = self.selected else {
            return false;
        };
        if idx >= self.bookmarks.len() {
            self.selected = None;
            return false;
        }
        self.bookmarks.remove(idx);
        self.selected = if self.bookmarks.is_empty() {
            None
        } else {
            Some(idx.min(self.bookmarks.len() - 1))
        };
        let _ = self.persist();
        true
    }
}

#[cfg(not(target_family = "wasm"))]
pub fn config_file_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("red_black_knights").join("bookmarks.toml")
}

#[cfg(not(target_family = "wasm"))]
pub fn load_collection() -> Option<BookmarkCollection> {
    if calibration_config::smoke_test_mode() {
        return None;
    }
    let path = config_file_path();
    let text = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&text).ok()
}

#[cfg(not(target_family = "wasm"))]
pub fn save_collection(collection: &BookmarkCollection) -> std::io::Result<()> {
    if calibration_config::smoke_test_mode() {
        return Ok(());
    }
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(collection)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)
}

#[cfg(target_family = "wasm")]
const STORAGE_KEY: &str = "red_black_knights_bookmarks";

#[cfg(target_family = "wasm")]
pub fn load_collection() -> Option<BookmarkCollection> {
    let text = read_local_storage()?;
    toml::from_str(&text).ok()
}

#[cfg(target_family = "wasm")]
pub fn save_collection(collection: &BookmarkCollection) -> std::io::Result<()> {
    let text = toml::to_string_pretty(collection)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_local_storage(&text)
}

#[cfg(target_family = "wasm")]
fn read_local_storage() -> Option<String> {
    let window = web_sys::window()?;
    let storage = window.local_storage().ok()??;
    storage.get_item(STORAGE_KEY).ok()?
}

#[cfg(target_family = "wasm")]
fn write_local_storage(text: &str) -> std::io::Result<()> {
    let window = web_sys::window()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no window"))?;
    let storage = window
        .local_storage()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "no localStorage"))?
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "localStorage disabled")
        })?;
    storage
        .set_item(STORAGE_KEY, text)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "localStorage set failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmark_collection_round_trips_through_toml() {
        let def = GameDefinition::knight_2_pairwise();
        let collection = BookmarkCollection {
            bookmarks: vec![Bookmark::capture(
                "Test".into(),
                &def,
                CameraSessionConfig {
                    x: 1.0,
                    y: 2.0,
                    zoom: 3.0,
                },
                42,
            )],
        };
        let text = toml::to_string_pretty(&collection).unwrap();
        let loaded: BookmarkCollection = toml::from_str(&text).unwrap();
        assert_eq!(loaded, collection);
    }
}
