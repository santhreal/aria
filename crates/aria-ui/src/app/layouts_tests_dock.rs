use super::*;

#[test]
fn animation_workspace_opens_timeline_graph_and_workspaces_scale_spec_px_at_1920_and_1280() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);

    assert!(!cx.update(|app| subject.read(app).timeline_graph_open()));
    assert!(apply(&subject, &cx, "win-workspace-animation"));
    assert!(
        cx.update(|app| subject.read(app).timeline_graph_open()),
        "Animation workspace must open the timeline graph editor"
    );

    for width in [1920.0_f32, 1280.0_f32] {
        cx.update(|app| {
            subject.update(app, |state: &mut AppState, cx| {
                state.observe_main_window_size(width, 900.0);
                assert!(state.apply_workspace("win-workspace-editing", cx));
                let crate::dock::Node::Split { children: rows, .. } = &state.dock_tree.root else {
                    panic!("expected root column split");
                };
                let crate::dock::Node::Split {
                    children: upper, ..
                } = &rows[0].1
                else {
                    panic!("expected upper row split");
                };
                assert!(
                    (upper[0].0 * width - 320.0).abs() < 0.5,
                    "left frame at {width}px was {}px",
                    upper[0].0 * width
                );
                assert!(
                    (upper[2].0 * width - 300.0).abs() < 0.5,
                    "right frame at {width}px was {}px",
                    upper[2].0 * width
                );
                let crate::dock::Node::Split {
                    children: lower, ..
                } = &rows[1].1
                else {
                    panic!("expected lower row split");
                };
                assert!(
                    (lower[1].0 * width - 64.0).abs() < 0.5,
                    "Audio Meters at {width}px was {}px",
                    lower[1].0 * width
                );
            });
        });
    }
}

/// A panel id nothing registers: a plugin that declared it and was then removed.
const UNREGISTERED: crate::dock::PanelId = "aria.panel.unregistered-test";

#[test]
fn unregistered_panels_are_filtered_and_all_unregistered_frame_is_omitted_from_drawn_layout() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    cx.update(|app| {
        let state = subject.read(app);
        assert!(state.is_dock_panel_registered(crate::dock::PANEL_EFFECTS));
        assert!(state.is_dock_panel_registered(crate::dock::PANEL_AUDIO_METERS));
        assert!(state.is_dock_panel_registered(crate::dock::PANEL_CHARACTER));
        assert!(state.is_dock_panel_registered(crate::dock::PANEL_PARAGRAPH));
        assert!(state.is_dock_panel_registered(crate::dock::PANEL_JOBS));
        assert!(!state.is_dock_panel_registered(UNREGISTERED));

        let right = state
            .dock_tree
            .frame(crate::dock::FrameId::RIGHT)
            .expect("RIGHT frame");
        assert_eq!(right.tabs, vec![crate::dock::PANEL_EFFECTS]);
        assert!(state.node_has_registered_panel(&crate::dock::Node::Frame(right.clone())));

        let only_unregistered = crate::dock::Node::Frame(crate::dock::Frame::new(
            crate::dock::FrameId(99),
            vec![UNREGISTERED],
        ));
        assert!(
            !state.node_has_registered_panel(&only_unregistered),
            "a frame whose every tab is unregistered must be omitted from the drawn layout"
        );
    });
}

#[test]
fn restore_split_weights_restores_workspace_saved_ratio_and_min_sizes_clamp_drag_and_resize() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    assert!(apply(&subject, &cx, "win-workspace-editing"));

    cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            assert!(state.dock_tree.drag_split_at(&[], 0, 0.20));
            assert!(state.restore_split_weights(&[], cx));
            let crate::dock::Node::Split { children, .. } = &state.dock_tree.root else {
                panic!("expected root split");
            };
            assert!((children[0].0 - 0.55).abs() < 1e-4);
            assert!((children[1].0 - 0.45).abs() < 1e-4);

            // Dragging upper row splitter 0 left cannot shrink the Left frame below 160 px at 1000 px width.
            assert!(state.drag_split_clamped(&[0], 0, -0.50, 1000.0));
            let crate::dock::Node::Split { children: rows, .. } = &state.dock_tree.root else {
                panic!("expected root split");
            };
            let crate::dock::Node::Split {
                children: upper, ..
            } = &rows[0].1
            else {
                panic!("expected upper row split");
            };
            assert!(
                upper[0].0 * 1000.0 >= 160.0 - 0.5,
                "Left frame shrank below 160px minimum: {}",
                upper[0].0 * 1000.0
            );

            // Window resize to 840 px clamps Left and Right frames up to >= 160 px and takes delta from Viewer slack.
            state.dock_tree = crate::dock::DockTree::editing_for_width(1920.0);
            assert!(state.rebalance_splits_for_window(840.0, 600.0));
            let crate::dock::Node::Split {
                children: rows2, ..
            } = &state.dock_tree.root
            else {
                panic!("expected root split");
            };
            let crate::dock::Node::Split {
                children: upper2, ..
            } = &rows2[0].1
            else {
                panic!("expected upper row split");
            };
            assert!(upper2[0].0 * 840.0 >= 160.0 - 0.5);
            assert!(upper2[2].0 * 840.0 >= 160.0 - 0.5);
            assert!(upper2[1].0 * 840.0 >= 320.0 - 0.5);
        });
    });
}

