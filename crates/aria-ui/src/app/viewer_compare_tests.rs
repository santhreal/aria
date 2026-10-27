//! Viewer comparison.
//!
//! The class closed here: a comparison that shows something other than what
//! the row asked for. A picture rendered for a reference already replaced is
//! dropped rather than drawn under the newer reference's caption; a frame
//! reference keeps the frame it was taken at while the playhead moves; a still
//! that cannot be read stops the comparison and reports why; and the divider
//! follows the pointer across the stage it is drawn on, clamped to it, along
//! the axis the layout splits. And a second class: a reference picture let go
//! of stayed in the window atlas for the life of the window. Every path that
//! lets one go hands it to the atlas release.
//!
//! The drawn cases drive the shell's own pointer routing: a press on the
//! divider, moves anywhere in the window, and the release.
//!
//! Not covered: pixels of the drawn reference, and a Source tab's decode,
//! which `pool_preview_tests` drives.

use gpui::{
    AppContext as _, Bounds, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext,
    point,
};

use super::{CompareLayout, Reference, divider_at};
use crate::app::AppState;
use crate::app::picker::Answer;
use crate::app::premiere_tests::with_clip;

const STAGE: [f32; 4] = [100.0, 50.0, 400.0, 300.0];

fn texture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

fn one_pixel() -> std::sync::Arc<gpui::RenderImage> {
    crate::frame::rgba_bytes_to_render(1, 1, &[255, 0, 0, 255]).expect("one pixel")
}

#[test]
fn the_divider_follows_the_pointer_along_the_split_axis_clamped_to_the_stage() {
    let cases = [
        (CompareLayout::Vertical, [200.0, 9999.0], 0.25),
        (CompareLayout::SideBySide, [400.0, -50.0], 0.75),
        (CompareLayout::Horizontal, [9999.0, 125.0], 0.25),
        (CompareLayout::Vertical, [0.0, 0.0], 0.0),
        (CompareLayout::Vertical, [900.0, 0.0], 1.0),
        (CompareLayout::Horizontal, [0.0, 900.0], 1.0),
    ];
    for (layout, at, want) in cases {
        assert_eq!(
            divider_at(layout, STAGE, at),
            Some(want),
            "{layout:?} at {at:?}"
        );
    }
    let flat = [0.0, 0.0, 0.0, 0.0];
    for layout in [CompareLayout::Vertical, CompareLayout::Horizontal] {
        assert_eq!(divider_at(layout, flat, [5.0, 5.0]), None, "{layout:?}");
    }
}

#[test]
fn a_picture_for_a_replaced_reference_is_dropped() {
    let (mut state, _) = with_clip();
    let at = state.playhead;
    let frame = state.viewer_compare.begin(Reference::Frame { at });
    let still = state
        .viewer_compare
        .begin(Reference::Still { path: texture() });
    assert!(!state.viewer_compare.land(frame, one_pixel()));
    assert!(
        state.viewer_compare.picture().is_none(),
        "the late frame was kept"
    );
    assert!(state.viewer_compare.land(still, one_pixel()));
    assert!(state.viewer_compare.picture().is_some());
    state.stop_comparing();
    assert!(
        !state.viewer_compare.land(still, one_pixel()),
        "a stopped comparison kept a picture"
    );
    assert!(state.viewer_compare.picture().is_none());
}

/// Only a press on the divider moves it, and only while something is compared.
#[test]
fn the_divider_moves_only_while_it_is_held() {
    let (mut state, _) = with_clip();
    state.viewer_compare.record_stage(STAGE);
    state.grab_compare_divider();
    assert!(
        !state.drag_compare_divider([200.0, 60.0]),
        "nothing is compared"
    );
    state
        .viewer_compare
        .begin(Reference::Frame { at: state.playhead });
    assert!(
        !state.drag_compare_divider([200.0, 60.0]),
        "the divider is not held"
    );
    state.grab_compare_divider();
    assert!(state.drag_compare_divider([200.0, 60.0]));
    assert_eq!(state.viewer_compare.divider(), 0.25);
    assert!(
        !state.drag_compare_divider([200.0, 90.0]),
        "a move along the line moved it"
    );
    state.set_compare_layout(CompareLayout::Horizontal);
    assert!(state.drag_compare_divider([200.0, 200.0]));
    assert_eq!(state.viewer_compare.divider(), 0.5);
    assert!(state.release_compare_divider());
    assert!(!state.release_compare_divider(), "released twice");
    assert!(!state.drag_compare_divider([400.0, 200.0]));
}

