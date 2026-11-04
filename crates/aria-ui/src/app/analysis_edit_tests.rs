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
