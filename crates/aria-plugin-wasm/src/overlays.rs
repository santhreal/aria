//! Bridge between the WIT viewer-overlay records and [`aria_plugin`]'s
//! [`ViewerOverlay`], [`OverlayScene`], [`OverlayShape`], and [`OverlayEvent`].

use std::sync::Arc;

use aria_plugin::{
    DragPhase, HandleLook, OverlayColor, OverlayEvent, OverlayScene, OverlayShape, OverlaySpec,
    PluginContext, ViewerOverlay,
};

use crate::bindings;
use crate::host::WasmPluginShared;

/// [`ViewerOverlay`] backed by a guest WASM component's `overlay-scene` and
/// `overlay-event` exports.
pub(crate) struct WasmOverlay {
    overlay_id: String,
    shared: Arc<WasmPluginShared>,
    scoped_ctx: PluginContext,
}

impl WasmOverlay {
    pub(crate) fn new(
        overlay_id: String,
        shared: Arc<WasmPluginShared>,
        scoped_ctx: PluginContext,
    ) -> Self {
        Self {
            overlay_id,
            shared,
            scoped_ctx,
        }
    }
}

impl ViewerOverlay for WasmOverlay {
    fn scene(&self, _ctx: &PluginContext) -> OverlayScene {
        let shapes = self
            .shared
            .call_overlay_scene(&self.scoped_ctx, &self.overlay_id);
        shapes_from_wit(&shapes)
    }

    fn event(&self, _ctx: &PluginContext, event: &OverlayEvent) -> Result<(), String> {
        let wit_event = overlay_event_to_wit(event);
        self.shared
            .call_overlay_event(&self.scoped_ctx, &self.overlay_id, &wit_event)
    }
}

/// Register a guest-declared viewer overlay on `scoped_ctx`.
pub(crate) fn register_wasm_overlay(
    scoped_ctx: &PluginContext,
    shared: Arc<WasmPluginShared>,
    spec: bindings::OverlaySpec,
) -> Result<(), String> {
    let host_spec = OverlaySpec {
        id: spec.id.clone(),
        title: spec.title.into(),
    };
    let overlay = WasmOverlay::new(spec.id, shared, scoped_ctx.clone());
    scoped_ctx
        .register_viewer_overlay(host_spec, overlay)
        .map(drop)
        .map_err(|err| err.to_string())
}

fn point_from_wit(pt: bindings::Point) -> [f32; 2] {
    [pt.x, pt.y]
}

fn color_from_wit(color: bindings::OverlayColor) -> OverlayColor {
    match color {
        bindings::OverlayColor::Accent => OverlayColor::Accent,
        bindings::OverlayColor::Text => OverlayColor::Text,
        bindings::OverlayColor::Muted => OverlayColor::Muted,
        bindings::OverlayColor::Success => OverlayColor::Success,
        bindings::OverlayColor::Warning => OverlayColor::Warning,
        bindings::OverlayColor::Danger => OverlayColor::Danger,
        bindings::OverlayColor::Rgba((r, g, b, a)) => OverlayColor::Rgba([r, g, b, a]),
    }
}

fn handle_look_from_wit(look: bindings::HandleLook) -> HandleLook {
    match look {
        bindings::HandleLook::Dot => HandleLook::Dot,
        bindings::HandleLook::Square => HandleLook::Square,
        bindings::HandleLook::Cross => HandleLook::Cross,
    }
}

fn shape_from_wit(shape: &bindings::OverlayShape) -> OverlayShape {
    match shape {
        bindings::OverlayShape::Line(line) => OverlayShape::Line {
            from: point_from_wit(line.from),
            to: point_from_wit(line.to),
            color: color_from_wit(line.color),
            width: line.width,
        },
        bindings::OverlayShape::Rect(rect) => OverlayShape::Rect {
            origin: point_from_wit(rect.origin),
            size: point_from_wit(rect.size),
            color: color_from_wit(rect.color),
            width: rect.width,
            fill: rect.fill.map(color_from_wit),
        },
        bindings::OverlayShape::Ellipse(ellipse) => OverlayShape::Ellipse {
            center: point_from_wit(ellipse.center),
            radii: point_from_wit(ellipse.radii),
            color: color_from_wit(ellipse.color),
            width: ellipse.width,
            fill: ellipse.fill.map(color_from_wit),
        },
        bindings::OverlayShape::Path(path) => OverlayShape::Path {
            points: path.points.iter().copied().map(point_from_wit).collect(),
            closed: path.closed,
            color: color_from_wit(path.color),
            width: path.width,
        },
        bindings::OverlayShape::Text(text) => OverlayShape::Text {
            at: point_from_wit(text.at),
            text: Arc::from(text.text.as_str()),
            color: color_from_wit(text.color),
            size: text.size,
        },
        bindings::OverlayShape::Handle(handle) => OverlayShape::Handle {
            id: Arc::from(handle.id.as_str()),
            at: point_from_wit(handle.at),
            color: color_from_wit(handle.color),
            look: handle_look_from_wit(handle.look),
        },
    }
}

/// Convert a guest `list<overlay-shape>` into a host [`OverlayScene`].
pub(crate) fn shapes_from_wit(shapes: &[bindings::OverlayShape]) -> OverlayScene {
    OverlayScene {
        shapes: shapes.iter().map(shape_from_wit).collect(),
    }
}

