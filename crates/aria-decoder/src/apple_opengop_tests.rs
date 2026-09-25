//! Open-GOP seek-point recovery cases, driven by the checked-in
//! `tests/fixtures/opengop_h264.mp4` fixture.
//!
//! Declared from `apple_integration_tests.rs` with `#[path]`; `use super::*`
//! reaches that module's fixture helpers and, through them, `apple.rs`'s own
//! imports, so the move changed no name resolution.

use super::*;

/// The regression this fixture exists for: a seek whose effective sync
/// sample has leading B-frames must recover (back off, walk, trim) and
/// land on the exact requested frame — before the recovery path existed,
/// this exact call returned "Cannot Decode".
#[test]
fn open_gop_seek_point_failure_recovers_exactly() {
    let mut dec = AvfDecoder::open(&opengop_fixture(), OutputMode::Cpu).expect("open");
    // 5.0 s: inside the GOP whose sync sample (4.0 s) is undecodable as
    // a start point.
    let target = RationalTime::new(120, OPENGOP_FPS);
    let frame = dec.frame_at(target).expect("must recover").expect("frame");
    assert_eq!(aria_core::resample(frame.pts, OPENGOP_FPS).value, 120);

    // The decoder is healthy afterwards: sequential decode continues.
    let next = dec.next_frame().expect("decode").expect("frame");
    assert_eq!(aria_core::resample(next.pts, OPENGOP_FPS).value, 121);
}

/// Random access across good and bad GOPs, both directions, lands
/// exactly everywhere — the whole timeline is reachable, not just the
/// GOPs whose sync samples happen to be closed.
#[test]
fn open_gop_random_access_reaches_every_gop() {
    let mut dec = AvfDecoder::open(&opengop_fixture(), OutputMode::Cpu).expect("open");
    // 5.5 s and 9 s sit in bad GOPs (syncs 4 s / 8 s); the rest are good
    // or sequential rolls. Mix directions to exercise seek and roll.
    for value in [132i64, 12, 216, 60, 220, 100, 230] {
        let target = RationalTime::new(value, OPENGOP_FPS);
        let frame = dec
            .frame_at(target)
            .unwrap_or_else(|e| panic!("frame_at {value}: {e}"))
            .unwrap_or_else(|| panic!("frame_at {value}: no frame"));
        assert_eq!(
            aria_core::resample(frame.pts, OPENGOP_FPS).value,
            value,
            "target {value}"
        );
    }
}

/// The nearest-sync scrub path takes the same reader seek and must
/// recover the same way.
#[test]
fn open_gop_frame_at_nearest_recovers() {
    let mut dec = AvfDecoder::open(&opengop_fixture(), OutputMode::Cpu).expect("open");
    let target = RationalTime::new(216, OPENGOP_FPS); // 9.0 s, bad GOP
    let frame = dec
        .frame_at_nearest(target)
        .expect("must recover")
        .expect("frame");
    // AVAssetReader trims to the requested range, so nearest on this
    // backend is the frame at/after the target.
    assert_eq!(aria_core::resample(frame.pts, OPENGOP_FPS).value, 216);
}
