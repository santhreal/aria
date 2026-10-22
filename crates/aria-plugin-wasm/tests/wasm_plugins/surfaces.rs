//! WASM sidebar panels, viewer overlays, and video scopes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aria_commands::{Command, EditCommand};
use aria_models::CanvasAspect;
use aria_plugin::{
    DragPhase, HandleLook, Origin, OverlayColor, OverlayEvent, OverlayShape, PanelEvent,
    PanelLocation, PanelSpec, ScopeError, ScopeFrame, ScopeImage, UiNode,
};
use aria_plugin_wasm::{Limits, load};

use super::{FIXTURE_WASM, FLUSH_TIMEOUT, SERIAL, setup_host_context, write_package};

fn expected_tools_tree(presses: u32, level: f64, note: &str) -> UiNode {
    UiNode::Column {
        children: vec![
            UiNode::heading(format!("Tools ({presses})")),
            UiNode::button("bump", format!("Count: {presses}")),
            UiNode::Slider {
                id: "level".into(),
                label: "Level".into(),
                value: level,
                min: 0.0,
                max: 1.0,
                step: 0.25,
            },
            UiNode::input("note", note, "Enter note"),
        ],
    }
}

/// A WASM sidebar panel builds its initial `UiNode` tree on first read, rebuilds after
/// button, slider, and input events, records guest event errors, and rebuilds on
/// `refresh-panel`.
#[test]
fn panel_view_events_and_refresh_rebuild_exact_ui_tree() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "none", &[], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let panels = ctx.panels();
    assert_eq!(
        panels,
        vec![Arc::new(PanelSpec {
            id: "example.tools-panel".to_owned(),
            title: "WASM Tools".into(),
            location: PanelLocation::Sidebar,
        })]
    );

    let _ = ctx.panel("example.tools-panel").expect("panel registered");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    let initial = ctx.panel("example.tools-panel").expect("panel state");
    assert_eq!(initial.refused, None);
    assert_eq!(initial.event_error, None);
    assert_eq!(
        initial.tree.as_deref(),
        Some(&expected_tools_tree(0, 0.5, "ready"))
    );

    ctx.send_panel_event(
        "example.tools-panel",
        PanelEvent::Pressed { id: "bump".into() },
    )
    .expect("press bump");
    ctx.send_panel_event(
        "example.tools-panel",
        PanelEvent::Slid {
            id: "level".into(),
            value: 0.74,
        },
    )
    .expect("slide level");
    ctx.send_panel_event(
        "example.tools-panel",
        PanelEvent::Submitted {
            id: "note".into(),
            text: "updated".into(),
        },
    )
    .expect("submit note");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));

    let updated = ctx.panel("example.tools-panel").expect("panel state");
    assert_eq!(updated.refused, None);
    assert_eq!(updated.event_error, None);
    assert_eq!(
        updated.tree.as_deref(),
        Some(&expected_tools_tree(1, 0.75, "updated"))
    );

    ctx.send_panel_event(
        "example.tools-panel",
        PanelEvent::Submitted {
            id: "note".into(),
            text: "ERROR".into(),
        },
    )
    .expect("submit error sentinel");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    let with_err = ctx.panel("example.tools-panel").expect("panel state");
    assert_eq!(
        with_err.event_error.as_deref(),
        Some("note rejected by guest")
    );

    ctx.run_command("example.refresh-panel")
        .expect("run refresh-panel command");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    let refreshed = ctx.panel("example.tools-panel").expect("panel state");
    assert_eq!(
        refreshed.tree.as_deref(),
        Some(&expected_tools_tree(11, 0.75, "updated"))
    );

    ctx.unmount("com.example.wasm-tools").expect("unmount");
    assert!(ctx.panels().is_empty());
}

/// A malformed flat `list<ui-node>` returned by `panel-view` is rejected with the bad
/// node index, keeping the last valid tree and recording a `PluginFault`; a trapping
/// `panel-view` fails the plugin for the session, drops its panel, and records the failure.
#[test]
fn panel_malformed_list_records_fault_and_trapping_view_fails_plugin() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "none", &[], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let _ = ctx.panel("example.tools-panel").expect("panel registered");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    assert_eq!(
        ctx.panel("example.tools-panel")
            .expect("panel")
            .tree
            .as_deref(),
        Some(&expected_tools_tree(0, 0.5, "ready"))
    );

    // 1. Trigger a malformed flat `list<ui-node>` (node 0 referencing child index 99).
    ctx.send_panel_event(
        "example.tools-panel",
        PanelEvent::Submitted {
            id: "note".into(),
            text: "MALFORMED".into(),
        },
    )
    .expect("trigger malformed flat list");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));

    assert!(ctx.is_mounted("com.example.wasm-tools"));
    let after_malformed = ctx
        .panel("example.tools-panel")
        .expect("panel still registered");
    assert_eq!(
        after_malformed.tree.as_deref(),
        Some(&expected_tools_tree(0, 0.5, "ready")),
        "last valid tree stays when malformed flat list is refused"
    );
    let refused = after_malformed
        .refused
        .as_deref()
        .expect("refused reason set");
    assert!(
        refused.contains("node 0") && refused.contains("child index 99"),
        "unexpected refused reason: {refused}"
    );
    let faults = ctx.faults();
    assert_eq!(faults.len(), 1, "{faults:?}");
    assert_eq!(faults[0].plugin, "com.example.wasm-tools");
    assert!(
        faults[0].message.contains("node 0") && faults[0].message.contains("child index 99"),
        "unexpected fault message: {}",
        faults[0].message
    );

    // 2. Trigger a guest trap inside `panel-view`.
    ctx.send_panel_event(
        "example.tools-panel",
        PanelEvent::Submitted {
            id: "note".into(),
            text: "TRAP_VIEW".into(),
        },
    )
    .expect("trigger trapping view");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));

    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.panels().is_empty());
    assert_eq!(ctx.failures().len(), 1);
    assert_eq!(ctx.failures()[0].id, "com.example.wasm-tools");
    assert!(
        ctx.failures()[0]
            .reason
            .contains("panel example.tools-panel panicked while drawing"),
        "unexpected failure reason: {}",
        ctx.failures()[0].reason
    );
}

