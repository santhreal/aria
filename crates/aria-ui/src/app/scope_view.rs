//! The scope strip: whether it is open, and the textures it shows.
//!
//! Scopes read the frame that is already on screen, so they cost a pass over
//! that frame and nothing else: no extra render, no decode. The pass runs on
//! the preview worker, never on the window's thread. A frame presented as a
//! texture is counted on the device that rendered it and only the counts are
//! read back ([`measure_texture`]); a frame read back to the CPU is counted
//! there ([`measure`]). Both count the same pixels into the same bins.
//!
//! While the strip is open each Viewer frame is measured where it is rendered
//! and arrives with its scopes; during playback at most 30 frames a second are
//! measured, and the strip holds the last reading between them. Opening the
//! strip, or stopping playback, measures the frame already on screen, so a
//! paused playhead does not show an empty or stale scope.
//!
//! A measurement is kept with the tick it came from, and one that arrives for
//! a frame no longer on screen is discarded, so outside playback the strip
//! never shows one frame's numbers under another frame's picture.
//!
//! Plugin scopes are measured in the same pass, after the built-in three, from
//! the same frame. Each draws into an image the worker keeps between frames,
//! and the time each takes is its plugin's frame cost in the Plugins list.

use std::sync::Arc;

use aria_analysis::{
    FrameScopes, Rgba8Frame, ScopeConfig, WaveformChannel, WaveformTrace, measure_scopes,
    scope_sample_stride,
};
use aria_compositor::{GpuScopes, ScopeChannel, ScopeGrid, wgpu};
use aria_core::{RationalTime, RgbaImage};
use aria_plugin::{PluginContext, PluginScope, ScopeError, ScopeFrame, ScopeImage};
use gpui::RenderImage;

use super::AppState;
use crate::frame::{rgba_bytes_to_render, rgba_to_render};
use crate::scopes::{ScopeImages, WaveformMode};

/// The three scopes as GPUI textures, with what they were measured from.
#[derive(Clone)]
pub struct ScopeTextures {
    /// Code values per horizontal position: luma, or a parade of three.
    pub waveform: Arc<RenderImage>,
    /// What `waveform` counted.
    pub trace: WaveformTrace,
    /// Chroma on the Cb/Cr plane.
    pub vectorscope: Arc<RenderImage>,
    /// Code-value distribution per channel.
    pub histogram: Arc<RenderImage>,
    /// Every registered plugin scope's reading of the same frame, in
    /// registration order.
    pub plugins: Vec<PluginScopeTexture>,
    /// Pixels that contributed, which is below the frame's pixel count
    /// whenever the sampling budget applied.
    pub sampled: u32,
    /// The sequence tick of the frame these belong to.
    pub tick: i64,
}

impl std::fmt::Debug for ScopeTextures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopeTextures")
            .field("trace", &self.trace)
            .field("plugins", &self.plugins)
            .field("sampled", &self.sampled)
            .field("tick", &self.tick)
            .finish_non_exhaustive()
    }
}

/// One plugin scope's reading of a frame.
#[derive(Clone)]
pub struct PluginScopeTexture {
    /// The scope and the plugin that registered it.
    pub scope: PluginScope,
    /// The drawn image, or why the measurement drew nothing.
    pub drawn: Result<Arc<RenderImage>, String>,
    /// The line the scope set under its image; empty when it set none.
    pub readout: String,
}

impl std::fmt::Debug for PluginScopeTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginScopeTexture")
            .field("scope", &self.scope.spec.id)
            .field("drawn", &self.drawn.as_ref().map(|_| ()))
            .field("readout", &self.readout)
            .finish()
    }
}

/// What a measurement repaints, kept between frames so a repaint reuses its
/// buffers.
#[derive(Default)]
pub struct ScopeBuffers {
    /// The built-in scopes' bitmaps.
    pub builtin: ScopeImages,
    /// The plugin scopes and their images.
    pub plugins: PluginScopes,
}

impl ScopeBuffers {
    /// Buffers that measure the scopes registered in `ctx` beside the
    /// built-in three.
    pub fn with_plugins(ctx: PluginContext) -> Self {
        Self {
            builtin: ScopeImages::default(),
            plugins: PluginScopes {
                ctx: Some(ctx),
                ..PluginScopes::default()
            },
        }
    }
}

