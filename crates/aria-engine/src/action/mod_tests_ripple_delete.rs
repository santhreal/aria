//! `ripple_delete` cases.

use super::*;

#[test]
fn add_clip_inverse_oscillates() {
    let mut project = setup();
    let media_id = project.add_media(MediaSource::new(
        "/tmp/x.mp4",
        1920,
        1080,
        Rational::FPS_24,
        240,
        true,
    ));
    let track = project.add_track(TrackKind::Video, "V1");

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let (id, inv1) = add_clip::execute(
        &mut ctx,
        track,
        media_id,
        TimeRange::at_rate(0, 48, Rational::FPS_24),
        RationalTime::new(0, Rational::FPS_24),
    )
    .unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 1);

    let inv2 = inv1.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 0);

    let inv3 = inv2.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 1);
    assert!(ctx.project.clip(id).is_some());

    let _ = inv3.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 0);
}

#[test]
fn remove_media_inverse_restores_snapshot() {
    let mut project = setup();
    let media = MediaSource::new("/tmp/y.mp4", 1280, 720, Rational::FPS_24, 100, false);
    let id = project.add_media(media.clone());

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let inv1 = Box::new(remove_media::RemoveMediaAction { media: id });
    let inv2 = inv1.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.media_count(), 0);

    let inv3 = inv2.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.media_count(), 1);
    assert_eq!(ctx.project.media(id).unwrap(), &media);

    let _ = inv3.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.media_count(), 0);
}

#[test]
fn split_clip_inverse_oscillates() {
    let mut project = setup();
    let media_id = project.add_media(MediaSource::new(
        "/tmp/split.mp4",
        1920,
        1080,
        Rational::FPS_24,
        240,
        false,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip_id = project
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

    let (_tail, inv1) = split_clip::execute(&mut ctx, clip_id, rt(24)).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 2);
    assert_eq!(
        ctx.project.clip(clip_id).unwrap().timeline.duration.value,
        24
    );

    let inv2 = inv1.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 1);
    assert_eq!(
        ctx.project.clip(clip_id).unwrap().timeline.duration.value,
        48
    );

    let inv3 = inv2.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 2);
    assert_eq!(
        ctx.project.clip(clip_id).unwrap().timeline.duration.value,
        24
    );

    let _ = inv3.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 1);
}

#[test]
fn ripple_delete_inverse_oscillates() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Adjustment, "FX");
    let first = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(0, 10)))
        .unwrap();
    let second = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(20, 10)))
        .unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let inv1 = ripple_delete::execute(&mut ctx, first).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 1);
    assert_eq!(ctx.project.clip(second).unwrap().timeline.start.value, 10);

    let inv2 = inv1.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 2);
    assert_eq!(ctx.project.clip(second).unwrap().timeline.start.value, 20);

    let _ = inv2.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 1);
}

#[test]
fn ripple_delete_middle_of_three_adjacent_oscillates() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Adjustment, "FX");
    let a = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(0, 10)))
        .unwrap();
    let b = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(10, 10)))
        .unwrap();
    let c = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(20, 10)))
        .unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let inv1 = ripple_delete::execute(&mut ctx, b).unwrap();
    assert_eq!(ctx.project.clip(c).unwrap().start().value, 10);

    let inv2 = inv1.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.clip(b).unwrap().start().value, 10);
    assert_eq!(ctx.project.clip(c).unwrap().start().value, 20);

    let _ = inv2.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.clip(c).unwrap().start().value, 10);
    assert!(ctx.project.clip(b).is_none());
    let _ = a;
}

#[test]
fn trim_clip_inverse_restores_snapshot() {
    let mut project = setup();
    let media_id = project.add_media(MediaSource::new(
        "/tmp/trim.mp4",
        1280,
        720,
        Rational::FPS_24,
        240,
        false,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip_id = project
        .add_clip(
            track,
            media_id,
            TimeRange::at_rate(0, 48, Rational::FPS_24),
            RationalTime::new(0, Rational::FPS_24),
        )
        .unwrap();
    let before = ctx_project_clip(&project, clip_id);

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let inv1 = trim_clip::execute(&mut ctx, clip_id, tr(10, 28)).unwrap();
    assert_eq!(ctx.project.clip(clip_id).unwrap().timeline.start.value, 10);

    let inv2 = inv1.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.clip(clip_id).unwrap().timeline, before.timeline);
    assert_eq!(
        ctx.project.clip(clip_id).unwrap().source_range(),
        before.source_range()
    );

    let _ = inv2.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.clip(clip_id).unwrap().timeline.start.value, 10);
}

#[test]
fn move_clip_inverse_oscillates() {
    let mut project = setup();
    let v1 = project.add_track(TrackKind::Text, "T1");
    let v2 = project.add_track(TrackKind::Text, "T2");
    let clip_id = project
        .timeline_mut()
        .add_clip(v1, Clip::generated(Generator::text("x"), tr(5, 15)))
        .unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let inv1 = move_clip::execute(&mut ctx, clip_id, v2, rt(40)).unwrap();
    assert_eq!(ctx.project.timeline().track_of(clip_id), Some(v2));

    let inv2 = inv1.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().track_of(clip_id), Some(v1));
    assert_eq!(ctx.project.clip(clip_id).unwrap().start().value, 5);

    let _ = inv2.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().track_of(clip_id), Some(v2));
}

