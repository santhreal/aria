//! Auto-duck: the curve geometry, the silence inversion, the refusals, and one
//! run end to end.
//!
//! The run decodes the 48 kHz sine fixture on the voice lane, which has no
//! silence, so its voice span is the whole file. What it does not cover is a
//! voice with pauses in it: the inversion and the merge of close spans are
//! pinned on their own below.

use std::time::{Duration, Instant};

use aria_models::{ClipId, MediaId, Param, Rational, TrackId, TrackKind};
use gpui::AppContext;

use super::AppState;
use super::analysis::ticks_for_seconds;
use super::decline::Decline;
use super::duck::{DuckForm, DuckRefusal, duck_curve, voice_spans};
use super::premiere_tests::{with_clip, with_media};
use crate::app::fixtures::asset;
use crate::types::{EditMode, UiTrackKind};

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

/// **The Duck Under Voice row and the run state one reason for one state.**
///
/// The class closed: a guard that collapsed every refusal of the precheck into
/// one sentence, so with a voice track already flagged and its lane empty, or
/// with the voice clip itself selected, the row still read "Flag a voice track
/// and select an audio clip to duck" while the Audio tab's line under the
/// button and the status line of a run stated the real reason. Each reason in
/// [`DuckRefusal::ALL`] is dressed, the precheck is asserted to give it, and the
/// shipped row's guard is asserted to read it, so a new reason without a
/// dressing fails to compile and a guard that words a reason its own way fails.
///
/// Not covered: the panel line, drawn from the same precheck call.
#[test]
fn the_duck_row_states_the_reason_the_run_would_post() {
    let row = crate::commands::BUILTIN_JOB_COMMANDS
        .iter()
        .find(|entry| entry.id == "pal-duck")
        .expect("Duck Under Voice ships");
    for why in DuckRefusal::ALL {
        let state = refused(why);
        assert_eq!(state.duck_precheck().err(), Some(why), "{why:?} dressed");
        let expected = match why {
            // Every row that acts on the selection states an empty one the same way.
            DuckRefusal::NoSelection => Decline::NoSelection.reason(),
            _ => why.reason(),
        };
        assert_eq!(row.blocked_by(&state), Some(expected), "{why:?}");
    }
}

/// A state the precheck refuses for `why`, built without a decode.
fn refused(why: DuckRefusal) -> AppState {
    match why {
        DuckRefusal::NoSelection => {
            let (mut state, _) = with_clip();
            state.clear_selection();
            state
        }
        // The fixture clip is on a video lane, and no lane is the duck source.
        DuckRefusal::NoSource => {
            let (mut state, clip) = with_clip();
            state.select_clip(clip);
            state
        }
        DuckRefusal::SilentSource => {
            let mut state = ready_duck();
            voice_lane(&mut state).clips.to_mut().clear();
            state
        }
        DuckRefusal::OnlyVoice => {
            let mut state = ready_duck();
            state.selection = vec![ClipId::from_raw(202)];
            state
        }
        DuckRefusal::NoMusic => {
            let mut state = ready_duck();
            state.store_mut().sequence.tracks[0].kind = UiTrackKind::Video;
            state
        }
    }
}

/// The lane [`ready_duck`] flags as the voice.
fn voice_lane(state: &mut AppState) -> &mut crate::types::UiTrack {
    state
        .store_mut()
        .sequence
        .tracks
        .iter_mut()
        .find(|track| track.duck_source)
        .expect("ready_duck flags a voice lane")
}

/// Flagging a lane through the track's own toggle names it as the source, and
/// an empty flagged lane refuses before a decode.
#[test]
fn a_flagged_empty_lane_refuses_before_it_costs_a_decode() {
    let (mut state, clip) = with_clip();
    state.select_clip(clip);
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
    assert_eq!(state.duck_precheck().err(), Some(DuckRefusal::SilentSource));
}

