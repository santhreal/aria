//! Render and replace, and the way back.
//!
//! Closes defect class: a bake that cannot be undone once the baked clip has
//! been touched. Swapping the content is the easy half; every case here is
//! about the return, because that is where an offset can be dropped, applied
//! twice, or applied in the wrong time base and still look plausible.
//!
//! What these cases cover beyond the reported need:
//!
//! - The round trip is asserted on the whole clip, not on the fields the bake
//!   happens to reset, so a look field added later is covered by existing
//!   cases rather than by remembering to extend a list.
//! - The offset cases are parameterized over trim, move, and retimed-original,
//!   which are the three ways the mapping can be wrong independently.
//! - A split baked clip restores both halves, since a split copies the snapshot
//!   into two clips that must each read their own offset.
//! - Every refusal is asserted by its wording, and the lane-bar refusal is
//!   driven from the catalog so a ninth generator is covered when it is added.

use super::*;
use crate::clip::{Generator, generator_specs};

const R: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, R)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, R)
}

/// A project holding one 48-tick media clip starting at tick 100, dressed with
/// a look worth rendering, plus a rendered intermediate in the pool that covers
/// it.
fn with_clip() -> (Project, ClipId, MediaId) {
    let mut project = Project::new("bake", R);
    let track = project.add_track(TrackKind::Video, "V1");
    let source = project.add_media(MediaSource::new("/tmp/shot.mp4", 1920, 1080, R, 600, true));
    let clip = project
        .add_clip(track, source, tr(200, 48), rt(100))
        .expect("the clip");
    let rendered = project.add_media(MediaSource::new(
        "/tmp/shot-rendered.mov",
        1920,
        1080,
        R,
        48,
        true,
    ));
    dress(&mut project, clip);
    (project, clip, rendered)
}

/// Give the clip a look worth baking: a keyframed transform, an effect, a
/// grade, and a fade.
fn dress(project: &mut Project, clip: ClipId) {
    project
        .set_param_keyframe(
            clip,
            ClipParam::Opacity,
            rt(100),
            ParamValue::Scalar(0.2),
            Easing::Linear,
            None,
        )
        .expect("opacity keyframe");
    project
        .set_param_keyframe(
            clip,
            ClipParam::Opacity,
            rt(140),
            ParamValue::Scalar(1.0),
            Easing::Linear,
            None,
        )
        .expect("opacity keyframe");
    project.add_effect(clip, "gaussian_blur").expect("effect");
    project
        .set_clip_audio(clip, None, rt(6), rt(6))
        .expect("fades");
}

fn clip_of(project: &Project, clip: ClipId) -> &Clip {
    project.timeline().clip(clip).expect("the clip")
}

#[test]
fn a_bake_swaps_the_content_and_spends_the_look() {
    let (mut project, clip, rendered) = with_clip();
    let before = Clip::clone(clip_of(&project, clip));

    project.bake_clip(clip, rendered).expect("bake");

    let baked = clip_of(&project, clip);
    assert_eq!(
        baked.content,
        ClipSource::Media {
            media: rendered,
            source: tr(0, 48)
        },
        "the baked clip does not decode the rendered file from its first frame"
    );
    assert!(baked.effects.is_empty(), "the effect stack was not spent");
    assert!(
        !baked.transform.is_animated(),
        "the transform animation was not spent"
    );
    assert_eq!(baked.fade_in, 0, "the fade was not spent");
    assert_eq!(baked.timeline, before.timeline, "the clip moved");
    assert_eq!(
        baked.bake.as_deref(),
        Some(&before),
        "the snapshot is not what the bake replaced"
    );
}

#[test]
fn a_restore_puts_back_the_whole_clip() {
    let (mut project, clip, rendered) = with_clip();
    let before = Clip::clone(clip_of(&project, clip));

    project.bake_clip(clip, rendered).expect("bake");
    project.restore_baked_clip(clip).expect("restore");

    assert_eq!(
        clip_of(&project, clip),
        &before,
        "the restored clip is not the clip that was baked"
    );
}

