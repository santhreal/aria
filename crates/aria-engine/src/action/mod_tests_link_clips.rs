//! `link_clips` cases.

use super::*;

#[test]
fn ripple_insert_rejected_placement_restores_shift() {
    let mut project = setup();
    let media_id = project.add_media(MediaSource::new(
        "/tmp/ri-bad.mp4",
        1920,
        1080,
        Rational::FPS_24,
        240,
        false,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(
            track,
            media_id,
            TimeRange::at_rate(0, 48, Rational::FPS_24),
            RationalTime::new(0, Rational::FPS_24),
        )
        .unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    // Source range exceeds the media's 240 ticks → add_clip rejects after
    // the shift already ran; the shift must be rolled back atomically.
    let result = ripple_insert::execute(
        &mut ctx,
        track,
        media_id,
        TimeRange::at_rate(200, 100, Rational::FPS_24),
        rt(0),
    );
    assert!(result.is_err());
    assert_eq!(ctx.project.timeline().clip_count(), 1);
    assert_eq!(
        ctx.project.clip(clip).unwrap().start().value,
        0,
        "shift undone"
    );
}

#[test]
fn compound_action_applies_in_reverse_and_oscillates() {
    let mut project = setup();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    // Gesture: AddTrack + AddGenerated. Inverses are stored in execution
    // order; undo must run them in reverse (remove the clip before its
    // track) or the second inverse hits an unknown clip.
    let (track, inv_track) =
        add_track::execute(&mut ctx, TrackKind::Text, "T1", None, false).unwrap();
    let (clip, inv_clip) =
        add_generated::execute(&mut ctx, track, Generator::text("x"), tr(0, 10)).unwrap();

    let compound: Box<dyn EditAction> = Box::new(CompoundAction {
        actions: vec![inv_track, inv_clip],
    });

    let redo = compound.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().track_count(), 0);
    assert_eq!(ctx.project.timeline().clip_count(), 0);

    let undo = redo.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().track_count(), 1);
    assert_eq!(ctx.project.timeline().track_of(clip), Some(track));

    let _ = undo.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().track_count(), 0);
    assert_eq!(ctx.project.timeline().clip_count(), 0);
}

#[test]
fn add_transition_inverse_oscillates() {
    use crate::action::edit::set_transition;
    let mut project = setup();
    let track = project.add_track(TrackKind::Sticker, "S1");
    let left = project
        .timeline_mut()
        .add_clip(
            track,
            Clip::generated(
                Generator::SolidColor {
                    rgba: [255, 0, 0, 255],
                },
                tr(0, 24),
            ),
        )
        .unwrap();
    let _right = project
        .timeline_mut()
        .add_clip(
            track,
            Clip::generated(
                Generator::SolidColor {
                    rgba: [0, 0, 255, 255],
                },
                tr(24, 24),
            ),
        )
        .unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let inv1 = set_transition::add_transition(&mut ctx, left, "crossfade").unwrap();
    assert!(
        ctx.project
            .timeline()
            .track(track)
            .unwrap()
            .transition_at(left)
            .is_some(),
        "transition added"
    );

    let inv2 = inv1.apply(&mut ctx).unwrap();
    assert!(
        ctx.project
            .timeline()
            .track(track)
            .unwrap()
            .transition_at(left)
            .is_none()
    );

    let _ = inv2.apply(&mut ctx).unwrap();
    assert!(
        ctx.project
            .timeline()
            .track(track)
            .unwrap()
            .transition_at(left)
            .is_some()
    );
}

