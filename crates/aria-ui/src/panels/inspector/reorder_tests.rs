//! The controls that move an entry of a chain one place earlier or later
//! stand in one column down the chain, and each moves the entry it is drawn on.
//!
//! The class closed here: a row's move control drawn in another column than
//! the rows above and below draw it. The first entry cannot move earlier and
//! the last cannot move later, and while nothing stood in for the missing
//! control, the last row's move-earlier stood where the other rows draw
//! move-later, because the controls end their row. Both chains the inspector
//! draws are read, the Effects tab's effects and the Audio tab's inserts, with
//! three entries each, so a first, a middle and a last row are all drawn. Each
//! control is pressed where it is drawn and the chain's order is read back.
//!
//! Not covered: a lane's and the master's insert chains on the Audio Track
//! Mixer, which draw the insert rows read here.

use std::path::PathBuf;

use aria_models::{AudioInsertTarget, ClipId};
use gpui::{Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext};

use crate::app::AppState;
use crate::test_support::{drawn_shell, panel_bounds};

/// How far apart two lefts may be and still stand in one column.
const EPS: f32 = 0.51;

/// A chain the inspector draws with move controls.
struct Chain {
    /// The shell's test tag.
    tag: &'static str,
    /// The inspector tab the chain is on.
    tab: &'static str,
    /// The fixture whose clip holds the chain.
    media: &'static str,
    /// The stem of the move controls' debug selectors, which `-up-{row}` or
    /// `-down-{row}` ends.
    stem: &'static str,
    /// The entries added, in order.
    entries: [&'static str; 3],
    /// Append an entry to the selected clip's chain.
    add: fn(&mut AppState, &str),
    /// The chain's entries, in order.
    order: fn(&AppState) -> Vec<String>,
}

const EFFECTS: Chain = Chain {
    tag: "reorder-effects",
    tab: "effects",
    media: "texture.png",
    stem: "inspector-fx",
    entries: ["gaussian_blur", "color_overlay", "duotone"],
    add: add_effect,
    order: effects,
};

const INSERTS: Chain = Chain {
    tag: "reorder-inserts",
    tab: "audio",
    media: "fixture-sine-48k-2s.wav",
    stem: "insert",
    entries: ["swap_channels", "fill_left", "mono"],
    add: add_insert,
    order: inserts,
};

fn add_effect(state: &mut AppState, id: &str) {
    state
        .add_effect_to_selected(id)
        .unwrap_or_else(|err| panic!("adding {id}: {err}"));
}

fn effects(state: &AppState) -> Vec<String> {
    state
        .selected_ui_clip_ref()
        .expect("a selected clip")
        .effects
        .iter()
        .map(|effect| effect.id.to_string())
        .collect()
}

/// The selected clip's insert chain.
fn clip_chain(state: &AppState) -> AudioInsertTarget {
    AudioInsertTarget::Clip(state.selected_clip().expect("a selected clip"))
}

fn add_insert(state: &mut AppState, id: &str) {
    let target = clip_chain(state);
    state
        .add_audio_insert(target, id.to_string())
        .unwrap_or_else(|err| panic!("adding {id}: {err}"));
}

fn inserts(state: &AppState) -> Vec<String> {
    state
        .audio_insert_rows(clip_chain(state))
        .iter()
        .map(|insert| insert.effect_id.to_string())
        .collect()
}

/// Where row `row` of `chain` draws its move control `way`, if it draws one.
fn control(
    visual: &mut VisualTestContext,
    chain: &Chain,
    way: &str,
    row: usize,
) -> Option<Bounds<Pixels>> {
    let selector: &'static str = format!("{}-{way}-{row}", chain.stem).leak();
    panel_bounds(visual, selector)
}

/// [`control`], which the row must draw.
fn drawn(visual: &mut VisualTestContext, chain: &Chain, way: &str, row: usize) -> Bounds<Pixels> {
    control(visual, chain, way, row)
        .unwrap_or_else(|| panic!("{}: row {row} draws no move control {way}", chain.stem))
}

fn moves_stand_in_one_column(cx: &mut TestAppContext, chain: &Chain) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, chain.tag);
    let media = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(chain.media);
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(media).expect("import the fixture");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the clip");
            let clip = state
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|track| track.clips.iter())
                .map(|clip| ClipId::from_raw(clip.id))
                .next()
                .expect("a clip");
            state.select_clip(clip);
            for entry in chain.entries {
                (chain.add)(state, entry);
            }
            state.inspector_tab = chain.tab.to_string();
            // The inspector fills the editor, so the Audio tab's inserts, under
            // its level, fade and pitch rows, are drawn on screen.
            let frame = state.dock_tree.open(crate::dock::PANEL_INSPECTOR);
            assert!(
                state.dock_tree.toggle_maximize(frame),
                "the inspector's frame fills the editor"
            );
            cx.notify();
        })
    });
    visual.run_until_parked();
    let order = |visual: &mut VisualTestContext| visual.read(|app| (chain.order)(state.read(app)));
    let [first, middle, last] = chain.entries;
    assert_eq!(
        order(visual),
        [first, middle, last],
        "{}: the chain as added",
        chain.stem
    );

    assert!(
        control(visual, chain, "up", 0).is_none(),
        "{}: the first row draws a move-earlier",
        chain.stem
    );
    assert!(
        control(visual, chain, "down", 2).is_none(),
        "{}: the last row draws a move-later",
        chain.stem
    );
    let left = |bounds: Bounds<Pixels>| f32::from(bounds.left());
    let (up, last_up) = (drawn(visual, chain, "up", 1), drawn(visual, chain, "up", 2));
    assert!(
        (left(last_up) - left(up)).abs() <= EPS,
        "{}: the last row's move-earlier at {last_up:?} is out of the middle row's \
         column at {up:?}",
        chain.stem
    );
    let (first_down, down) = (
        drawn(visual, chain, "down", 0),
        drawn(visual, chain, "down", 1),
    );
    assert!(
        (left(first_down) - left(down)).abs() <= EPS,
        "{}: the first row's move-later at {first_down:?} is out of the middle row's \
         column at {down:?}",
        chain.stem
    );

    visual.simulate_click(last_up.center(), Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        order(visual),
        [first, last, middle],
        "{}: the last row's move-earlier",
        chain.stem
    );
    let first_down = drawn(visual, chain, "down", 0);
    visual.simulate_click(first_down.center(), Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        order(visual),
        [last, first, middle],
        "{}: the first row's move-later",
        chain.stem
    );
}

#[gpui::test]
async fn an_effects_move_controls_stand_in_one_column(cx: &mut TestAppContext) {
    moves_stand_in_one_column(cx, &EFFECTS);
}

#[gpui::test]
async fn an_inserts_move_controls_stand_in_one_column(cx: &mut TestAppContext) {
    moves_stand_in_one_column(cx, &INSERTS);
}
