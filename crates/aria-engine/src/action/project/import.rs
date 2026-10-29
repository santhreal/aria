use std::path::Path;

use aria_models::MediaId;

use crate::action::edit::remove_media::RemoveMediaAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;
use crate::import::import_media;

/// Probe `path` and add it to the media pool, returning the new [`MediaId`] and
/// the inverse that removes it. Re-importing a file already in the pool is a
/// no-op: the existing id is returned with no inverse.
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    path: &Path,
) -> Result<(MediaId, Option<Box<dyn EditAction>>), EngineError> {
    if let Some(existing) = ctx.project.find_media_by_path(path) {
        return Ok((existing, None));
    }

    // `dunce`: `canonicalize` spells the result `\\?\`-verbatim on Windows,
    // which Media Foundation cannot open on mapped drives and which poisons
    // every later path comparison against the pre-canonical form.
    let path = dunce::canonicalize(path).map_err(EngineError::Io)?;
    let media = import_media(&path)?;
    let id = ctx.project.add_media(media);
    Ok((id, Some(Box::new(RemoveMediaAction { media: id }))))
}
