//! UI settings persisted to `ui-settings.json` in the data dir.
//!
//! Ported from Comet (<https://github.com/zeronsh/comet>)
//! MIT License — see LICENSES/comet-MIT.txt

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The smallest main window, in logical pixels (`docs/spec/layout.md:22-27`).
///
/// The platform holds the window at this size or larger and a restored
/// placement opens no smaller. A window the window manager sizes below it in
/// either direction shows the Viewer and the Timeline only.
pub const MIN_WINDOW_WIDTH: f32 = 1024.0;
pub const MIN_WINDOW_HEIGHT: f32 = 640.0;

pub const SIDEBAR_MIN: f32 = 208.0;
pub const SIDEBAR_MAX: f32 = 400.0;
pub const SIDEBAR_DEFAULT: f32 = 256.0;

pub const INSPECTOR_MIN: f32 = 360.0;
pub const INSPECTOR_MAX: f32 = 760.0;
pub const INSPECTOR_DEFAULT: f32 = 520.0;

pub const TIMELINE_MIN_HEIGHT: f32 = 120.0;
pub const TIMELINE_DEFAULT_HEIGHT: f32 = 280.0;

/// The Timeline's outline column, the track headers and property names to the
/// left of the time area. The minimum holds a lane header's badge, every one
/// of its toggles, and a few characters of its name.
pub const OUTLINE_MIN: f32 = 240.0;
pub const OUTLINE_MAX: f32 = 480.0;
pub const OUTLINE_DEFAULT: f32 = 300.0;

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
    /// The width of the Timeline's outline column, from [`OUTLINE_MIN`] to
    /// [`OUTLINE_MAX`].
    pub outline_width: f32,
    pub appearance: crate::appearance::AppearanceMode,
    pub home_rail: crate::types::HomeRail,
    /// How much motion runs. Persisted here rather than read from the
    /// environment on every launch: a choice made in Settings has to survive
    /// the next start.
    pub motion: crate::motion::MotionPreference,
    /// Rebound keys, by binding id, spelled the way Settings shows them
    /// (`Ctrl+Shift+P`). A `None` value is a cleared binding, which
    /// is not the same as an absent one: absent means the shipped chord.
    ///
    /// Sorted rather than insertion-ordered so the saved file does not churn
    /// when one binding changes.
    pub keys: BTreeMap<String, Option<String>>,
    /// The named set in `<data dir>/keymaps` last loaded or saved. Settings
    /// marks it; the keys in force are `keys` either way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keymap: Option<String>,
    /// The saved custom arrangement, or nothing when none was saved.
    ///
    /// One slot rather than a list: see [`crate::layouts`] for why a menu of
    /// user-named arrangements is a menu the static bar cannot draw. Absent means
    /// the Custom row is not offered at all, which is not the same as a slot that
    /// happens to match a shipped workspace.
    pub workspace: Option<crate::layouts::PaneLayout>,
    /// Current dock tree layout persisted across restarts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dock_tree: Option<crate::dock::DockTree>,
    /// Remembered dock tree edits per workspace id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub workspace_trees: BTreeMap<String, crate::dock::DockTree>,
    /// Most recently applied workspace id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_workspace: Option<String>,
    /// How the pool draws itself. A view choice rather than a window size, but it
    /// lives here for the same reason: it is a preference about the app, not a
    /// fact about the project, and it has to survive the next launch.
    pub pool_view: crate::types::PoolView,
    /// Write each dropped playback frame and its cause to the log, under the
    /// `aria::playback` target.
    pub log_dropped_frames: bool,
    /// The theme file applied over the appearance, by file name without
    /// `.toml`, from `<data dir>/themes`. `None` is the shipped theme.
    pub theme_file: Option<String>,
    /// The size the Viewer renders playback frames at.
    pub playback_resolution: crate::resolution::PlaybackResolution,
    /// The size the Viewer renders a paused or scrubbed frame at.
    pub paused_resolution: crate::resolution::PausedResolution,
    /// Whether the Viewer decodes through proxies where they exist. Export
    /// reads the originals either way.
    pub use_proxies: bool,
    /// Whether the Viewer draws a transparency grid where no layer covers
    /// the canvas, instead of the sequence's background colour.
    pub transparency_grid: bool,
    /// Play a 40 ms audio grain at each new pointer position while dragging
    /// the playhead, the ruler, or a timeline scrub.
    pub audio_scrubbing: bool,
    /// Timeline Display Settings: thumbnails, waveform scale, and how the view
    /// follows the playhead.
    pub timeline_display: crate::timeline_display::TimelineDisplay,
    /// What the scopes' waveform monitor shows: luma, or an RGB or YCbCr
    /// parade.
    pub waveform: crate::scopes::WaveformMode,
    /// The Effects panel's favourites, as `kind:id` keys such as
    /// `video:gaussian_blur`, in the order they were starred.
    pub favourite_effects: Vec<String>,
    /// Saved main window placement per display arrangement signature
    /// (`docs/spec/layout.md:24-26`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub window_placements: BTreeMap<String, WindowPlacement>,
    /// Most recent display arrangement signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_display_arrangement: Option<String>,
}