/// A state a duck can run from: the fixture's clip on a music lane, selected,
/// and the same media on a lane flagged as the voice.
pub(crate) fn ready_duck() -> AppState {
    let (mut state, _) = with_clip();
    let media_id = {
        let media = state.store_mut().media.first_mut().expect("fixture media");
        media.has_audio = true;
        media.missing = false;
        media.id
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
    state
}

#[test]
fn a_duck_resolves_voice_media_by_id() {
    let state = ready_duck();
    let media_path = state
        .store
        .media
        .first()
        .expect("fixture media")
        .path
        .clone();
    let request = state.duck_precheck().expect("ready duck");
    assert_eq!(request.music, vec![ClipId::from_raw(201)]);
    assert_eq!(request.voice.len(), 1);
    assert_eq!(request.voice[0].clip, ClipId::from_raw(202));
    assert_eq!(request.voice[0].path.to_string_lossy(), media_path.as_ref());
}

/// The Audio tab's three duck settings cycle and wrap, so a click always
/// changes the value it names.
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

/// **A duck run writes the dip onto the selection as one undo step, and a
/// second run replaces the first rather than ducking the dip.**
///
/// The class closed: everything between the detector and the project. The
/// curve and the inversion are pinned above, and nothing ran the path that
/// measures the voice lane on a job, maps each voice clip's source spans onto
/// the timeline, reads the clip's un-ducked level, and writes the keyframes.
/// This drives `duck_selected` through the job and the window's wake and reads
/// the engine. A voice span left in source ticks, a window measured from tick
/// 0, a duck that takes more than one undo, a re-run that keeps the old
/// keyframes, and a re-run that dips from the dip each change the curve or the
/// history read back.
///
/// Not covered: two voice clips, a voice with pauses, and a selection of
/// several clips.
#[test]
fn a_duck_run_writes_one_undoable_dip_and_a_rerun_replaces_it() {
    let (mut state, filler) = with_media(asset("fixture-sine-48k-2s.wav"));
    state.edit_mode = EditMode::Overwrite;
    let rate = state.playhead.rate;
    let sine = state
        .engine
        .lock()
        .project()
        .clip(filler)
        .and_then(|clip| clip.media())
        .expect("the sine's media");
    let (_, music) = sine_on_a_new_lane(&mut state, sine, ticks_for_seconds(0.5, rate));
    let (voice_lane, voice) = sine_on_a_new_lane(&mut state, sine, ticks_for_seconds(1.0, rate));
    state
        .toggle_track_duck_source(voice_lane)
        .expect("flag the voice lane");
    let ((music_start, music_end), level) = held(&state, music);
    let ((voice_start, voice_end), _) = held(&state, voice);
    // The voice opens inside the music and runs past its end, so the dip lands
    // where the voice starts and holds to the music's last tick.
    assert_eq!(
        (music_start, voice_start),
        (ticks_for_seconds(0.5, rate), ticks_for_seconds(1.0, rate))
    );
    assert!(voice_end > music_end, "the voice ends inside the music");
    assert_eq!(level, Param::Constant(1.0));
    let dip = |form: DuckForm| {
        let attack = ticks_for_seconds(form.attack_seconds, rate);
        vec![
            (voice_start - attack, 1.0),
            (voice_start, form.amount),
            (music_end - 1, form.amount),
        ]
    };
    let ducked = "Ducked 1 clip with 3 keyframes under 1 voice span";

    let cx = gpui::TestAppContext::single();
    let entity = cx.update(|app| app.new(|_| state));
    let first = DuckForm::default();
    assert_eq!(run_duck(&entity, &cx, music, first), ducked);
    let read = |clip| on(&entity, &cx, |state| envelope(state, clip));
    assert_eq!(read(music), dip(first), "the first duck");
    for clip in [voice, filler] {
        let level = on(&entity, &cx, |state| held(state, clip).1);
        assert_eq!(
            level,
            Param::Constant(1.0),
            "{clip:?} is not selected and kept its level"
        );
    }

    let undone = on(&entity, &cx, |state| {
        assert!(state.undo(), "the duck is on the history");
        held(state, music).1
    });
    assert_eq!(
        undone,
        Param::Constant(1.0),
        "one undo takes the whole duck back"
    );
    assert!(
        on(&entity, &cx, |state| state.redo()),
        "the duck is on the redo history"
    );
    assert_eq!(read(music), dip(first), "one redo puts the whole duck back");

    let second = DuckForm {
        amount: 0.5,
        attack_seconds: 0.25,
        release_seconds: 0.5,
    };
    assert_eq!(run_duck(&entity, &cx, music, second), ducked);
    assert_eq!(
        read(music),
        dip(second),
        "a second duck replaces the first, from the un-ducked level"
    );
    assert!(
        on(&entity, &cx, |state| state.undo()),
        "the second duck is on the history"
    );
    assert_eq!(
        read(music),
        dip(first),
        "one undo takes the second duck back"
    );
}

/// `body` against the state `entity` holds.
fn on<R>(
    entity: &gpui::Entity<AppState>,
    cx: &gpui::TestAppContext,
    body: impl FnOnce(&mut AppState) -> R,
) -> R {
    cx.update(|app| entity.update(app, |state, _| body(state)))
}

/// Place `media` at `at` on a new audio lane: the lane and its clip.
fn sine_on_a_new_lane(state: &mut AppState, media: MediaId, at: i64) -> (TrackId, ClipId) {
    let lane = state
        .add_lane_of_kind(TrackKind::Audio, None)
        .expect("a new audio lane");
    state
        .drop_media_on_timeline(media, Some(lane), at)
        .expect("place the sine");
    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.id == lane.raw())
        .and_then(|track| track.clips.first())
        .map(|clip| ClipId::from_raw(clip.id))
        .expect("the sine on its lane");
    (lane, clip)
}

