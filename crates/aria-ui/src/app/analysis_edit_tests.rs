//! The edits a detector's moments drive: Split at Shots and Remove Silence.
//!
//! # Why
//!
//! A detector reads the selected clip's media, so its edits belong to that
//! clip and to the selected clips linked to it. Both edits looked the clip up
//! again at each tick across every lane and took the first clip there: a
//! title or an adjustment above the picture was cut, or a piece of it
//! ripple-deleted, while the picture ran on whole. Each cut was also its own
//! undo step, and a linked sound was left uncut. These tests put a clip above
//! and a clip below a linked picture and sound, all four over the same frames,
//! and read every lane after each edit and after one undo.
//!
//! Not caught here: a linked sound shorter than its picture, whose silent
//! piece at the picture's edge is left to the ripple of its sync lock.

use aria_models::{
    ClipId, MediaId, MediaSource, Project, Rational, RationalTime, TimeRange, TrackId, TrackKind,
};

use super::AppState;
use super::analysis::AnalysisKind;
use super::fixtures::store_moments as store;
use super::premiere_tests::with_clip;

/// Every clip's length: ten seconds at 24 fps.
const LENGTH: i64 = 240;

/// A real file, so the moments index has a content key to file moments under.
fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

fn place(project: &mut Project, media: MediaId, kind: TrackKind, name: &str) -> ClipId {
    let rate = project.timeline().frame_rate;
    let track = project.add_track(kind, name);
    project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(0, LENGTH, rate),
            RationalTime::new(0, rate),
        )
        .expect("an empty lane takes the clip")
}

/// Four lanes over the same frames `[0, 240)`: Under, a picture and its
/// linked sound, and Over, the pair selected through linked selection. The
/// lane order puts a clip that is not under analysis before the picture
/// whichever way the lanes stack.
fn scene() -> (AppState, ClipId) {
    let (mut state, _) = with_clip();
    let rate = Rational::FPS_24;
    let mut project = Project::new("Moments", rate);
    let media = project.add_media(MediaSource::new(fixture(), 64, 64, rate, LENGTH, true));
    place(&mut project, media, TrackKind::Video, "Under");
    let picture = place(&mut project, media, TrackKind::Video, "Picture");
    place(&mut project, media, TrackKind::Video, "Over");
    let sound = place(&mut project, media, TrackKind::Audio, "Sound");
    project
        .link_clips(&[picture, sound])
        .expect("link the pair");
    state.engine.lock().reset_project(project);
    state.sync_from_engine();
    state.linked_selection = true;
    state.select_clip_linked(picture, true);
    assert_eq!(state.selection.len(), 2, "the pair is selected");
    (state, picture)
}

fn lane(state: &AppState, name: &str) -> TrackId {
    state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.name.as_ref() == name)
        .map(|track| TrackId::from_raw(track.id))
        .expect("the scene names its lanes")
}

/// Every clip on `name` as its span and link, in timeline order.
fn clips(state: &AppState, name: &str) -> Vec<((i64, i64), Option<u64>)> {
    let id = lane(state, name).raw();
    let mut clips: Vec<_> = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|track| track.id == id)
        .flat_map(|track| track.clips.iter())
        .map(|clip| {
            (
                (clip.timeline_start_ticks, clip.timeline_end_ticks),
                clip.linked_to,
            )
        })
        .collect();
    clips.sort_unstable();
    clips
}

fn spans(state: &AppState, name: &str) -> Vec<(i64, i64)> {
    clips(state, name)
        .into_iter()
        .map(|(span, _)| span)
        .collect()
}

fn links(state: &AppState, name: &str) -> Vec<Option<u64>> {
    clips(state, name)
        .into_iter()
        .map(|(_, link)| link)
        .collect()
}

/// The two lanes not under analysis are each one whole clip.
fn assert_untouched(state: &AppState, what: &str) {
    for name in ["Over", "Under"] {
        assert_eq!(spans(state, name), [(0, LENGTH)], "{name} after {what}");
    }
}

/// Each piece of the picture is linked to the piece of sound beside it, and
/// no two pieces share a link.
fn assert_paired(state: &AppState, pieces: usize) {
    let picture = links(state, "Picture");
    assert_eq!(links(state, "Sound"), picture, "picture and sound pieces");
    let mut distinct: Vec<u64> = picture.iter().flatten().copied().collect();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), pieces, "one link per pair: {picture:?}");
}

