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

use aria_core::{
    EncodeError, EncoderConfig, FrameData, PixelFormat, RationalTime, VideoEncoder, VideoFrame,
};
use aria_models::Project;
use parking_lot::RwLock;

use crate::context::PluginContext;
use crate::error::PluginError;
use crate::events::panic_message;
use crate::project_host::{Origin, ProjectHost, Receipt, TransactError, Transaction};
use crate::registration::{Contribution, ContributionKind, Registration};

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

/// What a media encoder writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderSpec {
    /// Unique among encoders.
    pub id: String,
    /// The format as the Export dialog lists it, such as `Apple ProRes 422`.
    pub name: String,
    /// The extension of the file it writes, without the dot. Stored lowercase.
    pub extension: String,
    /// Whether the encoder accepts audio blocks via [`VideoEncoder::push_audio`].
    pub audio: bool,
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

/// Opens a [`VideoEncoder`] session writing an encoded media container to a
/// destination path.
pub trait Encoder: Send + Sync {
    /// Open an encoder session writing `config` to `path`.
    fn open(
        &self,
        path: &Path,
        config: &EncoderConfig,
    ) -> Result<Box<dyn VideoEncoder>, EncodeError>;
}

/// One export frame as a plugin encoder receives it: straight-alpha RGBA8
/// rows of `stride` bytes, and the presentation time as `pts.value` ticks at
/// `pts.rate.num` ticks per second (`pts.rate.den` is 1).
#[derive(Debug, Clone, Copy)]
pub struct EncoderFrame<'a> {
    pub rgba: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pts: RationalTime,
}

impl<'a> EncoderFrame<'a> {
    /// View `frame`, which must be a CPU [`PixelFormat::Rgba8`] frame.
    pub fn new(frame: &'a VideoFrame) -> Result<Self, EncodeError> {
        if frame.format != PixelFormat::Rgba8 {
            return Err(EncodeError::Unsupported(format!(
                "plugin encoder expects Rgba8 frame, got {:?}",
                frame.format
            )));
        }
        let FrameData::Cpu(cpu) = &frame.data else {
            return Err(EncodeError::Unsupported(
                "plugin encoder requires CPU RGBA frame".to_owned(),
            ));
        };
        let Some(plane) = cpu.planes.first() else {
            return Err(EncodeError::Encode(
                "RGBA frame has no pixel plane".to_owned(),
            ));
        };
        let stride = u32::try_from(plane.stride).map_err(|_| {
            EncodeError::Encode(format!("RGBA row of {} bytes exceeds u32", plane.stride))
        })?;
        let (width, height) = frame.coded_size;
        Ok(Self {
            rgba: &plane.data,
            width,
            height,
            stride,
            pts: encoder_pts(frame.pts)?,
        })
    }
}

/// `pts` at the whole tick rate a plugin encoder reads: see
/// [`RationalTime::at_whole_rate`].
pub fn encoder_pts(pts: RationalTime) -> Result<RationalTime, EncodeError> {
    pts.at_whole_rate().map_err(|err| {
        EncodeError::Encode(format!(
            "presentation time {} at {}/{}: {err}",
            pts.value, pts.rate.num, pts.rate.den
        ))
    })
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
    plugin: Option<std::borrow::Cow<'static, str>>,
    handler: Arc<H>,
}

