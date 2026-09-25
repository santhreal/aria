//! Measuring borrowed PCM: loudness, silence, and beats.
//!
//! [`InterleavedPcm`] is a validated view, not a copy. Construction scans once
//! for frame alignment and non-finite samples, and every measurement then runs
//! over the caller's own buffer, so analysing an hour of audio allocates only
//! the results.
//!
//! All three detectors share one rolling energy pass. Windows are addressed by
//! frame index rather than by accumulated seconds, because adding a hop in
//! floating point drifts, and a drifting window puts a beat in the wrong place
//! by the end of a long take.

use crate::{
    AudioInputError, BeatConfig, LoudnessConfig, SilenceConfig, TimeSpan, duration_to_frames,
    seconds_at_frame,
};

/// One window in a loudness contour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoudnessPoint {
    /// Exact half-open window boundaries derived from PCM frame indices.
    pub span: TimeSpan,
    /// Window RMS in dBFS, clamped to the configured finite floor and `0 dBFS`.
    pub dbfs: f32,
}

/// A detected region of silence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilenceSpan {
    /// Exact half-open boundaries derived from PCM frame indices.
    pub span: TimeSpan,
}

/// A beat/onset candidate detected from a positive short-time energy change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatEvent {
    /// Event time in seconds, mapped from the end frame of the causal window.
    pub time_seconds: f64,
    /// Positive log-energy increase in decibels, always finite.
    pub strength_db: f32,
}

/// A validated borrowed view of finite, interleaved `f32` PCM.
///
/// Construction performs one validation scan but does not copy the samples.
#[derive(Clone, Copy)]
pub struct InterleavedPcm<'a> {
    samples: &'a [f32],
    sample_rate: u32,
    channels: usize,
}

impl<'a> InterleavedPcm<'a> {
    /// Validates and borrows an interleaved PCM slice.
    ///
    /// An empty slice is valid when `sample_rate` and `channels` are non-zero.
    ///
    /// # Errors
    ///
    /// Rejects a zero sample rate, zero channels, a sample count that does not
    /// contain complete frames, or any NaN/infinite sample.
    pub fn new(
        samples: &'a [f32],
        sample_rate: u32,
        channels: usize,
    ) -> Result<Self, AudioInputError> {
        if sample_rate == 0 {
            return Err(AudioInputError::ZeroSampleRate);
        }
        if channels == 0 {
            return Err(AudioInputError::ZeroChannels);
        }
        if !samples.len().is_multiple_of(channels) {
            return Err(AudioInputError::NonFrameAligned {
                sample_count: samples.len(),
                channels,
            });
        }
        if let Some(index) = samples.iter().position(|sample| !sample.is_finite()) {
            return Err(AudioInputError::NonFiniteSample { index });
        }

        Ok(Self {
            samples,
            sample_rate,
            channels,
        })
    }

