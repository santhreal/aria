//! Clip > Audio Channels: how a clip's left and right channels reach its lane.
//!
//! A choice other than Stereo is a channel tool (Swap Channels, Fill Right
//! with Left, Fill Left with Right, or Mono) at the head of the clip's audio
//! insert chain, so the mix, the meters, and export apply it, and the inspector
//! lists it with the clip's other audio effects. Choosing again replaces the
//! tool; Stereo removes it. Each choice is one undo step over every selected
//! clip that carries sound.

use aria_commands::{Command, EditCommand};
use aria_core::count::counted;
use aria_engine::EngineError;
use aria_models::{AudioInsertTarget, ClipId};

use super::AppState;

/// One Audio Channels choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioChannels {
    /// Left plays left and right plays right.
    Stereo,
    /// Left plays right and right plays left.
    Swap,
    /// The left channel plays on both sides.
    LeftToBoth,
    /// The right channel plays on both sides.
    RightToBoth,
    /// Both channels mixed, on both sides.
    Mono,
}

impl AudioChannels {
    /// Every choice, in menu order.
    pub const ALL: [Self; 5] = [
        Self::Stereo,
        Self::Swap,
        Self::LeftToBoth,
        Self::RightToBoth,
        Self::Mono,
    ];

    /// The Clip > Audio Channels row that picks this choice.
    pub fn row(self) -> &'static str {
        match self {
            Self::Stereo => "clip-channels-stereo",
            Self::Swap => "clip-channels-swap",
            Self::LeftToBoth => "clip-channels-left",
            Self::RightToBoth => "clip-channels-right",
            Self::Mono => "clip-channels-mono",
        }
    }

    /// The choice the row `id` picks, or `None` for any other row.
    pub fn of_row(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|choice| choice.row() == id)
    }

    /// The menu label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Stereo => "Stereo",
            Self::Swap => "Swap Channels",
            Self::LeftToBoth => "Left to Both",
            Self::RightToBoth => "Right to Both",
            Self::Mono => "Mono",
        }
    }

    /// The shipped channel tool that plays this choice, by audio effect id.
    pub fn tool(self) -> Option<&'static str> {
        match self {
            Self::Stereo => None,
            Self::Swap => Some("swap_channels"),
            Self::LeftToBoth => Some("fill_right"),
            Self::RightToBoth => Some("fill_left"),
            Self::Mono => Some("mono"),
        }
    }

    /// The choice a channel tool plays, or `None` for any other effect.
    fn of_tool(effect_id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|choice| choice.tool() == Some(effect_id))
    }
}

impl AppState {
    /// The selected clips as projected, in lane order. A menu draw reads these,
    /// so they come from the store rather than the session.
    fn selected_rows(&self) -> impl Iterator<Item = &crate::types::UiClip> {
        self.store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .filter(|clip| self.selection.contains(&ClipId::from_raw(clip.id)))
    }

    /// Whether a selected clip's audio insert chain accepts an effect.
    pub(crate) fn selection_has_sound(&self) -> bool {
        self.selected_rows().any(|clip| clip.can_hold_inserts)
    }

    /// The selected clips whose audio insert chain accepts an effect.
    pub(crate) fn sounding_selection(&self) -> Vec<ClipId> {
        self.selected_rows()
            .filter(|clip| clip.can_hold_inserts)
            .map(|clip| ClipId::from_raw(clip.id))
            .collect()
    }

    /// The choice every selected clip with sound plays, or `None` when they
    /// differ or none is selected. The first channel tool in a chain that is
    /// not bypassed is the clip's choice.
    pub fn selection_audio_channels(&self) -> Option<AudioChannels> {
        let mut agreed = None;
        for clip in self.selected_rows().filter(|clip| clip.can_hold_inserts) {
            let choice = clip
                .audio_inserts
                .iter()
                .filter(|insert| !insert.bypass)
                .find_map(|insert| AudioChannels::of_tool(&insert.effect_id))
                .unwrap_or(AudioChannels::Stereo);
            match agreed {
                None => agreed = Some(choice),
                Some(seen) if seen != choice => return None,
                Some(_) => {}
            }
        }
        agreed
    }

    /// Clip > Audio Channels > `choice` on every selected clip with sound, as
    /// one undo step.
    pub fn set_audio_channels(&mut self, choice: AudioChannels) -> Result<(), EngineError> {
        let clips = self.sounding_selection();
        if clips.is_empty() {
            self.status_message = Some("Select a clip with sound".into());
            return Ok(());
        }
        let plans: Vec<(ClipId, Vec<usize>, usize)> = {
            let engine = self.engine.lock();
            let project = engine.project();
            clips
                .iter()
                .filter_map(|&clip| {
                    let chain = project.audio_inserts(AudioInsertTarget::Clip(clip)).ok()?;
                    let tools: Vec<usize> = chain
                        .iter()
                        .enumerate()
                        .filter(|(_, insert)| AudioChannels::of_tool(&insert.effect_id).is_some())
                        .map(|(index, _)| index)
                        .collect();
                    let kept = chain.len() - tools.len();
                    Some((clip, tools, kept))
                })
                .collect()
        };
        self.one_step(|this| {
            for (clip, tools, kept) in plans {
                let target = AudioInsertTarget::Clip(clip);
                // Highest index first, so each removal leaves the indices still
                // to remove where they were.
                for index in tools.into_iter().rev() {
                    this.apply_command(Command::Edit(EditCommand::RemoveAudioEffect {
                        target,
                        index,
                    }))?;
                }
                let Some(tool) = choice.tool() else {
                    continue;
                };
                this.apply_command(Command::Edit(EditCommand::AddAudioEffect {
                    target,
                    effect_id: tool.to_owned(),
                }))?;
                if kept > 0 {
                    this.apply_command(Command::Edit(EditCommand::MoveAudioEffect {
                        target,
                        from_index: kept,
                        to_index: 0,
                    }))?;
                }
            }
            Ok(())
        })?;
        self.status_message = Some(format!(
            "Audio channels: {} on {}",
            choice.label(),
            counted(clips.len(), "clip", "clips")
        ));
        Ok(())
    }
}

#[cfg(test)]
#[path = "audio_channels_tests.rs"]
mod tests;
