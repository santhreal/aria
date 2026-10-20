//! The picks in the settings dialog's project section: the canvas,
//! the sequence rate, the audio layout and the time display.
//!
//! These three commands claimed a surface no test could see. Each row was a
//! `div()` with its setter sealed in a gpui closure, so `Set canvas`, `Set
//! sequence frame rate` and `Set audio layout` sat outside the forward reach
//! census: a row wired to the wrong setter, or deleted, turned nothing red. The
//! rows the dialog offers, including the frame rates, are declared here and the
//! panel builds each row from one of them.
//!
//! Two neighbours in the same dialog stay out, because neither is a project
//! edit:
//!
//! - The appearance and motion rows, which write the settings file and repaint
//!   the window through the gpui context. Nothing in the project changes, so
//!   there is no command to reach.
//! - The theme row, for the same reason: it persists a choice and repaints.
//!
//! The canvas background is not here either. The dialog draws no background
//! picker; the palette's reset row and the assistant are what reach it, and its
//! command row says so.

use aria_engine::EngineError;
use aria_models::{AudioLayout, CanvasAspect, Rational, TimeDisplay};

use crate::app::AppState;

/// One pick in the project section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// The canvas shape.
    Aspect(CanvasAspect),
    /// An explicit render size, or `None` to derive it from the aspect and the
    /// footage.
    Resolution(Option<[u32; 2]>),
    /// The sequence's frame rate, which retimes every placement.
    Rate(Rational),
    /// The sequence's audio channel layout.
    Layout(AudioLayout),
    /// How the sequence counts time: timecode, frames, or feet and frames.
    Display(TimeDisplay),
}

impl Pick {
    /// The picked control, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Aspect(_) => "a canvas aspect",
            Self::Resolution(None) => "the derived canvas size",
            Self::Resolution(Some(_)) => "an explicit canvas size",
            Self::Rate(_) => "a sequence frame rate",
            Self::Layout(_) => "an audio layout",
            Self::Display(_) => "a time display",
        }
    }
}

/// The frame rates the dialog offers, in the order it draws them.
///
/// The list lives with the rows rather than in the panel: it is what the surface
/// offers, and a census presses every member of it.
pub const RATES: [Rational; 8] = [
    Rational::FPS_23_976,
    Rational::FPS_24,
    Rational::FPS_25,
    Rational::FPS_29_97,
    Rational::FPS_30,
    Rational::FPS_50,
    Rational::FPS_59_94,
    Rational::FPS_60,
];

/// Install one pick on the open project.
pub fn apply(state: &mut AppState, pick: Pick) -> Result<(), EngineError> {
    match pick {
        Pick::Aspect(aspect) => state.set_canvas_aspect(aspect),
        Pick::Resolution(size) => state.set_canvas_resolution(size),
        Pick::Rate(rate) => state.set_sequence_rate(rate),
        Pick::Layout(layout) => state.set_audio_layout(layout),
        Pick::Display(display) => state.set_time_display(display),
    }
}
