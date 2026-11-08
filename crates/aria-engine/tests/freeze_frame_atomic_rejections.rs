//! `freeze_frame` tests covering atomic rejections. A `#[path]` child of the
//! `freeze_frame` test binary; the tests reach their fixtures through
//! `super::*`.

use super::*;

#[test]
fn source_kind_link_lock_and_existing_freeze_rejections_are_atomic() {
    let mut fixture = video_fixture(80);
    let link = LinkId::next();
    for clip in [fixture.source, fixture.next] {
        fixture.project.timeline_mut().clip_mut(clip).unwrap().link = Some(link);
    }
    assert_refused_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(30), rt(5)),
        FreezeRefusal::Linked,
    );

    // A later video/audio pair whose sound lane has its sync lock cleared
    // would be shifted on only the video lane. Reject rather than silently
    // desynchronizing that live group.
    let mut fixture = video_fixture(80);
    let audio_track = fixture.project.add_track(TrackKind::Audio, "A1");
    let audio = fixture
        .project
        .add_clip(audio_track, fixture.media, tr(300, 20), rt(80))
        .unwrap();
    let link = LinkId::next();
    fixture
        .project
        .timeline_mut()
        .clip_mut(fixture.next)
        .unwrap()
        .link = Some(link);
    fixture.project.timeline_mut().clip_mut(audio).unwrap().link = Some(link);
    let a1 = fixture.project.timeline_mut().track_mut(audio_track);
    a1.unwrap().sync_lock = false;
    assert_refused_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(60), rt(5)),
        FreezeRefusal::SplitsLink,
    );

    let mut fixture = video_fixture(80);
    fixture
        .project
        .timeline_mut()
        .track_mut(fixture.track)
        .unwrap()
        .locked = true;
    assert_refused_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(30), rt(5)),
        FreezeRefusal::TrackLocked,
    );

    let mut project = Project::new("image", FPS);
    let image = project.add_media(MediaSource::image("/tmp/still.png", 1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(
            track,
            image,
            project.media(image).unwrap().full_range(),
            rt(0),
        )
        .unwrap();
    assert_refused_unchanged(
        project,
        freeze_command(clip, rt(0), rt(5)),
        FreezeRefusal::NoVideo,
    );

    let mut project = Project::new("audio-only media", FPS);
    let audio = project.add_media(MediaSource::new("/tmp/audio.wav", 0, 0, FPS, 240, true));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, audio, tr(0, 40), rt(0)).unwrap();
    assert_refused_unchanged(
        project,
        freeze_command(clip, rt(0), rt(5)),
        FreezeRefusal::NoVideo,
    );

    let mut project = Project::new("audio lane", FPS);
    let media = project.add_media(MediaSource::new(
        "/tmp/video-on-audio-lane.mp4",
        1920,
        1080,
        FPS,
        240,
        true,
    ));
    let track = project.add_track(TrackKind::Audio, "A1");
    let clip = project.add_clip(track, media, tr(0, 40), rt(0)).unwrap();
    assert_refused_unchanged(
        project,
        freeze_command(clip, rt(0), rt(5)),
        FreezeRefusal::NotOnVideoTrack,
    );

    let mut project = Project::new("generated", FPS);
    let track = project.add_track(TrackKind::Sticker, "Stickers");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [1, 2, 3, 255],
            },
            tr(0, 20),
        )
        .unwrap();
    assert_refused_unchanged(
        project,
        freeze_command(clip, rt(0), rt(5)),
        FreezeRefusal::NotOnVideoTrack,
    );

    let mut fixture = video_fixture(80);
    fixture
        .project
        .timeline_mut()
        .clip_mut(fixture.source)
        .unwrap()
        .freeze_frame = true;
    assert_refused_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(30), rt(5)),
        FreezeRefusal::AlreadyFrozen,
    );
}

#[test]
fn time_and_edge_animation_rejections_are_atomic() {
    let fixture = video_fixture(80);
    assert_rejected_unchanged(
        fixture.project,
        freeze_command(
            fixture.source,
            rt(30),
            RationalTime::new(5, Rational::FPS_30),
        ),
        "rate mismatch",
    );

    for duration in [0, -5] {
        let fixture = video_fixture(80);
        assert_rejected_unchanged(
            fixture.project,
            freeze_command(fixture.source, rt(30), rt(duration)),
            "invalid time range",
        );
    }

    let mut fixture = video_fixture(80);
    fixture
        .project
        .timeline_mut()
        .clip_mut(fixture.source)
        .unwrap()
        .animation_in = Some(AnimationRef::new("fade_in"));
    assert_refused_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(10), rt(5)),
        FreezeRefusal::Entrance,
    );

    let mut fixture = video_fixture(80);
    fixture
        .project
        .timeline_mut()
        .clip_mut(fixture.source)
        .unwrap()
        .animation_out = Some(AnimationRef::new("fade_out"));
    assert_refused_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(60), rt(5)),
        FreezeRefusal::Exit,
    );

    for (at, want) in [(10, FreezeRefusal::Entrance), (60, FreezeRefusal::Exit)] {
        let mut fixture = video_fixture(80);
        fixture
            .project
            .timeline_mut()
            .clip_mut(fixture.source)
            .unwrap()
            .animation_combo = Some(AnimationRef::new("pulse"));
        assert_refused_unchanged(
            fixture.project,
            freeze_command(fixture.source, rt(at), rt(5)),
            want,
        );
    }
}

