//! Every line a dock panel draws starts in sentence case.
//!
//! The class closed here: a readout value, a tag or a note that starts in
//! lower case among capitalised lines, as the pool's `linked` and `no`, the
//! history row's `current` and the mixer's `not in the mix` were. A line is held to [`lowercase_start`], the
//! check the status line and the dialogs are held to.
//!
//! The panels are read from the registry and the inspector's tabs from its tab
//! host when the test runs, so a panel or a tab added later is checked on the
//! run that adds it, and a tab no dressed clip kind draws fails the walk. Each
//! panel is docked into the launch layout's right frame and drawn with a media
//! clip selected, its source picked in the pool and a frame reading for the
//! Info panel. The Inspector is drawn once per tab for each of a media clip, a
//! title and a shape the tab applies to. The Media Support report is a dialog,
//! and `chrome::dialog_tests` walks it.
//!
//! A line the project supplies, such as a media name, a clip name, a lane name
//! or a path, is written as the project wrote it and is skipped. Not covered: a
//! lower case word inside a line, and a line a panel draws only after a choice
//! changes or a job reports.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use aria_models::{ClipId, Generator, Shape};
use gpui::{Bounds, TestAppContext, VisualTestContext, point, px, size};

use crate::app::AppState;
use crate::app::status::lowercase_start;
use crate::components::drawn;
use crate::dock::{DockTree, FrameId, PANEL_INSPECTOR, PanelId, Zone};
use crate::panels::inspector::tab_host::host_of as tab_host;
use crate::test_support::{drawn_shell, project_text};
use crate::types::UiClipKind;

/// The window the launch layout is built for.
const WINDOW_W: f32 = 1920.0;

#[gpui::test]
fn every_line_a_panel_draws_starts_in_sentence_case(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "panel-casing");
    visual.update(|_, app| app.set_reduce_motion(true));
    let clips = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let clips = dress(state);
            cx.notify();
            clips
        })
    });
    visual.run_until_parked();
    let project = visual.read(|app| project_text(state.read(app)));
    let (media, _) = clips[0];

    let mut offenders = Vec::new();
    let panels = visual.read(|app| state.read(app).registered_panel_ids());
    assert!(!panels.is_empty(), "the ui plugin registers panels");
    for panel in panels {
        let lines = drawn_lines(&state, visual, panel, |state| state.select_clip(media));
        check(panel, &lines, &project, &mut offenders);
    }

    let mut drawn_tabs = BTreeSet::new();
    for (clip, kind) in clips {
        for tab in tab_host().visible(kind) {
            let id = tab.id.to_string();
            let what = format!("the Inspector's {id} tab for a {kind:?} clip");
            let lines = drawn_lines(&state, visual, PANEL_INSPECTOR, |state| {
                state.select_clip(clip);
                state.inspector_tab = id.clone();
            });
            assert!(!lines.is_empty(), "{what} drew no line");
            check(&what, &lines, &project, &mut offenders);
            drawn_tabs.insert(id);
        }
    }
    let shipped: BTreeSet<String> = tab_host()
        .tabs()
        .iter()
        .map(|tab| tab.id.to_string())
        .collect();
    assert_eq!(
        drawn_tabs, shipped,
        "a tab applies to none of the dressed clip kinds; dress a clip it applies to"
    );
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// Place a media clip, a title and a shape, pick the media in the pool and
/// read a frame for the Info panel. Returns every clip with its kind, the media
/// clip first.
fn dress(state: &mut AppState) -> Vec<(ClipId, UiClipKind)> {
    let still = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
    let media = state.import_media(still).expect("import the still");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place the still");
    state
        .add_generated(Generator::text("Title"))
        .expect("place a title");
    state
        .add_generated(Generator::shape(Shape::Rectangle, [255, 255, 255, 255]))
        .expect("place a shape");
    state.select_pool_item(media.raw());
    let mut clips: Vec<(ClipId, UiClipKind)> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| (ClipId::from_raw(clip.id), clip.kind))
        .collect();
    clips.sort_by_key(|(_, kind)| *kind != UiClipKind::Media);
    for kind in [UiClipKind::Media, UiClipKind::Text, UiClipKind::Shape] {
        assert!(
            clips.iter().any(|(_, placed)| *placed == kind),
            "no {kind:?} clip was placed: {clips:?}"
        );
    }
    state.select_clip(clips[0].0);
    state.frame_stats = aria_render::FrameStats {
        decode_ms: 12.5,
        raster_ms: 3.25,
        composite_ms: 1.5,
        effect_passes: 2,
        effect_pass_bind_groups: 2,
    };
    clips
}

/// Dock `panel` alone into the launch layout's right frame, apply `then`, draw
/// twice and return every line drawn in the frame.
///
/// The second draw is the one read: the first records the frame's rect, which
/// some panels read to lay themselves out.
fn drawn_lines(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    panel: PanelId,
    then: impl FnOnce(&mut AppState),
) -> Vec<String> {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.dock_tree = DockTree::editing_for_width(WINDOW_W);
            state.dock_tree.dock(panel, FrameId::RIGHT, Zone::Center);
            then(state);
            cx.notify();
        })
    });
    visual.run_until_parked();
    visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
    visual.run_until_parked();
    let rect = visual.read(|app| {
        state
            .read(app)
            .dock_frame_rects
            .lock()
            .get(&FrameId::RIGHT)
            .copied()
    });
    let Some([x, y, w, h]) = rect else {
        panic!("{panel}: the right frame was not drawn");
    };
    let frame = Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
    visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .filter(|run| !drawn::is_marker(run) && frame.contains(&run.bounds.center()))
            .map(|run| run.text.to_string())
            .collect()
    })
}

/// Record every line of `lines` that starts in lower case and is not text the
/// project supplies.
fn check(what: &str, lines: &[String], project: &HashSet<String>, offenders: &mut Vec<String>) {
    for line in lines {
        if project.iter().any(|text| line.starts_with(text.as_str())) {
            continue;
        }
        if let Some(word) = lowercase_start(line) {
            offenders.push(format!("{what}: {line:?} starts with {word:?}"));
        }
    }
}
