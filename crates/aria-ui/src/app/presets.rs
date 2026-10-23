//! User-scoped and plugin-contributed animation presets (`.ariapreset`).
//!
//! User presets live under `<data dir>/presets/*.ariapreset`, one file per
//! preset, loaded on startup and sorted by name. Any `.ariapreset` file in the
//! directory that cannot be read or fails validation is recorded in
//! [`AppState::broken_presets`] with its path and reason so the Presets section
//! can surface it instead of dropping it in silence.
//!
//! Legacy `presets/effects.json` files from the pre-animation-preset format are
//! migrated once to `.ariapreset` files (anchor 0) and removed only after every
//! migrated file has been written.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    ClipId, ClipParam, EffectInstance, PRESET_FILE_EXTENSION, PRESET_MAX_BYTES, Preset,
    PresetParts, RationalTime,
};
use aria_plugin::PluginPreset;
use serde::{Deserialize, Serialize};

use super::AppState;
use super::decline::Decline;

const PRESETS_DIR: &str = "presets";
const LEGACY_PRESETS_FILE: &str = "effects.json";

/// Maximum length of a preset name typed in the inspector's draft field.
pub const PRESET_NAME_MAX_LEN: usize = 48;

/// A `.ariapreset` file in `<data dir>/presets/` that failed to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenPreset {
    pub path: PathBuf,
    pub reason: String,
}

/// The inspector's open "Save as preset" draft: the typed name and one toggle
/// per part (each effect by name, the audio inserts, each keyframed property
/// by label), all on by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetDraft {
    pub name: String,
    /// `(effect_index, display_label, included)` for each visual effect on the clip.
    pub effects: Vec<(usize, String, bool)>,
    /// Indices of audio inserts on the clip when the draft was opened.
    pub audio_effects: Vec<usize>,
    /// Whether the clip's audio inserts are included in the preset.
    pub include_audio_effects: bool,
    /// `(property_id, display_label, included)` for each keyframed property on the clip.
    pub properties: Vec<(String, String, bool)>,
}

impl PresetDraft {
    /// Build the [`PresetParts`] currently toggled on in this draft.
    pub fn selected_parts(&self) -> PresetParts {
        PresetParts {
            effects: self
                .effects
                .iter()
                .filter_map(|(idx, _, on)| on.then_some(*idx))
                .collect(),
            audio_effects: if self.include_audio_effects {
                self.audio_effects.clone()
            } else {
                Vec::new()
            },
            properties: self
                .properties
                .iter()
                .filter(|(_, _, on)| *on)
                .map(|(id, _, _)| id.clone())
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct LegacyPresetsFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    presets: Vec<LegacyEffectPreset>,
}

#[derive(Debug, Deserialize, Serialize)]
struct LegacyEffectPreset {
    name: String,
    #[serde(default)]
    effects: Vec<EffectInstance>,
}

/// Directory where user `.ariapreset` files are stored under `data_dir`.
pub fn presets_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(PRESETS_DIR)
}

/// Legacy `presets/effects.json` path under `data_dir`.
pub fn legacy_effect_presets_path(data_dir: &Path) -> PathBuf {
    presets_dir(data_dir).join(LEGACY_PRESETS_FILE)
}

/// Whether `path` has the `.ariapreset` extension (case-insensitive).
pub fn is_preset_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(PRESET_FILE_EXTENSION))
}

fn sanitize_stem(name: &str) -> String {
    let mut stem = String::new();
    let mut last_dash = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            stem.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if (ch == '-' || ch == '_' || ch.is_whitespace() || ch.is_ascii_punctuation())
            && !stem.is_empty()
            && !last_dash
        {
            stem.push('-');
            last_dash = true;
        }
    }
    while stem.ends_with('-') {
        stem.pop();
    }
    if stem.is_empty() {
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in name.trim().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("preset-{hash:016x}")
    } else {
        stem
    }
}

