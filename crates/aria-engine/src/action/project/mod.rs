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
/// referenced media path must exist on disk (Open), and the error lists each
/// one that does not; otherwise missing media is tolerated (Load). History is
/// cleared — a fresh document has no undo past.
pub(crate) fn load_session(
    ctx: &mut ApplyContext<'_>,
    path: PathBuf,
    strict: bool,
) -> Result<(), EngineError> {
    let mut loaded = Project::load_from_file(&path)?;
    sync_project_name_from_path(&mut loaded, &path);
    if strict {
        let mut missing: Vec<String> = loaded
            .media_iter()
            .filter(|media| !media.path().exists())
            .map(|media| media.path().display().to_string())
            .collect();
        if !missing.is_empty() {
            missing.sort_unstable();
            missing.dedup();
            return Err(EngineError::MissingMedia(missing));
        }
    }
    *ctx.project = loaded;
    *ctx.project_path = Some(path);
    ctx.history.clear();
    Ok(())
}

/// Adopt the file stem of `path`, ignoring internal shadow files (`untitled.aria`,
/// `recovery.aria`).
pub(crate) fn sync_project_name_from_path(project: &mut Project, path: &std::path::Path) {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(str::trim) else {
        return;
    };
    if stem.is_empty()
        || stem.eq_ignore_ascii_case("untitled")
        || stem.eq_ignore_ascii_case("recovery")
    {
        return;
    }
    project.name = stem.to_owned();
}
