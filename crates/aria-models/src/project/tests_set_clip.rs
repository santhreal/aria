//! `set_clip` cases.

use super::*;
use crate::clip::SlipRefusal;

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

/// A nested sequence shorter than the window has no tail to stop a slip, so
/// a slip by as far as a tick goes stops where the window ends at the last
/// tick a time holds, not past it, from any start. Red when the room on is
/// `i64::MAX` from the window's start.
#[test]
fn a_nested_sequence_slips_on_until_its_window_ends_at_the_last_tick() {
    for start in [0, 5, i64::MAX - 20] {
        let mut project = Project::new("slip", R24);
        let track = project.add_track(TrackKind::Video, "V1");
        let nest = project.create_sequence("Nest", R24);
        let clip = project
            .add_sequence_clip(track, nest, tr(0, 10), rt(0))
            .unwrap();
        project.slip_clip(clip, start).unwrap();
        project.slip_clip(clip, i64::MAX).unwrap();
        let window = project.clip(clip).unwrap().source_range().unwrap();
        assert_eq!(
            (window.start.value, window.end().map(|end| end.value)),
            (i64::MAX - 10, Ok(i64::MAX)),
            "slipped from {start}"
        );
    }
}

/// Every [`SlipRefusal`] is what a slip returns for the clip that raises it,
/// and the refused slip leaves the clip as it was.
///
/// Closes the class where a slip drag on a clip with no take to move inside
/// returned `InvalidParam`, which the editor reads as "That value isn't
/// accepted here. Enter another value": a slip of a still pointed at a value
/// field. Derived from [`SlipRefusal::ALL`] through an exhaustive match, so a
/// new refusal fails to build until it has a fixture. Not covered: a linked
/// partner's refusal through [`Project::slip_reach`].
#[test]
fn every_slip_refusal_is_raised_by_its_clip_and_leaves_it_whole() {
    for refusal in SlipRefusal::ALL {
        let (mut project, media_id, video) = project_with_media(100);
        let clip = match refusal {
            SlipRefusal::FreezeFrame => {
                let clip = project.add_clip(video, media_id, tr(0, 50), rt(0)).unwrap();
                project.timeline_mut().clip_mut(clip).unwrap().freeze_frame = true;
                clip
            }
            SlipRefusal::Still => {
                let image = project.add_media(MediaSource::image("slip.png", 64, 64));
                let full = project.media(image).unwrap().full_range();
                project.add_clip(video, image, full, rt(240)).unwrap()
            }
            SlipRefusal::Generated => {
                let text = project.add_track(TrackKind::Text, "T1");
                let generator = Generator::Text {
                    content: "hi".into(),
                    style: Default::default(),
                };
                project.add_generated(text, generator, tr(0, 48)).unwrap()
            }
            SlipRefusal::WholeTake => project
                .add_clip(video, media_id, tr(0, 100), rt(0))
                .unwrap(),
        };
        let before = project.clip(clip).unwrap().clone();
        for by in [-5, 0, 5] {
            assert_eq!(
                project.slip_clip(clip, by),
                Err(ModelError::Slip(refusal)),
                "{refusal:?} by {by}"
            );
        }
        assert_eq!(project.clip(clip), Some(&before), "{refusal:?}");
    }
}

#[test]
fn slip_multicam_moves_window_and_clamps_within_group_span() {
    let mut project = Project::new("multicam_slip", R24);
    let m1 = project.add_media(sample_media(R24, 200));
    let m2 = project.add_media(sample_media(R24, 200));
    let group = project
        .create_multicam_group(
            "Rig",
            vec![
                crate::multicam::MulticamAngle::new(m1, 0),
                crate::multicam::MulticamAngle::new(m2, 40),
            ],
        )
        .unwrap();
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_multicam_clip(track, group, tr(20, 100), rt(48))
        .unwrap();

    project.slip_clip(clip, 30).unwrap();
    assert_eq!(project.clip(clip).unwrap().timeline, tr(48, 100));
    assert_eq!(
        project.clip(clip).unwrap().source_range(),
        Some(tr(50, 100))
    );

    project.slip_clip(clip, -5_000).unwrap();
    assert_eq!(project.clip(clip).unwrap().source_range(), Some(tr(0, 100)));

    // Group span is max(0 + 200, 40 + 200) = 240, so max start for a 100-tick window is 140.
    project.slip_clip(clip, 5_000).unwrap();
    assert_eq!(
        project.clip(clip).unwrap().source_range(),
        Some(tr(140, 100))
    );

    let track2 = project.add_track(TrackKind::Video, "V2");
    let full = project
        .add_multicam_clip(track2, group, tr(0, 240), rt(0))
        .unwrap();
    assert_eq!(
        project.slip_clip(full, 1),
        Err(ModelError::Slip(SlipRefusal::WholeTake)),
        "a multicam clip spanning the whole group span has nowhere to slip"
    );
}