fn target_file_path_for(dir: &Path, name: &str) -> PathBuf {
    let base_stem = sanitize_stem(name);
    let candidate = dir.join(format!("{base_stem}.{PRESET_FILE_EXTENSION}"));
    if !candidate.exists() {
        return candidate;
    }
    if let Ok(bytes) = fs::read(&candidate)
        && let Ok(existing) = Preset::parse(&bytes)
        && existing.name.eq_ignore_ascii_case(name.trim())
    {
        return candidate;
    }
    let mut suffix = 2usize;
    loop {
        let numbered = dir.join(format!("{base_stem}-{suffix}.{PRESET_FILE_EXTENSION}"));
        if !numbered.exists() {
            return numbered;
        }
        if let Ok(bytes) = fs::read(&numbered)
            && let Ok(existing) = Preset::parse(&bytes)
            && existing.name.eq_ignore_ascii_case(name.trim())
        {
            return numbered;
        }
        suffix += 1;
    }
}

/// Migrate `<data_dir>/presets/effects.json` to `.ariapreset` files and delete
/// `effects.json` only after every file is written.
fn migrate_legacy_effects_json(data_dir: &Path) {
    let legacy_path = legacy_effect_presets_path(data_dir);
    if !legacy_path.is_file() {
        return;
    }
    let Ok(text) = fs::read_to_string(&legacy_path) else {
        return;
    };
    let parsed_entries = serde_json::from_str::<LegacyPresetsFile>(&text)
        .map(|file| file.presets)
        .or_else(|_| serde_json::from_str::<Vec<LegacyEffectPreset>>(&text));
    let Ok(entries) = parsed_entries else {
        return;
    };

    let mut seen = BTreeSet::new();
    let mut converted = Vec::with_capacity(entries.len());
    for entry in entries {
        let preset = Preset::new(entry.name, entry.effects, Vec::new(), Vec::new());
        if preset.validate().is_err() {
            return;
        }
        let key = preset.name.to_ascii_lowercase();
        if !seen.insert(key) {
            return;
        }
        converted.push(preset);
    }

    let dir = presets_dir(data_dir);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    for preset in &converted {
        let Ok(json) = preset.to_json() else {
            return;
        };
        let dest = target_file_path_for(&dir, &preset.name);
        if fs::write(&dest, json).is_err() {
            return;
        }
    }
    let _ = fs::remove_file(&legacy_path);
}

/// Load user presets and any broken `.ariapreset` files from `<data_dir>/presets/`.
pub fn load_presets(data_dir: &Path) -> (Vec<Preset>, Vec<BrokenPreset>) {
    migrate_legacy_effects_json(data_dir);

    let dir = presets_dir(data_dir);
    let Ok(read_dir) = fs::read_dir(&dir) else {
        return (Vec::new(), Vec::new());
    };
    let mut paths = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if is_preset_path(&path) {
            paths.push(path);
        }
    }
    paths.sort();

    let mut presets = Vec::new();
    let mut broken = Vec::new();
    let mut seen_names = BTreeSet::new();

    for path in paths {
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(err) => {
                broken.push(BrokenPreset {
                    path,
                    reason: err.to_string(),
                });
                continue;
            }
        };
        let mut bytes = Vec::new();
        if let Err(err) = file.take(PRESET_MAX_BYTES + 1).read_to_end(&mut bytes) {
            broken.push(BrokenPreset {
                path,
                reason: err.to_string(),
            });
            continue;
        }
        match Preset::parse(&bytes) {
            Ok(preset) => {
                let key = preset.name.to_ascii_lowercase();
                if !seen_names.insert(key) {
                    broken.push(BrokenPreset {
                        path,
                        reason: format!("duplicate preset name `{}`", preset.name),
                    });
                    continue;
                }
                presets.push(preset);
            }
            Err(err) => {
                broken.push(BrokenPreset {
                    path,
                    reason: err.to_string(),
                });
            }
        }
    }

    presets.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    broken.sort_by(|a, b| a.path.cmp(&b.path));
    (presets, broken)
}

fn write_preset_file(data_dir: &Path, preset: &Preset) -> Result<PathBuf, String> {
    let json = preset.to_json().map_err(|err| err.to_string())?;
    let dir = presets_dir(data_dir);
    fs::create_dir_all(&dir).map_err(|err| format!("could not create presets dir: {err}"))?;
    let path = target_file_path_for(&dir, &preset.name);
    fs::write(&path, json).map_err(|err| format!("could not write {}: {err}", path.display()))?;
    Ok(path)
}

