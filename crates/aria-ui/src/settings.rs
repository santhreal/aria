//! UI settings persisted to `ui-settings.json` in the data dir.
//!
//! Ported from Comet (<https://github.com/zeronsh/comet>)
//! MIT License — see LICENSES/comet-MIT.txt

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SIDEBAR_MIN: f32 = 208.0;
pub const SIDEBAR_MAX: f32 = 400.0;
pub const SIDEBAR_DEFAULT: f32 = 256.0;

pub const INSPECTOR_MIN: f32 = 360.0;
pub const INSPECTOR_MAX: f32 = 760.0;
pub const INSPECTOR_DEFAULT: f32 = 520.0;

pub const TIMELINE_MIN_HEIGHT: f32 = 120.0;
pub const TIMELINE_DEFAULT_HEIGHT: f32 = 280.0;

pub const SAVE_DEBOUNCE_MS: u64 = 400;

const FILE_NAME: &str = "ui-settings.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiSettings {
    pub sidebar_width: f32,
    pub sidebar_collapsed: bool,
    pub inspector_width: f32,
    pub inspector_collapsed: bool,
    pub timeline_height: f32,
    pub appearance: crate::appearance::AppearanceMode,
    pub home_rail: crate::types::HomeRail,
    /// How much motion runs. Persisted here rather than read from the
    /// environment on every launch: a choice the operator made in Settings has
    /// to survive the next start.
    pub motion: crate::motion::MotionPreference,
    /// Rebound keys, by binding id, spelled the way Settings shows them
    /// (`Ctrl+Shift+P`). A `None` value is a binding the operator cleared, which
    /// is not the same as an absent one: absent means the shipped chord.
    ///
    /// Sorted rather than insertion-ordered so the saved file does not churn
    /// when one binding changes.
    pub keys: BTreeMap<String, Option<String>>,
    /// The arrangement the operator saved, or nothing when they never did.
    ///
    /// One slot rather than a list: see [`crate::layouts`] for why a menu of
    /// user-named arrangements is a menu the static bar cannot draw. Absent means
    /// the Custom row is not offered at all, which is not the same as a slot that
    /// happens to match a shipped workspace.
    pub workspace: Option<crate::layouts::PaneLayout>,
    /// How the pool draws itself. A view choice rather than a window size, but it
    /// lives here for the same reason: it is a preference about the app, not a
    /// fact about the project, and it has to survive the next launch.
    pub pool_view: crate::types::PoolView,
}

/// The pane sizes a frame draws.
///
/// The five numbers the shell needs out of the settings, copied rather than
/// borrowed because a panel cannot hold a borrow of the state across the
/// listeners it attaches. Copying these is five words; copying the settings to
/// read a width copied the whole key map with it, once a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneGeometry {
    pub sidebar_width: f32,
    pub sidebar_collapsed: bool,
    pub inspector_width: f32,
    pub inspector_collapsed: bool,
    pub timeline_height: f32,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            sidebar_width: SIDEBAR_DEFAULT,
            sidebar_collapsed: false,
            inspector_width: INSPECTOR_DEFAULT,
            inspector_collapsed: false,
            timeline_height: TIMELINE_DEFAULT_HEIGHT,
            appearance: crate::appearance::AppearanceMode::default(),
            home_rail: crate::types::HomeRail::default(),
            motion: crate::motion::MotionPreference::default(),
            keys: BTreeMap::new(),
            workspace: None,
            pool_view: crate::types::PoolView::default(),
        }
    }
}

impl UiSettings {
    /// The pane sizes, for a frame that needs nothing else from here.
    pub fn panes(&self) -> PaneGeometry {
        PaneGeometry {
            sidebar_width: self.sidebar_width,
            sidebar_collapsed: self.sidebar_collapsed,
            inspector_width: self.inspector_width,
            inspector_collapsed: self.inspector_collapsed,
            timeline_height: self.timeline_height,
        }
    }

    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join(FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<UiSettings>(&text) {
                Ok(mut settings) => {
                    settings.clamp();
                    settings
                }
                Err(err) => {
                    tracing::warn!(error = %err, path = %path.display(), "corrupt ui-settings, using defaults");
                    Self::default()
                }
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(err) => {
                tracing::warn!(error = %err, path = %path.display(), "could not read ui-settings");
                Self::default()
            }
        }
    }

    pub fn save(&self, data_dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let path = data_dir.join(FILE_NAME);
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, text)
    }

    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join(FILE_NAME)
    }

    fn clamp(&mut self) {
        self.sidebar_width = self.sidebar_width.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
        self.inspector_width = self.inspector_width.clamp(INSPECTOR_MIN, INSPECTOR_MAX);
        self.timeline_height = self.timeline_height.max(TIMELINE_MIN_HEIGHT);
    }
}

/// `~/.aria` — the user data dir for UI settings.
pub fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".aria")
}

