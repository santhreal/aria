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
    assert_rejected_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(30), rt(5)),
        "unlink the group",
    );

    // A later detached video/audio pair would be shifted on only the video
    // lane. Reject rather than silently desynchronizing that live group.
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
    assert_rejected_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(60), rt(5)),
        "desynchronize",
    );

    let mut fixture = video_fixture(80);
    fixture
        .project
        .timeline_mut()
        .track_mut(fixture.track)
        .unwrap()
        .locked = true;
    assert_rejected_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(30), rt(5)),
        "locked",
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
    assert_rejected_unchanged(project, freeze_command(clip, rt(0), rt(5)), "video media");

    let mut project = Project::new("audio-only media", FPS);
    let audio = project.add_media(MediaSource::new("/tmp/audio.wav", 0, 0, FPS, 240, true));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, audio, tr(0, 40), rt(0)).unwrap();
    assert_rejected_unchanged(project, freeze_command(clip, rt(0), rt(5)), "video media");

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
    assert_rejected_unchanged(project, freeze_command(clip, rt(0), rt(5)), "video track");

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
    assert_rejected_unchanged(project, freeze_command(clip, rt(0), rt(5)), "video track");

    let mut fixture = video_fixture(80);
    fixture
        .project
        .timeline_mut()
        .clip_mut(fixture.source)
        .unwrap()
        .freeze_frame = true;
    assert_rejected_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(30), rt(5)),
        "already a freeze frame",
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
    assert_rejected_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(10), rt(5)),
        "animation",
    );

    let mut fixture = video_fixture(80);
    fixture
        .project
        .timeline_mut()
        .clip_mut(fixture.source)
        .unwrap()
        .animation_out = Some(AnimationRef::new("fade_out"));
    assert_rejected_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(60), rt(5)),
        "animation",
    );

    for at in [10, 60] {
        let mut fixture = video_fixture(80);
        fixture
            .project
            .timeline_mut()
            .clip_mut(fixture.source)
            .unwrap()
            .animation_combo = Some(AnimationRef::new("pulse"));
        assert_rejected_unchanged(
            fixture.project,
            freeze_command(fixture.source, rt(at), rt(5)),
            "animation",
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

    // This one passes command preflight, clears an orphan link internally,
    // then the hardened split continuity check rejects inside the entrance.
    // The link-clear inverse must run before the error escapes.
    let mut fixture = video_fixture(80);
    let orphan = LinkId::next();
    let source = fixture
        .project
        .timeline_mut()
        .clip_mut(fixture.source)
        .unwrap();
    source.link = Some(orphan);
    source.animation_in = Some(AnimationRef::new("fade_in"));
    assert_rejected_unchanged(
        fixture.project,
        freeze_command(fixture.source, rt(15), rt(6)),
        "entrance",
    );
}
