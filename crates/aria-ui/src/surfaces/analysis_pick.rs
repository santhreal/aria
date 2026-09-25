//! What an operator presses in the Analysis panel once a detector has run: the
//! clear beside a detector, the refresh, the two filters, the two seeks, and the
//! two edits that act on what was found.
//!
//! `Clear beats` claimed the panel's clear button and nothing could press it, so
//! a clear wired to the wrong detector would have deleted another one's batch
//! with nothing red. Every control that does not need the window is declared
//! here and the panel builds it from a row.
//!
//! Two controls stay out, and neither loses a command by it:
//!
//! - **Detect**, one per detector, and **Loudness**. Both queue a job through
//!   the window's context ([`AppState::analyze_selected`] takes a
//!   `Context<AppState>`), and a job is not a command. What the beat run writes
//!   when it finishes is `AppState::store_beats`, which the census drives
//!   directly, because that write is the surface `Set beats` names.
//! - The moment rows themselves. Clicking one seeks, which the two seek rows
//!   here already cover, and the row's own control is the seek.
//!
//! The two edits are refused rather than clamped when the detector found nothing
//! inside the selection: that is the model's answer, and the panel only draws
//! them once the count it read is above zero.

use aria_engine::EngineError;

use crate::app::AppState;
use crate::app::analysis::AnalysisKind;

/// One control the Analysis panel draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pick {
    /// Drop one detector's stored results for the selected media, and the grid
    /// it wrote on the clip.
    Clear(AnalysisKind),
    /// Re-read the index into the rows.
    Refresh,
    /// Show one detector's moments, or all of them.
    Filter(Option<AnalysisKind>),
    /// Hide moments the detector was less sure of than this.
    Confidence(f32),
    /// Move the playhead to the moment before it.
    SeekPrevious,
    /// Move the playhead to the moment after it.
    SeekNext,
    /// Cut the selected clip at every detected shot boundary inside it.
    SplitAtShots,
    /// Remove every detected silent span inside the selection, closing the gaps.
    CutSilence,
}

impl Pick {
    /// What the operator pressed, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Clear(_) => "clear one detector's results",
            Self::Refresh => "re-read the moments index",
            Self::Filter(_) => "filter the moments by detector",
            Self::Confidence(_) => "filter the moments by confidence",
            Self::SeekPrevious => "seek to the moment before the playhead",
            Self::SeekNext => "seek to the moment after the playhead",
            Self::SplitAtShots => "split the clip at its shot boundaries",
            Self::CutSilence => "ripple-delete the silence in the selection",
        }
    }
}

/// Press one control on the open project.
///
/// The refusals arrive as prose from two places: the index says why a read or a
/// delete failed, and the model says why an edit was refused. Both are what the
/// panel puts in front of the operator, so both come back as one string.
pub fn apply(state: &mut AppState, pick: Pick) -> Result<(), String> {
    match pick {
        Pick::Clear(kind) => state.clear_analysis(kind),
        Pick::Refresh => state.refresh_moments(),
        Pick::Filter(kind) => {
            state.set_analysis_kind_filter(kind);
            Ok(())
        }
        Pick::Confidence(minimum) => {
            state.set_analysis_minimum_confidence(minimum);
            Ok(())
        }
        Pick::SeekPrevious => {
            state.seek_previous_moment();
            Ok(())
        }
        Pick::SeekNext => {
            state.seek_next_moment();
            Ok(())
        }
        Pick::SplitAtShots => state.split_at_shots().map_err(describe),
        Pick::CutSilence => state.ripple_delete_silence().map_err(describe),
    }
}

/// The same prose the rest of the editor shows for an engine refusal.
fn describe(err: EngineError) -> String {
    crate::app::describe_engine_error(&err)
}
