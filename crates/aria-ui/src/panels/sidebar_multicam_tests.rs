//! A multicam group's angle rows fit the Project panel at every width the panel
//! can be.
//!
//! WHY: an angle row stands its chip, a 168 px sound strip, two nudge buttons
//! and its offset on one line, wider than the pool column at the narrowest and
//! the default panel width. The row fits only while the strip shrinks to what
//! the other parts leave; a strip that keeps its width draws the offset and the
//! forward nudge past the panel's edge, where they can be neither read nor
//! pressed. The class closed here, at the narrowest, the default and the
//! widest panel width, over every angle of a group: a part of an angle row
//! outside its group's box, a part off the row's center line, a strip wider
//! than its widest, and a group box that is not the pool column's width.
//!
//! Not covered: a long angle or group name, which the default names do not
//! reach; more groups than one, which stack the same rows.

use gpui::{Bounds, Pixels, TestAppContext, px};

use super::body_tests::open;
use super::layout_tests::{bounds, project_width};
use super::multicam::STRIP_W;
use crate::settings::{SIDEBAR_DEFAULT, SIDEBAR_MAX, SIDEBAR_MIN};
use crate::surfaces::sidebar_pick::{self, Pick};
use crate::test_support::drawn_shell;
use crate::theme::Theme;
use crate::types::SidebarTab;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

/// Every part of an angle row, left to right, by its selector suffix.
const PARTS: [&str; 5] = ["chip", "strip", "back", "offset", "forward"];

#[gpui::test]
fn every_multicam_angle_row_fits_its_group_at_every_panel_width(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "sidebar-multicam");
    let fixtures =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let (id, angles) = visual.update(|_, app| {
        state.update(app, |state, cx| {
            for name in [
                "fixture-640x360-24fps-3s-av.mp4",
                "fixture-640x360-30fps-3s.mp4",
            ] {
                state
                    .import_media(fixtures.join(name))
                    .unwrap_or_else(|err| panic!("import {name}: {err}"));
            }
            state.sync_from_engine();
            sidebar_pick::apply(state, Pick::MulticamGroup).expect("the pool builds a group");
            let (id, angles) = state
                .multicam_groups()
                .last()
                .map(|group| (group.id, group.angles.len()))
                .expect("the button built a group");
            // The widest offset a nudge leaves, so the label is at its longest
            // drawn width rather than the "0f" every angle starts at.
            sidebar_pick::apply(
                state,
                Pick::MulticamNudge {
                    group: aria_models::MulticamGroupId::from_raw(id),
                    angle: angles - 1,
                    frames: -12,
                },
            )
            .expect("the angle takes the nudge");
            cx.notify();
            (id, angles)
        })
    });
    assert_eq!(angles, 2, "both imported files are angles of the group");
    open(&state, visual, SidebarTab::Media);

    for width in [SIDEBAR_MIN, SIDEBAR_DEFAULT, SIDEBAR_MAX] {
        project_width(&state, visual, width);
        let column = bounds(visual, "pool-header");
        let group = bounds(visual, &format!("multicam-group-{id}"));
        assert!(
            near(group.left(), column.left()) && near(group.right(), column.right()),
            "at {width} px the group {group:?} is not the pool column {column:?} wide"
        );
        let inset = px(Theme::SPACE_XS - EPS);
        for angle in 0..angles {
            let drawn: Vec<(&str, Bounds<Pixels>)> = PARTS
                .iter()
                .map(|part| {
                    (
                        *part,
                        bounds(visual, &format!("multicam-{id}-{angle}-{part}")),
                    )
                })
                .collect();
            let line = drawn[0].1.center().y;
            for (part, at) in &drawn {
                assert!(
                    at.left() >= group.left() + inset && at.right() <= group.right() - inset,
                    "at {width} px angle {angle}'s {part} {at:?} stands outside its group {group:?}"
                );
                assert!(
                    near(at.center().y, line),
                    "at {width} px angle {angle}'s {part} {at:?} is off its row's line {line:?}"
                );
            }
            let strip = drawn[1].1.size.width;
            assert!(
                strip > px(0.0) && strip <= px(STRIP_W + EPS),
                "at {width} px angle {angle}'s strip is {strip:?} wide, not within 0..={STRIP_W} px"
            );
        }
    }
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() <= px(EPS)
}
