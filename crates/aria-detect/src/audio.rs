//! Decode a media file into the one audio shape every detector reads.

use std::path::Path;

use aria_analysis::InterleavedPcm;

/// Sample rate every audio detector runs at.
///
/// Beats, silence, and loudness are all decided from an envelope, not from
/// spectral detail, so 16 kHz mono is what the analysis crate configures and
/// decodes four times faster than 48 kHz stereo would.
pub const ANALYSIS_SAMPLE_RATE: u32 = 16_000;

/// Default maximum audio duration for one analysis run: 4 hours at [`ANALYSIS_SAMPLE_RATE`].
pub const DEFAULT_MAX_ANALYSIS_FRAMES: usize = ANALYSIS_SAMPLE_RATE as usize * 60 * 240;

/// Legacy 30-minute ceiling preserved for callers that explicitly request it.
pub const MAX_ANALYSIS_FRAMES: usize = ANALYSIS_SAMPLE_RATE as usize * 60 * 30;

/// Decode audio with a caller-configured maximum frame bound.
pub fn decode_mono_with_limit(
    path: &Path,
    max_frames: usize,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<f32>, String> {
    let pcm =
        aria_decoder::read_mono_pcm_with_cancel(path, ANALYSIS_SAMPLE_RATE, max_frames, &cancelled)
            .map_err(|err| err.to_string())?;
    if cancelled() {
        return Err("cancelled".into());
    }
    Ok(pcm)
}

/// Decode the whole file as analysis-rate mono, bounded and cancellable.
/// Uses [`DEFAULT_MAX_ANALYSIS_FRAMES`] so files longer than 30 minutes process successfully.
pub fn decode_mono(path: &Path, cancelled: impl Fn() -> bool) -> Result<Vec<f32>, String> {
    decode_mono_with_limit(path, DEFAULT_MAX_ANALYSIS_FRAMES, cancelled)
}

/// Validate decoded audio once, naming the reason it cannot be analyzed.
///
/// The detectors would each re-validate; doing it here means a NaN sample or a
/// truncated frame is reported as one sentence rather than three identical ones.
pub fn validated_pcm(pcm: &[f32]) -> Result<InterleavedPcm<'_>, String> {
    if pcm.is_empty() {
        return Err("the media decoded to no audio".into());
    }
    InterleavedPcm::new(pcm, ANALYSIS_SAMPLE_RATE, 1)
        .map_err(|err| format!("the decoded audio cannot be analyzed: {err}"))
}

/// Seconds of audio `pcm` represents at [`ANALYSIS_SAMPLE_RATE`].
///
/// One owner for the conversion: a caller that divides by its own idea of the
/// rate silently mismeasures every span it derives.
pub fn seconds_of(pcm: &[f32]) -> f64 {
    pcm.len() as f64 / f64::from(ANALYSIS_SAMPLE_RATE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The class closed here is "empty or unusable audio reads as an absence of
    /// events". Each refusal names its own reason.
    #[test]
    fn unusable_audio_is_refused_by_name() {
        let Err(err) = validated_pcm(&[]) else {
            panic!("empty audio was accepted");
        };
        assert!(err.contains("no audio"), "{err}");

        let Err(err) = validated_pcm(&[0.0, f32::NAN]) else {
            panic!("NaN audio was accepted");
        };
        assert!(err.contains("cannot be analyzed"), "{err}");

        let Ok(view) = validated_pcm(&[0.0, 0.1, -0.1]) else {
            panic!("clean audio was refused");
        };
        assert_eq!(view.frame_count(), 3);
    }

    /// The length-to-seconds conversion is the rate, not a caller's guess.
    #[test]
    fn seconds_follow_the_analysis_rate() {
        assert_eq!(seconds_of(&[]), 0.0);
        assert_eq!(
            seconds_of(&vec![0.0; ANALYSIS_SAMPLE_RATE as usize * 3]),
            3.0
        );

        // Long audio (>30 minutes, e.g. 45 minutes) computes exact seconds
        let forty_five_min_samples = ANALYSIS_SAMPLE_RATE as usize * 60 * 45;
        let pcm_long = vec![0.0f32; forty_five_min_samples];
        assert_eq!(seconds_of(&pcm_long), 45.0 * 60.0);
    }
}
