//! The Info panel prints the alpha under the pointer as Opacity prints.
//!
//! WHY: the sampled alpha printed `100 %`, a space between the number and its
//! sign, while the Opacity row of Effect Controls prints `100%`. The class: a
//! reading on the Info panel printed apart from the units of the property it
//! reads. Checked on the drawn panel for an opaque pixel, one a step short of
//! opaque, a half-covered one and a clear one, and against the Opacity units,
//! so a change to how Opacity prints moves the reading with it.
//!
//! Not covered: the RGB and hex rows, which no property row prints.

use aria_models::ClipParam;
use gpui::TestAppContext;

use crate::app::info::ViewerProbe;
use crate::app::param_units::Units;
use crate::dock::PANEL_INFO;
use crate::preview_worker::Probed;
use crate::test_support::drawn_shell;

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
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.focus_dock_panel(PANEL_INFO);
                state.viewer_probe = ViewerProbe {
                    at: Some(at),
                    color: Some(Probed {
                        at,
                        tick: 0,
                        rgba: [10, 20, 30, alpha],
                    }),
                };
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
        let runs: Vec<_> = visual.update(|window, _| {
            window
                .rendered_text_runs()
                .iter()
                .filter(|run| panel.contains(&run.bounds.center()))
                .cloned()
                .collect()
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
