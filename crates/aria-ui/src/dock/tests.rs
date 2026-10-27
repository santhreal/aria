//! Unit and invariant tests for [`super::DockTree`].
//!
//! Closes the class of dock tree operations that leave an empty frame, a
//! single-child split, a nested split along the same axis, a non-positive split
//! weight, a duplicated panel, an out-of-bounds active tab index, or a stale
//! maximized frame id. Does not cover GPUI window layout or pointer hit-testing.

use super::*;

const SAMPLE_PANELS: &[PanelId] = &[
    PANEL_PROJECT,
    PANEL_VIEWER,
    PANEL_TIMELINE,
    PANEL_INSPECTOR,
    PANEL_MIXER,
    PANEL_ASSISTANT,
    PANEL_SCOPES,
    PANEL_COLOR,
    PANEL_INFO,
    PANEL_EFFECTS,
    PANEL_AUDIO_METERS,
    PANEL_JOBS,
];

#[test]
fn shipped_editing_tree_satisfies_invariants_and_reading_order() {
    let tree = DockTree::editing();
    tree.check_invariants().expect("editing tree invariants");
    assert_eq!(
        tree.frame_ids(),
        vec![
            FrameId::LEFT,
            FrameId::CENTER,
            FrameId::RIGHT,
            FrameId::BOTTOM,
            FrameId::METERS,
        ]
    );
    assert_eq!(
        tree.frame(FrameId::LEFT).map(|f| f.tabs.as_slice()),
        Some(&[PANEL_PROJECT, PANEL_INSPECTOR][..])
    );
    assert_eq!(
        tree.frame(FrameId::CENTER).map(|f| f.tabs.as_slice()),
        Some(&[PANEL_VIEWER][..])
    );
    assert_eq!(
        tree.frame(FrameId::RIGHT).map(|f| f.tabs.as_slice()),
        Some(&[PANEL_EFFECTS][..])
    );
    assert_eq!(
        tree.frame(FrameId::BOTTOM).map(|f| f.tabs.as_slice()),
        Some(&[PANEL_TIMELINE][..])
    );
    assert_eq!(
        tree.frame(FrameId::METERS).map(|f| f.tabs.as_slice()),
        Some(&[PANEL_AUDIO_METERS][..])
    );
    assert_eq!(tree.split_weights(&[]), Some(vec![0.55, 0.45]));
    // Six panels at first glance; every other one opens from the Window menu.
    let open: usize = tree
        .frame_ids()
        .iter()
        .filter_map(|&id| tree.frame(id))
        .map(|frame| frame.tabs.len())
        .sum();
    assert_eq!(open, 6, "the Editing workspace opens with six panels");
    for closed in [PANEL_MIXER, PANEL_ASSISTANT, PANEL_INFO, PANEL_CHARACTER] {
        assert_eq!(tree.find(closed), None, "{closed} is open at first glance");
    }
    assert!(tree.is_active(PANEL_PROJECT));
    assert!(tree.is_active(PANEL_VIEWER));
    assert!(tree.is_active(PANEL_EFFECTS));
    assert!(tree.is_active(PANEL_TIMELINE));
    assert!(tree.is_active(PANEL_AUDIO_METERS));
    assert_eq!(tree.largest_frame(), FrameId::BOTTOM);

    // At both 1920 px and 1280 px, the upper row draws the left and right
    // frames at their pixel widths, and the lower row draws Audio Meters at
    // its own.
    for w in [1920.0f32, 1280.0f32] {
        let sized = DockTree::editing_for_width(w);
        let (upper, lower) = (DockTree::frame_pool(w, 3), DockTree::frame_pool(w, 2));
        let upper_weights = sized.split_weights(&[0]).expect("upper row");
        assert!((upper_weights[0] * upper - DockTree::EDITING_LEFT_PX).abs() < 0.5);
        assert!((upper_weights[2] * upper - DockTree::EDITING_RIGHT_PX).abs() < 0.5);
        let lower_weights = sized.split_weights(&[1]).expect("lower row");
        assert!((lower_weights[1] * lower - DockTree::EDITING_METERS_PX).abs() < 0.5);
    }
}