/// The registered plugin scopes, each with the image it draws into.
///
/// The list is read again only when the registry's revision moves, which is
/// when a plugin registers a scope or is unloaded, so a frame costs no
/// registry walk.
#[derive(Default)]
pub struct PluginScopes {
    ctx: Option<PluginContext>,
    /// The registry revision `held` was read at.
    revision: Option<u64>,
    held: Vec<(PluginScope, ScopeImage)>,
}

impl std::fmt::Debug for PluginScopes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginScopes")
            .field("revision", &self.revision)
            .field("held", &self.held.len())
            .finish_non_exhaustive()
    }
}

impl PluginScopes {
    /// Read the registered scopes again when the registry's revision moved.
    fn refresh(&mut self) {
        let Some(ctx) = &self.ctx else {
            return;
        };
        // Read before the list, so a registration between the two reads is
        // picked up on the next frame rather than missed.
        let revision = ctx.scopes_revision();
        if self.revision == Some(revision) {
            return;
        }
        let mut kept = std::mem::take(&mut self.held);
        self.held = ctx
            .scopes()
            .into_iter()
            .map(|scope| {
                let image = kept
                    .iter()
                    .position(|(held, _)| held.spec.id == scope.spec.id)
                    .map(|at| kept.swap_remove(at).1)
                    .unwrap_or_default();
                (scope, image)
            })
            .collect();
        self.revision = Some(revision);
    }

    /// Whether a plugin scope is registered. A plugin scope reads the
    /// frame's pixels, so a frame it measures is read back to the CPU.
    pub(crate) fn any(&mut self) -> bool {
        self.refresh();
        !self.held.is_empty()
    }

    /// Measure `frame` with every registered scope. Empty without a plugin
    /// context.
    fn measure(&mut self, frame: &RgbaImage) -> Vec<PluginScopeTexture> {
        self.refresh();
        let Some(ctx) = &self.ctx else {
            return Vec::new();
        };
        let Some(view) = ScopeFrame::new(frame.width, frame.height, &frame.pixels) else {
            return Vec::new();
        };
        let mut readings = Vec::with_capacity(self.held.len());
        for (scope, image) in &mut self.held {
            let drawn = match ctx.measure_scope(&scope.spec.id, &view, image) {
                Ok(()) => rgba_bytes_to_render(image.width(), image.height(), image.pixels())
                    .ok_or_else(|| {
                        format!("{} drew an image the strip cannot show", scope.spec.id)
                    }),
                // Unregistered since the list was read: gone from the strip
                // on the next frame, when the moved revision is read.
                Err(ScopeError::Unknown(_)) => continue,
                Err(err) => Err(err.to_string()),
            };
            readings.push(PluginScopeTexture {
                scope: scope.clone(),
                drawn,
                readout: image.readout().to_owned(),
            });
        }
        readings
    }
}

/// Measure `frame`, shown at `tick`, into textures with the waveform counting
/// `trace`, repainting the bitmaps in `buffers` in place.
///
/// `None` for a frame that cannot be viewed as RGBA8: zero-sized, or a buffer
/// that does not match its dimensions. The caller keeps the previous reading
/// rather than blanking the strip.
pub fn measure(
    frame: &RgbaImage,
    tick: i64,
    trace: WaveformTrace,
    buffers: &mut ScopeBuffers,
) -> Option<ScopeTextures> {
    let view = Rgba8Frame::new(
        &frame.pixels,
        frame.width as usize,
        frame.height as usize,
        frame.width as usize * 4,
        0.0,
    )
    .ok()?;
    let scopes = measure_scopes(view, ScopeConfig::default().with_waveform_trace(trace));
    let plugins = buffers.plugins.measure(frame);
    paint(&scopes, tick, buffers, plugins)
}

