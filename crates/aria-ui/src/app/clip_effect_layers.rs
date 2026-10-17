//! Layer-reference parameters: the last refused pick, shown on its effect row.
//!
//! A layer parameter that would reference its own clip, a clip outside the
//! sequence, or close a cycle through other clips' layer parameters is refused
//! by `Project::set_effect_instance`. The refusal is recorded here against the
//! clip, effect index, and parameter slot, so the inspector draws the reason
//! under that row without locking the engine. A later accepted layer pick
//! clears it.

use aria_engine::EngineError;
use aria_models::ClipId;
use gpui::SharedString;

use super::AppState;

/// The most recent refused layer pick, keyed to the row that made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LayerRefusal {
    clip: ClipId,
    index: usize,
    param: usize,
    message: SharedString,
}

impl AppState {
    /// Point layer parameter `param` of effect `index` on the selected clip at
    /// `layer`, recording a refusal for that row or clearing the last one.
    pub(crate) fn pick_selected_effect_layer(
        &mut self,
        index: usize,
        param: usize,
        layer: Option<ClipId>,
    ) -> Result<(), EngineError> {
        let result = self.set_selected_effect_layer(index, param, layer);
        self.prop_rows.layer_refusal = match (&result, self.selected_clip()) {
            (Err(err), Some(clip)) => Some(LayerRefusal {
                clip,
                index,
                param,
                message: SharedString::from(err.to_string()),
            }),
            _ => None,
        };
        result
    }

    /// The refusal recorded for layer parameter `param` of effect `index` on
    /// the selected clip, if its last pick was refused.
    pub(crate) fn effect_layer_refusal(&self, index: usize, param: usize) -> Option<SharedString> {
        let clip = self.selected_clip()?;
        self.prop_rows
            .layer_refusal
            .as_ref()
            .filter(|r| r.clip == clip && r.index == index && r.param == param)
            .map(|r| r.message.clone())
    }
}
