//! Plugin viewer overlays as the window holds them.
//!
//! A plugin draws over the Viewer frame through
//! [`aria_plugin::PluginContext::register_viewer_overlay`]: an
//! [`aria_plugin::OverlayScene`] of shapes in canvas pixels, built on the event delivery
//! thread. This module holds the last state the window read of every overlay,
//! the handle the pointer holds, and the scenes tessellated for the rectangle
//! the canvas was last drawn at. [`CanvasMap`] converts between the window
//! pixels a pointer event carries and the canvas pixels of a scene and of an
//! [`OverlayEvent`].

use std::sync::Arc;

use aria_plugin::{DragPhase, OverlayColor, OverlayEvent, OverlayShape, OverlayState};
use gpui::{Context, FillOptions, Path, PathBuilder, PathStyle, Pixels, Point, point, px};

use super::AppState;
use super::canvas_overlay::Rect;

/// The most points one tessellated path holds. gpui tessellates into 16-bit
/// vertex indices and a stroke emits several vertices per point, so a run of
/// shapes is cut into paths of at most this many points.
pub const BATCH_POINTS: usize = 4096;

/// The flattening tolerance lyon applies, in window pixels. An ellipse is
/// counted toward [`BATCH_POINTS`] by the segments it flattens to.
const TOLERANCE: f32 = 0.1;

/// Canvas pixels to window pixels and back, for one drawn canvas rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasMap {
    rect: Rect,
    scale: [f32; 2],
}

impl CanvasMap {
    /// The map for a canvas `canvas` pixels in size drawn at `rect`. `None`
    /// when either has no area.
    pub fn new(rect: Rect, canvas: [u32; 2]) -> Option<Self> {
        let [width, height] = canvas;
        if width == 0 || height == 0 || !(rect.w > 0.0 && rect.h > 0.0) {
            return None;
        }
        Some(Self {
            rect,
            scale: [rect.w / width as f32, rect.h / height as f32],
        })
    }

    /// The window point canvas point `[x, y]` is drawn at.
    pub fn to_window(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [
            self.rect.x + x * self.scale[0],
            self.rect.y + y * self.scale[1],
        ]
    }

    /// The canvas point under window point `[x, y]`. Not clamped: a drag may
    /// leave the frame.
    pub fn to_canvas(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [
            (x - self.rect.x) / self.scale[0],
            (y - self.rect.y) / self.scale[1],
        ]
    }

    /// Window pixels per canvas pixel, across and down.
    pub fn scale(&self) -> [f32; 2] {
        self.scale
    }
}

/// How a tessellated path is painted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Paint {
    Fill(OverlayColor),
    /// Stroked this many window pixels wide.
    Stroke(OverlayColor, f32),
}

/// Consecutive shapes painted the same way, tessellated as one path.
#[derive(Debug, Clone)]
pub struct Batch {
    pub paint: Paint,
    pub path: Path<Pixels>,
}

/// The handle the pointer holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayGrab {
    pub overlay: Arc<str>,
    pub handle: Arc<str>,
}

/// The tessellated scenes and what they were tessellated for.
#[derive(Debug)]
struct Drawn {
    revision: u64,
    map: CanvasMap,
    clip: Rect,
    batches: Arc<[Batch]>,
}

/// Every viewer overlay's last read state.
#[derive(Debug, Default)]
pub struct PluginOverlays {
    /// The overlay revision `shown` was read at; `None` before the first read.
    revision: Option<u64>,
    shown: Arc<[OverlayState]>,
    pub(crate) grab: Option<OverlayGrab>,
    drawn: Option<Drawn>,
}

impl PluginOverlays {
    /// Every overlay, in registration order. Later overlays draw above
    /// earlier ones.
    pub fn shown(&self) -> &Arc<[OverlayState]> {
        &self.shown
    }

    /// Whether any overlay has a scene to draw.
    pub fn any_scene(&self) -> bool {
        self.shown.iter().any(|state| state.scene.is_some())
    }
}

