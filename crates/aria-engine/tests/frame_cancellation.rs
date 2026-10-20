//! Abandoning a frame the scrub has already passed.
//!
//! The class these close: work that keeps running after nobody wants it. A
//! preview frame holds the engine lock for its whole length, so a frame that
//! cannot be stopped is a frame the *next* one waits for — the scrub feels slow
//! not because a frame is slow but because three stale ones are queued in front
//! of the wanted one.
//!
//! What they do not catch: how far into a frame the check lands on a real file.
//! The unit is a scene layer, so a single-layer frame is either abandoned before
//! it starts or paid for in full; the win grows with the layer count, and
//! measuring it needs a machine with a GPU. What is asserted here is the
//! contract every one of those cases rests on: a cancelled frame writes nothing,
//! says it was cancelled, and leaves the engine usable.

mod common;

use aria_commands::{Command, EditCommand};
use aria_core::Cancel;
use aria_engine::EngineError;
use aria_models::TrackKind;
use common::{import_asset, rt, small_video_asset, temp_engine, tr};

/// A one-clip sequence, or `None` when this machine has no video fixture.
fn one_clip_sequence() -> Option<(tempfile::TempDir, aria_engine::Engine)> {
    let path = small_video_asset()?;
    let (dir, mut engine) = temp_engine();
    let media = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");
    engine
        .apply(Command::Edit(EditCommand::AddClip {
            track,
            media,
            source: tr(0, 48),
            start: rt(0),
        }))
        .expect("place the clip");
    Some((dir, engine))
}

#[test]
fn a_cancelled_frame_is_refused_by_name() {
    let Some((_dir, mut engine)) = one_clip_sequence() else {
        return;
    };
    let cancel = Cancel::new();
    cancel.cancel();
    engine.cancel_next_frame(cancel);

    let err = engine
        .get_frame(rt(0))
        .expect_err("a cancelled frame must not return a picture");
    assert!(
        err.is_cancelled(),
        "a cancelled frame reported {err} instead of a cancellation"
    );
    assert!(
        matches!(
            err,
            EngineError::Render(aria_render::RenderError::Cancelled)
        ),
        "the refusal is not the render pipeline's cancellation: {err}"
    );
}

#[test]
fn the_token_is_spent_by_the_frame_that_reads_it() {
    let Some((_dir, mut engine)) = one_clip_sequence() else {
        return;
    };
    let cancel = Cancel::new();
    cancel.cancel();
    engine.cancel_next_frame(cancel);
    assert!(
        engine.get_frame(rt(0)).is_err(),
        "the armed frame was not cancelled"
    );

    // The next frame is a different frame. A token left behind must not cancel
    // it: that is the failure mode of holding cancellation on the renderer.
    engine
        .get_frame(rt(0))
        .expect("the frame after a cancelled one renders");
}

#[test]
fn a_token_that_never_fires_costs_a_frame_nothing() {
    let Some((_dir, mut engine)) = one_clip_sequence() else {
        return;
    };
    let cancel = Cancel::new();
    engine.cancel_next_frame(cancel.clone());
    engine
        .get_frame(rt(0))
        .expect("an uncancelled frame renders");
    // And cancelling after the frame landed changes nothing: the token is gone.
    cancel.cancel();
    engine
        .get_frame(rt(0))
        .expect("a later frame is not cancelled by a spent token");
}

#[test]
fn cancelling_between_the_arm_and_the_frame_still_stops_it() {
    let Some((_dir, mut engine)) = one_clip_sequence() else {
        return;
    };
    // What a scrub does: the token is armed, the playhead moves,
    // and only then does the frame start reading it.
    let cancel = Cancel::new();
    engine.cancel_next_frame(cancel.clone());
    cancel.cancel();
    let err = engine.get_frame(rt(0)).expect_err("cancelled after arming");
    assert!(err.is_cancelled(), "{err} is not a cancellation");
}

#[test]
fn a_cancelled_frame_leaves_the_engine_usable() {
    let Some((_dir, mut engine)) = one_clip_sequence() else {
        return;
    };
    let good = engine.get_frame(rt(0)).expect("a first frame");

    let cancel = Cancel::new();
    cancel.cancel();
    engine.cancel_next_frame(cancel);
    assert!(engine.get_frame(rt(0)).is_err());

    // Same time, same project: a cancelled frame must not have left a decoder
    // mid-seek or a cache half-written.
    let again = engine.get_frame(rt(0)).expect("a frame after the refusal");
    assert_eq!(
        (again.width, again.height),
        (good.width, good.height),
        "the frame after a cancellation is a different size"
    );
    assert_eq!(
        again.pixels, good.pixels,
        "the frame after a cancellation is not the frame that was asked for"
    );
}

#[test]
fn an_edit_still_applies_after_a_cancelled_frame() {
    let Some((_dir, mut engine)) = one_clip_sequence() else {
        return;
    };
    let cancel = Cancel::new();
    cancel.cancel();
    engine.cancel_next_frame(cancel);
    assert!(engine.get_frame(rt(0)).is_err());

    // The frame was abandoned, not the session: the lock came back and the
    // project is untouched.
    let before = engine.revision();
    common::add_track(&mut engine, TrackKind::Audio, "A1");
    assert!(
        engine.revision() > before,
        "the engine stopped accepting edits after a cancelled frame"
    );
}
