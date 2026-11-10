//! What a dropped path becomes.
//!
//! The app used to answer this itself with an extension test in two places: Home
//! knew about `.aria`, and everything else was imported as media, so a dropped LUT
//! or skill went to the media pool and failed there. The question now goes to the
//! plugins as a serial event — asked in mount order, first claim wins — so the
//! plugin that owns a file type is the one that recognizes it.
//!
//! Two answers are the host's, not a plugin's. A project file is the app's own
//! format, and losing one to a media import because the engine plugin did not mount
//! would be silent damage, so it is decided before anyone is asked. A file an
//! importer reads is a timeline: the importer registered its extensions, which is
//! that plugin's claim. The one exception is a grade claimed for the selection
//! while a clip is selected: it applies to that clip, and with none selected the
//! importer for its extension reads it. Nothing claiming the path means media:
//! import reports its own error for a file that is not media, which is the better
//! message anyway.
//!
//! A package waiting for `file:<ext>` loads before its path is classified, so
//! the claim or importer it registers decides the drop.

use std::path::{Path, PathBuf};

use aria_engine::EngineError;
use aria_models::PROJECT_FILE_EXTENSION;
use aria_plugin::{DropAction, ResolveDropped};

use super::AppState;

impl AppState {
    /// What this path should become. Media when no plugin claims it.
    pub fn classify_drop(&self, path: &Path) -> DropAction {
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case(PROJECT_FILE_EXTENSION))
        {
            return DropAction::OpenProject;
        }
        let event = ResolveDropped::path(path.to_path_buf());
        let claimed = self
            .ctx
            .events()
            .serial::<ResolveDropped, DropAction>(&event);
        if claimed == Some(DropAction::Handled) {
            return DropAction::Handled;
        }
        if claimed == Some(DropAction::ApplyLut) && self.selected_clip().is_some() {
            return DropAction::ApplyLut;
        }
        if self.ctx.importer_for(path).is_some() {
            return DropAction::ImportTimeline;
        }
        claimed.unwrap_or(DropAction::ImportMedia)
    }

    /// Ask the mounted plugins which one accepts `url`.
    pub fn classify_dropped_url(&self, url: &str) -> Option<DropAction> {
        let event = ResolveDropped::url(url);
        self.ctx
            .events()
            .serial::<ResolveDropped, DropAction>(&event)
    }

    /// Pass every dropped URL through [`ResolveDropped`]. An unclaimed URL
    /// posts `"No plugin accepts <url>"` to the status strip.
    pub fn handle_dropped_urls<I, S>(&mut self, urls: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for url in urls {
            let url = url.as_ref();
            if self.classify_dropped_url(url).is_none() {
                self.status_message = Some(format!("No plugin accepts {url}"));
            }
        }
    }

    /// Apply every dropped path according to what claimed it.
    ///
    /// Media is collected and placed as one chain, so dropping ten clips still
    /// lays them end to end. A project wins outright: opening one replaces the
    /// session, so anything dropped alongside it would be thrown away. Each
    /// timeline file is its own undo step, read before the media is placed. A
    /// plugin package goes to the install prompt, whatever else is dropped. A
    /// folder contributes the media files under it ([`folder_media`]) to the
    /// chain, in their place among the dropped paths.
    pub fn handle_dropped_paths(&mut self, paths: Vec<PathBuf>) -> Result<(), EngineError> {
        let paths = self.take_plugin_archives(paths);
        let paths = self.take_preset_files(paths);
        let mut media = Vec::new();
        let mut luts = Vec::new();
        let mut skills = Vec::new();
        let mut timelines = Vec::new();
        // The first dropped folder with no media file, and whether anything
        // other than a folder was dropped.
        let mut bare = None;
        let mut files = false;
        for path in paths {
            if path.is_dir() {
                let found = self.folder_media(&path);
                if found.is_empty() && bare.is_none() {
                    bare = Some(path);
                }
                media.extend(found);
                continue;
            }
            files = true;
            self.load_plugins_for_file(&path);
            match self.classify_drop(&path) {
                DropAction::OpenProject => return self.open_project(path),
                DropAction::ImportMedia => media.push(path),
                DropAction::ApplyLut => luts.push(path),
                DropAction::InstallSkill => skills.push(path),
                DropAction::ImportTimeline => timelines.push(path),
                DropAction::Handled => {}
            }
        }
        // The drop's own error line is written after the media import, because a
        // placement that lands clears the line. The later problem wins, and a
        // file the import rejects wins over all of them.
        let mut line = None;
        if let Some(folder) = bare {
            // Nothing else to do: the drop fails, so a drop on Home stays there.
            if !files && media.is_empty() {
                return Err(EngineError::Refused(no_media_copy(&folder)));
            }
            line = Some(DropLine::Bare(folder));
        }
        for path in skills {
            if let Err(err) = self.install_skill_file(&path) {
                self.status_message = Some(err);
            }
        }
        for path in timelines {
            if !self.import_timeline(&path)
                && let Some(reported) = self.last_error.take()
            {
                line = Some(DropLine::Timeline(reported));
            }
        }
        // A LUT applies to the selection. Dropping multiple LUTs is rejected as a
        // conflict to avoid ambiguous state or multiple engine commands.
        if luts.len() > 1 {
            line = Some(DropLine::Luts(luts.len()));
        } else if let Some(path) = luts.into_iter().next() {
            if self.selected_clip().is_none() {
                line = Some(DropLine::NoSelection(path));
            } else {
                self.set_selected_lut(path.to_string_lossy().to_string())?;
            }
        }
        if !media.is_empty() {
            self.import_dropped_paths(media)?;
            if self.last_error.is_some() {
                return Ok(());
            }
        }
        match line {
            None => {}
            Some(DropLine::Bare(folder)) => self.report_error(no_media_copy(&folder)),
            Some(DropLine::Timeline(reported)) => self.report_error(reported),
            Some(DropLine::Luts(count)) => self.report_error(format!(
                "Cannot apply {count} LUTs at once. Drop a single LUT file onto the selection."
            )),
            Some(DropLine::NoSelection(path)) => {
                self.report_error(format!("Select a clip to apply {}", path.display()))
            }
        }
        Ok(())
    }

    /// Open paths passed on the command line at startup.
    ///
    /// A failure keeps the session on Home, writes the refusal to the status
    /// strip through [`Self::report_error`], and prints it to standard error.
    pub fn open_cli_paths(&mut self, paths: Vec<PathBuf>) {
        let was_home = self.route == crate::types::UiRoute::Home;
        if !paths.is_empty()
            && let Err(err) = self.drop_on_home(paths)
        {
            if was_home && self.route != crate::types::UiRoute::Home {
                let _ = self.empty_session();
                self.go_home();
            }
            self.report_error(&err);
            tracing::error!(error = %err, "could not open the paths given on the command line");
            eprintln!("aria could not open what you asked for: {err}");
        }
    }

    /// The media files under a dropped `folder`, sorted by path: every file
    /// whose extension the import dialog offers, at most [`RELINK_MAX_DEPTH`]
    /// levels down. A file or folder whose name starts with a dot is skipped: a
    /// card read on macOS holds a `._C0001.MP4` metadata file beside each clip.
    /// A package waiting for one of the folder's file types loads first, so
    /// the decoder it registers is offered.
    ///
    /// [`RELINK_MAX_DEPTH`]: super::relink::RELINK_MAX_DEPTH
    fn folder_media(&mut self, folder: &Path) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = super::relink::walk(folder)
            .into_iter()
            .filter(|path| {
                path.strip_prefix(folder).is_ok_and(|inner| {
                    inner
                        .components()
                        .all(|part| !part.as_os_str().as_encoded_bytes().starts_with(b"."))
                })
            })
            .collect();
        let mut types = std::collections::BTreeSet::new();
        for path in &found {
            if let Some(ext) = path.extension()
                && types.insert(ext.to_ascii_lowercase())
            {
                self.load_plugins_for_file(path);
            }
        }
        let extensions = super::media::import::media_import_extensions();
        found.retain(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    extensions
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(ext))
                })
        });
        found.sort_unstable();
        found
    }
}

