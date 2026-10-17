use aria_models::{Patch, SourceChannel};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Restores one source channel's patch on the open sequence.
pub struct SetSourcePatchAction {
    pub channel: SourceChannel,
    pub patch: Patch,
}

impl EditAction for SetSourcePatchAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        execute(ctx, self.channel, self.patch)
    }
}

/// Patch `channel` of the open sequence to `patch`. The inverse restores the
/// channel's previous patch.
pub fn execute(
    ctx: &mut ApplyContext<'_>,
    channel: SourceChannel,
    patch: Patch,
) -> Result<Box<dyn EditAction>, EngineError> {
    let timeline = ctx.project.timeline_mut();
    let previous = timeline.source_patching().get(channel);
    timeline.set_source_patch(channel, patch)?;
    Ok(Box::new(SetSourcePatchAction {
        channel,
        patch: previous,
    }))
}
