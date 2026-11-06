use aria_models::{
    ClipId, ModelError, RationalTime, TimeError, TimeRange, TrackId, check_same_rate,
};

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
    from_start: i64,
    from_end: RationalTime,
    duration: RationalTime,
    dest: RationalTime,
}

/// Lift a clip, close the hole it left, open a hole at the drop, and place it
/// there. Linked companions take the same time delta and stay on their lanes.
/// One history entry; undo is the inverses in reverse.
pub(crate) fn execute(
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

    let mut from_lanes: Vec<TrackId> = group.iter().map(|m| m.from_track).collect();
    from_lanes.sort_by_key(|t| t.raw());
    from_lanes.dedup();

    // Close each member's origin hole on its own lane first (later origins
    // first), then close the deduplicated spans once on every other
    // sync-locked lane so picture and sound stay in step.
    for (member, _) in &lifted {
        if let Err(err) = shift_clips::execute(
            ctx,
            member.from_track,
            member.from_end,
            RationalTime::new(-member.duration.value, member.duration.rate),
        )
        .map(|action| inverses.push(action))
        {
            rollback(ctx, inverses);
            return Err(err);
        }
    }

    let mut spans: Vec<(RationalTime, RationalTime)> = group
        .iter()
        .map(|m| (RationalTime::new(m.from_start, m.duration.rate), m.from_end))
        .collect();
    spans.sort_by_key(|(from, to)| (std::cmp::Reverse(from.value), std::cmp::Reverse(to.value)));
    spans.dedup();
    for (from, to) in &spans {
        if let Err(err) =
            shift_clips::close_for_ripple_delete(ctx, &from_lanes, *from, *to, &mut inverses)
        {
            rollback(ctx, inverses);
            return Err(err);
        }
    }

    // The destination lane closed the primary's origin span when it is the
    // primary's origin lane, when a companion on the destination lane closed
    // that span, or when the destination lane rode along via sync lock. A
    // forward drop past the old end lands earlier by the primary's duration on
    // that closed timeline, and every companion takes the same landed delta.
    let primary = group
        .iter()
        .find(|m| m.id == clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let dest_closed_origin = group.iter().any(|m| {
        m.to_track == primary.to_track
            && m.from_track == primary.to_track
            && m.from_start == primary.from_start
            && m.from_end.value == primary.from_end.value
    }) || (!from_lanes.contains(&primary.to_track)
        && ctx
            .project
            .timeline()
            .track(primary.to_track)
            .is_some_and(|track| track.sync_lock));
    let mut drop = primary.dest;
    if dest_closed_origin && drop.value >= primary.from_end.value {
        drop = RationalTime::new(drop.value.saturating_sub(primary.duration.value), drop.rate);
    }
    drop = RationalTime::new(drop.value.max(0), drop.rate);
    let landed_delta = drop
        .value
        .checked_sub(primary.from_start)
        .ok_or(TimeError::Overflow)?;

    let mut to_lanes: Vec<TrackId> = group.iter().map(|m| m.to_track).collect();
    to_lanes.sort_by_key(|t| t.raw());
    to_lanes.dedup();
    let open_riders = shift_clips::rider_lanes(ctx.project.timeline(), &to_lanes);

    let mut placements: Vec<(&Member, aria_models::Clip, RationalTime)> = lifted
        .into_iter()
        .map(|(member, clip)| {
            let dest = if member.id == primary.id {
                drop
            } else {
                RationalTime::new(
                    member.from_start.saturating_add(landed_delta).max(0),
                    member.dest.rate,
                )
            };
            (member, clip, dest)
        })
        .collect();
    placements.sort_by_key(|(_, _, dest)| dest.value);

    // Cut straddling clips at the primary drop across both destination lanes
    // landing at `drop` and sync-locked rider lanes together so linked
    // straddlers keep their split tails linked.
    let mut cut_at_drop: Vec<TrackId> = placements
        .iter()
        .filter(|(_, _, dest)| dest.value == drop.value)
        .map(|(member, _, _)| member.to_track)
        .chain(open_riders.iter().copied())
        .collect();
    cut_at_drop.sort_by_key(|t| t.raw());
    cut_at_drop.dedup();
    if let Err(err) = shift_clips::cut_at(ctx, &cut_at_drop, drop, &mut inverses) {
        rollback(ctx, inverses);
        return Err(err);
    }
    for lane in open_riders {
        if let Err(err) = shift_clips::execute(ctx, lane, drop, primary.duration)
            .map(|action| inverses.push(action))
        {
            rollback(ctx, inverses);
            return Err(err);
        }
    }

    for (member, clip, dest) in placements {
        if let Err(err) = place(
            ctx,
            clip,
            member.duration,
            member.to_track,
            dest,
            dest.value != drop.value,
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
    let tl_rate = ctx.project.timeline().frame_rate;
    check_same_rate(start.rate, tl_rate)?;
    if start.value < 0 {
        return Err(ModelError::InvalidRange.into());
    }
    let primary = ctx.project.require_clip(clip)?;
    let delta = start
        .value
        .checked_sub(primary.start().value)
        .ok_or(TimeError::Overflow)?;
    let ids = ctx.project.move_companions(clip);
    let mut group = Vec::with_capacity(ids.len());
    for id in ids {
        let original = ctx.project.require_clip(id)?;
        let from_track = ctx
            .project
            .timeline()
            .track_of(id)
            .ok_or(ModelError::UnknownClip(id))?;
        let duration = original.timeline.duration;
        duration.value.checked_neg().ok_or(TimeError::Overflow)?;
        let from_end_tick = original
            .start()
            .value
            .checked_add(duration.value)
            .ok_or(TimeError::Overflow)?;
        let from_end = RationalTime::new(from_end_tick, original.start().rate);
        let dest = if id == clip {
            start
        } else {
            RationalTime::new(
                original.start().value.saturating_add(delta).max(0),
                start.rate,
            )
        };
        dest.value
            .checked_add(duration.value)
            .ok_or(TimeError::Overflow)?;
        let from_start = original.start().value;
        group.push(Member {
            id,
            from_track,
            to_track: if id == clip { to_track } else { from_track },
            from_start,
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
    duration: RationalTime,
    to_track: TrackId,
    dest: RationalTime,
    needs_cut: bool,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<(), EngineError> {
    if needs_cut {
        shift_clips::cut_at(ctx, &[to_track], dest, inverses)?;
    }
    inverses.push(shift_clips::execute(ctx, to_track, dest, duration)?);

    let id = lifted.id;
    lifted.timeline = TimeRange::at_rate(dest.value, duration.value, duration.rate);
    ctx.project.timeline_mut().add_clip(to_track, lifted)?;
    inverses.push(Box::new(RemoveClipAction { clip: id }));
    Ok(())
}

#[cfg(test)]
mod tests {
    //! WHY: Closes the class of multi-lane ripple-move desynchronizations where
    //! linked companions sharing a timeline span closed sync-locked rider lanes
    //! once per companion (deleting and shifting $N\times$ the moved span),
    //! cross-track ripple moves treated `to_track` as both a rider lane and a
    //! destination lane while skipping `from_track` and computing different
    //! post-close landing deltas for linked companions, and straddling clips on
    //! destination or rider lanes were not split at the landing tick.
    //! Does not catch UI drag-ghost preview calculation before mouse release.

    use aria_commands::{Command, EditCommand, EditOutcome};
    use aria_models::{Generator, Rational, RationalTime, TimeRange, TrackId, TrackKind};

    use crate::{ApplyOutcome, Engine, EngineConfig};

    fn rt(ticks: i64) -> RationalTime {
        RationalTime::new(ticks, Rational::FPS_24)
    }

    fn tr(start: i64, duration: i64) -> TimeRange {
        TimeRange::at_rate(start, duration, Rational::FPS_24)
    }

    fn add_lane(engine: &mut Engine, kind: TrackKind, name: &str) -> TrackId {
        match engine
            .apply(Command::Edit(EditCommand::AddTrack {
                kind,
                name: name.into(),
                index: None,
                pinned: false,
            }))
            .expect("add track")
        {
            ApplyOutcome::Edited(EditOutcome::CreatedTrack(id)) => id,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn add_bar(
        engine: &mut Engine,
        track: TrackId,
        start: i64,
        duration: i64,
    ) -> aria_models::ClipId {
        match engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(start, duration),
            }))
            .expect("add bar")
        {
            ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn span(engine: &Engine, clip: aria_models::ClipId) -> (i64, i64) {
        let c = engine.project().clip(clip).expect("clip exists");
        (c.timeline.start.value, c.timeline.end_tick())
    }

    fn lane_spans(engine: &Engine, track: TrackId) -> Vec<(i64, i64)> {
        engine
            .project()
            .timeline()
            .track(track)
            .expect("track exists")
            .clips_ordered()
            .map(|c| (c.timeline.start.value, c.timeline.end_tick()))
            .collect()
    }

    #[test]
    fn ripple_move_of_linked_pair_closes_sync_locked_rider_lane_once() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine");
        let v1 = add_lane(&mut engine, TrackKind::Adjustment, "V1");
        let a1 = add_lane(&mut engine, TrackKind::Adjustment, "A1");
        let rider = add_lane(&mut engine, TrackKind::Adjustment, "V2");

        let pic_a = add_bar(&mut engine, v1, 0, 24);
        let pic_b = add_bar(&mut engine, v1, 24, 24);
        let snd_a = add_bar(&mut engine, a1, 0, 24);
        let snd_b = add_bar(&mut engine, a1, 24, 24);
        engine
            .apply(Command::Edit(EditCommand::LinkClips {
                clips: vec![pic_a, snd_a],
            }))
            .expect("link A");

        let r1 = add_bar(&mut engine, rider, 0, 24);
        let r2 = add_bar(&mut engine, rider, 24, 24);
        let r3 = add_bar(&mut engine, rider, 48, 24);

        engine
            .apply(Command::Edit(EditCommand::RippleMoveClip {
                clip: pic_a,
                to_track: v1,
                start: rt(48),
            }))
            .expect("ripple move linked pair");

        assert_eq!(span(&engine, pic_b), (0, 24));
        assert_eq!(span(&engine, pic_a), (24, 48));
        assert_eq!(span(&engine, snd_b), (0, 24));
        assert_eq!(span(&engine, snd_a), (24, 48));
        assert!(
            engine.project().clip(r1).is_none(),
            "rider clip under old span [0,24) is closed"
        );
        assert_eq!(
            span(&engine, r2),
            (0, 24),
            "second rider clip must shift once to [0,24), not be deleted by a duplicate close"
        );
        assert_eq!(
            span(&engine, r3),
            (48, 72),
            "third rider clip shifts left to 24 on close and back to 48 when the landing hole opens"
        );

        assert!(engine.undo());
        assert_eq!(span(&engine, r1), (0, 24));
        assert_eq!(span(&engine, r2), (24, 48));
        assert_eq!(span(&engine, r3), (48, 72));
    }

    #[test]
    fn cross_track_ripple_move_keeps_linked_companion_and_destination_lane_in_sync() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine");
        let v1 = add_lane(&mut engine, TrackKind::Adjustment, "V1");
        let v2 = add_lane(&mut engine, TrackKind::Adjustment, "V2");
        let a1 = add_lane(&mut engine, TrackKind::Adjustment, "A1");

        let pic_a = add_bar(&mut engine, v1, 0, 24);
        let pic_b = add_bar(&mut engine, v1, 24, 48);
        let snd_a = add_bar(&mut engine, a1, 0, 24);
        let snd_b = add_bar(&mut engine, a1, 24, 48);
        let v2_after = add_bar(&mut engine, v2, 72, 24);
        engine
            .apply(Command::Edit(EditCommand::LinkClips {
                clips: vec![pic_a, snd_a],
            }))
            .expect("link A");

        // Move pic_a from V1 to V2 at pre-close tick 72 (after pic_b [24,72)).
        engine
            .apply(Command::Edit(EditCommand::RippleMoveClip {
                clip: pic_a,
                to_track: v2,
                start: rt(72),
            }))
            .expect("cross-track ripple move");

        assert_eq!(
            engine.project().timeline().track_of(pic_a),
            Some(v2),
            "primary moved to V2"
        );
        assert_eq!(
            span(&engine, pic_a),
            (48, 72),
            "primary lands at post-close tick 48 on V2"
        );
        assert_eq!(
            span(&engine, snd_a),
            (48, 72),
            "linked audio companion lands at the exact same post-close tick 48 on A1"
        );
        assert_eq!(span(&engine, pic_b), (0, 48));
        assert_eq!(span(&engine, snd_b), (0, 48));
        assert_eq!(
            span(&engine, v2_after),
            (72, 96),
            "destination lane clip after the drop shifts right once after closing the origin span"
        );

        assert!(engine.undo());
        assert_eq!(engine.project().timeline().track_of(pic_a), Some(v1));
        assert_eq!(span(&engine, pic_a), (0, 24));
        assert_eq!(span(&engine, snd_a), (0, 24));
        assert_eq!(span(&engine, v2_after), (72, 96));
    }

    #[test]
    fn ripple_move_splits_straddling_clip_at_drop_on_sync_locked_rider_lane() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine");
        let v1 = add_lane(&mut engine, TrackKind::Adjustment, "V1");
        let a1 = add_lane(&mut engine, TrackKind::Adjustment, "A1");

        let a = add_bar(&mut engine, v1, 0, 24);
        let b = add_bar(&mut engine, v1, 24, 24);
        let c = add_bar(&mut engine, v1, 48, 24);
        // Continuous audio bed [0, 72) on sync-locked A1.
        let bed = add_bar(&mut engine, a1, 0, 72);

        // Reorder A between B and C: pre-close start 48 -> post-close drop 24.
        engine
            .apply(Command::Edit(EditCommand::RippleMoveClip {
                clip: a,
                to_track: v1,
                start: rt(48),
            }))
            .expect("ripple move between B and C");

        assert_eq!(span(&engine, b), (0, 24));
        assert_eq!(span(&engine, a), (24, 48));
        assert_eq!(span(&engine, c), (48, 72));
        assert_eq!(
            lane_spans(&engine, a1),
            vec![(0, 24), (48, 72)],
            "rider audio bed must split at the post-close drop (24) so C's half shifts to [48,72)"
        );

        assert!(engine.undo());
        assert_eq!(span(&engine, bed), (0, 72));
        assert_eq!(lane_spans(&engine, a1), vec![(0, 72)]);
    }
}
