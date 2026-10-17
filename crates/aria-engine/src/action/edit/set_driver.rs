use aria_models::{ClipId, ClipParam, DriverCode, ModelError};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Attach (or with `None` remove) the driver on `param` of `clip`.
///
/// A driver that would read its own value back through other drivers is
/// refused here, before it reaches the project. Code that does not compile,
/// or names a language no plugin registers, is attached: it is reported and
/// skipped at render until it compiles, so a project opened without a
/// language plugin keeps its drivers. The inverse is a full-clip restore.
pub fn set_driver(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    param: ClipParam,
    code: Option<DriverCode>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    if let Some(code) = &code
        && aria_render::compile_driver(code).is_ok()
    {
        aria_render::check_driver(ctx.project, clip, param, code)
            .map_err(ModelError::InvalidParam)?;
    }
    ctx.project.set_param_driver(clip, param, code)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}
