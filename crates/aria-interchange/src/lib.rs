//! Timeline interchange: CMX 3600 EDL, Final Cut Pro XML and OpenTimelineIO,
//! and SubRip and WebVTT subtitles.
//!
//! Each timeline format reads a file into a [`Cut`] and writes a cut out.
//! [`import::place`] moves a cut into the open project, and [`project::cut`]
//! takes one out of it. Each subtitle format reads cues onto text tracks and
//! writes the text clips of the text tracks ([`subtitles`]).
//! [`InterchangePlugin`] registers an importer and an exporter for every
//! format.

pub mod edl;
pub mod fcpxml;
pub mod import;
mod model;
pub mod otio;
pub mod project;
pub mod subtitles;
pub mod timecode;
mod url;

use std::io::{self, Read as _, Write};
use std::path::Path;

use aria_models::{Project, Rational};
use aria_plugin::{
    Exporter, ExporterSpec, Importer, ImporterSpec, InterchangeReport, Plugin, PluginContext,
    PluginError, PluginManifest, TransactError, Transaction,
};

pub use model::{Cut, Event, Lane, LaneKind, Mark, Read, Source};
pub use subtitles::{Cue, Cues, SUBTITLE_FORMATS, SubtitleFormat};

/// The largest timeline file read. Past it, a file is not an edit list.
pub const MAX_FILE_BYTES: u64 = 256 << 20;

/// One format: what it is called, and how it reads and writes.
#[derive(Debug)]
pub struct Format {
    /// The importer and exporter id.
    pub id: &'static str,
    /// The format as a menu lists it.
    pub name: &'static str,
    /// The extension written; read with the others in `reads`.
    pub extension: &'static str,
    /// Every extension read.
    pub reads: &'static [&'static str],
    /// Read a file's text at a rate it may not state, titled `name` when it
    /// has no title of its own.
    pub read: fn(&str, Rational, &str) -> Result<Read, String>,
    /// Write a cut, returning what the format could not hold.
    pub write: fn(&Cut, &mut dyn Write) -> io::Result<Vec<String>>,
}

/// Every format, in the order menus list them.
pub const FORMATS: [Format; 3] = [
    Format {
        id: "aria.interchange.edl",
        name: "CMX 3600 EDL",
        extension: "edl",
        reads: &["edl"],
        read: edl::read,
        write: edl::write,
    },
    Format {
        id: "aria.interchange.fcpxml",
        name: "Final Cut Pro XML",
        extension: "fcpxml",
        reads: &["fcpxml"],
        read: fcpxml::read,
        write: fcpxml::write,
    },
    Format {
        id: "aria.interchange.otio",
        name: "OpenTimelineIO",
        extension: "otio",
        reads: &["otio"],
        read: otio::read,
        write: otio::write,
    },
];

/// Registers an importer and an exporter for every format in [`FORMATS`] and
/// [`SUBTITLE_FORMATS`].
#[derive(Debug, Default, Clone, Copy)]
pub struct InterchangePlugin;

static MANIFEST: PluginManifest = PluginManifest::new(
    "aria.interchange",
    "Timeline interchange",
    env!("CARGO_PKG_VERSION"),
    &[],
    &[],
);

impl Plugin for InterchangePlugin {
    fn manifest(&self) -> &PluginManifest {
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        for format in &FORMATS {
            ctx.register_importer(
                ImporterSpec {
                    id: format.id.to_owned(),
                    name: format.name.to_owned(),
                    extensions: format.reads.iter().map(|ext| (*ext).to_owned()).collect(),
                },
                FormatImporter(format),
            )?;
            ctx.register_exporter(
                ExporterSpec {
                    id: format.id.to_owned(),
                    name: format.name.to_owned(),
                    extension: format.extension.to_owned(),
                },
                FormatExporter(format),
            )?;
        }
        for format in &SUBTITLE_FORMATS {
            ctx.register_importer(
                ImporterSpec {
                    id: format.id.to_owned(),
                    name: format.name.to_owned(),
                    extensions: vec![format.extension.to_owned()],
                },
                SubtitleImporter(format),
            )?;
            ctx.register_exporter(
                ExporterSpec {
                    id: format.id.to_owned(),
                    name: format.name.to_owned(),
                    extension: format.extension.to_owned(),
                },
                SubtitleExporter(format),
            )?;
        }
        Ok(())
    }
}

