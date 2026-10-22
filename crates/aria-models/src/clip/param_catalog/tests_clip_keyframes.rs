//! The keyframes of a whole clip, read in one pass.
//!
//! The projected keyframe strip reads [`visit_clip_keyframes`], and a surface
//! offers the rows [`visit_clip_params`] visits. Closes the class where the two
//! go apart: a keyframe on an offered row missing from the strip, or a keyframe
//! on a row nothing offers drawn on it. Every offered row of every clip shape
//! is keyed on ticks of its own, with a mask of every kind, an effect of every
//! shipped id, and a test effect with a parameter of every kind, so a row the
//! reader skips or adds changes the list. Keyframes are also planted where no
//! row is offered: an audio envelope on a generated clip, a mask property its
//! shape does not draw with, a hidden effect parameter, and a parameter of an
//! effect no catalog lists. Does not check what the strip draws from the ticks.

use super::*;
use crate::MaskParam;
use crate::clip::ClipSource;
use crate::effects::{EffectInstance, EffectParamSpec, EffectSpec};

/// Every keyframe the offered rows report, row by row.
fn offered(clip: &Clip) -> Vec<(i64, Easing)> {
    let mut marks = Vec::new();
    visit_clip_params(clip, |row| {
        row.visit_keyframes(clip, |tick, easing| marks.push((tick, easing)));
    });
    marks
}

/// Every keyframe the one-pass reader reports.
fn read(clip: &Clip) -> Vec<(i64, Easing)> {
    let mut marks = Vec::new();
    visit_clip_keyframes(clip, |tick, easing| marks.push((tick, easing)));
    marks
}

fn offered_params(clip: &Clip) -> Vec<ClipParam> {
    clip_params_for(clip).iter().map(|row| row.param).collect()
}

/// A test effect `id` with a parameter of every kind, and a scalar hidden
/// while its toggle holds its default. The catalog is process-wide and rejects
/// a duplicate id, so each test registers its own.
fn register_every_kind(id: &str) -> (crate::Registered, std::sync::Arc<EffectSpec>) {
    let mut params = crate::effects::typed_tests::every_kind();
    params.push(
        EffectParamSpec::scalar("hidden", "Hidden", 0.0, 0.0, 1.0)
            .with_visible_when("toggle", "false"),
    );
    crate::register_effect(EffectSpec {
        id: id.into(),
        label: "Every kind".into(),
        category: "Test".into(),
        params,
        ..EffectSpec::default()
    })
    .expect("the test effect registers")
}

/// Every clip shape a row can need a route on. The generated one carries a
/// mask of every kind and an effect of every shipped id, then `effect`, then an
/// id no catalog lists.
fn fixtures(effect: &str) -> [Clip; 4] {
    let mut dressed = fully_dressed_clip();
    dressed.masks = MaskKind::ALL.iter().map(|&kind| Mask::new(kind)).collect();
    dressed.effects = crate::shipped_effect_catalog()
        .iter()
        .map(|spec| spec.id.as_str())
        .chain([effect, "not-an-effect"])
        .map(EffectInstance::new)
        .collect();
    [
        dressed,
        dressed_text_clip(),
        dressed_shape_clip(),
        dressed_media_clip(),
    ]
}

/// Two keyframes on every row `clip` offers, on ticks no other row uses, each
/// holding the value already there. Returns how many were written.
fn key_every_row(clip: &mut Clip) -> usize {
    let rows = clip_params_for(clip);
    for (index, row) in rows.iter().enumerate() {
        let value = row
            .sample(clip, 0)
            .unwrap_or_else(|| panic!("{} is offered and samples nothing", row.id));
        let first = 2 * index as i64;
        for (tick, easing) in [(first, Easing::Linear), (first + 1, Easing::EaseIn)] {
            row.param
                .set_keyframe(clip, tick, value, easing)
                .unwrap_or_else(|err| panic!("key {}: {err:?}", row.id));
        }
    }
    2 * rows.len()
}

/// Keyframes on routes `clip` stores and offers no row for, past every tick
/// [`key_every_row`] writes. Returns how many were planted.
fn plant_unoffered(clip: &mut Clip) -> usize {
    const PLANTED: i64 = 100_000;
    let mut planted = 0;
    if !matches!(clip.content, ClipSource::Media { .. }) {
        clip.volume.set_keyframe(PLANTED, 0.5, Easing::Linear);
        planted += 1;
    }
    for mask in &mut clip.masks {
        if !MaskParam::Roundness.applies_to(mask.kind) {
            mask.roundness.set_keyframe(PLANTED, 0.5, Easing::Linear);
            planted += 1;
        }
    }
    // Hidden on the test effect, and a name no other spec declares.
    for instance in &mut clip.effects {
        let mut hidden = Param::Constant(0.0);
        hidden.set_keyframe(PLANTED, 0.5, Easing::Linear);
        instance.params.insert("hidden".into(), hidden);
        planted += 1;
    }
    planted
}

#[test]
fn the_one_pass_reader_lists_what_the_offered_rows_list() {
    let (_registered, effect) = register_every_kind("test.clip-keyframes.offered");
    for mut clip in fixtures(&effect.id) {
        let before = offered_params(&clip);
        let written = key_every_row(&mut clip);
        plant_unoffered(&mut clip);
        assert_eq!(
            offered_params(&clip),
            before,
            "keying the rows changed which rows are offered"
        );
        let expected = offered(&clip);
        assert_eq!(
            expected.len(),
            written,
            "the offered rows do not list every keyframe written on them"
        );
        assert_eq!(read(&clip), expected);
    }
}

#[test]
fn a_keyframe_where_no_row_is_offered_is_not_listed() {
    let (_registered, effect) = register_every_kind("test.clip-keyframes.unoffered");
    let [dressed, text, shape, _] = fixtures(&effect.id);
    // A volume curve on each generated clip; on the dressed one, also a
    // roundness curve on a mask that draws without one, and a hidden scalar on
    // every effect.
    let effects = dressed.effects.len();
    for (mut clip, at_least) in [(dressed, effects + 2), (text, 1), (shape, 1)] {
        let planted = plant_unoffered(&mut clip);
        assert!(
            planted >= at_least,
            "{planted} planted keyframes, expected at least {at_least}"
        );
        let listed = read(&clip);
        assert!(
            listed.is_empty(),
            "of {planted} planted keyframes, {listed:?} were listed"
        );
    }
}
