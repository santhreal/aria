//! The first text drawn in an Effect Controls tab body does not repeat the
//! tab's label as a bare heading.
//!
//! Closes the class "an Effect Controls tab body that opens with a section
//! heading repeating the tab's label under the underlined tab". Look, Effects,
//! Audio, Speed, Text, Shape, and Info each opened with `section_label` set to
//! the tab's label, so selecting Look drew "Look" directly under the underlined
//! "Look" tab before the Filter row. Transform keeps its heading line because
//! `inspector-reset` stands at the right end of that line.
//!
//! Every tab in [`host_of`] and every [`UiClipKind`] the tab applies to are
//! walked at run time. [`place_clip`] matches [`UiClipKind`] without a wildcard
//! arm, so a new clip kind or a new tab is checked on the run that adds it.
//! [`PENDING`] lists the tabs whose files are held by other lanes in flight;
//! each listed tab is asserted to still draw its repeated heading until that
//! lane lands and [`PENDING`] shrinks.
//!
//! Not covered: section headings further down a tab body, such as Presets or
//! Audio effects, which head sub-blocks below the first row.

use std::collections::BTreeSet;
use std::path::PathBuf;

use aria_models::{ClipId, Generator, Shape};
use gpui::{Bounds, Pixels, TestAppContext, TextRunLayout, VisualTestContext};

use super::host_of;
use crate::app::AppState;
use crate::components::drawn;
use crate::dock::PANEL_INSPECTOR;
use crate::panels::inspector::strip_tests::middle;
use crate::test_support::{drawn_shell, panel_bounds};
use crate::types::UiClipKind;

/// Tabs whose opening heading line holds a reset button, and the reset button's
/// debug selector.
const WITH_RESET: [(&str, &str); 1] = [("transform", "inspector-reset")];

/// Tabs whose files are held by other lanes in flight (`OverlayBatch1` for
/// `effects.rs`, `WrapEdge` for `text.rs`) and still open with a bare
/// `section_label` repeating the tab's label.
const PENDING: &[&str] = &[];

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// Place a clip of `kind` on the timeline and return its id. No wildcard arm:
/// a new [`UiClipKind`] variant fails to build until given a fixture here.
fn place_clip(state: &mut AppState, kind: UiClipKind) -> ClipId {
    match kind {
        UiClipKind::Media => {
            let media = state
                .import_media(fixture("fixture-640x360-24fps-3s.mp4"))
                .expect("import the video fixture");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the media clip");
        }
        UiClipKind::Audio => {
            let audio = state
                .import_media(fixture("fixture-sine-48k-2s.wav"))
                .expect("import the audio fixture");
            state
                .drop_media_on_timeline(audio, None, 0)
                .expect("place the audio clip");
        }
        UiClipKind::Text => {
            state
                .add_generated(Generator::text("Sample"))
                .expect("place the title clip");
        }
        UiClipKind::Shape => {
            state
                .add_generated(Generator::shape(Shape::Rectangle, [255, 255, 255, 255]))
                .expect("place the shape clip");
        }
        UiClipKind::Generator => {
            state
                .add_generated(Generator::SolidColor {
                    rgba: [255, 255, 255, 255],
                })
                .expect("place the solid clip");
        }
    }
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.kind == kind)
        .map(|clip| ClipId::from_raw(clip.id))
        .unwrap_or_else(|| panic!("no {kind:?} clip on the timeline"))
}

/// Select `clip` and `tab`, draw the shell, and return the bounds of
/// `inspector-body` together with the text runs painted inside it.
fn body_runs(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    clip: ClipId,
    tab: &str,
) -> (Bounds<Pixels>, Vec<TextRunLayout>) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.dock_tree.open(PANEL_INSPECTOR);
            state.select_clip(clip);
            state.inspector_tab = tab.to_string();
            cx.notify();
        });
    });
    visual.run_until_parked();
    visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
    visual.run_until_parked();
    let body = panel_bounds(visual, "inspector-body")
        .unwrap_or_else(|| panic!("tab `{tab}` drew no inspector-body"));
    let runs = visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .filter(|run| {
                !drawn::is_marker(run)
                    && !run.text.trim().is_empty()
                    && body.contains(&run.bounds.center())
            })
            .cloned()
            .collect()
    });
    (body, runs)
}

