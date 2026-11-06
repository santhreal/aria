//! What the Info panel draws: the alpha under the pointer as Opacity prints
//! it, and the Errors section only while a fault is held.
//!
//! WHY, alpha: the sampled alpha printed `100 %`, a space between the number
//! and its sign, while the Opacity row of Effect Controls prints `100%`. The
//! class: a reading on the Info panel printed apart from the units of the
//! property it reads. Checked on the drawn panel for an opaque pixel, one a
//! step short of opaque, a half-covered one and a clear one, and against the
//! Opacity units, so a change to how Opacity prints moves the reading with it.
//!
//! WHY, errors: every session drew an Errors heading over "No errors.", a
//! status section holding nothing, beside Drag and Playback, which are drawn
//! only while they hold something. Checked with no fault, then with a mount
//! failure recorded on the session's plugin context.
//!
//! Not covered: the RGB and hex rows, which no property row prints; driver
//! and plugin faults, which reach the same rows through `info_faults`.

use aria_models::ClipParam;
use gpui::{TestAppContext, TextRunLayout, VisualTestContext};

use crate::app::AppState;
use crate::app::info::ViewerProbe;
use crate::app::param_units::Units;
use crate::dock::PANEL_INFO;
use crate::preview_worker::Probed;
use crate::test_support::drawn_shell;

/// Bring Info forward, let `change` set the session, draw, and return the
/// text runs drawn inside the panel.
fn panel_runs(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    change: impl FnOnce(&mut AppState),
) -> Vec<TextRunLayout> {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.focus_dock_panel(PANEL_INFO);
            change(state);
            cx.notify();
        })
    });
    for _ in 0..2 {
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
    }
    let panel = visual
        .debug_bounds("info-panel")
        .expect("the Info panel is drawn");
    visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .filter(|run| panel.contains(&run.bounds.center()))
            .cloned()
            .collect()
    })
}

#[gpui::test]
fn the_alpha_under_the_pointer_prints_as_opacity_does(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "info-alpha");
    visual.update(|_, app| app.set_reduce_motion(true));
    let opacity = Units::of(ClipParam::Opacity, &ClipParam::Opacity.range());
    // 254 is 99.6% coverage and rounds up as Opacity rounds; read against 256
    // it would print 99%.
    for (alpha, printed) in [(255, "100%"), (254, "100%"), (128, "50%"), (0, "0%")] {
        assert_eq!(
            opacity.print(f32::from(alpha) / 255.0),
            printed,
            "Opacity prints a {alpha}/255 coverage"
        );
        let at = [3, 4];
        let runs = panel_runs(&state, visual, |state| {
            state.viewer_probe = ViewerProbe {
                at: Some(at),
                color: Some(Probed {
                    at,
                    tick: 0,
                    rgba: [10, 20, 30, alpha],
                }),
            };
        });
        let label = runs
            .iter()
            .find(|run| run.text.as_ref() == "Alpha")
            .unwrap_or_else(|| panic!("no Alpha row drawn for alpha {alpha}"));
        let row: Vec<&str> = runs
            .iter()
            .filter(|run| {
                (run.bounds.center().y - label.bounds.center().y).abs() < gpui::px(1.0)
                    && run.bounds.left() >= label.bounds.right()
            })
            .map(|run| run.text.as_ref())
            .collect();
        assert_eq!(row, [printed], "the Alpha row for alpha {alpha}");
    }
}

#[gpui::test]
fn the_errors_section_is_drawn_only_while_a_fault_is_held(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "info-errors");
    visual.update(|_, app| app.set_reduce_motion(true));
    let texts = |runs: &[TextRunLayout]| -> Vec<String> {
        runs.iter().map(|run| run.text.to_string()).collect()
    };

    let quiet = texts(&panel_runs(&state, visual, |_| {}));
    assert!(
        quiet.iter().any(|text| text == "Last frame"),
        "the panel was not drawn: {quiet:?}"
    );
    for absent in ["Errors", "No errors."] {
        assert!(
            !quiet.iter().any(|text| text == absent),
            "a session with no fault drew {absent:?}: {quiet:?}"
        );
    }

    let failed = texts(&panel_runs(&state, visual, |state| {
        state.ctx.record_external_failure(
            "test.broken".into(),
            "Broken Plugin".into(),
            "the package did not load".into(),
        );
    }));
    for present in ["Errors", "Broken Plugin", "the package did not load"] {
        assert!(
            failed.iter().any(|text| text == present),
            "a held mount failure drew no {present:?}: {failed:?}"
        );
    }
}