/// The source ticks a slip drag moves a take by: the pointer's travel at the
/// clip's speed, in the ticks of its take, against the pointer on a clip
/// played forward and with it on one played backward. The class this closes:
/// a slip drag sent the pointer's timeline ticks as source ticks, so a take at
/// another rate, or played at another speed, slid under the pointer at the
/// wrong pace.
#[test]
fn a_slip_drag_moves_the_take_by_the_travel_at_its_speed_and_rate() {
    let mut project = Project::new("slip", R24);
    let picture = project.add_media(sample_media(R30, 900));
    let video = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(video, picture, tr_at(300, 300, R30), rt(0))
        .unwrap();
    let travel = |ticks| RationalTime::new(ticks, R24);

    // Half a second on a 24 fps timeline is fifteen frames of a 30 fps take,
    // which goes the other way under the pointer.
    assert_eq!(project.clip(clip).unwrap().slip_for(travel(12)), Some(-15));
    assert_eq!(project.clip(clip).unwrap().slip_for(travel(-12)), Some(15));

    // At twice the speed, played backward, half a second of timeline is a
    // second of take, which goes with the pointer.
    project
        .set_clip_speed(clip, Rational::new(2, 1), true)
        .unwrap();
    assert_eq!(project.clip(clip).unwrap().slip_for(travel(12)), Some(30));

    let stickers = project.add_track(TrackKind::Sticker, "S1");
    let solid = project
        .add_generated(
            stickers,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 48),
        )
        .unwrap();
    assert_eq!(
        project.clip(solid).unwrap().slip_for(travel(12)),
        None,
        "a generated clip plays no take"
    );
}