#[test]
fn dock_to_each_zone_produces_expected_tree_and_flattens_same_axis() {
    for zone in Zone::ALL {
        let mut tree = DockTree::editing();
        let target = FrameId::RIGHT;
        let resulting_frame = tree
            .dock(PANEL_MIXER, target, zone)
            .expect("dock into right frame");
        tree.check_invariants()
            .unwrap_or_else(|err| panic!("zone {zone:?} broke invariants: {err}"));
        assert_eq!(tree.find(PANEL_MIXER), Some(resulting_frame));
        assert!(tree.is_active(PANEL_MIXER));

        match zone {
            Zone::Center => {
                assert_eq!(resulting_frame, target);
                let frame = tree.frame(target).expect("target frame");
                assert_eq!(frame.tabs, vec![PANEL_EFFECTS, PANEL_MIXER]);
                assert_eq!(frame.active, 1);
            }
            Zone::Left | Zone::Right => {
                assert_ne!(resulting_frame, target);
                // The upper split is `Axis::Row`, so splitting `RIGHT`
                // horizontally flattens into the upper row rather than nesting
                // a `Row` inside a `Row`.
                let Some(Node::Split {
                    axis: Axis::Row,
                    children: row,
                }) = tree.node_at(&[0])
                else {
                    panic!("upper node is a row split");
                };
                assert_eq!(row.len(), 4, "same-axis split flattened into upper row");
                let last_two = [
                    match &row[2].1 {
                        Node::Frame(f) => f.id,
                        other => panic!("expected frame at slot 2, got {other:?}"),
                    },
                    match &row[3].1 {
                        Node::Frame(f) => f.id,
                        other => panic!("expected frame at slot 3, got {other:?}"),
                    },
                ];
                if zone == Zone::Left {
                    assert_eq!(last_two, [resulting_frame, target]);
                } else {
                    assert_eq!(last_two, [target, resulting_frame]);
                }
            }
            Zone::Top | Zone::Bottom => {
                assert_ne!(resulting_frame, target);
                // Splitting a frame in `Axis::Row` along `Top` or `Bottom`
                // creates a nested `Axis::Column` split at the third slot of the
                // upper row.
                let Some(Node::Split {
                    axis: Axis::Row,
                    children: row,
                }) = tree.node_at(&[0])
                else {
                    panic!("upper node is a row split");
                };
                assert_eq!(row.len(), 3);
                let Node::Split {
                    axis: Axis::Column,
                    children: right_col,
                } = &row[2].1
                else {
                    panic!("third upper slot is now a vertical split");
                };
                let pair: Vec<FrameId> = right_col
                    .iter()
                    .map(|(_, node)| match node {
                        Node::Frame(f) => f.id,
                        other => panic!("expected frame, got {other:?}"),
                    })
                    .collect();
                if zone == Zone::Top {
                    assert_eq!(pair, vec![resulting_frame, target]);
                } else {
                    assert_eq!(pair, vec![target, resulting_frame]);
                }
            }
        }
    }
}

#[test]
fn closing_last_tab_removes_frame_collapses_split_and_clears_maximized() {
    let mut tree = DockTree::editing();
    tree.toggle_maximize(FrameId::BOTTOM);
    assert_eq!(tree.maximized, Some(FrameId::BOTTOM));

    // Close the Timeline, which is the only tab in `FrameId::BOTTOM`.
    // The lower `Axis::Row` split now has a single child (`FrameId::METERS`),
    // so the lower split collapses directly to `FrameId::METERS` and
    // `maximized` clears.
    assert!(tree.close(PANEL_TIMELINE));
    tree.check_invariants().expect("invariants after close");
    assert_eq!(tree.find(PANEL_TIMELINE), None);
    assert_eq!(tree.maximized, None);
    assert!(tree.close(PANEL_AUDIO_METERS));
    tree.check_invariants()
        .expect("invariants after closing lower row");

    // Close Left and Right frames too; the root collapses to the single Center
    // frame, whose last tab refuses to close so the root never becomes empty.
    assert!(tree.close(PANEL_PROJECT));
    assert!(tree.close(PANEL_INSPECTOR));
    assert!(tree.close(PANEL_EFFECTS));
    tree.check_invariants()
        .expect("invariants after collapsing to 1 frame");
    assert!(
        matches!(&tree.root, Node::Frame(f) if f.id == FrameId::CENTER),
        "root collapsed to single frame: {:?}",
        tree.root
    );
    assert!(!tree.close(PANEL_VIEWER), "last panel in root cannot close");
    tree.check_invariants().expect("invariants with sole panel");
}

