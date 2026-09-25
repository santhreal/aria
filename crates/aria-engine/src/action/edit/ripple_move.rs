use aria_models::{ClipId, ModelError, RationalTime, TimeRange, TrackId};

use crate::action::edit::insert_clip::InsertClipAction;
use crate::action::edit::remove_clip::RemoveClipAction;
use crate::action::edit::shift_clips;
use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

/// One clip in a ripple-move, including linked companions.
struct Member {
    id: ClipId,
    from_track: TrackId,
    to_track: TrackId,
    from_end: RationalTime,
    duration: RationalTime,
    dest: RationalTime,
}

/// Lift a clip, close the hole it left, open a hole at the drop, and place it
/// there. Linked companions take the same time delta and stay on their lanes.
/// One history entry; undo is the inverses in reverse.
pub fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    to_track: TrackId,
    start: RationalTime,
) -> Result<Box<dyn EditAction>, EngineError> {
    let mut group = members(ctx, clip, to_track, start)?;
    // Later origins first so a same-lane companion's hole close cannot shift
    // an earlier origin we still need to close.
    group.sort_by_key(|member| std::cmp::Reverse(member.from_end.value));

    let mut inverses: Vec<Box<dyn EditAction>> = Vec::new();
    let mut lifted = Vec::with_capacity(group.len());
    for member in &group {
        match ctx.project.remove_clip(member.id) {
            Some(clip) => {
                inverses.push(Box::new(InsertClipAction {
                    track: member.from_track,
                    clip: clip.clone(),
                }));
                lifted.push((member, clip));
            }
            None => {
                rollback(ctx, inverses);
                return Err(EngineError::from(ModelError::UnknownClip(member.id)));
            }
        }
    }

    // Lanes a member leaves close through their own `place`. Every other
    // sync-locked lane closes the same spans here, so its material stays in
    // step with the move — the rule ripple delete already applies. Without
    // this, Shift-dragging linked picture moved the sound lane nowhere: the
    // picture hole closed, but the sound kept sitting at the old ticks.
    let mut lanes: Vec<TrackId> = group.iter().map(|m| m.from_track).collect();
    lanes.sort_by_key(|t| t.raw());
    lanes.dedup();

    // Later spans first, matching the per-lane ordering `place` uses, so an
    // earlier span's ticks are not disturbed before it is closed.
    let mut spans: Vec<(RationalTime, RationalTime)> = group
        .iter()
        .map(|m| {
            (
                RationalTime::new(m.from_end.value - m.duration.value, m.duration.rate),
                m.from_end,
            )
        })
        .collect();
    spans.sort_by_key(|(from, _)| std::cmp::Reverse(from.value));
    for (from, to) in &spans {
        if let Err(err) =
            shift_clips::close_for_ripple_delete(ctx, &lanes, *from, *to, &mut inverses)
        {
            rollback(ctx, inverses);
            return Err(err);
        }
    }

    // The drop opens its hole on the rider lanes too, so what follows the
    // moved material makes room on every lane in step — the insert-side half
    // of the same rule. Companions follow the primary's delta, so the
    // primary's destination span is the one the gesture opened.
    let primary = group.iter().find(|m| m.id == clip).expect("primary member");
    // Mirror `place`'s same-lane landing adjustment: a forward move along one
    // lane lands earlier by its own duration once the hole behind it closed,
    // and that landed position is what every lane makes room at.
    let mut drop = primary.dest;
    if primary.from_track == primary.to_track && drop.value >= primary.from_end.value {
        drop = RationalTime::new(drop.value - primary.duration.value, drop.rate);
    }
    let rider_lanes: Vec<TrackId> = ctx
        .project
        .timeline()
        .tracks_ordered()
        .filter(|t| t.sync_lock && !lanes.contains(&t.id))
        .map(|t| t.id)
        .collect();
    for lane in rider_lanes {
        if let Err(err) = shift_clips::execute(ctx, lane, drop, primary.duration)
            .map(|action| inverses.push(action))
        {
            rollback(ctx, inverses);
            return Err(err);
        }
    }

    for (member, clip) in lifted {
        if let Err(err) = place(
            ctx,
            clip,
            member.from_track,
            member.from_end,
            member.duration,
            member.to_track,
            member.dest,
            &mut inverses,
        ) {
            rollback(ctx, inverses);
            return Err(err);
        }
    }

    Ok(Box::new(CompoundAction { actions: inverses }))
}

fn members(
    ctx: &ApplyContext<'_>,
    clip: ClipId,
    to_track: TrackId,
    start: RationalTime,
) -> Result<Vec<Member>, EngineError> {
    let primary = ctx
        .project
        .clip(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let delta = start.value - primary.start().value;
    let ids = match primary.link {
        Some(link) => ctx.project.linked_clips(link),
        None => vec![clip],
    };
    let mut group = Vec::with_capacity(ids.len());
    for id in ids {
        let original = ctx.project.clip(id).ok_or(ModelError::UnknownClip(id))?;
        let from_track = ctx
            .project
            .timeline()
            .track_of(id)
            .ok_or(ModelError::UnknownClip(id))?;
        let duration = original.timeline.duration;
        let from_end = RationalTime::new(
            original.start().value + duration.value,
            original.start().rate,
        );
        let dest = if id == clip {
            start
        } else {
            RationalTime::new((original.start().value + delta).max(0), start.rate)
        };
        group.push(Member {
            id,
            from_track,
            to_track: if id == clip { to_track } else { from_track },
            from_end,
            duration,
            dest,
        });
    }
    Ok(group)
}

fn rollback(ctx: &mut ApplyContext<'_>, inverses: Vec<Box<dyn EditAction>>) {
    for inverse in inverses.into_iter().rev() {
        if inverse.apply(ctx).is_err() {
            tracing::error!("ripple-move rollback failed");
            break;
        }
    }
}

fn place(
    ctx: &mut ApplyContext<'_>,
    mut lifted: aria_models::Clip,
    from_track: TrackId,
    from_end: RationalTime,
    duration: RationalTime,
    to_track: TrackId,
    start: RationalTime,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<(), EngineError> {
    inverses.push(shift_clips::execute(
        ctx,
        from_track,
        from_end,
        RationalTime::new(-duration.value, duration.rate),
    )?);

    let mut dest = start;
    if from_track == to_track && dest.value >= from_end.value {
        dest = RationalTime::new(dest.value - duration.value, dest.rate);
    }
    dest = RationalTime::new(dest.value.max(0), dest.rate);

    inverses.push(shift_clips::execute(ctx, to_track, dest, duration)?);

    let id = lifted.id;
    lifted.timeline = TimeRange::at_rate(dest.value, duration.value, duration.rate);
    ctx.project.timeline_mut().add_clip(to_track, lifted)?;
    inverses.push(Box::new(RemoveClipAction { clip: id }));
    Ok(())
}