#[test]
fn shot_cuts_split_the_selected_pair_alone_and_undo_in_one_step() {
    let (mut state, _) = scene();
    // Two boundaries inside the clip and one past its end.
    store(
        &state,
        AnalysisKind::Shots,
        &[(2.5, 2.5, 1.0), (6.0, 6.0, 1.0), (20.0, 20.0, 1.0)],
    );
    state.refresh_moments().expect("read the moments");
    let link = links(&state, "Picture");

    state.split_at_shots().expect("split at shots");

    let cut = [(0, 60), (60, 144), (144, LENGTH)];
    assert_eq!(spans(&state, "Picture"), cut);
    assert_eq!(spans(&state, "Sound"), cut);
    assert_untouched(&state, "the split");
    assert_paired(&state, 3);
    assert_eq!(
        links(&state, "Picture")[0],
        link[0],
        "the heads keep their link"
    );
    assert_eq!(
        state.status_message.as_deref(),
        Some("Split at 2 shot boundaries")
    );

    assert!(state.undo(), "the split is one undo step");
    assert_eq!(clips(&state, "Picture"), [((0, LENGTH), link[0])]);
    assert_eq!(clips(&state, "Sound"), [((0, LENGTH), link[0])]);
    assert_untouched(&state, "the undo");
}

#[test]
fn a_shot_cut_of_a_picture_selected_alone_leaves_its_sound_whole() {
    let (mut state, picture) = scene();
    state.select_clip_linked(picture, false);
    store(&state, AnalysisKind::Shots, &[(2.5, 2.5, 1.0)]);
    state.refresh_moments().expect("read the moments");
    let link = links(&state, "Picture")[0];

    state.split_at_shots().expect("split at shots");

    assert_eq!(
        clips(&state, "Picture"),
        [((0, 60), link), ((60, LENGTH), None)],
        "the tail of a cut its sound did not share stays unlinked"
    );
    assert_eq!(clips(&state, "Sound"), [((0, LENGTH), link)]);
    assert_untouched(&state, "the split");
}

#[test]
fn silence_removal_takes_each_span_from_the_selected_pair_and_undoes_in_one_step() {
    // The sound in sync lock, so the picture's ripple removes its piece, and
    // out of it, so the sound's own piece closes its lane alone.
    for sound_locked in [true, false] {
        let (mut state, _) = scene();
        // Over and Under out of sync lock: only an edit that names them moves
        // them.
        for name in ["Over", "Under"] {
            let track = lane(&state, name);
            state
                .toggle_track_sync_lock(track)
                .expect("unlock the lane");
        }
        if !sound_locked {
            let track = lane(&state, "Sound");
            state
                .toggle_track_sync_lock(track)
                .expect("unlock the sound");
        }
        // [48, 72) and [144, 168).
        store(
            &state,
            AnalysisKind::Silence,
            &[(2.0, 3.0, 1.0), (6.0, 7.0, 1.0)],
        );
        state.refresh_moments().expect("read the moments");

        state.ripple_delete_silence().expect("remove the silence");

        let closed = [(0, 48), (48, 120), (120, 192)];
        let what = format!("silence removal, sound in sync lock {sound_locked}");
        assert_eq!(spans(&state, "Picture"), closed, "{what}");
        assert_eq!(spans(&state, "Sound"), closed, "{what}");
        assert_untouched(&state, &what);
        assert_paired(&state, 3);
        assert_eq!(
            state.status_message.as_deref(),
            Some("Removed 2 silent spans")
        );

        assert!(state.undo(), "the removal is one undo step");
        assert_eq!(spans(&state, "Picture"), [(0, LENGTH)], "{what}, undone");
        assert_eq!(spans(&state, "Sound"), [(0, LENGTH)], "{what}, undone");
        assert_untouched(&state, &what);
    }
}

/// One clip of `media_rate` media on a 24 fps sequence: the source window
/// `[start, start + length)` in media ticks, at `speed`, reversed or not, from
/// tick 0, selected.
fn retimed(media_rate: Rational, start: i64, length: i64, speed: i32, reversed: bool) -> AppState {
    let (mut state, _) = with_clip();
    let mut project = Project::new("Moments", Rational::FPS_24);
    let media = project.add_media(MediaSource::new(
        fixture(),
        64,
        64,
        media_rate,
        start + length + i64::from(media_rate.num),
        true,
    ));
    let track = project.add_track(TrackKind::Video, "Picture");
    let clip = project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(start, length, media_rate),
            RationalTime::new(0, Rational::FPS_24),
        )
        .expect("an empty lane takes the clip");
    project
        .set_clip_speed(clip, Rational::new(speed, 1), reversed)
        .expect("retime the clip");
    state.engine.lock().reset_project(project);
    state.sync_from_engine();
    state.select_clip(clip);
    state
}

