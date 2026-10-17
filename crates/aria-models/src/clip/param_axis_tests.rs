//! Conformance for the [`ClipParam`] registry.
//!
//! The class closed here is "a `ClipParam` variant behaves differently from
//! what it declares". Before the registry, each variant was an arm in a
//! handful of matches — three in the transform, five in the project, eight
//! hand-written catalog rows — and most of those matches ended in a wildcard,
//! so a variant added without its arms fell into somebody else's and shipped
//! silently mis-routed.
//!
//! [`ClipParam::spec`] now holds the only exhaustive match over the enum, so a
//! variant with no row does not compile. That closes *existence*. These tests
//! close *coherence*: the row's class, lane, kind, range, read accessor and
//! write accessor have to agree with each other and with the catalog, on every
//! variant, enumerated from [`ClipParam::CENSUS`] rather than from a list
//! written here.

use super::*;

use crate::clip::{Generator, ParamAxis, clip_param_catalog};
use crate::ids::MediaId;
use crate::time::{Rational, TimeRange};

const FPS_24: Rational = Rational::FPS_24;

/// A media-backed clip: the only shape both the visual rows and the audio
/// envelopes have a route on, so one fixture exercises every row that declares
/// a slot.
fn media_clip() -> Clip {
    Clip::from_media(
        MediaId::from_raw(1),
        TimeRange::at_rate(0, 96, FPS_24),
        TimeRange::at_rate(0, 96, FPS_24),
    )
}

/// A value inside whatever the row declares, for the kind it declares.
///
/// `0.5` clamped into the declared range lands inside every shipped scalar
/// bound (opacity `[0,1]`, volume `[0,10]`, pan `[-1,1]`, scale above epsilon)
/// and needs no per-property table, which is the point: a new property is
/// covered by declaring its range.
fn in_range_value(param: ClipParam, frac: f32) -> Option<ParamValue> {
    let v = param.range().clamp(frac);
    match param.kind()? {
        ParamKind::Scalar => Some(ParamValue::Scalar(v)),
        ParamKind::Vec2 => Some(ParamValue::Vec2([v, v])),
        ParamKind::Vec3 => Some(ParamValue::Vec3([v, v, v])),
        ParamKind::Color => Some(ParamValue::Color([10, 20, 30, 255])),
        ParamKind::Rect => Some(ParamValue::Rect([0.0, 0.0, v, v])),
    }
}

/// The value a route currently holds, whatever kind it is.
fn sample(route: ParamRoute<'_>, tick: i64) -> Option<ParamValue> {
    Some(match route {
        ParamRoute::Scalar(p) => ParamValue::Scalar(p.sample(tick)),
        ParamRoute::Vec2(p) => ParamValue::Vec2(p.sample(tick)),
        ParamRoute::Vec3(p) => ParamValue::Vec3(p.sample(tick)),
        ParamRoute::Scale(p) => {
            let s = p.sample(tick);
            ParamValue::Vec2([s.x, s.y])
        }
        ParamRoute::Color(p) => ParamValue::Color(p.sample(tick)),
        ParamRoute::Rect(p) => {
            let r = p.sample(tick);
            ParamValue::Rect([r.x, r.y, r.w, r.h])
        }
        ParamRoute::Const(value) => value,
        ParamRoute::Absent => return None,
    })
}

/// Existence is enforced by the compiler; this holds the row to its own
/// declaration. `browse` is the catalog contribution, and only a directly
/// addressed property has one — a wrapper's rows belong to the members of its
/// axis, and an excluded parameter has none by definition. A new variant that
/// declares `Direct` and forgets its catalog facts (or declares them while
/// delegating) turns this red.
#[test]
fn a_rows_browse_facts_follow_the_class_it_declares() {
    for (param, spec) in clip_param_axis() {
        let direct = matches!(spec.class, ClipParamClass::Direct(_));
        assert_eq!(
            direct,
            spec.browse.is_some(),
            "{param:?} declares {:?} but {} catalog facts",
            spec.class,
            if spec.browse.is_some() {
                "carries"
            } else {
                "carries no"
            }
        );
    }
}

