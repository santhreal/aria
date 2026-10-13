//! Importers and exporters: plugins that read another editor's timeline file
//! into the open project, and write the open timeline as one.
//!
//! An import runs as one [`ProjectHost::transact_with`] transaction named
//! `Import <file>`: one undo step, and nothing of it remains when the importer
//! returns an error or panics. An export writes beside the destination and
//! renames over it once the exporter returns, so a failed export leaves the
//! destination as it was. A panic in either fails its plugin for the session.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use aria_models::Project;
use parking_lot::RwLock;

use crate::context::PluginContext;
use crate::error::PluginError;
use crate::events::panic_message;
use crate::project_host::{Origin, ProjectHost, Receipt, TransactError, Transaction};
use crate::registration::Registration;

/// What an importer reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImporterSpec {
    /// Unique among importers.
    pub id: String,
    /// The format as a menu lists it, such as `CMX 3600 EDL`.
    pub name: String,
    /// The file extensions it reads, without the dot. Matched without regard
    /// to case, and stored lowercase.
    pub extensions: Vec<String>,
}

/// What an exporter writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExporterSpec {
    /// Unique among exporters.
    pub id: String,
    /// The format as a menu lists it.
    pub name: String,
    /// The extension of the file it writes, without the dot. Stored lowercase.
    pub extension: String,
}

/// What a file held, or the project holds, that the format could not carry:
/// one line per item, in the order met.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InterchangeReport {
    pub skipped: Vec<String>,
}

/// Reads a timeline file into the open project.
pub trait Importer: Send + Sync {
    /// Read `file` into the project through `tx`. Every command applied
    /// through `tx` lands as one undo step; an error or a panic leaves the
    /// project as it was.
    fn import(
        &self,
        file: &Path,
        tx: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError>;
}

/// Writes the open timeline as a file.
pub trait Exporter: Send + Sync {
    /// Write the open timeline of `project` to `out`. An error is the write
    /// failure, or the reason the format cannot hold the timeline.
    fn export(&self, project: &Project, out: &mut dyn Write) -> std::io::Result<InterchangeReport>;
}

/// Why an import or an export did not complete.
#[derive(Debug, thiserror::Error)]
pub enum InterchangeError {
    #[error("no importer reads {}", .0.display())]
    NoImporter(PathBuf),
    #[error("no exporter is registered as {0}")]
    NoExporter(String),
    /// Nothing of the import remains.
    #[error("{importer} could not import {}: {source}", path.display())]
    Import {
        importer: String,
        path: PathBuf,
        #[source]
        source: TransactError,
    },
    /// The destination is unchanged.
    #[error("{exporter} could not export {}: {source}", path.display())]
    Export {
        exporter: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The destination is unchanged, and the exporter's plugin failed.
    #[error("{exporter} panicked exporting {}: {message}", path.display())]
    ExportPanicked {
        exporter: String,
        path: PathBuf,
        message: String,
    },
}

/// One registered importer or exporter.
struct Entry<S, H: ?Sized> {
    spec: Arc<S>,
    /// The plugin that registered it, or `None` for the host.
    plugin: Option<&'static str>,
    handler: Arc<H>,
}

impl<S, H: ?Sized> Entry<S, H> {
    /// A copy to call through after the registry lock is released.
    fn share(&self) -> Self {
        Self {
            spec: Arc::clone(&self.spec),
            plugin: self.plugin,
            handler: Arc::clone(&self.handler),
        }
    }
}

/// The importers and exporters registered on one context, in registration
/// order.
#[derive(Default)]
pub(crate) struct Interchange {
    importers: RwLock<Vec<Entry<ImporterSpec, dyn Importer>>>,
    exporters: RwLock<Vec<Entry<ExporterSpec, dyn Exporter>>>,
}

impl Interchange {
    /// The first registered importer that lists the extension of `path`.
    fn importer(&self, path: &Path) -> Option<Entry<ImporterSpec, dyn Importer>> {
        let extension = extension_of(path)?;
        self.importers
            .read()
            .iter()
            .find(|entry| entry.spec.extensions.contains(&extension))
            .map(Entry::share)
    }

