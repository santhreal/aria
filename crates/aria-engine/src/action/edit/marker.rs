//! Ruler and clip marker edits: add / remove / set / edit, each with an
//! exact inverse so marker gestures oscillate under undo/redo like every
//! other timeline edit.

use aria_models::{ClipId, Marker, MarkerColor, MarkerId, MarkerKind, ModelError, RationalTime};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Add a sequence marker at `at`. An omitted color cycles the fixed palette by
/// the current marker count, so consecutive unstyled markers stay distinct.
pub fn add(
    ctx: &mut ApplyContext<'_>,
    at: RationalTime,
    name: String,
    color: Option<MarkerColor>,
) -> Result<(MarkerId, Box<dyn EditAction>), EngineError> {
    let timeline = ctx.project.timeline_mut();
    let color = color.unwrap_or_else(|| MarkerColor::cycle(timeline.marker_count()));
    let id = timeline.add_marker(Marker::new(at, name, color))?;
    Ok((id, Box::new(RemoveMarkerAction { marker: id })))
}

/// Add a clip marker on `clip` at `at` (in clip source time).
pub fn add_on_clip(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    at: RationalTime,
    name: String,
    color: Option<MarkerColor>,
) -> Result<(MarkerId, Box<dyn EditAction>), EngineError> {
    let timeline = ctx.project.timeline_mut();
    let color = color.unwrap_or_else(|| MarkerColor::cycle(timeline.marker_count()));
    let id = timeline.add_clip_marker(clip, Marker::new(at, name, color))?;
    Ok((id, Box::new(RemoveMarkerAction { marker: id })))
}

/// Move / rename / recolor / re-annotate a marker (preserving its existing
/// duration and kind); the inverse restores its prior state.
pub fn set(
    ctx: &mut ApplyContext<'_>,
    marker: MarkerId,
    at: RationalTime,
    name: String,
    color: MarkerColor,
    comment: String,
) -> Result<Box<dyn EditAction>, EngineError> {
    let timeline = ctx.project.timeline_mut();
    let before = timeline
        .marker(marker)
        .cloned()
        .ok_or(ModelError::UnknownMarker(marker))?;
    timeline.set_marker(marker, at, name, color, comment)?;
    Ok(Box::new(ReplaceMarkerAction { before }))
}

/// Update all fields (`at`, `duration`, `name`, `color`, `comment`, `kind`) of
/// a sequence or clip marker as a single undoable step.
#[allow(clippy::too_many_arguments)]
pub fn edit(
    ctx: &mut ApplyContext<'_>,
    marker: MarkerId,
    at: RationalTime,
    duration: RationalTime,
    name: String,
    color: MarkerColor,
    comment: String,
    kind: MarkerKind,
) -> Result<Box<dyn EditAction>, EngineError> {
    let timeline = ctx.project.timeline_mut();
    let before = timeline
        .marker(marker)
        .cloned()
        .ok_or(ModelError::UnknownMarker(marker))?;
    timeline.edit_marker(marker, at, duration, name, color, comment, kind)?;
    Ok(Box::new(ReplaceMarkerAction { before }))
}

/// Remove a marker (sequence or clip); applying returns the restore (same id
/// and owning clip, if any) as the inverse.
pub struct RemoveMarkerAction {
    pub marker: MarkerId,
}

impl EditAction for RemoveMarkerAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let timeline = ctx.project.timeline_mut();
        let clip = timeline.marker_clip(self.marker);
        let removed = timeline
            .remove_marker(self.marker)
            .ok_or(ModelError::UnknownMarker(self.marker))?;
        Ok(Box::new(RestoreMarkerAction {
            clip,
            marker: removed,
        }))
    }
}

/// Re-insert a removed marker snapshot (undo of remove).
struct RestoreMarkerAction {
    clip: Option<ClipId>,
    marker: Marker,
}

impl EditAction for RestoreMarkerAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let timeline = ctx.project.timeline_mut();
        let id = match self.clip {
            Some(clip_id) => timeline.add_clip_marker(clip_id, self.marker)?,
            None => timeline.add_marker(self.marker)?,
        };
        Ok(Box::new(RemoveMarkerAction { marker: id }))
    }
}

/// Swap a marker back to a captured snapshot (set/edit-marker undo/redo).
struct ReplaceMarkerAction {
    before: Marker,
}

impl EditAction for ReplaceMarkerAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let id = self.before.id;
        let timeline = ctx.project.timeline_mut();
        let current = timeline
            .marker(id)
            .cloned()
            .ok_or(ModelError::UnknownMarker(id))?;
        timeline.edit_marker(
            id,
            self.before.tick,
            self.before.duration,
            self.before.name,
            self.before.color,
            self.before.comment,
            self.before.kind,
        )?;
        Ok(Box::new(ReplaceMarkerAction { before: current }))
    }
}