    /// The original borrowed interleaved samples.
    #[must_use]
    pub const fn samples(self) -> &'a [f32] {
        self.samples
    }

    /// Frames per second.
    #[must_use]
    pub const fn sample_rate(self) -> u32 {
        self.sample_rate
    }

    /// Number of interleaved channels per frame.
    #[must_use]
    pub const fn channels(self) -> usize {
        self.channels
    }

    /// Number of complete PCM frames.
    #[must_use]
    pub fn frame_count(self) -> usize {
        self.samples.len() / self.channels
    }

    /// Exact input duration as `frame_count / sample_rate` seconds.
    #[must_use]
    pub fn duration_seconds(self) -> f64 {
        seconds_at_frame(self.frame_count(), self.sample_rate)
    }

    /// Whether the PCM contains no frames.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.samples.is_empty()
    }

    /// Computes a deterministic windowed-RMS loudness contour.
    ///
    /// Channel energies are averaged, preventing opposite-polarity cancellation.
    /// The final point may cover a shorter partial window. Empty PCM returns an
    /// empty vector.
    ///
    /// Complexity is `O(samples + windows)` time and `O(windows)` output space.
    #[must_use]
    pub fn loudness_contour(self, config: LoudnessConfig) -> Vec<LoudnessPoint> {
        let windows = energy_windows(self, config.window_seconds(), config.hop_seconds());

        windows
            .into_iter()
            .map(|window| LoudnessPoint {
                span: TimeSpan::from_frames(window.start_frame, window.end_frame, self.sample_rate),
                dbfs: dbfs_from_mean_square(window.mean_square, config.silence_floor_dbfs()),
            })
            .collect()
    }

    /// Detects silence with entry/exit hysteresis, short-gap merging, and a
    /// minimum final duration.
    ///
    /// A region enters silence at or below `threshold_dbfs` and remains silent
    /// until it rises above `threshold_dbfs + hysteresis_db`. Raw silent regions
    /// separated by at most `merge_gap_seconds` are merged *before* the
    /// minimum-duration filter is applied.
    ///
    /// Complexity is `O(samples + windows)` time and `O(windows)` worst-case
    /// auxiliary/output space.
    #[must_use]
    pub fn detect_silence(self, config: SilenceConfig) -> Vec<SilenceSpan> {
        if self.is_empty() {
            return Vec::new();
        }

        let windows = energy_windows(
            self,
            config.loudness().window_seconds(),
            config.loudness().hop_seconds(),
        );
        let exit_threshold = config.threshold_dbfs() + config.hysteresis_db();
        let mut raw_spans = Vec::new();
        let mut silence_start = None;

        for window in windows {
            let dbfs =
                dbfs_from_mean_square(window.mean_square, config.loudness().silence_floor_dbfs());
            let is_silent = if silence_start.is_some() {
                dbfs <= exit_threshold
            } else {
                dbfs <= config.threshold_dbfs()
            };

            match (silence_start, is_silent) {
                (None, true) => silence_start = Some(window.start_frame),
                (Some(start_frame), false) => {
                    if window.start_frame > start_frame {
                        raw_spans.push(FrameSpan {
                            start_frame,
                            end_frame: window.start_frame,
                        });
                    }
                    silence_start = None;
                }
                _ => {}
            }
        }

        if let Some(start_frame) = silence_start {
            raw_spans.push(FrameSpan {
                start_frame,
                end_frame: self.frame_count(),
            });
        }

        let mut merged: Vec<FrameSpan> = Vec::with_capacity(raw_spans.len());
        let maximum_gap_frames = config.merge_gap_seconds() * f64::from(self.sample_rate);

        for span in raw_spans {
            if let Some(previous) = merged.last_mut() {
                let gap_frames = span.start_frame.saturating_sub(previous.end_frame);
                if gap_frames as f64 <= maximum_gap_frames {
                    previous.end_frame = span.end_frame;
                    continue;
                }
            }
            merged.push(span);
        }

        let minimum_frames = config.minimum_duration_seconds() * f64::from(self.sample_rate);
        merged
            .into_iter()
            .filter(|span| span.duration_frames() as f64 >= minimum_frames)
            .map(|span| SilenceSpan {
                span: TimeSpan::from_frames(span.start_frame, span.end_frame, self.sample_rate),
            })
            .collect()
    }

    /// Detects beat/onset candidates from positive short-time log-energy change.
    ///
    /// Each positive change is compared with the mean plus a configurable
    /// standard-deviation multiple over a preceding local history. A minimum
    /// onset strength rejects numerical/tonal ripple, and the refractory
    /// interval suppresses candidates too close to the last emitted event.
    ///
    /// Complexity is `O(samples + windows)` time and `O(windows)` auxiliary
    /// space. Empty PCM and constant energy return no events.
    #[must_use]
    pub fn detect_beats(self, config: BeatConfig) -> Vec<BeatEvent> {
        if self.is_empty() {
            return Vec::new();
        }

        let mut windows = energy_windows(
            self,
            config.loudness().window_seconds(),
            config.loudness().hop_seconds(),
        );
        let complete_window_frames = duration_to_frames(
            config.loudness().window_seconds(),
            self.sample_rate,
            self.frame_count(),
        );
        let complete_window_count = windows
            .iter()
            .position(|window| window.end_frame - window.start_frame < complete_window_frames)
            .unwrap_or(windows.len());
        windows.truncate(complete_window_count);

        let mut previous_dbfs =
            windows
                .first()
                .map_or(config.loudness().silence_floor_dbfs(), |window| {
                    dbfs_from_mean_square(
                        window.mean_square,
                        config.loudness().silence_floor_dbfs(),
                    )
                });
        let mut fluxes = Vec::with_capacity(windows.len());

        for (index, window) in windows.iter().enumerate() {
            let current_dbfs =
                dbfs_from_mean_square(window.mean_square, config.loudness().silence_floor_dbfs());
            let flux = if index == 0 {
                0.0
            } else {
                (current_dbfs - previous_dbfs).max(0.0)
            };
            fluxes.push(flux);
            previous_dbfs = current_dbfs;
        }

        let hop_frames = duration_to_frames(
            config.loudness().hop_seconds(),
            self.sample_rate,
            self.frame_count(),
        );
        let history_frames = duration_to_frames(
            config.adaptive_window_seconds(),
            self.sample_rate,
            self.frame_count(),
        );
        let history_limit = history_frames.div_ceil(hop_frames).max(1);
        let refractory_frames = config.refractory_seconds() * f64::from(self.sample_rate);

        let mut events = Vec::new();
        let mut history_sum = 0.0_f64;
        let mut history_squared_sum = 0.0_f64;
        let mut last_event_frame = None;

        for (index, (&flux, window)) in fluxes.iter().zip(&windows).enumerate() {
            if index > history_limit {
                let expired = f64::from(fluxes[index - history_limit - 1]);
                history_sum -= expired;
                history_squared_sum -= expired * expired;
            }

            let history_count = index.min(history_limit);
            let adaptive_threshold = if history_count == 0 {
                f64::from(config.minimum_onset_db())
            } else {
                let count = history_count as f64;
                let mean = history_sum / count;
                let variance = (history_squared_sum / count - mean * mean).max(0.0);
                (mean + f64::from(config.threshold_stddevs()) * variance.sqrt())
                    .max(f64::from(config.minimum_onset_db()))
            };

            let flux_f64 = f64::from(flux);
            if flux_f64 >= adaptive_threshold {
                let event_frame = window.end_frame;
                let outside_refractory = last_event_frame.is_none_or(|previous_frame| {
                    event_frame.saturating_sub(previous_frame) as f64 >= refractory_frames
                });

                if outside_refractory {
                    events.push(BeatEvent {
                        time_seconds: seconds_at_frame(event_frame, self.sample_rate),
                        strength_db: flux,
                    });
                    last_event_frame = Some(event_frame);
                }
            }

            history_sum += flux_f64;
            history_squared_sum += flux_f64 * flux_f64;
        }

        events
    }
}

