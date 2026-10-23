//! A plugin viewer overlay as the window drives it: placed under every zoom
//! and pan, dragged, refused, tessellated, and gone.
//!
//! Each case drives the host's own calls (`record_overlay_frame`,
//! `sync_plugin_overlays`, the handle grab, carry and release,
//! `overlay_batches`) against an overlay registered on the session's real
//! context, and asserts what reached the plugin, the status bar, and the
//! paths a frame paints. Placing the elements is not covered: the gpui
//! elements in `panels/preview/plugin_overlays.rs` call these same methods.

use std::sync::Arc;
use std::time::Duration;

use aria_plugin::{
    DragPhase, HandleLook, OverlayColor, OverlayEvent, OverlayScene, OverlayShape, OverlaySpec,
    PluginContext, Registration, ViewerOverlay,
};
use parking_lot::Mutex;

use super::AppState;
use crate::app::canvas_overlay::{Rect, fit_frame, view_frame};
use crate::app::plugin_overlays::{BATCH_POINTS, Batch, CanvasMap, Paint};
use crate::app::preview_view::PreviewView;

const OVERLAY: &str = "test.guides";

/// What the probe overlay holds, and every event it received.
#[derive(Default)]
struct Held {
    events: Vec<OverlayEvent>,
    knob: [f32; 2],
    /// Drawn before the knob.
    shapes: Vec<OverlayShape>,
    locked: bool,
    broken: bool,
}

#[derive(Clone, Default)]
struct Probe(Arc<Mutex<Held>>);

impl Probe {
    fn new() -> Self {
        let probe = Self::default();
        probe.0.lock().knob = [100.0, 50.0];
        probe
    }
}

impl ViewerOverlay for Probe {
    fn scene(&self, _: &PluginContext) -> OverlayScene {
        let held = self.0.lock();
        let mut shapes = held.shapes.clone();
        shapes.push(OverlayShape::Handle {
            id: "knob".into(),
            at: held.knob,
            color: OverlayColor::Accent,
            look: HandleLook::Dot,
        });
        if held.broken {
            shapes.push(line([0.0, 0.0], [10.0, 0.0], OverlayColor::Accent, 0.0));
        }
        OverlayScene { shapes }
    }

    fn event(&self, _: &PluginContext, event: &OverlayEvent) -> Result<(), String> {
        let mut held = self.0.lock();
        held.events.push(event.clone());
        if held.locked {
            return Err("the knob is locked".into());
        }
        let OverlayEvent::Drag { at, .. } = event;
        held.knob = *at;
        Ok(())
    }
}

fn line(from: [f32; 2], to: [f32; 2], color: OverlayColor, width: f32) -> OverlayShape {
    OverlayShape::Line {
        from,
        to,
        color,
        width,
    }
}

fn drag(phase: DragPhase, at: [f32; 2]) -> OverlayEvent {
    OverlayEvent::Drag {
        handle: "knob".into(),
        phase,
        at,
    }
}

fn session() -> AppState {
    let mut state = AppState::new(
        PluginContext::new(),
        crate::scratch_pool::scratch("plugin-overlays"),
    )
    .expect("the session opens");
    state.new_project().expect("a project opens");
    state
}

fn register(state: &AppState, probe: &Probe) -> Registration {
    state
        .ctx
        .register_viewer_overlay(
            OverlaySpec {
                id: OVERLAY.into(),
                title: "Guides".into(),
            },
            probe.clone(),
        )
        .expect("the probe overlay registers")
}

/// Read until the overlays stop moving: a read can queue a build, and a build
/// moves the revision when it lands. Bounded, so a read that never settles
/// fails here rather than hanging the suite.
fn settle(state: &mut AppState) {
    for _ in 0..16 {
        assert!(
            state.ctx.events().flush(Duration::from_secs(5)),
            "the delivery thread did not drain"
        );
        if !state.sync_plugin_overlays() {
            return;
        }
    }
    panic!("the overlays never stopped moving");
}

/// The picture area at half the canvas size from the window origin: with the
/// view fitted, one window pixel is two canvas pixels, exactly.
fn half(state: &AppState) -> Rect {
    let [width, height] = state.canvas_size();
    Rect::new(0.0, 0.0, width as f32 * 0.5, height as f32 * 0.5)
}

/// A session with the probe registered for its lifetime, read once it
/// settled, and the picture area painted at half size.
fn mounted() -> (AppState, Probe, Registration) {
    let mut state = session();
    let probe = Probe::new();
    let registration = register(&state, &probe);
    settle(&mut state);
    let area = half(&state);
    state.record_overlay_frame(area);
    (state, probe, registration)
}

