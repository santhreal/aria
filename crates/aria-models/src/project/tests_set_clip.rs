//! `set_clip` cases.

use super::*;

#[test]
fn split_clip_rejects_wrong_timeline_rate() {
    let (mut project, media_id, track) = project_with_media(100);
    let clip = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();
    assert_eq!(
        project.split_clip(clip, RationalTime::new(25, R30)),
        Err(ModelError::RateMismatch {
            expected: R24,
            got: R30,
        })
    );
}

// --- slip_clip --------------------------------------------------------

/// A slip moves the window inside the take and nothing else. The class these
/// close: trimming derives the source from the timeline, so before this existed
/// the only way to change which part of a take a clip shows was to move the clip
/// or change its length.
#[test]
fn slip_moves_the_source_window_and_leaves_the_timeline_alone() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(48))
        .unwrap();

    project.slip_clip(clip, 30).unwrap();
    let slipped = project.clip(clip).unwrap();
    assert_eq!(slipped.timeline, tr(48, 100), "the placement is untouched");
    assert_eq!(slipped.source_range(), Some(tr(30, 100)));

    project.slip_clip(clip, -10).unwrap();
    assert_eq!(
        project.clip(clip).unwrap().source_range(),
        Some(tr(20, 100))
    );
}

#[test]
fn slip_clamps_at_both_ends_of_the_take() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    // A drag past the head stops at the head rather than failing mid-gesture.
    project.slip_clip(clip, -5_000).unwrap();
    assert_eq!(project.clip(clip).unwrap().source_range(), Some(tr(0, 100)));
    project.slip_clip(clip, 5_000).unwrap();
    assert_eq!(
        project.clip(clip).unwrap().source_range(),
        Some(tr(400, 100)),
        "the window's end stops at the end of the take"
    );
    // Already at the limit: another slip in the same direction is a no-op, not
    // an error, so a drag held at the edge keeps working.
    project.slip_clip(clip, 1_000).unwrap();
    assert_eq!(
        project.clip(clip).unwrap().source_range(),
        Some(tr(400, 100))
    );
}

#[test]
fn slip_is_refused_where_there_is_no_take_to_slip_within() {
    // A generated clip has no source at all.
    let mut project = Project::new("slip", R24);
    let track = project.add_track(TrackKind::Text, "T1");
    let text = project
        .add_generated(
            track,
            Generator::Text {
                content: "hi".into(),
                style: Default::default(),
            },
            tr(0, 48),
        )
        .unwrap();
    assert!(project.slip_clip(text, 5).is_err());

    // A still shows one frame throughout.
    let video = project.add_track(TrackKind::Video, "V1");
    let image = project.add_media(MediaSource::image("/tmp/slip.png", 64, 64));
    let full = project.media(image).unwrap().full_range();
    let still = project.add_clip(video, image, full, rt(240)).unwrap();
    assert!(project.slip_clip(still, 5).is_err());

    // A clip spanning its whole source has nowhere to go.
    let (mut whole, media_id, lane) = project_with_media(100);
    let full = whole.add_clip(lane, media_id, tr(0, 100), rt(0)).unwrap();
    assert!(
        whole.slip_clip(full, 1).is_err(),
        "a window that is the whole take reports that rather than doing nothing"
    );
}

// --- trim_clip --------------------------------------------------------

#[test]
fn trim_clip_tail_shortens_media_source() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    project.trim_clip(clip, tr(0, 60)).unwrap();
    let trimmed = project.clip(clip).unwrap();
    assert_eq!(trimmed.timeline, tr(0, 60));
    assert_eq!(trimmed.source_range(), Some(tr(0, 60)));
}

#[test]
fn trim_extends_image_clips_past_the_default_window() {
    let mut project = Project::new("stills", R24);
    let media_id = project.add_media(MediaSource::image("/tmp/a.png", 800, 600));
    let track = project.add_track(TrackKind::Video, "V1");
    let full = project.media(media_id).unwrap().full_range();
    let clip = project.add_clip(track, media_id, full, rt(0)).unwrap();
    // The 5s default placement at 24 fps.
    assert_eq!(project.clip(clip).unwrap().timeline, tr(0, 120));

    // A still stretches to any length: 10s here, double its pool entry.
    project.trim_clip(clip, tr(0, 240)).unwrap();
    let stretched = project.clip(clip).unwrap();
    assert_eq!(stretched.timeline, tr(0, 240));
    let source = stretched.source_range().unwrap();
    assert_eq!(source.start.value, 0);
    assert!(source.duration.value > crate::media::STILL_DEFAULT_DURATION_TICKS);
}

