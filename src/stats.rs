//! Persisted game + settings state. Corrupt files reset; writes are atomic
//! (the pisweep `Store` pattern).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::piece::Color;
use crate::search::Level;

/// The game mode the user picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Human vs human.
    TwoPlayers,
    /// Human plays white, engine black.
    HumanWhite,
    /// Human plays black, engine white.
    HumanBlack,
}

impl Mode {
    pub fn computer_plays(self) -> Option<Color> {
        match self {
            Mode::TwoPlayers => None,
            Mode::HumanWhite => Some(Color::Black),
            Mode::HumanBlack => Some(Color::White),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Mode::TwoPlayers => "two-players",
            Mode::HumanWhite => "human-white",
            Mode::HumanBlack => "human-black",
        }
    }

    /// First run defaults to playing White against the computer.
    pub fn parse(id: &str) -> Mode {
        match id {
            "human-white" => Mode::HumanWhite,
            "human-black" => Mode::HumanBlack,
            "two-players" => Mode::TwoPlayers,
            _ => Mode::HumanWhite,
        }
    }
}

/// Saved mid-game state for resume.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SavedGame {
    /// Coordinate-notation move list (e2e4, e7e8q) from the start position.
    #[serde(default)]
    pub moves: Vec<String>,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub flipped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LevelRecord {
    #[serde(default)]
    pub wins: u32,
    #[serde(default)]
    pub losses: u32,
    #[serde(default)]
    pub draws: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Settings {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub flipped: bool,
    #[serde(default)]
    pub records: std::collections::BTreeMap<String, LevelRecord>,
}

impl Settings {
    pub fn record_for(&self, level: Level) -> LevelRecord {
        self.records.get(level.id()).cloned().unwrap_or_default()
    }

    pub fn record_mut(&mut self, level: Level) -> &mut LevelRecord {
        self.records.entry(level.id().to_string()).or_default()
    }
}

impl Level {
    pub fn id(self) -> &'static str {
        match self {
            Level::Easy => "easy",
            Level::Medium => "medium",
            Level::Hard => "hard",
        }
    }
}

pub struct Store {
    settings_path: PathBuf,
    game_path: PathBuf,
}

impl Store {
    pub fn new(settings_path: PathBuf, game_path: PathBuf) -> Self {
        Self {
            settings_path,
            game_path,
        }
    }

    pub fn load_settings(&self) -> Settings {
        fs::read_to_string(&self.settings_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &Settings) {
        atomic_write(
            &self.settings_path,
            &serde_json::to_string_pretty(settings).ok(),
        );
    }

    pub fn load_game(&self) -> Option<SavedGame> {
        fs::read_to_string(&self.game_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .filter(|game: &SavedGame| !game.moves.is_empty())
    }

    pub fn save_game(&self, game: &SavedGame) {
        atomic_write(&self.game_path, &serde_json::to_string_pretty(game).ok());
    }

    pub fn clear_game(&self) {
        let _ = fs::remove_file(&self.game_path);
    }

    /// Where PGN exports land: `<data>/exports/`.
    pub fn exports_dir(&self) -> PathBuf {
        self.game_path
            .parent()
            .map(|p| p.join("exports"))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

fn atomic_write(path: &Path, body: &Option<String>) {
    let Some(body) = body else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, body)
        .and_then(|_| fs::File::open(&tmp).and_then(|f| f.sync_all()))
        .is_ok()
    {
        let _ = fs::rename(&tmp, path);
    }
}

/// Data dir and file paths: `~/.local/share/pichess/{settings,game}.json`.
pub fn store_paths() -> (PathBuf, PathBuf, PathBuf) {
    let base = directories::ProjectDirs::from("dev", "pichess", "pichess")
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join("pichess-data"));
    let _ = fs::create_dir_all(&base);
    (base.join("settings.json"), base.join("game.json"), base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!("pichess-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let store = Store::new(dir.join("settings.json"), dir.join("game.json"));
        (store, dir)
    }

    #[test]
    fn mode_helpers() {
        assert_eq!(Mode::TwoPlayers.computer_plays(), None);
        assert_eq!(Mode::HumanWhite.computer_plays(), Some(Color::Black));
        assert_eq!(Mode::HumanBlack.computer_plays(), Some(Color::White));
        assert_eq!(Mode::parse("human-black"), Mode::HumanBlack);
        assert_eq!(Mode::parse("nope"), Mode::HumanWhite);
        assert_eq!(Mode::parse("two-players"), Mode::TwoPlayers);
        assert_eq!(Level::id(Level::Hard), "hard");
    }

    #[test]
    fn settings_roundtrip_and_corrupt_reset() {
        let (store, dir) = temp_store("settings");
        let mut settings = Settings {
            mode: Mode::HumanWhite.id().into(),
            level: Level::Hard.id().into(),
            ..Default::default()
        };
        settings.record_mut(Level::Hard).wins = 3;
        store.save_settings(&settings);
        assert_eq!(store.load_settings(), settings);

        fs::write(dir.join("settings.json"), "{{{nope").unwrap();
        assert_eq!(store.load_settings(), Settings::default());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn game_save_load_clear() {
        let (store, dir) = temp_store("game");
        assert_eq!(store.load_game(), None);
        let game = SavedGame {
            moves: vec!["e2e4".into(), "e7e5".into()],
            mode: Mode::TwoPlayers.id().into(),
            level: Level::Medium.id().into(),
            flipped: false,
        };
        store.save_game(&game);
        assert_eq!(store.load_game(), Some(game));
        store.clear_game();
        assert_eq!(store.load_game(), None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_game_is_not_resumable() {
        let (store, dir) = temp_store("empty");
        store.save_game(&SavedGame::default());
        assert_eq!(store.load_game(), None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn records_bump_per_level() {
        let mut settings = Settings::default();
        settings.record_mut(Level::Medium).wins += 1;
        settings.record_mut(Level::Hard).losses += 1;
        assert_eq!(settings.record_for(Level::Medium).wins, 1);
        assert_eq!(settings.record_for(Level::Hard).losses, 1);
        assert_eq!(settings.record_for(Level::Easy), LevelRecord::default());
    }
}
