//! The shipped beat, silence, and loudness analyzers, built on `aria_analysis`.

use std::ops::ControlFlow;

use super::{
    Analyzer, AnalyzerLoudness, AnalyzerRange, AnalyzerResult, AnalyzerRun, AnalyzerScope,
    AnalyzerSpec,
};
use crate::context::PluginContext;
use crate::error::PluginError;
use crate::registration::Registration;

/// Shipped beat analyzer id (`Clip > Analyze > Detect Beats`).
pub const SHIPPED_BEAT_ANALYZER_ID: &str = "aria.beats";

/// Shipped silence analyzer id (`Sequence > Analyze > Detect Silence`).
pub const SHIPPED_SILENCE_ANALYZER_ID: &str = "aria.silence";

/// Shipped loudness analyzer id (`Sequence > Analyze > Measure Loudness`).
pub const SHIPPED_LOUDNESS_ANALYZER_ID: &str = "aria.loudness";

/// Shipped beat analyzer backed by [`aria_analysis::InterleavedPcm::detect_beats`].
#[derive(Debug, Clone, Copy, Default)]
pub struct BeatAnalyzer {
    pub config: aria_analysis::BeatConfig,
}

impl BeatAnalyzer {
    pub const fn new(config: aria_analysis::BeatConfig) -> Self {
        Self { config }
    }

    pub fn spec() -> AnalyzerSpec {
        AnalyzerSpec::new(
            SHIPPED_BEAT_ANALYZER_ID,
            "Detect Beats",
            AnalyzerScope::Clip,
        )
        .with_category("Rhythm")
    }
}

struct BeatAnalyzerRun {
    rate: u32,
    channels: u16,
    config: aria_analysis::BeatConfig,
    base_frame: Option<i64>,
    samples: Vec<f32>,
}

impl Analyzer for BeatAnalyzer {
    fn begin(&self, rate: u32, channels: u16) -> Box<dyn AnalyzerRun> {
        Box::new(BeatAnalyzerRun {
            rate,
            channels,
            config: self.config,
            base_frame: None,
            samples: Vec::new(),
        })
    }
}

impl AnalyzerRun for BeatAnalyzerRun {
    fn block(&mut self, first_frame: i64, samples: &[f32]) -> ControlFlow<()> {
        if self.base_frame.is_none() {
            self.base_frame = Some(first_frame);
        }
        self.samples
            .extend(samples.iter().map(|&s| if s.is_finite() { s } else { 0.0 }));
        ControlFlow::Continue(())
    }

    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
        let channels = usize::from(self.channels);
        if self.rate == 0 || channels == 0 || self.samples.is_empty() {
            return Ok(AnalyzerResult::default());
        }
        let base_frame = self.base_frame.unwrap_or(0);
        let pcm = aria_analysis::InterleavedPcm::new(&self.samples, self.rate, channels)
            .map_err(|err| err.to_string())?;
        let total_frames = pcm.frame_count();
        let window_frames = (self.config.loudness().window_seconds() * f64::from(self.rate))
            .round()
            .max(1.0) as usize;
        let refractory_frames =
            (self.config.refractory_seconds() * f64::from(self.rate)).round() as i64;

        let mut beats = Vec::new();

        // Check for an impulse at the very beginning of the window (which window 0
        // flux suppresses in `InterleavedPcm::detect_beats`): if the first window
        // has energy above the onset floor and decays by window 2, refine its
        // leading edge inside window 0.
        let contour = pcm.loudness_contour(self.config.loudness());
        if contour.len() >= 3 {
            let first_db = contour[0].dbfs;
            let third_db = contour[2].dbfs;
            if first_db - third_db >= self.config.minimum_onset_db() {
                let onset =
                    refine_onset_frame(&self.samples, channels, 0, window_frames.min(total_frames));
                beats.push(base_frame + onset as i64);
            }
        }

        for event in pcm.detect_beats(self.config) {
            let end_frame =
                ((event.time_seconds * f64::from(self.rate)).round() as usize).min(total_frames);
            let start_frame = end_frame.saturating_sub(window_frames);
            let onset = refine_onset_frame(&self.samples, channels, start_frame, end_frame);
            let candidate = base_frame + onset as i64;
            if beats
                .last()
                .is_none_or(|&prev| candidate - prev >= refractory_frames.max(1))
            {
                beats.push(candidate);
            }
        }

