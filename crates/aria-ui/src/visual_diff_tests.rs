//! The scope pictures, against their committed references.
//!
//! Every case is named `snapshot_diff_*` so `cargo test -p aria-ui --lib
//! snapshot_diff` selects the whole harness and nothing else.
//!
//! The frame every scope is measured from is built here rather than decoded: a
//! synthetic frame is exact, so a difference is the painter's and never the
//! decoder's. It carries what the three scopes each need something to say
//! about — a full-range luma ramp across the width for the waveform, six
//! saturated primaries for the vectorscope, and a channel-separated block so
//! the histogram's three traces do not sit on top of each other.

use aria_analysis::{Rgba8Frame, ScopeConfig, WaveformTrace, measure_scopes};
use aria_core::RgbaImage;

use super::{BLESS, Difference, check, compare, diff_map, reference_names};
use crate::scopes::ScopeImages;

/// Width and height of the frame the scopes are measured from. Small enough to
/// stay under the sampling budget, so every pixel is read and the measurement
/// does not depend on a stride.
const FRAME_W: u32 = 128;
const FRAME_H: u32 = 72;

/// A frame with something for each scope to say.
fn frame() -> RgbaImage {
    let mut pixels = Vec::with_capacity((FRAME_W * FRAME_H * 4) as usize);
    // Saturated primaries and secondaries, in bar order.
    const BARS: [[u8; 3]; 6] = [
        [255, 0, 0],
        [255, 255, 0],
        [0, 255, 0],
        [0, 255, 255],
        [0, 0, 255],
        [255, 0, 255],
    ];
    for y in 0..FRAME_H {
        for x in 0..FRAME_W {
            let third = y * 3 / FRAME_H;
            let rgb = match third {
                // A luma ramp: the waveform's trace should climb left to right.
                0 => {
                    let v = (x * 255 / (FRAME_W - 1)) as u8;
                    [v, v, v]
                }
                // Colour bars: six points off the vectorscope's centre.
                1 => BARS[(x as usize * BARS.len() / FRAME_W as usize).min(BARS.len() - 1)],
                // One channel at a time, so the histogram draws three traces at
                // three different code values.
                _ => {
                    let v = (x * 255 / (FRAME_W - 1)) as u8;
                    match x % 3 {
                        0 => [v, 0, 0],
                        1 => [0, v / 2, 0],
                        _ => [0, 0, v / 4],
                    }
                }
            };
            pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    RgbaImage::new(FRAME_W, FRAME_H, pixels)
}

/// The three pictures the scope strip draws for that frame, with the waveform
/// counting luma.
fn scopes() -> ScopeImages {
    traced(WaveformTrace::Luma)
}

/// The three pictures the scope strip draws for that frame, with the waveform
/// counting `trace`.
fn traced(trace: WaveformTrace) -> ScopeImages {
    let source = frame();
    let view = Rgba8Frame::new(
        &source.pixels,
        source.width as usize,
        source.height as usize,
        source.width as usize * 4,
        0.0,
    )
    .expect("a well-formed frame");
    ScopeImages::paint(&measure_scopes(
        view,
        ScopeConfig::default().with_waveform_trace(trace),
    ))
}

/// Every reference the suite claims, in the order the strip shows them.
const CLAIMED: [&str; 5] = [
    "scope-waveform",
    "scope-waveform-rgb",
    "scope-waveform-ycbcr",
    "scope-vectorscope",
    "scope-histogram",
];

#[test]
fn snapshot_diff_waveform() {
    check("scope-waveform", &scopes().waveform).expect("the waveform matches its reference");
}

#[test]
fn snapshot_diff_waveform_rgb_parade() {
    check(
        "scope-waveform-rgb",
        &traced(WaveformTrace::RgbParade).waveform,
    )
    .expect("the RGB parade matches its reference");
}

#[test]
fn snapshot_diff_waveform_ycbcr_parade() {
    check(
        "scope-waveform-ycbcr",
        &traced(WaveformTrace::YcbcrParade).waveform,
    )
    .expect("the YCbCr parade matches its reference");
}

#[test]
fn snapshot_diff_vectorscope() {
    check("scope-vectorscope", &scopes().vectorscope)
        .expect("the vectorscope matches its reference");
}

#[test]
fn snapshot_diff_histogram() {
    check("scope-histogram", &scopes().histogram).expect("the histogram matches its reference");
}

/// **Every reference on disk is claimed by a case, and every case has one.**
///
/// Fails by default in both directions: a picture added to the directory with no
/// case reading it is a reference nothing asserts, and a case naming a reference
/// that is not there is caught by `check` itself. Without this, the harness could
/// quietly stop covering a scope and still pass.
///
/// A blessing run rewrites the directory instead of asserting on it, and the
/// cases that write run beside this one, so the set is only a set once the run
/// is over. The gate never blesses.
#[test]
fn snapshot_diff_covers_every_committed_reference() {
    if std::env::var_os(BLESS).is_some() {
        return;
    }
    let mut claimed: Vec<String> = CLAIMED.iter().map(|name| name.to_string()).collect();
    claimed.sort();
    assert_eq!(
        reference_names(),
        claimed,
        "the references on disk and the cases in this file have to be the same set: a \
         picture nobody compares is not a reference, and blessing one by accident is how \
         that happens"
    );
}

/// **A difference is reported as the region that changed, not as a count.**
///
/// The whole point of the harness: "the picture changed" is what the reader
/// already knew. Driven by poking a known rectangle into a real scope picture
/// and asserting the reported box is exactly that rectangle, so a diff that
/// reports the whole canvas — or the first differing pixel alone — fails here.
#[test]
fn snapshot_diff_names_the_region_that_changed() {
    let reference = scopes().waveform;
    let mut actual = RgbaImage::new(reference.width, reference.height, reference.pixels.to_vec());
    // A rectangle away from every edge, so a report that clamps to the canvas
    // is not accidentally right.
    let (x, y, w, h) = (11_u32, 7_u32, 5_u32, 3_u32);
    for row in y..y + h {
        for column in x..x + w {
            let at = ((row * actual.width + column) * 4) as usize;
            actual.pixels[at] = actual.pixels[at].wrapping_add(64);
        }
    }

    let difference = compare(&actual, &reference)
        .expect("same size")
        .expect("a poked rectangle is a difference");
    assert_eq!(
        difference,
        Difference {
            pixels: w * h,
            region: (x, y, w, h),
            worst: 64,
        }
    );

    // The map puts the mark where the region says it is, and nowhere else: a map
    // that marked every cell would still "report a region" and say nothing. The
    // poked rectangle is 5x3 of a 256x192 picture, so it falls inside one cell
    // of a 32x16 grid.
    let map = diff_map(&actual, &reference);
    assert_eq!(map.matches('#').count(), 1, "one cell holds it:\n{map}");
    let marked = map
        .lines()
        .enumerate()
        .find_map(|(row, line)| line.find('#').map(|column| (row, column - 2)))
        .expect("a marked cell");
    assert_eq!(
        marked,
        (
            (y * 16 / reference.height) as usize,
            (x * 32 / reference.width) as usize
        ),
        "the mark is not over the rectangle:\n{map}"
    );
}

/// A picture the same in every pixel is not a difference, and one that changed
/// shape is not a region.
#[test]
fn snapshot_diff_refuses_a_reshaped_picture() {
    let reference = scopes().histogram;
    let same = RgbaImage::new(reference.width, reference.height, reference.pixels.to_vec());
    assert_eq!(compare(&same, &reference), Ok(None));

    let taller = RgbaImage::new(
        reference.width,
        reference.height + 1,
        vec![0; reference.pixels.len() + (reference.width * 4) as usize],
    );
    let refusal = compare(&taller, &reference).expect_err("a reshaped picture has no region");
    assert!(
        refusal.contains(&reference.height.to_string()),
        "the refusal has to name both shapes, got {refusal}"
    );
}

/// Blessing is opt-in, and this is the name the failure message tells an
/// operator to set. A rename that left the message behind would send them to a
/// variable that does nothing.
///
/// Not run while blessing: under that variable `check` writes the picture
/// instead of refusing it, which would leave a reference no case claims — the
/// first draft of this file did exactly that.
#[test]
fn snapshot_diff_names_the_variable_that_records_a_picture() {
    if std::env::var_os(BLESS).is_some() {
        return;
    }
    let refusal = check("scope-no-such-picture", &scopes().waveform)
        .expect_err("there is no reference by that name");
    assert!(
        refusal.contains(BLESS),
        "a missing reference has to say how to record one, got {refusal}"
    );
    assert!(
        !reference_names()
            .iter()
            .any(|name| name == "scope-no-such-picture"),
        "asking about a missing reference must not create one"
    );
}
