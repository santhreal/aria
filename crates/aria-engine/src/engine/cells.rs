//! Sequence frames rendered away from the session's renderer.
//!
//! A filmstrip cell is one sequence frame fit to a thumbnail. Rendered through
//! the session's renderer, a cell holds the session for its whole decode and
//! moves the decoder the Viewer reads from to the cell's source time, so the
//! Viewer's next frame seeks back and decodes a GOP again. A [`CellRenderer`]
//! renders from a [`CellSource`] on decoders and pipelines of its own: the
//! session is held only to take the source, which costs reference counts.

use std::sync::Arc;

use aria_compositor::GpuContext;
use aria_models::{Project, RationalTime};
use aria_render::{MediaSources, Renderer, RgbaImage};

use super::Engine;
use crate::error::EngineError;

/// What a [`CellRenderer`] renders from: the committed project, the files the
/// session decodes each media id from, and the session's GPU device.
#[derive(Debug)]
pub struct CellSource {
    project: Arc<Project>,
    media: MediaSources,
    gpu: GpuContext,
}

impl Engine {
    /// The [`CellSource`] for the project as it is now. Live preview
    /// overrides are not part of it: a cell shows the committed project.
    pub fn cell_source(&self) -> CellSource {
        CellSource {
            project: self.snapshot(),
            media: self.renderer.media_sources(),
            gpu: self.renderer.gpu().share(),
        }
    }
}

/// A renderer of sequence frames with decoders of its own, on the session's
/// GPU device. The first frame builds it.
#[derive(Debug, Default)]
pub struct CellRenderer {
    renderer: Option<Renderer>,
}

impl CellRenderer {
    /// The frame `source`'s project plays at `at`, fit within
    /// `max_width`×`max_height` (aspect kept, never upscaled).
    pub fn render_fit(
        &mut self,
        source: &CellSource,
        at: RationalTime,
        max_width: u32,
        max_height: u32,
    ) -> Result<RgbaImage, EngineError> {
        let renderer = self
            .renderer
            .get_or_insert_with(|| Renderer::with_gpu(source.gpu.share()));
        renderer.adopt_media_sources(&source.media);
        Ok(renderer.render_frame_fit(&source.project, at, max_width, max_height)?)
    }
}