fn events(probe: &Probe) -> Vec<OverlayEvent> {
    probe.0.lock().events.clone()
}

fn knob(state: &AppState) -> Option<[f32; 2]> {
    let scene = state.plugin_overlays.shown().first()?.scene.clone()?;
    scene.shapes.iter().find_map(|shape| match shape {
        OverlayShape::Handle { at, .. } => Some(*at),
        _ => None,
    })
}

fn close(a: Rect, b: Rect) -> bool {
    [(a.x, b.x), (a.y, b.y), (a.w, b.w), (a.h, b.h)]
        .iter()
        .all(|(left, right)| (left - right).abs() < 1e-3)
}

#[test]
fn the_canvas_rectangle_follows_zoom_and_pan() {
    let container = Rect::new(10.0, 20.0, 400.0, 300.0);
    let fitted = PreviewView::default();
    assert!(close(
        view_frame(container, fitted, [1920, 1080]),
        Rect::new(10.0, 57.5, 400.0, 225.0)
    ));
    let zoomed = PreviewView {
        zoom: Some(2.0),
        pan: (-30.0, 15.0),
        ..PreviewView::default()
    };
    // The frame image is 800 by 600 at (-20, 35); the canvas fits it 800 wide.
    assert!(close(
        view_frame(container, zoomed, [1920, 1080]),
        Rect::new(-20.0, 110.0, 800.0, 450.0)
    ));

    let map = CanvasMap::new(Rect::new(-20.0, 110.0, 800.0, 450.0), [1600, 900])
        .expect("the canvas has an area");
    assert_eq!(map.scale(), [0.5, 0.5]);
    assert_eq!(map.to_window([100.0, 40.0]), [30.0, 130.0]);
    assert_eq!(map.to_canvas([30.0, 130.0]), [100.0, 40.0]);
    assert_eq!(
        map.to_canvas([-40.0, 100.0]),
        [-40.0, -20.0],
        "a point off the frame is not clamped"
    );
    assert!(CanvasMap::new(Rect::new(0.0, 0.0, 0.0, 10.0), [1920, 1080]).is_none());
    assert!(CanvasMap::new(Rect::new(0.0, 0.0, 10.0, 10.0), [0, 1080]).is_none());
}

/// The class this closes: the recorded canvas rectangle ignored the zoom and
/// the pan, so on a zoomed Viewer every handle, the transform handles
/// included, sat where the fitted frame would have been.
#[test]
fn the_recorded_canvas_is_the_one_drawn_under_zoom_and_pan() {
    let mut state = session();
    let container = Rect::new(10.0, 20.0, 400.0, 300.0);
    state.record_overlay_frame(container);
    let size = state.canvas_size();
    assert_eq!(state.overlay_frame.canvas, Some(fit_frame(container, size)));

    state.preview_view.zoom = Some(2.0);
    state.preview_view.pan = (-30.0, 15.0);
    assert!(state.record_overlay_frame(container), "the canvas moved");
    let drawn = state.overlay_frame.canvas.expect("recorded");
    assert_eq!(drawn, view_frame(container, state.preview_view, size));
    let fitted = fit_frame(container, size);
    assert!(
        (drawn.w - fitted.w * 2.0).abs() < 1e-3,
        "drawn at twice the fit"
    );
    assert_eq!(
        state.overlay_frame.container,
        Some(container),
        "elements are still placed from the picture area"
    );
}

#[test]
fn a_drag_reaches_the_overlay_in_canvas_pixels_and_moves_its_handle() {
    let (mut state, probe, _registration) = mounted();
    assert_eq!(knob(&state), Some([100.0, 50.0]));

    assert!(state.grab_overlay_handle(OVERLAY, &Arc::from("knob"), [50.0, 25.0]));
    settle(&mut state);
    assert!(state.carry_overlay_handle([60.0, 30.0]));
    settle(&mut state);
    assert_eq!(knob(&state), Some([120.0, 60.0]), "the step was drawn");
    assert!(state.release_overlay_handle([70.0, 40.0]));
    settle(&mut state);

    assert_eq!(
        events(&probe),
        [
            drag(DragPhase::Start, [100.0, 50.0]),
            drag(DragPhase::Move, [120.0, 60.0]),
            drag(DragPhase::End, [140.0, 80.0]),
        ]
    );
    assert_eq!(knob(&state), Some([140.0, 80.0]));
    assert_eq!(state.plugin_overlays.grab, None);
    assert_eq!(state.last_error, None);
    assert!(!state.carry_overlay_handle([0.0, 0.0]), "nothing is held");
    assert!(!state.release_overlay_handle([0.0, 0.0]));
    assert_eq!(events(&probe).len(), 3);
}

