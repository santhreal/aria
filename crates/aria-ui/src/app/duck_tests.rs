//! Auto-duck: the curve geometry, the silence inversion, and the refusals.
//!
//! The measurement itself is not covered here and cannot be: the only audio
//! fixture in the tree is a silent still image, so a decode would produce one
//! silent span and prove nothing about a voice. What is covered is everything
//! between the detector and the project — the part that turns spans into an
//! envelope, and the part that decides a duck may not run at all.

use aria_models::{ClipId, Rational, TrackKind};

use super::duck::{DuckForm, duck_curve, voice_spans};
use super::premiere_tests::with_clip;
use crate::types::UiTrackKind;

/// A silent still has no voice, and a duck with nothing to measure must say so
/// rather than write a flat envelope.
#[test]
fn a_curve_with_no_voice_is_no_curve() {
    assert!(duck_curve(&[], (0, 240), 1.0, 0.3, 12, 24).is_empty());
    // A window a keyframe cannot land in, either.
    assert!(duck_curve(&[(10, 20)], (0, 0), 1.0, 0.3, 12, 24).is_empty());
}

/// One sentence is four keyframes: down before it, held across it, back up after.
#[test]
fn one_voice_span_ramps_down_and_back_up() {
    let points = duck_curve(&[(100, 160)], (0, 480), 1.0, 0.25, 12, 24);
    assert_eq!(
        points,
        vec![(88, 1.0), (100, 0.25), (160, 0.25), (184, 1.0)],
        "the ramp does not start before the voice and end after it"
    );
}

/// The class this closes: two sentences with a breath between them used to be
/// two dips, and the music rose to full between them for a fifth of a second.
/// With less room than the ramps need, the dip is held across the gap.
#[test]
fn spans_closer_than_the_ramps_are_one_dip() {
    let points = duck_curve(&[(100, 160), (170, 220)], (0, 480), 1.0, 0.25, 12, 24);
    assert_eq!(
        points,
        vec![(88, 1.0), (100, 0.25), (220, 0.25), (244, 1.0)],
        "a gap shorter than release+attack was not merged"
    );

    // Far enough apart, they stay two dips: the music does come back up.
    let apart = duck_curve(&[(100, 160), (300, 340)], (0, 480), 1.0, 0.25, 12, 24);
    assert_eq!(
        apart,
        vec![
            (88, 1.0),
            (100, 0.25),
            (160, 0.25),
            (184, 1.0),
            (288, 1.0),
            (300, 0.25),
            (340, 0.25),
            (364, 1.0),
        ]
    );
}

/// Voice that starts before the clip does means the clip opens already ducked.
/// The alternative is a keyframe outside the clip, which the engine refuses, and
/// a dip that never arrives.
#[test]
fn voice_over_the_clip_edges_clamps_to_the_deeper_value() {
    let points = duck_curve(&[(-500, 40)], (0, 240), 1.0, 0.25, 12, 24);
    assert_eq!(
        points,
        vec![(0, 0.25), (40, 0.25), (64, 1.0)],
        "the clip did not open ducked"
    );

    // And voice still running at the tail leaves it ducked to the last tick.
    let tail = duck_curve(&[(200, 900)], (0, 240), 1.0, 0.25, 12, 24);
    assert_eq!(
        tail,
        vec![(188, 1.0), (200, 0.25), (239, 0.25)],
        "the clip did not stay ducked to its last tick"
    );
}

/// Every span is clipped to the window, so voice entirely outside it writes
/// nothing at all rather than a keyframe pinned to an edge.
#[test]
fn voice_outside_the_window_writes_nothing() {
    assert!(duck_curve(&[(600, 700)], (0, 240), 1.0, 0.25, 12, 24).is_empty());
    assert!(duck_curve(&[(-200, -10)], (0, 240), 1.0, 0.25, 12, 24).is_empty());
}

/// Curve work is bounded by voice spans, not by the duration of the timeline
/// window. An effectively unbounded project still emits four points for one
/// sentence.
#[test]
fn ducking_an_unbounded_timeline_visits_only_voice_spans() {
    assert_eq!(
        duck_curve(&[(100, 160)], (0, i64::MAX), 1.0, 0.25, 12, 24),
        vec![(88, 1.0), (100, 0.25), (160, 0.25), (184, 1.0)]
    );
}

/// The dip is a share of the clip's own level, not an absolute gain: ducking a
/// clip already at half volume must not raise it.
#[test]
fn the_dip_is_relative_to_the_clips_own_level() {
    let points = duck_curve(&[(100, 160)], (0, 480), 0.5, 0.5 * 0.3, 12, 24);
    assert_eq!(points[0].1, 0.5, "the base is not the clip's own level");
    assert!(
        points[1].1 < 0.2,
        "the dip is not a share of the base: {:?}",
        points[1]
    );
}

#[test]
fn duck_curve_properties_negative_ticks_zero_attack_release_and_sub_tick_gaps() {
    // Negative ticks: window and spans entirely in negative timeline space
    let neg = duck_curve(&[(-300, -200)], (-400, -100), 1.0, 0.25, 10, 20);
    assert_eq!(
        neg,
        vec![(-310, 1.0), (-300, 0.25), (-200, 0.25), (-180, 1.0)]
    );

    // Zero attack and release: transitions are instantaneous at the span boundaries
    let zero_ramp = duck_curve(&[(100, 160)], (0, 480), 1.0, 0.25, 0, 0);
    assert_eq!(zero_ramp, vec![(100, 0.25), (160, 0.25)]);

    // Sub-tick / contiguous spans: adjacent spans merge smoothly into a single dip
    let contiguous = duck_curve(&[(100, 150), (150, 200)], (0, 480), 1.0, 0.25, 10, 20);
    assert_eq!(
        contiguous,
        vec![(90, 1.0), (100, 0.25), (200, 0.25), (220, 1.0)]
    );

    let adjacent = duck_curve(&[(100, 150), (151, 200)], (0, 480), 1.0, 0.25, 10, 20);
    assert_eq!(
        adjacent,
        vec![(90, 1.0), (100, 0.25), (200, 0.25), (220, 1.0)]
    );
}