#[test]
fn structural_edit_prunes_dead_junction_and_undo_restores_it() {
    use crate::action::dispatch;
    use aria_commands::{Command, EditCommand};
    let mut project = setup();
    let track = project.add_track(TrackKind::Sticker, "S1");
    let left = project
        .timeline_mut()
        .add_clip(
            track,
            Clip::generated(
                Generator::SolidColor {
                    rgba: [255, 0, 0, 255],
                },
                tr(0, 24),
            ),
        )
        .unwrap();
    let _right = project
        .timeline_mut()
        .add_clip(
            track,
            Clip::generated(
                Generator::SolidColor {
                    rgba: [0, 0, 255, 255],
                },
                tr(24, 24),
            ),
        )
        .unwrap();
    project.add_transition(left, "crossfade").unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    // Move the left clip away: the junction breaks and the transition is
    // pruned as part of the edit.
    let (_out, inverse) = dispatch(
        Command::Edit(EditCommand::MoveClip {
            clip: left,
            to_track: track,
            start: rt(100),
        }),
        &mut ctx,
    )
    .unwrap();
    let inverse = inverse.expect("structural edit records an inverse");
    assert!(
        ctx.project
            .timeline()
            .track(track)
            .unwrap()
            .transition_at(left)
            .is_none(),
        "broken junction pruned"
    );

    // Undo restores both the clip position and the pruned transition.
    let redo = inverse.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.clip(left).unwrap().start().value, 0);
    assert!(
        ctx.project
            .timeline()
            .track(track)
            .unwrap()
            .transition_at(left)
            .is_some(),
        "undo brings the junction back"
    );

    // Redo moves again and re-prunes.
    let _ = redo.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.clip(left).unwrap().start().value, 100);
    assert!(
        ctx.project
            .timeline()
            .track(track)
            .unwrap()
            .transition_at(left)
            .is_none()
    );
}

#[test]
fn transition_guard_snapshots_only_tracks_named_by_the_edit() {
    use aria_commands::EditCommand;
    use aria_models::{ClipId, TrackId};

    fn add_pair(project: &mut Project, track: TrackId, start: i64) -> ClipId {
        let left = project
            .timeline_mut()
            .add_clip(
                track,
                Clip::generated(
                    Generator::SolidColor {
                        rgba: [255, 0, 0, 255],
                    },
                    tr(start, 24),
                ),
            )
            .unwrap();
        project
            .timeline_mut()
            .add_clip(
                track,
                Clip::generated(
                    Generator::SolidColor {
                        rgba: [0, 0, 255, 255],
                    },
                    tr(start + 24, 24),
                ),
            )
            .unwrap();
        project.add_transition(left, "crossfade").unwrap();
        left
    }

    let mut project = setup();
    let touched = project.add_track(TrackKind::Sticker, "Touched");
    let untouched = project.add_track(TrackKind::Sticker, "Untouched");
    let touched_clip = add_pair(&mut project, touched, 0);
    let _untouched_clip = add_pair(&mut project, untouched, 100);
    let mut project_path = None;
    let mut history = History::new(32);
    let ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let snapshot =
        dispatch::transitions_guard(&ctx, &EditCommand::RemoveClip { clip: touched_clip })
            .expect("the touched track has a transition");

    assert_eq!(snapshot.len(), 1, "an unrelated track was cloned");
    assert_eq!(snapshot[0].0, touched);
    assert_eq!(snapshot[0].1.len(), 1);
}

#[test]
fn set_track_flags_inverse_oscillates() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Video, "V1");
    assert!(project.timeline().track(track).unwrap().enabled);

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    // Disable the track; only `enabled` changes, the rest stay put.
    let inv1 =
        set_track_flags::execute(&mut ctx, track, set_track_flags::FlagPatch::enabled(false))
            .unwrap();
    assert!(!ctx.project.timeline().track(track).unwrap().enabled);
    assert!(!ctx.project.timeline().track(track).unwrap().muted);
    assert!(!ctx.project.timeline().track(track).unwrap().locked);

    // Undo restores the full snapshot.
    let inv2 = inv1.apply(&mut ctx).unwrap();
    assert!(ctx.project.timeline().track(track).unwrap().enabled);

    // Redo disables again.
    let _ = inv2.apply(&mut ctx).unwrap();
    assert!(!ctx.project.timeline().track(track).unwrap().enabled);
}

