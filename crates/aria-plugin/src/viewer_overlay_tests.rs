//! Tests for viewer overlays.
//!
//! Closes the class "an overlay draws a scene the host cannot place, hears a
//! drag its scene could not produce, or falls behind the pointer": every case
//! goes through `register_viewer_overlay`, `viewer_overlay`,
//! `send_overlay_event` and `refresh_viewer_overlay`, the entry points every
//! host and plugin use, and every [`Problem`] and [`SceneError`] is reached
//! through [`OverlayScene::check`] by an exhaustive match. The build, refresh,
//! panic and ownership rules are the shared [`crate::live`] driver's, which
//! the panel tests cover too. It does not cover how a host maps or draws a
//! scene; the Viewer tests in `aria-ui` own that.

use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::mpsc;
use std::time::Duration;

use parking_lot::Mutex;

use super::*;
use crate::plugin::{Plugin, PluginManifest};

const SETTLE: Duration = Duration::from_secs(10);
const PLUGIN: &str = "test.overlay";
const ID: &str = "test.overlay.pin";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Mode {
    #[default]
    Normal,
    PanicScene,
    Refuse,
}

/// What a test overlay heard, and how it behaves next.
#[derive(Default)]
struct Probe {
    pin: Mutex<[f32; 2]>,
    heard: Mutex<Vec<OverlayEvent>>,
    scenes: AtomicUsize,
    mode: Mutex<Mode>,
}

/// One handle, `pin`, that a drag moves.
struct Pin(Arc<Probe>);

impl ViewerOverlay for Pin {
    fn scene(&self, _: &PluginContext) -> OverlayScene {
        self.0.scenes.fetch_add(1, SeqCst);
        match *self.0.mode.lock() {
            Mode::PanicScene => panic!("no scene"),
            Mode::Refuse => {
                return OverlayScene {
                    shapes: vec![line([0.0, 0.0], [f32::NAN, 1.0])],
                };
            }
            Mode::Normal => {}
        }
        OverlayScene {
            shapes: vec![OverlayShape::Handle {
                id: "pin".into(),
                at: *self.0.pin.lock(),
                color: OverlayColor::Accent,
                look: HandleLook::Dot,
            }],
        }
    }

    fn event(&self, _: &PluginContext, event: &OverlayEvent) -> Result<(), String> {
        self.0.heard.lock().push(event.clone());
        let OverlayEvent::Drag { at, .. } = event;
        *self.0.pin.lock() = *at;
        Ok(())
    }
}

struct WithOverlay {
    manifest: PluginManifest,
    overlay: &'static str,
    probe: Arc<Probe>,
}

impl Plugin for WithOverlay {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register_viewer_overlay(
            OverlaySpec {
                id: self.overlay.to_owned(),
                title: "Pin".into(),
            },
            Pin(Arc::clone(&self.probe)),
        )?;
        Ok(())
    }
}

fn with_overlay(plugin: &'static str, overlay: &'static str) -> WithOverlay {
    WithOverlay {
        manifest: PluginManifest::new(plugin, plugin, "0.1.0", &[], &[]),
        overlay,
        probe: Arc::default(),
    }
}

/// A context whose pin overlay has drawn its first scene.
fn drawn() -> (PluginContext, WithOverlay) {
    let ctx = PluginContext::new();
    let plugin = with_overlay(PLUGIN, ID);
    ctx.mount(&plugin).expect("mounts");
    let _ = ctx.viewer_overlay(ID);
    settle(&ctx);
    assert!(state(&ctx).scene.is_some(), "the first build did not land");
    (ctx, plugin)
}

fn settle(ctx: &PluginContext) {
    assert!(
        ctx.events().flush(SETTLE),
        "the delivery thread did not settle"
    );
}

fn state(ctx: &PluginContext) -> OverlayState {
    ctx.viewer_overlay(ID).expect("the pin is registered")
}

/// Park the delivery thread until the returned sender drops.
fn hold(ctx: &PluginContext) -> mpsc::Sender<()> {
    let (release, wait) = mpsc::channel::<()>();
    let (took, taken) = mpsc::channel();
    ctx.events().defer(move || {
        let _ = took.send(());
        let _ = wait.recv();
    });
    taken
        .recv_timeout(SETTLE)
        .expect("the delivery thread took the hold");
    release
}

