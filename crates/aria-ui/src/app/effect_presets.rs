//! User-scoped effect preset persistence and state reach (BACKLOG 58).
//!
//! Presets are stored under the user's profile directory (`presets/effects.json`).
//! They are loaded on startup and persisted on modification. Corrupted or unreadable
//! files gracefully fall back to an empty list without crashing the application.

use std::fs;
use std::path::{Path, PathBuf};

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{EffectPreset, validate_preset_list};
use serde::{Deserialize, Serialize};

use super::AppState;

const PRESETS_DIR: &str = "presets";
const PRESETS_FILE: &str = "effects.json";
const PRESETS_VERSION: u32 = 1;

/// The on-disk wrapper for the presets file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PresetsFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    presets: Vec<EffectPreset>,
}

/// The path where effect presets are persisted for `data_dir`.
pub fn effect_presets_path(data_dir: &Path) -> PathBuf {
    data_dir.join(PRESETS_DIR).join(PRESETS_FILE)
}

/// Load effect presets from disk.
///
/// Returns an empty list if the file is missing, unreadable, or contains
/// invalid JSON / invalid presets, ensuring app startup never fails on corrupt data.
pub fn load_effect_presets(data_dir: &Path) -> Vec<EffectPreset> {
    let path = effect_presets_path(data_dir);
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    if let Ok(file) = serde_json::from_str::<PresetsFile>(&text) {
        if validate_preset_list(&file.presets).is_ok() {
            return file.presets;
        }
    }
    if let Ok(presets) = serde_json::from_str::<Vec<EffectPreset>>(&text) {
        if validate_preset_list(&presets).is_ok() {
            return presets;
        }
    }
    Vec::new()
}

/// Save effect presets to disk.
pub fn save_effect_presets(data_dir: &Path, presets: &[EffectPreset]) -> Result<(), String> {
    validate_preset_list(presets).map_err(|err| err.to_string())?;
    let path = effect_presets_path(data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("could not create presets dir: {err}"))?;
    }
    let file = PresetsFile {
        version: PRESETS_VERSION,
        presets: presets.to_vec(),
    };
    let json = serde_json::to_string_pretty(&file)
        .map_err(|err| format!("could not serialize effect presets: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("could not write effect presets: {err}"))?;
    Ok(())
}

impl AppState {
    /// The currently loaded effect presets.
    pub fn effect_presets(&self) -> &[EffectPreset] {
        &self.effect_presets
    }

    /// Look up an effect preset by name.
    pub fn effect_preset(&self, name: &str) -> Option<&EffectPreset> {
        let trimmed = name.trim();
        self.effect_presets
            .iter()
            .find(|p| p.name.trim().eq_ignore_ascii_case(trimmed))
    }

    /// Save the selected clip's current effect stack as a new named preset.
    ///
    /// Refuses empty or duplicate names, or when no clip is selected.
    /// Persists to disk immediately on success.
    pub fn save_selected_effect_preset(&mut self, name: impl AsRef<str>) -> Result<(), String> {
        let name_str = name.as_ref().trim();
        if name_str.is_empty() {
            return Err("effect preset name cannot be empty".to_string());
        }
        if self
            .effect_presets
            .iter()
            .any(|p| p.name.trim().eq_ignore_ascii_case(name_str))
        {
            return Err(format!("duplicate effect preset name '{name_str}'"));
        }
        let clip_id = self.selected_clip().ok_or("no clip selected")?;
        let effects = {
            let engine = self.engine.lock();
            let clip = engine
                .project()
                .clip(clip_id)
                .ok_or("selected clip not found in project")?;
            clip.effects.to_vec()
        };
        let preset = EffectPreset::new(name_str.to_string(), effects);
        preset.validate().map_err(|err| err.to_string())?;

        let mut updated = self.effect_presets.to_vec();
        updated.push(preset);
        save_effect_presets(&self.data_dir, &updated)?;
        self.effect_presets = updated;
        self.status_message = Some(format!("Saved preset '{name_str}'"));
        Ok(())
    }

    /// Apply an effect preset to the selected clip by name.
    ///
    /// Emits a single [`EditCommand::ApplyEffectPreset`] command, creating one undo entry.
    pub fn apply_effect_preset(&mut self, name: &str) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let preset = self
            .effect_preset(name)
            .map(|p| EffectPreset::new(p.name.to_string(), p.effects.to_vec()))
            .ok_or_else(|| EngineError::Refused(format!("unknown effect preset '{name}'")))?;
        self.apply_command(Command::Edit(EditCommand::ApplyEffectPreset {
            clip,
            preset,
        }))?;
        self.status_message = Some(format!("Applied preset '{name}'"));
        Ok(())
    }

    /// Delete an effect preset by name and persist the updated list.
    pub fn delete_effect_preset(&mut self, name: &str) -> Result<(), String> {
        let trimmed = name.trim();
        let mut updated = self.effect_presets.to_vec();
        let before_len = updated.len();
        updated.retain(|p| !p.name.trim().eq_ignore_ascii_case(trimmed));
        if updated.len() == before_len {
            return Err(format!("preset '{name}' not found"));
        }
        save_effect_presets(&self.data_dir, &updated)?;
        self.effect_presets = updated;
        self.status_message = Some(format!("Deleted preset '{name}'"));
        Ok(())
    }
}
