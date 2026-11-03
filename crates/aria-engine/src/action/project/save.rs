use std::path::PathBuf;

use crate::action::ApplyContext;
use crate::error::EngineError;

pub(crate) fn execute(ctx: &mut ApplyContext<'_>, path: PathBuf) -> Result<(), EngineError> {
    super::sync_project_name_from_path(ctx.project, &path);
    ctx.project.save_to_file(&path)?;
    *ctx.project_path = Some(path);
    Ok(())
}
