//! Title text: content, the face and paragraph switches, case, alignment, the
//! font, and the text-effect preset catalog.
//!
//! `SetGenerator` carries the *whole* generator, so every operation here reads
//! the clip's current generator out of the engine and changes one field of it. Building
//! a fresh `Generator::text(content)` instead — which is what the content setter
//! used to do — silently discards the font, size, colour, alignment, stroke,
//! shadow, background, and preset. That is the "one-shot command carrying a whole
//! struct" class, and text was the last surface still hitting it.

use std::sync::LazyLock;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    ClipId, ClipSource, Generator, TextAlignH, TextAlignV, TextBackground, TextCase, TextShadow,
    TextStroke, TextStyle,
};

use super::AppState;

#[path = "clip_text_range.rs"]
mod range;
#[path = "text_caret.rs"]
pub(crate) mod text_caret;
#[path = "tool_hold.rs"]
pub(crate) mod tool_hold;

/// One on/off property of a title's type, drawn as a chip in the Character and
/// Paragraph panels and the inspector's Text tab.
///
/// The outline, the shadow and the card are optional blocks in the model. The
/// switch puts one in at its default look or takes it out, and the property rows
/// that belong to it arrive and leave with it. Switching one clears the text
/// effect preset, because the treatments no longer read as that preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSwitch {
    Bold,
    Italic,
    Underline,
    Kerning,
    Wrap,
    Stroke,
    Shadow,
    Background,
}

impl TextSwitch {
    /// Every switch, in declaration order, so a switch's discriminant is its
    /// index here.
    pub const ALL: [Self; 8] = [
        Self::Bold,
        Self::Italic,
        Self::Underline,
        Self::Kerning,
        Self::Wrap,
        Self::Stroke,
        Self::Shadow,
        Self::Background,
    ];

    /// The switches of the face, drawn with the font in the Character panel.
    pub const FACE: [Self; 4] = [Self::Bold, Self::Italic, Self::Underline, Self::Kerning];

    /// The treatments drawn around the glyphs, each an optional block.
    pub const TREATMENTS: [Self; 3] = [Self::Stroke, Self::Shadow, Self::Background];

    pub fn label(self) -> &'static str {
        match self {
            Self::Bold => "Bold",
            Self::Italic => "Italic",
            Self::Underline => "Underline",
            Self::Kerning => "Kerning",
            Self::Wrap => "Wrap",
            Self::Stroke => "Stroke",
            Self::Shadow => "Shadow",
            Self::Background => "Card",
        }
    }

    /// Whether the switch is on in `style`.
    pub fn of(self, style: &TextStyle) -> bool {
        match self {
            Self::Bold => style.bold,
            Self::Italic => style.italic,
            Self::Underline => style.underline,
            Self::Kerning => style.kerning,
            Self::Wrap => style.wrap,
            Self::Stroke => style.stroke.is_some(),
            Self::Shadow => style.shadow.is_some(),
            Self::Background => style.background.is_some(),
        }
    }

    fn flip(self, style: &mut TextStyle) {
        match self {
            Self::Bold => style.bold = !style.bold,
            Self::Italic => style.italic = !style.italic,
            Self::Underline => style.underline = !style.underline,
            Self::Kerning => style.kerning = !style.kerning,
            Self::Wrap => style.wrap = !style.wrap,
            Self::Stroke => {
                if style.stroke.take().is_none() {
                    style.stroke = Some(TextStroke::default());
                }
                style.effect_preset = None;
            }
            Self::Shadow => {
                if style.shadow.take().is_none() {
                    style.shadow = Some(TextShadow::default());
                }
                style.effect_preset = None;
            }
            Self::Background => {
                if style.background.take().is_none() {
                    style.background = Some(TextBackground::default());
                }
                style.effect_preset = None;
            }
        }
    }
}

/// The parts of a title's style a panel draws as chips, copied out so a draw
/// holds no borrow of the state while it builds listeners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextFace {
    pub case: TextCase,
    pub align_h: TextAlignH,
    pub align_v: TextAlignV,
    on: [bool; TextSwitch::ALL.len()],
}

impl TextFace {
    fn of(style: &TextStyle) -> Self {
        Self {
            case: style.case,
            align_h: style.align_h,
            align_v: style.align_v,
            on: TextSwitch::ALL.map(|switch| switch.of(style)),
        }
    }

    pub fn is_on(self, switch: TextSwitch) -> bool {
        self.on[switch as usize]
    }
}

/// Every installed font family, scanned once.
///
/// The scan is the expensive part (system font directories, hundreds of
/// milliseconds) and its answer does not change while the editor runs, so it is
/// paid for on the first ask and shared from then on. A `LazyLock` rather than a
/// field on the state: nothing about it is per-project, and a field would have to
/// be threaded through every constructor to say the same thing.
static FAMILIES: LazyLock<Vec<String>> = LazyLock::new(aria_text::system_font_families);