/// Source Tab with nothing in the Source monitor compares nothing and reports
/// the reason the menu row is drawn disabled with.
#[test]
fn source_tab_needs_a_source_in_the_monitor() {
    let (mut state, _) = with_clip();
    state.compare_with_source();
    assert!(!state.viewer_compare.comparing());
    let reason = crate::chrome::menu_guards::source("view-compare-source", &state);
    assert!(reason.is_some(), "the row is drawn enabled with no source");
    assert_eq!(state.status_message.as_deref(), reason);
}

/// Source Tab holds the source in the Source monitor and brings Program back
/// to the Viewer, where the comparison is drawn.
#[test]
fn source_tab_compares_with_the_source_in_the_monitor() {
    let (mut state, _) = with_clip();
    let media = state
        .store
        .media
        .first()
        .map(|item| item.id)
        .expect("a pool item");
    state.select_pool_item(media);
    assert!(
        state.toggle_source_monitor(),
        "the pool item cannot be previewed"
    );
    let name = state
        .monitor_source()
        .map(|view| view.name.clone())
        .expect("a source");
    assert!(crate::chrome::menu_guards::source("view-compare-source", &state).is_none());
    state.compare_with_source();
    assert_eq!(state.viewer_compare.reference(), Some(&Reference::Source));
    assert!(
        state.sources.focused().is_none(),
        "the Source tab still covers Program"
    );
    assert_eq!(
        state.monitor_source().map(|view| view.name.clone()),
        Some(name.clone())
    );
    assert_eq!(state.status_message, Some(format!("Comparing with {name}")));
}

fn entity(state: AppState, cx: &mut TestAppContext) -> gpui::Entity<AppState> {
    cx.update(|app| app.new(|_| state))
}

/// The frame under the playhead is rendered and held; moving the playhead
/// afterwards leaves the reference where it was taken.
#[gpui::test]
fn the_frame_reference_holds_the_frame_it_was_taken_at(cx: &mut TestAppContext) {
    let (state, _) = with_clip();
    let state = entity(state, cx);
    let at = cx.update(|app| state.read(app).playhead);
    cx.update(|app| state.update(app, |state, cx| state.compare_with_frame(cx)));
    cx.run_until_parked();
    cx.update(|app| {
        let read = state.read(app);
        assert_eq!(read.last_error, None);
        assert_eq!(
            read.viewer_compare.reference(),
            Some(&Reference::Frame { at })
        );
        assert!(
            read.viewer_compare.picture().is_some(),
            "the frame never arrived"
        );
    });
    cx.update(|app| {
        state.update(app, |state, _| {
            state.playhead.value += 12;
        })
    });
    cx.update(|app| {
        assert_eq!(
            state.read(app).viewer_compare.reference(),
            Some(&Reference::Frame { at }),
            "the reference followed the playhead"
        );
    });
}

/// Still asks for a path, then decodes the answer at its own size.
#[gpui::test]
fn the_still_reference_reads_the_file_it_was_answered_with(cx: &mut TestAppContext) {
    let (mut state, _) = with_clip();
    let want = aria_decoder::probe_image(&texture()).expect("the fixture is an image");
    state.supply_pick(Answer::file(texture()));
    let state = entity(state, cx);
    cx.update(|app| state.update(app, |state, cx| state.pick_compare_still(cx)));
    cx.run_until_parked();
    cx.update(|app| {
        let read = state.read(app);
        assert_eq!(read.last_error, None);
        assert_eq!(
            read.viewer_compare.reference(),
            Some(&Reference::Still { path: texture() })
        );
        let picture = read
            .viewer_compare
            .picture()
            .expect("the still never arrived");
        let size = picture.size(0);
        assert_eq!(
            (size.width.0 as u32, size.height.0 as u32),
            (want.width, want.height)
        );
    });
}

/// A still that cannot be read stops the comparison and says which file.
#[gpui::test]
fn an_unreadable_still_stops_the_comparison(cx: &mut TestAppContext) {
    let (state, _) = with_clip();
    let state = entity(state, cx);
    let missing = std::path::PathBuf::from("/nonexistent/compare.png");
    cx.update(|app| {
        state.update(app, |state, cx| {
            state.compare_with_still(missing.clone(), cx)
        })
    });
    cx.run_until_parked();
    cx.update(|app| {
        let read = state.read(app);
        assert!(!read.viewer_compare.comparing());
        let error = read.last_error.clone().unwrap_or_default();
        assert!(error.contains("compare.png"), "{error}");
    });
}

