//! Viewer overlays: drawing and draggable handles a plugin puts over the
//! Viewer frame.
//!
//! A plugin returns an [`OverlayScene`] of shapes placed in canvas pixels: the
//! sequence frame's own coordinates, origin at its top left, `x` right and `y`
//! down. The host maps them onto the frame however the Viewer is fitted,
//! zoomed, or panned, and draws nothing over a source frame. Stroke widths and
//! text sizes are screen pixels, so a line stays one pixel wide at any zoom.
//!
//! A [`OverlayShape::Handle`] is draggable. The host reports a press, each
//! pointer step, and the release as an [`OverlayEvent`] in canvas pixels, and
//! the next scene the plugin returns is what the Viewer shows. Pointer steps
//! queued while the plugin is still handling an earlier one merge into the
//! latest, so a slow handler sees fewer steps rather than falling behind.
//!
//! A scene is built off the drawing thread, as a declared
//! panel's tree is. A plugin whose scene follows the playhead or the selection
//! subscribes to [`PlayheadMoved`](crate::PlayheadMoved) or
//! [`SelectionChanged`](crate::SelectionChanged) and calls
//! [`PluginContext::refresh_viewer_overlay`].

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::context::PluginContext;
use crate::error::PluginError;
use crate::live::{Access, Live, LiveKind};
use crate::registration::{Contribution, ContributionKind, Registration};
use crate::ui_tree::{MAX_TEXT_BYTES, MAX_TREE_TEXT_BYTES, MAX_WIDGET_ID_BYTES};

/// How many shapes a scene may hold.
pub const MAX_SHAPES: usize = 4096;
/// How many points every path in a scene may hold together.
pub const MAX_PATH_POINTS: usize = 65_536;
/// The farthest a coordinate may sit from the canvas origin, in canvas pixels.
pub const MAX_COORDINATE: f32 = 1_000_000.0;
/// The widest stroke, in screen pixels.
pub const MAX_STROKE: f32 = 32.0;
/// The smallest and largest text size, in screen pixels.
pub const TEXT_SIZE_RANGE: [f32; 2] = [6.0, 96.0];

/// The colour a shape is drawn in. The named colours follow the theme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayColor {
    #[default]
    Accent,
    Text,
    Muted,
    Success,
    Warning,
    Danger,
    /// Straight-alpha sRGB.
    Rgba([u8; 4]),
}

/// How a handle is drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleLook {
    #[default]
    Dot,
    Square,
    Cross,
}

fn hairline() -> f32 {
    1.0
}

fn text_size() -> f32 {
    12.0
}

/// One shape of a scene. Points are canvas pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverlayShape {
    Line {
        from: [f32; 2],
        to: [f32; 2],
        #[serde(default)]
        color: OverlayColor,
        #[serde(default = "hairline")]
        width: f32,
    },
    /// An axis-aligned rectangle from its top-left corner.
    Rect {
        origin: [f32; 2],
        size: [f32; 2],
        #[serde(default)]
        color: OverlayColor,
        #[serde(default = "hairline")]
        width: f32,
        #[serde(default)]
        fill: Option<OverlayColor>,
    },
    Ellipse {
        center: [f32; 2],
        radii: [f32; 2],
        #[serde(default)]
        color: OverlayColor,
        #[serde(default = "hairline")]
        width: f32,
        #[serde(default)]
        fill: Option<OverlayColor>,
    },
    /// Straight segments through `points`, back to the first when `closed`.
    Path {
        points: Vec<[f32; 2]>,
        #[serde(default)]
        closed: bool,
        #[serde(default)]
        color: OverlayColor,
        #[serde(default = "hairline")]
        width: f32,
    },
    /// One line of text with its top-left corner at `at`.
    Text {
        at: [f32; 2],
        text: Arc<str>,
        #[serde(default)]
        color: OverlayColor,
        #[serde(default = "text_size")]
        size: f32,
    },
    /// A draggable point. `id` is unique within the scene and comes back in
    /// each [`OverlayEvent`] the handle produces.
    Handle {
        id: Arc<str>,
        at: [f32; 2],
        #[serde(default)]
        color: OverlayColor,
        #[serde(default)]
        look: HandleLook,
    },
}

/// What an overlay draws.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OverlayScene {
    pub shapes: Vec<OverlayShape>,
}

impl OverlayScene {
    /// Whether the scene has a handle `id`.
    pub fn has_handle(&self, id: &str) -> bool {
        self.shapes
            .iter()
            .any(|shape| matches!(shape, OverlayShape::Handle { id: held, .. } if &**held == id))
    }

