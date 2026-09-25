use aria_models::{ClipId, LinkAssignment};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// The undoable unit for clip linkage. Membership rules live in the model
/// (`Project::link_plan` / `unlink_plan`); this adds only the inverse capture,
/// so the group a script forms and the group a command forms are the same
/// group. The inverse oscillates like any snapshot swap.
pub struct SetClipLinksAction {
    pub links: Vec<LinkAssignment>,
}

/// Put `clips` into one freshly allocated link group.
pub fn execute(
    ctx: &mut ApplyContext<'_>,
    clips: &[ClipId],
) -> Result<Box<dyn EditAction>, EngineError> {
    if clips.len() < 2 {
        return Err(EngineError::Model(aria_models::ModelError::InvalidParam(
            "link requires at least 2 clips".into(),
        )));
    }
    let links = ctx.project.link_plan(clips)?;
    Box::new(SetClipLinksAction { links }).apply(ctx)
}

/// Dissolve every link group touched by `clips`.
pub fn unlink(
    ctx: &mut ApplyContext<'_>,
    clips: &[ClipId],
) -> Result<Box<dyn EditAction>, EngineError> {
    let links = ctx.project.unlink_plan(clips)?;
    Box::new(SetClipLinksAction { links }).apply(ctx)
}

impl EditAction for SetClipLinksAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let previous = ctx.project.set_clip_links(&self.links)?;
        Ok(Box::new(SetClipLinksAction { links: previous }))
    }
}
