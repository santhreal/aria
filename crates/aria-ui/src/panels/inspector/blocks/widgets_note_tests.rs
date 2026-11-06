//! A note Effect Controls draws faint states an absence.
//!
//! Closes the class "a fact of the selection drawn in the ink of an absence":
//! Speed's "Flat 100.0%", "Ramp on a … base" and "Length …", and Audio's
//! "Listening to {track}", read `text_faint` beside the "None" and "Add a mask"
//! notes, at about 3:1 on the dark ground. An absence holds nothing of the
//! selection, so the faint notes of every tab are drawn for two selections that
//! differ only in their values, a speed of 100% and 50% and the duck source on
//! one track and then another, flat and ramped, and must read alike. A note
//! added later that prints a value in faint ink differs between the two and
//! fails, whatever tab draws it: the tabs are read from the tab host at run
//! time. Each fact named above must be drawn at least once, so the census does
//! not pass by drawing none of them.
//!
//! Not covered: a fact that reads the same for both selections, such as a
//! fixed count, and text drawn outside [`super::label_note`], such as the
//! Markers panel's notes.

use aria_models::{SPEED_CURVE_SCALE, TrackId, TrackKind};
use gpui::{Entity, SharedString, TestAppContext, VisualTestContext};

use super::{drawn_notes, record_lines};
use crate::app::{AppState, DEFAULT_INSPECTOR_TAB};
use crate::panels::inspector::strip_tests::drawn_with;
use crate::panels::inspector::tab_host::host_of;
use crate::theme::Theme;
use crate::types::UiTrackKind;

/// A movie with picture and sound, so the Speed and Audio tabs are offered.
const MOVIE: &str = "fixture-640x360-24fps-3s-av.mp4";

/// The two audio tracks the duck source moves between.
const VOICES: [&str; 2] = ["Voice", "Dialog"];

/// The start of each note that states a fact of the selection.
const FACTS: [&str; 4] = ["Flat ", "Ramp on a ", "Length ", "Listening to "];

/// Retime the selected clip to `percent` through Speed/Duration, ramp it when
/// `ramp`, and make the track named `voice` the only duck source.
fn dress(state: &mut AppState, percent: f32, voice: &str, ramp: bool) {
    state.open_speed_duration();
    state
        .set_speed_duration_percent(percent)
        .expect("a speed in range");
    state.apply_speed_duration();
    if ramp {
        state
            .set_selected_ramp_point(SPEED_CURVE_SCALE / 2, 1.5)
            .expect("ramp the clip");
    }
    let audio: Vec<(TrackId, bool)> = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|track| track.kind == UiTrackKind::Audio)
        .map(|track| (TrackId::from_raw(track.id), &*track.name == voice))
        .collect();
    for (track, wanted) in audio {
        if state.track_is_duck_source(track) != wanted {
            state
                .toggle_track_duck_source(track)
                .expect("flag the duck source");
        }
    }
}

/// Every note drawn on every tab, and the faint ones among them, once each.
fn notes(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
) -> (Vec<SharedString>, Vec<SharedString>) {
    let faint = visual.read(|app| Theme::of(app).text_faint);
    let (mut all, mut faint_only) = (Vec::new(), Vec::new());
    for tab in host_of().tabs() {
        record_lines();
        visual.update(|window, app| {
            state.update(app, |state, _| state.inspector_tab = tab.id.to_string());
            window.refresh();
        });
        visual.run_until_parked();
        for (text, ink) in drawn_notes() {
            if ink == faint {
                faint_only.push(text.clone());
            }
            all.push(text);
        }
    }
    for list in [&mut all, &mut faint_only] {
        list.sort();
        list.dedup();
    }
    (all, faint_only)
}

#[gpui::test]
async fn a_faint_note_reads_the_same_whatever_the_selection_holds(cx: &mut TestAppContext) {
    let (state, visual) = drawn_with(cx, "note-ink", MOVIE, DEFAULT_INSPECTOR_TAB, |state| {
        state
            .unlink_selected()
            .expect("unlink the sound from the take");
        for voice in VOICES {
            state
                .add_track(TrackKind::Audio, voice)
                .expect("an audio track");
        }
    });
    let mut drawn = Vec::new();
    for ramp in [false, true] {
        let mut faint = Vec::new();
        for (percent, voice) in [(100.0, VOICES[0]), (50.0, VOICES[1])] {
            visual
                .update(|_, app| state.update(app, |state, _| dress(state, percent, voice, ramp)));
            let (all, faint_only) = notes(&state, visual);
            drawn.extend(all);
            faint.push(((percent, voice), faint_only));
        }
        let [(first, a), (second, b)] = <[_; 2]>::try_from(faint).expect("two selections");
        assert_eq!(
            a,
            b,
            "the faint notes differ between {first:?} and {second:?}{}, so one states a value of \
             the selection in the ink of an absence",
            if ramp { ", ramped" } else { "" }
        );
    }
    for fact in FACTS {
        assert!(
            drawn.iter().any(|text| text.starts_with(fact)),
            "no note starting {fact:?} is drawn among {drawn:?}, so its ink goes untested"
        );
    }
}
