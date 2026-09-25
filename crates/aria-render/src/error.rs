//! Errors raised while resolving or rendering a project.

use aria_compositor::CompositorError;
use aria_core::{DecodeError, EncodeError};
use aria_models::{MediaId, ModelError};

/// A render failure: a bad model query, a decode/compositor error, or content
/// the current renderer doesn't handle yet.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// A timeline/clip query against the model failed.
    #[error("model error: {0}")]
    Model(#[from] ModelError),

    /// Decoding a media frame failed.
    #[error("decode error: {0}")]
    Decode(#[from] DecodeError),

    /// The compositor failed to render the layer stack.
    #[error("compositor error: {0}")]
    Compositor(#[from] CompositorError),

    /// Encoding or muxing an exported frame failed.
    #[error("encode error: {0}")]
    Encode(#[from] EncodeError),

    /// Reading or writing a file failed (project load, frame output).
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// A clip references a media id that isn't in the project's media pool.
    #[error("media {0:?} is not in the project media pool")]
    MissingMedia(MediaId),

    /// The decoder reached end-of-stream before the requested source time.
    #[error("no frame available for media {media:?} at {time:?}")]
    NoFrame {
        media: MediaId,
        time: aria_core::RationalTime,
    },

    /// Content or a platform path the current renderer doesn't support yet.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// The export would have written no frames at all.
    ///
    /// The encoder accepts a zero-frame pass happily and muxes a header with
    /// no samples, so the refusal has to be ours: such a file reads as a
    /// successful export until somebody opens it.
    #[error("nothing to export: {0}")]
    Empty(#[from] EmptyExport),

    /// The export asked for an output this machine cannot write.
    ///
    /// Separate from [`RenderError::Unsupported`] because it is a refusal of a
    /// *choice*, before any work starts: the caller can pick again.
    #[error("{0}")]
    Format(#[from] ExportFormatError),

    /// The work was abandoned because whoever asked for it no longer wants it.
    ///
    /// Two callers raise this. An observed export cancelled by its progress
    /// callback: convenience file exports clean their private staging output and
    /// leave the requested destination untouched, while generic exports using a
    /// caller-owned encoder do not finalize it, so that encoder's output remains
    /// caller-managed. And a preview frame cancelled through
    /// [`Renderer::cancel_next_frame`](crate::Renderer::cancel_next_frame),
    /// which writes nothing: the sink is untouched and the scene is dropped
    /// where the check caught it.
    #[error("cancelled")]
    Cancelled,
}

/// Why an export would write no frames.
///
/// A closed set rather than a message, so every shell that turns a refusal into
/// copy has to answer for each reason: a new one stops compiling the match
/// instead of falling into a sentence about the wrong problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EmptyExport {
    /// No track holds a clip, so the timeline has no duration to render.
    #[error("the sequence has no content — place a clip first")]
    Sequence,
    /// The timeline has content, but the requested range covers no frame of it.
    #[error("the requested range covers no frame — move the in and out points apart")]
    Range,
    /// A sound-only export whose mix is silent. There is no picture to carry
    /// the file, so a silent one holds nothing at all.
    #[error("nothing in the sequence is audible — unmute a lane or raise a fader")]
    Silence,
}

impl EmptyExport {
    /// Every reason, so a shell's copy and its tests are checked against the
    /// set rather than against whichever reason someone remembered. Adding a
    /// variant changes this array's length and breaks both.
    pub const ALL: [Self; 3] = [Self::Sequence, Self::Range, Self::Silence];
}

/// A choice an export asked for that this machine cannot honour.
///
/// Raised while resolving an [`aria_core::ExportFormat`], before a staging file
/// exists. Each variant names the backend it was refused by and what is
/// available instead, because the caller is a picker, a command line, or an
/// agent, and all three can ask again.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportFormatError {
    /// The platform has no encoder at all, so no format is writable.
    #[error("this build has no encoder, so it cannot export")]
    NoEncoder,
    /// The codec is not one the installed backend writes. Swapping it for one
    /// that works would produce a file nobody asked for.
    #[error("{backend} cannot write {}; it writes {}", asked.label(), available.join(" or "))]
    Codec {
        asked: aria_core::VideoCodec,
        backend: &'static str,
        available: Vec<&'static str>,
    },
    /// The backend sets its own bitrate, so a target would be ignored.
    #[error("{backend} sets its own bitrate; ask for one it honours or leave it unset")]
    Bitrate { backend: &'static str },
    /// A size no encoder accepts.
    #[error("{}x{} is too small to encode; both sides must be at least {MIN_EXPORT_DIM}", asked.0, asked.1)]
    Size { asked: (u32, u32) },
    /// A rate that is not a rate.
    #[error("{asked:?} is not a frame rate an export can sample at")]
    Rate { asked: aria_core::Rational },
}

/// The smallest side an encoder will take. Below this a macroblock has nowhere
/// to go and every backend refuses, so the refusal is ours and it is by name.
pub const MIN_EXPORT_DIM: u32 = 16;

impl RenderError {
    pub fn unsupported(msg: impl Into<String>) -> Self {
        RenderError::Unsupported(msg.into())
    }
}
