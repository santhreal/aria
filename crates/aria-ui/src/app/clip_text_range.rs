//! Styling a range of a title's text from the Character panel.
//!
//! With characters selected in the Viewer by the Type tool, the face switches
//! (Bold, Italic, Underline), the font, the size, the tracking and the fill
//! read the run the selection starts in and write a [`TextStyleRun`] over the
//! selection. With nothing selected they read and write the whole title. The
//! other switches, the case, the treatments and the paragraph are the whole
//! title's in either case: a run does not hold them.

use aria_engine::EngineError;
use aria_models::{ClipParam, TextParam, TextStyle, TextStyleRun};

use super::{AppState, TextFace, TextSwitch};

/// The run field a property writes over a range, for the properties a run
/// holds.
fn run_param(param: ClipParam) -> Option<TextParam> {
    match param {
        ClipParam::Text {
            param: field @ (TextParam::Size | TextParam::LetterSpacing | TextParam::Fill),
        } => Some(field),
        _ => None,
    }
}

impl TextFace {
    /// The chips at character `index`: the title's face with the overrides of
    /// the run holding it on top.
    pub(super) fn at(style: &TextStyle, index: usize) -> Self {
        let mut face = Self::of(style);
        if let Some(run) = style.run_at(index) {
            for (switch, set) in [
                (TextSwitch::Bold, run.bold),
                (TextSwitch::Italic, run.italic),
                (TextSwitch::Underline, run.underline),
            ] {
                if let Some(on) = set {
                    face.on[switch as usize] = on;
                }
            }
        }
        face
    }
}

impl TextSwitch {
    /// Set this switch in a run. `false` for a switch a run does not hold.
    pub(super) fn set_run(self, run: &mut TextStyleRun, on: bool) -> bool {
        let slot = match self {
            Self::Bold => &mut run.bold,
            Self::Italic => &mut run.italic,
            Self::Underline => &mut run.underline,
            Self::Kerning | Self::Wrap | Self::Stroke | Self::Shadow | Self::Background => {
                return false;
            }
        };
        *slot = Some(on);
        true
    }

    /// Whether a run holds this switch.
    pub(super) fn per_run(self) -> bool {
        matches!(self, Self::Bold | Self::Italic | Self::Underline)
    }
}

impl AppState {
    /// Write `update` over the selected characters as a style run, as one
    /// edit. `false`, and nothing written, when nothing is selected: the caller
    /// styles the whole title.
    pub(super) fn style_text_range(
        &mut self,
        update: impl FnMut(&mut TextStyleRun),
    ) -> Result<bool, EngineError> {
        let Some(range) = self.text_style_range() else {
            return Ok(false);
        };
        self.edit_selected_text_style(|style| {
            let before = style.runs.clone();
            style.apply_range_override(range.start, range.end, update);
            style.runs != before
        })?;
        Ok(true)
    }

    /// The run the selection starts in, for a property a run holds.
    fn range_run(&self, param: ClipParam) -> Option<&TextStyleRun> {
        run_param(param)?;
        let range = self.text_style_range()?;
        self.selected_text_style()?.run_at(range.start)
    }

    /// What a scalar row of the selected title reads: the run's value under a
    /// selection that sets it, else `value`, the title's own.
    pub(crate) fn text_run_scalar(&self, param: ClipParam, value: f32) -> f32 {
        let Some(run) = self.range_run(param) else {
            return value;
        };
        match run_param(param) {
            Some(TextParam::Size) => run.size,
            Some(TextParam::LetterSpacing) => run.letter_spacing,
            _ => None,
        }
        .unwrap_or(value)
    }

    /// What the fill row of the selected title reads: the run's under a
    /// selection that sets it, else `value`, the title's own.
    pub(crate) fn text_run_color(&self, param: ClipParam, value: [u8; 4]) -> [u8; 4] {
        self.range_run(param)
            .and_then(|run| run.fill)
            .unwrap_or(value)
    }

    /// Write a scalar over the selected characters. `false` when the property
    /// is not one a run holds or nothing is selected, and the caller writes the
    /// whole title.
    pub(crate) fn style_text_run_scalar(
        &mut self,
        param: ClipParam,
        value: f32,
    ) -> Result<bool, EngineError> {
        let Some(field) = run_param(param) else {
            return Ok(false);
        };
        let Some(bounds) = self.key_param(param).map(|row| row.spec.range) else {
            return Ok(false);
        };
        if !value.is_finite() {
            return Ok(self.text_style_range().is_some());
        }
        let value = bounds.clamp(value);
        match field {
            TextParam::Size => self.style_text_range(|run| run.size = Some(value)),
            TextParam::LetterSpacing => {
                self.style_text_range(|run| run.letter_spacing = Some(value))
            }
            _ => Ok(false),
        }
    }

    /// Write the fill over the selected characters. `false` when the property
    /// is not the fill or nothing is selected.
    pub(crate) fn style_text_run_color(
        &mut self,
        param: ClipParam,
        rgba: [u8; 4],
    ) -> Result<bool, EngineError> {
        if run_param(param) != Some(TextParam::Fill) {
            return Ok(false);
        }
        self.style_text_range(|run| run.fill = Some(rgba))
    }
}

#[cfg(test)]
#[path = "clip_text_range_tests.rs"]
mod tests;
