use aria_core::DecodeError;
use aria_models::{ModelError, TimeError};
use aria_render::RenderError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Model(#[from] ModelError),

    #[error(transparent)]
    Time(#[from] TimeError),

    /// Preview/export failure from the render pipeline (decode, composite,
    /// encode, or I/O).
    #[error(transparent)]
    Render(#[from] RenderError),

    /// Native media probe/decode failure (unreadable or unsupported file),
    /// surfaced when importing or relinking media.
    #[error(transparent)]
    Decode(#[from] DecodeError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("import failed: {0}")]
    Import(String),

    #[error("export: {0}")]
    Export(String),

    #[error("media file not found: {0}")]
    MissingMedia(String),

    /// A command whose backend isn't available in this build (e.g. audio
    /// ducking and beat detection, which need the decoder's audio reader).
    #[error("unsupported on this build: {0}")]
    Unsupported(String),

    /// A host policy or plugin refused the command before it reached the model,
    /// or another thread held the project: the project is untouched and the
    /// reason is the refusal, not a failure. Produced by the project host, never
    /// by dispatch.
    #[error("refused: {0}")]
    Refused(String),
}

impl EngineError {
    /// Stable machine-readable error category for the FFI/agent wire format
    /// (`{"kind": …, "message": …}`). Kinds are part of the protocol: shells
    /// branch on them (e.g. `missing_media` → relink flow), so renaming one is
    /// a breaking change.
    pub fn kind(&self) -> &'static str {
        match self {
            EngineError::Model(_) => "model",
            EngineError::Time(_) => "time",
            EngineError::Render(_) => "render",
            EngineError::Decode(_) => "decode",
            EngineError::Io(_) => "io",
            EngineError::Import(_) => "import",
            EngineError::Export(_) => "export",
            EngineError::MissingMedia(_) => "missing_media",
            EngineError::Unsupported(_) => "unsupported",
            EngineError::Refused(_) => "refused",
        }
    }

    /// Whether this is work that was abandoned rather than work that failed.
    ///
    /// A cancelled frame is the expected end of a scrub: the playhead moved
    /// before the frame arrived. The distinction has one owner here so
    /// a shell cannot decide it by matching on a message — reporting a
    /// cancellation as an error would put "cancelled" on the status strip for
    /// every frame a scrub passes over.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, EngineError::Render(RenderError::Cancelled))
    }
}