/// A duck needs a selection, a flagged voice lane, and something on it. Each
/// refusal names what to do next, and none of them is reachable only through a
/// window.
#[test]
fn a_duck_refuses_before_it_costs_a_decode() {
    let (mut state, clip) = with_clip();
    state.clear_selection();
    assert_eq!(
        state.duck_precheck().unwrap_err(),
        "Select the music clip or clips to duck"
    );

    state.select_clip(clip);
    // The fixture clip is on a video lane, and no lane is the duck source yet.
    assert_eq!(
        state.duck_precheck().unwrap_err(),
        "Flag the voice lane as the duck source first"
    );

    state.add_track(TrackKind::Audio, "Voice").expect("lane");
    let voice = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.kind == UiTrackKind::Audio)
        .map(|track| aria_models::TrackId::from_raw(track.id))
        .expect("an audio lane");
    state.toggle_track_duck_source(voice).expect("flag it");
    assert_eq!(state.duck_source_name().as_deref(), Some("Voice"));
    // Flagged, but empty: there is nothing to measure the dips from.
    assert_eq!(
        state.duck_precheck().unwrap_err(),
        "Voice has no audio to measure"
    );
}

#[test]
fn a_duck_resolves_voice_media_by_id() {
    let (mut state, _) = with_clip();
    let (media_id, media_path) = {
        let media = state.store_mut().media.first_mut().expect("fixture media");
        media.has_audio = true;
        media.missing = false;
        (media.id, media.path.clone())
    };

    let template = state
        .store
        .sequence
        .tracks
        .first()
        .cloned()
        .expect("fixture track");
    let mut music = template.clone();
    music.id = 101;
    music.kind = UiTrackKind::Audio;
    music.name = "Music".into();
    music.duck_source = false;
    music.clips[0].id = 201;
    music.clips[0].track_id = music.id;
    music.clips[0].media_id = Some(media_id);

    let mut voice = template;
    voice.id = 102;
    voice.kind = UiTrackKind::Audio;
    voice.name = "Voice".into();
    voice.duck_source = true;
    voice.clips[0].id = 202;
    voice.clips[0].track_id = voice.id;
    voice.clips[0].media_id = Some(media_id);

    state.store_mut().sequence.tracks = vec![music, voice];
    state.selection = vec![ClipId::from_raw(201)];
    let request = state.duck_precheck().expect("ready duck");
    assert_eq!(request.music, vec![ClipId::from_raw(201)]);
    assert_eq!(request.voice.len(), 1);
    assert_eq!(request.voice[0].clip, ClipId::from_raw(202));
    assert_eq!(request.voice[0].path.to_string_lossy(), media_path.as_ref());
}

/// The panel's three choices cycle and wrap, so a click always changes the
/// value it names.
#[test]
fn the_duck_choices_cycle() {
    let (mut state, _) = with_clip();
    assert_eq!(state.duck.amount_label(), "30%");
    state.cycle_duck_amount();
    assert_eq!(state.duck.amount, 0.5);
    state.cycle_duck_amount();
    assert_eq!(state.duck.amount, 0.7);
    state.cycle_duck_amount();
    assert_eq!(state.duck.amount, 0.1, "the amount did not wrap");

    assert_eq!(state.duck.attack_label(), "150 ms");
    state.cycle_duck_attack();
    assert_eq!(state.duck.attack_seconds, 0.25);
    state.cycle_duck_release();
    assert_eq!(state.duck.release_seconds, 1.0);
    state.cycle_duck_release();
    assert_eq!(state.duck.release_seconds, 0.05, "the release did not wrap");
}

/// Voice spans are the complement of silence over the decoded length, in the
/// media's own source ticks. A recording that is silent end to end inverts to
/// nothing rather than to one span covering everything.
#[test]
fn silence_inverts_to_voice_in_source_ticks() {
    let rate = Rational::new(1000, 1);
    // Silence 0-1s and 3-4s of a 4s file: voice is the 1-3s middle.
    assert_eq!(
        voice_spans(&[(0.0, 1.0), (3.0, 4.0)], 4.0, rate),
        vec![(1000, 3000)]
    );

    // Nothing but silence: no voice at all.
    assert!(voice_spans(&[(0.0, 4.0)], 4.0, rate).is_empty());

    // Nothing detected: the whole file is voice.
    assert_eq!(voice_spans(&[], 2.5, rate), vec![(0, 2500)]);

    // Overlapping and unsorted spans are still inverted correctly.
    assert_eq!(
        voice_spans(&[(2.0, 3.0), (0.0, 1.5), (1.0, 2.5)], 4.0, rate),
        vec![(3000, 4000)]
    );
}

/// The default form is the shipped one, so a panel that never changed it still
/// writes a broadcast-sane dip.
#[test]
fn the_default_form_is_a_usable_duck() {
    let form = DuckForm::default();
    assert!(DuckForm::AMOUNTS.contains(&form.amount));
    assert!(DuckForm::TIMES.contains(&form.attack_seconds));
    assert!(DuckForm::TIMES.contains(&form.release_seconds));
    assert!(
        form.attack_seconds < form.release_seconds,
        "a duck that comes back up faster than it dips sounds like a mistake"
    );
}
