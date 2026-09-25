//! Audio layout edit: channel layout of the sequence, swapped as one undoable unit.

use aria_models::AudioLayout;

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Set the sequence audio channel layout; the inverse restores the prior layout.
pub fn set_audio_layout(
    ctx: &mut ApplyContext<'_>,
    layout: AudioLayout,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx.project.audio_layout();
    ctx.project.set_audio_layout(layout);
    Ok(Box::new(RestoreAudioLayoutAction { layout: before }))
}

/// Swap the audio layout back to a captured setting (set-audio-layout undo/redo).
struct RestoreAudioLayoutAction {
    layout: AudioLayout,
}

impl EditAction for RestoreAudioLayoutAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let current = ctx.project.audio_layout();
        ctx.project.set_audio_layout(self.layout);
        Ok(Box::new(RestoreAudioLayoutAction { layout: current }))
    }
}
