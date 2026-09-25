use aria_models::{
    Clip, ClipId, ModelError, RationalTime, Sequence, SequenceId, TimeRange, Track, TrackId,
};

use crate::action::edit::remove_clip::RemoveClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Action that undoes the creation of a nested sequence from a selection of clips.
pub struct UndoCreateNestedSequenceAction {
    pub sequence_id: SequenceId,
    pub nested_clip_id: ClipId,
    pub original_clips: Vec<(TrackId, Clip)>,
}

/// Action that redoes the creation of a nested sequence from a selection of clips.
pub struct RedoCreateNestedSequenceAction {
    pub sequence: Sequence,
    pub nested_clip: Clip,
    pub track_id: TrackId,
    pub original_clip_ids: Vec<ClipId>,
}

impl EditAction for UndoCreateNestedSequenceAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let nested_clip = ctx
            .project
            .timeline_mut()
            .remove_clip(self.nested_clip_id)
            .ok_or(ModelError::UnknownClip(self.nested_clip_id))?;
        let track_id = ctx
            .project
            .timeline()
            .track_of(self.nested_clip_id)
            .unwrap_or_else(|| self.original_clips[0].0);
        let sequence = ctx
            .project
            .remove_sequence(self.sequence_id)
            .ok_or(ModelError::UnknownSequence(self.sequence_id))?;

        let mut original_clip_ids = Vec::with_capacity(self.original_clips.len());
        for (trk_id, clip) in self.original_clips {
            original_clip_ids.push(clip.id);
            ctx.project.timeline_mut().add_clip(trk_id, clip)?;
        }

        Ok(Box::new(RedoCreateNestedSequenceAction {
            sequence,
            nested_clip,
            track_id,
            original_clip_ids,
        }))
    }
}

impl EditAction for RedoCreateNestedSequenceAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let mut original_clips = Vec::with_capacity(self.original_clip_ids.len());
        for id in &self.original_clip_ids {
            let trk_id = ctx
                .project
                .timeline()
                .track_of(*id)
                .ok_or(ModelError::UnknownClip(*id))?;
            let clip = ctx
                .project
                .timeline_mut()
                .remove_clip(*id)
                .ok_or(ModelError::UnknownClip(*id))?;
            original_clips.push((trk_id, clip));
        }

        let sequence_id = self.sequence.id;
        ctx.project.add_sequence(self.sequence);
        let nested_clip_id = self.nested_clip.id;
        ctx.project
            .timeline_mut()
            .add_clip(self.track_id, self.nested_clip)?;

        Ok(Box::new(UndoCreateNestedSequenceAction {
            sequence_id,
            nested_clip_id,
            original_clips,
        }))
    }
}

/// Create a nested sequence from a selection of clips on the active timeline.
pub fn create_nested_sequence(
    ctx: &mut ApplyContext<'_>,
    name: String,
    clips: &[ClipId],
) -> Result<(ClipId, Box<dyn EditAction>), EngineError> {
    if clips.is_empty() {
        return Err(EngineError::Model(ModelError::InvalidParam(
            "cannot create a nested sequence from an empty clip selection".into(),
        )));
    }

    let tl = ctx.project.timeline();
    let mut min_start = i64::MAX;
    let mut max_end = i64::MIN;
    let mut original_clips = Vec::with_capacity(clips.len());
    let mut first_track_id = None;

    for &id in clips {
        let clip = tl.clip(id).ok_or(ModelError::UnknownClip(id))?;
        let trk_id = tl.track_of(id).ok_or(ModelError::UnknownClip(id))?;
        if first_track_id.is_none() {
            first_track_id = Some(trk_id);
        }
        min_start = min_start.min(clip.timeline.start.value);
        max_end = max_end.max(clip.timeline.end_tick());
        original_clips.push((trk_id, Clip::clone(clip)));
    }

    let first_track_id = first_track_id.expect("non-empty clips");
    let tl_rate = ctx.project.timeline().frame_rate;
    let duration = max_end - min_start;
    if duration <= 0 {
        return Err(EngineError::Model(ModelError::InvalidRange));
    }

    let seq_name = if name.is_empty() {
        "Nested Sequence".to_string()
    } else {
        name
    };
    let mut new_seq = Sequence::new(seq_name, tl_rate);

    // Replicate each source lane once, in id order, so a nested sequence built
    // from the same selection twice has the same lane order.
    let mut track_map: std::collections::BTreeMap<TrackId, TrackId> =
        std::collections::BTreeMap::new();
    for (trk_id, clip) in &original_clips {
        let mapped_trk_id = match track_map.get(trk_id) {
            Some(mapped) => *mapped,
            None => {
                let source = ctx
                    .project
                    .timeline()
                    .track(*trk_id)
                    .ok_or(ModelError::UnknownTrack(*trk_id))?;
                let lane = Track::new(source.kind, source.name.to_string());
                let mapped = new_seq.timeline.add_track(lane);
                track_map.insert(*trk_id, mapped);
                mapped
            }
        };
        let mut shifted_clip = Clip::clone(clip);
        shifted_clip.timeline.start.value -= min_start;
        new_seq.timeline.add_clip(mapped_trk_id, shifted_clip)?;
    }

    let source = TimeRange::at_rate(0, duration, tl_rate);
    let timeline = TimeRange::at_rate(min_start, duration, tl_rate);
    let nested_clip = Clip::from_sequence(new_seq.id, source, timeline);

    // The nested clip lands on the lane the selection came from, and only a
    // media-bearing lane accepts one. Refuse before the project is touched: the
    // same check inside `add_clip` fires after the sequence is registered and
    // the selection removed, which would leave the clips deleted and an orphan
    // sequence behind. Nothing above this point has mutated the project.
    let dest = ctx
        .project
        .timeline()
        .track(first_track_id)
        .ok_or(ModelError::UnknownTrack(first_track_id))?;
    if !dest.kind.accepts_clip(&nested_clip) {
        return Err(EngineError::Model(ModelError::IncompatibleTrackKind {
            track: first_track_id,
            kind: dest.kind,
        }));
    }

    let sequence_id = ctx.project.add_sequence(new_seq);

    for &id in clips {
        ctx.project.timeline_mut().remove_clip(id);
    }

    let nested_clip_id = ctx
        .project
        .timeline_mut()
        .add_clip(first_track_id, nested_clip)?;

    Ok((
        nested_clip_id,
        Box::new(UndoCreateNestedSequenceAction {
            sequence_id,
            nested_clip_id,
            original_clips,
        }),
    ))
}

/// Place a secondary sequence on a track of the active timeline.
pub fn place_nested_sequence(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    sequence: SequenceId,
    source: TimeRange,
    start: RationalTime,
) -> Result<(ClipId, Box<dyn EditAction>), EngineError> {
    let clip_id = ctx
        .project
        .add_sequence_clip(track, sequence, source, start)?;
    Ok((clip_id, Box::new(RemoveClipAction { clip: clip_id })))
}

/// Action to switch active sequence in undo/redo.
pub struct OpenSequenceAction {
    pub previous: Option<SequenceId>,
}

impl EditAction for OpenSequenceAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let current = ctx.project.active_sequence();
        ctx.project.set_active_sequence(self.previous)?;
        Ok(Box::new(OpenSequenceAction { previous: current }))
    }
}

/// Open a sequence for editing (`None` ⇔ main root timeline).
pub fn open_sequence(
    ctx: &mut ApplyContext<'_>,
    sequence: Option<SequenceId>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let previous = ctx.project.active_sequence();
    ctx.project.set_active_sequence(sequence)?;
    Ok(Box::new(OpenSequenceAction { previous }))
}