    /// The exporter registered as `id`.
    fn exporter(&self, id: &str) -> Option<Entry<ExporterSpec, dyn Exporter>> {
        self.exporters
            .read()
            .iter()
            .find(|entry| entry.spec.id == id)
            .map(Entry::share)
    }
}

#[derive(Clone, Copy)]
enum Side {
    Importer,
    Exporter,
}

/// Removes one entry when its plugin's registration drops.
struct Unregister {
    registry: Weak<Interchange>,
    side: Side,
    id: String,
}

impl Drop for Unregister {
    fn drop(&mut self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        // The handlers drop after the lock is released, so their drop code
        // may read the registry.
        match self.side {
            Side::Importer => {
                let removed: Vec<_> = registry
                    .importers
                    .write()
                    .extract_if(.., |entry| entry.spec.id == self.id)
                    .collect();
                drop(removed);
            }
            Side::Exporter => {
                let removed: Vec<_> = registry
                    .exporters
                    .write()
                    .extract_if(.., |entry| entry.spec.id == self.id)
                    .collect();
                drop(removed);
            }
        }
    }
}

fn refused(reason: String) -> PluginError {
    PluginError::InterchangeRefused(reason)
}

/// `raw` as a stored extension: ASCII letters and digits, lowercase.
fn extension(id: &str, raw: &str) -> Result<String, PluginError> {
    if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(refused(format!(
            "{id}: the extension {raw:?} is not letters and digits without a dot"
        )));
    }
    Ok(raw.to_ascii_lowercase())
}

/// The lowercase extension of `path`, if it has one.
fn extension_of(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
}

/// Where an export writes before it renames over `path`.
fn partial_path(path: &Path) -> std::io::Result<PathBuf> {
    let name = path.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "the path names no file")
    })?;
    let mut partial = std::ffi::OsString::from(".");
    partial.push(name);
    partial.push(".partial");
    Ok(path.with_file_name(partial))
}

/// Write through `handler` to `partial`, flushed and synced.
fn write_partial(
    handler: &dyn Exporter,
    project: &Project,
    partial: &Path,
) -> std::thread::Result<std::io::Result<InterchangeReport>> {
    let file = match File::create(partial) {
        Ok(file) => file,
        Err(err) => return Ok(Err(err)),
    };
    let mut out = BufWriter::new(file);
    let written = catch_unwind(AssertUnwindSafe(|| handler.export(project, &mut out)))?;
    Ok(written.and_then(|report| {
        let file = out
            .into_inner()
            .map_err(std::io::IntoInnerError::into_error)?;
        file.sync_all()?;
        Ok(report)
    }))
}

impl PluginContext {
    /// Add an importer. File > Import and a drop on the window route a file
    /// whose extension `spec` lists to `importer`; when two importers list
    /// one extension, the one registered first reads it.
    ///
    /// Held on the terms of [`register_effect`](Self::register_effect).
    /// Refused when the id is empty or taken, or an extension is empty or
    /// holds anything but ASCII letters and digits.
    pub fn register_importer(
        &self,
        spec: ImporterSpec,
        importer: impl Importer + 'static,
    ) -> Result<Registration, PluginError> {
        let ImporterSpec {
            id,
            name,
            extensions,
        } = spec;
        if id.is_empty() {
            return Err(refused("an importer needs an id".into()));
        }
        if extensions.is_empty() {
            return Err(refused(format!("{id}: an importer reads no extension")));
        }
        let extensions = extensions
            .iter()
            .map(|raw| extension(&id, raw))
            .collect::<Result<Vec<_>, _>>()?;
        let registry = self.interchange();
        {
            let mut importers = registry.importers.write();
            if importers.iter().any(|entry| entry.spec.id == id) {
                return Err(refused(format!("an importer is registered as {id}")));
            }
            importers.push(Entry {
                spec: Arc::new(ImporterSpec {
                    id: id.clone(),
                    name,
                    extensions,
                }),
                plugin: self.plugin_scope(),
                handler: Arc::new(importer),
            });
        }
        Ok(self.retain_if_mounting(Registration::holding(Unregister {
            registry: Arc::downgrade(registry),
            side: Side::Importer,
            id,
        })))
    }