/// Measure `frame`, a texture on `pass`'s device shown at `tick`, into
/// textures with the waveform counting `trace`: the pass counts on the device,
/// and only the counts are read back. Plugin scopes read pixels, so the caller
/// measures a frame with [`measure`] instead while one is registered.
///
/// # Errors
///
/// The pass's error when it cannot count `frame`. The caller reads frames
/// back from then on.
pub fn measure_texture(
    pass: &mut GpuScopes,
    frame: &wgpu::Texture,
    tick: i64,
    trace: WaveformTrace,
    buffers: &mut ScopeBuffers,
) -> Result<Option<ScopeTextures>, aria_compositor::CompositorError> {
    let config = ScopeConfig::default().with_waveform_trace(trace);
    let wgpu::Extent3d { width, height, .. } = frame.size();
    let stride = scope_sample_stride(width as usize, height as usize, config.maximum_samples());
    let grid = ScopeGrid {
        columns: u32::try_from(config.waveform_columns()).unwrap_or(u32::MAX),
        size: u32::try_from(config.vectorscope_size()).unwrap_or(u32::MAX),
        stride: u32::try_from(stride).unwrap_or(u32::MAX),
        waveform: grid_channels(trace),
    };
    let counts = pass.measure(frame, grid)?;
    let Ok(scopes) = FrameScopes::from_counts(
        config,
        counts.histogram,
        counts.waveform,
        counts.vectorscope,
        counts.sampled,
        0.0,
    ) else {
        return Ok(None);
    };
    Ok(paint(&scopes, tick, buffers, Vec::new()))
}

/// The pass's numbering of `channel`.
const fn scope_channel(channel: WaveformChannel) -> ScopeChannel {
    match channel {
        WaveformChannel::Luma => ScopeChannel::Luma,
        WaveformChannel::Red => ScopeChannel::Red,
        WaveformChannel::Green => ScopeChannel::Green,
        WaveformChannel::Blue => ScopeChannel::Blue,
        WaveformChannel::Cb => ScopeChannel::Cb,
        WaveformChannel::Cr => ScopeChannel::Cr,
    }
}

/// The `N` channels `trace` counts, in the pass's numbering and the order the
/// trace lays them out. A wrong `N` fails to compile.
const fn mapped_channels<const N: usize>(trace: WaveformTrace) -> [ScopeChannel; N] {
    let channels = trace.channels();
    assert!(channels.len() == N, "the channel count of a trace");
    let mut mapped = [ScopeChannel::Luma; N];
    let mut index = 0;
    while index < N {
        mapped[index] = scope_channel(channels[index]);
        index += 1;
    }
    mapped
}

/// The channels a scope pass counts for `trace`, as the grid holds them.
fn grid_channels(trace: WaveformTrace) -> &'static [ScopeChannel] {
    const LUMA: [ScopeChannel; 1] = mapped_channels(WaveformTrace::Luma);
    const RGB: [ScopeChannel; 3] = mapped_channels(WaveformTrace::RgbParade);
    const YCBCR: [ScopeChannel; 3] = mapped_channels(WaveformTrace::YcbcrParade);
    match trace {
        WaveformTrace::Luma => &LUMA,
        WaveformTrace::RgbParade => &RGB,
        WaveformTrace::YcbcrParade => &YCBCR,
    }
}

/// Repaint the built-in bitmaps in `buffers` from `scopes` and wrap them,
/// with the plugin scopes' readings of the same frame, as textures.
fn paint(
    scopes: &FrameScopes,
    tick: i64,
    buffers: &mut ScopeBuffers,
    plugins: Vec<PluginScopeTexture>,
) -> Option<ScopeTextures> {
    let images = &mut buffers.builtin;
    images.repaint(scopes);
    Some(ScopeTextures {
        waveform: rgba_to_render(&images.waveform)?,
        trace: scopes.waveform.trace(),
        vectorscope: rgba_to_render(&images.vectorscope)?,
        histogram: rgba_to_render(&images.histogram)?,
        plugins,
        sampled: scopes.sampled,
        tick,
    })
}

impl AppState {
    /// Whether a scope reading is on screen anywhere: the strip beside the
    /// Viewer is open, or the Scopes panel is the front tab of its frame.
    pub fn scopes_wanted(&self) -> bool {
        self.scopes_open || self.dock_tree.is_active(crate::dock::PANEL_SCOPES)
    }

    /// Show or hide the scope strip.
    pub fn toggle_scopes(&mut self) {
        self.scopes_open = !self.scopes_open;
        self.sync_scope_demand();
    }