fn remove_preset_files_named(data_dir: &Path, name: &str) -> Result<bool, String> {
    let dir = presets_dir(data_dir);
    let Ok(read_dir) = fs::read_dir(&dir) else {
        return Ok(false);
    };
    let trimmed = name.trim();
    let mut removed = false;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !is_preset_path(&path) {
            continue;
        }
        if let Ok(bytes) = fs::read(&path)
            && let Ok(preset) = Preset::parse(&bytes)
            && preset.name.trim().eq_ignore_ascii_case(trimmed)
        {
            fs::remove_file(&path)
                .map_err(|err| format!("could not delete {}: {err}", path.display()))?;
            removed = true;
        }
    }
    Ok(removed)
}

impl AppState {
    /// User presets loaded from `<data_dir>/presets/*.ariapreset`, sorted by name.
    pub fn presets(&self) -> &[Preset] {
        &self.presets
    }

    /// Corrupt or invalid `.ariapreset` files found in `<data_dir>/presets/`.
    pub fn broken_presets(&self) -> &[BrokenPreset] {
        &self.broken_presets
    }

    /// Presets contributed by currently mounted plugins.
    pub fn plugin_presets(&self) -> Vec<PluginPreset> {
        self.ctx.presets()
    }

    /// Look up a preset by name across user presets and mounted plugin presets.
    pub fn preset(&self, name: &str) -> Option<Preset> {
        let trimmed = name.trim();
        if let Some(found) = self
            .presets
            .iter()
            .find(|p| p.name.trim().eq_ignore_ascii_case(trimmed))
        {
            return Some(found.clone());
        }
        self.ctx
            .presets()
            .into_iter()
            .find(|entry| entry.preset.name.trim().eq_ignore_ascii_case(trimmed))
            .map(|entry| (*entry.preset).clone())
    }

    /// The open preset save draft, if any.
    pub fn preset_draft(&self) -> Option<&PresetDraft> {
        self.preset_draft.as_ref()
    }

    /// Open the preset save draft over the primary selected clip, with every
    /// available part toggled on by default.
    pub fn open_preset_draft(&mut self) {
        let Some(clip_id) = self.selected_clip() else {
            self.status_message = Some("Select a clip to save a preset".into());
            return;
        };
        let properties: Vec<(String, String, bool)> = self
            .key_params()
            .iter()
            .filter(|row| !matches!(row.param(), ClipParam::Effect { .. }) && !row.marks.is_empty())
            .map(|row| (row.id.to_string(), row.label.to_string(), true))
            .collect();
        let draft = {
            let engine = self.engine.lock();
            let Some(clip) = engine.project().timeline().clip(clip_id) else {
                self.status_message = Some("Select a clip to save a preset".into());
                return;
            };
            let effects = clip
                .effects
                .iter()
                .enumerate()
                .map(|(index, fx)| (index, fx.display_name(), true))
                .collect();
            let audio_effects: Vec<usize> = (0..clip.audio_effects.len()).collect();
            let include_audio_effects = !audio_effects.is_empty();
            PresetDraft {
                name: String::new(),
                effects,
                audio_effects,
                include_audio_effects,
                properties,
            }
        };
        self.preset_draft = Some(draft);
    }

    /// Close the preset save draft without saving.
    pub fn cancel_preset_draft(&mut self) {
        self.preset_draft = None;
    }

    /// Whether the preset save draft is open.
    pub fn preset_draft_open(&self) -> bool {
        self.preset_draft.is_some()
    }

    /// Current text in the preset save draft's name field.
    pub fn preset_draft_text(&self) -> String {
        self.preset_draft
            .as_ref()
            .map(|draft| draft.name.clone())
            .unwrap_or_default()
    }

    pub fn preset_draft_push(&mut self, ch: char) {
        if let Some(draft) = self.preset_draft.as_mut()
            && draft.name.chars().count() < PRESET_NAME_MAX_LEN
        {
            draft.name.push(ch);
        }
    }

