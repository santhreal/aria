//! Session-level project commands (Import/RelinkMedia/Save/Open/Load and the
//! template pair SaveTemplate/ApplyTemplate).
//!
//! Import and relink probe media through the native, FFmpeg-free decoder
//! ([`crate::import::import_media`]) — the same codec used for preview/export
//! reports the metadata.

use std::path::PathBuf;

use aria_models::Project;

use crate::action::ApplyContext;
use crate::error::EngineError;

pub(crate) mod apply_template;
pub(crate) mod import;
pub(crate) mod load;
pub(crate) mod open;
pub(crate) mod relink;
pub(crate) mod save;
pub(crate) mod save_template;
pub(crate) mod set_media_label;

/// Replace the session project from a `.aria` file. With `strict`, every
/// referenced media path must exist on disk (Open); otherwise missing media is
/// tolerated (Load). History is cleared — a fresh document has no undo past.
pub(crate) fn load_session(
    ctx: &mut ApplyContext<'_>,
    path: PathBuf,
    strict: bool,
) -> Result<(), EngineError> {
    let loaded = Project::load_from_file(&path)?;
    if strict {
        for media in loaded.media_iter() {
            if !media.path().exists() {
                return Err(EngineError::MissingMedia(
                    media.path().display().to_string(),
                ));
            }
        }
    }
    *ctx.project = loaded;
    *ctx.project_path = Some(path);
    ctx.history.clear();
    Ok(())
}
