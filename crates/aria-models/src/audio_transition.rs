//! Audio transitions: the gain curves of a crossfade at the cut between two
//! abutting clips on an audio lane.
//!
//! A [`Transition`](crate::Transition) on an audio lane stores one of these
//! ids where a picture lane stores a [`TransitionSpec`](crate::TransitionSpec)
//! id. The set is closed: each entry is a curve the mixer evaluates per sample,
//! not a pass a plugin supplies, so it is an enum rather than a catalog file.
//!
//! The window is the transition's own: `duration` ticks centred on the cut.
//! The outgoing clip plays past the cut into its source handle and fades out
//! over the window; the incoming clip starts early from its handle and fades
//! in. A side with less handle than half the window fades over the part of the
//! window it covers, so its sound reaches silence where its media ends instead
//! of stopping at a partial level.

use std::f32::consts::FRAC_PI_2;

/// The level the Exponential Fade curve starts from, in dBFS.
pub const EXPONENTIAL_FLOOR_DB: f32 = -60.0;

/// A crossfade curve for the cut between two audio clips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioTransition {
    /// Equal-power: `sin` in, `cos` out, so uncorrelated material keeps its
    /// loudness through the cut.
    ConstantPower,
    /// Linear amplitude: the two gains sum to one, so correlated material keeps
    /// its level through the cut.
    ConstantGain,
    /// Level linear in decibels, from [`EXPONENTIAL_FLOOR_DB`] to 0 dB.
    ExponentialFade,
}

impl AudioTransition {
    /// Every audio transition, in the order a list shows them.
    pub const ALL: [Self; 3] = [
        Self::ConstantPower,
        Self::ConstantGain,
        Self::ExponentialFade,
    ];

    /// The transition Ctrl+Shift+T applies.
    pub const DEFAULT: Self = Self::ConstantPower;

    /// The id a [`Transition`](crate::Transition) stores.
    pub const fn id(self) -> &'static str {
        match self {
            Self::ConstantPower => "constant_power",
            Self::ConstantGain => "constant_gain",
            Self::ExponentialFade => "exponential_fade",
        }
    }

    /// The name a list shows.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ConstantPower => "Constant Power",
            Self::ConstantGain => "Constant Gain",
            Self::ExponentialFade => "Exponential Fade",
        }
    }

    /// The audio transition stored as `id`, or `None` for any other id.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|transition| transition.id() == id)
    }

    /// The incoming side's gain at `progress` through its fade: 0 at the start,
    /// 1 at the end. Progress outside `[0, 1]` reads as the nearer end.
    pub fn incoming(self, progress: f32) -> f32 {
        let p = if progress.is_nan() {
            0.0
        } else {
            progress.clamp(0.0, 1.0)
        };
        match self {
            Self::ConstantPower => (p * FRAC_PI_2).sin(),
            Self::ConstantGain => p,
            Self::ExponentialFade if p <= 0.0 => 0.0,
            Self::ExponentialFade => 10f32.powf(EXPONENTIAL_FLOOR_DB * (1.0 - p) / 20.0),
        }
    }

    /// The outgoing side's gain at `progress` through its fade: 1 at the start,
    /// 0 at the end. The mirror of [`incoming`](Self::incoming).
    pub fn outgoing(self, progress: f32) -> f32 {
        let p = if progress.is_nan() {
            0.0
        } else {
            progress.clamp(0.0, 1.0)
        };
        self.incoming(1.0 - p)
    }
}

/// Which transitions a lane's cuts take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransitionMedia {
    /// A blend from the transition catalog, composited by the renderer.
    Picture,
    /// An [`AudioTransition`] crossfade, applied by the mixer.
    Audio,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ids round-trip, and no audio id shadows a picture transition: a project
    /// file stores only the id, so a shared id would read back as the wrong kind.
    #[test]
    fn ids_round_trip_and_stay_apart_from_the_picture_catalog() {
        for transition in AudioTransition::ALL {
            assert_eq!(AudioTransition::from_id(transition.id()), Some(transition));
            assert!(
                crate::shipped_transition_catalog()
                    .iter()
                    .all(|spec| spec.id != transition.id()),
                "{} is also a picture transition id",
                transition.id()
            );
        }
        assert_eq!(AudioTransition::from_id("crossfade"), None);
    }

    /// Every curve runs from silence to full level, and outgoing mirrors
    /// incoming, so the two sides of a cut meet at the same level.
    #[test]
    fn every_curve_runs_from_silence_to_unity_and_mirrors() {
        for transition in AudioTransition::ALL {
            assert_eq!(transition.incoming(0.0), 0.0, "{transition:?} at 0");
            assert!(
                (transition.incoming(1.0) - 1.0).abs() < 1e-6,
                "{transition:?} at 1"
            );
            assert_eq!(transition.outgoing(1.0), 0.0, "{transition:?} out at 1");
            assert!(
                (transition.outgoing(0.0) - 1.0).abs() < 1e-6,
                "{transition:?} out at 0"
            );
            for step in 0..=20 {
                let p = step as f32 / 20.0;
                assert!(
                    (transition.outgoing(p) - transition.incoming(1.0 - p)).abs() < 1e-6,
                    "{transition:?} at {p}"
                );
            }
            assert_eq!(transition.incoming(-0.5), 0.0);
            assert!((transition.incoming(1.5) - 1.0).abs() < 1e-6);
            assert_eq!(transition.incoming(f32::NAN), 0.0);
        }
    }

    /// The curves differ where it is audible: at the midpoint Constant Power
    /// sums to unit power, Constant Gain to unit amplitude, and Exponential
    /// Fade sits 30 dB down on each side.
    #[test]
    fn the_midpoint_is_where_the_curves_differ() {
        let mid = |t: AudioTransition| (t.incoming(0.5), t.outgoing(0.5));
        let (i, o) = mid(AudioTransition::ConstantPower);
        assert!((i * i + o * o - 1.0).abs() < 1e-6);
        let (i, o) = mid(AudioTransition::ConstantGain);
        assert!((i + o - 1.0).abs() < 1e-6);
        let (i, _) = mid(AudioTransition::ExponentialFade);
        assert!((20.0 * i.log10() - EXPONENTIAL_FLOOR_DB / 2.0).abs() < 1e-3);
    }
}
