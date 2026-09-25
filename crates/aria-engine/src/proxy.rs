//! One pool source as a project of its own, at proxy size.
//!
//! A proxy is a smaller re-encode of a source file that the renderer decodes in
//! place of the original ([`Engine::set_media_proxy`](crate::Engine::set_media_proxy)).
//! The substitution has one hard requirement: the proxy must be the same content
//! frame for frame, because placement geometry and normalized UVs come from the
//! model's dimensions and not from the file that was opened. Only resolution and
//! GOP structure may differ.
//!
//! That requirement is why a proxy is produced by the same render and export path
//! as everything else rather than by a transcoder of its own: a second decode and
//! encode owner would be a second opinion about orientation, colour, stills, and
//! rate conversion, and the first frame where the two disagreed would be a
//! preview showing a picture the export does not have.
//!
//! So a proxy pass is a one-clip project: the source, whole, on a canvas at the
//! reduced size. Rendering it is then an ordinary export.

use aria_models::{CanvasSettings, MediaSource, Project, RationalTime, TrackKind};

use crate::error::EngineError;

/// Longest edge of a generated proxy, in pixels.
///
/// 960 is the smallest size that still reads as the shot on a full-screen
/// preview, and it is a quarter of the pixels of 1080p footage — the point of a
/// proxy is that the decode is cheap, and decode cost scales with area.
pub const PROXY_MAX_EDGE: u32 = 960;

/// A project that renders `source` from its first frame to its last, on a canvas
/// scaled so neither edge exceeds `max_edge`.
///
/// Refuses what a proxy cannot help: a source with no picture, a still (decoded
/// once and cached, so a smaller copy buys nothing), a source that reports no
/// frames, and a size that cannot be expressed as a canvas.
pub fn proxy_project(source: MediaSource, max_edge: u32) -> Result<Project, EngineError> {
    if source.is_audio_only() {
        return Err(EngineError::Refused(
            "that source has no picture, so a proxy would decode nothing".into(),
        ));
    }
    if source.is_image {
        return Err(EngineError::Refused(
            "a still is decoded once and kept, so a proxy would not make it faster".into(),
        ));
    }
    let frames = source.duration.value;
    if frames <= 0 {
        return Err(EngineError::Refused(
            "that source reports no frames to render".into(),
        ));
    }
    let size = proxy_size(source.width, source.height, max_edge).ok_or_else(|| {
        EngineError::Refused(format!(
            "{}×{} does not scale to a canvas a proxy can be written at",
            source.width, source.height
        ))
    })?;
    let rate = source.frame_rate;
    let whole = source.full_range();

    let mut project = Project::new("proxy", rate);
    // The source's own framing at a smaller size. Anything derived would
    // letterbox the proxy, and a letterboxed proxy is not the same content:
    // every clip placed from that media would preview with bars the export
    // does not have.
    project.timeline_mut().set_canvas(CanvasSettings {
        resolution: Some(size),
        ..CanvasSettings::default()
    });
    let media = project.add_media(source);
    let track = project.add_track(TrackKind::Video, "V1");
    project.add_clip(track, media, whole, RationalTime::new(0, rate))?;
    Ok(project)
}

/// `[w, h]` for a proxy of a `width`×`height` source, or `None` when the result
/// would not be a canvas.
///
/// Never upscales: a source already under `max_edge` is proxied at its own size,
/// so a proxy is always at most as expensive to decode as the original. Both
/// edges are rounded down to even values because H.264 needs them, and a
/// rounding that would leave an edge outside the canvas bounds is refused rather
/// than clamped — clamping one edge alone changes the aspect ratio, which is the
/// one thing a proxy may not do.
pub fn proxy_size(width: u32, height: u32, max_edge: u32) -> Option<[u32; 2]> {
    if width == 0 || height == 0 || max_edge == 0 {
        return None;
    }
    let longest = width.max(height);
    let size = if longest > max_edge {
        let scale = f64::from(max_edge) / f64::from(longest);
        [
            even(f64::from(width) * scale),
            even(f64::from(height) * scale),
        ]
    } else {
        [even(f64::from(width)), even(f64::from(height))]
    };
    CanvasSettings::resolution_is_valid(size).then_some(size)
}

/// Round to the nearest pixel, then down to an even count. Negative input
/// saturates at zero so an invalid size is refused by the bounds check rather
/// than wrapping into a valid one.
fn even(value: f64) -> u32 {
    let rounded = value.round();
    if rounded < 0.0 {
        return 0;
    }
    let pixels = rounded as u32;
    pixels & !1
}

#[cfg(test)]
#[path = "proxy_tests.rs"]
mod proxy_tests;
