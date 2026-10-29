use aria_models::MediaSource;

use crate::action::edit::remove_media::RemoveMediaAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

pub(crate) struct InsertMediaAction {
    pub media: MediaSource,
}

impl EditAction for InsertMediaAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let id = self.media.id;
        ctx.project.add_media(self.media);
        Ok(Box::new(RemoveMediaAction { media: id }))
    }
}
