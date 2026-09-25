//! One representative value of every layer topology.
//!
//! Split out of the table only for the file-size ceiling; the rows in
//! [`super::SHIPPED`] point straight at these, and a topology without one
//! cannot fill in its `census` column.

use aria_core::{Rational, RationalTime};
use aria_models::{BlendMode, MatteSource, MediaId};
use aria_shapes::{BezierPath, SdfParams};
use aria_text::TextStyle;

use super::LayerSource;
use crate::scene::{SceneLayer, SizeSpec};

/// The media a census value names. Nothing decodes a census layer — the rows are
/// read for their columns, and the conformance suite builds its own real
/// fixtures — so a placeholder id is honest here.
const CENSUS_MEDIA: MediaId = MediaId::from_raw(0);

/// The size the census text run is authored at, read from the model's own text
/// default rather than written down here.
///
/// Nothing measures a census layer, so the number only has to be a size a real
/// title could have — and the one place that is already decided is the style a
/// freshly dropped title gets.
fn census_font_size() -> f32 {
    aria_models::TextStyle::default().size.sample(0)
}

pub(super) fn census_media() -> LayerSource {
    LayerSource::Media {
        media: CENSUS_MEDIA,
        source_time: RationalTime::new(0, Rational::FPS_30),
    }
}

pub(super) fn census_still() -> LayerSource {
    LayerSource::Still {
        media: CENSUS_MEDIA,
    }
}

pub(super) fn census_sticker() -> LayerSource {
    LayerSource::Sticker {
        asset: String::new(),
        local_time: 0.0,
    }
}

pub(super) fn census_lottie() -> LayerSource {
    LayerSource::Lottie {
        path: String::new(),
        local_time: 0.0,
    }
}

pub(super) fn census_text() -> LayerSource {
    LayerSource::Text {
        content: String::new(),
        style: TextStyle::new(census_font_size()),
        animation: None,
    }
}

pub(super) fn census_solid() -> LayerSource {
    LayerSource::Solid([0, 0, 0, 0])
}

pub(super) fn census_canvas_pass() -> LayerSource {
    LayerSource::CanvasPass
}

pub(super) fn census_shape() -> LayerSource {
    LayerSource::Shape {
        params: SdfParams::Ellipse,
        fill: [0, 0, 0, 0],
        stroke: None,
        pad: 0.0,
    }
}

pub(super) fn census_path_shape() -> LayerSource {
    LayerSource::PathShape {
        path: BezierPath {
            points: Vec::new(),
            closed: false,
        },
        fill: [0, 0, 0, 0],
        stroke: None,
        raster_scale: 1.0,
    }
}

pub(super) fn census_transition() -> LayerSource {
    LayerSource::Transition {
        outgoing: Box::new(census_layer()),
        incoming: Box::new(census_layer()),
        transition_id: String::new(),
        progress: 0.0,
    }
}

pub(super) fn census_matte() -> LayerSource {
    LayerSource::Matte {
        layers: Box::new([census_layer(), census_layer()]),
        read: MatteSource::Alpha,
        invert: false,
        softness: 0.0,
    }
}

/// A placed layer for the wrapper census values to hold.
pub(super) fn census_layer() -> SceneLayer {
    SceneLayer {
        clip: None,
        source: census_solid(),
        center: [0.0, 0.0],
        anchor_point: [0.5, 0.5],
        size: SizeSpec::Fixed([0.0, 0.0]),
        rotation: 0.0,
        opacity: 1.0,
        uv: [0.0, 0.0, 1.0, 1.0],
        effects: Vec::new(),
        mask: None,
        chroma_key: None,
        color_grade: None,
        lut: None,
        blend_mode: BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    }
}