/// Validates PCM and computes a windowed-RMS loudness contour.
///
/// This convenience API performs `O(samples + windows)` work, borrows rather
/// than copies `samples`, and returns an empty vector for valid empty PCM.
///
/// # Errors
///
/// Returns [`AudioInputError`] when the PCM metadata, frame alignment, or
/// sample values are invalid.
pub fn loudness_contour(
    samples: &[f32],
    sample_rate: u32,
    channels: usize,
    config: LoudnessConfig,
) -> Result<Vec<LoudnessPoint>, AudioInputError> {
    let pcm = InterleavedPcm::new(samples, sample_rate, channels)?;
    Ok(pcm.loudness_contour(config))
}

/// Validates PCM and detects silence with hysteresis and short-gap merging.
///
/// This convenience API performs `O(samples + windows)` work, borrows rather
/// than copies `samples`, and returns an empty vector for valid empty PCM.
///
/// # Errors
///
/// Returns [`AudioInputError`] when the PCM metadata, frame alignment, or
/// sample values are invalid.
pub fn detect_silence(
    samples: &[f32],
    sample_rate: u32,
    channels: usize,
    config: SilenceConfig,
) -> Result<Vec<SilenceSpan>, AudioInputError> {
    let pcm = InterleavedPcm::new(samples, sample_rate, channels)?;
    Ok(pcm.detect_silence(config))
}