/// The mapping case. Each row trims or moves the baked clip, then restores it,
/// and states the source window the original must come back with.
#[test]
fn a_trim_on_the_baked_clip_lands_on_the_original() {
    // (label, head trim in ticks, expected source in-point, expected duration)
    let rows: &[(&str, i64, i64, i64)] = &[
        ("untouched", 0, 200, 48),
        ("trimmed 12 in", 12, 212, 36),
        ("trimmed 47 in", 47, 247, 1),
    ];
    for (label, trim, expected_start, expected_duration) in rows {
        let (mut project, clip, rendered) = with_clip();
        project.bake_clip(clip, rendered).expect("bake");
        if *trim != 0 {
            project
                .trim_clip(clip, tr(100 + trim, 48 - trim))
                .unwrap_or_else(|err| panic!("{label}: {err}"));
        }
        project
            .restore_baked_clip(clip)
            .unwrap_or_else(|err| panic!("{label}: {err}"));

        let restored = clip_of(&project, clip);
        assert_eq!(
            restored.source_range(),
            Some(tr(*expected_start, *expected_duration)),
            "{label}: the restored source window is not the trimmed window"
        );
        assert_eq!(
            restored.timeline,
            tr(100 + trim, 48 - trim),
            "{label}: the restored clip is not where the baked clip was"
        );
    }
}

#[test]
fn a_trim_on_a_retimed_original_maps_through_its_speed() {
    let (mut project, clip, rendered) = with_clip();
    // Half speed: 48 timeline ticks now consume 24 source ticks.
    project
        .set_clip_speed(clip, Rational::new(1, 2), false)
        .expect("speed");
    let timeline = clip_of(&project, clip).timeline;
    let rendered_long = project.add_media(MediaSource::new(
        "/tmp/slow-rendered.mov",
        1920,
        1080,
        R,
        timeline.duration.value,
        true,
    ));
    project.bake_clip(clip, rendered_long).expect("bake");
    assert!(
        project.media(rendered).is_some(),
        "the fixture's own intermediate went missing"
    );

    // Trim 24 timeline ticks off the head: at half speed that is 12 source
    // ticks into the original, not 24.
    project
        .trim_clip(
            clip,
            tr(timeline.start.value + 24, timeline.duration.value - 24),
        )
        .expect("trim");
    project.restore_baked_clip(clip).expect("restore");

    let restored = clip_of(&project, clip);
    assert_eq!(
        restored.source_range().map(|range| range.start.value),
        Some(212),
        "the restored in-point did not go through the original's speed"
    );
    assert_eq!(
        restored.speed,
        Rational::new(1, 2),
        "the restored clip lost its retiming"
    );
}

#[test]
fn a_moved_bake_restores_where_it_was_moved_to() {
    let (mut project, clip, rendered) = with_clip();
    project.bake_clip(clip, rendered).expect("bake");
    let track = project.timeline().track_of(clip).expect("the track");
    project.move_clip(clip, track, rt(500)).expect("move");
    project.restore_baked_clip(clip).expect("restore");

    let restored = clip_of(&project, clip);
    assert_eq!(
        restored.timeline,
        tr(500, 48),
        "the restored clip did not stay where the baked clip was moved"
    );
    assert_eq!(
        restored.source_range(),
        Some(tr(200, 48)),
        "moving a bake changed which part of the source comes back"
    );
}

#[test]
fn both_halves_of_a_split_bake_restore_their_own_window() {
    let (mut project, clip, rendered) = with_clip();
    project.bake_clip(clip, rendered).expect("bake");
    let right = project.split_clip(clip, rt(124)).expect("split");

    project.restore_baked_clip(clip).expect("restore left");
    project.restore_baked_clip(right).expect("restore right");

    assert_eq!(
        clip_of(&project, clip).source_range(),
        Some(tr(200, 24)),
        "the left half restored the wrong window"
    );
    assert_eq!(
        clip_of(&project, right).source_range(),
        Some(tr(224, 24)),
        "the right half restored the wrong window"
    );
}