#[test]
fn pointer_positions_follow_zoom_and_pan() {
    let (mut state, probe, _registration) = mounted();
    // Twice the fit, panned 40 left and 20 up: one window pixel is one canvas
    // pixel, and the canvas origin sits at (-40, -20).
    state.preview_view.zoom = Some(2.0);
    state.preview_view.pan = (-40.0, -20.0);
    let area = half(&state);
    state.record_overlay_frame(area);

    assert!(state.grab_overlay_handle(OVERLAY, &Arc::from("knob"), [60.0, 30.0]));
    assert!(state.release_overlay_handle([61.0, 33.0]));
    settle(&mut state);
    assert_eq!(
        events(&probe),
        [
            drag(DragPhase::Start, [100.0, 50.0]),
            drag(DragPhase::End, [101.0, 53.0]),
        ]
    );
}

#[test]
fn a_press_on_a_handle_the_scene_lacks_is_refused_and_holds_nothing() {
    let (mut state, probe, _registration) = mounted();
    assert!(!state.grab_overlay_handle(OVERLAY, &Arc::from("ghost"), [50.0, 25.0]));
    assert_eq!(
        state.last_error.as_deref(),
        Some("Viewer overlay test.guides has no handle ghost")
    );
    assert_eq!(state.plugin_overlays.grab, None);
    settle(&mut state);
    assert!(events(&probe).is_empty());
}

#[test]
fn a_press_before_the_first_paint_sends_nothing() {
    let mut state = session();
    let probe = Probe::new();
    let _registration = register(&state, &probe);
    settle(&mut state);
    assert!(!state.grab_overlay_handle(OVERLAY, &Arc::from("knob"), [50.0, 25.0]));
    assert_eq!(state.plugin_overlays.grab, None);
    settle(&mut state);
    assert!(events(&probe).is_empty());
}

#[test]
fn a_handler_error_and_a_refused_scene_reach_the_status_bar_once_each() {
    let (mut state, probe, _registration) = mounted();
    probe.0.lock().locked = true;
    assert!(state.grab_overlay_handle(OVERLAY, &Arc::from("knob"), [50.0, 25.0]));
    settle(&mut state);
    assert_eq!(
        state.last_error.as_deref(),
        Some("Guides: the knob is locked")
    );

    // A rebuild that changes nothing else does not report the same failure
    // again.
    state.last_error = None;
    state
        .ctx
        .refresh_viewer_overlay(OVERLAY)
        .expect("the host refreshes its own overlay");
    settle(&mut state);
    assert_eq!(state.last_error, None);

    // A second failure is a new report, though its text is the same.
    assert!(state.carry_overlay_handle([60.0, 30.0]));
    settle(&mut state);
    assert_eq!(
        state.last_error.as_deref(),
        Some("Guides: the knob is locked")
    );
    assert!(state.release_overlay_handle([60.0, 30.0]));
    settle(&mut state);

    state.last_error = None;
    probe.0.lock().broken = true;
    state
        .ctx
        .refresh_viewer_overlay(OVERLAY)
        .expect("the host refreshes its own overlay");
    settle(&mut state);
    assert_eq!(
        state.last_error.as_deref(),
        Some("Guides: shape 1: the stroke width 0 is not above 0 and at most 32")
    );
    assert_eq!(
        knob(&state),
        Some([100.0, 50.0]),
        "the last scene that passed is still drawn"
    );
}

#[test]
fn a_held_handle_is_let_go_when_its_overlay_unregisters() {
    let (mut state, probe, registration) = mounted();
    assert!(state.grab_overlay_handle(OVERLAY, &Arc::from("knob"), [50.0, 25.0]));
    settle(&mut state);
    drop(registration);
    settle(&mut state);
    assert!(state.plugin_overlays.shown().is_empty());
    assert_eq!(state.plugin_overlays.grab, None);
    assert!(!state.release_overlay_handle([60.0, 30.0]));
    assert_eq!(events(&probe), [drag(DragPhase::Start, [100.0, 50.0])]);
    assert_eq!(state.last_error, None);
}

fn batches(state: &mut AppState) -> Arc<[Batch]> {
    let map = state.overlay_map().expect("the canvas was painted");
    let area = half(state);
    state.overlay_batches(map, area)
}

fn paints(batches: &[Batch]) -> Vec<Paint> {
    batches.iter().map(|batch| batch.paint).collect()
}

