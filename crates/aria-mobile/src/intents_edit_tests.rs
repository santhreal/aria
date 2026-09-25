//! The gestures that edit a clip already on the timeline: cutting it, copying
//! it, retiming it from the preset catalog, and setting its levels.
//!
//! The class these close: four intents (`split`, `duplicate`, `set_speed_preset`
//! and `set_audio`) were named only by tests that need a native encoder, so on
//! a host without one — every Linux run — the gestures compiled, dispatched,
//! and were asserted on by nothing. These four cases run everywhere: the
//! fixture is synthetic, no file is read, and no frame is decoded.
//!
//! Declared from `intents.rs` with `#[path]`; the fixtures come from
//! `gesture_tests`, which owns them.

use super::gesture_tests::*;
use super::*;

/// A cut lands on the frame the gesture named and leaves the track gapless: two
/// clips of one second each, the right-hand piece reported back as the new id.
#[test]
fn a_split_cuts_on_the_frame_it_names_and_leaves_no_hole() {
    let (mut engine, main, clips) = main_with(1);
    let response = run(
        &mut engine,
        Intent::Split {
            clip: clips[0],
            seconds: 1.0,
        },
    )
    .expect("split the clip");
    let new = clip_of(&response);

    assert_eq!(
        spans(&engine, main),
        vec![(0, 30), (30, 30)],
        "the cut is at one second and neither side moved"
    );
    assert_eq!(
        order(&engine, main),
        vec![clips[0], new],
        "the new clip is the right-hand piece"
    );
    assert!(engine.undo(), "a split is one history entry");
    assert_eq!(
        spans(&engine, main),
        vec![(0, 60)],
        "undo re-joins the clip"
    );
}

/// A copy sits immediately after its original and the rest of main slides: the
/// track a lane clip cannot ripple is the one gesture that must stay gapless.
#[test]
fn a_duplicate_lands_after_its_original_and_ripples_main() {
    let (mut engine, main, clips) = main_with(2);
    let response =
        run(&mut engine, Intent::Duplicate { clip: clips[0] }).expect("duplicate the clip");
    let copy = clip_of(&response);

    assert_eq!(
        order(&engine, main),
        vec![clips[0], copy, clips[1]],
        "the copy takes the slot behind its original"
    );
    assert_eq!(
        spans(&engine, main),
        vec![(0, 60), (60, 60), (120, 60)],
        "the copy is the same length and the follower slid by exactly that much"
    );
    assert!(engine.undo(), "the whole duplication is one history entry");
    assert_eq!(spans(&engine, main), vec![(0, 60), (60, 60)]);
}

/// A retime preset is a catalog id, resolved through the catalog and readable
/// back as the same id. The ids come from the data file rather than this test,
/// because a host may ship its own `speed_presets.toml`.
#[test]
fn a_speed_preset_comes_from_the_catalog_and_an_unknown_id_is_refused() {
    let (mut engine, _, clips) = main_with(1);
    let spec = aria_models::speed_preset_catalog()
        .first()
        .expect("a shipped speed preset");

    run(
        &mut engine,
        Intent::SetSpeedPreset {
            clip: clips[0],
            preset: Some(spec.id.to_string()),
        },
    )
    .expect("apply the preset");
    assert_eq!(
        aria_models::speed_preset_id(&clip_at(&engine, clips[0]).speed_curve),
        Some(spec.id.as_str()),
        "the clip carries the curve the id names"
    );

    let refused = run(
        &mut engine,
        Intent::SetSpeedPreset {
            clip: clips[0],
            preset: Some("not-a-preset".to_owned()),
        },
    );
    assert!(
        refused.is_err(),
        "an id no catalog knows is a refusal, not a silent constant speed"
    );
    assert_eq!(
        aria_models::speed_preset_id(&clip_at(&engine, clips[0]).speed_curve),
        Some(spec.id.as_str()),
        "the refusal left the applied preset alone"
    );

    run(
        &mut engine,
        Intent::SetSpeedPreset {
            clip: clips[0],
            preset: None,
        },
    )
    .expect("clear the preset");
    let curve = &clip_at(&engine, clips[0]).speed_curve;
    assert!(!curve.is_animated(), "no preset is constant speed");
    assert_eq!(aria_models::speed_preset_id(curve), None);
}

/// The volume slider and the two fade handles land as a gain and two tick
/// counts, rounded to the frame grid by the same helper the whole module uses.
#[test]
fn levels_and_fades_land_as_a_gain_and_two_tick_counts() {
    let (mut engine, _, clips) = main_with(1);
    run(
        &mut engine,
        Intent::SetAudio {
            clip: clips[0],
            volume: Some(0.5),
            fade_in_seconds: 0.5,
            fade_out_seconds: 0.25,
        },
    )
    .expect("set the levels");

    let clip = clip_at(&engine, clips[0]);
    assert_eq!(
        clip.volume.constant(),
        Some(0.5),
        "a flat gain, not a curve"
    );
    assert_eq!(
        (clip.fade_in, clip.fade_out),
        (frame_tick(0.5, FPS), frame_tick(0.25, FPS)),
        "the fades are frame-rounded seconds"
    );
    assert_eq!((clip.fade_in, clip.fade_out), (15, 8), "at 30 fps");
    assert!(clip.has_custom_audio(), "the mix is no longer the default");

    // `volume: None` keeps the gain: the fade handles move without the slider.
    run(
        &mut engine,
        Intent::SetAudio {
            clip: clips[0],
            volume: None,
            fade_in_seconds: 0.0,
            fade_out_seconds: 0.0,
        },
    )
    .expect("clear the fades");
    let clip = clip_at(&engine, clips[0]);
    assert_eq!(clip.volume.constant(), Some(0.5), "the gain is untouched");
    assert_eq!((clip.fade_in, clip.fade_out), (0, 0));
}