/// Saved placement of the main editor window for a display arrangement
/// (`docs/spec/layout.md:24-26`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowPlacement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
    pub display_index: usize,
}

impl Default for WindowPlacement {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
            maximized: true,
            display_index: 0,
        }
    }
}

/// Stable signature naming the current monitor layout (`[x, y, w, h]` per display).
pub fn display_arrangement_signature(displays: &[[f32; 4]]) -> String {
    use std::fmt::Write as _;
    if displays.is_empty() {
        return "primary".to_string();
    }
    let mut out = String::new();
    for (idx, &[x, y, w, h]) in displays.iter().enumerate() {
        if idx > 0 {
            out.push(';');
        }
        let _ = write!(
            out,
            "{},{},{}x{}",
            x.round() as i32,
            y.round() as i32,
            w.round() as i32,
            h.round() as i32
        );
    }
    out
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            sidebar_width: SIDEBAR_DEFAULT,
            sidebar_collapsed: false,
            inspector_width: INSPECTOR_DEFAULT,
            inspector_collapsed: false,
            timeline_height: TIMELINE_DEFAULT_HEIGHT,
            outline_width: OUTLINE_DEFAULT,
            appearance: crate::appearance::AppearanceMode::default(),
            home_rail: crate::types::HomeRail::default(),
            motion: crate::motion::MotionPreference::default(),
            keys: BTreeMap::new(),
            keymap: None,
            workspace: None,
            dock_tree: None,
            workspace_trees: BTreeMap::new(),
            active_workspace: None,
            pool_view: crate::types::PoolView::default(),
            log_dropped_frames: false,
            theme_file: None,
            playback_resolution: crate::resolution::PlaybackResolution::default(),
            paused_resolution: crate::resolution::PausedResolution::default(),
            use_proxies: true,
            transparency_grid: false,
            audio_scrubbing: true,
            timeline_display: crate::timeline_display::TimelineDisplay::default(),
            waveform: crate::scopes::WaveformMode::default(),
            favourite_effects: Vec::new(),
            window_placements: BTreeMap::new(),
            last_display_arrangement: None,
        }
    }
}

impl UiSettings {
    /// Resolve the main window placement for `displays` (`docs/spec/layout.md:24-26`).
    ///
    /// Restores the saved size, position, display, and maximized state for this
    /// display arrangement when present; otherwise opens maximized on the
    /// display it last closed on (or the primary display on first launch).
    pub fn resolve_window_placement(&self, displays: &[[f32; 4]]) -> WindowPlacement {
        let sig = display_arrangement_signature(displays);
        let count = displays.len().max(1);
        if let Some(&saved) = self.window_placements.get(&sig) {
            let display_index = saved.display_index.min(count - 1);
            let mut p = saved;
            p.display_index = display_index;
            p.width = p.width.max(MIN_WINDOW_WIDTH);
            p.height = p.height.max(MIN_WINDOW_HEIGHT);
            return p;
        }
        let fallback_display = self
            .last_display_arrangement
            .as_ref()
            .and_then(|last| self.window_placements.get(last))
            .map(|p| p.display_index.min(count - 1))
            .unwrap_or(0);
        let (dx, dy, dw, dh) = displays
            .get(fallback_display)
            .map(|&[x, y, w, h]| (x, y, w, h))
            .unwrap_or((0.0, 0.0, 1920.0, 1080.0));
        let width = 1440.0_f32.min(dw.max(MIN_WINDOW_WIDTH));
        let height = 900.0_f32.min(dh.max(MIN_WINDOW_HEIGHT));
        WindowPlacement {
            x: dx + ((dw - width) * 0.5).max(0.0),
            y: dy + ((dh - height) * 0.5).max(0.0),
            width,
            height,
            maximized: true,
            display_index: fallback_display,
        }
    }