fn drag(handle: &str, phase: DragPhase, at: [f32; 2]) -> OverlayEvent {
    OverlayEvent::Drag {
        handle: handle.into(),
        phase,
        at,
    }
}

fn line(from: [f32; 2], to: [f32; 2]) -> OverlayShape {
    OverlayShape::Line {
        from,
        to,
        color: OverlayColor::Accent,
        width: 1.0,
    }
}

fn handle(id: &str, at: [f32; 2]) -> OverlayShape {
    OverlayShape::Handle {
        id: id.into(),
        at,
        color: OverlayColor::Accent,
        look: HandleLook::Dot,
    }
}

fn scene(shapes: Vec<OverlayShape>) -> OverlayScene {
    OverlayScene { shapes }
}

/// One refused scene for every [`Problem`] and every scene-wide
/// [`SceneError`]. The match is exhaustive, so a new rule does not compile
/// until it has a case here.
fn refused_case(error: &SceneError) -> OverlayScene {
    let text = |text: &str, size: f32| OverlayShape::Text {
        at: [0.0, 0.0],
        text: text.into(),
        color: OverlayColor::Text,
        size,
    };
    match error {
        SceneError::TooManyShapes(n) => scene(vec![line([0.0, 0.0], [1.0, 1.0]); *n]),
        SceneError::TooManyPoints(_) => scene(vec![
            OverlayShape::Path {
                points: vec![[0.0, 0.0]; MAX_PATH_POINTS / 2 + 1],
                closed: false,
                color: OverlayColor::Accent,
                width: 1.0,
            };
            2
        ]),
        SceneError::TooMuchText(_) => {
            let chunk = "x".repeat(MAX_TEXT_BYTES);
            let count = MAX_TREE_TEXT_BYTES / MAX_TEXT_BYTES + 1;
            scene((0..count).map(|_| text(&chunk, 12.0)).collect())
        }
        SceneError::DuplicateHandle(id) => {
            scene(vec![handle(id, [0.0, 0.0]), handle(id, [1.0, 1.0])])
        }
        SceneError::Shape { problem, .. } => scene(vec![match problem {
            Problem::Point(point) => line(*point, [0.0, 0.0]),
            Problem::Extent(size) => OverlayShape::Rect {
                origin: [0.0, 0.0],
                size: *size,
                color: OverlayColor::Accent,
                width: 1.0,
                fill: None,
            },
            Problem::Stroke(width) => OverlayShape::Ellipse {
                center: [0.0, 0.0],
                radii: [4.0, 4.0],
                color: OverlayColor::Accent,
                width: *width,
                fill: Some(OverlayColor::Muted),
            },
            Problem::ShortPath(n) => OverlayShape::Path {
                points: vec![[0.0, 0.0]; *n],
                closed: true,
                color: OverlayColor::Accent,
                width: 1.0,
            },
            Problem::LongText(n) => text(&"x".repeat(*n), 12.0),
            Problem::MultilineText => text("one\ntwo", 12.0),
            Problem::TextSize(size) => text("label", *size),
            Problem::HandleId(n) => handle(&"h".repeat(*n), [0.0, 0.0]),
        }]),
    }
}

#[test]
fn every_rule_refuses_its_scene_with_the_reason() {
    let shape = |problem| SceneError::Shape { index: 0, problem };
    let errors = [
        SceneError::TooManyShapes(MAX_SHAPES + 1),
        SceneError::TooManyPoints(MAX_PATH_POINTS + 2),
        SceneError::TooMuchText((MAX_TREE_TEXT_BYTES / MAX_TEXT_BYTES + 1) * MAX_TEXT_BYTES),
        SceneError::DuplicateHandle("pin".into()),
        shape(Problem::Point([f32::INFINITY, 0.0])),
        shape(Problem::Point([MAX_COORDINATE * 2.0, 0.0])),
        shape(Problem::Extent([-1.0, 4.0])),
        shape(Problem::Stroke(0.0)),
        shape(Problem::Stroke(MAX_STROKE + 0.5)),
        shape(Problem::ShortPath(1)),
        shape(Problem::LongText(MAX_TEXT_BYTES + 1)),
        shape(Problem::MultilineText),
        shape(Problem::TextSize(TEXT_SIZE_RANGE[0] - 1.0)),
        shape(Problem::TextSize(TEXT_SIZE_RANGE[1] + 1.0)),
        shape(Problem::HandleId(0)),
        shape(Problem::HandleId(MAX_WIDGET_ID_BYTES + 1)),
    ];
    for error in errors {
        let refused = refused_case(&error).check();
        assert_eq!(refused, Err(error.clone()), "{error}");
    }

    let control = scene(vec![handle("a\u{7}", [0.0, 0.0])]).check();
    assert_eq!(control, Err(shape(Problem::HandleId(2))));
}