struct FormatImporter(&'static Format);

impl Importer for FormatImporter {
    fn import(
        &self,
        file: &Path,
        tx: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError> {
        let text = read_text(file).map_err(|err| {
            TransactError::Aborted(format!("{} could not be read: {err}", file.display()))
        })?;
        let rate = tx.project().timeline().frame_rate;
        let name = model::file_stem(file, "Imported timeline");
        let Read { cut, mut skipped } = (self.0.read)(&text, rate, &name).map_err(|reason| {
            TransactError::Aborted(format!(
                "{} is not a {} file: {reason}",
                file.display(),
                self.0.name
            ))
        })?;
        let base = file.parent().unwrap_or_else(|| Path::new(""));
        skipped.extend(import::place(&cut, base, tx)?);
        Ok(InterchangeReport { skipped })
    }
}

struct FormatExporter(&'static Format);

impl Exporter for FormatExporter {
    fn export(&self, project: &Project, out: &mut dyn Write) -> io::Result<InterchangeReport> {
        let Read { cut, mut skipped } = project::cut(project);
        skipped.extend((self.0.write)(&cut, out)?);
        Ok(InterchangeReport { skipped })
    }
}

struct SubtitleImporter(&'static SubtitleFormat);

impl Importer for SubtitleImporter {
    fn import(
        &self,
        file: &Path,
        tx: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError> {
        let text = read_text(file).map_err(|err| {
            TransactError::Aborted(format!("{} could not be read: {err}", file.display()))
        })?;
        let Cues { cues, skipped } = (self.0.read)(&text).map_err(|reason| {
            TransactError::Aborted(format!(
                "{} is not a {} file: {reason}",
                file.display(),
                self.0.name
            ))
        })?;
        subtitles::place(&cues, &model::file_stem(file, "Subtitles"), tx)?;
        Ok(InterchangeReport { skipped })
    }
}

struct SubtitleExporter(&'static SubtitleFormat);

impl Exporter for SubtitleExporter {
    fn export(&self, project: &Project, out: &mut dyn Write) -> io::Result<InterchangeReport> {
        let Cues { cues, mut skipped } = subtitles::cues(project);
        if cues.is_empty() {
            return Err(io::Error::other(format!(
                "{} has no text clip on a shown text track to write as subtitles",
                project.name
            )));
        }
        skipped.extend((self.0.write)(&cues, out)?);
        Ok(InterchangeReport { skipped })
    }
}

/// The text of `file`, at most [`MAX_FILE_BYTES`] of it, without a byte-order
/// mark. Bytes that are not UTF-8 read as replacement characters, so a
/// Latin-1 EDL still reads.
fn read_text(file: &Path) -> io::Result<String> {
    read_at_most(file, MAX_FILE_BYTES)
}

/// [`read_text`] with `limit` in place of [`MAX_FILE_BYTES`].
fn read_at_most(file: &Path, limit: u64) -> io::Result<String> {
    let too_large = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("timeline files over {} are not read", size(limit)),
        )
    };
    let file = std::fs::File::open(file)?;
    if file.metadata()?.len() > limit {
        return Err(too_large());
    }
    // A pipe or device states no length; the read stops one byte past the limit.
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(too_large());
    }
    let mut text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
    };
    if text.starts_with('\u{feff}') {
        text.drain(..'\u{feff}'.len_utf8());
    }
    Ok(text)
}

/// `bytes` in MiB when a whole number of them, else in bytes.
fn size(bytes: u64) -> String {
    if bytes % (1 << 20) == 0 {
        format!("{} MiB", bytes >> 20)
    } else {
        format!("{bytes} bytes")
    }
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
