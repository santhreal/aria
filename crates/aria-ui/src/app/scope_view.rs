//! The scope strip: whether it is open, and the textures it shows.
//!
//! Scopes read the frame that is already on screen, so they cost a pass over
//! `last_frame` and nothing else — no extra render, no decode. That pass is
//! only taken while the strip is open, which is why the toggle measures
//! immediately instead of waiting for the next frame: opening the strip on a
//! paused playhead would otherwise show an empty scope.
//!
//! The measurement is kept with the timestamp it came from, so a strip cannot
//! show one frame's numbers under another frame's picture.

use std::sync::Arc;

use aria_analysis::{Rgba8Frame, ScopeConfig, measure_scopes};
use gpui::RenderImage;

use super::AppState;
use crate::frame::rgba_to_render;
use crate::scopes::ScopeImages;

/// The three scopes as GPUI textures, with what they were measured from.
pub struct ScopeTextures {
    /// Luma per horizontal position.
    pub waveform: Arc<RenderImage>,
    /// Chroma on the Cb/Cr plane.
    pub vectorscope: Arc<RenderImage>,
    /// Code-value distribution per channel.
    pub histogram: Arc<RenderImage>,
    /// Pixels that contributed, which is below the frame's pixel count
    /// whenever the sampling budget applied.
    pub sampled: u32,
    /// Timestamp of the frame these belong to.
    pub timestamp_seconds: f64,
    /// Originating frame pointer address to reuse textures without reallocating
    /// on consecutive refreshes of the exact same frame.
    pub(crate) frame_origin: usize,
}

impl AppState {
    /// Show or hide the scope strip. Opening it measures the frame on screen.
    pub fn toggle_scopes(&mut self) {
        self.scopes_open = !self.scopes_open;
        if self.scopes_open {
            self.refresh_scopes();
        } else {
            self.scope_textures = None;
        }
    }

    /// Measure the frame on screen, if the strip is open and there is one.
    ///
    /// Called when a new preview frame lands, so the scopes follow the
    /// playhead. A frame that cannot be viewed as RGBA8 (zero-sized, or a
    /// buffer that does not match its dimensions) leaves the previous
    /// measurement in place rather than blanking the strip.
    pub fn refresh_scopes(&mut self) {
        if !self.scopes_open {
            self.scope_textures = None;
            return;
        }
        let Some(frame) = self.last_frame.as_ref() else {
            return;
        };
        let frame_origin = Arc::as_ptr(frame) as usize;
        if let Some(textures) = &self.scope_textures {
            if textures.frame_origin == frame_origin {
                return;
            }
        }
        let Ok(view) = Rgba8Frame::new(
            &frame.pixels,
            frame.width as usize,
            frame.height as usize,
            frame.width as usize * 4,
            0.0,
        ) else {
            return;
        };
        let scopes = measure_scopes(view, ScopeConfig::default());
        let images = self.scope_images.get_or_insert_with(ScopeImages::default);
        images.repaint(&scopes);
        let Some(waveform) = rgba_to_render(&images.waveform) else {
            return;
        };
        let Some(vectorscope) = rgba_to_render(&images.vectorscope) else {
            return;
        };
        let Some(histogram) = rgba_to_render(&images.histogram) else {
            return;
        };
        self.scope_textures = Some(ScopeTextures {
            waveform,
            vectorscope,
            histogram,
            sampled: scopes.sampled,
            timestamp_seconds: scopes.timestamp_seconds,
            frame_origin,
        });
    }

    /// What the strip's header says: which frame, and how much of it was read.
    pub fn scope_summary(&self) -> String {
        match &self.scope_textures {
            None if !self.scopes_open => "Scopes off".to_string(),
            None => "No frame".to_string(),
            Some(textures) => format!("{} px sampled", textures.sampled),
        }
    }
}