    /// Why the host will not draw this scene, if it will not.
    pub fn check(&self) -> Result<(), SceneError> {
        if self.shapes.len() > MAX_SHAPES {
            return Err(SceneError::TooManyShapes(self.shapes.len()));
        }
        let mut points = 0_usize;
        let mut text = 0_usize;
        let mut handles: Vec<&str> = Vec::new();
        for (index, shape) in self.shapes.iter().enumerate() {
            let at = |problem: Problem| SceneError::Shape { index, problem };
            match shape {
                OverlayShape::Line {
                    from, to, width, ..
                } => {
                    check_point(*from).map_err(at)?;
                    check_point(*to).map_err(at)?;
                    check_stroke(*width).map_err(at)?;
                }
                OverlayShape::Rect {
                    origin,
                    size,
                    width,
                    ..
                } => {
                    check_point(*origin).map_err(at)?;
                    check_extent(*size).map_err(at)?;
                    check_stroke(*width).map_err(at)?;
                }
                OverlayShape::Ellipse {
                    center,
                    radii,
                    width,
                    ..
                } => {
                    check_point(*center).map_err(at)?;
                    check_extent(*radii).map_err(at)?;
                    check_stroke(*width).map_err(at)?;
                }
                OverlayShape::Path {
                    points: path,
                    width,
                    ..
                } => {
                    if path.len() < 2 {
                        return Err(at(Problem::ShortPath(path.len())));
                    }
                    points += path.len();
                    if points > MAX_PATH_POINTS {
                        return Err(SceneError::TooManyPoints(points));
                    }
                    for point in path {
                        check_point(*point).map_err(at)?;
                    }
                    check_stroke(*width).map_err(at)?;
                }
                OverlayShape::Text {
                    at: origin,
                    text: line,
                    size,
                    ..
                } => {
                    check_point(*origin).map_err(at)?;
                    if line.len() > MAX_TEXT_BYTES {
                        return Err(at(Problem::LongText(line.len())));
                    }
                    if line.contains('\n') {
                        return Err(at(Problem::MultilineText));
                    }
                    let [low, high] = TEXT_SIZE_RANGE;
                    if !(low..=high).contains(size) {
                        return Err(at(Problem::TextSize(*size)));
                    }
                    text += line.len();
                    if text > MAX_TREE_TEXT_BYTES {
                        return Err(SceneError::TooMuchText(text));
                    }
                }
                OverlayShape::Handle { id, at: point, .. } => {
                    if id.is_empty()
                        || id.len() > MAX_WIDGET_ID_BYTES
                        || id.chars().any(char::is_control)
                    {
                        return Err(at(Problem::HandleId(id.len())));
                    }
                    if handles.contains(&&**id) {
                        return Err(SceneError::DuplicateHandle(id.to_string()));
                    }
                    handles.push(id);
                    check_point(*point).map_err(at)?;
                }
            }
        }
        Ok(())
    }
}

fn check_point([x, y]: [f32; 2]) -> Result<(), Problem> {
    let inside = |v: f32| v.is_finite() && v.abs() <= MAX_COORDINATE;
    if inside(x) && inside(y) {
        Ok(())
    } else {
        Err(Problem::Point([x, y]))
    }
}

fn check_extent([w, h]: [f32; 2]) -> Result<(), Problem> {
    let fits = |v: f32| v.is_finite() && (0.0..=MAX_COORDINATE * 2.0).contains(&v);
    if fits(w) && fits(h) {
        Ok(())
    } else {
        Err(Problem::Extent([w, h]))
    }
}

fn check_stroke(width: f32) -> Result<(), Problem> {
    if width.is_finite() && width > 0.0 && width <= MAX_STROKE {
        Ok(())
    } else {
        Err(Problem::Stroke(width))
    }
}

/// What is wrong with one shape.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum Problem {
    #[error("the point {0:?} is not finite or lies beyond {MAX_COORDINATE} canvas pixels")]
    Point([f32; 2]),
    #[error("the size {0:?} is negative, not finite, or too large")]
    Extent([f32; 2]),
    #[error("the stroke width {0} is not above 0 and at most {MAX_STROKE}")]
    Stroke(f32),
    #[error("a path needs 2 points, not {0}")]
    ShortPath(usize),
    #[error("the text is {0} bytes, over {MAX_TEXT_BYTES}")]
    LongText(usize),
    #[error("the text holds a line break; draw one text shape per line")]
    MultilineText,
    #[error("the text size {0} is outside 6 to 96")]
    TextSize(f32),
    #[error(
        "a handle id is {0} bytes or holds a control character; it needs 1 to {MAX_WIDGET_ID_BYTES} bytes"
    )]
    HandleId(usize),
}