#[test]
fn a_scene_at_every_limit_is_drawn() {
    let at_limits = scene(vec![
        OverlayShape::Path {
            points: vec![[MAX_COORDINATE, -MAX_COORDINATE]; MAX_PATH_POINTS],
            closed: true,
            color: OverlayColor::Rgba([255, 0, 0, 128]),
            width: MAX_STROKE,
        },
        OverlayShape::Text {
            at: [0.0, 0.0],
            text: "x".repeat(MAX_TEXT_BYTES).into(),
            color: OverlayColor::Warning,
            size: TEXT_SIZE_RANGE[0],
        },
        OverlayShape::Text {
            at: [0.0, 0.0],
            text: "big".into(),
            color: OverlayColor::Danger,
            size: TEXT_SIZE_RANGE[1],
        },
        OverlayShape::Rect {
            origin: [0.0, 0.0],
            size: [0.0, 0.0],
            color: OverlayColor::Success,
            width: 1.0,
            fill: Some(OverlayColor::Rgba([0, 0, 0, 0])),
        },
        handle(&"h".repeat(MAX_WIDGET_ID_BYTES), [1.0, 1.0]),
    ]);
    assert_eq!(at_limits.check(), Ok(()));
    let full = scene(vec![line([0.0, 0.0], [1.0, 1.0]); MAX_SHAPES]);
    assert_eq!(full.check(), Ok(()));
}

#[test]
fn a_scene_reads_from_json_with_its_defaults() {
    let json = r#"{"shapes":[
        {"kind":"line","from":[0,0],"to":[10,5]},
        {"kind":"handle","id":"corner","at":[3,4],"color":{"rgba":[255,0,0,255]},"look":"cross"},
        {"kind":"text","at":[1,2],"text":"Track 1"},
        {"kind":"rect","origin":[0,0],"size":[4,4],"fill":"muted"}
    ]}"#;
    let read: OverlayScene = serde_json::from_str(json).expect("a scene");
    assert_eq!(
        read,
        scene(vec![
            line([0.0, 0.0], [10.0, 5.0]),
            OverlayShape::Handle {
                id: "corner".into(),
                at: [3.0, 4.0],
                color: OverlayColor::Rgba([255, 0, 0, 255]),
                look: HandleLook::Cross,
            },
            OverlayShape::Text {
                at: [1.0, 2.0],
                text: "Track 1".into(),
                color: OverlayColor::Accent,
                size: 12.0,
            },
            OverlayShape::Rect {
                origin: [0.0, 0.0],
                size: [4.0, 4.0],
                color: OverlayColor::Accent,
                width: 1.0,
                fill: Some(OverlayColor::Muted),
            },
        ])
    );
    let back: OverlayScene =
        serde_json::from_str(&serde_json::to_string(&read).expect("writes")).expect("reads");
    assert_eq!(back, read);
}

#[test]
fn a_drag_moves_the_handle_and_the_scene_follows() {
    let (ctx, plugin) = drawn();
    for (phase, at) in [
        (DragPhase::Start, [0.0, 0.0]),
        (DragPhase::Move, [40.0, 30.0]),
        (DragPhase::End, [40.0, 30.0]),
    ] {
        ctx.send_overlay_event(ID, drag("pin", phase, at))
            .expect("queued");
        settle(&ctx);
    }
    let shown = state(&ctx).scene.expect("a scene");
    assert_eq!(shown.shapes, vec![handle("pin", [40.0, 30.0])]);
    assert_eq!(plugin.probe.heard.lock().len(), 3);
}