#[test]
fn add_clip_allows_oversized_image_windows() {
    let mut project = Project::new("stills", R24);
    let media_id = project.add_media(MediaSource::image("/tmp/a.png", 800, 600));
    let track = project.add_track(TrackKind::Video, "V1");
    // Place a 20s clip from the 5s pool entry directly (agent add_clip).
    let window = TimeRange::at_rate(0, 20_000, crate::media::STILL_TICK_RATE);
    let clip = project.add_clip(track, media_id, window, rt(0)).unwrap();
    assert_eq!(project.clip(clip).unwrap().timeline, tr(0, 480));

    // Video media keeps its real material bound.
    let video = project.add_media(sample_media(R24, 100));
    assert!(matches!(
        project.add_clip(track, video, tr(0, 101), rt(600)),
        Err(ModelError::SourceOutOfBounds)
    ));
}

#[test]
fn trim_clip_generated_only_moves_timeline() {
    let mut project = Project::new("test", R24);
    let track = project.add_track(TrackKind::Adjustment, "FX");
    let clip = project
        .add_generated(track, Generator::Adjustment, tr(0, 100))
        .unwrap();

    project.trim_clip(clip, tr(20, 50)).unwrap();
    let trimmed = project.clip(clip).unwrap();
    assert_eq!(trimmed.timeline, tr(20, 50));
    assert_eq!(trimmed.source_range(), None);
}

// --- set_clip_speed (M1) -----------------------------------------------

#[test]
fn set_clip_speed_rescales_timeline_duration() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    // 2× halves the footprint; the source window is untouched.
    project
        .set_clip_speed(clip, Rational::new(2, 1), false)
        .unwrap();
    let c = project.clip(clip).unwrap();
    assert_eq!(c.timeline, tr(0, 50));
    assert_eq!(c.source_range(), Some(tr(0, 100)));
    assert_eq!(c.speed, Rational::new(2, 1));

    // Back to 1× restores the original footprint.
    project
        .set_clip_speed(clip, Rational::new(1, 1), false)
        .unwrap();
    assert_eq!(project.clip(clip).unwrap().timeline, tr(0, 100));

    // Slow motion stretches it.
    project
        .set_clip_speed(clip, Rational::new(1, 2), false)
        .unwrap();
    assert_eq!(project.clip(clip).unwrap().timeline, tr(0, 200));
}

#[test]
fn set_clip_pitch_toggles_the_flag_without_moving_the_clip() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();
    project
        .set_clip_speed(clip, Rational::new(2, 1), false)
        .unwrap();
    assert!(
        project.clip(clip).unwrap().preserve_pitch,
        "locked by default"
    );

    // Unlocking pitch keeps the retimed footprint (pure audio property).
    project.set_clip_pitch(clip, false).unwrap();
    let c = project.clip(clip).unwrap();
    assert!(!c.preserve_pitch);
    assert_eq!(c.timeline, tr(0, 50));

    project.set_clip_pitch(clip, true).unwrap();
    assert!(project.clip(clip).unwrap().preserve_pitch);
}

#[test]
fn set_clip_pitch_rejects_generated_clips() {
    let mut project = Project::new("p", R24);
    let track = project.add_track(TrackKind::Sticker, "T1");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 0, 255],
            },
            tr(0, 10),
        )
        .unwrap();
    assert!(
        project.set_clip_pitch(clip, false).is_err(),
        "no audio to retime"
    );
}

#[test]
fn set_clip_denoise_toggles_and_returns_the_previous_flag() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();
    assert!(!project.clip(clip).unwrap().denoise, "off by default");

    // Turning it on returns the old value (false); turning it off again
    // returns true — the engine uses this for the undo inverse.
    assert!(!project.set_clip_denoise(clip, true).unwrap());
    assert!(project.clip(clip).unwrap().denoise);
    assert!(project.set_clip_denoise(clip, false).unwrap());
    assert!(!project.clip(clip).unwrap().denoise);
}

#[test]
fn set_clip_denoise_rejects_generated_clips() {
    let mut project = Project::new("p", R24);
    let track = project.add_track(TrackKind::Sticker, "T1");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 0, 255],
            },
            tr(0, 10),
        )
        .unwrap();
    assert!(
        project.set_clip_denoise(clip, true).is_err(),
        "no audio to clean"
    );
}

