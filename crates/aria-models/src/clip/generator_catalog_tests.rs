use super::*;

/// The exhaustive `index_of` match is the compile-time gate; this is the
/// proof that each arm points at its *own* row instead of reusing a
/// neighbour's, which would compile and silently mis-describe a generator.
///
/// Both sides are derived: the rows come from [`SHIPPED`], the samples from
/// each row's own census. Nothing here counts variants.
#[test]
fn a_row_per_generator_variant() {
    for (index, spec) in SHIPPED.iter().enumerate() {
        let sample = (spec.census)();
        assert_eq!(
            index_of(&sample),
            index,
            "the '{}' row's census resolves to '{}' instead",
            spec.id,
            SHIPPED[index_of(&sample)].id
        );
        assert_eq!(
            generator_spec(&sample).id,
            spec.id,
            "generator_spec did not return the '{}' row",
            spec.id
        );
        assert_eq!(
            generator_spec_by_id(spec.id).map(|found| found.id),
            Some(spec.id),
            "'{}' is not reachable by id",
            spec.id
        );
    }
}

#[test]
fn every_row_has_a_distinct_id_and_a_label() {
    let mut ids: Vec<&str> = SHIPPED.iter().map(|spec| spec.id).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), before, "two generator rows share an id");
    for spec in SHIPPED {
        assert!(!spec.label.is_empty(), "'{}' has no label", spec.id);
        assert!(
            !spec.renders_with.is_empty(),
            "'{}' resolves to no layer topology",
            spec.id
        );
    }
}

/// A lane bar has no geometry of its own, and anything that paints has
/// geometry. The two columns are one fact seen from both sides, so a row
/// that declares a painting canvas pass is a contradiction.
#[test]
fn painting_and_canvas_pass_are_opposites() {
    for spec in SHIPPED {
        let canvas_pass = spec.renders_with == ["canvas-pass"];
        assert_ne!(
            canvas_pass, spec.paints,
            "'{}' claims paints={} and canvas-pass={canvas_pass}",
            spec.id, spec.paints
        );
    }
}

/// Every row's census value must survive its own declared validator: a
/// representative value the model would refuse is not representative.
#[test]
fn every_census_value_validates() {
    for spec in SHIPPED {
        (spec.validate)(&(spec.census)())
            .unwrap_or_else(|err| panic!("the '{}' census is invalid: {err}", spec.id));
    }
}

/// The `animatable` column is a claim about the walk, so it is checked against
/// the walk: a row that declares keyframes must offer at least one curve to
/// [`ParamWalk`], and a row that declares none must offer nothing at all.
///
/// This is what keeps the column honest in both directions. A generator that
/// grows an animatable parameter and forgets the column reads as static to
/// every surface that asks; one that claims keyframes it does not carry sends a
/// rebase looking for curves that are not there.
#[test]
fn the_animatable_column_matches_what_the_walk_finds() {
    for spec in SHIPPED {
        let mut generator = (spec.census)();
        let mut curves = 0usize;
        crate::param_walk::ParamWalk::walk_params(&mut generator, &mut |_| {
            curves += 1;
            Ok(())
        })
        .expect("counting cannot fail");
        assert_eq!(
            spec.animatable,
            curves > 0,
            "'{}' declares animatable={} but the walk found {curves} curve(s)",
            spec.id,
            spec.animatable
        );
    }
}

/// Rebasing content that carries no curves must leave it byte for byte as it
/// was: a no-op is a no-op, not an unnoticed rewrite.
#[test]
fn a_row_without_keyframes_is_unchanged_by_a_rebase() {
    for spec in SHIPPED.iter().filter(|spec| !spec.animatable) {
        let before = (spec.census)();
        let mut after = before.clone();
        crate::param_walk::ParamWalk::walk_params(&mut after, &mut |curve| curve.shift(10_000))
            .expect("a no-op rebase cannot fail");
        assert_eq!(before, after, "'{}' changed under a rebase", spec.id);
    }
}

