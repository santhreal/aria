//! Clip animation and audio-role commands.
//!
//! `SetClipAnimation` takes one slot at a time and the model enforces the
//! combo/edge exclusion, so the surface only has to send the edited slot.
//! `AnimationRef` knobs are normalized against the preset's [`AnimationKnobs`],
//! so a knob a preset does not expose is refused: the setters here drop it
//! instead of erroring.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    ANIMATION_INTENSITY_RANGE, ANIMATION_SPEED_RANGE, ANIMATION_STAGGER_RANGE, AnimationRef,
    AnimationSlot, AudioRole, animation_catalog, animation_spec,
};

use super::AppState;
use crate::types::UiAnimation;

/// The three slots, in inspector order.
pub const SLOTS: [AnimationSlot; 3] = [AnimationSlot::In, AnimationSlot::Out, AnimationSlot::Combo];

/// One offerable animation preset: what to apply, what to call it, and whether
/// this build has a renderer for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationChoice {
    pub id: &'static str,
    pub label: &'static str,
    /// False when no sampler implements the id, so applying it moves nothing.
    pub rendered: bool,
}

/// One animation knob: (id, label, reader, writer, range, availability).
type AnimKnob = (
    &'static str,
    &'static str,
    fn(&UiAnimation) -> f32,
    fn(&mut AnimationRef, f32),
    (f32, f32),
    fn(&aria_models::AnimationKnobs) -> bool,
);

pub const ANIM_KNOBS: &[AnimKnob] = &[
    (
        "anim-speed",
        "Speed",
        |a| a.speed,
        |a, v| a.speed = v,
        ANIMATION_SPEED_RANGE,
        |k| k.speed,
    ),
    (
        "anim-intensity",
        "Intensity",
        |a| a.intensity,
        |a, v| a.intensity = v,
        ANIMATION_INTENSITY_RANGE,
        |k| k.intensity,
    ),
    (
        "anim-stagger",
        "Stagger",
        |a| a.stagger,
        |a, v| a.stagger = v,
        ANIMATION_STAGGER_RANGE,
        |k| k.stagger,
    ),
];

/// Read one animation knob by id.
pub fn anim_knob(animation: &UiAnimation, id: &str) -> f32 {
    ANIM_KNOBS
        .iter()
        .find(|(knob, ..)| *knob == id)
        .map(|(_, _, get, ..)| get(animation))
        .unwrap_or(0.0)
}

/// Whether the preset behind `animation` exposes the knob named by `id`.
pub fn anim_knob_available(animation: &UiAnimation, id: &str) -> bool {
    let Some(spec) = animation_spec(&animation.id) else {
        return false;
    };
    ANIM_KNOBS
        .iter()
        .find(|(knob, ..)| *knob == id)
        .is_some_and(|(_, _, _, _, _, available)| available(&spec.knobs))
}

impl AppState {
    /// Read the animation in one slot of the selection.
    pub fn selected_animation(&self, slot: AnimationSlot) -> Option<&UiAnimation> {
        self.selected_ui_clip_ref()?.animation(slot)
    }

    /// Where one knob of the animation in `slot` stands, `None` with no
    /// animation there.
    pub(crate) fn selected_animation_knob(&self, slot: AnimationSlot, id: &str) -> Option<f32> {
        self.selected_animation(slot).map(|now| anim_knob(now, id))
    }