impl<S, H: ?Sized> Entry<S, H> {
    /// A copy to call through after the registry lock is released.
    fn share(&self) -> Self {
        Self {
            spec: Arc::clone(&self.spec),
            plugin: self.plugin.clone(),
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
    encoders: RwLock<Vec<Entry<EncoderSpec, dyn Encoder>>>,
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

    /// The encoder registered as `id`.
    fn encoder(&self, id: &str) -> Option<Entry<EncoderSpec, dyn Encoder>> {
        self.encoders
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
    Encoder,
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
            Side::Encoder => {
                let removed: Vec<_> = registry
                    .encoders
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

fn encoder_refused(reason: String) -> PluginError {
    PluginError::EncoderRefused(reason)
}

fn encoder_extension(id: &str, raw: &str) -> Result<String, PluginError> {
    if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(encoder_refused(format!(
            "{id}: the extension {raw:?} is not letters and digits without a dot"
        )));
    }
    Ok(raw.to_ascii_lowercase())
}

struct GuardedEncoder {
    id: String,
    plugin: Option<std::borrow::Cow<'static, str>>,
    ctx: PluginContext,
    inner: Box<dyn VideoEncoder>,
}

impl GuardedEncoder {
    fn fail_on_panic(&self, step: &str, payload: &(dyn std::any::Any + Send)) -> String {
        let message = panic_message(payload);
        if let Some(plugin) = self.plugin.as_deref() {
            self.ctx.fail(
                plugin,
                format!("encoder {} panicked in {step}: {message}", self.id),
            );
        }
        format!("encoder {} panicked in {step}: {message}", self.id)
    }
}

impl VideoEncoder for GuardedEncoder {
    fn push(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        let inner = &mut self.inner;
        catch_unwind(AssertUnwindSafe(|| inner.push(frame))).unwrap_or_else(|payload| {
            Err(EncodeError::Encode(
                self.fail_on_panic("push", payload.as_ref()),
            ))
        })
    }

    fn push_audio(&mut self, samples: &[f32], pts: RationalTime) -> Result<(), EncodeError> {
        let inner = &mut self.inner;
        catch_unwind(AssertUnwindSafe(|| inner.push_audio(samples, pts))).unwrap_or_else(
            |payload| {
                Err(EncodeError::Encode(
                    self.fail_on_panic("push_audio", payload.as_ref()),
                ))
            },
        )
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        let inner = &mut self.inner;
        catch_unwind(AssertUnwindSafe(|| inner.finish())).unwrap_or_else(|payload| {
            Err(EncodeError::Encode(
                self.fail_on_panic("finish", payload.as_ref()),
            ))
        })
    }

    /// The plugin encoder's own report; a panic in it fails the plugin and
    /// reports the contribution id with the processor unknown.
    fn choice(&self) -> aria_core::EncoderChoice {
        let inner = &self.inner;
        catch_unwind(AssertUnwindSafe(|| inner.choice())).unwrap_or_else(|payload| {
            self.fail_on_panic("choice", payload.as_ref());
            aria_core::EncoderChoice::only(self.id.clone(), None)
        })
    }

    /// The plugin encoder's own answer; a panic in it fails the plugin and
    /// reports `false`, so export composites every frame.
    fn accepts_cpu_nv12(&self) -> bool {
        let inner = &self.inner;
        catch_unwind(AssertUnwindSafe(|| inner.accepts_cpu_nv12())).unwrap_or_else(|payload| {
            self.fail_on_panic("accepts_cpu_nv12", payload.as_ref());
            false
        })
    }
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
        let contribution = Contribution::new(ContributionKind::Importer, id.clone());
        Ok(self.retain_if_mounting(Registration::holding(
            contribution,
            Unregister {
                registry: Arc::downgrade(registry),
                side: Side::Importer,
                id,
            },
        )))
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
        let contribution = Contribution::new(ContributionKind::Exporter, id.clone());
        Ok(self.retain_if_mounting(Registration::holding(
            contribution,
            Unregister {
                registry: Arc::downgrade(registry),
                side: Side::Exporter,
                id,
            },
        )))
    }

    /// Add a media encoder, listed in the Export dialog. Held and refused on
    /// the terms of [`register_exporter`](Self::register_exporter).
    pub fn register_encoder(
        &self,
        spec: EncoderSpec,
        encoder: impl Encoder + 'static,
    ) -> Result<Registration, PluginError> {
        let EncoderSpec {
            id,
            name,
            extension: raw,
            audio,
        } = spec;
        if id.is_empty() {
            return Err(encoder_refused("an encoder needs an id".into()));
        }
        let extension = encoder_extension(&id, &raw)?;
        let registry = self.interchange();
        {
            let mut encoders = registry.encoders.write();
            if encoders.iter().any(|entry| entry.spec.id == id) {
                return Err(encoder_refused(format!("an encoder is registered as {id}")));
            }
            encoders.push(Entry {
                spec: Arc::new(EncoderSpec {
                    id: id.clone(),
                    name,
                    extension,
                    audio,
                }),
                plugin: self.plugin_scope(),
                handler: Arc::new(encoder),
            });
        }
        let contribution = Contribution::new(ContributionKind::Encoder, id.clone());
        Ok(self.retain_if_mounting(Registration::holding(
            contribution,
            Unregister {
                registry: Arc::downgrade(registry),
                side: Side::Encoder,
                id,
            },
        )))
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

    /// Every registered encoder, in registration order.
    pub fn encoders(&self) -> Vec<Arc<EncoderSpec>> {
        self.interchange()
            .encoders
            .read()
            .iter()
            .map(|entry| Arc::clone(&entry.spec))
            .collect()
    }

    /// The encoder spec registered under `id`.
    pub fn encoder(&self, id: &str) -> Option<Arc<EncoderSpec>> {
        self.interchange().encoder(id).map(|entry| entry.spec)
    }

    /// Open a [`VideoEncoder`] session for the plugin encoder registered under
    /// `id`.
    ///
    /// Panics in [`Encoder::open`], [`VideoEncoder::push`],
    /// [`VideoEncoder::push_audio`], or [`VideoEncoder::finish`] are caught,
    /// fail the owning plugin for the session, and return an [`EncodeError`].
    pub fn open_encoder(
        &self,
        id: &str,
        path: &Path,
        config: &EncoderConfig,
    ) -> Result<Box<dyn VideoEncoder>, EncodeError> {
        let Some(Entry {
            spec,
            plugin,
            handler,
        }) = self.interchange().encoder(id)
        else {
            return Err(EncodeError::Start(format!(
                "no encoder is registered as {id}; enable or reinstall the plugin that provides it"
            )));
        };
        let opened = catch_unwind(AssertUnwindSafe(|| handler.open(path, config)));
        let inner = match opened {
            Ok(Ok(encoder)) => encoder,
            Ok(Err(err)) => return Err(err),
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                if let Some(plugin) = plugin.as_deref() {
                    self.fail(
                        plugin,
                        format!("encoder {} panicked in open: {message}", spec.id),
                    );
                }
                return Err(EncodeError::Start(format!(
                    "encoder {} panicked in open: {message}",
                    spec.id
                )));
            }
        };
        Ok(Box::new(GuardedEncoder {
            id: spec.id.clone(),
            plugin,
            ctx: self.scoped(None),
            inner,
        }))
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
        let origin = plugin.as_deref().map_or(Origin::Editor, Origin::plugin);
        let file = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        host.transact_with(origin, format!("Import {file}"), |tx| {
            handler.import(path, tx)
        })
        .map_err(|source| {
            if let (TransactError::Panicked(message), Some(plugin)) = (&source, plugin.as_deref()) {
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
                if let Some(plugin) = plugin.as_deref() {
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