/// The reference WASM plugin registers its `example.safe-frame` viewer overlay,
/// computes exact safe-frame and focus crosshair shapes for the sequence frame
/// size, updates the handle position across `Start`/`Move`/`End` drags, supports
/// `refresh-viewer-overlay` outside `mount` while refusing `register-viewer-overlay`
/// outside `mount`, and faults the plugin on a guest trap inside `overlay-scene`.
#[test]
fn wasm_viewer_overlay_renders_frame_guides_drags_handle_refreshes_and_faults_on_trap() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &[], FIXTURE_WASM);
    let (ctx, host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let settle_overlay = |pred: &dyn Fn(&aria_plugin::OverlayState) -> bool| {
        let deadline = Instant::now() + FLUSH_TIMEOUT;
        while Instant::now() < deadline {
            assert!(ctx.events().flush(FLUSH_TIMEOUT));
            if let Some(st) = ctx.viewer_overlay("example.safe-frame")
                && pred(&st)
            {
                return st;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(ctx.events().flush(FLUSH_TIMEOUT));
        let st = ctx
            .viewer_overlay("example.safe-frame")
            .expect("example.safe-frame overlay present");
        assert!(pred(&st), "overlay predicate not satisfied: {st:?}");
        st
    };

    // 1. Trigger initial scene build on default 1920x1080 frame and assert exact shapes.
    assert_eq!(ctx.viewer_overlays().len(), 1);
    let _ = ctx.viewer_overlay("example.safe-frame");
    let initial = settle_overlay(&|st| st.scene.as_ref().is_some_and(|s| s.shapes.len() == 6));
    assert_eq!(&*initial.spec.title, "Safe Frame & Focus");
    assert_eq!(initial.refused, None);
    assert_eq!(
        initial.scene.as_ref().expect("initial scene").shapes,
        vec![
            OverlayShape::Rect {
                origin: [96.0, 54.0],
                size: [1728.0, 972.0],
                color: OverlayColor::Muted,
                width: 1.0,
                fill: None,
            },
            OverlayShape::Rect {
                origin: [192.0, 108.0],
                size: [1536.0, 864.0],
                color: OverlayColor::Text,
                width: 1.0,
                fill: None,
            },
            OverlayShape::Line {
                from: [936.0, 540.0],
                to: [984.0, 540.0],
                color: OverlayColor::Accent,
                width: 1.5,
            },
            OverlayShape::Line {
                from: [960.0, 516.0],
                to: [960.0, 564.0],
                color: OverlayColor::Accent,
                width: 1.5,
            },
            OverlayShape::Text {
                at: [970.0, 530.0],
                text: Arc::from("focus (960, 540) drags=0 refreshes=0"),
                color: OverlayColor::Text,
                size: 12.0,
            },
            OverlayShape::Handle {
                id: Arc::from("focus"),
                at: [960.0, 540.0],
                color: OverlayColor::Accent,
                look: HandleLook::Cross,
            },
        ]
    );

    // 2. Drag Start -> Move -> End moves the focus handle and updates the crosshair and label.
    for (phase, pt) in [
        (DragPhase::Start, [960.0, 540.0]),
        (DragPhase::Move, [700.0, 400.0]),
        (DragPhase::End, [500.0, 300.0]),
    ] {
        ctx.send_overlay_event(
            "example.safe-frame",
            OverlayEvent::Drag {
                handle: Arc::from("focus"),
                phase,
                at: pt,
            },
        )
        .expect("send_overlay_event");
    }
    let after_drag = settle_overlay(&|st| {
        st.scene.as_ref().is_some_and(|s| {
            s.shapes.contains(&OverlayShape::Handle {
                id: Arc::from("focus"),
                at: [500.0, 300.0],
                color: OverlayColor::Accent,
                look: HandleLook::Cross,
            }) && s.shapes.contains(&OverlayShape::Text {
                at: [510.0, 290.0],
                text: Arc::from("focus (500, 300) drags=1 refreshes=0"),
                color: OverlayColor::Text,
                size: 12.0,
            })
        })
    });
    assert_eq!(
        after_drag.scene.as_ref().expect("after_drag scene").shapes[2],
        OverlayShape::Line {
            from: [476.0, 300.0],
            to: [524.0, 300.0],
            color: OverlayColor::Accent,
            width: 1.5,
        }
    );

    // 3. Change sequence canvas aspect to 9:16 (1080x1920) and refresh outside mount.
    host.apply(
        &Origin::Editor,
        Command::Edit(EditCommand::SetCanvas {
            aspect: CanvasAspect::Tall9x16,
            background: [0, 0, 0],
            resolution: None,
        }),
    )
    .expect("set canvas aspect to 9:16");
    ctx.run_command("example.refresh-overlay")
        .expect("refresh overlay outside mount");
    let refreshed = settle_overlay(&|st| {
        st.scene.as_ref().is_some_and(|s| {
            s.shapes.contains(&OverlayShape::Text {
                at: [510.0, 290.0],
                text: Arc::from("focus (500, 300) drags=1 refreshes=1"),
                color: OverlayColor::Text,
                size: 12.0,
            })
        })
    });
    assert_eq!(
        refreshed.scene.as_ref().expect("refreshed scene").shapes[0],
        OverlayShape::Rect {
            origin: [54.0, 96.0],
            size: [972.0, 1728.0],
            color: OverlayColor::Muted,
            width: 1.0,
            fill: None,
        }
    );
    assert_eq!(
        refreshed.scene.as_ref().expect("refreshed scene").shapes[1],
        OverlayShape::Rect {
            origin: [108.0, 192.0],
            size: [864.0, 1536.0],
            color: OverlayColor::Text,
            width: 1.0,
            fill: None,
        }
    );

    // 4. Attempting `register-viewer-overlay` outside `mount` is refused.
    let late_err = ctx
        .run_command("example.late-register-overlay")
        .expect_err("register-viewer-overlay outside mount must fail");
    assert!(
        late_err
            .to_string()
            .contains("register-viewer-overlay is permitted only during mount"),
        "unexpected error: {late_err}"
    );

    // 5. A guest trap inside `overlay-scene` faults the plugin, unmounts it, and drops its overlay.
    ctx.run_command("example.trap-overlay-scene")
        .expect("arm trap-overlay-scene and queue refresh");
    let deadline = Instant::now() + FLUSH_TIMEOUT;
    while Instant::now() < deadline && ctx.is_mounted("com.example.wasm-tools") {
        assert!(ctx.events().flush(FLUSH_TIMEOUT));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.viewer_overlays().is_empty());
    assert_eq!(ctx.failures().len(), 1);
    assert!(
        ctx.failures()[0]
            .reason
            .contains("viewer overlay example.safe-frame panicked while drawing"),
        "unexpected failure reason: {}",
        ctx.failures()[0].reason
    );
}

/// The example's luma histogram measures a frame inside the component: a frame
/// half black and half white fills the first and last of its 64 columns to the
/// top, leaves every other column empty, and reads back the mean luma. The
/// scope leaves the strip when its plugin unmounts.
#[test]
fn wasm_scope_draws_a_histogram_of_the_frame_and_leaves_on_unmount() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();
    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let listed: Vec<_> = ctx
        .scopes()
        .into_iter()
        .map(|scope| {
            (
                scope.spec.id.clone(),
                scope.spec.name.clone(),
                scope.spec.width,
                scope.spec.height,
                scope.plugin_name.to_string(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [(
            "example.luma-histogram".to_owned(),
            "Luma Histogram".to_owned(),
            64,
            32,
            "Example WASM Tools".to_owned()
        )]
    );

    let pixels: Vec<u8> = [[0, 0, 0, 255], [255, 255, 255, 255]]
        .iter()
        .flat_map(|px| [*px, *px])
        .flatten()
        .collect();
    let frame = ScopeFrame::new(2, 2, &pixels).expect("a 2x2 frame");
    let mut image = ScopeImage::default();
    ctx.measure_scope("example.luma-histogram", &frame, &mut image)
        .expect("the histogram measures");
    assert_eq!((image.width(), image.height()), (64, 32));
    assert_eq!(image.readout(), "mean luma 127");
    let lit = |column: u32| {
        (0..32)
            .filter(|row| image.pixels()[((row * 64 + column) * 4 + 3) as usize] != 0)
            .count()
    };
    assert_eq!(lit(0), 32, "black fills the first column");
    assert_eq!(lit(63), 32, "white fills the last column");
    assert_eq!(
        (1..63).map(lit).sum::<usize>(),
        0,
        "nothing else is in the frame"
    );
    let cost = ctx.cost_of("com.example.wasm-tools").expect("mounted");
    assert_eq!(cost.frame.calls, 1, "{cost:?}");

    assert_eq!(
        ctx.measure_scope("example.missing", &frame, &mut image),
        Err(ScopeError::Unknown("example.missing".to_owned()))
    );

    ctx.unmount("com.example.wasm-tools").expect("unmount");
    assert!(ctx.scopes().is_empty());
}
