//! Where each camera of a group belongs on the group's clock, measured from the
//! sound the cameras recorded.
//!
//! # Why
//!
//! An angle's sync offset was a field nothing set. A group built from files that
//! did not start rolling together therefore cut to the wrong moment on every
//! camera but the first, and the only fix on offer was to rebuild the group.
//!
//! The measurement is a correlation of the peak envelopes the timeline already
//! decodes, so syncing a group costs no decode of its own and needs no second
//! representation of a source's sound. The first angle is the clock: it keeps
//! the offset it has, and every other angle is placed against it. Aligning
//! everything to the first camera rather than to a computed average is what lets
//! an operator sync twice and get the same answer.
//!
//! Nothing here reads the project or the store. The peaks come in, the offsets
//! come out, and the reasons an angle could not be placed come out beside them,
//! so the whole rule is testable without a window, a GPU, or a decode.

use aria_analysis::align_anywhere;

use super::decline::Decline;

/// The weakest correlation an offset may be applied from.
///
/// Measured rather than chosen: two envelopes of the same sound correlate above
/// 0.99 even after one is trimmed, and two unrelated recordings of the same
/// length score below 0.5 (`aria-analysis`' alignment tests pin both). Half is
/// therefore the gap between the two populations, and an angle that lands in it
/// is left where it is and reported, because an offset applied from a weak match
/// moves a camera that was already in sync.
pub const MIN_SYNC_SCORE: f32 = 0.5;

/// How far the best match must beat the next answer somewhere else.
///
/// A repeating sound matches equally well at every repeat, so the winner is
/// whichever repeat the search reached first. That is a coin toss, and applying
/// it moves a camera that may already have been right, so the gap between the
/// best score and the best score elsewhere has to be real. A shifted copy of a
/// recording beats its own rivals by more than half; a metronome beats them by
/// nothing at all (`multicam_sync_tests` pins both), so a twentieth is inside
/// the gap between those two populations and well clear of arithmetic noise.
pub const MIN_SYNC_MARGIN: f32 = 0.05;

/// One angle's measured place on the group's clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    /// Index into the group's angles.
    pub angle: usize,
    /// Where that camera's first frame belongs, in ticks of the sequence rate.
    pub offset: i64,
}

/// Every angle that measured, and the first reason one did not.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    /// The angles to move, in angle order. An angle already at its measured
    /// offset is included: the caller compares, because "nothing moved" is a
    /// different sentence from "nothing measured".
    pub moved: Vec<Measured>,
    /// Why an angle was left alone, when one was.
    pub refused: Option<Decline>,
}

/// Place every angle after the first against the first.
///
/// `angles[0]` is the reference and is never moved. Each entry is either that
/// camera's peak envelope or the reason it has none: the caller owns that
/// distinction, because "still being read" and "has no sound" are facts about
/// the store rather than about the measurement.
///
/// `ticks_per_sample` converts a lag in envelope samples into ticks of the
/// sequence rate, so this function needs to know neither rate.
///
/// `Err` is a reason the group cannot be synced at all. Per-angle reasons come
/// back in [`Measurement::refused`], because one silent camera is no reason to
/// leave the others out of sync.
pub fn measure(
    angles: &[Result<&[f32], Decline>],
    reference_offset: i64,
    ticks_per_sample: f64,
) -> Result<Measurement, Decline> {
    if angles.len() < 2 {
        return Err(Decline::OneAngleOnly);
    }
    let reference = angles[0]?;
    let mut moved = Vec::new();
    let mut refused = None;
    for (angle, sound) in angles.iter().enumerate().skip(1) {
        let mut refuse = |why: Decline| {
            if refused.is_none() {
                refused = Some(why);
            }
        };
        let peaks = match sound {
            Ok(peaks) => *peaks,
            Err(reason) => {
                refuse(*reason);
                continue;
            }
        };
        // No search radius: two cameras of one event can start any distance
        // apart, and a radius in seconds would be an invented fact about the
        // shoot.
        let Some(found) = align_anywhere(reference, peaks) else {
            refuse(Decline::NoAngleSound);
            continue;
        };
        if found.score < MIN_SYNC_SCORE {
            refuse(Decline::AnglesDoNotMatch);
            continue;
        }
        if found.score - found.rival < MIN_SYNC_MARGIN {
            refuse(Decline::AnglesRepeat);
            continue;
        }
        let ticks = (found.lag as f64 * ticks_per_sample).round() as i64;
        moved.push(Measured {
            angle,
            offset: reference_offset + ticks,
        });
    }
    if moved.is_empty() {
        return Err(refused.unwrap_or(Decline::AnglesDoNotMatch));
    }
    Ok(Measurement { moved, refused })
}

#[cfg(test)]
#[path = "multicam_sync_tests.rs"]
mod tests;