impl AppState {
    /// The selected clip's generator, if it is a title.
    fn selected_text_generator(&self) -> Option<(ClipId, Generator)> {
        let clip = self.selected_clip()?;
        match &self.selected_model()?.content {
            ClipSource::Generated(generator @ Generator::Text { .. }) => {
                Some((clip, generator.clone()))
            }
            _ => None,
        }
    }

    /// Whether the selection is a title, so the caller can hide the controls
    /// instead of offering a chip that would be refused.
    pub fn selected_is_title(&self) -> bool {
        self.selected_text_style().is_some()
    }

    /// The selected title's style, or `None` when the selection is not a title.
    pub fn selected_text_style(&self) -> Option<&TextStyle> {
        match &self.selected_model()?.content {
            ClipSource::Generated(Generator::Text { style, .. }) => Some(style),
            _ => None,
        }
    }

    /// The selected title's chips, or `None` when the selection is not a title.
    /// With characters selected in the Viewer, the face of the first of them.
    pub fn selected_text_face(&self) -> Option<TextFace> {
        let style = self.selected_text_style()?;
        Some(match self.text_style_range() {
            Some(range) => TextFace::at(style, range.start),
            None => TextFace::of(style),
        })
    }

    /// Change the selected title's style and send the whole generator, or send
    /// nothing when `edit` reports that it changed nothing. Inert when the
    /// selection is not a title.
    fn edit_selected_text_style(
        &mut self,
        edit: impl FnOnce(&mut TextStyle) -> bool,
    ) -> Result<(), EngineError> {
        let Some((clip, mut generator)) = self.selected_text_generator() else {
            return Ok(());
        };
        let Generator::Text { style, .. } = &mut generator else {
            return Ok(());
        };
        if !edit(style) {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetGenerator { clip, generator }))?;
        Ok(())
    }

    /// Flip one switch of the selected title, or, for a switch a run holds,
    /// of the characters selected in the Viewer.
    pub fn toggle_selected_text_switch(&mut self, switch: TextSwitch) -> Result<(), EngineError> {
        if switch.per_run()
            && let Some(face) = self.selected_text_face()
        {
            let on = !face.is_on(switch);
            if self.style_text_range(|run| {
                switch.set_run(run, on);
            })? {
                return Ok(());
            }
        }
        self.edit_selected_text_style(|style| {
            switch.flip(style);
            true
        })
    }

    /// Set how the selected title's letters are cased. Inert on the case in force.
    pub fn set_selected_text_case(&mut self, case: TextCase) -> Result<(), EngineError> {
        self.edit_selected_text_style(|style| std::mem::replace(&mut style.case, case) != case)
    }

    /// Set where the selected title's lines sit across the frame. Inert on the
    /// alignment in force.
    pub fn set_selected_text_align_h(&mut self, align: TextAlignH) -> Result<(), EngineError> {
        self.edit_selected_text_style(|style| std::mem::replace(&mut style.align_h, align) != align)
    }

    /// Set where the selected title's block sits down the frame. Inert on the
    /// alignment in force.
    pub fn set_selected_text_align_v(&mut self, align: TextAlignV) -> Result<(), EngineError> {
        self.edit_selected_text_style(|style| std::mem::replace(&mut style.align_v, align) != align)
    }

    /// Replace a title's content, keeping its style. `SetGenerator` carries the
    /// whole generator, so the style is resent from the project.
    pub fn set_selected_text(&mut self, content: String) -> Result<(), EngineError> {
        let Some((clip, mut generator)) = self.selected_text_generator() else {
            return Ok(());
        };
        if let Generator::Text {
            content: existing, ..
        } = &mut generator
        {
            if *existing == content {
                return Ok(());
            }
            *existing = content;
        }
        self.apply_command(Command::Edit(EditCommand::SetGenerator { clip, generator }))?;
        Ok(())
    }

    /// Apply a text-effect preset to the selected title. The engine bakes the
    /// preset's stroke, shadow, and background onto the style, so the saved file
    /// stays self-describing and a renderer never needs the catalog.
    pub fn set_selected_text_effect(&mut self, id: &str) -> Result<(), EngineError> {
        self.edit_selected_text_style(|style| {
            if style.effect_preset.as_deref() == Some(id) {
                return false;
            }
            style.effect_preset = Some(id.to_string());
            true
        })
    }

    /// Drop the preset, leaving the treatments it baked on. Clearing the chip does
    /// not undo the bake: the style is what the file holds, and a preset is a way
    /// of writing it, not a live reference.
    pub fn clear_selected_text_effect(&mut self) -> Result<(), EngineError> {
        self.edit_selected_text_style(|style| style.effect_preset.take().is_some())
    }

    /// Apply a cloud text preset to the selected title: font, size, fill, then the
    /// preset's look animations.
    ///
    /// An unknown animation id is skipped rather than refused, per the catalog
    /// contract: a pack authored against a newer build must still apply the parts this
    /// one understands. `set_selected_animation` is already inert on an unknown id.
    pub fn apply_text_preset(
        &mut self,
        preset: &aria_cloud::dto::TextPreset,
    ) -> Result<(), EngineError> {
        if !self.selected_is_title() {
            return Ok(());
        }
        self.edit_selected_text_style(|style| {
            // An empty family means the default font, which is what `""` already is.
            style.font = preset.font_family.clone();
            if preset.font_size.is_finite() && preset.font_size > 0.0 {
                style.size = aria_models::Param::Constant(preset.font_size);
            }
            style.fill = aria_models::Param::Constant(preset.fill);
            true
        })?;
        for id in [
            preset.animation_in.as_deref(),
            preset.animation_out.as_deref(),
            preset.animation_combo.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            self.set_selected_animation(id)?;
        }
        Ok(())
    }

    /// Every installed font family, in name order.
    ///
    /// The text crate parses the host catalog once and this UI handle keeps the
    /// resulting list for the process. The picker therefore reads a slice rather
    /// than allocating the list on every frame.
    pub fn font_families(&self) -> &'static [String] {
        &FAMILIES
    }

    /// The selected title's family, or `None` when the selection is not a title.
    /// With characters selected in the Viewer, the family of the first of them.
    ///
    /// An empty string is a real answer and means the system default, which is
    /// what `TextStyle::font` stores for "unset" — distinct from `None`, which
    /// means there is nothing to ask about.
    pub fn selected_font(&self) -> Option<String> {
        let style = self.selected_text_style()?;
        let run = self
            .text_style_range()
            .and_then(|range| style.run_at(range.start))
            .and_then(|run| run.font.as_ref());
        Some(run.unwrap_or(&style.font).clone())
    }

    /// Set the selected title's family. `""` returns it to the system default.
    ///
    /// A family the host does not have installed is refused rather than stored:
    /// cosmic-text falls back silently, so an unknown name would render as the
    /// default while the inspector claimed otherwise. The empty name is always
    /// accepted because it *is* the default.
    pub fn set_selected_font(&mut self, family: &str) -> Result<(), EngineError> {
        if !self.selected_is_title() {
            return Ok(());
        }
        if !family.is_empty() && !FAMILIES.iter().any(|known| known == family) {
            self.status_message = Some(format!("No font installed named {family}"));
            return Ok(());
        }
        // Over a selection the empty name drops the run's family, and the
        // characters take the title's.
        let run_font = (!family.is_empty()).then(|| family.to_string());
        if self.style_text_range(|run| run.font.clone_from(&run_font))? {
            return Ok(());
        }
        self.edit_selected_text_style(|style| {
            if style.font == family {
                return false;
            }
            style.font = family.to_string();
            true
        })
    }

    /// Step one family along the installed list, wrapping at both ends.
    ///
    /// The step is what a key or a palette row can carry; the picker's rows are
    /// the same list addressed by name. Wrapping rather than clamping, because a
    /// clamped step at the end of the list looks like a dead key.
    pub fn cycle_selected_font(&mut self, forward: bool) -> Result<(), EngineError> {
        if FAMILIES.is_empty() {
            self.status_message = Some("No fonts installed".into());
            return Ok(());
        }
        let Some(current) = self.selected_font() else {
            return Ok(());
        };
        let at = FAMILIES.iter().position(|name| *name == current);
        let next = match (at, forward) {
            // Unset, or a family that was uninstalled since: stepping forward
            // starts at the top of the list and back starts at the bottom.
            (None, true) => 0,
            (None, false) => FAMILIES.len() - 1,
            (Some(at), true) => (at + 1) % FAMILIES.len(),
            (Some(at), false) => (at + FAMILIES.len() - 1) % FAMILIES.len(),
        };
        let family = FAMILIES[next].clone();
        self.set_selected_font(&family)
    }

    /// The families a picker should draw: `span` names centred on the one in use,
    /// each with whether it is the one in use.
    ///
    /// A window rather than the whole list, because a host has hundreds of
    /// families and an inspector column that drew all of them would spend a frame
    /// laying out names nobody is looking at. Centred on the selection so
    /// stepping through with the palette row scrolls the window, and clamped to
    /// the ends so the last families are reachable by click too.
    pub fn font_window(&self, span: usize) -> Vec<(String, bool)> {
        if span == 0 || FAMILIES.is_empty() {
            return Vec::new();
        }
        let current = self.selected_font().unwrap_or_default();
        let at = FAMILIES
            .iter()
            .position(|name| *name == current)
            .unwrap_or(0);
        let span = span.min(FAMILIES.len());
        let half = span / 2;
        let first = at.saturating_sub(half).min(FAMILIES.len() - span);
        FAMILIES[first..first + span]
            .iter()
            .map(|name| (name.clone(), *name == current))
            .collect()
    }
}
