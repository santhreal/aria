//! The fx toggle on an effect header.
//!
//! Closes the class "the header's fx toggle switches another effect, switches
//! one way only, or leaves the header showing a state the model does not hold":
//! each press on the drawn toggle of each effect in a two-effect chain flips that
//! effect and no other, the header projection follows the model, and one undo
//! takes a press back. Does not cover what the renderer draws for a bypassed
//! effect; the resolve tests in `aria-render` do.

use std::path::PathBuf;

use aria_models::ClipId;
use gpui::{Modifiers, TestAppContext, VisualTestContext};

use crate::app::AppState;
use crate::test_support::{drawn_shell, panel_bounds};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// The bypass flag of each effect on the selected clip, as the model holds it
/// and as the header projection draws it.
fn flags(state: &gpui::Entity<AppState>, visual: &VisualTestContext) -> (Vec<bool>, Vec<bool>) {
    visual.read(|app| {
        let state = state.read(app);
        let model = state
            .selected_model()
            .map(|clip| clip.effects.iter().map(|fx| fx.bypass).collect())
            .unwrap_or_default();
        let drawn = state
            .selected_ui_clip_ref()
            .map(|clip| clip.effects.iter().map(|fx| fx.bypass).collect())
            .unwrap_or_default();
        (model, drawn)
    })
}

#[gpui::test]
async fn the_drawn_fx_toggle_switches_its_own_effect_and_undo_takes_it_back(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "effect-bypass");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(fixture()).expect("import the fixture");
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
            for id in ["vignette", "gaussian_blur"] {
                state
                    .add_effect_to_selected(id)
                    .unwrap_or_else(|err| panic!("adding {id}: {err}"));
            }
            state.inspector_tab = "effects".to_string();
            state.dock_tree.open(crate::dock::PANEL_INSPECTOR);
            cx.notify();
        });
    });
    visual.run_until_parked();
    let off = vec![false, false];
    assert_eq!(
        flags(&state, visual),
        (off.clone(), off),
        "an added effect is on"
    );

    for (index, after) in [
        (1, [false, true]),
        (0, [true, true]),
        (1, [true, false]),
        (0, [false, false]),
    ] {
        let selector: &'static str = crate::components::control_selector(
            "switch",
            crate::motion::MotionKey::new("inspector-fx-bypass", index).into(),
            &crate::components::Drawn::Switch,
        )
        .leak();
        let toggle = panel_bounds(visual, selector)
            .unwrap_or_else(|| panic!("the header of effect {index} draws no fx toggle"));
        visual.simulate_click(toggle.center(), Modifiers::none());
        visual.run_until_parked();
        assert_eq!(
            flags(&state, visual),
            (after.to_vec(), after.to_vec()),
            "after pressing the toggle of effect {index}"
        );
    }

    visual.update(|_, app| state.update(app, |state, _| assert!(state.undo())));
    visual.run_until_parked();
    let undone = vec![true, false];
    assert_eq!(
        flags(&state, visual),
        (undone.clone(), undone),
        "one undo takes the last press back"
    );
}