#[test]
fn float_open_failure_redocks_frame_and_save_workspace_menu_prompts_for_name() {
    let scratch = dir();
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&scratch), &cx);

    cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            let fid = state
                .float_dock_panel(crate::dock::PANEL_MIXER, (80.0, 80.0, 480.0, 320.0), cx)
                .expect("float mixer");
            assert_eq!(state.dock_tree.floating.len(), 1);
            assert!(state.handle_float_open_failure(fid, "display server refused window", cx));
            assert!(state.dock_tree.floating.is_empty());
            assert!(state.dock_tree.is_open(crate::dock::PANEL_MIXER));
            assert!(
                state
                    .last_error
                    .as_deref()
                    .is_some_and(|msg| msg.contains("display server refused window"))
            );

            // Window > Workspaces > Save as New Workspace opens RenameTarget::SaveWorkspace,
            // and committing the typed name writes `<data dir>/workspaces/<name>.json`.
            assert!(crate::chrome::menu_items::run_item(
                AppMenu::Window,
                layouts::SAVE_ID,
                state,
                cx,
            ));
            assert_eq!(
                state.rename,
                Some(crate::types::RenameTarget::SaveWorkspace)
            );
            for ch in "Evening Cut".chars() {
                state.rename_push(ch);
            }
            state.commit_rename().expect("commit workspace name");
        });
    });
    assert!(scratch.join("workspaces/Evening Cut.json").exists());
}