    pub fn preset_draft_pop(&mut self) {
        if let Some(draft) = self.preset_draft.as_mut() {
            draft.name.pop();
        }
    }

    /// Toggle whether effect `index` on the selected clip is included in the draft.
    pub fn toggle_preset_draft_effect(&mut self, index: usize) {
        if let Some(draft) = self.preset_draft.as_mut()
            && let Some((_, _, on)) = draft
                .effects
                .iter_mut()
                .find(|(fx_idx, _, _)| *fx_idx == index)
        {
            *on = !*on;
        }
    }

    /// Toggle whether the selected clip's audio inserts are included in the draft.
    pub fn toggle_preset_draft_audio_effects(&mut self) {
        if let Some(draft) = self.preset_draft.as_mut()
            && !draft.audio_effects.is_empty()
        {
            draft.include_audio_effects = !draft.include_audio_effects;
        }
    }

    /// Toggle whether keyframed property `id` is included in the draft.
    pub fn toggle_preset_draft_property(&mut self, id: &str) {
        if let Some(draft) = self.preset_draft.as_mut()
            && let Some((_, _, on)) = draft
                .properties
                .iter_mut()
                .find(|(prop_id, _, _)| prop_id == id)
        {
            *on = !*on;
        }
    }

    /// Commit the open preset draft.
    ///
    /// Closes the draft only on success; a refused name or an empty part
    /// selection leaves the draft open with the typed text and toggles intact.
    pub fn commit_preset_draft(&mut self) -> Result<(), String> {
        let Some(draft) = self.preset_draft.clone() else {
            return Ok(());
        };
        let parts = draft.selected_parts();
        self.save_selected_preset_parts(&draft.name, &parts)?;
        self.preset_draft = None;
        Ok(())
    }

    /// Save all effects, audio inserts, and keyframed properties of the primary
    /// selected clip as a named preset anchored at the playhead (when inside the
    /// clip, else at the clip start).
    pub fn save_selected_preset(&mut self, name: impl AsRef<str>) -> Result<(), String> {
        let clip_id = self.selected_clip().ok_or(Decline::NoSelection.reason())?;
        let parts = {
            let engine = self.engine.lock();
            let clip = engine
                .project()
                .timeline()
                .clip(clip_id)
                .ok_or("selected clip not found in project")?;
            PresetParts::all_of(clip)
        };
        self.save_selected_preset_parts(name, &parts)
    }

