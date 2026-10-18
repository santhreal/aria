//! Tests for re-creating a closed standard frame through [`DockTree::open`] and
//! [`DockTree::open_in`].
//!
//! Closes the class of reopened panels that land as a tab of an unrelated frame
//! (the Timeline's, as the largest) after their own frame closed or floated
//! away. Covers every [`DefaultFrame`] variant, Audio Meters and its own frame,
//! every shipped workspace tree in both its shipped and pane-layout forms, a
//! split inside the centre column, and the fallbacks when no anchor frame is
//! docked. Does not cover a frame dragged elsewhere before it closed: the
//! re-created frame returns to its edge of the shipped layout, not to the
//! dragged position.

use crate::dock::{
    Axis, DefaultFrame, DockTree, FloatRect, FrameId, Node, PANEL_AUDIO_METERS, PANEL_EFFECTS,
    PANEL_INFO, PANEL_INSPECTOR, PANEL_JOBS, PANEL_MIXER, PANEL_PROJECT, PANEL_TIMELINE,
    PANEL_VIEWER, PanelId, Zone,
};

const EPS: f32 = 1e-4;

/// Every [`DefaultFrame`] variant. [`re_created`] matches exhaustively, so a new
/// variant fails to compile there until it records whether its frame returns.
const DEFAULT_FRAMES: [DefaultFrame; 4] = [
    DefaultFrame::Left,
    DefaultFrame::Center,
    DefaultFrame::Right,
    DefaultFrame::Bottom,
];

/// Whether opening a panel re-creates the closed frame for `pref`, and the edge
/// of the window the re-created frame touches.
fn re_created(pref: DefaultFrame) -> Option<Edge> {
    match pref {
        DefaultFrame::Left => Some(Edge::Left),
        DefaultFrame::Right => Some(Edge::Right),
        DefaultFrame::Bottom => Some(Edge::Bottom),
        DefaultFrame::Center => None,
    }
}

#[derive(Debug, Clone, Copy)]
enum Edge {
    Left,
    Right,
    Bottom,
}

impl Edge {
    fn touched_by(self, [x, y, w, h]: [f32; 4]) -> bool {
        match self {
            Self::Left => x.abs() < EPS,
            Self::Right => (x + w - 1.0).abs() < EPS,
            Self::Bottom => (y + h - 1.0).abs() < EPS,
        }
    }
}

