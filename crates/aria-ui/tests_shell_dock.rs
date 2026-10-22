use gpui::{Modifiers, MouseButton, point, px};

use super::*;

/// Frame maximize on `` ` ``, frame cycling on `Ctrl+Tab` / `Ctrl+Shift+Tab`,
/// and closing the active tab on `Ctrl+W` all run through the shell's key
/// dispatcher and draw without panicking.
#[gpui::test]
fn interaction_dock_keys_maximize_cycle_and_close_tabs(cx: &mut gpui::TestAppContext) {
    use aria_ui::dock::{FrameId, PANEL_ASSISTANT, PANEL_MIXER, PANEL_VIEWER};

    let (state, visual) = editing!(cx, "dock-keys");

    // Shift+6 activates Audio Track Mixer in the centre frame.
    visual.simulate_keystrokes("shift-6");
    visual.run_until_parked();
    assert!(
        visual.read(|app| state.read(app).dock_tree.is_active(PANEL_MIXER)),
        "Shift+6 activates the Mixer tab"
    );
    assert_eq!(
        visual.read(|app| state.read(app).dock_tree.focused),
        FrameId::CENTER
    );

    // Backquote maximizes the focused frame; a second backquote restores.
    visual.simulate_keystrokes("`");
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).dock_tree.maximized),
        Some(FrameId::CENTER),
        "backquote maximizes the focused frame"
    );
    visual.simulate_keystrokes("`");
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).dock_tree.maximized),
        None,
        "second backquote restores the split tree"
    );

    // Ctrl+W closes the active Mixer tab and falls back to Viewer.
    visual.simulate_keystrokes("ctrl-w");
    visual.run_until_parked();
    assert!(
        !visual.read(|app| state.read(app).dock_tree.is_open(PANEL_MIXER)),
        "Ctrl+W closes the active Mixer tab"
    );
    assert!(
        visual.read(|app| state.read(app).dock_tree.is_active(PANEL_VIEWER))
            || visual.read(|app| state.read(app).dock_tree.is_active(PANEL_ASSISTANT)),
        "another tab in the centre frame becomes active"
    );

    // Ctrl+Tab and Ctrl+Shift+Tab cycle `focused` across open frames in reading order.
    let before = visual.read(|app| state.read(app).dock_tree.focused);
    visual.simulate_keystrokes("ctrl-tab");
    visual.run_until_parked();
    let next = visual.read(|app| state.read(app).dock_tree.focused);
    assert_ne!(next, before, "Ctrl+Tab advances to the next frame");
    visual.simulate_keystrokes("ctrl-shift-tab");
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).dock_tree.focused),
        before,
        "Ctrl+Shift+Tab returns to the previous frame"
    );
}

/// Dragging a tab onto another frame's drop zone docks it into a split, and
/// floating a panel opens a second GPUI window sharing `AppState`.
#[gpui::test]
fn interaction_dock_tab_drag_splits_frame_and_floating_window_shares_state(
    cx: &mut gpui::TestAppContext,
) {
    use aria_ui::dock::{FrameId, PANEL_ASSISTANT, PANEL_INSPECTOR, PANEL_MIXER, Zone};

    let (state, visual) = editing!(cx, "dock-drag-float");

    let center_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::CENTER))
        .expect("centre frame records its painted bounds");
    let right_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::RIGHT))
        .expect("right frame records its painted bounds");
    assert!(center_rect[2] > 0.0 && center_rect[3] > 0.0);
    assert!(right_rect[2] > 0.0 && right_rect[3] > 0.0);

    // Dock Assistant into the bottom half of the right frame and redraw.
    edit(&state, visual, |state| {
        assert!(
            state
                .dock_tree
                .dock(PANEL_ASSISTANT, FrameId::RIGHT, Zone::Bottom)
                .is_some()
        );
    });
    assert_eq!(
        visual.read(|app| state.read(app).dock_tree.root_frames().len()),
        6,
        "docking to Bottom splits the right frame into two frames (5 -> 6 root frames)"
    );
    assert!(
        visual.read(|app| state.read(app).dock_tree.check_invariants().is_ok()),
        "dock tree invariants hold after split"
    );

    // Float Mixer into a detached frame and open a second window with Shell::new_floating
    // sharing the same AppState entity.
    let float_id = visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .float_dock_panel(PANEL_MIXER, (100.0, 100.0, 480.0, 320.0), cx)
                .expect("float mixer")
        })
    });
    let shared = gpui::Entity::clone(&state);
    let (_float_shell, float_visual) =
        window::window_view(cx, |_, cx| Shell::new_floating(shared, float_id, cx));
    float_visual.run_until_parked();
    assert_eq!(
        float_visual.read(|app| state.read(app).dock_tree.floating.len()),
        1,
        "floating window shares AppState and sees the floating frame"
    );

    // Redocking from the shared state returns the tab to the main tree.
    float_visual.update(|_, app| {
        state.update(app, |state, cx| {
            assert!(state.redock_floating_frame(float_id, cx));
            assert!(state.dock_tree.is_open(PANEL_INSPECTOR));
        });
    });
    float_visual.run_until_parked();
    assert!(
        float_visual.read(|app| state.read(app).dock_tree.floating.is_empty()),
        "redocking empties the floating list"
    );
    assert!(
        float_visual.read(|app| state.read(app).dock_tree.is_open(PANEL_MIXER)),
        "redocked Mixer tab is back in the main tree"
    );
}