/// The catalog's head rows are generated from the registry, so this is the
/// claim that generation is faithful: every direct row reaches the catalog
/// under the id its class declares, carrying the facts its row declares, and
/// nothing else in the catalog claims to be one of them.
#[test]
fn every_direct_row_reaches_the_catalog_with_its_own_facts() {
    let mut seen = 0_usize;
    for (param, spec) in clip_param_axis() {
        let (ClipParamClass::Direct(id), Some(browse)) = (spec.class, spec.browse.as_ref()) else {
            continue;
        };
        seen += 1;
        let rows: Vec<_> = clip_param_catalog()
            .iter()
            .filter(|row| row.param == param)
            .collect();
        assert_eq!(rows.len(), 1, "{param:?} has {} catalog rows", rows.len());
        let row = rows[0];
        assert_eq!(row.id, id, "{param:?} row id");
        assert_eq!(row.label, browse.label, "{param:?} row label");
        assert_eq!(row.group, browse.group, "{param:?} row group");
        assert_eq!(row.kind, browse.kind, "{param:?} row kind");
        assert_eq!(row.range, browse.range, "{param:?} row range");
        assert_eq!(row.scope, spec.scope, "{param:?} row scope");
    }
    // Derived on both sides: the number of direct rows is whatever the registry
    // declares, and every one of them had to be found.
    let direct = clip_param_axis()
        .filter(|(_, spec)| matches!(spec.class, ClipParamClass::Direct(_)))
        .count();
    assert_eq!(seen, direct, "a direct row carried no catalog facts");
    assert!(direct > 0, "the registry declares no direct property");
}

/// A row that declares a slot must be able to write through it and read the
/// same value back. This is what makes the read and write accessors one row
/// rather than two lists: a row whose accessors name different fields, or
/// whose declared range refuses its own midpoint, fails here.
#[test]
fn a_row_with_a_slot_reads_back_what_it_writes() {
    let mut written = 0_usize;
    for (param, spec) in clip_param_axis() {
        if spec.slot.is_none() {
            continue;
        }
        let mut clip = media_clip();
        let value = in_range_value(param, 0.5).expect("a slot row declares its kind");
        param
            .set_keyframe(&mut clip, 12, value, Easing::Linear)
            .unwrap_or_else(|e| panic!("{param:?} refused its own in-range value: {e}"));
        let got = sample(param.route(&clip), 12)
            .unwrap_or_else(|| panic!("{param:?} wrote a keyframe its read accessor cannot see"));
        assert_eq!(got, value, "{param:?} read back a different value");

        // The keyframe is there, so removing it at the same tick succeeds and
        // removing it twice does not.
        param
            .remove_keyframe(&mut clip, 12)
            .unwrap_or_else(|e| panic!("{param:?} could not remove its own keyframe: {e}"));
        assert!(
            param.remove_keyframe(&mut clip, 12).is_err(),
            "{param:?} removed a keyframe that was not there"
        );

        // A constant write lands on the same route.
        param
            .set_constant(&mut clip, value)
            .unwrap_or_else(|e| panic!("{param:?} refused a constant: {e}"));
        assert_eq!(sample(param.route(&clip), 0), Some(value));
        written += 1;
    }
    assert!(written > 0, "no row declares a slot");
}

/// A row with no slot is not writable through a param command, whatever verb
/// is used. The speed ramp is the shipped case: it is keyframed on its own
/// normalized scale through `set_clip_speed_curve`, and a param command that
/// quietly landed somewhere plausible instead is the defect.
#[test]
fn a_row_with_no_slot_refuses_every_param_command() {
    for (param, spec) in clip_param_axis() {
        if spec.slot.is_some() || !matches!(spec.class, ClipParamClass::Excluded(_)) {
            continue;
        }
        let mut clip = media_clip();
        let value = in_range_value(param, 0.5).unwrap_or(ParamValue::Scalar(1.0));
        assert!(
            param
                .set_keyframe(&mut clip, 0, value, Easing::Linear)
                .is_err(),
            "{param:?} is excluded but accepted a keyframe"
        );
        assert!(
            param.set_constant(&mut clip, value).is_err(),
            "{param:?} is excluded but accepted a constant"
        );
        assert!(
            param.remove_keyframe(&mut clip, 0).is_err(),
            "{param:?} is excluded but accepted a keyframe removal"
        );
    }
}

