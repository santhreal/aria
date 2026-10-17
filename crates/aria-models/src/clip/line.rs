//! The property a clip's bar on the timeline draws as its line.

use super::{Clip, ClipParam, ParamScope, ParamSpec, clip_params_for, param_spec};

/// The property a clip's bar draws while none is chosen: Volume on an audio
/// lane, Opacity on any other lane.
pub fn default_line_param(audio_lane: bool) -> ClipParam {
    if audio_lane {
        ClipParam::Volume
    } else {
        ClipParam::Opacity
    }
}

impl Clip {
    /// The row of `param` on this clip, when the clip has that property and a
    /// clip on an audio lane would offer it: an audio lane offers no picture
    /// property.
    pub fn line_row(&self, param: ClipParam, audio_lane: bool) -> Option<ParamSpec> {
        let row = match param {
            // An effect parameter or a mask property exists once per effect or
            // mask on this clip, so it has no static row.
            ClipParam::Effect { .. } | ClipParam::Mask { .. } => clip_params_for(self)
                .into_iter()
                .find(|row| row.param == param)?,
            _ => param_spec(param)
                .filter(|row| row.available(self))
                .cloned()?,
        };
        offered_on(&row, audio_lane).then_some(row)
    }

    /// Every row the clip's bar can draw as its line, in catalog order. One
    /// pass over the catalog, where [`Clip::line_row`] per effect parameter
    /// rebuilds it once per row.
    pub fn line_rows(&self, audio_lane: bool) -> Vec<ParamSpec> {
        clip_params_for(self)
            .into_iter()
            .filter(|row| offered_on(row, audio_lane))
            .collect()
    }

    /// The row the clip's bar draws as its line: [`Clip::line_param`] while
    /// the clip still has that property, else the lane's default. `None` when
    /// the clip has neither, as a generated clip on an audio lane has no
    /// Volume.
    pub fn line_spec(&self, audio_lane: bool) -> Option<ParamSpec> {
        self.line_param
            .and_then(|param| self.line_row(param, audio_lane))
            .or_else(|| self.line_row(default_line_param(audio_lane), audio_lane))
    }
}

/// Whether a bar on an audio lane, or on any other lane when `audio_lane` is
/// false, may draw `row`: an audio lane offers no picture property.
fn offered_on(row: &ParamSpec, audio_lane: bool) -> bool {
    !(audio_lane && matches!(row.scope, ParamScope::Visual))
}