/// The engine's `(start, end)` of `clip` in timeline ticks, and its volume.
fn held(state: &AppState, clip: ClipId) -> ((i64, i64), Param<f32>) {
    let engine = state.engine.lock();
    let held = engine
        .project()
        .clip(clip)
        .expect("the clip is in the project");
    let start = held.timeline.start.value;
    (
        (start, start + held.timeline.duration.value),
        held.volume.clone(),
    )
}

/// `clip`'s volume keyframes at timeline ticks; empty for a constant level.
fn envelope(state: &AppState, clip: ClipId) -> Vec<(i64, f32)> {
    let ((start, _), volume) = held(state, clip);
    volume
        .keyframes()
        .iter()
        .map(|key| (start + key.tick, key.value))
        .collect()
}

/// Run Duck Under Voice on `music` with `form`, and the status line it ends on.
///
/// Moves the clock on so the job's wake is read (see `handoff::CHECK`), then
/// waits for the job to settle so the next run is not deduplicated against it.
/// Fails after 30 s of real time, so a run that never reports is a red test
/// rather than a hang.
fn run_duck(
    entity: &gpui::Entity<AppState>,
    cx: &gpui::TestAppContext,
    music: ClipId,
    form: DuckForm,
) -> String {
    const MEASURING: &str = "Measuring the voice…";
    cx.update(|app| {
        entity.update(app, |state, cx| {
            state.duck = form;
            state.selection = vec![music];
            state.last_error = None;
            state.duck_selected(cx).expect("the duck starts");
            assert_eq!(state.status_message.as_deref(), Some(MEASURING));
        })
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        cx.executor().advance_clock(crate::handoff::CHECK);
        cx.run_until_parked();
        let (status, error) = on(entity, cx, |state| {
            (state.status_message.clone(), state.last_error.clone())
        });
        if let Some(error) = error {
            break format!("error: {error}");
        }
        if status.as_deref() != Some(MEASURING) {
            break status.unwrap_or_default();
        }
        assert!(Instant::now() < deadline, "the duck did not report in 30 s");
        std::thread::sleep(Duration::from_millis(5));
    };
    while on(entity, cx, |state| {
        state.refresh_jobs();
        state.running_job_count() > 0
    }) {
        assert!(
            Instant::now() < deadline,
            "the duck's job did not end in 30 s"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    status
}
