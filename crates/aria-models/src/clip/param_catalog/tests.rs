//! Conformance for the animatable-property registry.
//!
//! The class closed here is "a property exists in the model and nothing can
//! reach it". Before the registry, a property was an enum variant plus a
//! hand-written arm in each surface that cared, and a missing arm was silent:
//! the keyframe strip offered six of the fifty-eight properties a clip
//! animates, and the suite was green.
//!
//! So these tests do not check a property list. They enumerate the enums the
//! model declares and require a catalog row for every member, then require each
//! row to actually work: a route that finds the value, a kind that matches what
//! the route yields, availability that follows the optional block, and a
//! keyframe that reads back. Adding a variant to `LookParam`, `StyleParam`,
//! `TextParam`, or `ShapeParam` puts it in that enum's `ALL` by construction, so
//! a new property with no row turns this red rather than shipping unreachable.
//!
//! The parameters outside those four axes are classified by
//! `ClipParam::class`, on the enum: a variant declares whether it addresses a
//! property directly, delegates to an axis, or is excluded and why. The sweep
//! below reads that declaration for every member of `ClipParam::CENSUS`. A new
//! variant compiles only once it is classified, and the census is emitted from
//! the same variant list as the enum, so neither half can go short in silence.

use super::*;
use crate::MaskKind;
use crate::clip::{Clip, ClipParamClass, Generator, ParamAxis, Shape, TextParam, TextStyle};
use crate::look::{
    ChromaKey, Filter, LayerBackground, LayerGlow, LayerOutline, LayerShadow, LayerStyles, Lut,
    Mask, MatteSource, TrackMatte,
};
use crate::param::{Easing, Param};
use crate::time::{Rational, TimeRange};

const FPS_24: Rational = Rational::FPS_24;

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, FPS_24)
}

/// A generated clip with every optional look block enabled, so every row that
/// depends on one has a route.
fn fully_dressed_clip() -> Clip {
    let mut clip = Clip::generated(
        Generator::SolidColor {
            rgba: [255, 255, 255, 255],
        },
        tr(0, 48),
    );
    clip.filter = Some(Filter {
        id: "warm".into(),
        intensity: Param::Constant(0.5),
    });
    clip.lut = Some(Lut {
        path: "/tmp/look.cube".into(),
        intensity: Param::Constant(0.5),
    });
    clip.masks = vec![Mask::new(MaskKind::Circle), Mask::new(MaskKind::Rectangle)];
    clip.chroma_key = Some(ChromaKey {
        rgb: [0, 177, 64],
        strength: Param::Constant(0.5),
        shadow: Param::Constant(0.0),
    });
    clip.matte = Some(TrackMatte {
        source: MatteSource::Luma,
        invert: false,
        softness: Param::Constant(0.25),
    });
    clip.styles = LayerStyles {
        shadow: Some(LayerShadow {
            rgba: Param::Constant([0, 0, 0, 255]),
            offset: Param::Constant([4.0, 4.0]),
            blur: Param::Constant(8.0),
        }),
        glow: Some(LayerGlow {
            rgba: Param::Constant([255, 255, 255, 255]),
            radius: Param::Constant(12.0),
            intensity: Param::Constant(1.0),
        }),
        outline: Some(LayerOutline {
            rgba: Param::Constant([0, 0, 0, 255]),
            width: Param::Constant(2.0),
        }),
        background: Some(LayerBackground {
            rgba: Param::Constant([0, 0, 0, 200]),
            padding: Param::Constant(8.0),
            radius: Param::Constant(4.0),
        }),
    };
    clip
}

/// A title clip with stroke, shadow, and card enabled: the text rows' routes.
fn dressed_text_clip() -> Clip {
    let mut style = TextStyle::default();
    style.stroke = Some(crate::clip::TextStroke {
        rgba: Param::Constant([0, 0, 0, 255]),
        width: Param::Constant(2.0),
    });
    // Normalized, like the model validates them: a text shadow's blur and
    // distance are fractions of the glyph size, not pixels.
    style.shadow = Some(crate::clip::TextShadow {
        rgba: Param::Constant([0, 0, 0, 128]),
        blur: Param::Constant(0.5),
        distance: Param::Constant(0.25),
    });
    style.background = Some(crate::clip::TextBackground {
        rgba: Param::Constant([0, 0, 0, 200]),
        radius: Param::Constant(0.25),
    });
    Clip::generated(
        Generator::Text {
            content: "Aria".into(),
            style,
        },
        tr(0, 48),
    )
}

/// A star with a stroke: the shape rows' routes, including `InnerRatio`, which
/// only exists on a star.
fn dressed_shape_clip() -> Clip {
    Clip::generated(
        Generator::Shape {
            shape: Shape::Star {
                points: 5,
                inner_ratio: Param::Constant(0.5),
            },
            rgba: Param::Constant([255, 0, 0, 255]),
            width: Param::Constant(200.0),
            height: Param::Constant(200.0),
            corner_radius: Param::Constant(0.0),
            stroke: Some(crate::clip::ShapeStroke {
                rgba: Param::Constant([0, 0, 0, 255]),
                width: Param::Constant(3.0),
            }),
        },
        tr(0, 48),
    )
}

