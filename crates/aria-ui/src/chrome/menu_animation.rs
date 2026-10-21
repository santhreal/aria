//! The Animation menu: keyframes, interpolation, and the property rows.

use std::borrow::Cow;

use crate::app::AppState;
use crate::app::menu_verbs::KeyInterpolation;
use crate::app::property_rows::RevealAction;

use super::menu_guards as guards;
use super::menu_items::{MenuItem, MenuItems, report};

const INTERPOLATION: &str = "Keyframe Interpolation";
const ASSISTANT: &str = "Keyframe Assistant";

static ANIMATION_ITEMS: &[MenuItem] = &[
    MenuItem::new("anim-add-keyframe", "Add Keyframe", |_, s, _| {
        let added = s.add_keyframes_on_selected_properties().map(drop);
        report(s, added)
    })
    .when(guards::property_rows),
    MenuItem::new("anim-toggle-hold", "Toggle Hold Keyframe", |_, s, _| {
        let toggled = s.toggle_hold_selected_or_playhead();
        report(s, toggled)
    })
    .key("key-toggle-hold")
    .when(guards::keyframes),
    MenuItem::new("anim-interp-linear", "Linear", |_, s, _| {
        let set = s.set_keyframe_interpolation(KeyInterpolation::Linear);
        report(s, set)
    })
    .when(guards::keyframes)
    .under(INTERPOLATION),
    MenuItem::new("anim-interp-bezier", "Bezier", |_, s, _| {
        let set = s.set_keyframe_interpolation(KeyInterpolation::Bezier);
        report(s, set)
    })
    .when(guards::keyframes)
    .under(INTERPOLATION),
    MenuItem::new("anim-interp-hold", "Hold", |_, s, _| {
        let set = s.set_keyframe_interpolation(KeyInterpolation::Hold);
        report(s, set)
    })
    .when(guards::keyframes)
    .under(INTERPOLATION),
    MenuItem::new(
        "anim-keyframe-velocity",
        "Keyframe Velocity…",
        |_, s, _| s.open_keyframe_velocity(),
    )
    .when(guards::keyframes),
    MenuItem::new("anim-easy-ease", "Easy Ease", |_, s, _| {
        let eased = s.easy_ease_selected_or_playhead();
        report(s, eased)
    })
    .key("key-easy-ease")
    .when(guards::keyframes)
    .under(ASSISTANT),
    MenuItem::new("anim-easy-ease-in", "Easy Ease In", |_, s, _| {
        let eased = s.easy_ease_in_selected_or_playhead();
        report(s, eased)
    })
    .key("key-easy-ease-in")
    .when(guards::keyframes)
    .under(ASSISTANT),
    MenuItem::new("anim-easy-ease-out", "Easy Ease Out", |_, s, _| {
        let eased = s.easy_ease_out_selected_or_playhead();
        report(s, eased)
    })
    .key("key-easy-ease-out")
    .when(guards::keyframes)
    .under(ASSISTANT),
    MenuItem::new("anim-time-reverse", "Time-Reverse Keyframes", |_, s, _| {
        let reversed = s.time_reverse_selected_keyframes().map(drop);
        report(s, reversed)
    })
    .when(guards::keyframe_span)
    .under(ASSISTANT),
    MenuItem::new("anim-add-expression", "Add Expression", |_, s, _| {
        let added = s.add_expression_to_selected_property();
        report(s, added)
    })
    .when(guards::clip_property_row),
    MenuItem::new(
        "anim-reveal-keyed",
        "Reveal Properties with Keyframes",
        |_, s, _| s.reveal_properties(RevealAction::AnimatedOrModified, false),
    )
    .key("key-reveal-animated")
    .when(guards::selection),
    MenuItem::new(
        "anim-reveal-modified",
        "Reveal Modified Properties",
        |_, s, _| s.reveal_properties(RevealAction::Modified, false),
    )
    .when(guards::selection),
    MenuItem::new("anim-graph-editor", "Graph Editor", |_, s, _| {
        s.toggle_timeline_graph_editor()
    })
    .key("key-graph-editor"),
];

pub(super) fn animation_items(_state: &AppState) -> MenuItems {
    Cow::Borrowed(ANIMATION_ITEMS)
}
