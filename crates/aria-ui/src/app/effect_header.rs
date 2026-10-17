//! The effect header's verbs: reset, rename, duplicate, copy, paste, save as a
//! preset and remove, each on one effect named by its clip and its place in
//! the chain.
//!
//! The header's menu names its clip rather than reading the selection, so a
//! verb lands on the effect the menu was opened on. A verb that writes more
//! than one command is one undo step named for the verb, and none of them
//! writes anything when there is nothing to change.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, EffectInstance};

use super::AppState;

/// The undo step a duplicate is recorded as.
pub(crate) const DUPLICATE_EFFECT_LABEL: &str = "Duplicate effect";

/// The undo step a paste is recorded as.
pub(crate) const PASTE_EFFECT_LABEL: &str = "Paste effect";

impl AppState {
    /// The model instance at `index` in `clip`'s chain.
    pub(crate) fn effect_on(&self, clip: ClipId, index: usize) -> Option<EffectInstance> {
        let engine = self.engine.lock();
        engine
            .project()
            .timeline()
            .clip(clip)?
            .effects
            .get(index)
            .cloned()
    }

    /// How many effects `clip`'s chain holds, from the model.
    fn chain_len(&self, clip: ClipId) -> usize {
        let engine = self.engine.lock();
        engine
            .project()
            .timeline()
            .clip(clip)
            .map_or(0, |model| model.effects.len())
    }