#[test]
fn a_lane_bar_cannot_be_baked() {
    let bars: Vec<&'static crate::clip::GeneratorSpec> = generator_specs()
        .iter()
        .filter(|spec| !spec.paints)
        .collect();
    assert!(!bars.is_empty(), "the catalog declares no lane bar");
    for spec in bars {
        let mut project = Project::new("bars", R);
        let track = project.add_track(spec.lane, "L1");
        let bar = project
            .add_generated(track, (spec.census)(), tr(0, 48))
            .expect("the bar");
        let rendered = project.add_media(MediaSource::new("/tmp/bar.mov", 1920, 1080, R, 48, true));

        let err = project.bake_clip(bar, rendered).expect_err("refusal");
        assert_eq!(
            err,
            ModelError::InvalidParam("a lane bar has no picture of its own to render".into()),
            "'{}' was baked instead of refused",
            spec.id
        );
    }
}

#[test]
fn a_painting_generator_can_be_baked() {
    let mut project = Project::new("title", R);
    let track = project.add_track(TrackKind::Text, "T1");
    let title = project
        .add_generated(track, Generator::text("hello"), tr(0, 48))
        .expect("the title");
    let rendered = project.add_media(MediaSource::new("/tmp/title.mov", 1920, 1080, R, 48, true));

    project.bake_clip(title, rendered).expect("bake");
    assert!(
        clip_of(&project, title).is_baked(),
        "a title that draws its own pixels was not baked"
    );

    project.restore_baked_clip(title).expect("restore");
    assert!(
        matches!(
            clip_of(&project, title).content,
            ClipSource::Generated(Generator::Text { .. })
        ),
        "the title did not come back"
    );
}

/// Every refusal, by its wording: a message that does not name what is missing
/// is the same defect as no message.
#[test]
fn every_refusal_names_what_is_wrong() {
    let (mut project, clip, rendered) = with_clip();

    assert_eq!(
        project.restore_baked_clip(clip).expect_err("not baked"),
        ModelError::InvalidParam("that clip was not rendered and replaced".into())
    );

    let wrong_rate = project.add_media(MediaSource::new(
        "/tmp/thirty.mov",
        1920,
        1080,
        Rational::new(30, 1),
        48,
        true,
    ));
    assert_eq!(
        project.bake_clip(clip, wrong_rate).expect_err("rate"),
        ModelError::RateMismatch {
            expected: R,
            got: Rational::new(30, 1)
        }
    );

    let too_short = project.add_media(MediaSource::new("/tmp/short.mov", 1920, 1080, R, 20, true));
    assert_eq!(
        project.bake_clip(clip, too_short).expect_err("length"),
        ModelError::InvalidParam("the rendered file covers 20 of the clip's 48 ticks".into())
    );

    assert_eq!(
        project
            .bake_clip(clip, MediaId::from_raw(9_999))
            .expect_err("unknown media"),
        ModelError::UnknownMedia(MediaId::from_raw(9_999))
    );

    let plain_track = project.add_track(TrackKind::Video, "V2");
    let plain_source =
        project.add_media(MediaSource::new("/tmp/plain.mp4", 1920, 1080, R, 600, true));
    let plain = project
        .add_clip(plain_track, plain_source, tr(0, 48), rt(100))
        .expect("a clip with nothing on it");
    assert_eq!(
        project
            .bake_clip(plain, rendered)
            .expect_err("nothing on it"),
        ModelError::InvalidParam(
            "nothing is applied to that clip, so rendering it would only re-encode it".into()
        )
    );

    project.bake_clip(clip, rendered).expect("bake");
    assert_eq!(
        project.bake_clip(clip, rendered).expect_err("twice"),
        ModelError::InvalidParam(
            "that clip is already rendered; restore it before rendering it again".into()
        )
    );

    project
        .set_clip_speed(clip, Rational::new(2, 1), false)
        .expect("speed");
    assert_eq!(
        project.restore_baked_clip(clip).expect_err("retimed"),
        ModelError::InvalidParam(
            "that clip was retimed after it was rendered; undo the retime before restoring it"
                .into()
        )
    );
}