    /// Add an exporter, listed in the File menu and the export dialog. Held
    /// and refused on the terms of [`register_importer`](Self::register_importer).
    pub fn register_exporter(
        &self,
        spec: ExporterSpec,
        exporter: impl Exporter + 'static,
    ) -> Result<Registration, PluginError> {
        let ExporterSpec {
            id,
            name,
            extension: raw,
        } = spec;
        if id.is_empty() {
            return Err(refused("an exporter needs an id".into()));
        }
        let extension = extension(&id, &raw)?;
        let registry = self.interchange();
        {
            let mut exporters = registry.exporters.write();
            if exporters.iter().any(|entry| entry.spec.id == id) {
                return Err(refused(format!("an exporter is registered as {id}")));
            }
            exporters.push(Entry {
                spec: Arc::new(ExporterSpec {
                    id: id.clone(),
                    name,
                    extension,
                }),
                plugin: self.plugin_scope(),
                handler: Arc::new(exporter),
            });
        }
        Ok(self.retain_if_mounting(Registration::holding(Unregister {
            registry: Arc::downgrade(registry),
            side: Side::Exporter,
            id,
        })))
    }

    /// Every registered importer, in registration order.
    pub fn importers(&self) -> Vec<Arc<ImporterSpec>> {
        self.interchange()
            .importers
            .read()
            .iter()
            .map(|entry| Arc::clone(&entry.spec))
            .collect()
    }

    /// Every registered exporter, in registration order.
    pub fn exporters(&self) -> Vec<Arc<ExporterSpec>> {
        self.interchange()
            .exporters
            .read()
            .iter()
            .map(|entry| Arc::clone(&entry.spec))
            .collect()
    }

    /// The importer that reads `path`, by its extension.
    pub fn importer_for(&self, path: &Path) -> Option<Arc<ImporterSpec>> {
        self.interchange().importer(path).map(|entry| entry.spec)
    }

    /// Read `path` into the open project through the importer for its
    /// extension, as one undo step named `Import <file name>`.
    ///
    /// On an error nothing of the import remains. An importer that panics
    /// also fails its plugin for the session.
    pub fn import_file(
        &self,
        host: &ProjectHost,
        path: &Path,
    ) -> Result<(InterchangeReport, Receipt), InterchangeError> {
        let Some(Entry {
            spec,
            plugin,
            handler,
        }) = self.interchange().importer(path)
        else {
            return Err(InterchangeError::NoImporter(path.to_owned()));
        };
        let origin = plugin.map_or(Origin::Editor, Origin::plugin);
        let file = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        host.transact_with(origin, format!("Import {file}"), |tx| {
            handler.import(path, tx)
        })
        .map_err(|source| {
            if let (TransactError::Panicked(message), Some(plugin)) = (&source, plugin) {
                self.fail(plugin, format!("importer {} panicked: {message}", spec.id));
            }
            InterchangeError::Import {
                importer: spec.id.clone(),
                path: path.to_owned(),
                source,
            }
        })
    }

    /// Write the open timeline of `project` to `path` through the exporter
    /// registered as `exporter`.
    ///
    /// The exporter writes to a hidden file beside `path`, which is renamed
    /// over `path` only once the exporter returns and the bytes are synced:
    /// on an error or a panic `path` is unchanged and the hidden file is
    /// removed. An exporter that panics also fails its plugin for the session.
    pub fn export_file(
        &self,
        exporter: &str,
        project: &Project,
        path: &Path,
    ) -> Result<InterchangeReport, InterchangeError> {
        let Some(Entry {
            spec,
            plugin,
            handler,
        }) = self.interchange().exporter(exporter)
        else {
            return Err(InterchangeError::NoExporter(exporter.to_owned()));
        };
        let failed = |source| InterchangeError::Export {
            exporter: spec.id.clone(),
            path: path.to_owned(),
            source,
        };
        let partial = partial_path(path).map_err(failed)?;
        let exported = match write_partial(handler.as_ref(), project, &partial) {
            Ok(written) => written
                .and_then(|report| std::fs::rename(&partial, path).map(|()| report))
                .map_err(failed),
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                if let Some(plugin) = plugin {
                    self.fail(plugin, format!("exporter {} panicked: {message}", spec.id));
                }
                Err(InterchangeError::ExportPanicked {
                    exporter: spec.id.clone(),
                    path: path.to_owned(),
                    message,
                })
            }
        };
        if exported.is_err() {
            // Absent when the create failed; nothing else can be done here.
            let _ = std::fs::remove_file(&partial);
        }
        exported
    }
}

#[cfg(test)]
#[path = "interchange_tests.rs"]
mod tests;