/// WHY: a detector's moments were placed by converting their seconds at the
/// sequence rate and adding the clip's timeline start minus its source start,
/// which counts at the media's own rate, and speed and direction were never
/// read. A trimmed clip of a sound file, which counts a thousand ticks a
/// second, put every moment seconds before the clip, so Split at Shots and
/// Remove Silence found nothing inside it; a video at another frame rate, a
/// sped-up clip and a reversed clip were cut frames away from the detected
/// point or not at all. Each case detects a shot at 3.5 s of a trimmed or
/// retimed clip's media, a silent span, and a shot past the source window,
/// then reads where the rows, a seek, the split and the removal land. Not
/// covered: a speed ramp, which the clip mapping approximates.
#[test]
fn moments_cut_where_the_clip_plays_them_at_any_source_rate_trim_and_speed() {
    // (case, media rate, source start, length, speed, reversed, shot and
    // silence in media seconds, the shot's tick, the silence's span, the
    // clip's end).
    type Case = (
        &'static str,
        Rational,
        i64,
        i64,
        i32,
        bool,
        [f64; 3],
        i64,
        (i64, i64),
        i64,
    );
    let cases: [Case; 4] = [
        (
            "a 30 fps video trimmed a second in",
            Rational::new(30, 1),
            30,
            300,
            1,
            false,
            [3.5, 5.0, 6.0],
            60,
            (96, 120),
            LENGTH,
        ),
        (
            "a sound file trimmed half a second in",
            Rational::new(1000, 1),
            500,
            10_000,
            1,
            false,
            [3.0, 5.5, 6.5],
            60,
            (120, 144),
            LENGTH,
        ),
        (
            "a clip at twice speed",
            Rational::FPS_24,
            0,
            LENGTH,
            2,
            false,
            [5.0, 6.0, 7.0],
            60,
            (72, 84),
            LENGTH / 2,
        ),
        (
            "a reversed clip",
            Rational::FPS_24,
            0,
            LENGTH,
            1,
            true,
            [2.5, 2.0, 3.0],
            180,
            (168, 192),
            LENGTH,
        ),
    ];
    for (case, rate, start, length, speed, reversed, at, shot, silence, end) in cases {
        let [cut, quiet, loud] = at;
        let scene = || {
            let mut state = retimed(rate, start, length, speed, reversed);
            store(
                &state,
                AnalysisKind::Shots,
                &[(cut, cut, 1.0), (20.0, 20.0, 1.0)],
            );
            store(&state, AnalysisKind::Silence, &[(quiet, loud, 1.0)]);
            state.refresh_moments().expect("read the moments");
            state
        };

        let mut state = scene();
        let mut read: Vec<Option<(i64, i64)>> =
            state.analysis.moments.iter().map(|row| row.at).collect();
        read.sort_unstable();
        let mut expected = vec![Some((shot, shot)), None, Some(silence)];
        expected.sort_unstable();
        assert_eq!(read, expected, "{case}: the rows");
        state.set_playhead(0);
        state.seek_next_moment();
        assert_eq!(
            state.store.playhead_ticks,
            shot.min(silence.0),
            "{case}: Next Detected Moment"
        );
        state.split_at_shots().expect("split at shots");
        assert_eq!(
            spans(&state, "Picture"),
            [(0, shot), (shot, end)],
            "{case}: Split at Shots"
        );

        let mut state = scene();
        state.ripple_delete_silence().expect("remove the silence");
        let (from, to) = silence;
        assert_eq!(
            spans(&state, "Picture"),
            [(0, from), (from, end - (to - from))],
            "{case}: Remove Silence"
        );
    }
}

/// WHY: a detected beat grid was handed to the plugin analyzer path as
/// sequence samples counted from the clip's start at the source's own pace,
/// and that path mapped them back through the clip's speed and direction: a
/// beat on a clip at twice speed landed past its end and was dropped, and a
/// beat on a reversed clip was stored at the mirrored source point. The grid
/// is now the detected seconds in source ticks. Each case stores a beat at
/// the media second of the shot in the moments cases and one past the source
/// window, and reads where the ruler draws it. Not covered: a speed ramp.
#[test]
fn a_beat_grid_lands_where_the_clip_plays_it_at_any_source_rate_trim_and_speed() {
    let cases = [
        (
            "a 30 fps video trimmed a second in",
            Rational::new(30, 1),
            30,
            300,
            1,
            false,
            3.5,
            60,
        ),
        (
            "a sound file trimmed half a second in",
            Rational::new(1000, 1),
            500,
            10_000,
            1,
            false,
            3.0,
            60,
        ),
        (
            "a clip at twice speed",
            Rational::FPS_24,
            0,
            LENGTH,
            2,
            false,
            5.0,
            60,
        ),
        (
            "a reversed clip",
            Rational::FPS_24,
            0,
            LENGTH,
            1,
            true,
            2.5,
            180,
        ),
    ];
    for (case, rate, start, length, speed, reversed, at, tick) in cases {
        let mut state = retimed(rate, start, length, speed, reversed);
        let clip = state.selected_clip().expect("the clip is selected");
        state.store_beats(clip, &[at, 20.0]);
        let drawn: Vec<i64> = state
            .selected_ui_clip_ref()
            .expect("the clip")
            .beat_ticks
            .iter()
            .copied()
            .collect();
        assert_eq!(drawn, [tick], "{case}");
        assert_eq!(
            state.status_message.as_deref(),
            Some("Stored 1 beat on the clip"),
            "{case}"
        );
    }
}
