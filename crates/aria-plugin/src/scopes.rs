//! Scopes: plugins that measure the Viewer frame for the scope strip.
//!
//! A scope is an id, a caption, and the size of the image it draws. While the
//! scope strip is open, the preview worker measures every frame it shows with
//! each registered scope, on the worker's thread, after the waveform,
//! vectorscope, and histogram. A scope reads the frame as RGBA8 and draws into
//! an image of its declared size, cleared to transparent black before each
//! call, and may set one line of readout text. Time spent measuring is
//! recorded as the plugin's frame cost. A scope that panics fails its plugin
//! for the session.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Instant;

use crate::context::PluginContext;
use crate::error::PluginError;
use crate::events::panic_message;
use crate::listed::{Listed, owner};
use crate::plugin::PluginManifest;
use crate::registration::{Contribution, ContributionKind, Registration};

/// Widest and tallest image a scope may draw, in pixels.
pub const MAX_SCOPE_SIDE: u32 = 512;

/// Longest readout line a scope may set, in bytes.
pub const MAX_SCOPE_READOUT: usize = 160;

/// Longest scope id, in bytes.
const MAX_SCOPE_ID: usize = 128;

/// What a scope is called and how large it draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeSpec {
    /// ASCII letters, digits, `.`, `-` and `_`, unique among every plugin's
    /// scopes.
    pub id: String,
    /// The caption the scope strip shows above the image.
    pub name: String,
    /// Width of the image the scope draws, 1 to [`MAX_SCOPE_SIDE`].
    pub width: u32,
    /// Height of the image the scope draws, 1 to [`MAX_SCOPE_SIDE`].
    pub height: u32,
}

/// The frame a scope measures: tightly packed RGBA8, sRGB encoded, straight
/// alpha, top row first.
#[derive(Debug, Clone, Copy)]
pub struct ScopeFrame<'a> {
    width: u32,
    height: u32,
    pixels: &'a [u8],
}

impl<'a> ScopeFrame<'a> {
    /// View `pixels` as a `width` by `height` frame, or `None` when either
    /// side is zero or the buffer is not `width * height * 4` bytes.
    pub fn new(width: u32, height: u32, pixels: &'a [u8]) -> Option<Self> {
        let len = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        (width > 0 && height > 0 && pixels.len() == len).then_some(Self {
            width,
            height,
            pixels,
        })
    }

    /// Width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Every pixel, four bytes each, row after row.
    pub const fn pixels(&self) -> &'a [u8] {
        self.pixels
    }

    /// The pixel at column `x` and row `y`, or `None` outside the frame.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels
            .get(at..at + 4)
            .and_then(|px| <[u8; 4]>::try_from(px).ok())
    }
}

/// The image a scope draws: RGBA8, straight alpha, top row first, at the size
/// its spec declares, and one line of readout text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScopeImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    readout: String,
}

impl ScopeImage {
    /// A transparent `width` by `height` image with no readout.
    pub fn new(width: u32, height: u32) -> Self {
        let mut image = Self::default();
        image.reset(width, height);
        image
    }

    /// Width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Every pixel, four bytes each, row after row.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Every pixel, for writing. The length is fixed at `width * height * 4`.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Set the pixel at column `x` and row `y`. A position outside the image
    /// is ignored.
    pub fn put(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let at = (y as usize * self.width as usize + x as usize) * 4;
        if let Some(px) = self.pixels.get_mut(at..at + 4) {
            px.copy_from_slice(&rgba);
        }
    }

    /// The readout line, empty when the scope set none.
    pub fn readout(&self) -> &str {
        &self.readout
    }

    /// Set the readout line shown under the image.
    pub fn set_readout(&mut self, text: impl Into<String>) {
        self.readout = text.into();
    }

    /// Resize to `width` by `height`, clear every pixel to transparent black,
    /// and clear the readout. Keeps the buffer's allocation.
    pub(crate) fn reset(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.pixels.clear();
        self.pixels.resize(width as usize * height as usize * 4, 0);
        self.readout.clear();
    }
}

/// Measures a frame into an image.
pub trait Scope: Send + Sync {
    /// Draw a measurement of `frame` into `image`, which is transparent and at
    /// the spec's size on entry, and optionally set its readout. `ctx` is the
    /// registering plugin's view of the context. An `Err` is shown in place
    /// of the image.
    fn measure(
        &self,
        ctx: &PluginContext,
        frame: &ScopeFrame<'_>,
        image: &mut ScopeImage,
    ) -> Result<(), String>;
}

impl<F> Scope for F
where
    F: Fn(&PluginContext, &ScopeFrame<'_>, &mut ScopeImage) -> Result<(), String> + Send + Sync,
{
    fn measure(
        &self,
        ctx: &PluginContext,
        frame: &ScopeFrame<'_>,
        image: &mut ScopeImage,
    ) -> Result<(), String> {
        self(ctx, frame, image)
    }
}

/// One registered scope, as the scope strip and the Plugins list read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginScope {
    /// The registering plugin's id, empty for the host.
    pub plugin_id: Arc<str>,
    /// The registering plugin's name.
    pub plugin_name: Arc<str>,
    pub spec: Arc<ScopeSpec>,
}

