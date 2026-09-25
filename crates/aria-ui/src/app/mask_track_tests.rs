//! The mask tracking pass, driven by scripted frames.
//!
//! The class these close: a tracker is judged by what it does when it fails. A
//! pass that writes a keyframe per frame regardless of confidence produces an
//! animation that looks authored and drifts off the subject; one that cannot be
//! stopped holds the window for the length of the range. So the cases are: a
//! feature that moves is followed, a feature that is lost stops the pass at the
//! tick that lost it, a cancel ends it early, and every keyframe is one undo
//! entry.
//!
//! Frames are synthetic, so nothing here needs a GPU. The scripted source
//! behaves like the shipped one: one frame per tick, `None` past the end.

use std::sync::atomic::AtomicBool;

use aria_commands::{Command, EditCommand};
use aria_core::RgbaImage;
use aria_models::{CanvasAspect, MaskKind, Param, RationalTime};

use super::*;
use crate::app::transform_handle_tests::selected;

/// A frame with a patterned 12 x 12 patch whose top-left is `(sx, sy)`.
///
/// The patch is four differently-lit quadrants rather than one flat square: a
/// mask sits *on* the feature, so the pixels the tracker correlates are the
/// pixels inside it. A uniform square has no gradient there, and the tracker
/// correctly reports no lock — which is the honest answer to a featureless
/// patch, not something to loosen the floor for. The quadrants are all
/// different so the correlation peak is unique inside the search window.
pub(super) fn square_frame(sx: i32, sy: i32) -> RgbaImage {
    let (w, h) = (160u32, 90u32);
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let inside = x >= sx && x < sx + 12 && y >= sy && y < sy + 12;
            let v = if inside {
                match ((x - sx) / 6, (y - sy) / 6) {
                    (0, 0) => 235,
                    (1, 0) => 60,
                    (0, 1) => 120,
                    _ => 200,
                }
            } else {
                20
            };
            let p = ((y as u32 * w + x as u32) * 4) as usize;
            pixels[p] = v;
            pixels[p + 1] = v;
            pixels[p + 2] = v;
            pixels[p + 3] = 255;
        }
    }
    RgbaImage::new(w, h, pixels)
}

/// Flat grey: no feature to correlate, so the tracker must not claim a lock.
pub(super) fn flat_frame() -> RgbaImage {
    let (w, h) = (160u32, 90u32);
    let mut pixels = vec![90u8; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        pixels[i * 4 + 3] = 255;
    }
    RgbaImage::new(w, h, pixels)
}

/// Frames handed out per tick, like the renderer hands out composites.
pub(super) struct Scripted {
    pub(super) frames: Vec<RgbaImage>,
    /// Every tick asked for, so a test can assert the pass stopped reading.
    pub(super) asked: Vec<i64>,
}

impl Scripted {
    pub(super) fn walking(count: i64, dx: i32, dy: i32) -> Self {
        Self {
            frames: (0..count)
                .map(|i| square_frame(40 + dx * i as i32, 30 + dy * i as i32))
                .collect(),
            asked: Vec::new(),
        }
    }
}

impl MaskFrames for Scripted {
    fn frame(&mut self, at: RationalTime) -> Option<RgbaImage> {
        self.asked.push(at.value);
        let index = usize::try_from(at.value).ok()?;
        self.frames
            .get(index)
            .map(|frame| RgbaImage::new(frame.width, frame.height, frame.pixels.to_vec()))
    }
}

/// A session whose canvas is exactly the size of the scripted frames, with the
/// still selected and a mask over the square those frames start with.
///
/// The canvas resolution matters: the pass reads the layer quad in canvas
/// pixels and correlates in frame pixels, so a fixture whose frames are a
/// different size than its canvas would be measuring two different pictures.
///
/// The mask geometry is derived from the quad the resolver actually produces
/// rather than assumed to be the canvas. The still is square, so it aspect-fits
/// to 90 x 90 inside a 160 x 90 canvas, and a mask offset stated as a fraction
/// of the canvas would sit fifteen pixels off the square — over flat
/// background, where the tracker correctly reports no feature.
pub(super) fn tracked() -> AppState {
    let mut state = selected();
    state
        .apply_command(Command::Edit(EditCommand::SetCanvas {
            aspect: CanvasAspect::Wide16x9,
            background: [0, 0, 0],
            resolution: Some([160, 90]),
        }))
        .expect("a canvas the size of the frames");
    state
        .set_selected_mask_kind(MaskKind::Rectangle)
        .expect("draw a mask");
    // The square the frames start with spans (40, 30)–(52, 42) in frame pixels,
    // so its centre is (46, 36) and its extent 12 x 12. A mask stores both as
    // fractions of the layer it sits on.
    let ([cx, cy], [qw, qh]) = quad(&state);
    state
        .set_selected_mask_geometry(
            [(46.0 - cx) / qw, (36.0 - cy) / qh],
            [12.0 / qw, 12.0 / qh],
            0.0,
        )
        .expect("place the mask");
    state
}

/// The layer quad the pass will read, `(centre, size)` in canvas pixels.
pub(super) fn quad(state: &AppState) -> ([f32; 2], [f32; 2]) {
    state
        .selected_layer_quad()
        .expect("the still places a quad at the playhead")
}

