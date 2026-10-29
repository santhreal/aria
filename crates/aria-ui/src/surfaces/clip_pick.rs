//! The picks in the inspector's remaining clip blocks: audio,
//! animation, speed, template, text and transition, the Transform tab's Parent
//! row, the swatches of every colour property row, and the Character and
//! Paragraph panels.
//!
//! The picture blocks became a registry first
//! ([`inspector_pick`](crate::surfaces::inspector_pick)); these were the rest of
//! the same defect. Twenty gpui listeners, each sealing one setter, and thirteen
//! of the model's commands reachable only through them.
//!
//! Two things stay out, each because it is a different kind of surface rather
//! than a pick:
//!
//! - The ramp editor's pointer work on the speed curve, which is a press, a drag
//!   and a release like a mixer fader, so it wants a gesture registry of its own.
//! - The duck form's Duck button, which takes a gpui context because it reads a
//!   lane's samples while it runs.
//!
//! With several clips selected a pick reaches the other selected clips of the
//! drawn clip's kind that read what the drawn clip reads through it
//! ([`Pick::peers`]), in one undo step. A step lands on the drawn clip and the
//! others take the value it landed on, so clips that read alike stay alike.

use aria_engine::EngineError;
use aria_models::{
    AnimationSlot, AudioRole, ClipId, ClipParam, MediaId, ParentKeep, SlotMedia, TextAlignH,
    TextAlignV, TextCase, TransitionAlignment,
};

use crate::app::AppState;
use crate::app::clip_text::TextSwitch;
use crate::types::UiSlots as _;

/// One update to a transition's parameter slot.
#[derive(Clone, Debug)]
pub enum TransitionSlotEdit {
    /// Step a scalar or angle slot by `by`.
    NudgeScalar(f32),
    /// Step an integer slot by `by`.
    NudgeInt(i32),
    /// Flip a boolean slot.
    ToggleBool,
    /// Pick a choice option by id.
    Choice(String),
    /// Pick an 8-bit RGBA colour swatch.
    Color([u8; 4]),
    /// Step one axis (`0` = X, `1` = Y) of a 2D vector or size slot.
    NudgeVec2 { axis: usize, by: f32 },
    /// Step one axis (`0` = X, `1` = Y, `2` = Z) of a 3D point slot.
    NudgePoint3 { axis: usize, by: f32 },
    /// Replace a 1D curve slot's control points.
    Curve(Vec<aria_models::EffectCurvePoint>),
    /// Replace a gradient slot's color stops.
    Gradient(Vec<aria_models::EffectGradientStop>),
    /// Set a text, font, or file slot.
    Text(String),
    /// Set a layer reference slot (`None` clears it).
    Layer(Option<ClipId>),
}