#[test]
fn add_generated_inverse_oscillates() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Sticker, "S1");

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let (id, inv1) = add_generated::execute(
        &mut ctx,
        track,
        Generator::SolidColor { rgba: [9, 8, 7, 6] },
        tr(0, 12),
    )
    .unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 1);

    let inv2 = inv1.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 0);

    let inv3 = inv2.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 1);
    assert!(ctx.project.clip(id).is_some());

    let _ = inv3.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 0);
}

#[test]
fn split_generated_clip_inverse_oscillates() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Text, "T1");
    let clip_id = project
        .timeline_mut()
        .add_clip(
            track,
            Clip::generated(Generator::text("split me"), tr(0, 20)),
        )
        .unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let (_tail, inv1) = split_clip::execute(&mut ctx, clip_id, rt(10)).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 2);

    let inv2 = inv1.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 1);
    assert_eq!(
        ctx.project.clip(clip_id).unwrap().timeline.duration.value,
        20
    );

    let _ = inv2.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 2);
}

#[test]
fn shift_clips_inverse_selects_exactly_the_shifted_set() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Adjustment, "FX");
    // A [60,80) stays put; B [100,120) shifts left into [80,100). The
    // inverse boundary (B's new start, 80) must re-select only B — a
    // naive `from + delta` boundary (also 80 here, but 60-adjacent cases
    // differ) could drag A along.
    let a = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(60, 20)))
        .unwrap();
    let b = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(100, 20)))
        .unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let inv = shift_clips::execute(&mut ctx, track, rt(100), rt(-20)).unwrap();
    assert_eq!(ctx.project.clip(b).unwrap().start().value, 80);

    let redo = inv.apply(&mut ctx).unwrap();
    assert_eq!(
        ctx.project.clip(a).unwrap().start().value,
        60,
        "A untouched"
    );
    assert_eq!(ctx.project.clip(b).unwrap().start().value, 100);

    let _ = redo.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.clip(a).unwrap().start().value, 60);
    assert_eq!(ctx.project.clip(b).unwrap().start().value, 80);
}

#[test]
fn ripple_insert_shifts_then_places_and_oscillates() {
    let mut project = setup();
    let media_id = project.add_media(MediaSource::new(
        "/tmp/ri.mp4",
        1920,
        1080,
        Rational::FPS_24,
        240,
        false,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let first = project
        .add_clip(
            track,
            media_id,
            TimeRange::at_rate(0, 48, Rational::FPS_24),
            RationalTime::new(0, Rational::FPS_24),
        )
        .unwrap();
    let second = project
        .add_clip(
            track,
            media_id,
            TimeRange::at_rate(48, 48, Rational::FPS_24),
            RationalTime::new(48, Rational::FPS_24),
        )
        .unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    // Insert 24 ticks at the boundary between the two clips.
    let (inserted, inv) = ripple_insert::execute(
        &mut ctx,
        track,
        media_id,
        TimeRange::at_rate(100, 24, Rational::FPS_24),
        rt(48),
    )
    .unwrap();
    assert_eq!(ctx.project.clip(first).unwrap().start().value, 0);
    assert_eq!(ctx.project.clip(inserted).unwrap().start().value, 48);
    assert_eq!(ctx.project.clip(second).unwrap().start().value, 72);

    let redo = inv.apply(&mut ctx).unwrap();
    assert!(ctx.project.clip(inserted).is_none());
    assert_eq!(ctx.project.clip(second).unwrap().start().value, 48);

    let _ = redo.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().clip_count(), 3);
    assert_eq!(ctx.project.clip(second).unwrap().start().value, 72);
}

/// A media source and a lane, for the insert cases below.
fn lane_with_media(
    project: &mut Project,
    name: &str,
) -> (aria_models::MediaId, aria_models::TrackId) {
    let media = project.add_media(MediaSource::new(
        format!("/media/{name}.mp4"),
        1920,
        1080,
        Rational::FPS_24,
        240,
        false,
    ));
    let track = project.add_track(TrackKind::Video, name);
    (media, track)
}

fn src(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, Rational::FPS_24)
}

