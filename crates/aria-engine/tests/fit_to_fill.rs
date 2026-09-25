//! `FitToFill`: the four-point edit.
//!
//! The class these close: a fit has to open its span *before* the clip is
//! retimed, or the retime is refused as an overlap. So both ways of opening it
//! are asserted (ripple and cover), in both directions (a span longer and
//! shorter than the take), plus the refusal and the single undo entry.
//!
//! Not covered: fitting a clip that already carries a speed curve. A curve is
//! kept by `set_clip_speed` and asserted with the speed commands.

mod common;

use std::path::PathBuf;

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::{ApplyOutcome, Engine};
use aria_models::{ClipId, MediaId, Rational, TimeRange, TrackId, TrackKind};
use common::{add_media_clip, add_track, import_asset, rt, temp_engine};

/// A still at 1000 ticks per second on a 24 fps timeline: one second of source
/// is exactly 24 timeline ticks.
const PER_SECOND: i64 = 1000;

fn still() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

fn lane() -> (tempfile::TempDir, Engine, MediaId, TrackId, Rational) {
    let (dir, mut engine) = temp_engine();
    let media = import_asset(&mut engine, &still());
    let rate = engine.project().media(media).expect("media").frame_rate;
    let track = add_track(&mut engine, TrackKind::Video, "V1");
    (dir, engine, media, track, rate)
}

fn src(rate: Rational, seconds: i64) -> TimeRange {
    TimeRange::at_rate(0, seconds * PER_SECOND, rate)
}

fn span(engine: &Engine, clip: ClipId) -> (i64, i64) {
    let clip = engine.project().clip(clip).expect("clip present");
    (clip.timeline.start.value, clip.timeline.end_tick())
}

fn fit(
    engine: &mut Engine,
    track: TrackId,
    media: MediaId,
    source: TimeRange,
    at: i64,
    duration: i64,
    ripple: bool,
) -> Result<ClipId, aria_engine::EngineError> {
    let outcome = engine.apply(Command::Edit(EditCommand::FitToFill {
        track,
        media,
        source,
        at: rt(at),
        duration: rt(duration),
        ripple,
    }))?;
    match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => Ok(id),
        other => panic!("expected Created, got {other:?}"),
    }
}

#[test]
fn a_fit_occupies_exactly_the_span_it_was_given() {
    let (_dir, mut engine, media, track, rate) = lane();

    // One second of source stretched over two seconds of timeline.
    let new = fit(&mut engine, track, media, src(rate, 1), 0, 48, false).expect("fit");

    assert_eq!(span(&engine, new), (0, 48));
    let clip = engine.project().clip(new).expect("clip");
    assert_eq!(
        clip.speed.as_f64(),
        0.5,
        "the take is retimed rather than trimmed"
    );
}

#[test]
fn a_fit_shorter_than_the_take_speeds_it_up() {
    let (_dir, mut engine, media, track, rate) = lane();

    let new = fit(&mut engine, track, media, src(rate, 2), 0, 24, false).expect("fit");

    assert_eq!(span(&engine, new), (0, 24));
    assert_eq!(
        engine.project().clip(new).expect("clip").speed.as_f64(),
        2.0
    );
}

#[test]
fn a_fit_that_needs_no_retime_leaves_the_speed_alone() {
    let (_dir, mut engine, media, track, rate) = lane();

    let new = fit(&mut engine, track, media, src(rate, 1), 0, 24, false).expect("fit");

    assert_eq!(span(&engine, new), (0, 24));
    assert_eq!(
        engine.project().clip(new).expect("clip").speed.as_f64(),
        1.0,
        "an exact fit is not a retime"
    );
}

#[test]
fn a_covering_fit_clears_the_whole_span_not_the_take() {
    let (_dir, mut engine, media, track, rate) = lane();
    let a = add_media_clip(&mut engine, track, media, src(rate, 1), rt(0));
    let b = add_media_clip(&mut engine, track, media, src(rate, 1), rt(24));
    let c = add_media_clip(&mut engine, track, media, src(rate, 1), rt(48));

    // The take is 24 ticks; the span is 48, so `b` must go too.
    let new = fit(&mut engine, track, media, src(rate, 1), 0, 48, false).expect("fit");

    assert!(engine.project().clip(a).is_none());
    assert!(engine.project().clip(b).is_none());
    assert_eq!(span(&engine, c), (48, 72), "nothing downstream moves");
    assert_eq!(span(&engine, new), (0, 48));
}

#[test]
fn a_rippled_fit_opens_the_span_it_will_fill() {
    let (_dir, mut engine, media, track, rate) = lane();
    let a = add_media_clip(&mut engine, track, media, src(rate, 1), rt(0));

    let new = fit(&mut engine, track, media, src(rate, 1), 0, 48, true).expect("fit");

    assert_eq!(
        span(&engine, a),
        (48, 72),
        "the lane is pushed by the span, not by the take"
    );
    assert_eq!(span(&engine, new), (0, 48));
}

#[test]
fn a_fit_with_no_span_is_refused_and_changes_nothing() {
    let (_dir, mut engine, media, track, rate) = lane();
    let a = add_media_clip(&mut engine, track, media, src(rate, 1), rt(0));

    let err = fit(&mut engine, track, media, src(rate, 1), 0, 0, false).expect_err("refused");

    assert!(
        matches!(err, aria_engine::EngineError::Refused(_)),
        "got {err:?}"
    );
    assert_eq!(span(&engine, a), (0, 24));
    assert_eq!(engine.project().timeline().clip_count(), 1);
}

#[test]
fn one_undo_restores_everything_a_fit_took() {
    let (_dir, mut engine, media, track, rate) = lane();
    let a = add_media_clip(&mut engine, track, media, src(rate, 3), rt(0));

    let new = fit(&mut engine, track, media, src(rate, 1), 24, 24, false).expect("fit");
    assert_eq!(engine.project().timeline().clip_count(), 3);

    assert!(
        engine.undo(),
        "the cover, the placement and the retime are one"
    );
    assert!(engine.project().clip(new).is_none());
    assert_eq!(span(&engine, a), (0, 72));
    assert_eq!(engine.project().timeline().clip_count(), 1);

    assert!(engine.redo());
    assert_eq!(span(&engine, new), (24, 48));
}
