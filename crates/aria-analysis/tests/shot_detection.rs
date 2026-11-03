use aria_analysis::{
    MAX_SHOT_CONTEXT_FRAMES, MAX_SHOT_HISTOGRAM_BINS, MAX_SHOT_SAMPLE_HEIGHT,
    MAX_SHOT_SAMPLE_WIDTH, Rgba8Frame, Rgba8FrameError, ShotBoundary, ShotBoundaryKind,
    ShotDetectionConfig, ShotDetectionError, ShotDetector, ShotNeighborhood, ShotScoreWeights,
    detect_shots,
};

/// Width and height of the frames the neighborhood tests build: the default
/// sampling grid, so every pixel is sampled.
const W: usize = 64;
const H: usize = 36;

fn solid_rgba(width: usize, height: usize, rgba: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(width * height * 4);
    for _ in 0..width * height {
        bytes.extend_from_slice(&rgba);
    }
    bytes
}

fn checkerboard_rgba(width: usize, height: usize, inverted: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let bright = ((x + y) % 2 == 0) ^ inverted;
            let value = if bright { 255 } else { 0 };
            bytes.extend_from_slice(&[value, value, value, 255]);
        }
    }
    bytes
}

fn padded_checkerboard(
    width: usize,
    height: usize,
    row_stride: usize,
    inverted: bool,
    padding: u8,
) -> Vec<u8> {
    let mut bytes = vec![padding; row_stride * height];
    for y in 0..height {
        for x in 0..width {
            let bright = ((x + y) % 2 == 0) ^ inverted;
            let value = if bright { 255 } else { 0 };
            let pixel = y * row_stride + x * 4;
            bytes[pixel..pixel + 4].copy_from_slice(&[value, value, value, 255]);
        }
    }
    bytes
}

fn frame<'a>(
    bytes: &'a [u8],
    width: usize,
    height: usize,
    row_stride: usize,
    timestamp_seconds: f64,
) -> Rgba8Frame<'a> {
    Rgba8Frame::new(bytes, width, height, row_stride, timestamp_seconds)
        .expect("synthetic frame is valid")
}

/// A plain threshold detector on an 8x8 grid: the neighborhood test is off, so
/// each test that uses it exercises the score, threshold, and duration rules.
fn test_config(threshold: f32, minimum_duration_seconds: f64) -> ShotDetectionConfig {
    ShotDetectionConfig::new(
        8,
        8,
        16,
        ShotScoreWeights::default(),
        threshold,
        minimum_duration_seconds,
    )
    .expect("test shot configuration is valid")
    .with_neighborhood(ShotNeighborhood::OFF)
}

/// The default threshold on the default grid with the neighborhood test off and
/// no minimum duration: a cut wherever one frame-to-frame score clears the
/// threshold.
fn plain_threshold() -> ShotDetectionConfig {
    let defaults = ShotDetectionConfig::default();
    ShotDetectionConfig::new(
        defaults.sample_width(),
        defaults.sample_height(),
        defaults.luma_histogram_bins(),
        defaults.score_weights(),
        defaults.hard_cut_threshold(),
        0.0,
    )
    .expect("default fields are valid")
    .with_neighborhood(ShotNeighborhood::OFF)
}