/// The error line for a dropped folder with no media file under it.
fn no_media_copy(folder: &Path) -> String {
    let name = folder.file_name().map_or_else(
        || folder.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    format!("No media in {name}. Drop a folder that holds video, audio, or still files.")
}

/// A problem with one part of a drop, written to the error line once the rest
/// of the drop has landed.
enum DropLine {
    /// A dropped folder with no media file under it.
    Bare(PathBuf),
    /// A timeline file that did not import, with the line its import wrote.
    Timeline(String),
    /// Several LUT files at once.
    Luts(usize),
    /// A LUT file with no clip selected to apply it to.
    NoSelection(PathBuf),
}

#[cfg(test)]
#[path = "drops_folder_tests.rs"]
mod folder_tests;
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::premiere_tests::with_clip;

    /// The conflict line of a drop with several LUT files stays on the error
    /// line when a file beside them lands: a placement that lands clears the
    /// line, so the drop writes it after the import. The file beside them is
    /// one the pool lacks: [`with_clip`] pools `texture.png`, and a path
    /// already pooled imports as the entry it already has.
    #[test]
    fn dropping_multiple_luts_reports_conflict() {
        let tone = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/fixture-sine-48k-2s.wav");
        for (beside, landed) in [(None, 0), (Some(tone), 1)] {
            let (mut state, _) = with_clip();
            let _claim = state
                .ctx
                .on_serial::<ResolveDropped, DropAction, _>(|event| {
                    event
                        .item
                        .as_path()
                        .and_then(|path| path.extension())
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("cube"))
                        .then_some(DropAction::ApplyLut)
                });
            let pooled = state.store.media.len();
            let mut paths = vec![PathBuf::from("first.cube"), PathBuf::from("second.cube")];
            paths.extend(beside);
            state.handle_dropped_paths(paths).expect("the drop applies");
            assert_eq!(
                state.last_error.as_deref(),
                Some("Cannot apply 2 LUTs at once. Drop a single LUT file onto the selection."),
                "{landed} file beside the LUTs"
            );
            assert_eq!(state.store.media.len(), pooled + landed);
        }
    }
}

#[cfg(test)]
#[path = "lut_import_tests.rs"]
mod lut_import_tests;
