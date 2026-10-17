//! Declarative viewer-overlay scenes, shapes, and drag events for native plugins.
//!
//! Serializes to the JSON schema consumed by Aria's `overlay_scene`,
//! `overlay_event`, and `register_viewer_overlay` host callbacks.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

fn hairline() -> f32 {
    1.0
}

fn text_size() -> f32 {
    12.0
}

/// Stroke or fill color for an [`OverlayShape`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayColor {
    /// Theme accent color.
    #[default]
    Accent,
    /// Primary foreground text color.
    Text,
    /// Secondary muted foreground color.
    Muted,
    /// Positive / ok status color.
    Success,
    /// Caution / warning status color.
    Warning,
    /// Error / danger status color.
    Danger,
    /// Explicit straight-alpha sRGB color.
    Rgba([u8; 4]),
}

/// Visual glyph drawn for an [`OverlayShape::Handle`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleLook {
    /// Filled circular handle.
    #[default]
    Dot,
    /// Outlined square handle.
    Square,
    /// Crosshair handle.
    Cross,
}

/// One vector primitive or interactive handle in an [`OverlayScene`].
///
/// Coordinates are in sequence frame pixels (`(0, 0)` at the top-left of the
/// video frame, `(width, height)` at the bottom-right). Stroke widths, text
/// sizes, and handle hit targets stay in screen pixels regardless of Viewer zoom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverlayShape {
    /// Straight line segment between `from` and `to`.
    Line {
        /// Start point in sequence frame pixels.
        from: [f32; 2],
        /// End point in sequence frame pixels.
        to: [f32; 2],
        /// Stroke color.
        #[serde(default)]
        color: OverlayColor,
        /// Stroke width in screen pixels.
        #[serde(default = "hairline")]
        width: f32,
    },
    /// Axis-aligned rectangle at `origin` with dimensions `size`.
    Rect {
        /// Top-left corner in sequence frame pixels.
        origin: [f32; 2],
        /// Width and height in sequence frame pixels (`>= 0`).
        size: [f32; 2],
        /// Stroke color.
        #[serde(default)]
        color: OverlayColor,
        /// Stroke width in screen pixels (`0` for fill-only).
        #[serde(default = "hairline")]
        width: f32,
        /// Optional interior fill color.
        #[serde(default)]
        fill: Option<OverlayColor>,
    },
    /// Axis-aligned ellipse centered at `center` with `radii`.
    Ellipse {
        /// Center in sequence frame pixels.
        center: [f32; 2],
        /// Horizontal and vertical radii in sequence frame pixels (`>= 0`).
        radii: [f32; 2],
        /// Stroke color.
        #[serde(default)]
        color: OverlayColor,
        /// Stroke width in screen pixels (`0` for fill-only).
        #[serde(default = "hairline")]
        width: f32,
        /// Optional interior fill color.
        #[serde(default)]
        fill: Option<OverlayColor>,
    },
    /// Polyline or closed polygon through `points` (at least 2 points).
    Path {
        /// Vertices in sequence frame pixels.
        points: Vec<[f32; 2]>,
        /// Whether to connect the last vertex back to the first.
        #[serde(default)]
        closed: bool,
        /// Stroke color.
        #[serde(default)]
        color: OverlayColor,
        /// Stroke width in screen pixels.
        #[serde(default = "hairline")]
        width: f32,
    },
    /// Text label anchored at `at`.
    Text {
        /// Anchor point in sequence frame pixels.
        at: [f32; 2],
        /// UTF-8 label text.
        text: Arc<str>,
        /// Text color.
        #[serde(default)]
        color: OverlayColor,
        /// Font size in screen pixels.
        #[serde(default = "text_size")]
        size: f32,
    },
    /// Draggable handle at `at`, identified by `id` within the scene.
    Handle {
        /// Unique handle identifier within the scene.
        id: Arc<str>,
        /// Position in sequence frame pixels.
        at: [f32; 2],
        /// Handle color.
        #[serde(default)]
        color: OverlayColor,
        /// Handle glyph style.
        #[serde(default)]
        look: HandleLook,
    },
}

/// Complete scene returned by [`Plugin::overlay_scene`](crate::Plugin::overlay_scene).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OverlayScene {
    /// Shapes drawn back-to-front over the sequence frame.
    pub shapes: Vec<OverlayShape>,
}

impl OverlayScene {
    /// Construct a scene from `shapes`.
    #[must_use]
    pub fn new(shapes: Vec<OverlayShape>) -> Self {
        Self { shapes }
    }

    /// Whether the scene contains a handle with `id`.
    #[must_use]
    pub fn has_handle(&self, id: &str) -> bool {
        self.shapes
            .iter()
            .any(|s| matches!(s, OverlayShape::Handle { id: h, .. } if &**h == id))
    }
}

/// Phase of a pointer drag on an [`OverlayShape::Handle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DragPhase {
    /// Pointer button pressed on the handle.
    Start,
    /// Pointer moved while dragging the handle.
    Move,
    /// Pointer button released.
    End,
}

/// User interaction on a viewer overlay, delivered to
/// [`Plugin::overlay_event`](crate::Plugin::overlay_event).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverlayEvent {
    /// Drag interaction on `handle` at frame-pixel coordinates `at`.
    Drag {
        /// Target handle id.
        handle: Arc<str>,
        /// Drag lifecycle phase.
        phase: DragPhase,
        /// Pointer position in sequence frame pixels.
        at: [f32; 2],
    },
}

/// Registration metadata for a viewer overlay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlaySpec {
    /// Unique overlay identifier.
    pub id: String,
    /// Display title shown in the Viewer overlays menu.
    pub title: String,
}

impl OverlaySpec {
    /// Construct a viewer overlay specification.
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_scene_and_events_round_trip_json() {
        let scene = OverlayScene::new(vec![
            OverlayShape::Line {
                from: [0.0, 0.0],
                to: [1920.0, 1080.0],
                color: OverlayColor::Accent,
                width: 1.5,
            },
            OverlayShape::Rect {
                origin: [96.0, 54.0],
                size: [1728.0, 972.0],
                color: OverlayColor::Muted,
                width: 1.0,
                fill: Some(OverlayColor::Rgba([0, 0, 0, 64])),
            },
            OverlayShape::Ellipse {
                center: [960.0, 540.0],
                radii: [200.0, 100.0],
                color: OverlayColor::Success,
                width: 2.0,
                fill: None,
            },
            OverlayShape::Path {
                points: vec![[10.0, 20.0], [30.0, 40.0], [50.0, 20.0]],
                closed: true,
                color: OverlayColor::Warning,
                width: 1.0,
            },
            OverlayShape::Text {
                at: [100.0, 100.0],
                text: Arc::from("Title Safe"),
                color: OverlayColor::Text,
                size: 14.0,
            },
            OverlayShape::Handle {
                id: Arc::from("focus"),
                at: [960.0, 540.0],
                color: OverlayColor::Danger,
                look: HandleLook::Cross,
            },
        ]);
        assert!(scene.has_handle("focus"));
        assert!(!scene.has_handle("other"));

        let json = serde_json::to_string(&scene).unwrap();
        let decoded: OverlayScene = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, scene);

        for phase in [DragPhase::Start, DragPhase::Move, DragPhase::End] {
            let ev = OverlayEvent::Drag {
                handle: Arc::from("focus"),
                phase,
                at: [640.0, 360.0],
            };
            let ev_json = serde_json::to_string(&ev).unwrap();
            let ev_back: OverlayEvent = serde_json::from_str(&ev_json).unwrap();
            assert_eq!(ev_back, ev);
        }
    }
}