/// A lane bar has no content of its own, so its parameters can only be
/// addressed in the programme's time: keyed clip-locally, a bar's look would
/// slide every time the bar was dragged, which is the defect the column exists
/// to close.
///
/// The variant space is read from [`SHIPPED`], so a ninth generator is covered
/// the day it is added rather than the day someone remembers this file.
#[test]
fn a_lane_bar_addresses_its_parameters_in_sequence_time() {
    for spec in SHIPPED.iter().filter(|spec| !spec.paints) {
        assert_eq!(
            spec.param_time,
            ParamTime::Sequence,
            "'{}' paints nothing, so a clip-local keyframe has no content to sit on",
            spec.id
        );
    }
}

/// The other side of the pairing. A generator that draws its own pixels carries
/// its animation with it: a fill's fade belongs to the fill, not to the second
/// of programme it happens to cover. Sequence timing on one would detach the
/// animation from the picture it animates, so it goes red here rather than
/// shipping.
#[test]
fn a_painting_generator_addresses_its_parameters_in_its_own_time() {
    for spec in SHIPPED.iter().filter(|spec| spec.paints) {
        assert_eq!(
            spec.param_time,
            ParamTime::ClipLocal,
            "'{}' paints its own pixels, so its animation belongs to the clip",
            spec.id
        );
    }
}

#[test]
fn shape_validation_rejects_nan_and_infinite_dimensions() {
    assert!(validate_shape_dim(100.0).is_ok());
    assert!(validate_shape_dim(f32::NAN).is_err());
    assert!(validate_shape_dim(f32::INFINITY).is_err());
    assert!(validate_shape_dim(-1.0).is_err());
    assert!(validate_shape_dim(0.0).is_err());

    assert!(validate_stroke_width(5.0).is_ok());
    assert!(validate_stroke_width(f32::NAN).is_err());
    assert!(validate_stroke_width(f32::INFINITY).is_err());

    assert!(validate_corner_radius(10.0).is_ok());
    assert!(validate_corner_radius(f32::NAN).is_err());
    assert!(validate_corner_radius(f32::INFINITY).is_err());
}

#[test]
fn generator_exhaustive_variant_deserialization_roundtrip() {
    for spec in SHIPPED {
        let generator = (spec.census)();
        let json = serde_json::to_string(&generator).expect("serialize");
        let deserialized: Generator = serde_json::from_str(&json).expect("deserialize");
        match (&generator, &deserialized) {
            (Generator::Text { content: c1, .. }, Generator::Text { content: c2, .. }) => {
                assert_eq!(c1, c2);
            }
            (Generator::SolidColor { rgba: r1 }, Generator::SolidColor { rgba: r2 }) => {
                assert_eq!(r1, r2);
            }
            (Generator::Shape { shape: s1, .. }, Generator::Shape { shape: s2, .. }) => {
                assert_eq!(s1, s2);
            }
            (Generator::Sticker { asset: a1 }, Generator::Sticker { asset: a2 }) => {
                assert_eq!(a1, a2);
            }
            (Generator::Lottie { path: p1, .. }, Generator::Lottie { path: p2, .. }) => {
                assert_eq!(p1, p2);
            }
            (Generator::Effect, Generator::Effect)
            | (Generator::Filter, Generator::Filter)
            | (Generator::Adjustment, Generator::Adjustment) => {}
            _ => panic!("variant mismatch between {generator:?} and {deserialized:?}"),
        }
    }
}

#[test]
fn resolve_presets_fails_closed_on_invalid_preset_reference() {
    let mut generator = Generator::Text {
        content: "sample".into(),
        style: crate::clip::text::TextStyle {
            effect_preset: Some("nonexistent_preset_id_xyz".into()),
            ..Default::default()
        },
    };
    let err = generator.resolve_presets().unwrap_err();
    assert!(matches!(err, ModelError::InvalidParam(_)));
}