    /// Put every parameter of the effect back at its catalog default and drop
    /// its keyframes. The name and the switch stay.
    pub fn reset_effect(&mut self, clip: ClipId, index: usize) -> Result<(), EngineError> {
        let Some(effect) = self.effect_on(clip, index) else {
            return Ok(());
        };
        let reset = effect.reset();
        let name = effect.display_name();
        if reset == effect {
            self.status_message = Some(format!("{name} is already at its defaults"));
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetEffectInstance {
            clip,
            index,
            effect: reset,
        }))?;
        self.status_message = Some(format!("Reset {name}"));
        Ok(())
    }

    /// Reset the selected clip's effect at `index`.
    pub fn reset_selected_effect(&mut self, index: usize) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.reset_effect(clip, index)
    }

    /// Move the effect at `from` in `clip`'s chain to just above the effect at
    /// `before`, or to the end of the chain when `before` is `None` or past it.
    /// Dropping an effect on itself or on the effect just below it leaves the
    /// chain as it is and writes nothing.
    pub fn move_effect_before(
        &mut self,
        clip: ClipId,
        from: usize,
        before: Option<usize>,
    ) -> Result<(), EngineError> {
        let len = self.chain_len(clip);
        if from >= len {
            return Ok(());
        }
        let before = before.filter(|&at| at < len).unwrap_or(len);
        let to = if before > from { before - 1 } else { before };
        if to == from {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::MoveEffect {
            clip,
            from_index: from,
            to_index: to,
        }))?;
        Ok(())
    }

    /// [`Self::move_effect_before`] on the selected clip's chain.
    pub fn drop_selected_effect(
        &mut self,
        from: usize,
        before: Option<usize>,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.move_effect_before(clip, from, before)
    }

    /// Give the effect `name`. An empty name, or the catalog label, takes the
    /// given name off, so the header draws the label again and follows it.
    pub fn rename_effect(
        &mut self,
        clip: ClipId,
        index: usize,
        name: &str,
    ) -> Result<(), EngineError> {
        let Some(mut effect) = self.effect_on(clip, index) else {
            return Ok(());
        };
        let name = name.trim();
        let label = aria_models::effect_spec(&effect.effect_id).map(|spec| spec.label.clone());
        let given = (!name.is_empty() && label.as_deref() != Some(name)).then(|| name.to_owned());
        if effect.name == given {
            return Ok(());
        }
        effect.name = given;
        self.apply_command(Command::Edit(EditCommand::SetEffectInstance {
            clip,
            index,
            effect,
        }))?;
        Ok(())
    }

    /// Insert a copy of the effect directly below it, with every parameter
    /// and keyframe, named after it with the next free number.
    pub fn duplicate_effect(&mut self, clip: ClipId, index: usize) -> Result<(), EngineError> {
        let Some(mut copy) = self.effect_on(clip, index) else {
            return Ok(());
        };
        copy.name = Some(self.next_free_effect_name(clip, &copy.display_name()));
        self.one_step_as(DUPLICATE_EFFECT_LABEL, |state| {
            state.insert_effect_instance(clip, copy, index + 1)
        })
    }

    /// Hold a copy of the effect, every parameter and keyframe, for a paste.
    pub fn copy_effect(&mut self, clip: ClipId, index: usize) {
        let Some(effect) = self.effect_on(clip, index) else {
            return;
        };
        self.status_message = Some(format!("Copied {}", effect.display_name()));
        self.copied_effect = Some(effect);
    }

    /// Insert the copied effect into `clip`'s chain at `at`, or at the end
    /// when `at` is past it.
    pub fn paste_effect(&mut self, clip: ClipId, at: usize) -> Result<(), EngineError> {
        let Some(effect) = self.copied_effect.clone() else {
            self.status_message = Some("Copy an effect first".into());
            return Ok(());
        };
        self.one_step_as(PASTE_EFFECT_LABEL, |state| {
            state.insert_effect_instance(clip, effect, at)
        })
    }

    /// Add the copied effect to the end of every selected clip that draws a
    /// picture, as one undo step. A clip on a sound lane is passed over: an
    /// effect there has nothing to draw on.
    pub fn paste_effect_to_selection(&mut self) -> Result<(), EngineError> {
        let Some(effect) = self.copied_effect.clone() else {
            self.status_message = Some("Copy an effect first".into());
            return Ok(());
        };
        let targets: Vec<ClipId> = {
            let engine = self.engine.lock();
            let timeline = engine.project().timeline();
            self.selection
                .iter()
                .copied()
                .filter(|&clip| {
                    timeline
                        .track_of(clip)
                        .and_then(|track| timeline.track(track))
                        .is_some_and(|track| track.kind.is_visual())
                })
                .collect()
        };
        if targets.is_empty() {
            self.status_message = Some("Select a clip that draws a picture to paste onto".into());
            return Ok(());
        }
        let name = effect.display_name();
        self.one_step_as(PASTE_EFFECT_LABEL, |state| {
            for clip in targets {
                state.insert_effect_instance(clip, effect.clone(), usize::MAX)?;
            }
            Ok(())
        })?;
        self.status_message = Some(format!("Pasted {name}"));
        Ok(())
    }

    /// Open the preset draft on the effect alone: the clip is selected, the
    /// draft holds only this effect, and the name starts as the effect's.
    pub fn save_effect_as_preset(&mut self, clip: ClipId, index: usize) {
        let Some(effect) = self.effect_on(clip, index) else {
            return;
        };
        if self.selected_clip() != Some(clip) {
            self.select_clip(clip);
        }
        self.open_preset_draft();
        let Some(draft) = self.preset_draft.as_mut() else {
            return;
        };
        for (at, _, on) in &mut draft.effects {
            *on = *at == index;
        }
        draft.include_audio_effects = false;
        for (_, _, on) in &mut draft.properties {
            *on = false;
        }
        draft.name = effect
            .display_name()
            .chars()
            .take(super::presets::PRESET_NAME_MAX_LEN)
            .collect();
        self.inspector_tab = "effects".to_string();
    }

    /// Take the effect out of `clip`'s chain.
    pub fn remove_effect(&mut self, clip: ClipId, index: usize) -> Result<(), EngineError> {
        if index >= self.chain_len(clip) {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::RemoveEffect { clip, index }))?;
        Ok(())
    }

    /// Add `effect` to `clip`'s chain and move it to `at`, inside a caller's
    /// undo step. The model has no insert, so the effect is added at the end,
    /// written whole, and moved up when `at` is inside the chain.
    fn insert_effect_instance(
        &mut self,
        clip: ClipId,
        effect: EffectInstance,
        at: usize,
    ) -> Result<(), EngineError> {
        let end = self.chain_len(clip);
        self.apply_command(Command::Edit(EditCommand::AddEffect {
            clip,
            effect_id: effect.effect_id.clone(),
        }))?;
        if effect != EffectInstance::new(effect.effect_id.clone()) {
            self.apply_command(Command::Edit(EditCommand::SetEffectInstance {
                clip,
                index: end,
                effect,
            }))?;
        }
        if at < end {
            self.apply_command(Command::Edit(EditCommand::MoveEffect {
                clip,
                from_index: end,
                to_index: at,
            }))?;
        }
        Ok(())
    }

    /// `base` with the lowest number from 2 up that no effect in `clip`'s
    /// chain is drawn as. A trailing number on `base` is replaced rather than
    /// extended, so a copy of "Blur 2" is "Blur 3" and not "Blur 2 2", and a
    /// long `base` is cut so the name stays within the model's limit.
    fn next_free_effect_name(&self, clip: ClipId, base: &str) -> String {
        let taken: Vec<String> = {
            let engine = self.engine.lock();
            engine
                .project()
                .timeline()
                .clip(clip)
                .map(|model| {
                    model
                        .effects
                        .iter()
                        .map(EffectInstance::display_name)
                        .collect()
                })
                .unwrap_or_default()
        };
        let stem = match base.rsplit_once(' ') {
            Some((stem, number))
                if !stem.is_empty() && number.parse::<u32>().is_ok_and(|n| n >= 2) =>
            {
                stem
            }
            _ => base,
        };
        let numbered = |n: u32| {
            let suffix = format!(" {n}");
            let room = aria_models::EFFECT_NAME_MAX_CHARS.saturating_sub(suffix.chars().count());
            let head: String = stem.chars().take(room).collect();
            format!("{}{suffix}", head.trim_end())
        };
        (2u32..)
            .map(numbered)
            .find(|name| !taken.iter().any(|taken| taken == name))
            .unwrap_or_else(|| stem.to_owned())
    }
}