    /// Record the main window placement for `displays`.
    pub fn record_window_placement(
        &mut self,
        displays: &[[f32; 4]],
        mut placement: WindowPlacement,
    ) {
        let sig = display_arrangement_signature(displays);
        placement.width = placement.width.max(MIN_WINDOW_WIDTH);
        placement.height = placement.height.max(MIN_WINDOW_HEIGHT);
        if !displays.is_empty() {
            placement.display_index = placement.display_index.min(displays.len() - 1);
        } else {
            placement.display_index = 0;
        }
        self.window_placements.insert(sig.clone(), placement);
        self.last_display_arrangement = Some(sig);
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
        self.outline_width = clamp_outline(self.outline_width);
        if let Some(tree) = &mut self.dock_tree {
            tree.normalize();
        }
        for tree in self.workspace_trees.values_mut() {
            tree.normalize();
        }
    }
}

/// `width` as an outline column can be: inside [`OUTLINE_MIN`] and
/// [`OUTLINE_MAX`], and the default for a width that is not a number.
pub fn clamp_outline(width: f32) -> f32 {
    if width.is_nan() {
        OUTLINE_DEFAULT
    } else {
        width.clamp(OUTLINE_MIN, OUTLINE_MAX)
    }
}

/// `~/.aria`, the data dir for UI settings.
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
/// assertion back in the reader, one layer further from the setting.
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
    /// intended directory.
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
            outline_width: 10.0,
            ..UiSettings::default()
        };
        s.clamp();
        assert_eq!(s.sidebar_width, SIDEBAR_MIN);
        assert_eq!(s.inspector_width, INSPECTOR_MAX);
        assert_eq!(s.timeline_height, TIMELINE_MIN_HEIGHT);
        assert_eq!(s.outline_width, OUTLINE_MIN);
    }

    /// The outline column is never narrower than a lane header's controls nor
    /// wider than its range, and a width that is not a number is the default
    /// rather than a column of no width. A file from before the setting opens
    /// at the default.
    #[test]
    fn the_outline_width_stays_in_its_range() {
        let cases = [
            (0.0, OUTLINE_MIN),
            (-1.0, OUTLINE_MIN),
            (OUTLINE_MIN - 0.5, OUTLINE_MIN),
            (OUTLINE_MIN, OUTLINE_MIN),
            (OUTLINE_DEFAULT, OUTLINE_DEFAULT),
            (OUTLINE_MAX, OUTLINE_MAX),
            (OUTLINE_MAX + 0.5, OUTLINE_MAX),
            (f32::INFINITY, OUTLINE_MAX),
            (f32::NEG_INFINITY, OUTLINE_MIN),
            (f32::NAN, OUTLINE_DEFAULT),
        ];
        for (width, wanted) in cases {
            assert_eq!(clamp_outline(width), wanted, "{width}");
        }
        assert!((OUTLINE_MIN..=OUTLINE_MAX).contains(&UiSettings::default().outline_width));
        let older = r#"{"sidebarWidth":300.0,"appearance":"dark"}"#;
        let parsed: UiSettings = serde_json::from_str(older).expect("older settings file");
        assert_eq!(parsed.outline_width, OUTLINE_DEFAULT);
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
    /// back that was removed.
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

    #[test]
    fn audio_scrubbing_defaults_to_on_for_older_files_and_round_trips() {
        let older = r#"{"sidebarWidth":300.0}"#;
        let parsed: UiSettings = serde_json::from_str(older).expect("older settings file");
        assert!(parsed.audio_scrubbing);

        let settings = UiSettings {
            audio_scrubbing: false,
            ..UiSettings::default()
        };
        let text = serde_json::to_string(&settings).expect("serialize");
        assert!(text.contains("\"audioScrubbing\":false"), "{text}");
        let back: UiSettings = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, settings);
    }

    #[test]
    fn the_waveform_mode_defaults_to_luma_for_older_files_and_round_trips() {
        let older = r#"{"sidebarWidth":300.0}"#;
        let parsed: UiSettings = serde_json::from_str(older).expect("older settings file");
        assert_eq!(parsed.waveform, crate::scopes::WaveformMode::Luma);

        for mode in crate::scopes::WaveformMode::ALL {
            // The saved spelling is a file format: a new mode names its own.
            let spelled = match mode {
                crate::scopes::WaveformMode::Luma => "luma",
                crate::scopes::WaveformMode::RgbParade => "rgbParade",
                crate::scopes::WaveformMode::YcbcrParade => "ycbcrParade",
            };
            let settings = UiSettings {
                waveform: mode,
                ..UiSettings::default()
            };
            let text = serde_json::to_string(&settings).expect("serialize");
            assert!(
                text.contains(&format!("\"waveform\":\"{spelled}\"")),
                "{text}"
            );
            let back: UiSettings = serde_json::from_str(&text).expect("deserialize");
            assert_eq!(back, settings);
        }
    }
}
