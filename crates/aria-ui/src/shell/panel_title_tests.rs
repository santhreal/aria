//! A panel's body does not repeat the title its tab shows.
//!
//! The class closed here: a panel heading its body with its own name, as the
//! Audio Track Mixer did above its strips, so the name reads twice a tab's
//! height apart and the row costs the panel its height. The tab is where a
//! frame names its panel, in every frame, floating or docked.
//!
//! The panels are read from the registry when the test runs, so a panel added
//! to the app is drawn here without an edit. Each is docked into the launch
//! layout's right frame with a clip selected, so a panel that shows a
//! selection has one, and every line of text drawn in the frame under the
//! panel's tab must differ from the tab's title.
//!
//! Not covered: the title inside a longer line, such as a sentence that names
//! the panel, and a state other than the one a selected clip draws, such as an
//! empty project.

use std::path::PathBuf;

use gpui::{Bounds, TestAppContext, point, px, size};

use crate::components::drawn;
use crate::dock::{DockTree, FrameId, Zone};
use crate::interned::intern;
use crate::test_support::drawn_shell;

/// The window the launch layout is built for.
const WINDOW_W: f32 = 1920.0;

#[gpui::test]
fn no_panel_repeats_its_tab_title_under_its_tab(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "panel-title");
    visual.update(|_, app| app.set_reduce_motion(true));
    let still = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(still).expect("import the still");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the still");
            let clip = state
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|track| track.clips.iter())
                .map(|clip| aria_models::ClipId::from_raw(clip.id))
                .next()
                .expect("a clip");
            state.select_clip(clip);
            cx.notify();
        })
    });
    visual.run_until_parked();
    let panels = visual.read(|app| state.read(app).registered_panel_ids());
    assert!(!panels.is_empty(), "the ui plugin registers panels");
    let mut repeats = Vec::new();
    for panel in panels {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.dock_tree = DockTree::editing_for_width(WINDOW_W);
                state.dock_tree.dock(panel, FrameId::RIGHT, Zone::Center);
                cx.notify();
            })
        });
        visual.run_until_parked();
        // A second draw: the first records the frame's rect, which some
        // panels read to lay themselves out.
        visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
        visual.run_until_parked();
        let (title, rect) = visual.read(|app| {
            let read = state.read(app);
            let rect = read.dock_frame_rects.lock().get(&FrameId::RIGHT).copied();
            (read.dock_panel_title(panel), rect)
        });
        let Some([x, y, w, h]) = rect else {
            repeats.push(format!("{panel}: the right frame was not drawn"));
            continue;
        };
        let frame = Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
        let Some(tab) = visual.debug_bounds(intern(&format!("dock-tab-{panel}"))) else {
            repeats.push(format!("{panel}: its tab was not drawn"));
            continue;
        };
        let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
        repeats.extend(
            runs.iter()
                .filter(|run| !drawn::is_marker(run) && run.text.as_ref() == title)
                .filter(|run| {
                    let mid = run.bounds.center();
                    frame.contains(&mid) && mid.y > tab.bottom()
                })
                .map(|run| {
                    let at = run.bounds;
                    format!("{panel}: `{title}` drawn at {at:?}, under its tab at {tab:?}")
                }),
        );
    }
    assert!(
        repeats.is_empty(),
        "a panel repeats the title its tab shows:\n{}",
        repeats.join("\n")
    );
}