#[gpui::test]
fn no_inspector_tab_opens_with_a_bare_heading_repeating_its_label(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "inspector-tab-headings");
    visual.update(|_, app| app.set_reduce_motion(true));

    let clips: Vec<(UiClipKind, ClipId)> = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let placed = UiClipKind::ALL
                .into_iter()
                .map(|kind| (kind, place_clip(state, kind)))
                .collect();
            cx.notify();
            placed
        })
    });
    visual.run_until_parked();

    let host = host_of();
    let mut reached_tabs = BTreeSet::new();
    let mut reached_reset = BTreeSet::new();
    let mut reached_pending = BTreeSet::new();

    for (kind, clip) in clips {
        let ui = visual.read(|app| state.read(app).ui_clip(clip).expect("projected clip"));
        for tab in host.tabs().iter().filter(|tab| (tab.applies)(&ui)) {
            let id = tab.id.as_ref();
            let label = tab.label.as_ref();
            let what = format!("Effect Controls tab `{id}` ({label:?}) on a {kind:?} clip");
            let (body, runs) = body_runs(&state, visual, clip, id);
            let first = runs
                .first()
                .unwrap_or_else(|| panic!("{what} drew no text inside inspector-body"));
            reached_tabs.insert(id.to_string());

            if let Some(&(_, reset_sel)) = WITH_RESET.iter().find(|&&(reset_id, _)| reset_id == id)
            {
                reached_reset.insert(id.to_string());
                assert_eq!(
                    first.text.as_ref(),
                    label,
                    "{what} did not open with its `{label}` heading line"
                );
                let reset = panel_bounds(visual, reset_sel)
                    .unwrap_or_else(|| panic!("{what} drew no `{reset_sel}`"));
                assert!(
                    body.contains(&reset.center()),
                    "{what} drew `{reset_sel}` outside inspector-body"
                );
                assert!(
                    (middle(reset) - middle(first.bounds)).abs() < 1.0,
                    "{what} drew `{reset_sel}` at {reset:?}, off the line of `{label}` at {:?}",
                    first.bounds
                );
                let below = runs
                    .iter()
                    .find(|run| run.bounds.top() >= first.bounds.bottom())
                    .unwrap_or_else(|| panic!("{what} drew no row below its heading line"));
                assert_ne!(
                    below.text.as_ref(),
                    label,
                    "{what} repeated `{label}` below its heading line"
                );
            } else if PENDING.contains(&id) {
                reached_pending.insert(id.to_string());
                assert_eq!(
                    first.text.as_ref(),
                    label,
                    "{what} opened with {:?} instead of `{label}`; remove `{id}` from PENDING",
                    first.text
                );
            } else {
                assert_ne!(
                    first.text.as_ref(),
                    label,
                    "{what} opens with a bare heading repeating its tab label `{label}`"
                );
            }
        }
    }

    let expected_tabs: BTreeSet<String> =
        host.tabs().iter().map(|tab| tab.id.to_string()).collect();
    assert_eq!(
        reached_tabs, expected_tabs,
        "a registered tab applied to none of UiClipKind::ALL"
    );
    let expected_reset: BTreeSet<String> =
        WITH_RESET.iter().map(|&(id, _)| id.to_string()).collect();
    assert_eq!(
        reached_reset, expected_reset,
        "WITH_RESET lists a tab id that was never reached"
    );
    let expected_pending: BTreeSet<String> = PENDING.iter().map(|&id| id.to_string()).collect();
    assert_eq!(
        reached_pending, expected_pending,
        "PENDING lists a tab id that was never reached"
    );
}