        Ok(AnalyzerResult {
            beats,
            ..Default::default()
        })
    }
}

fn refine_onset_frame(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
) -> usize {
    if start_frame >= end_frame || channels == 0 {
        return start_frame;
    }
    let mut max_energy = 0.0f32;
    for frame in start_frame..end_frame {
        let base = frame * channels;
        let energy: f32 = samples[base..base + channels].iter().map(|&s| s * s).sum();
        if energy > max_energy {
            max_energy = energy;
        }
    }
    if max_energy <= 0.0 {
        return start_frame;
    }
    let threshold = max_energy * 0.25;
    for frame in start_frame..end_frame {
        let base = frame * channels;
        let energy: f32 = samples[base..base + channels].iter().map(|&s| s * s).sum();
        if energy >= threshold {
            return frame;
        }
    }
    start_frame
}

/// Shipped silence analyzer backed by [`aria_analysis::InterleavedPcm::detect_silence`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SilenceAnalyzer {
    pub config: aria_analysis::SilenceConfig,
}

impl SilenceAnalyzer {
    pub const fn new(config: aria_analysis::SilenceConfig) -> Self {
        Self { config }
    }

    pub fn spec() -> AnalyzerSpec {
        AnalyzerSpec::new(
            SHIPPED_SILENCE_ANALYZER_ID,
            "Detect Silence",
            AnalyzerScope::Sequence,
        )
        .with_category("Segmentation")
    }
}

struct SilenceAnalyzerRun {
    rate: u32,
    channels: u16,
    config: aria_analysis::SilenceConfig,
    base_frame: Option<i64>,
    samples: Vec<f32>,
}

impl Analyzer for SilenceAnalyzer {
    fn begin(&self, rate: u32, channels: u16) -> Box<dyn AnalyzerRun> {
        Box::new(SilenceAnalyzerRun {
            rate,
            channels,
            config: self.config,
            base_frame: None,
            samples: Vec::new(),
        })
    }
}

impl AnalyzerRun for SilenceAnalyzerRun {
    fn block(&mut self, first_frame: i64, samples: &[f32]) -> ControlFlow<()> {
        if self.base_frame.is_none() {
            self.base_frame = Some(first_frame);
        }
        self.samples
            .extend(samples.iter().map(|&s| if s.is_finite() { s } else { 0.0 }));
        ControlFlow::Continue(())
    }

    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
        let channels = usize::from(self.channels);
        if self.rate == 0 || channels == 0 || self.samples.is_empty() {
            return Ok(AnalyzerResult::default());
        }
        let base_frame = self.base_frame.unwrap_or(0);
        let pcm = aria_analysis::InterleavedPcm::new(&self.samples, self.rate, channels)
            .map_err(|err| err.to_string())?;
        let ranges = pcm
            .detect_silence(self.config)
            .into_iter()
            .filter_map(|span| {
                let start =
                    base_frame + (span.span.start_seconds() * f64::from(self.rate)).round() as i64;
                let end =
                    base_frame + (span.span.end_seconds() * f64::from(self.rate)).round() as i64;
                (end > start).then(|| AnalyzerRange::new(start, end, "Silence"))
            })
            .collect();
        Ok(AnalyzerResult {
            ranges,
            ..Default::default()
        })
    }
}

/// Shipped loudness analyzer backed by [`aria_analysis::InterleavedPcm::loudness_contour`].
#[derive(Debug, Clone, Copy, Default)]
pub struct LoudnessAnalyzer {
    pub config: aria_analysis::LoudnessConfig,
}

impl LoudnessAnalyzer {
    pub const fn new(config: aria_analysis::LoudnessConfig) -> Self {
        Self { config }
    }

    pub fn spec() -> AnalyzerSpec {
        AnalyzerSpec::new(
            SHIPPED_LOUDNESS_ANALYZER_ID,
            "Measure Loudness",
            AnalyzerScope::InOut,
        )
        .with_category("Loudness")
    }
}