#[test]
fn pointer_steps_queued_behind_a_busy_handler_merge_into_the_latest() {
    let (ctx, plugin) = drawn();
    let release = hold(&ctx);
    let sent = [
        drag("pin", DragPhase::Start, [0.0, 0.0]),
        drag("pin", DragPhase::Move, [1.0, 1.0]),
        drag("pin", DragPhase::Move, [2.0, 2.0]),
        drag("pin", DragPhase::End, [2.0, 2.0]),
        drag("pin", DragPhase::Start, [2.0, 2.0]),
        drag("pin", DragPhase::Move, [3.0, 3.0]),
        drag("pin", DragPhase::Move, [4.0, 4.0]),
        drag("pin", DragPhase::Move, [5.0, 5.0]),
    ];
    for event in sent {
        ctx.send_overlay_event(ID, event).expect("queued");
    }
    drop(release);
    settle(&ctx);
    // A step never jumps a release or a press: only consecutive steps merge.
    assert_eq!(
        *plugin.probe.heard.lock(),
        vec![
            drag("pin", DragPhase::Start, [0.0, 0.0]),
            drag("pin", DragPhase::Move, [2.0, 2.0]),
            drag("pin", DragPhase::End, [2.0, 2.0]),
            drag("pin", DragPhase::Start, [2.0, 2.0]),
            drag("pin", DragPhase::Move, [5.0, 5.0]),
        ]
    );

    // A step sent after the last one was handled queues again.
    ctx.send_overlay_event(ID, drag("pin", DragPhase::Move, [6.0, 6.0]))
        .expect("queued");
    settle(&ctx);
    assert_eq!(plugin.probe.heard.lock().len(), 6);
}

#[test]
fn steps_of_different_handles_do_not_merge() {
    let (ctx, plugin) = drawn();
    let release = hold(&ctx);
    for event in [
        drag("pin", DragPhase::Move, [1.0, 1.0]),
        drag("gone", DragPhase::Move, [2.0, 2.0]),
        drag("pin", DragPhase::Move, [3.0, 3.0]),
    ] {
        ctx.send_overlay_event(ID, event).expect("queued");
    }
    drop(release);
    settle(&ctx);
    assert_eq!(plugin.probe.heard.lock().len(), 3);
}

#[test]
fn a_drag_the_scene_cannot_produce_never_reaches_the_plugin() {
    let ctx = PluginContext::new();
    let plugin = with_overlay(PLUGIN, ID);
    ctx.mount(&plugin).expect("mounts");
    assert_eq!(
        ctx.send_overlay_event(ID, drag("pin", DragPhase::Start, [0.0, 0.0])),
        Err(OverlayError::NotDrawn(ID.into()))
    );
    let _ = ctx.viewer_overlay(ID);
    settle(&ctx);

    let cases = [
        (
            drag("missing", DragPhase::Start, [0.0, 0.0]),
            OverlayError::NoHandle {
                overlay: ID.into(),
                handle: "missing".into(),
            },
        ),
        (
            drag("pin", DragPhase::Move, [f32::NAN, 0.0]),
            OverlayError::BadPoint(ID.into()),
        ),
        (
            drag("pin", DragPhase::End, [0.0, f32::INFINITY]),
            OverlayError::BadPoint(ID.into()),
        ),
    ];
    for (event, error) in cases {
        assert_eq!(ctx.send_overlay_event(ID, event), Err(error));
    }
    assert_eq!(
        ctx.send_overlay_event(
            "test.overlay.none",
            drag("pin", DragPhase::Start, [0.0, 0.0])
        ),
        Err(OverlayError::Unknown("test.overlay.none".into()))
    );
    settle(&ctx);
    assert!(plugin.probe.heard.lock().is_empty());

    // A step or release continues a press even when the handle left the scene.
    ctx.send_overlay_event(ID, drag("missing", DragPhase::End, [1.0, 1.0]))
        .expect("queued");
    settle(&ctx);
    assert_eq!(plugin.probe.heard.lock().len(), 1);
}

