//! Set a media-pool entry's label colour.
//!
//! Self-inverse: applying it returns the same action carrying the colour that
//! was there before, so undo and redo are the same code path.

use aria_models::{LabelColor, MediaId, ModelError};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

pub(crate) struct SetMediaLabelAction {
    pub media: MediaId,
    pub label: Option<LabelColor>,
}

pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    media: MediaId,
    label: Option<LabelColor>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let source = ctx
        .project
        .media_mut(media)
        .ok_or(EngineError::Model(ModelError::UnknownMedia(media)))?;
    let before = std::mem::replace(&mut source.label, label);
    Ok(Box::new(SetMediaLabelAction {
        media,
        label: before,
    }))
}

impl EditAction for SetMediaLabelAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        execute(ctx, self.media, self.label)
    }
}