fn drawn(shapes: Vec<OverlayShape>) -> (AppState, Arc<[Batch]>) {
    let (mut state, probe, registration) = mounted();
    probe.0.lock().shapes = shapes;
    state
        .ctx
        .refresh_viewer_overlay(OVERLAY)
        .expect("the host refreshes its own overlay");
    settle(&mut state);
    let batches = batches(&mut state);
    state._services.push(registration);
    (state, batches)
}

#[test]
fn consecutive_shapes_painted_alike_share_one_path_in_scene_order() {
    let accent = OverlayColor::Accent;
    let (_state, batches) = drawn(vec![
        line([0.0, 0.0], [100.0, 0.0], accent, 1.0),
        line([0.0, 10.0], [100.0, 10.0], accent, 1.0),
        OverlayShape::Rect {
            origin: [10.0, 10.0],
            size: [50.0, 20.0],
            color: OverlayColor::Danger,
            width: 2.0,
            fill: Some(OverlayColor::Rgba([0, 0, 0, 128])),
        },
        OverlayShape::Ellipse {
            center: [200.0, 200.0],
            radii: [40.0, 20.0],
            color: accent,
            width: 1.0,
            fill: None,
        },
        line([0.0, 20.0], [100.0, 20.0], accent, 1.0),
        line([0.0, 30.0], [100.0, 30.0], accent, 3.0),
        OverlayShape::Text {
            at: [5.0, 5.0],
            text: "label".into(),
            color: accent,
            size: 12.0,
        },
    ]);
    assert_eq!(
        paints(&batches),
        [
            Paint::Stroke(accent, 1.0),
            Paint::Fill(OverlayColor::Rgba([0, 0, 0, 128])),
            Paint::Stroke(OverlayColor::Danger, 2.0),
            Paint::Stroke(accent, 1.0),
            Paint::Stroke(accent, 3.0),
        ],
        "text and handles are elements, not paths"
    );
    assert!(batches.iter().all(|batch| !batch.path.vertices.is_empty()));
}

#[test]
fn shapes_outside_the_picture_area_are_not_tessellated() {
    let far = -100_000.0;
    let (_state, batches) = drawn(vec![
        line([far, far], [far + 10.0, far], OverlayColor::Accent, 1.0),
        OverlayShape::Ellipse {
            center: [far, 0.0],
            radii: [100.0, 100.0],
            color: OverlayColor::Accent,
            width: 1.0,
            fill: Some(OverlayColor::Muted),
        },
    ]);
    assert!(batches.is_empty());
}

#[test]
fn a_path_longer_than_one_batch_is_split_and_keeps_its_closing_segment() {
    let count = BATCH_POINTS * 2 + 10;
    let points: Vec<[f32; 2]> = (0..count).map(|x| [x as f32, 100.0]).collect();
    let (_state, batches) = drawn(vec![OverlayShape::Path {
        points,
        closed: true,
        color: OverlayColor::Accent,
        width: 1.0,
    }]);
    assert_eq!(
        paints(&batches),
        [Paint::Stroke(OverlayColor::Accent, 1.0); 3],
        "three pieces of at most {BATCH_POINTS} points"
    );
    let left = |batch: &Batch| {
        batch
            .path
            .vertices
            .iter()
            .map(|vertex| f32::from(vertex.xy_position.x))
            .fold(f32::MAX, f32::min)
    };
    // Canvas x runs 0 to 8201 at half scale; the last piece starts near 4095
    // and reaches the start only through the closing segment.
    assert!(left(&batches[1]) > 2000.0);
    assert!(left(&batches[2]) < 1.0, "the closing segment is drawn");
}

#[test]
fn the_tessellation_is_reused_until_the_scene_or_the_view_moves() {
    let (mut state, first) = drawn(vec![line(
        [0.0, 0.0],
        [100.0, 0.0],
        OverlayColor::Accent,
        1.0,
    )]);
    let again = batches(&mut state);
    assert!(
        Arc::ptr_eq(&first, &again),
        "an unchanged frame reuses the paths"
    );

    let map = state.overlay_map().expect("painted");
    let moved = state.overlay_batches(map, Rect::new(0.0, 0.0, 100.0, 100.0));
    assert!(!Arc::ptr_eq(&first, &moved), "a new clip tessellates again");

    let ahead = batches(&mut state);
    assert!(state.grab_overlay_handle(OVERLAY, &Arc::from("knob"), [50.0, 25.0]));
    settle(&mut state);
    let after = batches(&mut state);
    assert!(
        !Arc::ptr_eq(&ahead, &after),
        "a new scene tessellates again"
    );
}