/// Why a measurement drew nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScopeError {
    /// No scope is registered under this id.
    #[error("no plugin scope is registered as {0}")]
    Unknown(String),
    /// The scope returned this reason, or a readout the host does not accept.
    #[error("{0}")]
    Failed(String),
    /// The scope panicked with this message. Its plugin is failed.
    #[error("the scope panicked: {0}")]
    Panicked(String),
}

#[derive(Clone)]
pub(crate) struct HeldScope {
    listed: PluginScope,
    /// The registering plugin's manifest, or `None` for the host.
    manifest: Option<Arc<PluginManifest>>,
    handler: Arc<dyn Scope>,
}

pub(crate) type Scopes = Listed<HeldScope>;

fn refused(reason: String) -> PluginError {
    PluginError::ScopeRefused(reason)
}

/// Refuses a spec the scope strip cannot show.
fn check(spec: &ScopeSpec) -> Result<(), PluginError> {
    let id = &spec.id;
    if id.is_empty()
        || id.len() > MAX_SCOPE_ID
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return Err(refused(format!(
            "scope id {id:?} must be 1 to {MAX_SCOPE_ID} ASCII letters, digits, `.`, `-` or `_`"
        )));
    }
    if spec.name.trim().is_empty() {
        return Err(refused(format!("{id}: a scope needs a name")));
    }
    let sides = 1..=MAX_SCOPE_SIDE;
    if !sides.contains(&spec.width) || !sides.contains(&spec.height) {
        return Err(refused(format!(
            "{id}: a scope draws 1 to {MAX_SCOPE_SIDE} pixels on a side, not {}x{}",
            spec.width, spec.height
        )));
    }
    Ok(())
}

impl PluginContext {
    /// Add a scope to the scope strip.
    ///
    /// Held on the terms of [`register_effect`](Self::register_effect).
    /// Refused when the id is malformed or any plugin already registered a
    /// scope under it, the name is empty, or a side is 0 or over
    /// [`MAX_SCOPE_SIDE`].
    pub fn register_scope(
        &self,
        spec: ScopeSpec,
        scope: impl Scope + 'static,
    ) -> Result<Registration, PluginError> {
        check(&spec)?;
        let (plugin_id, plugin_name) = owner(self);
        let spec = Arc::new(spec);
        let id = spec.id.clone();
        let held = HeldScope {
            listed: PluginScope {
                plugin_id: Arc::clone(&plugin_id),
                plugin_name,
                spec,
            },
            manifest: self.scope(),
            handler: Arc::new(scope),
        };
        let listed = self
            .scope_registry()
            .insert_unique(&plugin_id, held, |other| other.listed.spec.id == id)
            .ok_or_else(|| refused(format!("a scope is registered as {id}")))?;
        let contribution = Contribution::new(ContributionKind::Scope, id);
        Ok(self.retain_if_mounting(Registration::holding(contribution, listed)))
    }

    /// Every registered scope, in registration order.
    pub fn scopes(&self) -> Vec<PluginScope> {
        self.scope_registry().entries(|held| held.listed.clone())
    }

    /// Counts scope registrations and removals.
    pub fn scopes_revision(&self) -> u64 {
        self.scope_registry().revision()
    }

    /// Measure `frame` with the scope registered as `id`, drawing into
    /// `image`.
    ///
    /// `image` is resized to the scope's size and cleared first, keeping its
    /// allocation, so a caller that holds one image per scope draws every
    /// frame without allocating. The scope runs with no registry lock held,
    /// and the time it takes is recorded as its plugin's frame cost. A panic
    /// fails the registering plugin for the session. A readout over
    /// [`MAX_SCOPE_READOUT`] bytes or over one line fails the measurement.
    pub fn measure_scope(
        &self,
        id: &str,
        frame: &ScopeFrame<'_>,
        image: &mut ScopeImage,
    ) -> Result<(), ScopeError> {
        let Some(held) = self.scope_registry().find(|held| held.listed.spec.id == id) else {
            return Err(ScopeError::Unknown(id.to_owned()));
        };
        let spec = &held.listed.spec;
        image.reset(spec.width, spec.height);
        let view = self.scoped(held.manifest.clone());
        let started = Instant::now();
        let measured = catch_unwind(AssertUnwindSafe(|| {
            held.handler.measure(&view, frame, image)
        }));
        if let Some(manifest) = &held.manifest {
            self.record_frame_cost(&manifest.id, started.elapsed());
        }
        match measured {
            Ok(Ok(())) => {
                let readout = image.readout();
                if readout.len() > MAX_SCOPE_READOUT || readout.contains(['\n', '\r']) {
                    image.reset(spec.width, spec.height);
                    return Err(ScopeError::Failed(format!(
                        "{id} set a readout over one line or {MAX_SCOPE_READOUT} bytes"
                    )));
                }
                Ok(())
            }
            Ok(Err(reason)) => Err(ScopeError::Failed(reason)),
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                if let Some(manifest) = &held.manifest {
                    self.fail(&manifest.id, format!("scope {id} panicked: {message}"));
                }
                Err(ScopeError::Panicked(message))
            }
        }
    }
}

#[cfg(test)]
#[path = "scopes_tests.rs"]
mod tests;