/// One pick in an audio, animation, speed, template, text or
/// transition block, or in the Parent row.
#[derive(Debug)]
pub enum Pick {
    /// Play the clip backwards.
    ToggleReverse,
    /// Keep the pitch where it is while the speed changes.
    TogglePitchLock,
    /// Tag an audio clip with what it is, or clear the tag.
    Role(Option<AudioRole>),
    /// How long the clip fades up, in ticks.
    FadeIn(i64),
    /// How long it fades out.
    FadeOut(i64),
    /// A look animation from the slot's choices. The id carries its own slot, so
    /// an exit cannot land in the entrance.
    Animation(&'static str),
    /// One step of one of an applied animation's knobs.
    AnimationKnob {
        slot: AnimationSlot,
        knob: &'static str,
        by: f32,
    },
    /// Take the animation out of this slot.
    ClearAnimation(AnimationSlot),
    /// A ramp preset from the speed catalog.
    SpeedPreset(String),
    ClearSpeedCurve,
    /// Repoint the clip at another pooled item.
    ReplaceMedia(MediaId),
    /// Restrict what a template slot accepts.
    SlotAccepts(SlotMedia),
    /// Whether this clip is a template slot at all.
    ToggleReplaceable,
    /// Move the slot earlier or later in the fill order.
    SlotOrder(i32),
    /// Drop the slot's placeholder hint.
    ClearSlotLabel,
    /// Whether a template may retype this title.
    ToggleTextEditable,
    /// A text effect, or the same one again to take it off.
    TextEffect(String),
    ClearTextEffect,
    /// A whole look from the text-preset library.
    TextPreset(aria_cloud::dto::TextPreset),
    /// A font family from the installed list.
    Font(String),
    /// The next or previous installed family.
    CycleFont {
        forward: bool,
    },
    /// One on/off property of the title's type.
    TextSwitch(TextSwitch),
    /// How the title's letters are cased.
    TextCase(TextCase),
    /// Where the title's lines sit across the frame.
    TextAlignH(TextAlignH),
    /// Where the title's block sits down the frame.
    TextAlignV(TextAlignV),
    /// A swatch on a colour property's row.
    ParamColor {
        param: ClipParam,
        rgba: [u8; 4],
    },
    /// A transition on the junction the clip owns.
    Transition(String),
    /// How wide that transition is, in ticks.
    TransitionLength(i64),
    /// How that transition sits relative to the cut.
    TransitionAlignment(TransitionAlignment),
    /// Update one transition parameter slot.
    TransitionParam {
        slot: usize,
        edit: TransitionSlotEdit,
    },
    ClearTransition,
    /// A clip from the Parent row's menu, kept in place or with Shift snapped
    /// onto its anchor.
    Parent {
        parent: ClipId,
        keep: ParentKeep,
    },
    /// The Parent row's None: the clip stays where it is drawn.
    ReleaseParent,
}

impl Pick {
    /// The picked control, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::ToggleReverse => "the clip reversed",
            Self::TogglePitchLock => "pitch lock toggled",
            Self::Role(_) => "an audio role",
            Self::FadeIn(_) => "the fade up",
            Self::FadeOut(_) => "the fade out",
            Self::Animation(_) => "a look animation",
            Self::AnimationKnob { .. } => "an animation knob stepped",
            Self::ClearAnimation(_) => "an animation slot emptied",
            Self::SpeedPreset(_) => "a speed ramp preset",
            Self::ClearSpeedCurve => "the speed curve cleared",
            Self::ReplaceMedia(_) => "the clip's media replaced",
            Self::SlotAccepts(_) => "what a template slot accepts",
            Self::ToggleReplaceable => "the clip marked as a template slot",
            Self::SlotOrder(_) => "the slot's fill order stepped",
            Self::ClearSlotLabel => "the slot's hint cleared",
            Self::ToggleTextEditable => "the title marked editable",
            Self::TextEffect(_) => "a text effect",
            Self::ClearTextEffect => "the text effect cleared",
            Self::TextPreset(_) => "a text preset from the library",
            Self::Font(_) => "a font family",
            Self::CycleFont { .. } => "the next or previous font",
            Self::TextSwitch(_) => "a type switch flipped",
            Self::TextCase(_) => "the title's letter case",
            Self::TextAlignH(_) => "the title's horizontal alignment",
            Self::TextAlignV(_) => "the title's vertical alignment",
            Self::ParamColor { .. } => "a color swatch on a property row",
            Self::Transition(_) => "a transition on the junction",
            Self::TransitionLength(_) => "the transition's length stepped",
            Self::TransitionAlignment(_) => "the transition's alignment",
            Self::TransitionParam { .. } => "a transition's parameter stepped",
            Self::ClearTransition => "the transition taken off",
            Self::Parent { .. } => "a parent from the Parent row",
            Self::ReleaseParent => "the parent released",
        }
    }

    /// The other selected clips this pick reaches besides the drawn one: those
    /// of its kind that read what it reads through the pick. A value reaches
    /// every one of them; a switch reaches those it reads alike on, so every
    /// clip ends where the drawn one does; a clear reaches those holding
    /// something to clear; a step reaches those holding the same thing.
    pub fn peers(&self, state: &AppState) -> Vec<ClipId> {
        match self {
            Self::ToggleReverse => state.peers_reading(|clip| clip.reversed),
            Self::TogglePitchLock => state.peers_reading(|clip| clip.preserve_pitch),
            Self::Role(_)
            | Self::FadeIn(_)
            | Self::FadeOut(_)
            | Self::Animation(_)
            | Self::SpeedPreset(_)
            | Self::ReplaceMedia(_) => state.peers_reading(|_| ()),
            Self::AnimationKnob { slot, .. } => state
                .peers_reading(|clip| clip.animation(*slot).map(|animation| animation.id.clone())),
            Self::ClearAnimation(slot) => {
                state.peers_reading(|clip| clip.animation(*slot).is_some())
            }
            Self::ClearSpeedCurve => state.peers_reading(|clip| clip.time_remap),
            Self::SlotAccepts(_) | Self::SlotOrder(_) | Self::ToggleReplaceable => {
                state.peers_reading(|clip| clip.replaceable.is_some())
            }
            Self::ClearSlotLabel => state.peers_reading(|clip| {
                clip.replaceable
                    .as_ref()
                    .is_some_and(|slot| slot.label.is_some())
            }),
            Self::ToggleTextEditable => state.peers_reading(|clip| clip.text_editable),
            Self::TextEffect(_)
            | Self::TextPreset(_)
            | Self::Font(_)
            | Self::CycleFont { .. }
            | Self::TextCase(_)
            | Self::TextAlignH(_)
            | Self::TextAlignV(_) => state.titles_reading(|_| ()),
            Self::ClearTextEffect => state.titles_reading(|style| style.effect_preset.is_some()),
            Self::TextSwitch(switch) => state.titles_reading(|style| switch.of(style)),
            Self::ParamColor { param, .. } => state.peers_sharing(*param),
            Self::Transition(_) => state.peers_reading(|clip| clip.transition_media),
            Self::TransitionLength(_) | Self::TransitionAlignment(_) | Self::ClearTransition => {
                state.peers_reading(|clip| clip.transition.is_some())
            }
            Self::TransitionParam { slot, edit } => {
                let toggle = matches!(edit, TransitionSlotEdit::ToggleBool);
                state.peers_reading(|clip| {
                    clip.transition.as_ref().map(|transition| {
                        let flag = toggle.then(|| transition.boolean(*slot));
                        (transition.id.clone(), flag)
                    })
                })
            }
            Self::Parent { parent, .. } => {
                let mut peers = state.peers_reading(|_| ());
                peers.retain(|peer| peer != parent);
                peers
            }
            Self::ReleaseParent => state.peers_reading(|clip| clip.parent.is_some()),
        }
    }
}

