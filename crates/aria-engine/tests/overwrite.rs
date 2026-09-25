//! `OverwriteClip`: a cover, not a ripple.
//!
//! What is asserted here is the rule of overwrite rather than one timeline that
//! happens to work: for every way a clip can meet the covered span — wholly
//! inside it, reaching into it from either side, straddling it, missing it —
//! the material outside the span keeps its exact position, and nothing
//! downstream moves. The whole cover is one history entry.
//!
//! Not covered: a lane whose clips have been retimed, where the source ticks a
//! trim consumes are scaled by clip speed. That scaling lives in
//! `Project::trim_clip` and is asserted with the trim commands.

mod common;

use std::path::PathBuf;

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::{ApplyOutcome, Engine};
use aria_models::{ClipId, MediaId, Rational, TimeRange, TrackId, TrackKind};
use common::{add_media_clip, add_track, import_asset, rt, temp_engine};

/// A still at 1000 ticks per second against a 24 fps timeline: 1000 source
/// ticks is exactly 24 timeline ticks, so every span in this file is an exact
/// number of frames rather than a rounded one.
const PER_SECOND: i64 = 1000;

fn still() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// An engine with one video lane and a still in the pool.
fn lane() -> (tempfile::TempDir, Engine, MediaId, TrackId, Rational) {
    let (dir, mut engine) = temp_engine();
    let media = import_asset(&mut engine, &still());
    let rate = engine.project().media(media).expect("media").frame_rate;
    let track = add_track(&mut engine, TrackKind::Video, "V1");
    (dir, engine, media, track, rate)
}

/// `seconds` of the still, as a source range.
fn src(rate: Rational, seconds: i64) -> TimeRange {
    TimeRange::at_rate(0, seconds * PER_SECOND, rate)
}

/// `(start, end)` of a clip in timeline ticks.
fn span(engine: &Engine, clip: ClipId) -> (i64, i64) {
    let clip = engine.project().clip(clip).expect("clip present");
    (clip.timeline.start.value, clip.timeline.end_tick())
}

fn overwrite(
    engine: &mut Engine,
    track: TrackId,
    media: MediaId,
    source: TimeRange,
    at: i64,
) -> ClipId {
    let outcome = engine
        .apply(Command::Edit(EditCommand::OverwriteClip {
            track,
            media,
            source,
            at: rt(at),
        }))
        .expect("overwrite");
    match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    }
}

#[test]
fn a_clip_wholly_inside_the_span_is_removed_and_its_neighbours_stay_put() {
    let (_dir, mut engine, media, track, rate) = lane();
    let a = add_media_clip(&mut engine, track, media, src(rate, 1), rt(0));
    let b = add_media_clip(&mut engine, track, media, src(rate, 1), rt(24));
    let c = add_media_clip(&mut engine, track, media, src(rate, 1), rt(48));

    let new = overwrite(&mut engine, track, media, src(rate, 1), 24);

    assert!(
        engine.project().clip(b).is_none(),
        "the covered clip is gone"
    );
    assert_eq!(span(&engine, a), (0, 24), "the head clip does not move");
    assert_eq!(
        span(&engine, c),
        (48, 72),
        "overwrite is not a ripple: nothing downstream moves"
    );
    assert_eq!(span(&engine, new), (24, 48));
    assert_eq!(engine.project().timeline().clip_count(), 3);
}

#[test]
fn a_clip_the_span_reaches_into_is_trimmed_to_the_span_start() {
    let (_dir, mut engine, media, track, rate) = lane();
    let a = add_media_clip(&mut engine, track, media, src(rate, 2), rt(0));

    let new = overwrite(&mut engine, track, media, src(rate, 1), 24);

    assert_eq!(
        span(&engine, a),
        (0, 24),
        "the part outside the span survives"
    );
    assert_eq!(span(&engine, new), (24, 48));
    assert_eq!(engine.project().timeline().clip_count(), 2);
}