#[test]
fn set_clip_speed_curve_rederives_duration_from_average() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    // Average 2× ramp halves the footprint (source ÷ avg), like constant 2×.
    let ramp = crate::clip::speed_preset("montage").unwrap();
    let avg = {
        let mut probe = project.clip(clip).unwrap().clone();
        probe.speed_curve = ramp.clone();
        probe.speed_curve_average()
    };
    project.set_clip_speed_curve(clip, Some(ramp)).unwrap();
    let expected = (100.0 / avg).round() as i64;
    assert_eq!(project.clip(clip).unwrap().timeline, tr(0, expected));
    assert!(project.clip(clip).unwrap().has_speed_curve());

    // Clearing the ramp restores the original footprint exactly.
    project.set_clip_speed_curve(clip, None).unwrap();
    assert_eq!(project.clip(clip).unwrap().timeline, tr(0, 100));
    assert!(!project.clip(clip).unwrap().has_speed_curve());
}

#[test]
fn set_clip_speed_curve_rejects_bad_targets() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();
    // Out-of-range ramp value.
    let bad = Param::Keyframed {
        keyframes: vec![
            crate::param::Keyframe {
                tick: 0,
                value: 0.0,
                easing: Easing::Linear,
                tangents: None,
            },
            crate::param::Keyframe {
                tick: 1000,
                value: 1.0,
                easing: Easing::Linear,
                tangents: None,
            },
        ],
    };
    assert!(project.set_clip_speed_curve(clip, Some(bad)).is_err());
    // Generated clips cannot be retimed.
    let sticker = project.add_track(TrackKind::Sticker, "S");
    let generated = project
        .add_generated(
            sticker,
            Generator::SolidColor {
                rgba: [0, 0, 0, 255],
            },
            tr(200, 10),
        )
        .unwrap();
    assert!(matches!(
        project.set_clip_speed_curve(generated, Some(crate::clip::speed_preset("hero").unwrap())),
        Err(ModelError::InvalidParam(_))
    ));
}

#[test]
fn set_clip_speed_reverse_keeps_duration() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    project
        .set_clip_speed(clip, Rational::new(1, 1), true)
        .unwrap();
    let c = project.clip(clip).unwrap();
    assert_eq!(c.timeline, tr(0, 100));
    assert!(c.reversed && c.is_retimed());
}

#[test]
fn set_clip_speed_rejects_invalid_targets() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    assert!(matches!(
        project.set_clip_speed(clip, Rational::new(0, 1), false),
        Err(ModelError::InvalidParam(_))
    ));
    assert!(matches!(
        project.set_clip_speed(clip, Rational::new(-2, 1), false),
        Err(ModelError::InvalidParam(_))
    ));

    let fx = project.add_track(TrackKind::Adjustment, "FX");
    let generated = project
        .add_generated(fx, Generator::Adjustment, tr(0, 100))
        .unwrap();
    assert!(matches!(
        project.set_clip_speed(generated, Rational::new(2, 1), false),
        Err(ModelError::InvalidParam(_))
    ));

    let missing = ClipId::next();
    assert_eq!(
        project.set_clip_speed(missing, Rational::new(2, 1), false),
        Err(ModelError::UnknownClip(missing))
    );
}

#[test]
fn set_clip_speed_rejects_overlap_with_neighbor() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();
    // Neighbor right behind the clip: slowing to ½× (200 ticks) collides.
    project
        .add_clip(track, media_id, tr(0, 50), rt(100))
        .unwrap();

    assert_eq!(
        project.set_clip_speed(clip, Rational::new(1, 2), false),
        Err(ModelError::Overlap(track))
    );
    // The clip is untouched after the rejection.
    let c = project.clip(clip).unwrap();
    assert_eq!(c.timeline, tr(0, 100));
    assert_eq!(c.speed, Rational::new(1, 1));
}

// --- set_clip_beats (M8 Phase 6) -----------------------------------------

#[test]
fn set_clip_beats_sorts_dedups_clamps_and_returns_previous() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    // Out-of-window (>=100), duplicate, and unsorted input is normalized.
    let prev = project
        .set_clip_beats(clip, vec![60, 10, 10, 200, 30])
        .unwrap();
    assert!(prev.is_empty(), "no beats before the first detect");
    assert_eq!(project.clip(clip).unwrap().beats, vec![10, 30, 60]);

    // Replacing returns the old list (the inverse snapshot).
    let prev = project.set_clip_beats(clip, vec![5]).unwrap();
    assert_eq!(prev, vec![10, 30, 60]);
    assert_eq!(project.clip(clip).unwrap().beats, vec![5]);
}
