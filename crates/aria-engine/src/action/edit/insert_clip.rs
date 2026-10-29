use aria_models::{Clip, TrackId};

use crate::action::edit::remove_clip::RemoveClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

pub(crate) struct InsertClipAction {
    pub track: TrackId,
    pub clip: Clip,
}

impl EditAction for InsertClipAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let id = self.clip.id;
        ctx.project.timeline_mut().add_clip(self.track, self.clip)?;
        Ok(Box::new(RemoveClipAction { clip: id }))
    }
}