/// The profile root as an absolute path, or why it cannot be one.
///
/// `ARIA_DATA_DIR=scratch/profile` is a reasonable thing to type, and every
/// storage layout under the root refuses a relative path, so the resolution
/// happens once where the session is built rather than as an assertion in each
/// reader. The working directory is the only thing a relative override can mean.
///
/// The returned path is absolute or this is an error: a resolution that came
/// back relative anyway (an unreachable working directory) would put the
/// assertion back in the reader, one layer further from the operator.
pub fn absolute_data_dir(dir: &Path) -> Result<PathBuf, String> {
    if dir.is_absolute() {
        return Ok(dir.to_path_buf());
    }
    let refuse = |what: String| {
        format!(
            "the data directory {} is not absolute and could not be resolved \
             against the working directory: {what}",
            dir.display()
        )
    };
    let resolved = std::path::absolute(dir).map_err(|err| refuse(err.to_string()))?;
    if !resolved.is_absolute() {
        return Err(refuse(format!("it resolved to {}", resolved.display())));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_in_range() {
        let s = UiSettings::default();
        assert!((SIDEBAR_MIN..=SIDEBAR_MAX).contains(&s.sidebar_width));
        assert!((INSPECTOR_MIN..=INSPECTOR_MAX).contains(&s.inspector_width));
        assert!(s.timeline_height >= TIMELINE_MIN_HEIGHT);
    }

    /// Resolution keeps what was asked for and refuses only what cannot be a
    /// path at all, so a relative override reaches the storage layout as the
    /// directory the operator meant.
    #[test]
    fn a_relative_profile_root_resolves_and_an_empty_one_is_refused() {
        let absolute = std::env::temp_dir().join("aria-absolute-root");
        assert_eq!(
            absolute_data_dir(&absolute).as_deref(),
            Ok(absolute.as_path()),
            "an absolute root was rewritten"
        );

        let relative = Path::new("profiles").join("scratch");
        let resolved = absolute_data_dir(&relative).expect("a relative root resolves");
        assert!(resolved.is_absolute(), "{}", resolved.display());
        assert!(
            resolved.ends_with(&relative),
            "the resolved root dropped what was asked for: {}",
            resolved.display()
        );

        let refusal = absolute_data_dir(Path::new("")).expect_err("an empty root is not a path");
        assert!(
            refusal.contains("not absolute"),
            "the refusal does not say what is wrong: {refusal}"
        );
    }

    #[test]
    fn clamp_heals_hand_edited_values() {
        let mut s = UiSettings {
            sidebar_width: 10.0,
            inspector_width: 9000.0,
            timeline_height: 10.0,
            ..UiSettings::default()
        };
        s.clamp();
        assert_eq!(s.sidebar_width, SIDEBAR_MIN);
        assert_eq!(s.inspector_width, INSPECTOR_MAX);
        assert_eq!(s.timeline_height, TIMELINE_MIN_HEIGHT);
    }

    /// A file written before this setting existed still opens the app, with the
    /// motion the app shipped with. Persisted shapes outlive the code that
    /// wrote them, and a settings file that failed to parse would silently
    /// reset the pane widths too.
    #[test]
    fn a_file_written_before_the_motion_setting_loads_as_full_motion() {
        let older = r#"{"sidebarWidth":300.0,"appearance":"dark"}"#;
        let parsed: UiSettings = serde_json::from_str(older).expect("older settings file");
        assert_eq!(parsed.motion, crate::motion::MotionPreference::Full);
        assert_eq!(parsed.sidebar_width, 300.0);
    }

    /// The choice is written under the key the file already uses for everything
    /// else, so a hand-edited settings file and the dialog agree.
    #[test]
    fn the_motion_choice_survives_a_round_trip() {
        let settings = UiSettings {
            motion: crate::motion::MotionPreference::Brisk,
            ..UiSettings::default()
        };
        let text = serde_json::to_string(&settings).expect("serialize");
        assert!(text.contains("\"motion\":\"brisk\""), "{text}");
        let back: UiSettings = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, settings);
    }

    /// A file written before keys could be rebound opens with the shipped keys,
    /// and a cleared binding is a `null` rather than an absent row: the two mean
    /// different things, and a serializer that dropped the `null` would put a key
    /// back that the operator took away.
    #[test]
    fn a_cleared_binding_persists_as_null_and_an_older_file_has_no_keys() {
        let older = r#"{"sidebarWidth":300.0}"#;
        let parsed: UiSettings = serde_json::from_str(older).expect("older settings file");
        assert!(parsed.keys.is_empty());

        let mut settings = UiSettings::default();
        settings.keys.insert("key-save".to_string(), None);
        settings
            .keys
            .insert("key-split".to_string(), Some("Ctrl+Alt+x".to_string()));
        let text = serde_json::to_string(&settings).expect("serialize");
        assert!(text.contains("\"key-save\":null"), "{text}");
        assert!(text.contains("\"key-split\":\"Ctrl+Alt+x\""), "{text}");
        let back: UiSettings = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, settings);
    }

    /// A file written before workspaces existed has no slot, which is not the
    /// same as a slot holding the default arrangement: absent means the Custom
    /// row is not offered at all. The panel and the route are spelled out rather
    /// than numbered, so a variant added in the middle of either enum cannot
    /// silently reinterpret a saved slot.
    #[test]
    fn the_saved_workspace_slot_round_trips_and_an_older_file_has_none() {
        let older = r#"{"sidebarWidth":300.0}"#;
        let parsed: UiSettings = serde_json::from_str(older).expect("older settings file");
        assert!(parsed.workspace.is_none());

        let settings = UiSettings {
            workspace: Some(crate::layouts::PaneLayout {
                sidebar_collapsed: true,
                panel: crate::types::ActivePanel::Inspector,
                route: crate::types::UiRoute::Home,
                ..crate::layouts::PaneLayout::default()
            }),
            ..UiSettings::default()
        };
        let text = serde_json::to_string(&settings).expect("serialize");
        assert!(text.contains("\"panel\":\"inspector\""), "{text}");
        assert!(text.contains("\"route\":\"home\""), "{text}");
        let back: UiSettings = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, settings);
    }
}
