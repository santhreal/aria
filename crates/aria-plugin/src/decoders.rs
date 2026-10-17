//! Decoders: plugins that read media formats for import, playback, analysis,
//! and export.
//!
//! A registered decoder joins [`aria_decoder`]'s process-wide list, so every
//! probe and decoder open in the process tries it for the extensions it
//! claims, by priority and then registration order, before the platform
//! decoder. A panic in any call the host makes into the decoder or into a
//! decoder or reader it opened fails its plugin for the session, which drops
//! the registration. Time spent decoding a frame is recorded as the plugin's
//! frame cost.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use aria_core::{AudioReader, DecodeError, RationalTime, SourceInfo, VideoDecoder, VideoFrame};
use aria_decoder::{ExternalDecoder, MediaProbe, register_external_decoder};

use crate::context::{PluginContext, WeakContext};
use crate::error::PluginError;
use crate::events::panic_message;
use crate::listed::{Listed, owner};
use crate::registration::{Contribution, ContributionKind, Registration};

/// What a decoder reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderSpec {
    /// Unique among the registering plugin's decoders.
    pub id: String,
    pub name: String,
    /// File extensions without the dot: ASCII letters and digits, any case.
    pub extensions: Vec<String>,
    /// Among decoders claiming one extension, a higher priority is tried
    /// first. The platform decoder is tried after every plugin decoder.
    pub priority: i32,
}

/// One registered decoder, as the Plugins list and the import dialogs read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDecoder {
    /// The registering plugin's id, empty for the host.
    pub plugin_id: Arc<str>,
    pub spec: Arc<DecoderSpec>,
}

pub(crate) type Decoders = Listed<PluginDecoder>;

fn refused(reason: String) -> PluginError {
    PluginError::DecoderRefused(reason)
}

impl PluginContext {
    /// Add a decoder for the files whose extension `spec` lists. Import,
    /// playback, export, and analysis open those files through `decoder`
    /// before the platform decoder.
    ///
    /// Held on the terms of [`register_effect`](Self::register_effect).
    /// Refused when the id is empty or this plugin already registered it, no
    /// extension is listed, or an extension holds anything but ASCII letters
    /// and digits.
    pub fn register_decoder(
        &self,
        spec: DecoderSpec,
        decoder: impl ExternalDecoder + 'static,
    ) -> Result<Registration, PluginError> {
        let DecoderSpec {
            id,
            name,
            extensions,
            priority,
        } = spec;
        if id.is_empty() {
            return Err(refused("a decoder needs an id".into()));
        }
        if extensions.is_empty() {
            return Err(refused(format!("{id}: a decoder reads no extension")));
        }
        let extensions = extensions
            .iter()
            .map(|raw| {
                if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return Err(refused(format!(
                        "{id}: the extension {raw:?} is not letters and digits without a dot"
                    )));
                }
                Ok(raw.to_ascii_lowercase())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (plugin_id, _) = owner(self);
        let spec = Arc::new(DecoderSpec {
            id: id.clone(),
            name,
            extensions,
            priority,
        });
        let listed = self
            .decoder_registry()
            .insert(
                &plugin_id,
                PluginDecoder {
                    plugin_id: Arc::clone(&plugin_id),
                    spec: Arc::clone(&spec),
                },
                |held| held.spec.id == id,
            )
            .ok_or_else(|| refused(format!("a decoder is registered as {id}")))?;
        let guarded = GuardedDecoder {
            id: Arc::from(id.as_str()),
            plugin: (!plugin_id.is_empty()).then_some(plugin_id),
            ctx: self.downgrade(),
            inner: Box::new(decoder),
        };
        let handle = register_external_decoder(&id, &spec.extensions, priority, Arc::new(guarded));
        let contribution = Contribution::new(ContributionKind::Decoder, id);
        Ok(self.retain_if_mounting(Registration::holding(contribution, (handle, listed))))
    }

    /// Every registered decoder, in registration order.
    pub fn decoders(&self) -> Vec<PluginDecoder> {
        self.decoder_registry().items()
    }

    /// Counts decoder registrations and removals.
    pub fn decoders_revision(&self) -> u64 {
        self.decoder_registry().revision()
    }
}

/// Who answers for a decoder's faults: its id, its plugin, and the context
/// that fails the plugin. Shared by the decoder and everything it opens.
#[derive(Clone)]
struct Fault {
    id: Arc<str>,
    plugin: Option<Arc<str>>,
    ctx: WeakContext,
}

impl Fault {
    /// Run `call`, failing the plugin and returning a decode error if it
    /// panics.
    fn guard<T>(
        &self,
        step: &str,
        call: impl FnOnce() -> Result<T, DecodeError>,
    ) -> Result<T, DecodeError> {
        catch_unwind(AssertUnwindSafe(call)).unwrap_or_else(|payload| {
            let reason = format!(
                "decoder {} panicked in {step}: {}",
                self.id,
                panic_message(payload.as_ref())
            );
            if let (Some(plugin), Some(ctx)) = (self.plugin.as_deref(), self.ctx.upgrade()) {
                ctx.fail(plugin, reason.clone());
            }
            Err(DecodeError::Decode(reason))
        })
    }