/// Every docked frame with its rect `[x, y, w, h]` as a fraction of the window.
fn rects(tree: &DockTree) -> Vec<(FrameId, [f32; 4])> {
    fn walk(node: &Node, [x, y, w, h]: [f32; 4], out: &mut Vec<(FrameId, [f32; 4])>) {
        match node {
            Node::Frame(frame) => out.push((frame.id, [x, y, w, h])),
            Node::Split { axis, children } => {
                let total: f32 = children.iter().map(|(cw, _)| *cw).sum();
                let mut at = 0.0;
                for (cw, child) in children {
                    let frac = cw / total;
                    let rect = match axis {
                        Axis::Row => [x + at * w, y, frac * w, h],
                        Axis::Column => [x, y + at * h, w, frac * h],
                    };
                    walk(child, rect, out);
                    at += frac;
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&tree.root, [0.0, 0.0, 1.0, 1.0], &mut out);
    out
}

fn rect_of(tree: &DockTree, id: FrameId) -> [f32; 4] {
    rects(tree)
        .into_iter()
        .find_map(|(fid, rect)| (fid == id).then_some(rect))
        .unwrap_or_else(|| panic!("frame {id} is not docked: {:?}", tree.root))
}

fn assert_rect(got: [f32; 4], want: [f32; 4], what: &str) {
    assert!(
        got.iter().zip(want).all(|(g, w)| (g - w).abs() < EPS),
        "{what}: rect {got:?}, want {want:?}"
    );
}

/// `tree` docks the frames of `shipped` in the same reading order and rects.
fn assert_same_layout(tree: &DockTree, shipped: &DockTree, what: &str) {
    let (got, want) = (rects(tree), rects(shipped));
    assert_eq!(
        got.iter().map(|(fid, _)| *fid).collect::<Vec<_>>(),
        want.iter().map(|(fid, _)| *fid).collect::<Vec<_>>(),
        "{what}: frames return in the shipped reading order"
    );
    for ((fid, g), (_, w)) in got.into_iter().zip(want) {
        assert_rect(g, w, &format!("{what}: frame {fid}"));
    }
}

/// Close every panel of the frame `id` and return them in tab order.
fn close_frame(tree: &mut DockTree, id: FrameId) -> Vec<PanelId> {
    let tabs = tree.frame(id).expect("frame is present").tabs.clone();
    for &panel in &tabs {
        assert!(tree.close(panel), "{panel} closes");
    }
    assert!(
        tree.frame(id).is_none(),
        "frame {id} is gone once its panels close"
    );
    tabs
}

fn shipped_and_pane_trees() -> Vec<(String, DockTree)> {
    crate::layouts::WORKSPACES
        .iter()
        .flat_map(|ws| {
            let shipped = crate::layouts::workspace_tree(ws.id, &[]).expect("shipped workspace");
            [
                (format!("{} shipped", ws.id), shipped),
                (format!("{} pane layout", ws.id), ws.panes.to_dock_tree()),
            ]
        })
        .collect()
}

#[test]
fn closing_and_reopening_a_standard_frame_restores_the_shipped_editing_layout() {
    let shipped = DockTree::editing();
    for pref in DEFAULT_FRAMES {
        if re_created(pref).is_none() {
            continue;
        }
        let id = pref.frame();
        let mut tree = shipped.clone();
        let panels = close_frame(&mut tree, id);
        for &panel in &panels {
            assert_eq!(tree.open(panel), id, "{panel} reopens in frame {id}");
        }
        tree.check_invariants()
            .expect("invariants after the frame returns");
        assert_eq!(tree.focused, id, "the re-created frame {id} takes focus");
        assert_eq!(tree.frame(id).expect("frame returns").tabs, panels);
        assert_same_layout(&tree, &shipped, &format!("{pref:?}"));
    }
}

#[test]
fn a_panel_floated_out_of_its_frame_and_closed_reopens_in_the_re_created_frame() {
    let shipped = DockTree::editing();
    let mut tree = shipped.clone();
    let floating = tree
        .float(PANEL_EFFECTS, FloatRect::new(200.0, 200.0, 420.0, 360.0))
        .expect("Effects & Presets floats");
    assert_ne!(
        floating,
        FrameId::RIGHT,
        "a floating frame takes a fresh id"
    );
    assert!(tree.frame(FrameId::RIGHT).is_none(), "its column closed");
    assert!(tree.close(PANEL_EFFECTS), "closing the floating window");

    assert_eq!(tree.open(PANEL_EFFECTS), FrameId::RIGHT);
    assert_eq!(
        tree.frame(FrameId::BOTTOM).expect("Timeline frame").tabs,
        [PANEL_TIMELINE]
    );
    assert_same_layout(&tree, &shipped, "Effects & Presets column");
}

#[test]
fn a_side_frame_returns_to_the_upper_row_not_into_a_split_inside_the_centre_column() {
    let shipped = DockTree::editing();
    let mut tree = shipped.clone();
    let below = tree
        .dock(PANEL_INFO, FrameId::CENTER, Zone::Bottom)
        .expect("Info docks under the Viewer");
    let beside = tree
        .dock(PANEL_MIXER, FrameId::CENTER, Zone::Left)
        .expect("Mixer docks left of the Viewer");
    close_frame(&mut tree, FrameId::RIGHT);

    assert_eq!(tree.open(PANEL_EFFECTS), FrameId::RIGHT);
    tree.check_invariants()
        .expect("invariants after the column returns");
    assert_rect(
        rect_of(&tree, FrameId::RIGHT),
        rect_of(&shipped, FrameId::RIGHT),
        "Effects & Presets column",
    );
    assert_eq!(
        tree.find(PANEL_INFO),
        Some(below),
        "the split under the Viewer stays"
    );
    assert_eq!(
        tree.find(PANEL_MIXER),
        Some(beside),
        "the split beside the Viewer stays"
    );
}

#[test]
fn a_side_frame_beside_a_lone_centre_frame_splits_it_at_the_editing_share() {
    let (left, _, right) =
        DockTree::upper_row_weights_for_width(DockTree::DEFAULT_WINDOW_WIDTH, true);
    let upper = 1.0 - DockTree::EDITING_TIMELINE_SHARE;
    for (panel, id, want) in [
        (PANEL_PROJECT, FrameId::LEFT, [0.0, 0.0, left, upper]),
        (PANEL_INFO, FrameId::RIGHT, [1.0 - right, 0.0, right, upper]),
    ] {
        let mut tree = DockTree::editing();
        close_frame(&mut tree, FrameId::LEFT);
        close_frame(&mut tree, FrameId::RIGHT);
        assert!(
            matches!(&tree.root, Node::Split { axis: Axis::Column, children }
                if matches!(&children[0].1, Node::Frame(f) if f.id == FrameId::CENTER)),
            "the upper row collapsed to the centre frame: {:?}",
            tree.root
        );
        assert_eq!(tree.open(panel), id);
        tree.check_invariants().expect("invariants after the split");
        assert_rect(
            rect_of(&tree, id),
            want,
            &format!("{panel} beside the Viewer"),
        );
    }
}

#[test]
fn the_timeline_frame_without_audio_meters_docked_returns_as_a_lower_row() {
    let shipped = DockTree::editing();
    let mut tree = shipped.clone();
    close_frame(&mut tree, FrameId::METERS);
    close_frame(&mut tree, FrameId::BOTTOM);

    assert_eq!(tree.open(PANEL_TIMELINE), FrameId::BOTTOM);
    tree.check_invariants()
        .expect("invariants after the lower row returns");
    let upper = 1.0 - DockTree::EDITING_TIMELINE_SHARE;
    assert_rect(
        rect_of(&tree, FrameId::BOTTOM),
        [0.0, upper, 1.0, DockTree::EDITING_TIMELINE_SHARE],
        "Timeline row",
    );
    for id in [FrameId::LEFT, FrameId::CENTER, FrameId::RIGHT] {
        assert_rect(
            rect_of(&tree, id),
            rect_of(&shipped, id),
            &format!("frame {id}"),
        );
    }
}

fn open_meters(tree: &mut DockTree) -> FrameId {
    tree.open(PANEL_AUDIO_METERS)
}

/// The Window menu, the Shift+digit keys, and redocking call `open_in`.
fn open_meters_in_default(tree: &mut DockTree) -> FrameId {
    tree.open_in(PANEL_AUDIO_METERS, DefaultFrame::Bottom)
}

#[test]
fn audio_meters_reopen_right_of_the_timeline_and_never_over_it() {
    let shipped = DockTree::editing();
    let paths: [(&str, fn(&mut DockTree) -> FrameId); 2] =
        [("open", open_meters), ("open_in", open_meters_in_default)];
    for (path, open) in paths {
        let mut tree = shipped.clone();
        close_frame(&mut tree, FrameId::METERS);
        assert_eq!(open(&mut tree), FrameId::METERS, "{path}");
        tree.check_invariants()
            .expect("invariants after Audio Meters return");
        assert_eq!(
            tree.frame(FrameId::BOTTOM).expect("Timeline frame").tabs,
            [PANEL_TIMELINE]
        );
        assert_same_layout(&tree, &shipped, path);
    }
}

#[test]
fn audio_meters_then_the_timeline_reopen_into_the_shipped_lower_row() {
    let shipped = DockTree::editing();
    let mut tree = shipped.clone();
    close_frame(&mut tree, FrameId::METERS);
    close_frame(&mut tree, FrameId::BOTTOM);

    assert_eq!(open_meters_in_default(&mut tree), FrameId::METERS);
    let upper = 1.0 - DockTree::EDITING_TIMELINE_SHARE;
    assert_rect(
        rect_of(&tree, FrameId::METERS),
        [0.0, upper, 1.0, DockTree::EDITING_TIMELINE_SHARE],
        "Audio Meters row",
    );
    assert_eq!(tree.open(PANEL_TIMELINE), FrameId::BOTTOM);
    tree.check_invariants()
        .expect("invariants after both return");
    assert_same_layout(&tree, &shipped, "Audio Meters, then the Timeline");
}

#[test]
fn audio_meters_rejoin_a_meters_frame_that_still_holds_another_panel() {
    let mut tree = DockTree::editing();
    assert!(tree.add_tab(FrameId::METERS, PANEL_JOBS));
    assert!(tree.close(PANEL_AUDIO_METERS));
    assert_eq!(
        tree.frame(FrameId::METERS).expect("frame stays").tabs,
        [PANEL_JOBS]
    );

    assert_eq!(open_meters_in_default(&mut tree), FrameId::METERS);
    assert_eq!(
        tree.frame(FrameId::METERS).expect("frame stays").tabs,
        [PANEL_JOBS, PANEL_AUDIO_METERS]
    );
    assert!(tree.is_active(PANEL_AUDIO_METERS));
    tree.check_invariants().expect("no duplicate frame id");
}

#[test]
fn in_every_shipped_workspace_reopened_audio_meters_leave_the_timeline_visible() {
    for (what, base) in shipped_and_pane_trees() {
        let mut tree = base;
        if tree.is_open(PANEL_AUDIO_METERS) {
            assert!(tree.close(PANEL_AUDIO_METERS), "{what}");
        }
        assert_eq!(open_meters_in_default(&mut tree), FrameId::METERS, "{what}");
        tree.check_invariants()
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_ne!(tree.find(PANEL_TIMELINE), Some(FrameId::METERS), "{what}");
        assert!(
            tree.is_active(PANEL_TIMELINE),
            "{what}: the Timeline stays the visible tab"
        );
        let rect = rect_of(&tree, FrameId::METERS);
        assert!(
            Edge::Bottom.touched_by(rect),
            "{what}: Audio Meters at {rect:?}"
        );
    }
}

#[test]
fn with_no_anchor_frame_docked_a_panel_opens_in_the_largest_frame() {
    // The centre frame has no re-created position of its own.
    let mut tree = DockTree::editing();
    close_frame(&mut tree, FrameId::CENTER);
    let largest = tree.largest_frame();
    assert_eq!(tree.open(PANEL_VIEWER), largest);
    assert!(tree.frame(FrameId::CENTER).is_none());

    // A side column re-creates beside the centre frame only.
    let mut tree = DockTree::editing();
    close_frame(&mut tree, FrameId::CENTER);
    close_frame(&mut tree, FrameId::RIGHT);
    let largest = tree.largest_frame();
    assert_eq!(tree.open(PANEL_INFO), largest);
    assert!(tree.frame(FrameId::RIGHT).is_none());
    tree.check_invariants()
        .expect("invariants after the fallback");
}

#[test]
fn a_floating_frame_holding_a_standard_id_keeps_it_when_a_panel_opens() {
    let mut tree = DockTree::editing();
    tree.float(PANEL_EFFECTS, FloatRect::new(200.0, 200.0, 420.0, 360.0))
        .expect("Effects & Presets floats");
    // A tree loaded from disk may float a frame under a standard id, with a
    // docked frame focused.
    tree.floating[0].frame.id = FrameId::RIGHT;
    tree.focused = FrameId::CENTER;
    tree.check_invariants()
        .expect("a floating RIGHT is a valid tree");
    let largest = tree.largest_frame();

    assert_eq!(tree.open(PANEL_INFO), largest);
    assert_eq!(
        tree.floating[0].frame.id,
        FrameId::RIGHT,
        "the floating window keeps its frame"
    );
    tree.check_invariants().expect("no duplicate frame id");
}

#[test]
fn every_shipped_workspace_re_creates_a_closed_side_frame_at_its_edge() {
    for (form, base) in shipped_and_pane_trees() {
        for pref in DEFAULT_FRAMES {
            let Some(edge) = re_created(pref) else {
                continue;
            };
            let id = pref.frame();
            if base.frame(id).is_none() {
                continue;
            }
            let what = format!("{form} {pref:?}");
            let mut tree = base.clone();
            let panels = close_frame(&mut tree, id);
            let frames = tree.frame_ids().len();
            for &panel in &panels {
                assert_eq!(tree.open_in(panel, pref), id, "{what}: {panel}");
            }
            tree.check_invariants()
                .unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_eq!(
                tree.frame_ids().len(),
                frames + 1,
                "{what}: one frame returns"
            );
            let rect = rect_of(&tree, id);
            assert!(
                edge.touched_by(rect),
                "{what}: rect {rect:?} misses the {edge:?} edge"
            );
        }
    }
}

#[test]
fn reopening_the_inspector_alone_recreates_the_left_frame_with_it() {
    let mut tree = DockTree::editing();
    close_frame(&mut tree, FrameId::LEFT);
    assert_eq!(tree.open(PANEL_INSPECTOR), FrameId::LEFT);
    assert_eq!(
        tree.frame(FrameId::LEFT).expect("left frame").tabs,
        [PANEL_INSPECTOR]
    );
    assert!(tree.is_active(PANEL_INSPECTOR));
    assert_eq!(
        tree.find(PANEL_PROJECT),
        None,
        "only the opened panel returns"
    );
}