/// Milliseconds: the rate a file with no picture in it is counted at, which is
/// what the probe reports for audio-only media.
const MS: Rational = Rational::new(1000, 1);

/// A one-second sound clip at tick 100 of a 24 fps sequence, dressed with a
/// fade, plus a one-second sound intermediate in the pool.
///
/// The intermediate counts 1000 ticks where the sequence counts 24, which is
/// the whole point: one-to-one is a claim about time, not about numbers.
fn with_sound() -> (Project, ClipId, MediaId) {
    let mut project = Project::new("bake", R);
    let track = project.add_track(TrackKind::Audio, "A1");
    let source = project.add_media(MediaSource::new("/tmp/song.wav", 0, 0, MS, 10_000, true));
    let clip = project
        .add_clip(track, source, TimeRange::at_rate(2_000, 1_000, MS), rt(100))
        .expect("the clip");
    project
        .set_clip_audio(clip, None, rt(6), rt(6))
        .expect("fades");
    let rendered = project.add_media(MediaSource::new(
        "/tmp/song-rendered.wav",
        0,
        0,
        MS,
        1_000,
        true,
    ));
    (project, clip, rendered)
}

/// A sound intermediate stands in for the clip and the whole clip comes back,
/// even though the file counts in its own rate.
#[test]
fn a_sound_bake_swaps_in_and_restores_whole() {
    let (mut project, clip, rendered) = with_sound();
    let before = Clip::clone(clip_of(&project, clip));

    project.bake_clip(clip, rendered).expect("bake");
    let baked = clip_of(&project, clip);
    assert_eq!(
        baked.source_range(),
        Some(TimeRange::at_rate(0, 1_000, MS)),
        "the file holds the clip's span from its first sample"
    );
    assert_eq!(
        baked.timeline,
        tr(100, 24),
        "the baked clip occupies what it replaced"
    );
    assert_eq!(baked.fade_in, 0, "the file carries the fade");

    project.restore_baked_clip(clip).expect("restore");
    assert_eq!(
        clip_of(&project, clip),
        &before,
        "the restored clip is not the clip that was rendered"
    );
}

/// The mapping case for sound. A quarter of a second off the head is 6 ticks of
/// programme and 250 ticks of the file, and the original has to come back at the
/// former: reading the offset as the file's own number would land ten seconds
/// into the source.
#[test]
fn a_trim_on_a_sound_bake_maps_through_the_files_rate() {
    let (mut project, clip, rendered) = with_sound();
    project.bake_clip(clip, rendered).expect("bake");
    project.trim_clip(clip, tr(106, 18)).expect("trim");

    let baked = clip_of(&project, clip);
    assert_eq!(
        baked.source_range(),
        Some(TimeRange::at_rate(250, 750, MS)),
        "the trim lands in the file at the file's own rate"
    );

    project.restore_baked_clip(clip).expect("restore");
    let restored = clip_of(&project, clip);
    assert_eq!(
        restored.source_range(),
        Some(TimeRange::at_rate(2_250, 750, MS)),
        "the restored source window is not the trimmed window"
    );
    assert_eq!(
        restored.timeline,
        tr(106, 18),
        "the restored clip is not where the baked clip was"
    );
}

/// Anything with a picture is still held to the sequence rate. A picture file at
/// another rate is not the clip's frames one-to-one, and resampling it would put
/// the wrong frame under every tick, so it is refused rather than accepted the
/// way a sound file is.
#[test]
fn a_picture_intermediate_at_another_rate_is_still_refused() {
    let (mut project, clip, _) = with_clip();
    let wrong = project.add_media(MediaSource::new(
        "/tmp/shot-rendered-30.mov",
        1920,
        1080,
        Rational::FPS_30,
        60,
        true,
    ));
    assert_eq!(
        project.bake_clip(clip, wrong).expect_err("another rate"),
        ModelError::RateMismatch {
            expected: R,
            got: Rational::FPS_30,
        }
    );
}