/// Apply one pick to the selected clip and to the other selected clips it
/// reaches ([`Pick::peers`]), as one undo step.
pub fn apply(state: &mut AppState, pick: Pick) -> Result<(), EngineError> {
    let peers = pick.peers(state);
    match pick {
        Pick::AnimationKnob { slot, knob, by } => state.land_with(
            peers,
            |state| state.nudge_selected_animation_knob(slot, knob, by),
            |state| state.selected_animation_knob(slot, knob),
            |state, &value| state.set_selected_animation_knob(slot, knob, value),
        ),
        Pick::CycleFont { forward } => state.land_with(
            peers,
            |state| state.cycle_selected_font(forward),
            |state| state.selected_font(),
            |state, family| state.set_selected_font(family),
        ),
        Pick::TransitionLength(delta) => state.land_with(
            peers,
            |state| state.nudge_selected_transition_ticks(delta),
            |state| state.selected_transition_ticks(),
            |state, &ticks| state.set_selected_transition_ticks(ticks),
        ),
        Pick::TransitionParam { slot, edit } => transition_slot(state, peers, slot, edit),
        // Marking clips as template slots numbers them in selection order, the
        // drawn clip first, because a fill order is each slot's own.
        Pick::ToggleReplaceable => state.land_with(
            peers,
            |state| state.toggle_selected_replaceable(),
            |_| Some(()),
            |state, _| state.toggle_selected_replaceable(),
        ),
        pick => state.edit_each(peers, |state| one(state, &pick)),
    }
}

