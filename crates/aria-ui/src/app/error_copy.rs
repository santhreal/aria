//! Human-readable engine errors. Every message names the next action.

use aria_engine::EngineError;

pub(crate) fn describe_engine_error(err: &EngineError) -> String {
    match err {
        EngineError::MissingMedia(path) => {
            format!("Couldn't find media at {path}. Move the file back or click Relink.")
        }
        EngineError::Import(msg) => {
            format!("Couldn't import that file: {msg}. Try another format or check the path.")
        }
        EngineError::Render(aria_render::RenderError::Empty(reason)) => {
            empty_export_copy(*reason).into()
        }
        EngineError::Export(msg) => {
            format!("Couldn't write the export: {msg}. Check the destination and free disk space.")
        }
        EngineError::Decode(_) => {
            "Couldn't decode this file. Use a supported video, audio, or image format.".into()
        }
        EngineError::Io(_) => {
            "Couldn't read or write that file. Check the path and that the disk is not full.".into()
        }
        EngineError::Render(_) => {
            "Couldn't render this frame. Scrub the playhead or reopen the project.".into()
        }
        EngineError::Model(model) => match model {
            aria_models::ModelError::InvalidProjectFile(_) => {
                "Couldn't open this project. The file may be corrupted. Pick another .aria file."
                    .into()
            }
            aria_models::ModelError::UnsupportedProjectSchema { found, expected } => {
                if found.kind == expected.kind && found.version > expected.version {
                    format!(
                        "Couldn't open this project: it uses project format version {}, and this build reads versions 1 to {}. Update Aria to open it.",
                        found.version, expected.version
                    )
                } else {
                    format!(
                        "Couldn't open this project: its format is '{}' version {}, and this build reads '{}' versions 1 to {}. Pick a valid .aria project file.",
                        found.kind, found.version, expected.kind, expected.version
                    )
                }
            }
            _ => "Couldn't apply that edit. Undo, then try again.".into(),
        },
        EngineError::Time(_) => {
            "That time is outside the sequence. Move the playhead and try again.".into()
        }
        EngineError::Unsupported(msg) => format!("This build can't do that yet: {msg}."),
        // The reason already names the obstacle and comes from the plugin that
        // knows it, so it is shown as written rather than wrapped in advice.
        EngineError::Refused(reason) => reason.clone(),
    }
}

/// The copy for an export refused before it starts: an export with nothing to
/// write reads as the engine's copy for the same reason, and any other refusal
/// as the render path words it.
pub(crate) fn describe_export_refusal(err: &aria_render::RenderError) -> String {
    match err {
        aria_render::RenderError::Empty(reason) => empty_export_copy(*reason).into(),
        other => other.to_string(),
    }
}

/// Why an export has nothing to write, and the control that fixes it. The
/// refusal is typed, so the copy answers for each reason and a new one stops
/// compiling here instead of borrowing the wrong sentence.
fn empty_export_copy(reason: aria_render::EmptyExport) -> &'static str {
    match reason {
        aria_render::EmptyExport::Sequence => {
            "Nothing to export. Your timeline is empty. Import media and place a clip first."
        }
        aria_render::EmptyExport::Range => {
            "Nothing to export. The marked range covers no frames. Move the in and out points \
             apart."
        }
        aria_render::EmptyExport::Silence => {
            "Nothing to export. Nothing in this sequence is audible. Unmute a lane or raise its \
             fader, then try again."
        }
    }
}