/// Why a host refused a scene.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SceneError {
    #[error("the scene holds {0} shapes, over {MAX_SHAPES}")]
    TooManyShapes(usize),
    #[error("the scene's paths hold {0} points, over {MAX_PATH_POINTS}")]
    TooManyPoints(usize),
    #[error("the scene holds {0} bytes of text, over {MAX_TREE_TEXT_BYTES}")]
    TooMuchText(usize),
    #[error("the handle id {0} is used twice")]
    DuplicateHandle(String),
    #[error("shape {index}: {problem}")]
    Shape { index: usize, problem: Problem },
}

/// Where a drag is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DragPhase {
    /// The handle was pressed.
    Start,
    /// The pointer moved with the handle held.
    Move,
    /// The handle was released.
    End,
}

/// What the user did to a handle of an overlay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverlayEvent {
    /// `at` is the pointer in canvas pixels, and may lie off the frame.
    Drag {
        handle: Arc<str>,
        phase: DragPhase,
        at: [f32; 2],
    },
}

impl OverlayEvent {
    /// Whether a queued `waiting` event may be replaced by this one: both are
    /// pointer steps of the same handle.
    fn merges(&self, waiting: &Self) -> bool {
        match (self, waiting) {
            (
                Self::Drag {
                    handle,
                    phase: DragPhase::Move,
                    ..
                },
                Self::Drag {
                    handle: held,
                    phase: DragPhase::Move,
                    ..
                },
            ) => handle == held,
            _ => false,
        }
    }
}

/// An overlay's id and what it is called where overlays are listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlaySpec {
    /// Unique across the context. By convention `<plugin id>.<overlay>`.
    pub id: String,
    pub title: Arc<str>,
}

/// An overlay's code: the scene it draws, and what it does with a drag.
pub trait ViewerOverlay: Send + Sync {
    /// The scene to draw. `ctx` is the registering plugin's view of the
    /// context.
    fn scene(&self, ctx: &PluginContext) -> OverlayScene;

    /// A handle in the scene was dragged. An `Err` is shown in the status bar
    /// as written. The scene is built again afterwards either way.
    fn event(&self, ctx: &PluginContext, event: &OverlayEvent) -> Result<(), String>;
}

/// What a host draws for one overlay.
#[derive(Debug, Clone)]
pub struct OverlayState {
    pub spec: Arc<OverlaySpec>,
    /// The latest scene that built and passed [`OverlayScene::check`], or
    /// `None` before the first.
    pub scene: Option<Arc<OverlayScene>>,
    /// Why the latest build was refused. Cleared by the next build that
    /// passes; the scene before it stays in `scene`.
    pub refused: Option<Arc<str>>,
    /// The error the latest event's handler returned. Cleared by the next
    /// event the handler accepts.
    pub event_error: Option<Arc<str>>,
}

/// Why an overlay call did not reach the overlay.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OverlayError {
    #[error("no viewer overlay is registered as {0}")]
    Unknown(String),
    #[error("viewer overlay {0} belongs to another plugin")]
    NotOwned(String),
    #[error("viewer overlay {0} has not drawn a scene yet")]
    NotDrawn(String),
    #[error("viewer overlay {overlay} has no handle {handle}")]
    NoHandle { overlay: String, handle: String },
    #[error("viewer overlay {0}: the pointer is not a finite point")]
    BadPoint(String),
}

/// Viewer overlays, for [`crate::live`].
pub(crate) enum OverlayKind {}

impl LiveKind for OverlayKind {
    type Spec = OverlaySpec;
    type View = dyn ViewerOverlay;
    type Content = OverlayScene;
    type Event = OverlayEvent;

    const NOUN: &'static str = "viewer overlay";
    const CONTENT: &'static str = "a scene";

    fn id(spec: &OverlaySpec) -> &str {
        &spec.id
    }

    fn build(view: &dyn ViewerOverlay, ctx: &PluginContext) -> OverlayScene {
        view.scene(ctx)
    }

    fn handle(
        view: &dyn ViewerOverlay,
        ctx: &PluginContext,
        event: &OverlayEvent,
    ) -> Result<(), String> {
        view.event(ctx, event)
    }

    fn check(scene: &OverlayScene) -> Result<(), String> {
        scene.check().map_err(|err| err.to_string())
    }
}

