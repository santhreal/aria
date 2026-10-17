use aria_models::{ClipId, EnableAssignment, GroupAssignment, LinkAssignment};

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

/// The undoable unit for clip groups (`Clip` > `Group` / `Ungroup`).
pub struct SetClipGroupsAction {
    pub groups: Vec<GroupAssignment>,
}

/// Put `clips` (2+ clips) into one freshly allocated clip group.
pub fn group(
    ctx: &mut ApplyContext<'_>,
    clips: &[ClipId],
) -> Result<Box<dyn EditAction>, EngineError> {
    if clips.len() < 2 {
        return Err(EngineError::Model(aria_models::ModelError::InvalidParam(
            "group requires at least 2 clips".into(),
        )));
    }
    let groups = ctx.project.group_plan(clips)?;
    Box::new(SetClipGroupsAction { groups }).apply(ctx)
}

/// Dissolve every clip group touched by `clips`.
pub fn ungroup(
    ctx: &mut ApplyContext<'_>,
    clips: &[ClipId],
) -> Result<Box<dyn EditAction>, EngineError> {
    let groups = ctx.project.ungroup_plan(clips)?;
    Box::new(SetClipGroupsAction { groups }).apply(ctx)
}

impl EditAction for SetClipGroupsAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let previous = ctx.project.set_clip_groups(&self.groups)?;
        Ok(Box::new(SetClipGroupsAction { groups: previous }))
    }
}

/// The undoable unit for enabling or disabling a set of clips (`Clip` > `Enable`).
pub struct SetClipsEnabledAction {
    pub plan: Vec<EnableAssignment>,
}

/// Set `enabled` on `clips` as one undo step.
pub fn set_enabled(
    ctx: &mut ApplyContext<'_>,
    clips: &[ClipId],
    enabled: bool,
) -> Result<Box<dyn EditAction>, EngineError> {
    let plan = ctx.project.enable_plan(clips, enabled)?;
    Box::new(SetClipsEnabledAction { plan }).apply(ctx)
}

impl EditAction for SetClipsEnabledAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let previous = ctx.project.set_clips_enabled(&self.plan)?;
        Ok(Box::new(SetClipsEnabledAction { plan: previous }))
    }
}