struct LoudnessAnalyzerRun {
    rate: u32,
    channels: u16,
    config: aria_analysis::LoudnessConfig,
    samples: Vec<f32>,
}

impl Analyzer for LoudnessAnalyzer {
    fn begin(&self, rate: u32, channels: u16) -> Box<dyn AnalyzerRun> {
        Box::new(LoudnessAnalyzerRun {
            rate,
            channels,
            config: self.config,
            samples: Vec::new(),
        })
    }
}

impl AnalyzerRun for LoudnessAnalyzerRun {
    fn block(&mut self, _first_frame: i64, samples: &[f32]) -> ControlFlow<()> {
        self.samples
            .extend(samples.iter().map(|&s| if s.is_finite() { s } else { 0.0 }));
        ControlFlow::Continue(())
    }

    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
        let channels = usize::from(self.channels);
        if self.rate == 0 || channels == 0 || self.samples.is_empty() {
            return Ok(AnalyzerResult {
                loudness: Some(AnalyzerLoudness {
                    integrated_lufs: f64::NEG_INFINITY,
                    true_peak_db: f64::NEG_INFINITY,
                    range_lu: 0.0,
                }),
                ..Default::default()
            });
        }
        let pcm = aria_analysis::InterleavedPcm::new(&self.samples, self.rate, channels)
            .map_err(|err| err.to_string())?;
        let peak = self.samples.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
        let true_peak_db = if peak > 0.0 {
            20.0 * f64::from(peak).log10()
        } else {
            f64::NEG_INFINITY
        };
        let contour = pcm.loudness_contour(self.config);
        let active: Vec<f64> = contour
            .iter()
            .map(|p| f64::from(p.dbfs))
            .filter(|&db| db > f64::from(self.config.silence_floor_dbfs()) && db > -70.0)
            .collect();
        let (integrated_lufs, range_lu) = if active.is_empty() {
            (f64::NEG_INFINITY, 0.0)
        } else {
            let mean_power: f64 = active
                .iter()
                .map(|&db| 10.0_f64.powf(db / 10.0))
                .sum::<f64>()
                / active.len() as f64;
            let ungated_lufs = -0.691 + 10.0 * mean_power.log10();
            let rel_gate = ungated_lufs - 10.0;
            let mut gated: Vec<f64> = active
                .into_iter()
                .filter(|&db| db - 0.691 >= rel_gate)
                .collect();
            if gated.is_empty() {
                (ungated_lufs, 0.0)
            } else {
                let gated_power: f64 = gated
                    .iter()
                    .map(|&db| 10.0_f64.powf(db / 10.0))
                    .sum::<f64>()
                    / gated.len() as f64;
                let lufs = -0.691 + 10.0 * gated_power.log10();
                gated.sort_by(|a, b| a.total_cmp(b));
                let lra = if gated.len() >= 2 {
                    let lo = gated[(gated.len() - 1) * 10 / 100];
                    let hi = gated[(gated.len() - 1) * 95 / 100];
                    (hi - lo).max(0.0)
                } else {
                    0.0
                };
                (lufs, lra)
            }
        };
        Ok(AnalyzerResult {
            loudness: Some(AnalyzerLoudness {
                integrated_lufs,
                true_peak_db,
                range_lu,
            }),
            ..Default::default()
        })
    }
}

/// Register the shipped beat, silence, and loudness analyzers on `ctx`.
pub fn register_shipped_analyzers(
    ctx: &PluginContext,
    beats: aria_analysis::BeatConfig,
    silence: aria_analysis::SilenceConfig,
    loudness: aria_analysis::LoudnessConfig,
) -> Result<Vec<Registration>, PluginError> {
    Ok(vec![
        ctx.register_analyzer(BeatAnalyzer::spec(), BeatAnalyzer::new(beats))?,
        ctx.register_analyzer(SilenceAnalyzer::spec(), SilenceAnalyzer::new(silence))?,
        ctx.register_analyzer(LoudnessAnalyzer::spec(), LoudnessAnalyzer::new(loudness))?,
    ])
}