/// Validates PCM and detects adaptive short-time-energy beat/onset events.
///
/// This convenience API performs `O(samples + windows)` work, borrows rather
/// than copies `samples`, and returns an empty vector for valid empty PCM.
///
/// # Errors
///
/// Returns [`AudioInputError`] when the PCM metadata, frame alignment, or
/// sample values are invalid.
pub fn detect_beats(
    samples: &[f32],
    sample_rate: u32,
    channels: usize,
    config: BeatConfig,
) -> Result<Vec<BeatEvent>, AudioInputError> {
    let pcm = InterleavedPcm::new(samples, sample_rate, channels)?;
    Ok(pcm.detect_beats(config))
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct EnergyWindow {
    start_frame: usize,
    end_frame: usize,
    mean_square: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameSpan {
    start_frame: usize,
    end_frame: usize,
}

impl FrameSpan {
    fn duration_frames(self) -> usize {
        self.end_frame.saturating_sub(self.start_frame)
    }
}

fn energy_windows(
    pcm: InterleavedPcm<'_>,
    window_seconds: f64,
    hop_seconds: f64,
) -> Vec<EnergyWindow> {
    let frame_count = pcm.frame_count();
    if frame_count == 0 {
        return Vec::new();
    }

    let window_frames = duration_to_frames(window_seconds, pcm.sample_rate, frame_count);
    let hop_frames = duration_to_frames(hop_seconds, pcm.sample_rate, frame_count);
    let window_count = (frame_count - 1) / hop_frames + 1;
    let mut windows = Vec::with_capacity(window_count);

    let mut start_frame = 0;
    let mut end_frame = window_frames.min(frame_count);
    let mut energy_sum = squared_sample_sum(pcm, start_frame, end_frame);

    loop {
        let sample_count = (end_frame - start_frame) * pcm.channels;
        windows.push(EnergyWindow {
            start_frame,
            end_frame,
            mean_square: energy_sum / sample_count as f64,
        });

        let Some(next_start_frame) = start_frame.checked_add(hop_frames) else {
            break;
        };
        if next_start_frame >= frame_count {
            break;
        }
        let next_end_frame = next_start_frame
            .saturating_add(window_frames)
            .min(frame_count);

        if next_start_frame < end_frame {
            energy_sum -= squared_sample_sum(pcm, start_frame, next_start_frame);
            if next_end_frame > end_frame {
                energy_sum += squared_sample_sum(pcm, end_frame, next_end_frame);
            }
            energy_sum = energy_sum.max(0.0);
        } else {
            energy_sum = squared_sample_sum(pcm, next_start_frame, next_end_frame);
        }

        start_frame = next_start_frame;
        end_frame = next_end_frame;
    }

    windows
}

fn squared_sample_sum(pcm: InterleavedPcm<'_>, start_frame: usize, end_frame: usize) -> f64 {
    let start_sample = start_frame * pcm.channels;
    let end_sample = end_frame * pcm.channels;
    pcm.samples[start_sample..end_sample]
        .iter()
        .map(|&sample| {
            let sample = f64::from(sample);
            sample * sample
        })
        .sum()
}

fn dbfs_from_mean_square(mean_square: f64, silence_floor_dbfs: f32) -> f32 {
    if mean_square <= 0.0 {
        return silence_floor_dbfs;
    }

    let raw_dbfs = 10.0 * mean_square.log10();
    if raw_dbfs.is_finite() {
        raw_dbfs.clamp(f64::from(silence_floor_dbfs), 0.0) as f32
    } else {
        0.0
    }
}

#[cfg(test)]
#[path = "pcm_tests.rs"]
mod tests;
