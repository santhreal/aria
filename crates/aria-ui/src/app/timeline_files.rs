//! Timeline files: another editor's cut read into the open project, and the
//! open timeline written as one, through the importers and exporters plugins
//! register.
//!
//! No format is named here. The dialogs offer what the registry holds, a picked
//! or dropped file goes to the importer registered for its extension, and an
//! export goes to the exporter whose extension the destination has.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_plugin::{ExporterSpec, InterchangeReport};

use super::AppState;
use super::picker::{Ask, FileSpec};

/// The reason an import or export cannot start when no plugin registered one.
const NO_INTERCHANGE: &str = "Enable the Timeline interchange plugin in Settings > Plugins, or install a plugin that reads or writes this format.";

/// What the last timeline import or export could not carry as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineReport {
    /// What ran, such as `Imported cut.edl`.
    pub title: gpui::SharedString,
    /// One line per item placed or written the nearest way, in the order met.
    pub lines: Vec<gpui::SharedString>,
}

impl AppState {
    /// Ask for a timeline file and read it into the open project.
    pub fn pick_and_import_timeline(&mut self) {
        let importers = self.ctx.importers();
        if importers.is_empty() {
            self.report_error(format!("No timeline importer is mounted. {NO_INTERCHANGE}"));
            return;
        }
        let every: Vec<&str> = importers
            .iter()
            .flat_map(|importer| importer.extensions.iter().map(String::as_str))
            .collect();
        let mut spec = FileSpec::new("Import Timeline").filter("Timeline files", &every);
        for importer in &importers {
            let extensions: Vec<&str> = importer.extensions.iter().map(String::as_str).collect();
            spec = spec.filter(importer.name.as_str(), &extensions);
        }
        if let Some(dir) = self.project_path.as_deref().and_then(Path::parent) {
            spec = spec.directory(dir);
        }
        self.request_pick(Ask::OpenFile(spec), |state, answer| {
            if let Some(path) = answer.one() {
                state.import_timeline(&path);
            }
        });
    }

    /// Read `path` into the open project through the importer registered for
    /// its extension, as one undo step. Returns whether it was imported; when
    /// it was not, the project is unchanged and the error is reported.
    pub fn import_timeline(&mut self, path: &Path) -> bool {
        self.load_plugins_for_file(path);
        let imported = self.ctx.import_file(self.host(), path);
        self.sync_from_engine();
        match imported {
            Ok((report, _)) => {
                self.clear_error();
                self.republish_audio();
                self.finish_timeline(format!("Imported {}", file_name(path)), report);
                true
            }
            Err(err) => {
                self.report_error(err.to_string());
                false
            }
        }
    }

    /// Ask where to write the open timeline. The format is the one whose
    /// extension the name given has.
    pub fn pick_and_export_timeline(&mut self) {
        self.ask_timeline_path(None);
    }

    /// Ask where to write the open timeline in one exporter's format only.
    pub fn pick_and_export_timeline_as(&mut self, exporter: &str) {
        self.ask_timeline_path(Some(exporter));
    }

    fn ask_timeline_path(&mut self, only: Option<&str>) {
        let exporters: Vec<Arc<ExporterSpec>> = self
            .ctx
            .exporters()
            .into_iter()
            .filter(|exporter| only.is_none_or(|id| exporter.id == id))
            .collect();
        let Some(first) = exporters.first() else {
            self.report_error(match only {
                Some(id) => format!("The timeline exporter {id} is not mounted. {NO_INTERCHANGE}"),
                None => format!("No timeline exporter is mounted. {NO_INTERCHANGE}"),
            });
            return;
        };
        let name = format!("{}.{}", self.host().snapshot().name, first.extension);
        let mut spec = FileSpec::new("Export Timeline").file_name(name);
        for exporter in &exporters {
            spec = spec.filter(exporter.name.as_str(), &[exporter.extension.as_str()]);
        }
        if let Some(dir) = self.project_path.as_deref().and_then(Path::parent) {
            spec = spec.directory(dir);
        }
        let forced = only.map(|_| Arc::clone(first));
        self.request_pick(Ask::SaveFile(spec), move |state, answer| {
            let Some(path) = answer.one() else {
                return;
            };
            match &forced {
                // One format was asked for: the file takes its extension
                // whatever name was typed.
                Some(exporter) => {
                    let path = if path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case(&exporter.extension))
                    {
                        path
                    } else {
                        path.with_extension(&exporter.extension)
                    };
                    state.write_timeline(exporter, &path);
                }
                None => {
                    state.export_timeline(&path);
                }
            }
        });
    }

    /// Write the open timeline to `path` through the exporter for its
    /// extension; a name with no extension takes the first exporter's. Returns
    /// whether it was written; when it was not, `path` is unchanged and the
    /// error is reported.
    pub fn export_timeline(&mut self, path: &Path) -> bool {
        let (exporter, path) = match exporter_for(&self.ctx.exporters(), path) {
            Ok(found) => found,
            Err(reason) => {
                self.report_error(reason);
                return false;
            }
        };
        self.write_timeline(&exporter, &path)
    }

    /// Write the open timeline to `path` through `exporter`.
    fn write_timeline(&mut self, exporter: &ExporterSpec, path: &Path) -> bool {
        let project = self.host().snapshot();
        match self.ctx.export_file(&exporter.id, &project, path) {
            Ok(report) => {
                self.clear_error();
                self.finish_timeline(format!("Exported {}", file_name(path)), report);
                true
            }
            Err(err) => {
                self.report_error(err.to_string());
                false
            }
        }
    }

    /// Close the report of the last import or export.
    pub fn dismiss_timeline_report(&mut self) {
        self.timeline_report = None;
    }

    /// Show `title` on the status line, and keep the report while it holds
    /// anything. A clean run replaces an earlier report rather than leaving it
    /// up beside a timeline it no longer describes.
    fn finish_timeline(&mut self, title: String, report: InterchangeReport) {
        self.status_message = Some(match report.skipped.len() {
            0 => title.clone(),
            1 => format!("{title}: 1 item in the Timeline file panel"),
            count => format!("{title}: {count} items in the Timeline file panel"),
        });
        self.timeline_report = (!report.skipped.is_empty()).then(|| TimelineReport {
            title: title.into(),
            lines: report.skipped.into_iter().map(Into::into).collect(),
        });
    }
}

/// The exporter that writes `path`, and the path it writes.
fn exporter_for(
    exporters: &[Arc<ExporterSpec>],
    path: &Path,
) -> Result<(Arc<ExporterSpec>, PathBuf), String> {
    let Some(first) = exporters.first() else {
        return Err(format!("No timeline exporter is mounted. {NO_INTERCHANGE}"));
    };
    let Some(extension) = path.extension() else {
        return Ok((Arc::clone(first), path.with_extension(&first.extension)));
    };
    let extension = extension.to_string_lossy();
    if let Some(exporter) = exporters
        .iter()
        .find(|exporter| exporter.extension.eq_ignore_ascii_case(&extension))
    {
        return Ok((Arc::clone(exporter), path.to_owned()));
    }
    let written: Vec<String> = exporters
        .iter()
        .map(|exporter| format!(".{}", exporter.extension))
        .collect();
    Err(format!(
        "No timeline exporter writes .{extension} files. Name the file with one of {}.",
        written.join(", ")
    ))
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
#[path = "timeline_files_tests.rs"]
mod tests;
