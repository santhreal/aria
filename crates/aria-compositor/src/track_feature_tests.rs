//! The feature tracker on synthetic frames.
//!
//! The class these close: a tracker that reports a position for every frame,
//! whatever the picture, writes confident keyframes on a lost feature. So the
//! cases here are the ones a lost track looks like — a flat region, a frame that
//! changed size, a patch off the edge, a motion past the search window — and each
//! is asserted to refuse or to score low rather than to answer.

use super::*;
use aria_core::RgbaImage;

/// A frame with a bright square at `(sx, sy)` on a dark field.
fn frame(width: u32, height: u32, sx: i32, sy: i32, square: i32) -> RgbaImage {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let inside = x >= sx && x < sx + square && y >= sy && y < sy + square;
            let v = if inside { 240 } else { 16 };
            let p = ((y as u32 * width + x as u32) * 4) as usize;
            pixels[p] = v;
            pixels[p + 1] = v;
            pixels[p + 2] = v;
            pixels[p + 3] = 255;
        }
    }
    RgbaImage::new(width, height, pixels)
}

/// A frame of one flat value: nothing to lock onto anywhere.
fn flat(width: u32, height: u32, value: u8) -> RgbaImage {
    let mut pixels = vec![value; (width * height * 4) as usize];
    for i in 0..(width * height) as usize {
        pixels[i * 4 + 3] = 255;
    }
    RgbaImage::new(width, height, pixels)
}

#[test]
fn a_moved_feature_is_found_at_its_new_offset() {
    let prev = frame(64, 64, 20, 20, 8);
    let next = frame(64, 64, 25, 23, 8);
    let found = track_patch(&prev, &next, Patch::centered(24.0, 24.0, 16.0, 16.0), 8)
        .expect("a bounded search over a moved square");
    assert_eq!(
        (found.dx, found.dy),
        (5, 3),
        "the offset the square moved by"
    );
    assert!(found.score > 0.9, "an exact patch scored {}", found.score);
}

#[test]
fn a_still_feature_reports_no_motion() {
    let prev = frame(64, 64, 20, 20, 8);
    let found = track_patch(&prev, &prev, Patch::centered(24.0, 24.0, 16.0, 16.0), 6)
        .expect("a search over an unchanged frame");
    assert_eq!(
        (found.dx, found.dy),
        (0, 0),
        "an unchanged frame was reported as having moved"
    );
}

#[test]
fn a_feature_that_left_the_search_window_scores_low() {
    let prev = frame(64, 64, 20, 20, 8);
    // Twelve pixels of motion against a four-pixel window: the true position is
    // unreachable, so every candidate is wrong and must say so.
    let next = frame(64, 64, 32, 20, 8);
    let found = track_patch(&prev, &next, Patch::centered(24.0, 24.0, 16.0, 16.0), 4)
        .expect("the search still runs");
    assert!(
        found.score < 0.5,
        "a lost feature reported confidence {}",
        found.score
    );
}

#[test]
fn a_flat_region_never_reports_a_lock() {
    let prev = flat(32, 32, 80);
    let next = flat(32, 32, 80);
    let found = track_patch(&prev, &next, Patch::centered(16.0, 16.0, 8.0, 8.0), 4)
        .expect("the search runs on flat picture too");
    assert_eq!(
        found.score, 0.0,
        "featureless picture scored like a match: {found:?}"
    );
}

#[test]
fn a_patch_off_the_edge_or_a_resized_frame_is_refused() {
    let prev = frame(32, 32, 8, 8, 6);
    let next = frame(32, 32, 9, 8, 6);
    assert!(
        track_patch(&prev, &next, Patch::centered(2.0, 2.0, 12.0, 12.0), 2).is_none(),
        "a patch hanging off the left edge was searched anyway"
    );
    assert!(
        track_patch(&prev, &next, Patch::centered(30.0, 30.0, 12.0, 12.0), 2).is_none(),
        "a patch hanging off the bottom edge was searched anyway"
    );
    let bigger = frame(48, 48, 9, 8, 6);
    assert!(
        track_patch(&prev, &bigger, Patch::centered(16.0, 16.0, 8.0, 8.0), 2).is_none(),
        "two frames of different sizes were correlated"
    );
    assert!(
        track_patch(&prev, &next, Patch::centered(16.0, 16.0, 8.0, 8.0), -1).is_none(),
        "a negative radius was searched"
    );
}

#[test]
fn a_zero_radius_search_only_reports_the_patch_where_it_was() {
    let prev = frame(32, 32, 8, 8, 6);
    let next = frame(32, 32, 11, 8, 6);
    let found = track_patch(&prev, &next, Patch::centered(11.0, 11.0, 10.0, 10.0), 0)
        .expect("a zero radius is a legal, if useless, search");
    assert_eq!((found.dx, found.dy), (0, 0));
}
