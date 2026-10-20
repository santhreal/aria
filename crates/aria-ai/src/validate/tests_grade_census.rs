//! Every grade slider is reachable from a tool call, by name, with its own
//! range.
//!
//! # Why
//!
//! The assistant's adjustment arguments used to be a hand-written list beside
//! the model's, and it drifted: twelve of the twenty-three sliders a clip
//! carries had no argument at all, so "warm the highlights with the gain
//! wheels" had no tool to call and nothing went red. These cases enumerate
//! `ColorAdjustments::SLIDERS` at run time and drive each one through the real
//! JSON tool-call path, so a slider that the schema cannot express, that
//! lowers onto the wrong field, or that takes a value outside its row fails
//! here — and a new row is covered the moment it exists.

use super::*;
use aria_models::ColorAdjustments;

/// The tool call a prompt phrase turns into: one slider, one value.
fn call(clip: u64, slider: &str, value: f64) -> Result<WireCommand, String> {
    let mut args = serde_json::Map::new();
    args.insert("clip".into(), clip.into());
    args.insert(slider.into(), value.into());
    crate::wire::WireCommand::from_tool_call("set_clip_adjustments", args.into())
}

/// A value inside the row that is not its neutral, so a write is observable.
fn inside(slider: &aria_models::AdjustSlider) -> f64 {
    let mid = f64::from(slider.min + slider.max) / 2.0;
    if mid == f64::from(slider.neutral) {
        f64::from(slider.max) / 2.0
    } else {
        mid
    }
}

#[test]
fn every_grade_slider_is_reachable_by_name_from_a_tool_call() {
    let (project, _, _, _, clip, _) = fixture();
    for slider in ColorAdjustments::SLIDERS {
        let value = inside(slider);
        let cmd = call(clip, slider.name, value)
            .unwrap_or_else(|e| panic!("the schema refuses {}: {e}", slider.name));
        match lower(&project, cmd) {
            EditCommand::SetClipAdjustments { adjust, .. } => {
                assert_eq!(
                    (slider.get)(&adjust).constant(),
                    Some(value as f32),
                    "{} lowered onto another field",
                    slider.name
                );
                for other in ColorAdjustments::SLIDERS {
                    if other.name == slider.name {
                        continue;
                    }
                    assert!(
                        other.at_neutral(&adjust),
                        "writing {} also moved {}",
                        slider.name,
                        other.name
                    );
                }
            }
            other => panic!("{} lowered to {other:?}", slider.name),
        }
    }
}

#[test]
fn a_grade_slider_refuses_a_value_outside_its_own_row() {
    let (project, _, _, _, clip, _) = fixture();
    for slider in ColorAdjustments::SLIDERS {
        // JSON has no infinity, so the far value stands in for one.
        for outside in [
            f64::from(slider.min) - 0.5,
            f64::from(slider.max) + 0.5,
            f64::from(slider.max) + 1e6,
        ] {
            let cmd = call(clip, slider.name, outside)
                .unwrap_or_else(|e| panic!("the schema refuses {}: {e}", slider.name));
            let msg = reject(&project, cmd);
            assert!(
                msg.contains(slider.name),
                "{} refused {outside} without naming itself: {msg}",
                slider.name
            );
        }
        let edge = call(clip, slider.name, f64::from(slider.max))
            .unwrap_or_else(|e| panic!("the schema refuses {}: {e}", slider.name));
        assert!(
            validate(&edge, &project).is_ok(),
            "{} refused its own upper bound",
            slider.name
        );
    }
}

#[test]
fn a_misspelled_slider_is_refused_rather_than_ignored() {
    // A name this build does not know is a mistake the model must see, not a
    // call that quietly changes nothing.
    let (_, _, _, _, clip, _) = fixture();
    let msg = call(clip, "brightnes", 0.5).expect_err("a misspelled slider");
    assert!(msg.contains("brightnes"), "{msg}");
}

#[test]
fn the_read_back_names_only_the_sliders_the_call_carries() {
    // What the transcript shows: the sliders that moved, by the
    // same names the tool takes, and nothing else.
    let (_, _, _, _, clip, _) = fixture();
    let cmd = crate::wire::WireCommand::from_tool_call(
        "set_clip_adjustments",
        serde_json::json!({ "clip": clip, "brightness": 0.2, "gain_master": 1.5 }),
    )
    .expect("a two-slider call");
    let said = crate::agent::describe_action(&cmd, None);
    assert!(
        said.contains("brightness 0.20") && said.contains("gain_master 1.50"),
        "the read-back lost a slider: {said}"
    );
    assert!(
        !said.contains("contrast"),
        "the read-back invented a slider: {said}"
    );
}