    fn send_animation(
        &mut self,
        slot: AnimationSlot,
        animation: Option<AnimationRef>,
    ) -> Result<(), EngineError> {
        // A committed edit outranks the preview of it: leaving the override set would
        // hide a later slot change behind a stale hover.
        self.clear_animation_preview();
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipAnimation {
            clip,
            slot,
            animation,
        }))?;
        Ok(())
    }

    /// Preview a catalog preset on the selection without editing it: the engine renders
    /// the slot as an override, so moving off the entry leaves nothing on the undo stack.
    ///
    /// Returns whether the override changed, so re-entering the entry the pointer is
    /// already on does not respawn a frame. Inert without a selection or for an unknown
    /// id: the catalog owns which slot a preset belongs to.
    pub fn preview_animation(&mut self, id: &str) -> bool {
        let Some(clip) = self.selected_clip() else {
            return false;
        };
        let Some(spec) = animation_spec(id) else {
            return false;
        };
        let next = Some((clip, spec.slot, Some(AnimationRef::new(&spec.id))));
        if self.animation_preview == next {
            return false;
        }
        self.animation_preview = next.clone();
        self.engine().lock().set_animation_override(next);
        true
    }

    /// Drop the hover preview. Returns whether there was one to drop.
    pub fn clear_animation_preview(&mut self) -> bool {
        if self.animation_preview.is_none() {
            return false;
        }
        self.animation_preview = None;
        self.engine().lock().set_animation_override(None);
        true
    }

    /// Apply a catalog preset to its own slot. The catalog owns which slot a
    /// preset belongs to, so the caller cannot put an exit in the entrance.
    pub fn set_selected_animation(&mut self, id: &str) -> Result<(), EngineError> {
        let Some(spec) = animation_spec(id) else {
            return Ok(());
        };
        self.send_animation(spec.slot, Some(AnimationRef::new(&spec.id)))
    }

    pub fn clear_selected_animation(&mut self, slot: AnimationSlot) -> Result<(), EngineError> {
        if self.selected_clip().is_none() {
            return Ok(());
        }
        self.send_animation(slot, None)
    }

    /// Set one knob of the animation in `slot`, clamped to the model's range.
    /// Inert when the slot is empty or the preset does not expose that knob.
    pub fn set_selected_animation_knob(
        &mut self,
        slot: AnimationSlot,
        id: &str,
        value: f32,
    ) -> Result<(), EngineError> {
        let Some(current) = self.selected_animation(slot) else {
            return Ok(());
        };
        if !value.is_finite() || !anim_knob_available(current, id) {
            return Ok(());
        }
        let Some((_, _, _, set, (min, max), _)) = ANIM_KNOBS.iter().find(|(knob, ..)| *knob == id)
        else {
            return Ok(());
        };
        let mut animation = AnimationRef {
            id: current.id.to_string(),
            speed: current.speed,
            intensity: current.intensity,
            stagger: current.stagger,
        };
        set(&mut animation, value.clamp(*min, *max));
        self.send_animation(slot, Some(animation))
    }

    pub fn nudge_selected_animation_knob(
        &mut self,
        slot: AnimationSlot,
        id: &str,
        step: f32,
    ) -> Result<(), EngineError> {
        let Some(current) = self.selected_animation(slot) else {
            return Ok(());
        };
        self.set_selected_animation_knob(slot, id, anim_knob(current, id) + step)
    }

    /// Catalog presets valid for the selection: text-only presets are hidden on
    /// non-text clips, because the command layer refuses them there.
    ///
    /// `rendered` is `aria_render`'s answer for whether this build animates the
    /// id at all. A catalog an installed data file replaced can name a preset no
    /// sampler implements; the chip says so rather than applying an edit that
    /// moves nothing.
    pub fn animation_choices(&self, slot: AnimationSlot) -> Vec<AnimationChoice> {
        let is_text = self
            .selected_ui_clip()
            .is_some_and(|clip| clip.kind == crate::types::UiClipKind::Text);
        animation_catalog()
            .iter()
            .filter(|spec| spec.slot == slot && (!spec.text_only || is_text))
            .map(|spec| AnimationChoice {
                id: spec.id.as_str(),
                label: spec.label.as_str(),
                rendered: aria_render::renders_animation(&spec.id, slot),
            })
            .collect()
    }

    /// Tag an audio clip with what it is. `None` clears the tag.
    pub fn set_selected_audio_role(&mut self, role: Option<AudioRole>) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetAudioRole { clip, role }))?;
        Ok(())
    }
}