/// A media clip: the only shape the audio envelopes have a route on, since a
/// generated clip has no track to mix.
fn dressed_media_clip() -> Clip {
    Clip::from_media(crate::ids::MediaId::from_raw(1), tr(0, 96), tr(0, 48))
}

/// Every clip shape a row can need a route on.
fn all_dressed() -> Vec<Clip> {
    vec![
        fully_dressed_clip(),
        dressed_text_clip(),
        dressed_shape_clip(),
        dressed_media_clip(),
    ]
}

/// Read the classification off the enum for every variant, and hold the
/// catalog to it.
#[test]
fn every_declared_property_has_a_catalog_row() {
    let mut missing = Vec::new();
    for axis in ParamAxis::ALL {
        for param in axis.params() {
            if param_spec(param).is_none() {
                missing.push(format!("{param:?}"));
            }
        }
    }
    for param in ClipParam::CENSUS {
        match param.class() {
            ClipParamClass::Direct(id) => {
                let spec = param_spec(*param)
                    .unwrap_or_else(|| panic!("{param:?} is animatable but has no catalog row"));
                assert_eq!(spec.id, id, "{param:?} row has the wrong id");
            }
            ClipParamClass::Delegated(axis) => {
                let same_variant = |other: &ClipParam| {
                    std::mem::discriminant(other) == std::mem::discriminant(param)
                };
                assert!(
                    axis.params().any(|other| same_variant(&other)),
                    "{param:?} delegates to {axis:?}, which addresses other variants"
                );
            }
            ClipParamClass::Excluded(reason) => {
                assert!(
                    param_spec(*param).is_none(),
                    "{param:?} is excluded ({reason:?}) but the catalog holds a row for it"
                );
            }
        }
    }
    assert!(
        missing.is_empty(),
        "animatable properties with no catalog row, so no surface can reach them: {missing:?}"
    );
}

#[test]
fn the_catalog_holds_no_row_twice() {
    let mut ids: Vec<&str> = clip_param_catalog().iter().map(|s| s.id.as_ref()).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "duplicate property id");

    let mut params: Vec<String> = clip_param_catalog()
        .iter()
        .map(|s| format!("{:?}", s.param))
        .collect();
    params.sort();
    params.dedup();
    assert_eq!(params.len(), count, "two rows name the same ClipParam");
}

/// A project holding one dressed clip, so a row whose block only exists on a
/// title or a shape is still exercised.
///
/// A chroma key is media-only in the model, so the sweep needs a media-backed
/// fixture as well as the generated ones — which is itself part of
/// the contract: a row available on a generated clip must be writable there.
fn generated_project(clip: Clip) -> (crate::Project, crate::ClipId) {
    let generator = match &clip.content {
        crate::ClipSource::Generated(generator) => generator.clone(),
        _ => unreachable!("this fixture is a generated clip"),
    };
    let kind = crate::TrackKind::for_generator(&generator).expect("a lane for the fixture");
    let mut project = crate::Project::new("p", FPS_24);
    let track = project.add_track(kind, "T1");
    let id = project
        .add_generated(track, generator, clip.timeline)
        .expect("clip");
    dress(&mut project, id, &clip, false);
    (project, id)
}

/// A media clip carrying every look block, including the chroma key the model
/// refuses on a generated clip.
fn dressed_media_project() -> (crate::Project, crate::ClipId) {
    let dressed = fully_dressed_clip();
    let mut project = crate::Project::new("p", FPS_24);
    let track = project.add_track(crate::TrackKind::Video, "V1");
    let media = project.add_media(crate::MediaSource::new(
        "/tmp/sample.mp4",
        1920,
        1080,
        FPS_24,
        500,
        true,
    ));
    let id = project
        .add_clip(track, media, tr(0, 48), crate::RationalTime::new(0, FPS_24))
        .expect("clip");
    dress(&mut project, id, &dressed, true);
    (project, id)
}

/// Apply the fixture's optional look blocks through the project, so each one is
/// validated on the way in.
fn dress(project: &mut crate::Project, id: crate::ClipId, clip: &Clip, media_backed: bool) {
    for mask in &clip.masks {
        project.add_clip_mask(id, mask.clone()).expect("mask");
    }
    if media_backed && let Some(chroma) = clip.chroma_key.clone() {
        project
            .set_clip_chroma_key(id, Some(chroma))
            .expect("chroma");
    }
    if let Some(matte) = clip.matte.clone() {
        project.set_clip_matte(id, Some(matte)).expect("matte");
    }
    if let Some(filter) = clip.filter.clone() {
        project.set_clip_filter(id, Some(filter)).expect("filter");
    }
    if let Some(lut) = clip.lut.clone() {
        project.set_clip_lut(id, Some(lut)).expect("lut");
    }
    project
        .set_layer_styles(id, clip.styles.clone())
        .expect("styles");
}

#[path = "tests_routes.rs"]
mod routes;

#[path = "tests_declared_facts.rs"]
mod declared_facts;

#[path = "tests_mask_rows.rs"]
mod mask_rows;

#[path = "tests_defaults.rs"]
mod defaults;

#[path = "tests_clip_keyframes.rs"]
mod clip_keyframes;
