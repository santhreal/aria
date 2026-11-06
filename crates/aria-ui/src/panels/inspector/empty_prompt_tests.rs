//! Effect Controls with nothing to show states what fills it.
//!
//! Closes the class "an empty panel or block promises what does not exist, or
//! reads as a block that failed to draw": the panel with no clip read "Select
//! a clip, or click a gap to edit the sequence.", but a gap press selects the
//! gap for Delete and leaves the panel as it was, and nothing in the panel
//! edits the sequence; the Effects tab over a clip with no effects drew its
//! `Effects` heading over nothing, straight into `Presets`. It now reads None
//! there, as Keyframes and Masks read over nothing. Each statement is read
//! where the panel draws it, and goes once there is something to show. The
//! words are written here rather than read from the panel, so a reworded
//! statement turns this red until the copy is checked against the panel's
//! behavior. Not covered: the Sequence readouts under the prompt.

use aria_models::{ClipId, TrackId};
use gpui::{TestAppContext, VisualTestContext};

use crate::app::AppState;

use super::strip_tests::drawn_with;

const PROMPT: &str = "Select a clip to edit its properties.";
/// The note at the top of the Effects tab over an empty chain, and the
/// Presets heading after it, as [`body_words`] joins them.
const NO_EFFECTS: &str = "None Presets";

/// The words the Inspector's body draws, in order, one space apart.
fn body_words(visual: &mut VisualTestContext) -> String {
    let body = visual
        .debug_bounds("inspector-body")
        .expect("the Inspector draws its body");
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    runs.iter()
        .filter(|run| body.contains(&run.bounds.center()))
        .flat_map(|run| run.text.split_whitespace().map(str::to_owned))
        .collect::<Vec<_>>()
        .join(" ")
}

fn change(
    visual: &mut VisualTestContext,
    state: &gpui::Entity<AppState>,
    edit: impl FnOnce(&mut AppState),
) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            edit(state);
            cx.notify();
        })
    });
    visual.run_until_parked();
}

#[gpui::test]
async fn the_empty_panel_states_what_puts_a_clip_in_it(cx: &mut TestAppContext) {
    let (state, visual) = drawn_with(
        cx,
        "inspector-empty-prompt",
        "texture.png",
        "transform",
        |_| {},
    );
    // A second copy placed at twice the first's end leaves a gap as long as
    // the first between them.
    let (lane, gap_at) = visual.update(|_, app| {
        state.update(app, |state, _| {
            let lane = TrackId::from_raw(state.store.sequence.tracks[0].id);
            let end = state.store.sequence.tracks[0].clips[0].timeline_end_ticks;
            let still = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/texture.png");
            let media = state.import_media(still).expect("import the still");
            state
                .drop_media_on_timeline(media, Some(lane), end * 2)
                .expect("place the second still");
            (lane, end + end / 2)
        })
    });
    visual.run_until_parked();

    let clip = visual.read(|app| state.read(app).store.sequence.tracks[0].clips[0].id);
    change(visual, &state, |state| {
        state.select_clip(ClipId::from_raw(clip))
    });
    let drawn = body_words(visual);
    assert!(
        !drawn.contains(PROMPT),
        "a selected clip still draws the prompt: {drawn}"
    );

    change(visual, &state, |state| state.clear_selection());
    let drawn = body_words(visual);
    assert!(
        drawn.contains(PROMPT),
        "with nothing selected the panel draws {drawn}"
    );

    change(visual, &state, |state| {
        assert!(
            state.select_gap_at(lane, gap_at).is_some(),
            "no gap between the stills"
        );
    });
    let drawn = body_words(visual);
    assert!(
        drawn.contains(PROMPT),
        "with a gap selected the panel draws {drawn}"
    );
}

#[gpui::test]
async fn an_empty_effect_chain_reads_none_between_its_headings(cx: &mut TestAppContext) {
    let (state, visual) = drawn_with(
        cx,
        "inspector-empty-chain",
        "texture.png",
        "effects",
        |_| {},
    );
    let drawn = body_words(visual);
    assert!(
        drawn.contains(NO_EFFECTS),
        "the Effects tab over a clip with no effects draws {drawn}"
    );

    change(visual, &state, |state| {
        state
            .add_effect_to_selected("gaussian_blur")
            .expect("add a blur");
    });
    let drawn = body_words(visual);
    assert!(
        !drawn.starts_with("None "),
        "a chain with a blur still reads None: {drawn}"
    );
}
