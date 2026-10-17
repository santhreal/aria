//! A `.cube` grade dropped or imported: onto the selected clip when one is
//! selected, else as a LUT filter bar across the sequence on a new filter
//! track, as one undo step. A file that is not a Cube LUT changes nothing.
//!
//! Not caught here: how the LUT renders, which the compositor's suite covers.

use std::path::{Path, PathBuf};

use aria_models::{ClipSource, Generator, Project, Track, TrackKind};

use super::AppState;
use crate::app::hook_tests::mounted_state_with_clip;

/// An identity table of size 2.
const IDENTITY: &str = "TITLE \"Warm\"\nLUT_3D_SIZE 2\n\
                        0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";

/// `text` written as `name` in a fresh directory.
fn lut_file(name: &str, text: &str) -> PathBuf {
    let path = crate::scratch_pool::scratch("lut-import").join(name);
    std::fs::write(&path, text).expect("writes the LUT");
    path
}

fn project(state: &AppState) -> std::sync::Arc<Project> {
    state.host().snapshot()
}

fn named<'p>(project: &'p Project, name: &str) -> Option<&'p Track> {
    project
        .timeline()
        .tracks_ordered()
        .find(|track| track.name == name)
}

/// Each track's name and clip count, bottom to top.
fn layout(project: &Project) -> Vec<(String, usize)> {
    project
        .timeline()
        .tracks_ordered()
        .map(|track| (track.name.clone(), track.clips_ordered().count()))
        .collect()
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("a UTF-8 path")
}

#[test]
fn the_cube_importer_is_listed_for_its_extension_in_any_case() {
    let (state, _, _) = mounted_state_with_clip();
    for name in ["Warm.cube", "Warm.CUBE"] {
        let spec = state
            .ctx
            .importer_for(Path::new(name))
            .unwrap_or_else(|| panic!("{name} has an importer"));
        assert_eq!(
            (spec.id.as_str(), spec.name.as_str()),
            ("aria.effects.cube", "Cube LUT")
        );
    }
}

#[test]
fn a_grade_dropped_with_nothing_selected_filters_the_sequence_as_one_undo_step() {
    let (mut state, clip, _) = mounted_state_with_clip();
    state.clear_selection();
    let cube = lut_file("Warm.cube", IDENTITY);
    let before = project(&state);
    let duration = before.timeline().duration().value;
    assert!(duration > 0, "the fixture clip gives the sequence a length");

    state
        .handle_dropped_paths(vec![cube.clone()])
        .expect("drops");
    assert_eq!(state.last_error, None);

    let after = project(&state);
    let track = named(&after, "Warm").expect("a track named after the file");
    assert_eq!(track.kind, TrackKind::Filter);
    let bars: Vec<_> = track.clips_ordered().collect();
    assert_eq!(bars.len(), 1);
    let bar = bars[0];
    assert!(
        matches!(bar.content, ClipSource::Generated(Generator::Filter)),
        "{:?}",
        bar.content
    );
    assert_eq!(
        (bar.timeline.start.value, bar.timeline.duration.value),
        (0, duration)
    );
    assert_eq!(
        bar.lut.as_ref().map(|lut| lut.path.as_str()),
        Some(path_text(&cube))
    );
    assert_eq!(
        after.clip(clip).expect("the clip").lut,
        None,
        "the unselected clip keeps its look"
    );

    assert!(state.undo(), "the import is one undo step");
    assert_eq!(layout(&project(&state)), layout(&before));
}

#[test]
fn a_grade_dropped_on_a_selected_clip_grades_that_clip_and_adds_no_track() {
    let (mut state, clip, _) = mounted_state_with_clip();
    state.select_clip(clip);
    let cube = lut_file("Warm.cube", IDENTITY);
    let before = layout(&project(&state));

    state
        .handle_dropped_paths(vec![cube.clone()])
        .expect("drops");
    assert_eq!(state.last_error, None);

    let after = project(&state);
    assert_eq!(
        after
            .clip(clip)
            .expect("the clip")
            .lut
            .as_ref()
            .map(|lut| lut.path.as_str()),
        Some(path_text(&cube))
    );
    assert_eq!(layout(&after), before);
}

#[test]
fn an_empty_sequence_gets_a_five_second_filter() {
    let (mut state, _, _) = mounted_state_with_clip();
    state.new_project().expect("a fresh project");
    let cube = lut_file("Show LUT.cube", IDENTITY);

    assert!(state.import_timeline(&cube), "{:?}", state.last_error);

    let after = project(&state);
    let rate = after.timeline().frame_rate;
    let bar = named(&after, "Show LUT")
        .expect("a track named after the file")
        .clips_ordered()
        .next()
        .expect("one bar");
    assert_eq!(
        bar.timeline.duration.value,
        5 * i64::from(rate.num) / i64::from(rate.den)
    );
}

#[test]
fn a_file_that_is_not_a_cube_lut_is_refused_and_changes_nothing() {
    let cases = [
        (
            "Short.cube",
            "LUT_3D_SIZE 2\n0 0 0\n",
            "is not a Cube LUT: expected 8 data rows, found 1",
        ),
        (
            "Flat.cube",
            "0 0 0\n",
            "is not a Cube LUT: missing LUT_3D_SIZE (1D LUTs are not supported)",
        ),
    ];
    for (name, text, reason) in cases {
        let (mut state, _, _) = mounted_state_with_clip();
        state.clear_selection();
        let cube = lut_file(name, text);
        let before = layout(&project(&state));

        state
            .handle_dropped_paths(vec![cube.clone()])
            .expect("drops");

        assert_eq!(
            state.last_error,
            Some(format!(
                "aria.effects.cube could not import {path}: {path} {reason}",
                path = cube.display()
            )),
            "{name}"
        );
        assert_eq!(layout(&project(&state)), before, "{name}");
    }
}

#[test]
fn a_missing_file_is_refused_with_the_read_error() {
    let (mut state, _, _) = mounted_state_with_clip();
    let cube = crate::scratch_pool::scratch("lut-import").join("Gone.cube");
    let before = layout(&project(&state));

    assert!(!state.import_timeline(&cube));

    let error = state.last_error.clone().expect("an error");
    let prefix = format!(
        "aria.effects.cube could not import {path}: {path} could not be read: ",
        path = cube.display()
    );
    assert!(error.starts_with(&prefix), "{error}");
    assert_eq!(layout(&project(&state)), before);
}