/// One pick on the selected clip alone.
fn one(state: &mut AppState, pick: &Pick) -> Result<(), EngineError> {
    match pick {
        Pick::ToggleReverse => state.toggle_selected_reverse(),
        Pick::TogglePitchLock => state.toggle_selected_pitch_lock(),
        Pick::Role(role) => state.set_selected_audio_role(*role),
        Pick::FadeIn(ticks) => state.set_selected_fade_in(*ticks),
        Pick::FadeOut(ticks) => state.set_selected_fade_out(*ticks),
        Pick::Animation(id) => state.set_selected_animation(id),
        Pick::AnimationKnob { slot, knob, by } => {
            state.nudge_selected_animation_knob(*slot, knob, *by)
        }
        Pick::ClearAnimation(slot) => state.clear_selected_animation(*slot),
        Pick::SpeedPreset(id) => state.set_selected_speed_preset(id),
        Pick::ClearSpeedCurve => state.clear_selected_speed_curve(),
        Pick::ReplaceMedia(media) => state.replace_selected_media(*media),
        Pick::SlotAccepts(accepts) => state.set_selected_slot_accepts(*accepts),
        Pick::ToggleReplaceable => state.toggle_selected_replaceable(),
        Pick::SlotOrder(delta) => state.nudge_selected_slot_order(*delta),
        Pick::ClearSlotLabel => state.set_selected_slot_label(""),
        Pick::ToggleTextEditable => state.toggle_selected_text_editable(),
        Pick::TextEffect(id) => state.set_selected_text_effect(id),
        Pick::ClearTextEffect => state.clear_selected_text_effect(),
        Pick::TextPreset(preset) => state.apply_text_preset(preset),
        Pick::Font(family) => state.set_selected_font(family),
        Pick::CycleFont { forward } => state.cycle_selected_font(*forward),
        Pick::TextSwitch(switch) => state.toggle_selected_text_switch(*switch),
        Pick::TextCase(case) => state.set_selected_text_case(*case),
        Pick::TextAlignH(align) => state.set_selected_text_align_h(*align),
        Pick::TextAlignV(align) => state.set_selected_text_align_v(*align),
        Pick::ParamColor { param, rgba } => state.set_param_color(*param, *rgba),
        Pick::Transition(id) => state.set_selected_transition(id),
        Pick::TransitionLength(delta) => state.nudge_selected_transition_ticks(*delta),
        Pick::TransitionAlignment(alignment) => state.set_selected_transition_alignment(*alignment),
        Pick::TransitionParam { slot, edit } => {
            state.edit_selected_transition_slot(*slot, edit.clone())
        }
        Pick::ClearTransition => state.clear_selected_transition(),
        // A parent one of the other clips cannot take, because it does not
        // overlap it or is its child, leaves that clip as it is.
        Pick::Parent { parent, keep } => {
            if state
                .selected_parent_choices()
                .is_some_and(|choices| choices.eligible.contains(parent))
            {
                state.set_selected_parent(Some(*parent), *keep)
            } else {
                Ok(())
            }
        }
        Pick::ReleaseParent => state.set_selected_parent(None, ParentKeep::Place),
    }
}

/// A transition slot edit on the drawn clip and the clips holding the same
/// transition: a step lands on the drawn clip and the others take the value
/// it landed on, on that axis alone; the rest repeat on every clip.
fn transition_slot(
    state: &mut AppState,
    peers: Vec<ClipId>,
    slot: usize,
    edit: TransitionSlotEdit,
) -> Result<(), EngineError> {
    match edit {
        TransitionSlotEdit::NudgeScalar(by) => state.land_with(
            peers,
            |state| state.nudge_selected_transition_param(slot, by),
            |state| state.selected_transition_param(slot),
            |state, &to| state.set_selected_transition_param(slot, to),
        ),
        TransitionSlotEdit::NudgeInt(by) => state.land_with(
            peers,
            |state| state.nudge_selected_transition_int(slot, by),
            |state| state.selected_transition_int(slot),
            |state, &to| state.set_selected_transition_int(slot, to),
        ),
        TransitionSlotEdit::NudgeVec2 { axis, by } => state.land_with(
            peers,
            |state| state.nudge_selected_transition_vec2(slot, axis, by),
            |state| state.selected_transition_vec2(slot)?.get(axis).copied(),
            |state, &to| state.set_selected_transition_axis(slot, axis, to),
        ),
        TransitionSlotEdit::NudgePoint3 { axis, by } => state.land_with(
            peers,
            |state| state.nudge_selected_transition_point3(slot, axis, by),
            |state| state.selected_transition_point3(slot)?.get(axis).copied(),
            |state, &to| state.set_selected_transition_axis(slot, axis, to),
        ),
        edit => state.edit_each(peers, |state| {
            state.edit_selected_transition_slot(slot, edit.clone())
        }),
    }
}