#[test]
fn a_clip_reaching_out_of_the_span_keeps_its_tail() {
    let (_dir, mut engine, media, track, rate) = lane();
    let a = add_media_clip(&mut engine, track, media, src(rate, 2), rt(24));

    let new = overwrite(&mut engine, track, media, src(rate, 2), 0);

    assert_eq!(
        span(&engine, a),
        (48, 72),
        "the clip is trimmed forward, not pushed"
    );
    assert_eq!(span(&engine, new), (0, 48));
    assert_eq!(engine.project().timeline().clip_count(), 2);
}

#[test]
fn a_straddled_clip_survives_on_both_sides_of_the_span() {
    let (_dir, mut engine, media, track, rate) = lane();
    add_media_clip(&mut engine, track, media, src(rate, 3), rt(0));

    let new = overwrite(&mut engine, track, media, src(rate, 1), 24);

    let mut spans: Vec<(i64, i64)> = engine
        .project()
        .timeline()
        .track(track)
        .expect("lane")
        .clips_ordered()
        .into_iter()
        .map(|clip| (clip.timeline.start.value, clip.timeline.end_tick()))
        .collect();
    spans.sort_unstable();
    assert_eq!(
        spans,
        vec![(0, 24), (24, 48), (48, 72)],
        "head, the new clip, and the tail"
    );
    assert_eq!(span(&engine, new), (24, 48));
}

#[test]
fn a_span_that_touches_nothing_only_adds() {
    let (_dir, mut engine, media, track, rate) = lane();
    let a = add_media_clip(&mut engine, track, media, src(rate, 1), rt(0));

    let new = overwrite(&mut engine, track, media, src(rate, 1), 48);

    assert_eq!(span(&engine, a), (0, 24));
    assert_eq!(span(&engine, new), (48, 72));
    assert_eq!(engine.project().timeline().clip_count(), 2);
}

/// An edge-to-edge cover is a remove, not a straddle: a clip whose bounds are
/// exactly the span has nothing left to show.
#[test]
fn an_exact_cover_removes_rather_than_splits() {
    let (_dir, mut engine, media, track, rate) = lane();
    let a = add_media_clip(&mut engine, track, media, src(rate, 1), rt(0));

    overwrite(&mut engine, track, media, src(rate, 1), 0);

    assert!(engine.project().clip(a).is_none());
    assert_eq!(engine.project().timeline().clip_count(), 1);
}

#[test]
fn one_undo_puts_back_everything_the_cover_took() {
    let (_dir, mut engine, media, track, rate) = lane();
    let a = add_media_clip(&mut engine, track, media, src(rate, 3), rt(0));
    let b = add_media_clip(&mut engine, track, media, src(rate, 1), rt(96));

    let new = overwrite(&mut engine, track, media, src(rate, 1), 24);
    assert_eq!(engine.project().timeline().clip_count(), 4);

    assert!(engine.undo(), "one entry for the whole cover");
    assert!(engine.project().clip(new).is_none());
    assert_eq!(
        span(&engine, a),
        (0, 72),
        "the straddled clip is whole again"
    );
    assert_eq!(span(&engine, b), (96, 120));
    assert_eq!(engine.project().timeline().clip_count(), 2);

    assert!(engine.redo());
    assert_eq!(engine.project().timeline().clip_count(), 4);
    assert_eq!(span(&engine, new), (24, 48));
}

#[test]
fn straddle_clear_partial_failure_rolls_back_atomically() {
    let (_dir, mut engine, media, track, rate) = lane();
    let a = add_media_clip(&mut engine, track, media, src(rate, 3), rt(0));

    // An overwrite with an invalid media id will fail at add_clip, after the
    // straddle clear step. The rollback must restore clip a as whole, with no
    // partial head/tail split leftovers.
    let bogus_media = aria_models::MediaId::next();
    let res = engine.apply(Command::Edit(EditCommand::OverwriteClip {
        track,
        media: bogus_media,
        source: src(rate, 1),
        at: rt(24),
    }));
    assert!(res.is_err(), "placement of unknown media fails");

    // Clip a must be restored to its pre-overwrite whole span (0..72)
    assert_eq!(
        span(&engine, a),
        (0, 72),
        "rollback restored the straddled clip"
    );
    assert_eq!(engine.project().timeline().clip_count(), 1);
}