    /// [`guard`](Self::guard), recording the call's time as frame cost.
    fn timed<T>(
        &self,
        step: &str,
        call: impl FnOnce() -> Result<T, DecodeError>,
    ) -> Result<T, DecodeError> {
        let started = Instant::now();
        let result = self.guard(step, call);
        if let (Some(plugin), Some(ctx)) = (self.plugin.as_deref(), self.ctx.upgrade()) {
            ctx.record_frame_cost(plugin, started.elapsed());
        }
        result
    }
}

struct GuardedDecoder {
    id: Arc<str>,
    plugin: Option<Arc<str>>,
    ctx: WeakContext,
    inner: Box<dyn ExternalDecoder>,
}

impl GuardedDecoder {
    fn fault(&self) -> Fault {
        Fault {
            id: Arc::clone(&self.id),
            plugin: self.plugin.clone(),
            ctx: self.ctx.clone(),
        }
    }
}

impl ExternalDecoder for GuardedDecoder {
    fn probe(&self, path: &Path) -> Result<MediaProbe, DecodeError> {
        self.fault().guard("probe", || self.inner.probe(path))
    }

    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoDecoder>, DecodeError> {
        let fault = self.fault();
        let inner = fault.guard("open_video", || self.inner.open_video(path))?;
        Ok(Box::new(GuardedVideo { fault, inner }))
    }

    fn open_audio(
        &self,
        path: &Path,
        out_rate: u32,
        channels: u16,
    ) -> Result<Box<dyn AudioReader>, DecodeError> {
        let fault = self.fault();
        let inner = fault.guard("open_audio", || {
            self.inner.open_audio(path, out_rate, channels)
        })?;
        Ok(Box::new(GuardedAudio { fault, inner }))
    }
}

struct GuardedVideo {
    fault: Fault,
    inner: Box<dyn VideoDecoder>,
}

impl VideoDecoder for GuardedVideo {
    fn info(&self) -> &SourceInfo {
        self.inner.info()
    }

    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        let inner = &mut self.inner;
        self.fault.guard("seek", || inner.seek(target))
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        let inner = &mut self.inner;
        self.fault.timed("next_frame", || inner.next_frame())
    }

    fn frame_at(&mut self, target: RationalTime) -> Result<Option<VideoFrame>, DecodeError> {
        let inner = &mut self.inner;
        self.fault.timed("frame_at", || inner.frame_at(target))
    }

    fn frame_at_nearest(
        &mut self,
        target: RationalTime,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        let inner = &mut self.inner;
        self.fault
            .timed("frame_at_nearest", || inner.frame_at_nearest(target))
    }
}

struct GuardedAudio {
    fault: Fault,
    inner: Box<dyn AudioReader>,
}

impl AudioReader for GuardedAudio {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let inner = &mut self.inner;
        self.fault.guard("read", || inner.read(out))
    }

    fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
        let inner = &mut self.inner;
        self.fault
            .guard("seek_to_frame", || inner.seek_to_frame(frame))
    }

    fn position(&self) -> Option<i64> {
        self.inner.position()
    }
}