    /// Save the specified `parts` of the primary selected clip as a named
    /// `.ariapreset` in `<data_dir>/presets/`.
    pub fn save_selected_preset_parts(
        &mut self,
        name: impl AsRef<str>,
        parts: &PresetParts,
    ) -> Result<(), String> {
        let name_str = name.as_ref().trim();
        if name_str.is_empty() {
            return Err("preset name cannot be empty; enter a preset name".to_string());
        }
        if self
            .presets
            .iter()
            .any(|p| p.name.trim().eq_ignore_ascii_case(name_str))
        {
            return Err(format!(
                "duplicate preset name '{name_str}'; choose a different name or delete the existing preset"
            ));
        }
        if parts.is_empty() {
            return Err(
                "preset selection is empty; enable at least one effect, audio insert, or property"
                    .to_string(),
            );
        }
        let clip_id = self.selected_clip().ok_or(Decline::NoSelection.reason())?;
        let preset = {
            let engine = self.engine.lock();
            let clip = engine
                .project()
                .timeline()
                .clip(clip_id)
                .ok_or("selected clip not found in project")?;
            let playhead = RationalTime::new(self.playhead.value, clip.timeline.start.rate);
            let anchor_local = if clip.timeline.contains(playhead).unwrap_or(false) {
                clip.param_tick(playhead.value)
            } else {
                0
            };
            Preset::capture(name_str, clip, anchor_local, parts).map_err(|err| err.to_string())?
        };

        write_preset_file(&self.data_dir, &preset)?;
        self.presets.push(preset);
        self.presets.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        });
        self.status_message = Some(format!("Saved preset '{name_str}'"));
        Ok(())
    }

    /// Apply a user or plugin preset by name across every selected clip in a
    /// single undo step, anchored at the playhead when it sits inside the clip
    /// and at the clip start otherwise.
    pub fn apply_preset(&mut self, name: &str) -> Result<(), EngineError> {
        if self.selection.is_empty() {
            return Ok(());
        }
        let preset = self
            .preset(name)
            .ok_or_else(|| EngineError::Refused(format!("unknown preset '{name}'")))?;
        let targets: Vec<(ClipId, RationalTime)> = {
            let engine = self.engine.lock();
            let project = engine.project();
            let mut list = Vec::with_capacity(self.selection.len());
            for &clip_id in &self.selection {
                let clip = project
                    .timeline()
                    .clip(clip_id)
                    .ok_or_else(|| EngineError::Refused(format!("unknown clip {clip_id}")))?;
                let playhead = RationalTime::new(self.playhead.value, clip.timeline.start.rate);
                let at = if clip.timeline.contains(playhead).unwrap_or(false) {
                    playhead
                } else {
                    clip.timeline.start
                };
                list.push((clip_id, at));
            }
            list
        };
        if targets.len() == 1 {
            let (clip, at) = targets[0];
            self.apply_command(Command::Edit(EditCommand::ApplyPreset { clip, preset, at }))?;
        } else {
            self.one_step(|state| {
                for (clip, at) in targets {
                    state.apply_command(Command::Edit(EditCommand::ApplyPreset {
                        clip,
                        preset: preset.clone(),
                        at,
                    }))?;
                }
                Ok(())
            })?;
        }
        self.status_message = Some(format!("Applied preset '{}'", name.trim()));
        Ok(())
    }

    /// Delete a user preset by name, removing its `.ariapreset` file from disk.
    pub fn delete_preset(&mut self, name: &str) -> Result<(), String> {
        let trimmed = name.trim();
        let before_len = self.presets.len();
        if !self
            .presets
            .iter()
            .any(|p| p.name.trim().eq_ignore_ascii_case(trimmed))
        {
            return Err(format!("preset '{name}' not found"));
        }
        remove_preset_files_named(&self.data_dir, trimmed)?;
        self.presets
            .retain(|p| !p.name.trim().eq_ignore_ascii_case(trimmed));
        debug_assert!(self.presets.len() < before_len);
        self.status_message = Some(format!("Deleted preset '{trimmed}'"));
        Ok(())
    }

    /// Import a `.ariapreset` file into the preset library, and apply it to the
    /// selected clips when any clip is selected.
    pub fn import_preset_file(&mut self, path: &Path) -> Result<String, String> {
        let result: Result<String, String> = (|| {
            let file = fs::File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
            let mut bytes = Vec::new();
            file.take(PRESET_MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|err| format!("{}: {err}", path.display()))?;
            let preset =
                Preset::parse(&bytes).map_err(|err| format!("{}: {err}", path.display()))?;
            if self
                .presets
                .iter()
                .any(|existing| existing.name.eq_ignore_ascii_case(&preset.name))
            {
                return Err(format!(
                    "preset '{}' already exists in the library; rename or delete the existing preset before importing {}",
                    preset.name,
                    path.display()
                ));
            }
            write_preset_file(&self.data_dir, &preset)?;
            let name = preset.name.clone();
            self.presets.push(preset);
            self.presets.sort_by(|a, b| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
                    .then_with(|| a.name.cmp(&b.name))
            });
            if !self.selection.is_empty() {
                self.apply_preset(&name).map_err(|err| err.to_string())?;
            } else {
                self.status_message = Some(format!("Installed preset '{name}'"));
            }
            Ok(name)
        })();
        if let Err(err) = &result {
            self.report_error(err);
        }
        result
    }

    /// Partition `.ariapreset` paths out of `paths` before plugins are asked,
    /// importing each into the preset library (and applying to selected clips).
    /// Returns the remaining paths in order.
    pub fn take_preset_files(&mut self, paths: Vec<PathBuf>) -> Vec<PathBuf> {
        let (presets, rest): (Vec<_>, Vec<_>) =
            paths.into_iter().partition(|path| is_preset_path(path));
        for path in presets {
            let _ = self.import_preset_file(&path);
        }
        rest
    }
}