/// An insert that lands inside a clip cuts it there, and the tail rides right
/// with everything after it.
///
/// Closes the class: the shift primitive selects clips by start tick, so an
/// insert inside a clip moved nothing and was then refused for overlapping the
/// clip it landed in — dropping a pool item halfway through a shot was an
/// error instead of an edit. The inverse rejoins the cut, so the gesture is
/// still one undo.
#[test]
fn an_insert_inside_a_clip_cuts_it_and_pushes_the_tail() {
    let mut project = setup();
    let (media, track) = lane_with_media(&mut project, "cut");
    let held = project
        .add_clip(track, media, src(0, 48), rt(0))
        .expect("place the clip the insert lands in");
    let later = project
        .add_clip(track, media, src(48, 24), rt(48))
        .expect("place a clip after it");

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let (inserted, inverse) = ripple_insert::execute(&mut ctx, track, media, src(0, 24), rt(24))
        .expect("an insert inside a clip is an edit, not a refusal");

    assert_eq!(
        ctx.project.timeline().clip_count(),
        4,
        "head, insert, tail, later"
    );
    let head = ctx
        .project
        .clip(held)
        .expect("the head keeps the original id");
    assert_eq!(head.start().value, 0);
    assert_eq!(
        head.timeline.duration.value, 24,
        "the head was cut at the insert"
    );
    assert_eq!(ctx.project.clip(inserted).unwrap().start().value, 24);
    assert_eq!(
        ctx.project.clip(later).unwrap().start().value,
        72,
        "the clip after the cut rode the whole insert"
    );
    // The tail is the fourth clip: the only one starting at the insert's end.
    let tail_starts: Vec<i64> = ctx
        .project
        .timeline()
        .track(track)
        .expect("lane")
        .clips()
        .map(|clip| clip.start().value)
        .collect();
    assert!(
        tail_starts.contains(&48),
        "the tail sits after the insert: {tail_starts:?}"
    );

    let redo = inverse.apply(&mut ctx).expect("undo the whole gesture");
    assert_eq!(
        ctx.project.timeline().clip_count(),
        2,
        "the cut was rejoined"
    );
    assert!(ctx.project.clip(inserted).is_none());
    assert_eq!(
        ctx.project.clip(held).unwrap().timeline.duration.value,
        48,
        "undo restored the clip whole"
    );
    assert_eq!(ctx.project.clip(later).unwrap().start().value, 48);

    let _ = redo.apply(&mut ctx).expect("redo the gesture");
    assert_eq!(ctx.project.timeline().clip_count(), 4);
    assert_eq!(ctx.project.clip(later).unwrap().start().value, 72);
}

/// An insert at a cut moves clips without making one: a boundary is already a
/// cut, so nothing is split and the lane gains exactly the inserted clip.
#[test]
fn an_insert_at_a_cut_does_not_split_anything() {
    let mut project = setup();
    let (media, track) = lane_with_media(&mut project, "boundary");
    let first = project.add_clip(track, media, src(0, 48), rt(0)).unwrap();
    let second = project.add_clip(track, media, src(48, 24), rt(48)).unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let (inserted, inverse) = ripple_insert::execute(&mut ctx, track, media, src(0, 12), rt(48))
        .expect("insert at the boundary");
    assert_eq!(ctx.project.timeline().clip_count(), 3);
    assert_eq!(ctx.project.clip(first).unwrap().timeline.duration.value, 48);
    assert_eq!(ctx.project.clip(inserted).unwrap().start().value, 48);
    assert_eq!(ctx.project.clip(second).unwrap().start().value, 60);

    let _ = inverse.apply(&mut ctx).expect("undo");
    assert_eq!(ctx.project.timeline().clip_count(), 2);
    assert_eq!(ctx.project.clip(second).unwrap().start().value, 48);
}

/// A sync-locked lane is cut at the same tick as the lane being inserted on,
/// so picture and sound stay in step through an insert that lands mid-clip.
/// A lane with its sync lock cleared keeps both its cut and its placement.
#[test]
fn a_sync_locked_lane_is_cut_with_the_insert_and_a_pinned_lane_is_not() {
    let mut project = setup();
    let (media, track) = lane_with_media(&mut project, "target");
    let (rider_media, rider) = lane_with_media(&mut project, "rider");
    let (pinned_media, pinned) = lane_with_media(&mut project, "pinned");
    project
        .timeline_mut()
        .track_mut(pinned)
        .expect("pinned lane")
        .sync_lock = false;

    project.add_clip(track, media, src(0, 48), rt(0)).unwrap();
    let riding = project
        .add_clip(rider, rider_media, src(0, 48), rt(0))
        .unwrap();
    let held = project
        .add_clip(pinned, pinned_media, src(0, 48), rt(0))
        .unwrap();

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let (_, inverse) = ripple_insert::execute(&mut ctx, track, media, src(0, 24), rt(24))
        .expect("insert inside a clip on a lane with a synced neighbor");

    assert_eq!(
        ctx.project.clip(riding).unwrap().timeline.duration.value,
        24,
        "the synced lane was cut at the insert tick"
    );
    assert_eq!(
        ctx.project.clip(held).unwrap().timeline.duration.value,
        48,
        "a lane with its sync lock cleared was left alone"
    );

    let _ = inverse.apply(&mut ctx).expect("undo");
    assert_eq!(
        ctx.project.clip(riding).unwrap().timeline.duration.value,
        48,
        "undo rejoined the synced lane's cut"
    );
}