/// Pointer gestures on the frame tab bar: middle-clicking the only tab in the
/// left frame closes it and collapses the frame, and dragging a tab from the
/// centre frame's tab bar into the bottom zone of the right frame splits the
/// right frame.
#[gpui::test]
fn interaction_dock_pointer_middle_close_and_drag_to_zone(cx: &mut gpui::TestAppContext) {
    use aria_ui::dock::{FrameId, PANEL_VIEWER};

    let (state, visual) = editing!(cx, "dock-pointer");

    let [left_x, left_y, _, _] = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::LEFT))
        .expect("left frame bounds");

    // Middle-click the Project tab, then the Effect Controls tab in the left
    // frame's 24 px tab bar: closing the last tab collapses the left frame.
    for panel in [aria_ui::dock::PANEL_PROJECT, aria_ui::dock::PANEL_INSPECTOR] {
        visual.simulate_mouse_down(
            point(px(left_x + 24.0), px(left_y + 12.0)),
            MouseButton::Middle,
            Modifiers::none(),
        );
        visual.simulate_mouse_up(
            point(px(left_x + 24.0), px(left_y + 12.0)),
            MouseButton::Middle,
            Modifiers::none(),
        );
        visual.run_until_parked();
        assert!(
            !visual.read(|app| state.read(app).dock_tree.is_open(panel)),
            "middle-clicking closes {panel}"
        );
    }
    assert!(
        visual.read(|app| state.read(app).dock_tree.frame(FrameId::LEFT).is_none()),
        "closing the last tab in the left frame collapses the frame"
    );

    // Drag the active Viewer tab from the centre frame's tab bar to the bottom
    // drop zone of the right frame.
    let [cx_x, cx_y, _, _] = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::CENTER))
        .expect("centre frame bounds");
    let [rx_x, rx_y, rx_w, rx_h] = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::RIGHT))
        .expect("right frame bounds");

    let start_pt = point(px(cx_x + 24.0), px(cx_y + 12.0));
    let drop_pt = point(px(rx_x + rx_w * 0.5), px(rx_y + rx_h * 0.90));
    visual.simulate_mouse_down(start_pt, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(drop_pt, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    visual.simulate_mouse_up(drop_pt, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();

    let viewer_frame = visual
        .read(|app| state.read(app).dock_tree.find(PANEL_VIEWER))
        .expect("Viewer remains docked");
    assert_ne!(
        viewer_frame,
        FrameId::CENTER,
        "dragging Viewer to the right frame's bottom zone moves it into a new split frame"
    );
}

/// Editing arrangement matches `docs/spec/layout.md`:
/// - Upper row: Left (`Project | Effect Controls`, 320 px), Centre (`Viewer`),
///   Right (`Effects & Presets` active, 300 px; unregistered `Info`/`Character`
///   filtered from drawn tabs).
/// - Lower row: `Timeline` on the left, `Audio Meters` (64 px) on the right.
/// - Dragging the splitter between `CENTER` and `RIGHT` or between upper and
///   lower rows resizes the frames, and double-clicking restores the workspace's
///   saved `0.55 / 0.45` ratio.
#[gpui::test]
fn interaction_dock_editing_arrangement_and_splitter_restore(cx: &mut gpui::TestAppContext) {
    use aria_ui::dock::{
        FrameId, PANEL_AUDIO_METERS, PANEL_EFFECTS, PANEL_INSPECTOR, PANEL_PROJECT, PANEL_TIMELINE,
        PANEL_VIEWER,
    };

    let (state, visual) = editing!(cx, "dock-editing-arrangement");

    let left_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::LEFT))
        .expect("LEFT frame drawn");
    let center_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::CENTER))
        .expect("CENTER frame drawn");
    let right_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::RIGHT))
        .expect("RIGHT frame drawn");
    let bottom_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::BOTTOM))
        .expect("BOTTOM timeline frame drawn");
    let meters_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::METERS))
        .expect("METERS audio meters frame drawn");

    // Upper row frames sit side by side above the lower row.
    assert!(left_rect[0] < center_rect[0] && center_rect[0] < right_rect[0]);
    assert!(bottom_rect[1] >= left_rect[1] + left_rect[3] - 2.0);
    assert!(meters_rect[0] >= bottom_rect[0] + bottom_rect[2] - 2.0);
    assert!(
        (meters_rect[2] - 64.0).abs() <= 12.0,
        "Audio Meters frame is 64 px wide in lower row: {meters_rect:?}"
    );

    visual.read(|app| {
        let s = state.read(app);
        let left = s.dock_tree.frame(FrameId::LEFT).expect("LEFT");
        assert_eq!(left.tabs, vec![PANEL_PROJECT, PANEL_INSPECTOR]);
        assert_eq!(left.active_panel(), PANEL_PROJECT);
        let right = s.dock_tree.frame(FrameId::RIGHT).expect("RIGHT");
        assert_eq!(right.active_panel(), PANEL_EFFECTS);
        let bottom = s.dock_tree.frame(FrameId::BOTTOM).expect("BOTTOM");
        assert_eq!(bottom.active_panel(), PANEL_TIMELINE);
        let meters = s.dock_tree.frame(FrameId::METERS).expect("METERS");
        assert_eq!(meters.active_panel(), PANEL_AUDIO_METERS);
        let center = s.dock_tree.frame(FrameId::CENTER).expect("CENTER");
        assert_eq!(center.active_panel(), PANEL_VIEWER);
    });

    // Drag the vertical splitter between the upper row and lower row down by 50 px,
    // then double-click it to restore the saved 0.55 / 0.45 workspace ratio.
    let split_x = center_rect[0] + center_rect[2] * 0.5;
    let split_y = left_rect[1] + left_rect[3] + 2.0;
    visual.simulate_mouse_down(
        point(px(split_x), px(split_y)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.simulate_mouse_move(
        point(px(split_x), px(split_y + 50.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.simulate_mouse_up(
        point(px(split_x), px(split_y + 50.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.run_until_parked();

    let moved_weights = visual
        .read(|app| state.read(app).dock_tree.split_weights(&[]))
        .expect("root weights");
    assert!(
        (moved_weights[0] - 0.55).abs() > 0.01,
        "dragging vertical splitter changed root weights: {moved_weights:?}"
    );

    // Double-click the moved vertical splitter to restore 0.55 / 0.45.
    let new_left_rect = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::LEFT))
        .expect("LEFT frame drawn");
    let new_split_y = new_left_rect[1] + new_left_rect[3] + 2.0;
    visual.simulate_event(gpui::MouseDownEvent {
        position: point(px(split_x), px(new_split_y)),
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    visual.run_until_parked();
    let restored_weights = visual
        .read(|app| state.read(app).dock_tree.split_weights(&[]))
        .expect("root weights");
    assert!((restored_weights[0] - 0.55).abs() < 1e-4);
    assert!((restored_weights[1] - 0.45).abs() < 1e-4);
}

/// Single-clicking an active tab keeps the frame open and focused (never
/// collapsing it), and motion/drop-zone/multi-monitor helpers obey the spec.
#[gpui::test]
fn interaction_dock_active_tab_click_never_collapses_and_helpers_match_spec(
    cx: &mut gpui::TestAppContext,
) {
    use aria_ui::dock::{FloatRect, FrameId, PANEL_PROJECT, Zone};
    use aria_ui::shell::dock_view::{
        DROP_ZONE_FOLLOW_SPEC, DROP_ZONE_SPEC, MAXIMIZE_SPEC, WORKSPACE_MORPH_SPEC,
        clamp_float_rect_to_displays, drop_zone_preview_fractions, splitter_motion_key,
    };

    let (state, visual) = editing!(cx, "dock-active-tab-click");
    let [left_x, left_y, left_w, _] = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::LEFT))
        .expect("LEFT bounds");

    // Click the already-active Project tab twice as single clicks: the frame
    // stays open and unmaximized, and focus moves to FrameId::LEFT.
    let project_tab_pt = point(px(left_x + 24.0), px(left_y + 12.0));
    visual.simulate_click(project_tab_pt, Modifiers::none());
    visual.run_until_parked();
    assert!(visual.read(|app| state.read(app).dock_tree.is_open(PANEL_PROJECT)));
    assert_eq!(
        visual.read(|app| state.read(app).dock_tree.focused),
        FrameId::LEFT
    );
    assert_eq!(visual.read(|app| state.read(app).dock_tree.maximized), None);
    let [_, _, left_w_after, _] = visual
        .read(|app| state.read(app).dock_frame_rect(FrameId::LEFT))
        .expect("LEFT still drawn");
    assert!((left_w_after - left_w).abs() < 1.0);

    // Splitter motion keys for sibling nested splits at the same depth never collide.
    assert_ne!(splitter_motion_key(&[0], 0), splitter_motion_key(&[1], 0));
    assert_ne!(splitter_motion_key(&[0], 0), splitter_motion_key(&[0], 1));

    // Drop-zone preview fractions cover the exact half/full regions (`layout.md:38-41`).
    assert_eq!(
        drop_zone_preview_fractions(Zone::Left),
        (0.0, 0.0, 0.5, 1.0)
    );
    assert_eq!(
        drop_zone_preview_fractions(Zone::Right),
        (0.5, 0.0, 0.5, 1.0)
    );
    assert_eq!(drop_zone_preview_fractions(Zone::Top), (0.0, 0.0, 1.0, 0.5));
    assert_eq!(
        drop_zone_preview_fractions(Zone::Bottom),
        (0.0, 0.5, 1.0, 0.5)
    );
    assert_eq!(
        drop_zone_preview_fractions(Zone::Center),
        (0.0, 0.0, 1.0, 1.0)
    );

    // Off-screen floating window is clamped onto the primary monitor (`layout.md:48`),
    // while a valid second-monitor rectangle is preserved.
    let displays = [[0.0, 0.0, 1920.0, 1080.0], [1920.0, 0.0, 1920.0, 1080.0]];
    let clamped = clamp_float_rect_to_displays(
        FloatRect {
            x: -2400.0,
            y: -1200.0,
            w: 480.0,
            h: 320.0,
        },
        (160.0, 80.0),
        &displays,
    );
    assert!(clamped.x >= 0.0 && clamped.x + clamped.w <= 1920.0);
    assert!(clamped.y >= 0.0 && clamped.y + clamped.h <= 1080.0);

    let second_mon = clamp_float_rect_to_displays(
        FloatRect {
            x: 2100.0,
            y: 120.0,
            w: 480.0,
            h: 320.0,
        },
        (160.0, 80.0),
        &displays,
    );
    assert_eq!(second_mon.x, 2100.0);
    assert_eq!(second_mon.y, 120.0);

    // MotionSpec timings match `docs/spec/layout.md:51-53, 81`, and the
    // docking and workspace curve matches `docs/spec/look.md` Motion.
    assert_eq!(DROP_ZONE_SPEC.delay_ms, 80);
    assert_eq!(DROP_ZONE_SPEC.duration_ms, 120);
    assert_eq!(DROP_ZONE_FOLLOW_SPEC.duration_ms, 120);
    assert_eq!(MAXIMIZE_SPEC.duration_ms, 160);
    assert_eq!(WORKSPACE_MORPH_SPEC.duration_ms, 180);
    assert_eq!(
        WORKSPACE_MORPH_SPEC.easing,
        gpui::motion::Easing::EaseOut,
        "a workspace change decelerates into place"
    );
}
