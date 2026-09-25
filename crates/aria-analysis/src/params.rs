//! Validated parameters for the audio detectors.
//!
//! Every configuration is checked at construction and names the field it
//! refused ([`crate::ConfigError`]). Nothing is clamped: a silently corrected
//! threshold reads as a detector that does not work, and the caller cannot tell
//! the difference between the value it asked for and the one it got.

use crate::ConfigError;

/// Validated window parameters for RMS loudness analysis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoudnessConfig {
    window_seconds: f64,
    hop_seconds: f64,
    silence_floor_dbfs: f32,
}

impl LoudnessConfig {
    /// Constructs a loudness configuration.
    ///
    /// `hop_seconds` may equal but must not exceed `window_seconds`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] unless window and hop are finite and positive,
    /// and the silence floor is finite and below `0 dBFS`.
    pub fn new(
        window_seconds: f64,
        hop_seconds: f64,
        silence_floor_dbfs: f32,
    ) -> Result<Self, ConfigError> {
        validate_positive_seconds("window_seconds", window_seconds)?;
        validate_positive_seconds("hop_seconds", hop_seconds)?;
        if hop_seconds > window_seconds {
            return Err(ConfigError::new(
                "hop_seconds",
                "must not exceed window_seconds",
            ));
        }
        if !silence_floor_dbfs.is_finite() || silence_floor_dbfs >= 0.0 {
            return Err(ConfigError::new(
                "silence_floor_dbfs",
                "must be finite and below 0 dBFS",
            ));
        }

        Ok(Self {
            window_seconds,
            hop_seconds,
            silence_floor_dbfs,
        })
    }

    /// Analysis window duration in seconds.
    #[must_use]
    pub const fn window_seconds(self) -> f64 {
        self.window_seconds
    }

    /// Distance between consecutive window starts in seconds.
    #[must_use]
    pub const fn hop_seconds(self) -> f64 {
        self.hop_seconds
    }

    /// Finite dBFS value emitted for mathematical zero and lower values.
    #[must_use]
    pub const fn silence_floor_dbfs(self) -> f32 {
        self.silence_floor_dbfs
    }
}

impl Default for LoudnessConfig {
    fn default() -> Self {
        Self {
            window_seconds: 0.050,
            hop_seconds: 0.025,
            silence_floor_dbfs: -120.0,
        }
    }
}

/// Validated silence detection parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilenceConfig {
    loudness: LoudnessConfig,
    threshold_dbfs: f32,
    minimum_duration_seconds: f64,
    merge_gap_seconds: f64,
    hysteresis_db: f32,
}

impl SilenceConfig {
    /// Constructs a silence configuration around validated loudness windows.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when the entry threshold lies outside the
    /// loudness floor/`0 dBFS` range, durations are negative or non-finite,
    /// hysteresis is negative/non-finite, or the hysteresis exit threshold
    /// would exceed `0 dBFS`.
    pub fn new(
        loudness: LoudnessConfig,
        threshold_dbfs: f32,
        minimum_duration_seconds: f64,
        merge_gap_seconds: f64,
        hysteresis_db: f32,
    ) -> Result<Self, ConfigError> {
        if !threshold_dbfs.is_finite()
            || threshold_dbfs < loudness.silence_floor_dbfs
            || threshold_dbfs >= 0.0
        {
            return Err(ConfigError::new(
                "threshold_dbfs",
                "must be finite, at least the silence floor, and below 0 dBFS",
            ));
        }
        validate_non_negative_seconds("minimum_duration_seconds", minimum_duration_seconds)?;
        validate_non_negative_seconds("merge_gap_seconds", merge_gap_seconds)?;
        if !hysteresis_db.is_finite() || hysteresis_db < 0.0 {
            return Err(ConfigError::new(
                "hysteresis_db",
                "must be finite and non-negative",
            ));
        }
        if threshold_dbfs + hysteresis_db > 0.0 {
            return Err(ConfigError::new(
                "hysteresis_db",
                "threshold_dbfs + hysteresis_db must not exceed 0 dBFS",
            ));
        }

        Ok(Self {
            loudness,
            threshold_dbfs,
            minimum_duration_seconds,
            merge_gap_seconds,
            hysteresis_db,
        })
    }