/// Convert a host [`OverlayEvent`] into the WIT `overlay-event` variant.
pub(crate) fn overlay_event_to_wit(event: &OverlayEvent) -> bindings::OverlayEvent {
    match event {
        OverlayEvent::Drag { handle, phase, at } => {
            let phase = match phase {
                DragPhase::Start => bindings::DragPhase::Start,
                DragPhase::Move => bindings::DragPhase::Move,
                DragPhase::End => bindings::DragPhase::End,
            };
            bindings::OverlayEvent::Drag(bindings::OverlayDrag {
                handle: handle.to_string(),
                phase,
                at: bindings::Point { x: at[0], y: at[1] },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_all_overlay_shape_variants_and_colors() {
        let wit_shapes = vec![
            bindings::OverlayShape::Line(bindings::OverlayLine {
                from: bindings::Point { x: 0.0, y: 10.0 },
                to: bindings::Point { x: 100.0, y: 20.0 },
                color: bindings::OverlayColor::Accent,
                width: 2.0,
            }),
            bindings::OverlayShape::Rect(bindings::OverlayRect {
                origin: bindings::Point { x: 12.0, y: 24.0 },
                size: bindings::Point { x: 320.0, y: 180.0 },
                color: bindings::OverlayColor::Text,
                width: 1.5,
                fill: Some(bindings::OverlayColor::Rgba((10, 20, 30, 128))),
            }),
            bindings::OverlayShape::Ellipse(bindings::OverlayEllipse {
                center: bindings::Point { x: 50.0, y: 60.0 },
                radii: bindings::Point { x: 25.0, y: 15.0 },
                color: bindings::OverlayColor::Muted,
                width: 1.0,
                fill: Some(bindings::OverlayColor::Success),
            }),
            bindings::OverlayShape::Path(bindings::OverlayPath {
                points: vec![
                    bindings::Point { x: 1.0, y: 2.0 },
                    bindings::Point { x: 3.0, y: 4.0 },
                    bindings::Point { x: 5.0, y: 2.0 },
                ],
                closed: true,
                color: bindings::OverlayColor::Warning,
                width: 2.5,
            }),
            bindings::OverlayShape::Text(bindings::OverlayText {
                at: bindings::Point { x: 80.0, y: 90.0 },
                text: "Safe Zone".to_owned(),
                color: bindings::OverlayColor::Danger,
                size: 14.0,
            }),
            bindings::OverlayShape::Handle(bindings::OverlayHandle {
                id: "h-dot".to_owned(),
                at: bindings::Point { x: 10.0, y: 10.0 },
                color: bindings::OverlayColor::Accent,
                look: bindings::HandleLook::Dot,
            }),
            bindings::OverlayShape::Handle(bindings::OverlayHandle {
                id: "h-square".to_owned(),
                at: bindings::Point { x: 20.0, y: 20.0 },
                color: bindings::OverlayColor::Text,
                look: bindings::HandleLook::Square,
            }),
            bindings::OverlayShape::Handle(bindings::OverlayHandle {
                id: "h-cross".to_owned(),
                at: bindings::Point { x: 30.0, y: 30.0 },
                color: bindings::OverlayColor::Muted,
                look: bindings::HandleLook::Cross,
            }),
        ];

        let scene = shapes_from_wit(&wit_shapes);
        scene.check().expect("converted scene should pass check");
        assert_eq!(
            scene,
            OverlayScene {
                shapes: vec![
                    OverlayShape::Line {
                        from: [0.0, 10.0],
                        to: [100.0, 20.0],
                        color: OverlayColor::Accent,
                        width: 2.0,
                    },
                    OverlayShape::Rect {
                        origin: [12.0, 24.0],
                        size: [320.0, 180.0],
                        color: OverlayColor::Text,
                        width: 1.5,
                        fill: Some(OverlayColor::Rgba([10, 20, 30, 128])),
                    },
                    OverlayShape::Ellipse {
                        center: [50.0, 60.0],
                        radii: [25.0, 15.0],
                        color: OverlayColor::Muted,
                        width: 1.0,
                        fill: Some(OverlayColor::Success),
                    },
                    OverlayShape::Path {
                        points: vec![[1.0, 2.0], [3.0, 4.0], [5.0, 2.0]],
                        closed: true,
                        color: OverlayColor::Warning,
                        width: 2.5,
                    },
                    OverlayShape::Text {
                        at: [80.0, 90.0],
                        text: Arc::from("Safe Zone"),
                        color: OverlayColor::Danger,
                        size: 14.0,
                    },
                    OverlayShape::Handle {
                        id: Arc::from("h-dot"),
                        at: [10.0, 10.0],
                        color: OverlayColor::Accent,
                        look: HandleLook::Dot,
                    },
                    OverlayShape::Handle {
                        id: Arc::from("h-square"),
                        at: [20.0, 20.0],
                        color: OverlayColor::Text,
                        look: HandleLook::Square,
                    },
                    OverlayShape::Handle {
                        id: Arc::from("h-cross"),
                        at: [30.0, 30.0],
                        color: OverlayColor::Muted,
                        look: HandleLook::Cross,
                    },
                ],
            }
        );
    }

    #[test]
    fn converts_all_drag_phases_to_wit() {
        for (host_phase, expected_wit) in [
            (DragPhase::Start, bindings::DragPhase::Start),
            (DragPhase::Move, bindings::DragPhase::Move),
            (DragPhase::End, bindings::DragPhase::End),
        ] {
            let event = OverlayEvent::Drag {
                handle: Arc::from("focus"),
                phase: host_phase,
                at: [640.5, 360.25],
            };
            let bindings::OverlayEvent::Drag(drag) = overlay_event_to_wit(&event);
            assert_eq!(drag.handle, "focus");
            assert_eq!(drag.phase, expected_wit);
            assert_eq!(drag.at.x, 640.5);
            assert_eq!(drag.at.y, 360.25);
        }
    }
}