/// An excluded parameter contributes no catalog row, and a delegated one
/// contributes no row *of its own*: every row carrying its variant names one of
/// its axis members, and the wrapper declares no label, group or slot.
///
/// The distinction is the whole point. A wrapper and its members are the same
/// enum variant with different payloads — `Shape { param: Width }` is both the
/// census sample for the wrapper and a real catalog row — so "a delegated
/// variant has no row" is not a claim that can be true, and asserting it went
/// red the moment the shape generator's properties reached the catalog. What is
/// true, and what a fifth axis is held to, is that the rows are members.
#[test]
fn a_variant_that_declares_no_catalog_row_has_none() {
    for (param, spec) in clip_param_axis() {
        match &spec.class {
            ClipParamClass::Direct(_) => continue,
            ClipParamClass::Excluded(reason) => assert!(
                !clip_param_catalog().iter().any(|row| row.param == param),
                "{param:?} is excluded ({reason:?}) but the catalog holds a row for it"
            ),
            ClipParamClass::Delegated(axis) => {
                assert!(
                    spec.browse.is_none() && spec.slot.is_none(),
                    "{param:?} delegates to {axis:?} but declares browse facts or a slot \
                     of its own, which its members already own"
                );
                let members: Vec<ClipParam> = axis.params().collect();
                let mut rows = 0;
                for row in clip_param_catalog() {
                    if std::mem::discriminant(&row.param) != std::mem::discriminant(&param) {
                        continue;
                    }
                    rows += 1;
                    assert!(
                        members.contains(&row.param),
                        "the catalog row for {:?} carries {axis:?}'s variant but is not one \
                         of its members",
                        row.param
                    );
                }
                assert!(
                    rows > 0,
                    "{param:?} delegates to {axis:?} and no member of it has a catalog row, \
                     so no surface can reach the axis at all"
                );
            }
        }
    }
}

/// The registry's kind and range accessors answer for a delegated wrapper by
/// reading the member's catalog row, so a surface asking the wrapper and a
/// surface asking the catalog get the same answer. A second table would drift
/// here.
#[test]
fn the_registry_answers_what_the_catalog_declares() {
    let mut checked = 0_usize;
    for axis in ParamAxis::ALL {
        for param in axis.params() {
            let row = crate::clip::param_spec(param)
                .unwrap_or_else(|| panic!("{param:?} has no catalog row"));
            assert_eq!(param.kind(), Some(row.kind), "{param:?} kind");
            assert_eq!(param.range(), row.range, "{param:?} range");
            checked += 1;
        }
    }
    assert!(checked > 0, "no axis declares a member");
}

/// A read accessor has to find the value on a clip that has the block, and
/// report `Absent` on one that does not. Availability is the whole reason the
/// route is an accessor rather than a field name: a mask's feather has no route
/// until that mask exists, and offering it would ship a dead control.
#[test]
fn a_delegated_read_accessor_follows_the_block_it_needs() {
    let mut bare = Clip::generated(
        Generator::SolidColor {
            rgba: [0, 0, 0, 255],
        },
        TimeRange::at_rate(0, 48, FPS_24),
    );
    // A bare generated clip has no mask, so the mask rows must go quiet while
    // the grade rows, which live on a struct every clip carries, must not.
    let feather = |mask| ClipParam::Mask {
        mask,
        param: crate::clip::MaskParam::Feather,
    };
    assert!(matches!(feather(0).route(&bare), ParamRoute::Absent));
    // One mask gives mask 0 a route and leaves mask 1 without one.
    bare.masks
        .push(crate::look::Mask::new(crate::look::MaskKind::Circle));
    assert!(matches!(feather(0).route(&bare), ParamRoute::Scalar(_)));
    assert!(matches!(feather(1).route(&bare), ParamRoute::Absent));
    let brightness = ClipParam::Look {
        param: crate::clip::LookParam::AdjustBrightness,
    };
    assert!(!matches!(brightness.route(&bare), ParamRoute::Absent));
}

