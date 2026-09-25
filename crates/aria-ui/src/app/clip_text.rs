//! Title text: content, and the text-effect preset catalog.
//!
//! `SetGenerator` carries the *whole* generator, so both operations here read the
//! clip's current generator out of the engine and change one field of it. Building
//! a fresh `Generator::text(content)` instead — which is what the content setter
//! used to do — silently discards the font, size, colour, alignment, stroke,
//! shadow, background, and preset. That is the "one-shot command carrying a whole
//! struct" class, and text was the last surface still hitting it.

use std::sync::LazyLock;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, ClipSource, Generator};

use super::AppState;

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
        self.selected_text_generator().is_some()
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
        let Some((clip, mut generator)) = self.selected_text_generator() else {
            return Ok(());
        };
        if let Generator::Text { style, .. } = &mut generator {
            if style.effect_preset.as_deref() == Some(id) {
                return Ok(());
            }
            style.effect_preset = Some(id.to_string());
        }
        self.apply_command(Command::Edit(EditCommand::SetGenerator { clip, generator }))?;
        Ok(())
    }

    /// Drop the preset, leaving the treatments it baked on. Clearing the chip does
    /// not undo the bake: the style is what the file holds, and a preset is a way
    /// of writing it, not a live reference.
    pub fn clear_selected_text_effect(&mut self) -> Result<(), EngineError> {
        let Some((clip, mut generator)) = self.selected_text_generator() else {
            return Ok(());
        };
        if let Generator::Text { style, .. } = &mut generator {
            if style.effect_preset.is_none() {
                return Ok(());
            }
            style.effect_preset = None;
        }
        self.apply_command(Command::Edit(EditCommand::SetGenerator { clip, generator }))?;
        Ok(())
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
        let Some((clip, mut generator)) = self.selected_text_generator() else {
            return Ok(());
        };
        if let Generator::Text { style, .. } = &mut generator {
            // An empty family means the default font, which is what `""` already is.
            style.font = preset.font_family.clone();
            if preset.font_size.is_finite() && preset.font_size > 0.0 {
                style.size = aria_models::Param::Constant(preset.font_size);
            }
            style.fill = aria_models::Param::Constant(preset.fill);
        }
        self.apply_command(Command::Edit(EditCommand::SetGenerator { clip, generator }))?;
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
    ///
    /// An empty string is a real answer and means the system default, which is
    /// what `TextStyle::font` stores for "unset" — distinct from `None`, which
    /// means there is nothing to ask about.
    pub fn selected_font(&self) -> Option<String> {
        let (_, generator) = self.selected_text_generator()?;
        match generator {
            Generator::Text { style, .. } => Some(style.font),
            _ => None,
        }
    }

    /// Set the selected title's family. `""` returns it to the system default.
    ///
    /// A family the host does not have installed is refused rather than stored:
    /// cosmic-text falls back silently, so an unknown name would render as the
    /// default while the inspector claimed otherwise. The empty name is always
    /// accepted because it *is* the default.
    pub fn set_selected_font(&mut self, family: &str) -> Result<(), EngineError> {
        let Some((clip, mut generator)) = self.selected_text_generator() else {
            return Ok(());
        };
        if !family.is_empty() && !FAMILIES.iter().any(|known| known == family) {
            self.status_message = Some(format!("No font installed named {family}"));
            return Ok(());
        }
        if let Generator::Text { style, .. } = &mut generator {
            if style.font == family {
                return Ok(());
            }
            style.font = family.to_string();
        }
        self.apply_command(Command::Edit(EditCommand::SetGenerator { clip, generator }))?;
        Ok(())
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
