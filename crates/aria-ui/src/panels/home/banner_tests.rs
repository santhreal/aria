//! The recovery banner draws Restore, the action it is shown for, as the solid
//! button, and Discard as the ghost button beside it.
//!
//! The class closed here: a banner whose two actions draw at one weight, so
//! the press that hands the work back and the press that deletes it read
//! alike. The ghost buttons a drawn Home frame records are read: Discard is
//! among them, Restore is not, and the frame draws the word Restore.
//!
//! Not covered: the solid button's colors, which `components::button` holds,
//! and the order of the two buttons.

use gpui::{Bounds, TestAppContext, point, px, size};

use crate::components::noted::{NotingControls, drawn_controls, record_controls};
use crate::motion::{ControlId, MotionKey};
use crate::test_support::drawn_shell_at;

#[gpui::test]
fn the_recovery_banner_draws_restore_solid_and_discard_as_a_ghost(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let at = Bounds::new(point(px(0.), px(0.)), size(px(1280.), px(800.)));
    let (state, mut visual) = drawn_shell_at(cx, "recovery-banner", at);
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.write_recovery().expect("a recovery copy on disk");
            state.go_home();
            assert!(
                state.pending_recovery().is_some(),
                "the banner has no offer to draw, so its buttons are not drawn"
            );
            cx.notify();
        })
    });

    let _noting = NotingControls::start();
    record_controls();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let (built, _) = drawn_controls();
    let ghost = |part: &str| {
        let id = format!("{:?}", ControlId::from(MotionKey::new("recovery", part)));
        built
            .iter()
            .any(|control| control.selector.starts_with("ghost-") && control.selector.contains(&id))
    };
    assert!(ghost("discard"), "Discard did not draw as a ghost button");
    assert!(
        !ghost("restore"),
        "Restore drew as a ghost button, at the weight of Discard"
    );

    let drawn: Vec<String> = visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .map(|run| run.text.to_string())
            .collect()
    });
    assert!(
        drawn.iter().any(|text| text == "Restore"),
        "the banner drew no Restore: {drawn:?}"
    );
}