#[test]
fn open_places_a_closed_panel_into_its_default_frame() {
    let mut tree = DockTree::editing();
    // `PANEL_JOBS` defaults to `Right` (`FrameId::RIGHT`), which exists. A
    // closed default frame is re-created: `dock/restore_tests.rs`.
    let placed = tree.open(PANEL_JOBS);
    assert_eq!(placed, FrameId::RIGHT);
    assert_eq!(tree.find(PANEL_JOBS), Some(FrameId::RIGHT));
    assert!(tree.is_active(PANEL_JOBS));
    tree.check_invariants().expect("invariants after open");
}

#[test]
fn reorder_cycle_float_and_weights_preserve_invariants() {
    let mut tree = DockTree::editing();
    // Mixer and Assistant open into the Viewer's frame, their default.
    assert_eq!(tree.open(PANEL_MIXER), FrameId::CENTER);
    assert_eq!(tree.open(PANEL_ASSISTANT), FrameId::CENTER);
    tree.activate(PANEL_MIXER);
    assert!(tree.is_active(PANEL_MIXER));

    // Reorder Center tabs `[viewer, mixer, assistant]` -> `[mixer, assistant, viewer]`,
    // keeping `PANEL_MIXER` active at its new index 0.
    assert!(tree.reorder(FrameId::CENTER, 0, 2));
    tree.check_invariants().expect("invariants after reorder");
    let center = tree.frame(FrameId::CENTER).expect("center");
    assert_eq!(
        center.tabs,
        vec![PANEL_MIXER, PANEL_ASSISTANT, PANEL_VIEWER]
    );
    assert_eq!(center.active, 0);
    assert!(tree.is_active(PANEL_MIXER));

    // Undock `PANEL_MIXER` into a floating window, then dock it back.
    let float_id = tree
        .float(PANEL_MIXER, (120.0, 90.0, 640.0, 360.0))
        .expect("floated mixer");
    tree.check_invariants().expect("invariants after float");
    assert_eq!(tree.floating.len(), 1);
    assert_eq!(tree.find(PANEL_MIXER), Some(float_id));

    // Cycling frames visits root frames in reading order, then the floating frame.
    assert_eq!(tree.next_frame_from(FrameId::LEFT), Some(FrameId::CENTER));
    assert_eq!(tree.next_frame_from(FrameId::CENTER), Some(FrameId::RIGHT));
    assert_eq!(tree.next_frame_from(FrameId::RIGHT), Some(FrameId::BOTTOM));
    assert_eq!(tree.next_frame_from(FrameId::BOTTOM), Some(FrameId::METERS));
    assert_eq!(tree.next_frame_from(FrameId::METERS), Some(float_id));
    assert_eq!(tree.next_frame_from(float_id), Some(FrameId::LEFT));
    assert_eq!(tree.prev_frame_from(FrameId::LEFT), Some(float_id));
    tree.focused = FrameId::LEFT;
    assert_eq!(tree.cycle_focus(1), FrameId::CENTER);
    assert_eq!(tree.cycle_focus(-1), FrameId::LEFT);

    // Docking the floating panel back removes its empty floating window.
    assert_eq!(
        tree.dock(PANEL_MIXER, FrameId::CENTER, Zone::Center),
        Some(FrameId::CENTER)
    );
    assert!(tree.floating.is_empty());
    tree.check_invariants().expect("invariants after re-dock");

    // Setting weights clamps non-positive weights and rejects NaN or mismatched lengths.
    assert!(!tree.set_weights(&[], &[0.5]));
    assert!(!tree.set_weights(&[], &[f32::NAN, 0.5]));
    assert!(tree.set_weights(&[], &[-5.0, 0.8]));
    tree.check_invariants()
        .expect("invariants after clamping weights");

    // `restore_split_from` restores the saved workspace ratio instead of 50/50.
    let saved = DockTree::editing();
    assert!(tree.restore_split_from(&[], &saved));
    assert_eq!(tree.split_weights(&[]), Some(vec![0.55, 0.45]));

    // For an ad-hoc split not present in `saved`, `restore_split_from` falls
    // back to equal weights.
    tree.dock(PANEL_MIXER, FrameId::CENTER, Zone::Bottom);
    assert!(tree.set_weights(&[0, 1], &[0.8, 0.2]));
    assert!(tree.restore_split_from(&[0, 1], &saved));
    assert_eq!(tree.split_weights(&[0, 1]), Some(vec![0.5, 0.5]));
}

