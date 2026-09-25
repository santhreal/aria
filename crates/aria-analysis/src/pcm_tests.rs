//! Tests for the rolling energy pass and the window spans it hands out.
//!
//! Both cases are about drift. The rolling sum must equal the naive sum over
//! the same frames, and a window's span must come from its frame indices rather
//! than from seconds accumulated hop by hop.

use super::*;

#[test]
fn rolling_energy_matches_naive_overlapping_windows() {
    let samples = [1.0, -1.0, 0.5, -0.5, 0.25, -0.25, 0.0, 0.0, -0.75, 0.75];
    let pcm = InterleavedPcm::new(&samples, 10, 2).expect("valid stereo");
    let windows = energy_windows(pcm, 0.3, 0.1);

    assert_eq!(windows.len(), 5);
    for window in windows {
        let naive_sum = squared_sample_sum(pcm, window.start_frame, window.end_frame);
        let naive_count = (window.end_frame - window.start_frame) * pcm.channels;
        let naive_mean = naive_sum / naive_count as f64;
        assert!((window.mean_square - naive_mean).abs() < 1.0e-12);
    }
}

#[test]
fn window_spans_come_from_frame_indices_without_accumulated_hops() {
    let samples = vec![0.25; 11];
    let contour = loudness_contour(
        &samples,
        10,
        1,
        LoudnessConfig::new(0.3, 0.2, -100.0).expect("valid config"),
    )
    .expect("valid PCM");

    assert_eq!(contour.len(), 6);
    assert_eq!(contour[3].span.start_seconds(), 0.6);
    assert_eq!(contour[3].span.end_seconds(), 0.9);
    assert_eq!(contour[5].span.start_seconds(), 1.0);
    assert_eq!(contour[5].span.end_seconds(), 1.1);
}

#[test]
fn pcm_boundary_and_rejection_tests() {
    // Empty buffer is valid when rate & channels > 0
    let empty: [f32; 0] = [];
    let pcm = InterleavedPcm::new(&empty, 44100, 2).expect("empty buffer is valid");
    assert!(pcm.is_empty());
    assert_eq!(pcm.frame_count(), 0);

    // Single sample with mono
    let single = [0.5f32];
    let pcm = InterleavedPcm::new(&single, 48000, 1).expect("single sample mono is valid");
    assert_eq!(pcm.frame_count(), 1);

    // Single sample with stereo (non-frame-aligned) -> typed error
    assert!(matches!(
        InterleavedPcm::new(&single, 48000, 2),
        Err(AudioInputError::NonFrameAligned { .. })
    ));

    // Odd sample rates (11025, 22050, 12345, 1)
    for rate in [1, 11025, 12345, 22050, 96000] {
        let pcm = InterleavedPcm::new(&[0.1, 0.2], rate, 2).expect("odd rates valid");
        assert_eq!(pcm.sample_rate(), rate);
    }

    // Zero rate / zero channels -> typed errors
    assert!(matches!(
        InterleavedPcm::new(&single, 0, 1),
        Err(AudioInputError::ZeroSampleRate)
    ));
    assert!(matches!(
        InterleavedPcm::new(&single, 44100, 0),
        Err(AudioInputError::ZeroChannels)
    ));

    // All-NaN or any-NaN input -> typed error
    let all_nan = [f32::NAN, f32::NAN];
    assert!(matches!(
        InterleavedPcm::new(&all_nan, 44100, 1),
        Err(AudioInputError::NonFiniteSample { index: 0 })
    ));
    let with_nan = [0.1, f32::NAN, 0.3];
    assert!(matches!(
        InterleavedPcm::new(&with_nan, 44100, 1),
        Err(AudioInputError::NonFiniteSample { index: 1 })
    ));
}