#[test]
fn a_plugin_drives_only_its_own_overlays() {
    let ctx = PluginContext::new();
    let mine = with_overlay("test.mine", "test.mine.pin");
    let theirs = with_overlay("test.theirs", "test.theirs.pin");
    ctx.mount(&mine).expect("mounts");
    ctx.mount(&theirs).expect("mounts");
    let _ = ctx.viewer_overlay("test.mine.pin");
    let _ = ctx.viewer_overlay("test.theirs.pin");
    settle(&ctx);

    let view = ctx.view(&mine.manifest);
    let foreign = "test.theirs.pin";
    let press = || drag("pin", DragPhase::Start, [0.0, 0.0]);
    assert_eq!(
        view.refresh_viewer_overlay(foreign),
        Err(OverlayError::NotOwned(foreign.into()))
    );
    assert_eq!(
        view.send_overlay_event(foreign, press()),
        Err(OverlayError::NotOwned(foreign.into()))
    );
    assert_eq!(view.refresh_viewer_overlay("test.mine.pin"), Ok(()));
    assert_eq!(view.send_overlay_event("test.mine.pin", press()), Ok(()));
    assert_eq!(ctx.send_overlay_event(foreign, press()), Ok(()));
    settle(&ctx);
    assert_eq!(mine.probe.heard.lock().len(), 1);
    assert_eq!(theirs.probe.heard.lock().len(), 1);
}

#[test]
fn a_refused_scene_keeps_the_last_good_one_and_records_a_fault() {
    let (ctx, plugin) = drawn();
    *plugin.probe.mode.lock() = Mode::Refuse;
    ctx.refresh_viewer_overlay(ID).expect("refreshes");
    settle(&ctx);

    let reason = SceneError::Shape {
        index: 0,
        problem: Problem::Point([f32::NAN, 1.0]),
    }
    .to_string();
    let shown = state(&ctx);
    assert_eq!(shown.refused.as_deref(), Some(reason.as_str()));
    assert_eq!(
        shown.scene.expect("the last good scene").shapes,
        vec![handle("pin", [0.0, 0.0])]
    );
    let faults = ctx.faults();
    assert_eq!(faults.len(), 1, "{faults:?}");
    assert_eq!(
        faults[0].message,
        format!("viewer overlay {ID} returned a scene the host refused: {reason}")
    );
}

#[test]
fn an_overlay_that_panics_fails_its_plugin_and_leaves() {
    let (ctx, plugin) = drawn();
    *plugin.probe.mode.lock() = Mode::PanicScene;
    ctx.refresh_viewer_overlay(ID).expect("refreshes");
    settle(&ctx);
    assert!(!ctx.is_mounted(PLUGIN));
    assert!(ctx.viewer_overlays().is_empty());
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(
        failures[0].reason,
        format!("viewer overlay {ID} panicked while drawing: no scene")
    );
}

#[test]
fn registration_is_refused_without_an_id_or_title_or_twice() {
    let ctx = PluginContext::new();
    let probe = Arc::new(Probe::default());
    let spec = |id: &str, title: &str| OverlaySpec {
        id: id.into(),
        title: title.into(),
    };
    let refused = |result: Result<Registration, PluginError>| match result {
        Err(PluginError::OverlayRefused(reason)) => reason,
        other => panic!("registered: {other:?}"),
    };
    assert_eq!(
        refused(ctx.register_viewer_overlay(spec(" ", "Pin"), Pin(Arc::clone(&probe)))),
        "a viewer overlay needs an id"
    );
    assert_eq!(
        refused(ctx.register_viewer_overlay(spec(ID, ""), Pin(Arc::clone(&probe)))),
        format!("{ID}: a viewer overlay needs a title")
    );
    let held = ctx
        .register_viewer_overlay(spec(ID, "Pin"), Pin(Arc::clone(&probe)))
        .expect("registers");
    assert_eq!(
        refused(ctx.register_viewer_overlay(spec(ID, "Pin"), Pin(Arc::clone(&probe)))),
        format!("a viewer overlay is registered as {ID}")
    );
    let before = ctx.overlay_revision();
    drop(held);
    assert!(ctx.viewer_overlays().is_empty());
    assert!(ctx.overlay_revision() > before);
    assert_eq!(
        probe.scenes.load(SeqCst),
        0,
        "a scene built before any read"
    );
}