fn bounds(visual: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    crate::test_support::panel_bounds(visual, selector)
        .unwrap_or_else(|| panic!("nothing drawn as {selector}"))
}

/// Pressing the divider and moving the pointer moves it, in every layout, and
/// the reference's share of the stage follows it.
#[gpui::test]
fn the_drawn_divider_follows_a_drag_in_every_layout(cx: &mut TestAppContext) {
    let (state, visual) = crate::test_support::drawn_shell(cx, "viewer-compare-drag");
    for layout in [
        CompareLayout::Vertical,
        CompareLayout::Horizontal,
        CompareLayout::SideBySide,
    ] {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.viewer_compare = Default::default();
                state.set_compare_layout(layout);
                state
                    .viewer_compare
                    .begin(Reference::Frame { at: state.playhead });
                cx.notify();
            });
        });
        visual.run_until_parked();
        let divider = bounds(visual, "compare-divider");
        let reference = bounds(visual, "compare-reference");
        let grab = divider.center();
        let target = if layout.horizontal() {
            point(grab.x, reference.origin.y + reference.size.height * 1.5)
        } else {
            point(reference.origin.x + reference.size.width * 1.5, grab.y)
        };
        visual.simulate_mouse_down(grab, MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_move(target, MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_up(target, MouseButton::Left, Modifiers::none());
        visual.run_until_parked();
        let split = visual.read(|app| state.read(app).viewer_compare.divider());
        assert!(
            (split - 0.75).abs() < 0.02,
            "{layout:?}: the divider sits at {split} after a drag to 3/4 of the stage"
        );
        assert!(
            !visual.read(|app| state.read(app).viewer_compare.dragging()),
            "{layout:?}: the divider stayed held after the release"
        );
        let moved = bounds(visual, "compare-reference");
        let (before, after) = if layout.horizontal() {
            (reference.size.height, moved.size.height)
        } else {
            (reference.size.width, moved.size.width)
        };
        assert!(
            after > before * 1.3,
            "{layout:?}: the reference's share did not follow the divider: {before:?} then {after:?}"
        );
    }
}

/// Every path that lets a picture go: a new reference, a stop, and a picture
/// landing over one held. A late picture was never held and is not counted.
#[test]
fn every_picture_the_comparison_lets_go_of_goes_to_the_atlas_release() {
    let (mut state, _) = with_clip();
    let at = state.playhead;
    let compare = &mut state.viewer_compare;
    let mut given = Vec::new();
    let mut released = Vec::new();
    let mut keep = |compare: &mut super::ViewerCompare, generation: u64| {
        let picture = one_pixel();
        assert!(compare.land(generation, std::sync::Arc::clone(&picture)));
        given.push(picture);
    };
    let still = compare.begin(Reference::Still { path: texture() });
    keep(compare, still);
    keep(compare, still);
    let frame = compare.begin(Reference::Frame { at });
    assert!(!compare.land(still, one_pixel()), "a late picture was kept");
    keep(compare, frame);
    state.stop_comparing();
    state.viewer_compare.retire_into(&mut released);
    crate::test_support::atlas::held_or_released_once(
        state.viewer_compare.picture(),
        &released,
        &given,
        "a new reference, a stop, and a landing over a held picture",
    );
    assert_eq!(released.len(), 3);
}

/// WHY: a reference picture the comparison let go of stayed in the window
/// atlas until the window closed, so each new reference added one. A still
/// drawn in the Viewer and then replaced is gone from the atlas once the
/// picture replacing it has landed.
#[gpui::test]
async fn a_picture_the_comparison_lets_go_of_leaves_the_window_atlas(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "viewer-compare-atlas");
    let compare = |visual: &mut VisualTestContext| {
        visual.update(|window, app| {
            state.update(app, |state, cx| {
                state.compare_with_still(texture(), cx);
                cx.notify();
            });
            window.refresh();
        });
        visual.run_until_parked();
        visual
            .read(|app| state.read(app).viewer_compare.picture().cloned())
            .expect("the still arrived")
    };
    let drawn = compare(visual);
    assert!(
        visual.update(|window, _| window.has_image_atlas_entry(&drawn)),
        "the Viewer drew the still"
    );
    let next = compare(visual);
    assert!(!std::sync::Arc::ptr_eq(&drawn, &next));
    assert!(
        !visual.update(|window, _| window.has_image_atlas_entry(&drawn)),
        "the replaced still stayed in the window atlas"
    );
}
