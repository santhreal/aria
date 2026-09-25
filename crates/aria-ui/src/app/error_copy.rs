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
        // The refusal is typed, so the copy answers for each reason and a new
        // one stops compiling here instead of borrowing the wrong sentence.
        EngineError::Render(aria_render::RenderError::Empty(reason)) => match reason {
            aria_render::EmptyExport::Sequence => {
                "Nothing to export. Your timeline is empty. Import media and place a clip first."
                    .into()
            }
            aria_render::EmptyExport::Range => {
                "Nothing to export. The marked range covers no frames. Move the in and out \
                 points apart."
                    .into()
            }
            aria_render::EmptyExport::Silence => {
                "Nothing to export. Nothing in this sequence is audible. Unmute a lane or raise \
                 its fader, then try again."
                    .into()
            }
        },
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
            aria_models::ModelError::InvalidProjectFile(_)
            | aria_models::ModelError::UnsupportedProjectSchema { .. } => {
                "Couldn't open this project. The file may be corrupted. Pick another .aria file."
                    .into()
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
