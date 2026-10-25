//! Layer-style and LUT commands for the selected clip.
//!
//! `SetClipLayerStyles` carries all four style blocks at once, and `SetClipLut`
//! the whole LUT, so every setter here starts from the model's blocks and
//! changes one thing: a block a toggle leaves alone, and the intensity of a
//! reloaded LUT, keep their keyframes. The numeric knobs and the colors live
//! in `clip_params.rs`, which addresses one property at a time. What stays
//! here is which blocks exist and which file the LUT reads.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{LayerBackground, LayerGlow, LayerOutline, LayerShadow, LayerStyles, Lut};

use super::AppState;
use super::picker::{Ask, FileSpec};
use crate::types::{UiLut, UiStyles};

/// The four layer styles, in inspector order.
pub const STYLES: &[(&str, &str)] = &[
    ("shadow", "Shadow"),
    ("glow", "Glow"),
    ("outline", "Outline"),
    ("background", "Background"),
];

/// Whether one style block is present.
pub fn style_enabled(styles: &UiStyles, id: &str) -> bool {
    match id {
        "shadow" => styles.shadow.is_some(),
        "glow" => styles.glow.is_some(),
        "outline" => styles.outline.is_some(),
        "background" => styles.background.is_some(),
        _ => false,
    }
}

impl AppState {
    fn send_styles(&mut self, styles: LayerStyles) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipLayerStyles {
            clip,
            styles,
        }))?;
        Ok(())
    }

    /// Add or remove one style block, keeping the other three.
    pub fn toggle_selected_style(&mut self, id: &str) -> Result<(), EngineError> {
        let Some(mut styles) = self.selected_model().map(|model| model.styles.clone()) else {
            return Ok(());
        };
        match id {
            "shadow" => {
                styles.shadow = match styles.shadow {
                    Some(_) => None,
                    None => Some(LayerShadow::default()),
                }
            }
            "glow" => {
                styles.glow = match styles.glow {
                    Some(_) => None,
                    None => Some(LayerGlow::default()),
                }
            }
            "outline" => {
                styles.outline = match styles.outline {
                    Some(_) => None,
                    None => Some(LayerOutline::default()),
                }
            }
            "background" => {
                styles.background = match styles.background {
                    Some(_) => None,
                    None => Some(LayerBackground::default()),
                }
            }
            _ => return Ok(()),
        }
        self.send_styles(styles)
    }

    pub fn clear_selected_styles(&mut self) -> Result<(), EngineError> {
        if self.selected_clip().is_none() {
            return Ok(());
        }
        self.send_styles(LayerStyles::default())?;
        self.status_message = Some("Cleared layer styles".into());
        Ok(())
    }

    fn send_lut(&mut self, lut: Option<Lut>) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipLut { clip, lut }))?;
        Ok(())
    }

    /// Load a `.cube` LUT, keeping the current intensity when one is already
    /// loaded. An empty path is refused by the model, so it is dropped here.
    pub fn set_selected_lut(&mut self, path: impl Into<String>) -> Result<(), EngineError> {
        let Some(model) = self.selected_model() else {
            return Ok(());
        };
        let path = path.into();
        if path.trim().is_empty() {
            return Ok(());
        }
        let mut lut = Lut::new(path);
        if let Some(existing) = &model.lut {
            lut.intensity = existing.intensity.clone();
        }
        self.send_lut(Some(lut))
    }

    pub fn clear_selected_lut(&mut self) -> Result<(), EngineError> {
        if self.selected_clip().is_none() {
            return Ok(());
        }
        self.send_lut(None)?;
        self.status_message = Some("Cleared LUT".into());
        Ok(())
    }

    /// The LUT currently applied to the selection, for display.
    pub fn selected_lut(&self) -> Option<UiLut> {
        self.selected_ui_clip()?.lut
    }
}

impl AppState {
    /// Pick a `.cube` file and load it onto the selection.
    pub fn pick_and_load_lut(&mut self) {
        if self.selected_clip_or_decline().is_none() {
            return;
        }
        self.request_pick(
            Ask::OpenFile(FileSpec::new("Load LUT").filter("Cube LUT", &["cube"])),
            |state, answer| {
                let Some(path) = answer.one() else {
                    return;
                };
                if let Err(err) = state.set_selected_lut(path.to_string_lossy().to_string()) {
                    state.report_error(&err);
                }
            },
        );
    }
}