/// The viewer overlays registered on one context, in registration order.
pub(crate) type Overlays = Live<OverlayKind>;

fn refused(reason: String) -> PluginError {
    PluginError::OverlayRefused(reason)
}

fn access(id: &str, denied: Access) -> OverlayError {
    match denied {
        Access::Unknown => OverlayError::Unknown(id.to_owned()),
        Access::NotOwned => OverlayError::NotOwned(id.to_owned()),
    }
}

impl PluginContext {
    /// Draw `overlay`'s scene over the Viewer frame.
    ///
    /// Held on the terms of [`register_importer`](Self::register_importer): a
    /// registration made during mount drops when the plugin unmounts. Refused
    /// when the id is empty or taken, or the title is empty. Nothing of
    /// `overlay` runs until a host first reads it.
    pub fn register_viewer_overlay(
        &self,
        spec: OverlaySpec,
        overlay: impl ViewerOverlay + 'static,
    ) -> Result<Registration, PluginError> {
        if spec.id.trim().is_empty() {
            return Err(refused("a viewer overlay needs an id".into()));
        }
        if spec.title.trim().is_empty() {
            return Err(refused(format!(
                "{}: a viewer overlay needs a title",
                spec.id
            )));
        }
        let contribution = Contribution::new(ContributionKind::Overlay, spec.id.clone());
        let overlay: Arc<dyn ViewerOverlay> = Arc::new(overlay);
        let unregister = self
            .overlay_registry()
            .insert(self, spec, overlay)
            .map_err(refused)?;
        Ok(self.retain_if_mounting(Registration::holding(contribution, unregister)))
    }

    /// Every registered viewer overlay, in registration order.
    pub fn viewer_overlays(&self) -> Vec<Arc<OverlaySpec>> {
        self.overlay_registry().specs()
    }

    /// What overlay `id` shows now. Never waits: the first read queues the
    /// first build, and until it lands `scene` is `None`.
    pub fn viewer_overlay(&self, id: &str) -> Option<OverlayState> {
        let shown = self.overlay_registry().read(self, id)?;
        Some(OverlayState {
            spec: shown.spec,
            scene: shown.content,
            refused: shown.refused,
            event_error: shown.event_error,
        })
    }

    /// Deliver a drag to overlay `id`, then build its scene again. Returns
    /// once the event is queued.
    ///
    /// A press must name a handle of the scene the overlay shows now; a step
    /// or a release continues the press it follows, so a handle the plugin
    /// dropped mid-drag still hears its release. `at` must be finite. A plugin
    /// may send events only to its own overlays.
    pub fn send_overlay_event(&self, id: &str, event: OverlayEvent) -> Result<(), OverlayError> {
        let registry = self.overlay_registry();
        let entry = registry
            .owned(self, id)
            .map_err(|denied| access(id, denied))?;
        let scene = entry
            .content()
            .ok_or_else(|| OverlayError::NotDrawn(id.to_owned()))?;
        let OverlayEvent::Drag { handle, phase, at } = &event;
        if !(at[0].is_finite() && at[1].is_finite()) {
            return Err(OverlayError::BadPoint(id.to_owned()));
        }
        if *phase == DragPhase::Start && !scene.has_handle(handle) {
            return Err(OverlayError::NoHandle {
                overlay: id.to_owned(),
                handle: handle.to_string(),
            });
        }
        registry.send(self, &entry, event.clone(), |waiting| event.merges(waiting));
        Ok(())
    }

    /// Build overlay `id`'s scene again, for state that changed outside a
    /// drag: the playhead moved, the selection changed. Refreshes queued
    /// before the build starts share one build. A plugin may refresh only its
    /// own overlays.
    pub fn refresh_viewer_overlay(&self, id: &str) -> Result<(), OverlayError> {
        let registry = self.overlay_registry();
        let entry = registry
            .owned(self, id)
            .map_err(|denied| access(id, denied))?;
        registry.refresh(self, &entry);
        Ok(())
    }

    /// A number that grows whenever any overlay's state, or the set of
    /// overlays, changes. A host compares it with the one it drew.
    pub fn overlay_revision(&self) -> u64 {
        self.overlay_registry().revision()
    }

    /// Set what runs, on whatever thread made the change, whenever
    /// [`overlay_revision`](Self::overlay_revision) grows.
    pub fn set_overlay_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        self.overlay_registry().set_waker(Arc::new(waker));
    }
}

#[cfg(test)]
#[path = "viewer_overlay_tests.rs"]
mod tests;
