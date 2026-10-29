//! The mixer draws a strip for a picture lane only while a clip on it holds its
//! sound in the picture.
//!
//! WHY: a drop places a video's sound on a linked audio lane, so the picture
//! lane it lands on plays nothing, and the new project's Main lane plays
//! nothing either; the mixer drew a fader, a pan and a meter for both. The
//! class: a strip for a picture lane the mix hears nothing from.
//!
//! Checked in the Audio workspace, through each change a picture lane's sound
//! takes: an empty picture lane draws no strip, so a project holding only one
//! draws the empty mixer line; a picture lane whose clip holds its sound draws
//! a strip; extracting that sound to an audio lane removes the strip; undoing
//! the extraction draws it again. An audio lane and a bus draw a strip while
//! they hold nothing, and the master draws one whenever any strip is drawn.
//!
//! Not covered: a hidden lane's mute, solo and automation, which stay in the
//! model and return with its strip.

use aria_models::TrackKind;
use gpui::{TestAppContext, VisualTestContext};

use crate::app::AppState;
use crate::app::fixtures::video_in_picture;
use crate::interned::intern;
use crate::test_support::drawn_shell;
use crate::types::UiTrackKind;

/// The raw ids of the lanes the mixer drew a strip for, the master as `0`,
/// sorted.
fn drawn(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) -> Vec<u64> {
    for _ in 0..2 {
        visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
        visual.run_until_parked();
    }
    let lanes: Vec<u64> = visual.read(|app| {
        let state = state.read(app);
        state
            .store
            .sequence
            .tracks
            .iter()
            .map(|lane| lane.id)
            .collect()
    });
    let mut drawn: Vec<u64> = lanes
        .into_iter()
        .chain([0])
        .filter(|id| {
            visual
                .debug_bounds(intern(&format!("mixer-head-{id}")))
                .is_some()
        })
        .collect();
    drawn.sort_unstable();
    drawn
}

/// The raw ids of the project's audio lanes.
fn audio_lanes(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) -> Vec<u64> {
    visual.read(|app| {
        let state = state.read(app);
        state
            .store
            .sequence
            .tracks
            .iter()
            .filter(|lane| lane.kind == UiTrackKind::Audio)
            .map(|lane| lane.id)
            .collect()
    })
}

fn sorted(mut ids: Vec<u64>) -> Vec<u64> {
    ids.sort_unstable();
    ids
}

#[gpui::test]
fn a_picture_lane_draws_a_strip_only_while_a_clip_on_it_holds_its_sound(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "mixer-sound-lanes");
    visual.update(|_, app| app.set_reduce_motion(true));
    let main = visual.update(|_, app| {
        state.update(app, |state, cx| {
            assert!(state.apply_workspace("win-workspace-audio", cx));
            state.store.sequence.tracks[0].id
        })
    });
    assert_eq!(
        drawn(&state, visual),
        Vec::<u64>::new(),
        "a project holding only the empty Main lane drew mixer strips"
    );
    assert!(
        visual.debug_bounds("empty-state-mixer").is_some(),
        "a project holding only the empty Main lane drew no empty mixer line"
    );

    let (picture, clip, audio, bus) = visual.update(|_, app| {
        state.update(app, |state, _| {
            let picture = state
                .add_lane_of_kind(TrackKind::Video, None)
                .unwrap_or_else(|err| panic!("a second picture lane: {err:?}"));
            let clip = video_in_picture(state, picture);
            let audio = state
                .add_lane_of_kind(TrackKind::Audio, None)
                .unwrap_or_else(|err| panic!("an audio lane: {err:?}"));
            state
                .add_submix_lane()
                .unwrap_or_else(|err| panic!("a submix bus: {err:?}"));
            let bus = state
                .mixer_strips()
                .unwrap_or_else(|refusal| panic!("the mix reads: {refusal}"))
                .iter()
                .find(|strip| strip.is_bus)
                .map(|strip| strip.track)
                .expect("the bus is in the mix");
            (picture.raw(), clip, audio.raw(), bus)
        })
    });
    assert_eq!(
        drawn(&state, visual),
        sorted(vec![picture, audio, bus, 0]),
        "with the video's sound in its picture clip, the mixer draws the picture \
         lane {picture}, the empty audio lane {audio}, the bus {bus} and the master, \
         and no strip for the empty Main lane {main}"
    );

    visual.update(|_, app| {
        state.update(app, |state, _| {
            state.select_clip(clip);
            state
                .extract_selected_audio()
                .unwrap_or_else(|err| panic!("extract the clip's sound: {err:?}"));
        })
    });
    let lanes = audio_lanes(&state, visual);
    assert!(
        lanes.contains(&audio),
        "the extraction removed the audio lane"
    );
    let mut after = lanes;
    after.extend([bus, 0]);
    assert_eq!(
        drawn(&state, visual),
        sorted(after),
        "with the video's sound extracted to an audio lane, the picture lane \
         {picture} still drew a strip, or an audio lane drew none"
    );

    let undone = visual.update(|_, app| state.update(app, |state, _| state.undo()));
    assert!(undone, "the extraction is one undo step");
    let mut restored = audio_lanes(&state, visual);
    restored.extend([picture, bus, 0]);
    assert_eq!(
        drawn(&state, visual),
        sorted(restored),
        "undoing the extraction put the sound back in the picture clip, and the \
         picture lane {picture} drew no strip"
    );
}
