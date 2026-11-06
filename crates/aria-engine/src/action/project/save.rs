use std::path::PathBuf;

use crate::action::ApplyContext;
use crate::error::EngineError;

pub(crate) fn execute(ctx: &mut ApplyContext<'_>, path: PathBuf) -> Result<(), EngineError> {
    let previous = ctx.project.name.clone();
    super::sync_project_name_from_path(ctx.project, &path);
    if let Err(err) = ctx.project.save_to_file(&path) {
        ctx.project.name = previous;
        return Err(err.into());
    }
    *ctx.project_path = Some(path);
    Ok(())
}