/// The predicate every surface reads before offering the preset has to be the
/// refusal the model actually performs. This is the class the old duplicate
/// belonged to: the AI validator kept its own list of which parameters accept a
/// preset, so a new property was accepted on the wire and refused by the model,
/// or worse the reverse. The sweep runs over `CENSUS` and over every member of
/// every axis, so a colour or a generator property added later is covered by
/// declaring its kind.
#[test]
fn a_parameter_that_advertises_no_easing_preset_refuses_one() {
    let mut clip = media_clip();
    let mut refused = 0_usize;
    let mut candidates: Vec<ClipParam> = clip_param_axis().map(|(param, _)| param).collect();
    for axis in ParamAxis::ALL {
        candidates.extend(axis.params());
    }
    for param in candidates {
        if param.supports_easing_preset() {
            continue;
        }
        assert!(
            param
                .apply_easing_preset(&mut clip, 0, PiecewiseEasingPreset::BounceOut)
                .is_err(),
            "{param:?} advertises no easing preset but accepted one"
        );
        refused += 1;
    }
    assert!(refused > 0, "every parameter advertises an easing preset");
}

/// A colour and a rect never accept a preset, whichever axis they belong to: a
/// preset extrapolates outside the keyframe pair, which neither can do. Stated
/// separately from the predicate so the product rule is asserted rather than
/// the implementation of the predicate.
#[test]
fn no_color_or_rect_parameter_accepts_an_easing_preset() {
    let mut checked = 0_usize;
    for axis in ParamAxis::ALL {
        for param in axis.params() {
            if !matches!(param.kind(), Some(ParamKind::Color | ParamKind::Rect)) {
                continue;
            }
            assert!(
                !param.supports_easing_preset(),
                "{param:?} is a colour or a rect and advertises an easing preset"
            );
            checked += 1;
        }
    }
    for (param, _) in clip_param_axis() {
        if !matches!(param.kind(), Some(ParamKind::Color | ParamKind::Rect)) {
            continue;
        }
        assert!(
            !param.supports_easing_preset(),
            "{param:?} is a colour or a rect and advertises an easing preset"
        );
        checked += 1;
    }
    assert!(checked > 0, "no colour or rect parameter is declared");
}

/// The accepted side has to be real. A row that advertises the preset and then
/// errors, or succeeds without expanding anything, is the same broken control
/// as one that refuses.
#[test]
fn an_accepted_easing_preset_expands_the_segment_it_names() {
    let mut exercised = 0_usize;
    for (param, spec) in clip_param_axis() {
        if spec.slot.is_none() || !param.supports_easing_preset() {
            continue;
        }
        let mut clip = media_clip();
        let from = in_range_value(param, 0.5).expect("a slot row declares its kind");
        let to = in_range_value(param, 0.9).expect("a slot row declares its kind");
        param
            .set_keyframe(&mut clip, 0, from, Easing::Linear)
            .unwrap();
        param
            .set_keyframe(&mut clip, 48, to, Easing::Linear)
            .unwrap();
        let row = crate::clip::param_spec(param).expect("a direct row is in the catalog");
        let before = row.keyframes(&clip).len();
        param
            .apply_easing_preset(&mut clip, 0, PiecewiseEasingPreset::BounceOut)
            .unwrap_or_else(|e| {
                panic!("{param:?} advertises an easing preset but refused one: {e}")
            });
        let after = row.keyframes(&clip).len();
        assert!(
            after > before,
            "{param:?} accepted a preset and expanded nothing ({before} -> {after})"
        );
        exercised += 1;
    }
    assert!(exercised > 0, "no slot row advertises an easing preset");
}

/// The validator column refuses a value of the wrong shape before anything is
/// written, for every variant that declares a kind. A row that accepted a
/// colour where it stores a scalar would corrupt on write instead of failing.
#[test]
fn the_validator_refuses_a_value_of_the_wrong_kind() {
    for (param, _) in clip_param_axis() {
        let Some(kind) = param.kind() else { continue };
        let wrong = match kind {
            ParamKind::Color => ParamValue::Scalar(0.5),
            _ => ParamValue::Color([1, 2, 3, 4]),
        };
        assert!(
            param.validate_value(wrong).is_err(),
            "{param:?} declares {kind:?} but validated {wrong:?}"
        );
    }
    for axis in ParamAxis::ALL {
        for param in axis.params() {
            let kind = param.kind().expect("an axis member declares its kind");
            let wrong = match kind {
                ParamKind::Color => ParamValue::Scalar(0.5),
                _ => ParamValue::Color([1, 2, 3, 4]),
            };
            assert!(
                param.validate_value(wrong).is_err(),
                "{param:?} declares {kind:?} but validated {wrong:?}"
            );
        }
    }
}
