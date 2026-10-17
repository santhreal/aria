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
    /// plugin package goes to the install prompt, whatever else is dropped.
    pub fn handle_dropped_paths(&mut self, paths: Vec<PathBuf>) -> Result<(), EngineError> {
        let paths = self.take_plugin_archives(paths);
        let paths = self.take_preset_files(paths);
        let mut media = Vec::new();
        let mut luts = Vec::new();
        let mut skills = Vec::new();
        let mut timelines = Vec::new();
        for path in paths {
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
        for path in skills {
            if let Err(err) = self.install_skill_file(&path) {
                self.status_message = Some(err);
            }
        }
        for path in timelines {
            self.import_timeline(&path);
        }
        // A LUT applies to the selection. Dropping multiple LUTs is rejected as a
        // conflict to avoid ambiguous state or multiple engine commands.
        if luts.len() > 1 {
            self.report_error(format!(
                "Cannot apply {} LUTs at once. Drop a single LUT file onto the selection.",
                luts.len()
            ));
        } else if let Some(path) = luts.into_iter().next() {
            if self.selected_clip().is_none() {
                self.report_error(format!("Select a clip to apply {}.", path.display()));
            } else {
                self.set_selected_lut(path.to_string_lossy().to_string())?;
            }
        }
        if !media.is_empty() {
            self.import_dropped_paths(media)?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::premiere_tests::with_clip;

    #[test]
    fn dropping_multiple_luts_reports_conflict() {
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
        let lut1 = PathBuf::from("first.cube");
        let lut2 = PathBuf::from("second.cube");
        let res = state.handle_dropped_paths(vec![lut1, lut2]);
        assert!(res.is_ok());
        assert!(state.last_error.is_some());
        assert!(
            state
                .last_error
                .as_ref()
                .unwrap()
                .contains("Cannot apply 2 LUTs")
        );
    }
}

#[cfg(test)]
#[path = "lut_import_tests.rs"]
mod lut_import_tests;
