//! What an export is, before anything renders it.
//!
//! # Why
//!
//! The Export button read four things off the state inside its own click
//! closure and handed them straight to a job that decodes, composites on the
//! GPU and encodes a file. Nothing in between was a value, so nothing could be
//! asserted: a button wired to the wrong destination, the wrong scale or the
//! wrong span would have turned nothing red, because the only way to observe
//! the wiring was to render.
//!
//! A request is that value. It takes no window, holds no lock, asks no encoder
//! and starts nothing, so a test presses the sheet's rows and reads what the
//! button would send. Resolving it against this machine — the encoder that has
//! to exist, the codec and bitrate it will honour, a span with frames in it —
//! stays with the queue, which is the step that spends the machine.

use std::path::PathBuf;

use aria_core::ExportFormat;

use super::picker::{Ask, FileSpec};
use super::{AppState, DialogKind};

/// One export, as the sheet's rows describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    /// Where the file is written.
    pub destination: PathBuf,
    /// Every choice the sheet offers, in the form the renderer takes them.
    pub format: ExportFormat,
    /// The size the file will have, after the scale and the even rule. Carried
    /// beside the format because the format holds what was asked for and this
    /// holds what an encoder can be given.
    pub size: (u32, u32),
    /// The span, worded as the sheet and the status line word it.
    pub range: String,
    /// Registered plugin encoder id, or `None` for the platform encoder.
    pub plugin_encoder: Option<String>,
}

impl AppState {
    /// Where an export lands when no file is named: beside the
    /// project, or in the data directory when the project has never been saved.
    ///
    /// Here rather than in the sheet, so the path the sheet prints and the path
    /// the button writes to are one answer.
    pub fn export_destination(&self) -> PathBuf {
        let ext = self.export_extension();
        match self.project_path.as_ref() {
            Some(path) => path.with_extension(&ext),
            None => self
                .data_dir
                .join(format!("{}.{ext}", self.store.project_name)),
        }
    }

    /// The export the sheet is currently describing, addressed at `destination`.
    pub fn export_request(&self, destination: PathBuf) -> ExportRequest {
        ExportRequest {
            destination,
            format: self.export_format(),
            size: self.export_size(),
            range: self.export_range_label(),
            plugin_encoder: self.export_options.plugin_encoder.clone(),
        }
    }

    /// Queue the export the sheet is describing, at the destination it prints.
    ///
    /// The Export button's whole body, as a method rather than a click closure:
    /// the sheet's button and the sheet's Enter key are two ways to press the
    /// same control, and a body that lived in the closure could only be reached
    /// by the pointer. The destination is read here rather than passed in, so a
    /// project saved while the sheet was open exports beside the file it was
    /// saved as.
    pub fn export_to_destination(&mut self) {
        let destination = self.export_destination();
        match self.export_job(destination) {
            // The job owns the progress now, so the sheet closes and the Jobs
            // board reports the pass. A modal sheet over a background export
            // would block the editing the background pass exists to allow.
            Ok(_) => self.open_dialog(DialogKind::Jobs),
            Err(err) => self.report_error(err),
        }
    }

    /// Ask where to write the export the sheet is describing, then queue it
    /// there: the sheet's Choose button.
    ///
    /// The dialog offers the format's own extension and proposes the name the
    /// sheet prints, so a save that keeps the proposal writes where Export would.
    pub fn pick_export_destination(&mut self) {
        let ext = self.export_extension();
        let name = self
            .export_destination()
            .file_name()
            .and_then(|name| name.to_str())
            .map_or_else(|| format!("export.{ext}"), ToOwned::to_owned);
        let spec = FileSpec::new("Export")
            .filter(self.export_format_label(), &[ext.as_str()])
            .file_name(name);
        self.request_pick(Ask::SaveFile(spec), |state, answer| {
            let Some(path) = answer.one() else {
                return;
            };
            match state.export_job(path) {
                Ok(_) => state.open_dialog(DialogKind::Jobs),
                Err(err) => state.report_error(err),
            }
        });
    }
}
