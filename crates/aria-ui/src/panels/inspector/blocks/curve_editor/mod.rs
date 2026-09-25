//! Curve editor: value against time for any animatable property.
//!
//! Evaluates the model's own [`aria_models::Param`] sampler across the property's
//! time extent to produce true-to-playback curve polylines.
//!
//! The geometry and the drag live beside the state
//! ([`crate::app::curve_geometry`], [`crate::app::curve_drag`]): a drag survives
//! between pointer events, and the state may not name a panel. What is left here
//! is the block that draws them.

pub mod picker;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod canvas;

pub use crate::app::curve_drag::{CurveDragDraft, CurveDragTarget};
pub use crate::app::curve_geometry::{
    BezierHandleKind, CurveBounds, CurveHandle, CurveKeyPoint, CurvePolyline, build_curve_polyline,
    sample_param_scalar,
};
pub use picker::curve_editor_block;
