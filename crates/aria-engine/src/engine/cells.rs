//! Lane frames rendered away from the session's renderer.
//!
//! A filmstrip cell is one lane's frame fit to a thumbnail: the lane's
//! pictures alone over the canvas background, so a cell shows the clip it is
//! drawn on and not the lanes above it. Rendered through the session's
//! renderer, a cell holds the session for its whole decode and moves the
//! decoder the Viewer reads from to the cell's source time, so the Viewer's
//! next frame seeks back and decodes a GOP again. A [`CellRenderer`] renders
//! from a [`CellSource`] on decoders and pipelines of its own: the session is
//! held only to take the source, which costs reference counts.

use std::sync::Arc;

use aria_compositor::GpuContext;
use aria_models::{Project, RationalTime, TrackId};
use aria_render::{MediaSources, Renderer, ResolveOverrides, RgbaImage, SeekPolicy};

use super::Engine;
use crate::error::EngineError;

/// What a [`CellRenderer`] renders from: the committed project, the files the
/// session decodes each media id from, and the session's GPU device, `None`
/// while the session has drawn no frame.
#[derive(Debug)]
pub struct CellSource {
    project: Arc<Project>,
    media: MediaSources,
    gpu: Option<GpuContext>,
}

impl Engine {
    /// The [`CellSource`] for the project as it is now. Live preview
    /// overrides are not part of it: a cell shows the committed project.
    pub fn cell_source(&self) -> CellSource {
        CellSource {
            project: self.snapshot(),
            media: self.renderer.media_sources(),
            gpu: self.renderer.gpu().map(GpuContext::share),
        }
    }
}

/// One frame a [`CellRenderer`] rendered.
#[derive(Debug)]
pub struct CellFrame {
    pub image: RgbaImage,
    /// A media layer shows the sync frame before its source time rather than
    /// the frame at it: the render decoded under [`SeekPolicy::NearestSync`]
    /// and snapped.
    pub snapped: bool,
}

/// A renderer of lane frames with decoders of its own, on the session's GPU
/// device, or on the process's headless one when the session has drawn no
/// frame yet. The first frame builds it.
#[derive(Debug, Default)]
pub struct CellRenderer {
    renderer: Option<Renderer>,
}

impl CellRenderer {
    /// The frame `lane` of `source`'s project shows at `at`, over the canvas
    /// background and whether or not the lane is hidden, fit within
    /// `max_width`×`max_height` (aspect kept, never upscaled), its media
    /// decoded under `policy`.
    pub fn render_fit(
        &mut self,
        source: &CellSource,
        lane: TrackId,
        at: RationalTime,
        max_width: u32,
        max_height: u32,
        policy: SeekPolicy,
    ) -> Result<CellFrame, EngineError> {
        let renderer = self.renderer.get_or_insert_with(|| match &source.gpu {
            Some(gpu) => Renderer::with_gpu(gpu.share()),
            None => Renderer::deferred(),
        });
        renderer.adopt_media_sources(&source.media);
        let image = renderer.render_frame_fit_with(
            &source.project,
            at,
            max_width,
            max_height,
            ResolveOverrides {
                lane: Some(lane),
                ..ResolveOverrides::default()
            },
            policy,
        )?;
        Ok(CellFrame {
            image,
            snapped: renderer.last_frame_stats().snapped_frames > 0,
        })
    }
}