    /// Start or stop measuring so it matches [`Self::scopes_wanted`].
    ///
    /// Starting asks the preview worker to measure the frame on screen, from
    /// its CPU copy or, for a frame held only as a GPU texture, by rendering
    /// its tick again; the reading arrives through [`Self::take_preview_mail`].
    /// Stopping drops the reading, so scopes shown again later read the frame
    /// then on screen rather than the one they were hidden on. Call it after
    /// anything that opens the strip or changes which dock tab is in front.
    pub(crate) fn sync_scope_demand(&mut self) {
        let wanted = self.scopes_wanted();
        if !self.preview_worker.set_scopes(wanted) {
            return;
        }
        if !wanted {
            self.retire_scopes(None);
            return;
        }
        self.measure_shown();
    }

    /// Measure the frame on screen unless the scopes are hidden or already
    /// read it: a frame shown without scopes outside playback, or the last
    /// frame of a run, whose measurement playback may have paced out.
    pub(crate) fn measure_shown(&mut self) {
        let Some(tick) = self.shown_tick else {
            return;
        };
        let trace = self.ui_settings.waveform.trace();
        if !self.scopes_wanted()
            || self
                .scope_textures
                .as_ref()
                .is_some_and(|read| read.tick == tick && read.trace == trace)
        {
            return;
        }
        match &self.last_frame {
            Some(rgba) => self.preview_worker.measure(Arc::clone(rgba), tick),
            None => self.preview_worker.measure_at(
                RationalTime::new(tick, self.playhead.rate),
                self.preview_fit(),
            ),
        }
    }

    /// Take a reading the scopes can show: one measured for the frame on
    /// screen, in the waveform mode chosen, while a scope is shown. Anything
    /// else is dropped. True when the reading changed.
    pub(crate) fn adopt_scopes(&mut self, measured: Option<ScopeTextures>) -> bool {
        let Some(measured) = measured else {
            return false;
        };
        if !self.scopes_wanted()
            || self.shown_tick != Some(measured.tick)
            || measured.trace != self.ui_settings.waveform.trace()
        {
            return false;
        }
        self.retire_scopes(Some(measured));
        true
    }

    /// Put `next` in the strip and queue the textures it replaces for release.
    fn retire_scopes(&mut self, next: Option<ScopeTextures>) {
        if let Some(old) = std::mem::replace(&mut self.scope_textures, next) {
            self.retired_images
                .extend([old.waveform, old.vectorscope, old.histogram]);
            self.retired_images.extend(
                old.plugins
                    .into_iter()
                    .filter_map(|reading| reading.drawn.ok()),
            );
        }
    }

    /// What the waveform shows.
    pub fn waveform_mode(&self) -> WaveformMode {
        self.ui_settings.waveform
    }

    /// Show `mode` in the waveform and measure the frame on screen again in
    /// it. The reading on screen stays until the new one arrives. False when
    /// `mode` was already shown.
    pub(crate) fn choose_waveform_mode(&mut self, mode: WaveformMode) -> bool {
        if self.ui_settings.waveform == mode {
            return false;
        }
        self.ui_settings.waveform = mode;
        self.preview_worker.set_waveform(mode.trace());
        self.measure_shown();
        true
    }

    /// Show `mode` in the waveform, measure the frame on screen again in it,
    /// and save the choice.
    pub fn set_waveform_mode(&mut self, mode: WaveformMode, cx: &mut gpui::Context<Self>) {
        if self.choose_waveform_mode(mode) {
            self.persist_settings_later(cx);
            cx.notify();
        }
    }

    /// What the scopes' header says: the frame the reading is for, in the
    /// sequence's time display, or why there is no reading.
    pub fn scope_summary(&self) -> String {
        match &self.scope_textures {
            None if !self.scopes_wanted() => "Scopes off".to_string(),
            None => "No frame".to_string(),
            Some(textures) if textures.sampled == 0 => format!(
                "Frame {}, no opaque pixels",
                self.sequence_time(textures.tick)
            ),
            Some(textures) => format!("Frame {}", self.sequence_time(textures.tick)),
        }
    }
}