/// Vertical stripes 8 pixels wide alternating `dark` and `light`, moved left by
/// `offset` pixels.
fn stripes(offset: usize, dark: [u8; 3], light: [u8; 3]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(W * H * 4);
    for _ in 0..H {
        for x in 0..W {
            let rgb = if (x + offset) % 16 < 8 { dark } else { light };
            bytes.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    bytes
}

/// Frames `index / 30` seconds apart over `images`.
fn at_30_fps(images: &[Vec<u8>]) -> Vec<Rgba8Frame<'_>> {
    images
        .iter()
        .enumerate()
        .map(|(index, bytes)| frame(bytes, W, H, W * 4, index as f64 / 30.0))
        .collect()
}

fn times(boundaries: &[ShotBoundary]) -> Vec<f64> {
    boundaries
        .iter()
        .map(|boundary| boundary.timestamp_seconds())
        .collect()
}

#[test]
fn frame_view_validates_metadata_without_copying() {
    let bytes = solid_rgba(2, 2, [10, 20, 30, 255]);
    let view = frame(&bytes, 2, 2, 8, 1.25);

    assert_eq!(view.bytes().as_ptr(), bytes.as_ptr());
    assert_eq!(view.bytes().len(), bytes.len());
    assert_eq!(view.width(), 2);
    assert_eq!(view.height(), 2);
    assert_eq!(view.row_stride(), 8);
    assert_eq!(view.visible_row_bytes(), 8);
    assert_eq!(view.timestamp_seconds(), 1.25);
}

#[test]
fn malformed_frame_buffers_and_arithmetic_overflow_are_rejected() {
    assert_eq!(
        Rgba8Frame::new(&[], 0, 1, 0, 0.0).err(),
        Some(Rgba8FrameError::ZeroWidth)
    );
    assert_eq!(
        Rgba8Frame::new(&[], 1, 0, 4, 0.0).err(),
        Some(Rgba8FrameError::ZeroHeight)
    );
    assert_eq!(
        Rgba8Frame::new(&[], usize::MAX, 1, usize::MAX, 0.0).err(),
        Some(Rgba8FrameError::RowBytesOverflow { width: usize::MAX })
    );
    assert_eq!(
        Rgba8Frame::new(&[0; 4], 2, 1, 4, 0.0).err(),
        Some(Rgba8FrameError::StrideTooSmall {
            row_stride: 4,
            minimum_row_stride: 8,
        })
    );
    assert_eq!(
        Rgba8Frame::new(&[], 1, usize::MAX, 4, 0.0).err(),
        Some(Rgba8FrameError::RequiredLengthOverflow {
            height: usize::MAX,
            row_stride: 4,
            visible_row_bytes: 4,
        })
    );
    assert_eq!(
        Rgba8Frame::new(&[0; 15], 2, 2, 8, 0.0).err(),
        Some(Rgba8FrameError::BufferTooShort {
            actual_length: 15,
            required_length: 16,
        })
    );
}

#[test]
fn nonfinite_and_negative_frame_timestamps_are_rejected() {
    let bytes = [0_u8; 4];
    assert_eq!(
        Rgba8Frame::new(&bytes, 1, 1, 4, f64::NAN).err(),
        Some(Rgba8FrameError::NonFiniteTimestamp)
    );
    assert_eq!(
        Rgba8Frame::new(&bytes, 1, 1, 4, f64::INFINITY).err(),
        Some(Rgba8FrameError::NonFiniteTimestamp)
    );
    assert_eq!(
        Rgba8Frame::new(&bytes, 1, 1, 4, -0.001).err(),
        Some(Rgba8FrameError::NegativeTimestamp)
    );
}

#[test]
fn shot_configuration_reports_precise_invalid_fields() {
    let error = ShotScoreWeights::new(f32::NAN, 1.0).expect_err("NaN weight must fail");
    assert_eq!(error.field(), "histogram_weight");
    assert_eq!(error.reason(), "must be finite and non-negative");

    let error = ShotScoreWeights::new(0.0, 0.0).expect_err("zero weights must fail");
    assert_eq!(error.field(), "score_weights");

    let weights = ShotScoreWeights::default();
    let invalid_cases = [
        ShotDetectionConfig::new(0, 1, 2, weights, 0.5, 0.0),
        ShotDetectionConfig::new(MAX_SHOT_SAMPLE_WIDTH + 1, 1, 2, weights, 0.5, 0.0),
        ShotDetectionConfig::new(1, 0, 2, weights, 0.5, 0.0),
        ShotDetectionConfig::new(1, MAX_SHOT_SAMPLE_HEIGHT + 1, 2, weights, 0.5, 0.0),
        ShotDetectionConfig::new(1, 1, 1, weights, 0.5, 0.0),
        ShotDetectionConfig::new(1, 1, MAX_SHOT_HISTOGRAM_BINS + 1, weights, 0.5, 0.0),
        ShotDetectionConfig::new(1, 1, 2, weights, 0.0, 0.0),
        ShotDetectionConfig::new(1, 1, 2, weights, f32::NAN, 0.0),
        ShotDetectionConfig::new(1, 1, 2, weights, 0.5, -0.1),
        ShotDetectionConfig::new(1, 1, 2, weights, 0.5, f64::INFINITY),
    ];
    assert!(invalid_cases.into_iter().all(|result| result.is_err()));

    let neighborhood_cases = [
        (
            ShotNeighborhood::new(MAX_SHOT_CONTEXT_FRAMES + 1, 1.0, 0.0, 0, 0.0),
            "radius_frames",
        ),
        (
            ShotNeighborhood::new(0, -0.1, 0.0, 0, 0.0),
            "contrast_ratio",
        ),
        (
            ShotNeighborhood::new(0, f32::INFINITY, 0.0, 0, 0.0),
            "contrast_ratio",
        ),
        (
            ShotNeighborhood::new(0, 1.0, 1.5, 0, 0.0),
            "contrast_margin",
        ),
        (
            ShotNeighborhood::new(0, 1.0, f32::NAN, 0, 0.0),
            "contrast_margin",
        ),
        (
            ShotNeighborhood::new(0, 1.0, 0.0, MAX_SHOT_CONTEXT_FRAMES + 1, 0.0),
            "persistence_frames",
        ),
        (
            ShotNeighborhood::new(0, 1.0, 0.0, 0, 1.5),
            "persistence_ratio",
        ),
    ];
    for (result, field) in neighborhood_cases {
        assert_eq!(result.expect_err("invalid neighborhood").field(), field);
    }
    assert!(
        ShotNeighborhood::new(
            MAX_SHOT_CONTEXT_FRAMES,
            4.0,
            1.0,
            MAX_SHOT_CONTEXT_FRAMES,
            1.0
        )
        .is_ok()
    );
}

#[test]
fn identical_frames_and_sequence_end_do_not_create_boundaries() {
    let bytes = checkerboard_rgba(8, 8, false);
    let frames = [
        frame(&bytes, 8, 8, 32, 0.0),
        frame(&bytes, 8, 8, 32, 1.0),
        frame(&bytes, 8, 8, 32, 2.0),
    ];

    assert!(
        detect_shots(&frames[..1], ShotDetectionConfig::default())
            .expect("ordered frames")
            .is_empty()
    );
    assert!(
        detect_shots(&frames, ShotDetectionConfig::default())
            .expect("ordered frames")
            .is_empty()
    );
}

#[test]
fn stark_content_change_emits_boundary_at_current_frame() {
    let first = checkerboard_rgba(8, 8, false);
    let second = checkerboard_rgba(8, 8, true);
    let frames = [frame(&first, 8, 8, 32, 0.0), frame(&second, 8, 8, 32, 1.0)];

    let boundaries = detect_shots(&frames, ShotDetectionConfig::default()).expect("ordered frames");
    assert_eq!(boundaries.len(), 1);
    let boundary = boundaries[0];
    assert_eq!(boundary.kind(), ShotBoundaryKind::VisualChange);
    assert_eq!(boundary.timestamp_seconds(), 1.0);
    assert_eq!(boundary.previous_frame_timestamp_seconds(), 0.0);
    assert_eq!(boundary.current_frame_timestamp_seconds(), 1.0);
    assert!(boundary.score().is_finite());
    assert!((0.0..=1.0).contains(&boundary.score()));
    assert!(boundary.score() >= ShotDetectionConfig::default().hard_cut_threshold());
}

#[test]
fn gradual_global_brightness_fade_stays_below_hard_cut_threshold() {
    let images: Vec<Vec<u8>> = (0..=8)
        .map(|step| {
            let value = (step * 31) as u8;
            solid_rgba(8, 8, [value, value, value, 255])
        })
        .collect();
    let frames: Vec<_> = images
        .iter()
        .enumerate()
        .map(|(index, bytes)| frame(bytes, 8, 8, 32, index as f64 * 0.25))
        .collect();

    assert!(
        detect_shots(&frames, test_config(0.38, 0.0))
            .expect("ordered frames")
            .is_empty()
    );
}

/// The minimum duration counts from the last cut, or from the first frame before
/// any cut, and a candidate it suppresses is dropped rather than moved later.
#[test]
fn minimum_duration_counts_from_the_last_cut_without_delaying_a_candidate() {
    let first = checkerboard_rgba(8, 8, false);
    let second = checkerboard_rgba(8, 8, true);
    let frames = [
        frame(&first, 8, 8, 32, 0.0),
        frame(&second, 8, 8, 32, 0.1),
        frame(&first, 8, 8, 32, 0.6),
        frame(&second, 8, 8, 32, 0.9),
        frame(&first, 8, 8, 32, 1.2),
    ];

    let boundaries = detect_shots(&frames, test_config(0.30, 0.5)).expect("ordered frames");
    assert_eq!(times(&boundaries), [0.6, 1.2]);
    assert_eq!(boundaries[0].previous_frame_timestamp_seconds(), 0.1);
    assert_eq!(boundaries[1].previous_frame_timestamp_seconds(), 0.9);
}

#[test]
fn padded_row_stride_is_honored_and_padding_is_ignored() {
    let first = padded_checkerboard(2, 2, 12, false, 0);
    let same_visible_pixels = padded_checkerboard(2, 2, 12, false, 255);
    let changed = padded_checkerboard(2, 2, 12, true, 17);
    let frames = [
        frame(&first, 2, 2, 12, 0.0),
        frame(&same_visible_pixels, 2, 2, 12, 1.0),
        frame(&changed, 2, 2, 12, 2.0),
    ];

    let boundaries = detect_shots(&frames, test_config(0.30, 0.0)).expect("ordered frames");
    assert_eq!(boundaries.len(), 1);
    assert_eq!(boundaries[0].timestamp_seconds(), 2.0);
}

/// A change of frame size is a cut with score 1.0 wherever it falls, even
/// inside a picture that changes every frame, which hides any visual cut.
#[test]
fn a_dimension_change_is_a_cut_inside_any_motion() {
    let images: Vec<(Vec<u8>, usize)> = (0..24)
        .map(|index| {
            let width = if index < 12 { 8 } else { 10 };
            (checkerboard_rgba(width, 8, index % 2 == 1), width)
        })
        .collect();
    let frames: Vec<Rgba8Frame<'_>> = images
        .iter()
        .enumerate()
        .map(|(index, (bytes, width))| frame(bytes, *width, 8, width * 4, index as f64 / 30.0))
        .collect();
    let plain = detect_shots(&frames, plain_threshold()).expect("ordered frames");
    assert_eq!(plain.len(), 23, "premise: every frame clears the threshold");

    let boundaries = detect_shots(&frames, ShotDetectionConfig::default()).expect("ordered frames");
    assert_eq!(times(&boundaries), [frames[12].timestamp_seconds()]);
    assert_eq!(boundaries[0].kind(), ShotBoundaryKind::DimensionChange);
    assert_eq!(boundaries[0].score(), 1.0);
}

#[test]
fn duplicate_timestamps_are_accepted_and_remain_sorted() {
    let first = checkerboard_rgba(8, 8, false);
    let second = checkerboard_rgba(8, 8, true);
    let frames = [frame(&first, 8, 8, 32, 0.0), frame(&second, 8, 8, 32, 0.0)];

    let boundaries =
        detect_shots(&frames, test_config(0.30, 0.0)).expect("duplicate timestamps are valid");
    assert_eq!(boundaries.len(), 1);
    assert_eq!(boundaries[0].previous_frame_timestamp_seconds(), 0.0);
    assert_eq!(boundaries[0].current_frame_timestamp_seconds(), 0.0);
}

#[test]
fn out_of_order_timestamp_fails_closed_without_mutating_streaming_state() {
    let bytes = checkerboard_rgba(4, 4, false);
    let first = frame(&bytes, 4, 4, 16, 1.0);
    let out_of_order = frame(&bytes, 4, 4, 16, 0.5);
    let next = frame(&bytes, 4, 4, 16, 2.0);
    let mut detector = ShotDetector::new(test_config(0.30, 0.0));

    assert_eq!(detector.push_frame(first), Ok(None));
    assert_eq!(
        detector.push_frame(out_of_order),
        Err(ShotDetectionError::OutOfOrderTimestamp {
            previous_timestamp_seconds: 1.0,
            current_timestamp_seconds: 0.5,
        })
    );
    assert_eq!(detector.push_frame(next), Ok(None));

    assert_eq!(
        detect_shots([first, out_of_order], test_config(0.30, 0.0)),
        Err(ShotDetectionError::OutOfOrderTimestamp {
            previous_timestamp_seconds: 1.0,
            current_timestamp_seconds: 0.5,
        })
    );
}

/// Three solid scenes cut at frames 10 and 28 of 30: the first cut resolves
/// during `push_frame`, the second only at `finish`.
fn two_cut_scenes() -> Vec<Vec<u8>> {
    (0..30)
        .map(|index| {
            let rgb = match index {
                0..10 => [200, 40, 40],
                10..28 => [40, 200, 40],
                _ => [40, 40, 200],
            };
            solid_rgba(W, H, [rgb[0], rgb[1], rgb[2], 255])
        })
        .collect()
}

/// A candidate resolves only after its lookahead arrives, so a streaming caller
/// receives some boundaries from `push_frame` and the rest from `finish`, and the
/// two together equal the one-shot result.
#[test]
fn streaming_with_finish_equals_one_shot_detection() {
    let images = two_cut_scenes();
    let frames = at_30_fps(&images);
    let config = ShotDetectionConfig::default();
    let one_shot = detect_shots(&frames, config).expect("ordered frames");
    assert_eq!(
        times(&one_shot),
        [
            frames[10].timestamp_seconds(),
            frames[28].timestamp_seconds()
        ]
    );

    let mut detector = ShotDetector::new(config);
    let pushed: Vec<_> = frames
        .iter()
        .filter_map(|&frame| detector.push_frame(frame).expect("ordered frames"))
        .collect();
    let finished: Vec<_> = detector.finish().collect();
    assert_eq!(times(&pushed), [frames[10].timestamp_seconds()]);
    assert_eq!(times(&finished), [frames[28].timestamp_seconds()]);
    assert_eq!([pushed, finished].concat(), one_shot);
    assert_eq!(
        detector.finish().count(),
        0,
        "a second finish resolves nothing"
    );
}

/// A reset detector reuses its frame buffers; no frame, score, or shot start
/// from the previous sequence may reach the next one. The previous sequence
/// ends in a cut later than the next sequence's first cut, so a kept shot start
/// would suppress that cut.
#[test]
fn a_reset_detector_matches_a_fresh_one() {
    let previous_images: Vec<Vec<u8>> = (0..50)
        .map(|index| {
            let rgb = if index < 35 {
                [40, 200, 40]
            } else {
                [40, 40, 200]
            };
            solid_rgba(W, H, [rgb[0], rgb[1], rgb[2], 255])
        })
        .collect();
    let images = two_cut_scenes();
    let frames = at_30_fps(&images);
    let config = ShotDetectionConfig::default();
    let fresh = detect_shots(&frames, config).expect("ordered frames");

    let mut detector = ShotDetector::new(config);
    let previous_frames = at_30_fps(&previous_images);
    let previous: Vec<_> = previous_frames
        .iter()
        .filter_map(|&frame| detector.push_frame(frame).expect("ordered frames"))
        .collect();
    assert_eq!(
        times(&previous),
        [previous_frames[35].timestamp_seconds()],
        "premise: the previous sequence moved the shot start past the next cut"
    );
    detector.reset();
    let mut reused: Vec<_> = frames
        .iter()
        .filter_map(|&frame| detector.push_frame(frame).expect("ordered frames"))
        .collect();
    reused.extend(detector.finish());
    assert_eq!(reused, fresh);
}

/// Retained feature storage is set by the configuration, reaches its limit, and
/// never passes it, however long the sequence or large the source.
#[test]
fn feature_storage_remains_bounded_by_configuration_not_source_size() {
    let config = ShotDetectionConfig::new(3, 2, 8, ShotScoreWeights::default(), 0.30, 0.0)
        .expect("valid bounded config");
    let bytes = checkerboard_rgba(40, 30, false);
    let mut detector = ShotDetector::new(config);
    assert_eq!(config.maximum_sampled_pixels(), 6);

    for pass in 0..2 {
        let mut most = 0;
        for index in 0..4 * MAX_SHOT_CONTEXT_FRAMES {
            detector
                .push_frame(frame(&bytes, 40, 30, 160, index as f64))
                .expect("ordered frames");
            let retained = detector.retained_sampled_pixels();
            assert!(
                retained <= detector.sampled_pixel_storage_limit(),
                "pass {pass}, frame {index}: {retained} retained"
            );
            most = most.max(retained);
        }
        assert_eq!(most, detector.sampled_pixel_storage_limit(), "pass {pass}");
        detector.reset();
        assert_eq!(detector.retained_sampled_pixels(), 0);
    }
}

/// The class closed: cuts six frames apart, a cut between two dark scenes that
/// differ only in a small bright region, and a cut into a scene whose exposure
/// ramps are each found on their exact frame, while an exposure fade within a
/// textured shot is not a cut.
#[test]
fn default_config_detects_fast_dark_and_localized_cuts_while_ignoring_textured_fades() {
    let width = 64;
    let height = 36;
    let stride = width * 4;
    let mut buffers = Vec::with_capacity(90);

    for idx in 0..90_usize {
        let mut pixels = vec![0_u8; height * stride];
        for y in 0..height {
            for x in 0..width {
                let off = y * stride + x * 4;
                let rgb = if idx < 18 {
                    let fade = 0.60 + 0.40 * (idx as f32 / 17.0);
                    let tile = (x / 8 + y / 6) % 2 == 0;
                    let r = if tile { 215.0 } else { 85.0 };
                    let g = if tile { 145.0 } else { 55.0 };
                    [(r * fade) as u8, (g * fade) as u8, (40.0 * fade) as u8]
                } else if idx < 24 {
                    if (x + y) % 7 < 3 {
                        [30, 195, 235]
                    } else {
                        [15, 55, 95]
                    }
                } else if idx < 30 {
                    if (10..26).contains(&x) && (8..24).contains(&y) {
                        [190, 120, 60]
                    } else {
                        [8, 10, 16]
                    }
                } else if idx < 48 {
                    if (38..54).contains(&x) && (12..28).contains(&y) {
                        [65, 155, 225]
                    } else {
                        [10, 12, 18]
                    }
                } else {
                    let ramp = ((idx - 48) as f32 * 1.5).min(45.0) as u8;
                    if x < width / 2 {
                        [160_u8.saturating_add(ramp), 80, 110]
                    } else {
                        [40, 150_u8.saturating_add(ramp), 95]
                    }
                };
                pixels[off] = rgb[0];
                pixels[off + 1] = rgb[1];
                pixels[off + 2] = rgb[2];
                pixels[off + 3] = 255;
            }
        }
        buffers.push(pixels);
    }

    let frames: Vec<Rgba8Frame<'_>> = buffers
        .iter()
        .enumerate()
        .map(|(idx, bytes)| frame(bytes, width, height, stride, idx as f64 / 30.0))
        .collect();

    let boundaries =
        detect_shots(&frames, ShotDetectionConfig::default()).expect("default shot detection");
    let expected: Vec<f64> = [18_usize, 24, 30, 48]
        .iter()
        .map(|&index| frames[index].timestamp_seconds())
        .collect();
    assert_eq!(times(&boundaries), expected, "boundaries: {boundaries:?}");
}

/// The class closed: a picture change that does not last, from a one-frame flash
/// to an insert as long as the persistence span, is a cut neither where it
/// starts nor where the scene returns. One frame longer, both edges are cuts.
/// Not caught: an insert that differs from the scene by less than the threshold.
#[test]
fn a_change_shorter_than_the_persistence_span_is_not_a_cut() {
    let scene = stripes(0, [100; 3], [160; 3]);
    let insert = solid_rgba(W, H, [255, 0, 255, 255]);
    let span = ShotNeighborhood::default().persistence_frames();
    for length in 1..=span + 1 {
        let images: Vec<Vec<u8>> = (0..40)
            .map(|index| {
                if (15..15 + length).contains(&index) {
                    insert.clone()
                } else {
                    scene.clone()
                }
            })
            .collect();
        let frames = at_30_fps(&images);
        let edges = vec![
            frames[15].timestamp_seconds(),
            frames[15 + length].timestamp_seconds(),
        ];
        let plain = detect_shots(&frames, plain_threshold()).expect("ordered frames");
        assert_eq!(
            times(&plain),
            edges,
            "premise: both edges clear the threshold"
        );

        let boundaries =
            detect_shots(&frames, ShotDetectionConfig::default()).expect("ordered frames");
        let expected = if length <= span { Vec::new() } else { edges };
        assert_eq!(times(&boundaries), expected, "a {length}-frame insert");
    }
}

/// The class closed: motion that changes every frame by more than the threshold
/// is not a run of cuts, nor a cut on the frame it starts from a still picture
/// or the frame it stops on, from motion that barely clears the threshold to
/// motion that changes nearly every pixel. Not caught: a camera jolt that moves
/// the picture farther in one frame than the motion around it does.
#[test]
fn motion_is_not_a_cut_where_it_starts_runs_or_stops() {
    let palettes = [
        ([100; 3], [160; 3]),
        ([90, 60, 30], [200, 170, 120]),
        ([30, 60, 140], [180, 200, 230]),
    ];
    for (dark, light) in palettes {
        // Still through frame 9, moving 4 pixels a frame through frame 29,
        // still after.
        let images: Vec<Vec<u8>> = (0..40_usize)
            .map(|index| stripes(4 * (index.clamp(9, 29) - 9), dark, light))
            .collect();
        let frames = at_30_fps(&images);
        let plain = detect_shots(&frames, plain_threshold()).expect("ordered frames");
        let moving: Vec<f64> = frames[10..30]
            .iter()
            .map(|frame| frame.timestamp_seconds())
            .collect();
        assert_eq!(
            times(&plain),
            moving,
            "premise: every moving frame of {dark:?}/{light:?} clears the threshold"
        );

        let boundaries =
            detect_shots(&frames, ShotDetectionConfig::default()).expect("ordered frames");
        assert_eq!(
            times(&boundaries),
            Vec::<f64>::new(),
            "{dark:?}/{light:?} stripes"
        );
    }
}

/// A cut between two moving pictures is a cut when it stands out from the
/// motion on each side. Not caught: a cut that scores within the contrast of
/// the motion on either side of it.
#[test]
fn a_cut_inside_motion_is_a_cut() {
    let images: Vec<Vec<u8>> = (0..60_usize)
        .map(|index| {
            if index < 30 {
                stripes(4 * index, [100; 3], [160; 3])
            } else {
                stripes(4 * index, [40, 160, 200], [60, 160, 200])
            }
        })
        .collect();
    let frames = at_30_fps(&images);
    let plain = detect_shots(&frames[..30], plain_threshold()).expect("ordered frames");
    assert_eq!(
        plain.len(),
        29,
        "premise: every frame of the motion before the cut clears the threshold"
    );

    let boundaries = detect_shots(&frames, ShotDetectionConfig::default()).expect("ordered frames");
    assert_eq!(times(&boundaries), [frames[30].timestamp_seconds()]);
}
