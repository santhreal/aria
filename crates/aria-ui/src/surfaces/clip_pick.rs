//! What an operator picks in the inspector's remaining clip blocks: audio,
//! animation, speed, template, text and transition.
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
//! Both are named in ledger row 140 and neither is stubbed here.

use aria_engine::EngineError;
use aria_models::{AnimationSlot, AudioRole, MediaId, SlotMedia};

use crate::app::AppState;

/// One thing an operator picks in an audio, animation, speed, template, text or
/// transition block.
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
    /// A transition on the junction the clip owns.
    Transition(String),
    /// How wide that transition is, in ticks.
    TransitionLength(i64),
    ClearTransition,
}

impl Pick {
    /// What the operator picked, for a census and for a failure message.
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
            Self::Transition(_) => "a transition on the junction",
            Self::TransitionLength(_) => "the transition's length stepped",
            Self::ClearTransition => "the transition taken off",
        }
    }
}

/// Apply one pick to the selected clip.
pub fn apply(state: &mut AppState, pick: Pick) -> Result<(), EngineError> {
    match pick {
        Pick::ToggleReverse => state.toggle_selected_reverse(),
        Pick::TogglePitchLock => state.toggle_selected_pitch_lock(),
        Pick::Role(role) => state.set_selected_audio_role(role),
        Pick::FadeIn(ticks) => state.set_selected_fade_in(ticks),
        Pick::FadeOut(ticks) => state.set_selected_fade_out(ticks),
        Pick::Animation(id) => state.set_selected_animation(id),
        Pick::AnimationKnob { slot, knob, by } => {
            state.nudge_selected_animation_knob(slot, knob, by)
        }
        Pick::ClearAnimation(slot) => state.clear_selected_animation(slot),
        Pick::SpeedPreset(id) => state.set_selected_speed_preset(&id),
        Pick::ClearSpeedCurve => state.clear_selected_speed_curve(),
        Pick::ReplaceMedia(media) => state.replace_selected_media(media),
        Pick::SlotAccepts(accepts) => state.set_selected_slot_accepts(accepts),
        Pick::ToggleReplaceable => state.toggle_selected_replaceable(),
        Pick::SlotOrder(delta) => state.nudge_selected_slot_order(delta),
        Pick::ClearSlotLabel => state.set_selected_slot_label(""),
        Pick::ToggleTextEditable => state.toggle_selected_text_editable(),
        Pick::TextEffect(id) => state.set_selected_text_effect(&id),
        Pick::ClearTextEffect => state.clear_selected_text_effect(),
        Pick::TextPreset(preset) => state.apply_text_preset(&preset),
        Pick::Font(family) => state.set_selected_font(&family),
        Pick::CycleFont { forward } => state.cycle_selected_font(forward),
        Pick::Transition(id) => state.set_selected_transition(&id),
        Pick::TransitionLength(delta) => state.nudge_selected_transition_ticks(delta),
        Pick::ClearTransition => state.clear_selected_transition(),
    }
}