#[test]
fn six_deep_nested_splitter_and_half_window_nested_split_span_and_window_placement() {
    use crate::dock::{
        Axis, DOCK_VERSION, DockTree, Frame, FrameId, Node, PANEL_ASSISTANT, PANEL_AUDIO_METERS,
        PANEL_EFFECTS, PANEL_INSPECTOR, PANEL_MIXER, PANEL_PROJECT, PANEL_TIMELINE, PANEL_VIEWER,
    };
    use crate::settings::{UiSettings, WindowPlacement};

    // Build a DockTree nested 6 splits deep: path `[0, 1, 0, 1, 0, 1]`.
    let deep_split = Node::Split {
        axis: Axis::Row,
        children: vec![
            (
                0.5,
                Node::Frame(Frame::new(FrameId(10), vec![PANEL_PROJECT])),
            ),
            (
                0.5,
                Node::Frame(Frame::new(FrameId(11), vec![PANEL_EFFECTS])),
            ),
        ],
    };
    let d5 = Node::Split {
        axis: Axis::Column,
        children: vec![
            (
                0.5,
                Node::Frame(Frame::new(FrameId(9), vec![PANEL_INSPECTOR])),
            ),
            (0.5, deep_split),
        ],
    };
    let d4 = Node::Split {
        axis: Axis::Row,
        children: vec![
            (0.5, d5),
            (
                0.5,
                Node::Frame(Frame::new(FrameId(8), vec![PANEL_ASSISTANT])),
            ),
        ],
    };
    let d3 = Node::Split {
        axis: Axis::Column,
        children: vec![
            (0.5, Node::Frame(Frame::new(FrameId(7), vec![PANEL_MIXER]))),
            (0.5, d4),
        ],
    };
    let d2 = Node::Split {
        axis: Axis::Row,
        children: vec![
            (0.5, d3),
            (
                0.5,
                Node::Frame(Frame::new(FrameId(6), vec![PANEL_AUDIO_METERS])),
            ),
        ],
    };
    let d1 = Node::Split {
        axis: Axis::Column,
        children: vec![
            (0.5, Node::Frame(Frame::new(FrameId(5), vec![PANEL_VIEWER]))),
            (0.5, d2),
        ],
    };
    let root = Node::Split {
        axis: Axis::Row,
        children: vec![
            (0.5, d1),
            (
                0.5,
                Node::Frame(Frame::new(FrameId(4), vec![PANEL_TIMELINE])),
            ),
        ],
    };

    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    cx.update(|app| {
        subject.update(app, |state: &mut AppState, _| {
            state.dock_tree = DockTree {
                version: DOCK_VERSION,
                root,
                floating: Vec::new(),
                maximized: None,
                focused: FrameId(10),
            };
            let deep_path = [0usize, 1, 0, 1, 0, 1];
            crate::shell::dock_view::set_active_split_drag_path(&deep_path);
            let recovered = crate::shell::dock_view::active_split_drag_path(&deep_path[..4]);
            assert_eq!(recovered, deep_path);

            // Drag the 6-deep splitter: only the 6-deep split's weights change;
            // ancestor splits at depth <= 4 remain untouched at [0.5, 0.5].
            assert!(state.drag_split_clamped(&recovered, 0, 0.15, 1000.0));
            assert_eq!(
                state.dock_tree.split_weights(&[0, 1, 0, 1]),
                Some(vec![0.5, 0.5])
            );
            let leaf_w = state
                .dock_tree
                .split_weights(&deep_path)
                .expect("6-deep weights");
            assert!(
                (leaf_w[0] - 0.65).abs() < 1e-4,
                "6-deep weight[0]={}",
                leaf_w[0]
            );
            assert!(
                (leaf_w[1] - 0.35).abs() < 1e-4,
                "6-deep weight[1]={}",
                leaf_w[1]
            );

            // Defect 2: a nested Row split that occupies half the window (`[0]` has weight 0.5
            // in a Row root of width 1200 px -> container is 600 px wide). Dragging its
            // splitter by N = 60 px moves the boundary by N = 60 px (weight delta = 60 / 600 = 0.10).
            state.dock_tree = DockTree {
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
                                        Node::Frame(Frame::new(
                                            FrameId::RIGHT,
                                            vec![PANEL_EFFECTS],
                                        )),
                                    ),
                                ],
                            },
                        ),
                        (
                            0.5,
                            Node::Frame(Frame::new(FrameId::CENTER, vec![PANEL_VIEWER])),
                        ),
                    ],
                },
                floating: Vec::new(),
                maximized: None,
                focused: FrameId::LEFT,
            };
            assert!((state.dock_tree.split_span_fraction(&[0], Axis::Row) - 0.5).abs() < 1e-5);
            assert!(state.drag_split_by_pixels(&[0], 0, 60.0, 1200.0));
            let nested_w = state.dock_tree.split_weights(&[0]).expect("nested weights");
            let boundary_px_before = 0.5 * 600.0;
            let boundary_px_after = nested_w[0] * 600.0;
            assert!(
                ((boundary_px_after - boundary_px_before) - 60.0).abs() < 0.5,
                "dragging by 60 px in a half-window split must move boundary by 60 px, got {}",
                boundary_px_after - boundary_px_before
            );
        });
    });

    // WindowPlacement per display arrangement (`docs/spec/layout.md:24-26`):
    // First launch opens maximized on primary display; saved placement restores
    // size, position, display, and maximized state per display arrangement.
    let mut settings = UiSettings::default();
    let dual = [[0.0, 0.0, 1920.0, 1080.0], [1920.0, 0.0, 2560.0, 1440.0]];
    let first = settings.resolve_window_placement(&dual);
    assert!(first.maximized);
    assert_eq!(first.display_index, 0);

    settings.record_window_placement(
        &dual,
        WindowPlacement {
            x: 2040.0,
            y: 120.0,
            width: 1600.0,
            height: 960.0,
            maximized: false,
            display_index: 1,
        },
    );
    let restored = settings.resolve_window_placement(&dual);
    assert_eq!(restored.display_index, 1);
    assert!(!restored.maximized);
    assert_eq!(
        (restored.x, restored.y, restored.width, restored.height),
        (2040.0, 120.0, 1600.0, 960.0)
    );

    // Different display arrangement (single monitor) opens maximized on primary display
    // while preserving the dual-monitor arrangement's saved entry.
    let single = [[0.0, 0.0, 1920.0, 1080.0]];
    let single_resolved = settings.resolve_window_placement(&single);
    assert!(single_resolved.maximized);
    assert_eq!(single_resolved.display_index, 0);
    assert_eq!(settings.resolve_window_placement(&dual).display_index, 1);
}