    /// Windowing and finite-floor parameters.
    #[must_use]
    pub const fn loudness(self) -> LoudnessConfig {
        self.loudness
    }

    /// dBFS level at or below which a silent region begins.
    #[must_use]
    pub const fn threshold_dbfs(self) -> f32 {
        self.threshold_dbfs
    }

    /// Minimum duration retained after short-gap merging.
    #[must_use]
    pub const fn minimum_duration_seconds(self) -> f64 {
        self.minimum_duration_seconds
    }

    /// Maximum non-silent gap merged between silent regions.
    #[must_use]
    pub const fn merge_gap_seconds(self) -> f64 {
        self.merge_gap_seconds
    }

    /// Extra dB above the entry threshold required to leave silence.
    #[must_use]
    pub const fn hysteresis_db(self) -> f32 {
        self.hysteresis_db
    }
}

impl Default for SilenceConfig {
    fn default() -> Self {
        Self {
            loudness: LoudnessConfig::default(),
            threshold_dbfs: -50.0,
            minimum_duration_seconds: 0.200,
            merge_gap_seconds: 0.100,
            hysteresis_db: 3.0,
        }
    }
}

/// Validated short-time energy onset/beat detection parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatConfig {
    loudness: LoudnessConfig,
    adaptive_window_seconds: f64,
    threshold_stddevs: f32,
    minimum_onset_db: f32,
    refractory_seconds: f64,
}

impl BeatConfig {
    /// Constructs a beat/onset configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] unless the adaptive window is finite/positive,
    /// the standard-deviation multiplier is finite/non-negative, the minimum
    /// onset is finite/positive, and the refractory duration is
    /// finite/non-negative.
    pub fn new(
        loudness: LoudnessConfig,
        adaptive_window_seconds: f64,
        threshold_stddevs: f32,
        minimum_onset_db: f32,
        refractory_seconds: f64,
    ) -> Result<Self, ConfigError> {
        validate_positive_seconds("adaptive_window_seconds", adaptive_window_seconds)?;
        if !threshold_stddevs.is_finite() || threshold_stddevs < 0.0 {
            return Err(ConfigError::new(
                "threshold_stddevs",
                "must be finite and non-negative",
            ));
        }
        if !minimum_onset_db.is_finite() || minimum_onset_db <= 0.0 {
            return Err(ConfigError::new(
                "minimum_onset_db",
                "must be finite and positive",
            ));
        }
        validate_non_negative_seconds("refractory_seconds", refractory_seconds)?;

        Ok(Self {
            loudness,
            adaptive_window_seconds,
            threshold_stddevs,
            minimum_onset_db,
            refractory_seconds,
        })
    }

    /// Windowing and finite-floor parameters.
    #[must_use]
    pub const fn loudness(self) -> LoudnessConfig {
        self.loudness
    }

    /// Duration of preceding positive changes used for the local threshold.
    #[must_use]
    pub const fn adaptive_window_seconds(self) -> f64 {
        self.adaptive_window_seconds
    }

    /// Standard-deviation multiple added to the local mean threshold.
    #[must_use]
    pub const fn threshold_stddevs(self) -> f32 {
        self.threshold_stddevs
    }

    /// Smallest positive log-energy change accepted as an onset.
    #[must_use]
    pub const fn minimum_onset_db(self) -> f32 {
        self.minimum_onset_db
    }

    /// Minimum time between emitted events.
    #[must_use]
    pub const fn refractory_seconds(self) -> f64 {
        self.refractory_seconds
    }
}

impl Default for BeatConfig {
    fn default() -> Self {
        Self {
            loudness: LoudnessConfig {
                window_seconds: 0.020,
                hop_seconds: 0.010,
                silence_floor_dbfs: -120.0,
            },
            adaptive_window_seconds: 0.500,
            threshold_stddevs: 1.5,
            minimum_onset_db: 3.0,
            refractory_seconds: 0.120,
        }
    }
}

fn validate_positive_seconds(field: &'static str, value: f64) -> Result<(), ConfigError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(ConfigError::new(field, "must be finite and positive"));
    }
    Ok(())
}

fn validate_non_negative_seconds(field: &'static str, value: f64) -> Result<(), ConfigError> {
    if !value.is_finite() || value < 0.0 {
        return Err(ConfigError::new(field, "must be finite and non-negative"));
    }
    Ok(())
}