#[test]
fn serde_round_trip_and_stale_version_rejected() {
    let mut tree = DockTree::editing();
    tree.float(PANEL_ASSISTANT, (200.0, 140.0, 500.0, 360.0));
    tree.toggle_maximize(FrameId::CENTER);

    let json = tree.to_json().expect("serialize dock tree");
    let loaded = DockTree::from_json(&json).expect("deserialize dock tree");
    assert_eq!(loaded, tree);
    loaded.check_invariants().expect("loaded invariants");

    // A stale v1 or v0 tree or a future version is rejected by `from_json` and
    // falls back to `DockTree::editing()` through `parse_or_editing`.
    for bad_ver in [0u32, 1, 99] {
        let stale = json.replace(
            &format!("\"version\": {DOCK_VERSION}"),
            &format!("\"version\": {bad_ver}"),
        );
        let err = DockTree::from_json(&stale).expect_err("wrong version must be rejected");
        assert!(err.contains("version"), "unexpected error: {err}");
        assert_eq!(DockTree::parse_or_editing(&stale), DockTree::editing());
    }
    assert_eq!(DockTree::parse_or_editing("{not json"), DockTree::editing());
}

#[test]
fn randomized_operation_sequences_preserve_every_invariant() {
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next_u32 = || -> u32 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 16) as u32
    };

    for trial in 0..32 {
        let mut tree = DockTree::editing();
        for step in 0..200 {
            let pick_panel = SAMPLE_PANELS[(next_u32() as usize) % SAMPLE_PANELS.len()];
            let frames = tree.frame_ids();
            let pick_frame = frames[(next_u32() as usize) % frames.len()];
            match next_u32() % 10 {
                0 => {
                    tree.open(pick_panel);
                }
                1 => {
                    tree.close(pick_panel);
                }
                2 => {
                    tree.activate(pick_panel);
                }
                3 => {
                    let zone = Zone::ALL[(next_u32() as usize) % Zone::ALL.len()];
                    tree.dock(pick_panel, pick_frame, zone);
                }
                4 => {
                    let x = (next_u32() % 800) as f32;
                    let y = (next_u32() % 600) as f32;
                    let w = (next_u32() % 600) as f32;
                    let h = (next_u32() % 400) as f32;
                    tree.float(pick_panel, (x, y, w, h));
                }
                5 => {
                    let from = (next_u32() as usize) % 5;
                    let to = (next_u32() as usize) % 5;
                    tree.reorder(pick_frame, from, to);
                }
                6 => {
                    tree.toggle_maximize(pick_frame);
                }
                7 => {
                    let a = ((next_u32() % 100) as f32) - 20.0;
                    let b = ((next_u32() % 100) as f32) - 20.0;
                    let c = ((next_u32() % 100) as f32) - 20.0;
                    if !tree.set_weights(&[], &[a, b]) {
                        tree.set_weights(&[], &[a, b, c]);
                    }
                }
                8 => {
                    tree.close_other_tabs(pick_frame, pick_panel);
                }
                _ => {
                    let next = tree.next_frame();
                    assert!(tree.frame(next).is_some());
                    let prev = tree.prev_frame();
                    assert!(tree.frame(prev).is_some());
                }
            }
            tree.check_invariants().unwrap_or_else(|err| {
                panic!("trial {trial} step {step} violated invariant: {err}\ntree: {tree:#?}")
            });
        }
    }
}