/// The mask centre keyframes the selected clip carries, oldest first.
pub(super) fn keyframes(state: &AppState) -> Vec<(i64, [f32; 2])> {
    let Some(clip) = state.selected_model() else {
        return Vec::new();
    };
    let Some(mask) = clip.mask.as_ref() else {
        return Vec::new();
    };
    match &mask.center {
        Param::Keyframed { keyframes } => {
            keyframes.iter().map(|key| (key.tick, key.value)).collect()
        }
        Param::Constant(_) => Vec::new(),
    }
}

/// The same keyframes in canvas pixels, which is what the scripted frames are
/// drawn in: a case states the motion it painted, not the fraction it becomes.
fn keyframe_pixels(state: &AppState) -> Vec<(i64, [f32; 2])> {
    let (_, [qw, qh]) = quad(state);
    keyframes(state)
        .into_iter()
        .map(|(tick, [x, y])| (tick, [x * qw, y * qh]))
        .collect()
}

#[test]
fn a_moving_feature_is_followed_frame_by_frame() {
    let mut state = tracked();
    // The square walks four pixels right and two down per frame.
    let mut source = Scripted::walking(5, 4, 2);
    let cancel = AtomicBool::new(false);
    let report = state
        .track_selected_mask_with(&mut source, 4, &cancel, CONFIDENCE_FLOOR)
        .expect("the pass runs");

    assert_eq!(
        report.written, 5,
        "one keyframe per frame including the first"
    );
    assert_eq!(report.lost_at, None, "a clean lock reported a loss");
    assert!(!report.cancelled);
    assert!(
        report.weakest > CONFIDENCE_FLOOR,
        "accepted a frame at {}",
        report.weakest
    );

    let keys = keyframe_pixels(&state);
    assert_eq!(keys.len(), 5, "keyframes written: {keys:?}");
    // Four frames of motion at (4, 2) pixels each, measured as a delta from
    // where the mask started.
    let first = keys.first().expect("a first keyframe").1;
    let last = keys.last().expect("a last keyframe").1;
    assert!(
        (last[0] - first[0] - 16.0).abs() < 0.5,
        "tracked x moved {} px not 16",
        last[0] - first[0]
    );
    assert!(
        (last[1] - first[1] - 8.0).abs() < 0.5,
        "tracked y moved {} px not 8",
        last[1] - first[1]
    );
}

#[test]
fn a_lost_feature_stops_the_pass_at_the_tick_that_lost_it() {
    let mut state = tracked();
    let mut source = Scripted {
        frames: vec![
            square_frame(40, 30),
            square_frame(44, 30),
            // Featureless: nothing to correlate, so the pass must stop rather
            // than keep writing the last offset it liked.
            flat_frame(),
            square_frame(52, 30),
        ],
        asked: Vec::new(),
    };
    let cancel = AtomicBool::new(false);
    let report = state
        .track_selected_mask_with(&mut source, 3, &cancel, CONFIDENCE_FLOOR)
        .expect("the pass runs");

    assert_eq!(report.lost_at, Some(2), "the tick that lost the feature");
    assert_eq!(report.written, 2, "keyframes written past the loss");
    assert!(
        !source.asked.contains(&3),
        "kept reading frames after it was lost: {:?}",
        source.asked
    );
    assert_eq!(keyframes(&state).len(), 2);
}

#[test]
fn a_cancelled_pass_keeps_what_it_had_and_stops_reading() {
    let mut state = tracked();
    let mut source = Scripted::walking(6, 3, 0);
    // Cancelled before the first step, which is the case a slow track hits: the
    // pass must end at the starting keyframe rather than walk the range.
    let cancel = AtomicBool::new(true);
    let report = state
        .track_selected_mask_with(&mut source, 5, &cancel, CONFIDENCE_FLOOR)
        .expect("the pass runs");

    assert!(report.cancelled, "a cancelled pass did not say so");
    assert_eq!(report.written, 1, "wrote past the cancel");
    assert_eq!(
        source.asked,
        vec![0],
        "read frames past the cancel: {:?}",
        source.asked
    );
}

#[test]
fn a_whole_track_is_one_undo_entry() {
    let mut state = tracked();
    let before = state.engine().lock().history_depth();
    let mut source = Scripted::walking(4, 4, 0);
    let cancel = AtomicBool::new(false);
    let report = state
        .track_selected_mask_with(&mut source, 3, &cancel, CONFIDENCE_FLOOR)
        .expect("the pass runs");
    assert_eq!(report.written, 4);
    assert_eq!(
        state.engine().lock().history_depth(),
        before + 1,
        "a track left more than one undo entry"
    );

    assert!(state.undo(), "the track undoes");
    assert!(
        keyframes(&state).is_empty(),
        "one undo left keyframes behind: {:?}",
        keyframes(&state)
    );
}

#[test]
fn nothing_to_track_is_a_no_op_not_an_error() {
    // A clip with no mask: there is no patch to follow, so the pass writes
    // nothing and never asks for a frame.
    let mut state = selected();
    let mut source = Scripted::walking(4, 4, 0);
    let cancel = AtomicBool::new(false);
    let report = state
        .track_selected_mask_with(&mut source, 4, &cancel, CONFIDENCE_FLOOR)
        .expect("no mask is not an error");
    assert_eq!(report.written, 0);
    assert!(source.asked.is_empty(), "read a frame with no mask");

    // A mask with a zero-length span is equally a no-op.
    let mut state = tracked();
    let report = state
        .track_selected_mask_with(&mut source, 0, &cancel, CONFIDENCE_FLOOR)
        .expect("a zero span is not an error");
    assert_eq!(report.written, 0);
}