/// A slip of one of a linked pair moves each take by the same stretch of
/// source time, each in the ticks of its own take, and stops where the first
/// take runs out. The class this closes: a linked pair slipped by one tick
/// count whatever each take's rate, and each member stopped at its own end,
/// so a slip of a picture over a sound file pulled the pair apart. Covers a
/// slip sent from either member, both ways, inside and past the room, and a
/// sound whose tail falls between two frames. Does not cover the engine
/// applying the shares, which `slip_clip` does one clip at a time.
#[test]
fn a_linked_slip_moves_each_take_by_the_same_time_and_stops_at_the_first_end() {
    let (mut project, picture_media, video) = project_with_media(500);
    let ms = Rational::new(1000, 1);
    let sound_media = project.add_media(MediaSource::new("/tmp/slip.wav", 0, 0, ms, 10_030, true));
    let audio = project.add_track(TrackKind::Audio, "A1");
    // Two seconds each: frames 48 to 96 of a 500-frame take, which has 2 s
    // behind the window and 16.8 s after it, and 2 s to 4 s of a 10.03 s
    // sound, which has 2 s behind and 6.03 s after.
    let picture = project
        .add_clip(video, picture_media, tr(48, 48), rt(0))
        .unwrap();
    let sound = project
        .add_clip(audio, sound_media, tr_at(2_000, 2_000, ms), rt(0))
        .unwrap();
    project.link_clips(&[picture, sound]).unwrap();
    let shares = |from: ClipId, by: i64| {
        let reach = project.slip_reach(from, by).unwrap();
        let share = |clip: ClipId| {
            reach
                .iter()
                .find(|(member, _)| *member == clip)
                .map(|&(_, by)| by)
        };
        (share(picture), share(sound))
    };

    // Inside the room: twelve frames are half a second.
    assert_eq!(shares(picture, 12), (Some(12), Some(500)));
    assert_eq!(shares(sound, -500), (Some(-12), Some(-500)));
    // Past the head: both takes have two seconds behind them.
    assert_eq!(shares(picture, -1_000), (Some(-48), Some(-2_000)));
    assert_eq!(shares(sound, -1_000_000), (Some(-48), Some(-2_000)));
    // Past the tail: the sound runs out after 6.03 s, before the picture. A
    // slip led by the picture stops at the last whole frame inside it, 144
    // frames, since 145 would carry the sound 6.042 s past its 6.03 s; one
    // led by the sound takes all of it and the picture the nearest frame.
    assert_eq!(shares(picture, 1_000), (Some(144), Some(6_000)));
    assert_eq!(shares(sound, 1_000_000), (Some(145), Some(6_030)));
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

#[test]
fn retiming_or_clearing_time_remapping_prunes_broken_transitions() {
    // 1. set_clip_speed breaking a right-hand transition abutment.
    let (mut project, media_id, track) = project_with_media(500);
    let left = project.add_clip(track, media_id, tr(0, 48), rt(0)).unwrap();
    let _right = project
        .add_clip(track, media_id, tr(48, 48), rt(48))
        .unwrap();
    project.add_transition(left, "crossfade").unwrap();
    assert!(project.has_transitions());
    project
        .set_clip_speed(left, Rational::new(2, 1), false)
        .unwrap();
    assert!(
        !project.has_transitions(),
        "set_clip_speed shrank left to 0..24 and broke the abutment at 48"
    );

    // 2. set_clip_speed_curve breaking a right-hand transition abutment.
    let (mut project, media_id, track) = project_with_media(500);
    let left = project.add_clip(track, media_id, tr(0, 48), rt(0)).unwrap();
    let _right = project
        .add_clip(track, media_id, tr(48, 48), rt(48))
        .unwrap();
    project.add_transition(left, "crossfade").unwrap();
    let ramp = crate::clip::speed_preset("montage").unwrap();
    project.set_clip_speed_curve(left, Some(ramp)).unwrap();
    assert!(
        !project.has_transitions(),
        "set_clip_speed_curve changed left's duration and broke the abutment at 48"
    );

    // 3. remove_clip_attributes(TimeRemapping) breaking a right-hand transition abutment.
    let (mut project, media_id, track) = project_with_media(500);
    let left = project.add_clip(track, media_id, tr(0, 24), rt(0)).unwrap();
    project
        .set_clip_speed_curve(left, Some(Param::Constant(0.5)))
        .unwrap();
    assert_eq!(project.clip(left).unwrap().timeline, tr(0, 48));
    let _right = project
        .add_clip(track, media_id, tr(48, 48), rt(48))
        .unwrap();
    project.add_transition(left, "crossfade").unwrap();
    project
        .remove_clip_attributes(left, &[crate::ClipAttribute::TimeRemapping])
        .unwrap();
    assert_eq!(project.clip(left).unwrap().timeline, tr(0, 24));
    assert!(
        !project.has_transitions(),
        "remove_clip_attributes(TimeRemapping) shrank left to 0..24 and broke the abutment at 48"
    );
}

#[test]
fn is_media_referenced_covers_all_sequences_and_baked_snapshots() {
    // 1. Media referenced inside a secondary sequence while root is active,
    // and media referenced on the root timeline while the secondary sequence is active.
    let (mut project, root_media, root_track) = project_with_media(500);
    let nested_media = project.add_media(sample_media(R24, 500));
    project
        .add_clip(root_track, root_media, tr(0, 48), rt(0))
        .unwrap();
    let seq_id = project.create_sequence("Scene 1", R24);
    project.set_active_sequence(Some(seq_id)).unwrap();
    let seq_track = project.add_track(TrackKind::Video, "V1");
    project
        .add_clip(seq_track, nested_media, tr(0, 48), rt(0))
        .unwrap();

    // While inside `seq_id`, `root_media` on the root timeline is still referenced.
    assert!(project.is_media_referenced(root_media));
    assert_eq!(
        project.remove_media(root_media),
        Err(ModelError::MediaReferenced(root_media))
    );

    // Switch back to the root timeline: `nested_media` inside `seq_id` is still referenced.
    project.set_active_sequence(None).unwrap();
    assert!(project.is_media_referenced(nested_media));
    assert_eq!(
        project.remove_media(nested_media),
        Err(ModelError::MediaReferenced(nested_media))
    );

    // 2. Media held inside `clip.bake` of a baked clip is still referenced
    // so `restore_baked_clip` never restores a dangling MediaId.
    let (mut project, raw_media, track) = project_with_media(500);
    let rendered_media = project.add_media(sample_media(R24, 48));
    let clip = project
        .add_clip(track, raw_media, tr(0, 48), rt(0))
        .unwrap();
    project.add_effect(clip, "gaussian_blur").unwrap();
    project.bake_clip(clip, rendered_media).unwrap();
    assert!(
        project.is_media_referenced(raw_media),
        "baked snapshot still references original media for restore_baked_clip"
    );
    assert_eq!(
        project.remove_media(raw_media),
        Err(ModelError::MediaReferenced(raw_media))
    );
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