#[test]
fn link_clips_inverse_restores_prior_links_and_oscillates() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Adjustment, "FX");
    let a = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(0, 10)))
        .unwrap();
    let b = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(20, 10)))
        .unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let inv1 = link_clips::execute(&mut ctx, &[a, b]).unwrap();
    let group = ctx.project.clip(a).unwrap().link;
    assert!(group.is_some());
    assert_eq!(ctx.project.clip(b).unwrap().link, group);

    // Re-linking `b` into a new pair must let undo restore the old group.
    let c = ctx
        .project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(40, 10)))
        .unwrap();
    let inv2 = link_clips::execute(&mut ctx, &[b, c]).unwrap();
    let pair = ctx.project.clip(b).unwrap().link;
    assert_ne!(pair, group);
    assert_eq!(ctx.project.clip(c).unwrap().link, pair);

    let redo2 = inv2.apply(&mut ctx).unwrap();
    assert_eq!(
        ctx.project.clip(b).unwrap().link,
        group,
        "old link restored"
    );
    assert_eq!(ctx.project.clip(c).unwrap().link, None);

    let _ = redo2.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.clip(b).unwrap().link, pair);

    // First link's inverse restores `a` to unlinked.
    let inv1b = inv1.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.clip(a).unwrap().link, None);
    let _ = inv1b.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.clip(a).unwrap().link, group);
}

#[test]
fn link_clips_unknown_clip_mutates_nothing() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Adjustment, "FX");
    let a = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(0, 10)))
        .unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let missing = aria_models::ClipId::from_raw(999);
    assert!(link_clips::execute(&mut ctx, &[a, missing]).is_err());
    assert_eq!(
        ctx.project.clip(a).unwrap().link,
        None,
        "validated before mutating"
    );
}

#[test]
fn unlink_one_member_dissolves_complete_group_only() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Adjustment, "FX");
    let a = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(0, 10)))
        .unwrap();
    let b = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(20, 10)))
        .unwrap();
    let c = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(40, 10)))
        .unwrap();
    let d = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(60, 10)))
        .unwrap();
    let touched = LinkId::next();
    let unrelated = LinkId::next();
    for clip in [a, b] {
        project.timeline_mut().clip_mut(clip).unwrap().link = Some(touched);
    }
    for clip in [c, d] {
        project.timeline_mut().clip_mut(clip).unwrap().link = Some(unrelated);
    }

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let _inverse = link_clips::unlink(&mut ctx, &[a]).unwrap();
    assert_eq!(ctx.project.clip(a).unwrap().link, None);
    assert_eq!(
        ctx.project.clip(b).unwrap().link,
        None,
        "one member dissolves its complete group"
    );
    assert_eq!(ctx.project.clip(c).unwrap().link, Some(unrelated));
    assert_eq!(ctx.project.clip(d).unwrap().link, Some(unrelated));
    assert_eq!(ctx.project.clip(a).unwrap().start().value, 0);
    assert_eq!(ctx.project.clip(b).unwrap().start().value, 20);
}

#[test]
fn unlink_clears_multiple_groups_with_duplicate_inputs() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Adjustment, "FX");
    let ids: Vec<_> = (0..6)
        .map(|index| {
            project
                .timeline_mut()
                .add_clip(
                    track,
                    Clip::generated(Generator::Adjustment, tr(index * 20, 10)),
                )
                .unwrap()
        })
        .collect();
    let first = LinkId::next();
    let second = LinkId::next();
    let unrelated = LinkId::next();
    for (clip, link) in [
        (ids[0], first),
        (ids[1], first),
        (ids[2], second),
        (ids[3], second),
        (ids[4], unrelated),
        (ids[5], unrelated),
    ] {
        project.timeline_mut().clip_mut(clip).unwrap().link = Some(link);
    }

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let _inverse = link_clips::unlink(&mut ctx, &[ids[2], ids[0], ids[0], ids[1], ids[2]]).unwrap();
    for clip in &ids[..4] {
        assert_eq!(ctx.project.clip(*clip).unwrap().link, None);
    }
    for clip in &ids[4..] {
        assert_eq!(
            ctx.project.clip(*clip).unwrap().link,
            Some(unrelated),
            "unrelated group remains linked"
        );
    }
}