impl AppState {
    /// Read the viewer overlays again whenever their revision moves, the way
    /// [`watch_plugin_panels`](Self::watch_plugin_panels) reads panels.
    pub fn watch_plugin_overlays(&mut self, cx: &mut Context<Self>) {
        let (wake, wakes) = crate::handoff::wakes::<()>();
        self.ctx.set_overlay_waker(move || {
            wake.wake(());
        });
        cx.spawn(async move |this, cx| {
            while let Some(()) = wakes.next_from_a_thread(cx.background_executor()).await {
                while wakes.try_next().is_some() {}
                if this
                    .update(cx, |state, cx| {
                        if state.sync_plugin_overlays() {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
        if self.sync_plugin_overlays() {
            cx.notify();
        }
    }

    /// Read every overlay again when the revision moved since the last read.
    /// Returns whether it moved.
    ///
    /// A scene the host refused, and an error a drag handler returned, reach
    /// the status bar once each. A held handle whose overlay is gone is let
    /// go. The first read of an overlay queues its first build, which moves
    /// the revision when it lands.
    pub fn sync_plugin_overlays(&mut self) -> bool {
        // Read before the overlays: a build landing between the two moves the
        // revision again, and the waker brings this back for it.
        let revision = self.ctx.overlay_revision();
        if self.plugin_overlays.revision == Some(revision) {
            return false;
        }
        let shown: Arc<[OverlayState]> = self
            .ctx
            .viewer_overlays()
            .iter()
            .filter_map(|spec| self.ctx.viewer_overlay(&spec.id))
            .collect();
        let before = std::mem::replace(&mut self.plugin_overlays.shown, Arc::clone(&shown));
        self.plugin_overlays.revision = Some(revision);
        if let Some(grab) = &self.plugin_overlays.grab
            && !shown.iter().any(|state| *state.spec.id == *grab.overlay)
        {
            self.plugin_overlays.grab = None;
        }
        for state in shown.iter() {
            let earlier = before.iter().find(|old| old.spec.id == state.spec.id);
            let refused = fresh(&state.refused, earlier.map(|old| &old.refused));
            let failed = fresh(&state.event_error, earlier.map(|old| &old.event_error));
            for text in [refused, failed].into_iter().flatten() {
                self.report_error(format!("{}: {text}", state.spec.title));
            }
        }
        true
    }

    /// Where the canvas was last drawn, as a map, or `None` before the first
    /// paint.
    pub fn overlay_map(&self) -> Option<CanvasMap> {
        CanvasMap::new(self.overlay_frame.canvas?, self.canvas_size())
    }

    /// Press handle `handle` of overlay `overlay` at window point `at`.
    /// Returns whether the press was sent; a refusal is reported.
    pub fn grab_overlay_handle(&mut self, overlay: &str, handle: &Arc<str>, at: [f32; 2]) -> bool {
        let Some(map) = self.overlay_map() else {
            return false;
        };
        let event = OverlayEvent::Drag {
            handle: Arc::clone(handle),
            phase: DragPhase::Start,
            at: map.to_canvas(at),
        };
        if let Err(err) = self.ctx.send_overlay_event(overlay, event) {
            self.report_error(err.to_string());
            return false;
        }
        self.plugin_overlays.grab = Some(OverlayGrab {
            overlay: Arc::from(overlay),
            handle: Arc::clone(handle),
        });
        true
    }

    /// The pointer moved to window point `at` with a handle held. Returns
    /// whether a handle is held.
    pub fn carry_overlay_handle(&mut self, at: [f32; 2]) -> bool {
        self.drag_overlay_handle(DragPhase::Move, at)
    }

    /// The pointer let go at window point `at`. Returns whether a handle was
    /// held.
    pub fn release_overlay_handle(&mut self, at: [f32; 2]) -> bool {
        let held = self.drag_overlay_handle(DragPhase::End, at);
        self.plugin_overlays.grab = None;
        held
    }

    fn drag_overlay_handle(&mut self, phase: DragPhase, at: [f32; 2]) -> bool {
        let Some(grab) = self.plugin_overlays.grab.clone() else {
            return false;
        };
        let Some(map) = self.overlay_map() else {
            return true;
        };
        let event = OverlayEvent::Drag {
            handle: grab.handle,
            phase,
            at: map.to_canvas(at),
        };
        if let Err(err) = self.ctx.send_overlay_event(&grab.overlay, event) {
            self.plugin_overlays.grab = None;
            self.report_error(err.to_string());
        }
        true
    }

    /// The lines, rectangles, ellipses and paths of every scene, tessellated
    /// for a canvas drawn under `map` and clipped to `clip`, from the cache
    /// while neither they nor the scenes moved. A shape the tessellator
    /// cannot draw is reported.
    pub fn overlay_batches(&mut self, map: CanvasMap, clip: Rect) -> Arc<[Batch]> {
        let revision = self.plugin_overlays.revision.unwrap_or(0);
        if let Some(drawn) = &self.plugin_overlays.drawn
            && drawn.revision == revision
            && drawn.map == map
            && drawn.clip == clip
        {
            return Arc::clone(&drawn.batches);
        }
        let mut out = Vec::new();
        let mut failed = Vec::new();
        for state in self.plugin_overlays.shown.iter() {
            let Some(scene) = &state.scene else {
                continue;
            };
            let mut tessellator = Tessellator::new(&map, clip);
            for shape in &scene.shapes {
                tessellator.shape(shape);
            }
            let (batches, lost) = tessellator.finish();
            out.extend(batches);
            if lost > 0 {
                failed.push(format!(
                    "{}: {lost} shapes could not be tessellated and are not drawn",
                    state.spec.title
                ));
            }
        }
        for message in failed {
            self.report_error(message);
        }
        let batches: Arc<[Batch]> = out.into();
        self.plugin_overlays.drawn = Some(Drawn {
            revision,
            map,
            clip,
            batches: Arc::clone(&batches),
        });
        batches
    }
}

/// `now` when it is a message the last read did not show. Compared by
/// address: the context stores each refusal and each handler error anew, so
/// the same text failing twice is reported twice.
fn fresh(now: &Option<Arc<str>>, then: Option<&Option<Arc<str>>>) -> Option<Arc<str>> {
    let text = now.as_ref()?;
    match then {
        Some(Some(old)) if Arc::ptr_eq(old, text) => None,
        _ => Some(Arc::clone(text)),
    }
}

/// The window point canvas point `canvas` is drawn at.
fn window_point(map: &CanvasMap, canvas: [f32; 2]) -> Point<Pixels> {
    let [x, y] = map.to_window(canvas);
    point(px(x), px(y))
}

/// Shapes being gathered into one path.
struct Run {
    paint: Paint,
    builder: PathBuilder,
    points: usize,
    shapes: usize,
}

/// One scene's shapes cut into [`Batch`]es. Consecutive shapes painted the
/// same way share a path, so a scene of guides in one colour paints as one
/// path, and painting order follows the scene.
struct Tessellator<'a> {
    map: &'a CanvasMap,
    clip: Rect,
    run: Option<Run>,
    out: Vec<Batch>,
    lost: usize,
}

impl<'a> Tessellator<'a> {
    fn new(map: &'a CanvasMap, clip: Rect) -> Self {
        Self {
            map,
            clip,
            run: None,
            out: Vec::new(),
            lost: 0,
        }
    }

    /// Whether a window-space box `[left, top, right, bottom]` grown by `pad`
    /// reaches the clip.
    fn visible(&self, [left, top, right, bottom]: [f32; 4], pad: f32) -> bool {
        left - pad <= self.clip.x + self.clip.w
            && right + pad >= self.clip.x
            && top - pad <= self.clip.y + self.clip.h
            && bottom + pad >= self.clip.y
    }

    /// The path for `paint`, continuing the current run when it is painted the
    /// same way and has room for `points` more.
    fn begin(&mut self, paint: Paint, points: usize) -> &mut PathBuilder {
        let fits = self
            .run
            .as_ref()
            .is_some_and(|run| run.paint == paint && run.points + points <= BATCH_POINTS);
        if !fits {
            self.flush();
        }
        let run = self.run.get_or_insert_with(|| Run {
            paint,
            builder: match paint {
                // Non-zero, so two overlapping fills of one run add up rather
                // than cancel; every fill below winds clockwise.
                Paint::Fill(_) => {
                    PathBuilder::fill().with_style(PathStyle::Fill(FillOptions::non_zero()))
                }
                Paint::Stroke(_, width) => PathBuilder::stroke(px(width)),
            },
            points: 0,
            shapes: 0,
        });
        run.points += points;
        run.shapes += 1;
        &mut run.builder
    }

    fn flush(&mut self) {
        let Some(run) = self.run.take() else {
            return;
        };
        match run.builder.build() {
            Ok(path) => self.out.push(Batch {
                paint: run.paint,
                path,
            }),
            Err(err) => {
                tracing::warn!(error = %err, "a viewer overlay path did not tessellate");
                self.lost += run.shapes;
            }
        }
    }

    fn shape(&mut self, shape: &OverlayShape) {
        match shape {
            OverlayShape::Line {
                from,
                to,
                color,
                width,
            } => {
                let (a, b) = (self.map.to_window(*from), self.map.to_window(*to));
                let bounds = [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[0].max(b[0]),
                    a[1].max(b[1]),
                ];
                if !self.visible(bounds, *width) {
                    return;
                }
                let path = self.begin(Paint::Stroke(*color, *width), 2);
                path.move_to(point(px(a[0]), px(a[1])));
                path.line_to(point(px(b[0]), px(b[1])));
            }
            OverlayShape::Rect {
                origin,
                size,
                color,
                width,
                fill,
            } => {
                let corner = [origin[0] + size[0], origin[1] + size[1]];
                let (a, b) = (self.map.to_window(*origin), self.map.to_window(corner));
                if !self.visible([a[0], a[1], b[0], b[1]], *width) {
                    return;
                }
                let corners = [
                    point(px(a[0]), px(a[1])),
                    point(px(b[0]), px(a[1])),
                    point(px(b[0]), px(b[1])),
                    point(px(a[0]), px(b[1])),
                ];
                let outline = |path: &mut PathBuilder| {
                    path.move_to(corners[0]);
                    for corner in &corners[1..] {
                        path.line_to(*corner);
                    }
                    path.close();
                };
                if let Some(fill) = fill {
                    outline(self.begin(Paint::Fill(*fill), 4));
                }
                outline(self.begin(Paint::Stroke(*color, *width), 4));
            }
            OverlayShape::Ellipse {
                center,
                radii,
                color,
                width,
                fill,
            } => {
                let [sx, sy] = self.map.scale();
                let [cx, cy] = self.map.to_window(*center);
                let (rx, ry) = (radii[0] * sx, radii[1] * sy);
                if !self.visible([cx - rx, cy - ry, cx + rx, cy + ry], *width) {
                    return;
                }
                let points = ellipse_points(rx.max(ry));
                let radius = point(px(rx), px(ry));
                let (right, left) = (point(px(cx + rx), px(cy)), point(px(cx - rx), px(cy)));
                // Right, down through the bottom to the left, and back over
                // the top: clockwise on screen.
                let outline = |path: &mut PathBuilder| {
                    path.move_to(right);
                    path.arc_to(radius, px(0.0), false, true, left);
                    path.arc_to(radius, px(0.0), false, true, right);
                    path.close();
                };
                if let Some(fill) = fill {
                    outline(self.begin(Paint::Fill(*fill), points));
                }
                outline(self.begin(Paint::Stroke(*color, *width), points));
            }
            OverlayShape::Path {
                points,
                closed,
                color,
                width,
            } => {
                let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                for canvas in points {
                    let [x, y] = self.map.to_window(*canvas);
                    bounds = [
                        bounds[0].min(x),
                        bounds[1].min(y),
                        bounds[2].max(x),
                        bounds[3].max(y),
                    ];
                }
                if !self.visible(bounds, *width) {
                    return;
                }
                let map = self.map;
                let paint = Paint::Stroke(*color, *width);
                if points.len() <= BATCH_POINTS {
                    let path = self.begin(paint, points.len());
                    path.move_to(window_point(map, points[0]));
                    for at in &points[1..] {
                        path.line_to(window_point(map, *at));
                    }
                    if *closed {
                        path.close();
                    }
                    return;
                }
                // Longer than one path holds: consecutive pieces share their
                // end point, and a closed path ends with the segment back to
                // its start.
                let mut start = 0;
                while start + 1 < points.len() {
                    let end = (start + BATCH_POINTS).min(points.len());
                    let last = end == points.len();
                    let path = self.begin(paint, end - start + usize::from(last && *closed));
                    path.move_to(window_point(map, points[start]));
                    for at in &points[start + 1..end] {
                        path.line_to(window_point(map, *at));
                    }
                    if last && *closed {
                        path.line_to(window_point(map, points[0]));
                    }
                    start = end - 1;
                }
            }
            // Drawn as elements above the paths: text needs the text system,
            // and a handle takes the pointer.
            OverlayShape::Text { .. } | OverlayShape::Handle { .. } => {}
        }
    }

    fn finish(mut self) -> (Vec<Batch>, usize) {
        self.flush();
        (self.out, self.lost)
    }
}

/// About how many segments lyon flattens an ellipse of radius `radius` window
/// pixels into: a chord of a circle strays at most `TOLERANCE` from its arc
/// when it spans `2 * sqrt(2 * radius * TOLERANCE)`.
fn ellipse_points(radius: f32) -> usize {
    if radius <= TOLERANCE {
        return 4;
    }
    let chord = 2.0 * (2.0 * radius * TOLERANCE).sqrt();
    let around = std::f32::consts::TAU * radius;
    (around / chord).ceil() as usize + 4
}