#[test]
fn each_invariant_violation_is_detected_and_repaired_by_normalize() {
    // 1. Empty frame in a split.
    let mut empty_frame = DockTree::editing();
    if let Node::Split { children, .. } = &mut empty_frame.root {
        children.push((
            0.1,
            Node::Frame(Frame {
                id: FrameId(77),
                tabs: Vec::new(),
                active: 0,
            }),
        ));
    }
    assert!(empty_frame.check_invariants().is_err());
    empty_frame.normalize();
    empty_frame
        .check_invariants()
        .expect("repaired empty frame");

    // 2. Single-child split.
    let mut single_child = DockTree {
        version: DOCK_VERSION,
        root: Node::Split {
            axis: Axis::Row,
            children: vec![(
                1.0,
                Node::Frame(Frame::new(FrameId::CENTER, vec![PANEL_VIEWER])),
            )],
        },
        floating: Vec::new(),
        focused: FrameId::CENTER,
        maximized: None,
    };
    assert!(single_child.check_invariants().is_err());
    single_child.normalize();
    single_child
        .check_invariants()
        .expect("repaired single-child split");
    assert!(matches!(single_child.root, Node::Frame(_)));

    // 3. Same-axis nested split.
    let mut nested_same_axis = DockTree {
        version: DOCK_VERSION,
        root: Node::Split {
            axis: Axis::Row,
            children: vec![
                (
                    0.5,
                    Node::Split {
                        axis: Axis::Row,
                        children: vec![
                            (
                                0.5,
                                Node::Frame(Frame::new(FrameId::LEFT, vec![PANEL_PROJECT])),
                            ),
                            (
                                0.5,
                                Node::Frame(Frame::new(FrameId::CENTER, vec![PANEL_VIEWER])),
                            ),
                        ],
                    },
                ),
                (
                    0.5,
                    Node::Frame(Frame::new(FrameId::RIGHT, vec![PANEL_INSPECTOR])),
                ),
            ],
        },
        floating: Vec::new(),
        focused: FrameId::CENTER,
        maximized: None,
    };
    assert!(nested_same_axis.check_invariants().is_err());
    nested_same_axis.normalize();
    nested_same_axis
        .check_invariants()
        .expect("repaired nested same-axis split");

    // 4. Non-positive split weight.
    let mut bad_weight = DockTree::editing();
    if let Node::Split { children, .. } = &mut bad_weight.root {
        children[0].0 = -0.5;
    }
    assert!(bad_weight.check_invariants().is_err());
    bad_weight.normalize();
    bad_weight
        .check_invariants()
        .expect("repaired non-positive split weight");

    // 5. Duplicate panel across two frames.
    let mut dup_panel = DockTree::editing();
    if let Some(left) = dup_panel.frame_mut(FrameId::LEFT) {
        left.tabs.push(PANEL_VIEWER);
    }
    assert!(dup_panel.check_invariants().is_err());
    dup_panel.normalize();
    dup_panel
        .check_invariants()
        .expect("repaired duplicate panel");

    // 6. Duplicate FrameId across two frames.
    let mut dup_frame_id = DockTree::editing();
    if let Some(right) = dup_frame_id.frame_mut(FrameId::RIGHT) {
        right.id = FrameId::LEFT;
    }
    assert!(dup_frame_id.check_invariants().is_err());
    dup_frame_id.normalize();
    dup_frame_id
        .check_invariants()
        .expect("repaired duplicate FrameId");

    // 7. Out-of-bounds active tab index.
    let mut bad_active = DockTree::editing();
    if let Some(center) = bad_active.frame_mut(FrameId::CENTER) {
        center.active = 99;
    }
    assert!(bad_active.check_invariants().is_err());
    bad_active.normalize();
    bad_active
        .check_invariants()
        .expect("repaired out-of-bounds active index");

    // 8. Stale maximized FrameId not in root.
    let mut bad_maximized = DockTree::editing();
    bad_maximized.maximized = Some(FrameId(999));
    assert!(bad_maximized.check_invariants().is_err());
    bad_maximized.normalize();
    bad_maximized
        .check_invariants()
        .expect("repaired stale maximized FrameId");
    assert_eq!(bad_maximized.maximized, None);

    // 9. Non-positive floating window dimensions.
    let mut bad_float = DockTree::editing();
    bad_float.floating.push(Floating {
        frame: Frame::new(FrameId(50), vec![PANEL_SCOPES]),
        x: 10.0,
        y: 10.0,
        w: -50.0,
        h: 0.0,
    });
    assert!(bad_float.check_invariants().is_err());
    bad_float.normalize();
    bad_float
        .check_invariants()
        .expect("repaired invalid floating dimensions");

    // 10. Stale focused FrameId not in tree.
    let mut bad_focused = DockTree::editing();
    bad_focused.focused = FrameId(999);
    assert!(bad_focused.check_invariants().is_err());
    bad_focused.normalize();
    bad_focused
        .check_invariants()
        .expect("repaired stale focused FrameId");
}