#[test]
fn overlap_and_mid_split_failures_leave_project_and_history_untouched() {
    // Public clip fields can expose a corrupt imported/plugin-authored track.
    // FreezeFrame fails closed before opening a hole in that state.
    let mut fixture = video_fixture(80);
    let blocker = fixture
        .project
        .add_clip(fixture.track, fixture.media, tr(500, 5), rt(0))
        .unwrap();
    fixture
        .project
        .timeline_mut()
        .clip_mut(blocker)
        .unwrap()
        .timeline
        .duration = rt(30);
    assert_rejected_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(10), rt(6)),
        "overlaps",
    );

    // A cut the freeze would make inside the entrance is refused by the
    // freeze check, before the orphan link on the source is cleared.
    let mut fixture = video_fixture(80);
    let orphan = LinkId::next();
    let source = fixture
        .project
        .timeline_mut()
        .clip_mut(fixture.source)
        .unwrap();
    source.link = Some(orphan);
    source.animation_in = Some(AnimationRef::new("fade_in"));
    let error = rejected_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(15), rt(6)),
    );
    assert!(
        matches!(
            error,
            EngineError::Model(ModelError::Split(SplitRefusal::ShortensEntrance))
        ),
        "expected a cut short of the entrance, got {error:?}"
    );
}

/// A tick before the clip's first or after its last is refused by the freeze
/// check before any edit.
#[test]
fn a_tick_off_the_clip_is_refused_unchanged() {
    for at in [9, 61] {
        let fixture = video_fixture(80);
        assert_refused_unchanged(
            fixture.project,
            freeze_command(fixture.source, rt(at), rt(5)),
            FreezeRefusal::OffClip,
        );
    }
}

/// The freeze edit applies at every tick where `Project::freeze_check`
/// passes, and fails with the check's own error where it fails, across the
/// ticks around the clip under each edge animation and link the check reads,
/// the cuts inside an animation included.
///
/// Closes "the row offers what the run refuses": the Freeze Frame row reads
/// the freeze check, so a refusal the edit makes without the check making it
/// draws the row enabled and fails on choose. Not covered: rate, overlap and
/// overflow refusals, which the edit makes on its own for a corrupt track.
#[test]
fn the_edit_applies_exactly_where_the_freeze_check_passes() {
    /// A sound lane holding `members`' partners at the same ticks, linked to
    /// each, with the lane's sync lock set to `sync_lock`.
    fn sound(fixture: &mut VideoFixture, members: &[ClipId], sync_lock: bool) {
        let lane = fixture.project.add_track(TrackKind::Audio, "A1");
        for &member in members {
            let span = fixture.project.clip(member).unwrap().timeline;
            let partner = fixture
                .project
                .add_clip(
                    lane,
                    fixture.media,
                    tr(500, span.duration.value),
                    span.start,
                )
                .unwrap();
            let link = LinkId::next();
            for clip in [member, partner] {
                fixture.project.timeline_mut().clip_mut(clip).unwrap().link = Some(link);
            }
        }
        fixture
            .project
            .timeline_mut()
            .track_mut(lane)
            .unwrap()
            .sync_lock = sync_lock;
    }
    let dressings: [(&str, fn(&mut VideoFixture)); 10] = [
        ("bare", |_| {}),
        ("entrance", |fixture| {
            let source = fixture.project.timeline_mut().clip_mut(fixture.source);
            source.unwrap().animation_in = Some(AnimationRef::new("fade_in"));
        }),
        ("exit", |fixture| {
            let source = fixture.project.timeline_mut().clip_mut(fixture.source);
            source.unwrap().animation_out = Some(AnimationRef::new("fade_out"));
        }),
        ("combo", |fixture| {
            let source = fixture.project.timeline_mut().clip_mut(fixture.source);
            source.unwrap().animation_combo = Some(AnimationRef::new("pulse"));
        }),
        ("linked", |fixture| {
            let link = LinkId::next();
            for clip in [fixture.source, fixture.next] {
                fixture.project.timeline_mut().clip_mut(clip).unwrap().link = Some(link);
            }
        }),
        ("linked after, riding", |fixture| {
            let next = fixture.next;
            sound(fixture, &[next], true)
        }),
        ("linked after, pinned", |fixture| {
            let next = fixture.next;
            sound(fixture, &[next], false)
        }),
        ("own sound, riding", |fixture| {
            let members = [fixture.source, fixture.next];
            sound(fixture, &members, true)
        }),
        ("own sound, pinned", |fixture| {
            let source = fixture.source;
            sound(fixture, &[source], false)
        }),
        ("riding fade", |fixture| {
            let lane = fixture.project.add_track(TrackKind::Video, "V2");
            let rider = fixture
                .project
                .add_clip(lane, fixture.media, tr(600, 40), rt(20))
                .unwrap();
            fixture
                .project
                .timeline_mut()
                .clip_mut(rider)
                .unwrap()
                .fade_in = 12;
        }),
    ];
    for (name, dress) in dressings {
        for at in 5..=65 {
            let mut fixture = video_fixture(80);
            dress(&mut fixture);
            let checked = fixture.project.freeze_check(fixture.source, at);
            let mut engine = engine(fixture.project);
            let applied =
                engine.apply(Command::Edit(freeze_command(fixture.source, rt(at), rt(5))));
            match (checked, applied) {
                (Ok(()), Ok(_)) => {}
                (Err(want), Err(EngineError::Model(got))) => {
                    assert_eq!(got, want, "{name} at {at}")
                }
                (checked, applied) => {
                    panic!("{name} at {at}: the check read {checked:?}, the edit {applied:?}")
                }
            }
        }
    }
}
