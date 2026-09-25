//! Multicam camera group actions: create group, place multicam clip, switch
//! active angle, move an angle on the group's clock.

use aria_models::{
    ClipId, MulticamAngle, MulticamCut, MulticamGroup, MulticamGroupId, RationalTime, TimeRange,
    TrackId,
};

use crate::action::edit::remove_clip::RemoveClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Create a new multicam group in the project. The inverse removes it.
pub fn create_group(
    ctx: &mut ApplyContext<'_>,
    name: String,
    angles: Vec<MulticamAngle>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let id = ctx.project.create_multicam_group(name, angles)?;
    Ok(Box::new(DropMulticamGroupAction { id }))
}

/// Place a multicam clip on a track. The inverse removes the clip.
pub fn add_multicam_clip(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    group: MulticamGroupId,
    source: TimeRange,
    start: RationalTime,
) -> Result<(ClipId, Box<dyn EditAction>), EngineError> {
    let id = ctx.project.add_multicam_clip(track, group, source, start)?;
    Ok((id, Box::new(RemoveClipAction { clip: id })))
}

/// Switch the active angle of a multicam group at a given tick.
/// The inverse restores the previous cuts configuration of the group.
pub fn switch_angle(
    ctx: &mut ApplyContext<'_>,
    group_id: MulticamGroupId,
    at: i64,
    angle: usize,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before_cuts = ctx
        .project
        .multicam_group(group_id)
        .ok_or(aria_models::ModelError::UnknownMulticamGroup(group_id))?
        .cuts
        .to_vec();

    ctx.project.switch_multicam_angle(group_id, at, angle)?;

    Ok(Box::new(RestoreMulticamCutsAction {
        group_id,
        cuts: before_cuts,
    }))
}

/// Move one angle on the group's common clock.
/// The inverse puts back the offset this call replaced.
pub fn set_angle_offset(
    ctx: &mut ApplyContext<'_>,
    group_id: MulticamGroupId,
    angle: usize,
    offset: i64,
) -> Result<Box<dyn EditAction>, EngineError> {
    let previous = ctx
        .project
        .set_multicam_angle_offset(group_id, angle, offset)?;
    Ok(Box::new(RestoreMulticamOffsetAction {
        group_id,
        angle,
        offset: previous,
    }))
}

struct DropMulticamGroupAction {
    id: MulticamGroupId,
}

impl EditAction for DropMulticamGroupAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let removed = ctx.project.remove_multicam_group(self.id)?;
        Ok(Box::new(RestoreMulticamGroupAction { group: removed }))
    }
}

struct RestoreMulticamGroupAction {
    group: MulticamGroup,
}

impl EditAction for RestoreMulticamGroupAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let id = self.group.id;
        ctx.project.add_multicam_group(self.group)?;
        Ok(Box::new(DropMulticamGroupAction { id }))
    }
}

struct RestoreMulticamCutsAction {
    group_id: MulticamGroupId,
    cuts: Vec<MulticamCut>,
}

impl EditAction for RestoreMulticamCutsAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let group = ctx
            .project
            .multicam_group_mut(self.group_id)
            .ok_or(aria_models::ModelError::UnknownMulticamGroup(self.group_id))?;
        let previous = std::mem::replace(&mut group.cuts, self.cuts);
        Ok(Box::new(RestoreMulticamCutsAction {
            group_id: self.group_id,
            cuts: previous,
        }))
    }
}

struct RestoreMulticamOffsetAction {
    group_id: MulticamGroupId,
    angle: usize,
    offset: i64,
}

impl EditAction for RestoreMulticamOffsetAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let previous =
            ctx.project
                .set_multicam_angle_offset(self.group_id, self.angle, self.offset)?;
        Ok(Box::new(RestoreMulticamOffsetAction {
            group_id: self.group_id,
            angle: self.angle,
            offset: previous,
        }))
    }
}
