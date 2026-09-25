//! Where one signal envelope sits against another: the lag that matches them
//! best, and how well it matched.
//!
//! This is the measurement behind syncing cameras by their sound. The inputs
//! are envelopes, not samples: one value per fixed slice of time, which is what
//! a waveform peak array already is. Correlating envelopes rather than raw PCM
//! is what makes the search cheap enough to run on a button press, and it is
//! enough for the job, because two microphones in one room agree about where
//! the loud moments are long before they agree about a waveform.
//!
//! The result is a lag, how well it matched, and how well anything else matched.
//! Nothing here decides whether that is good enough. The threshold belongs to
//! the caller: a surface that offers to align two files knows what it will do
//! with a weak match, and a number this module invented would be applied to
//! signals it never saw.

use crate::ConfigError;

/// Hard ceiling on the search radius, in envelope samples.
///
/// The search is `O(max_lag * len)`, so this is what keeps a hostile
/// configuration from turning one press into minutes of work. At the 40 peaks
/// per second the editor decodes, this is a search of about 25 minutes either
/// way, which is longer than any two cameras of the same event can plausibly
/// start apart.
pub const MAX_ALIGN_LAG: usize = 60_000;

/// Hard ceiling on the envelope length either side may carry.
///
/// At 40 peaks per second this is about 7 hours, past the point where a longer
/// input says anything the first hours did not.
pub const MAX_ALIGN_SAMPLES: usize = 1_000_000;

/// Samples per side the coarse pass of [`align_anywhere`] works over.
///
/// This is a bound on work, not a claim about the material: the pass costs
/// about `ALIGN_BUDGET` squared comparisons whatever the inputs are, and the
/// fine pass that follows restores full resolution. Raising it buys nothing,
/// because the coarse lag is refined anyway.
pub const ALIGN_BUDGET: usize = 4_096;

/// How far from the reported lag a rival score has to sit to count as a second
/// answer rather than the same peak's shoulder.
///
/// A correlation peak decays over a few samples, so a high score one or two lags
/// away is the same match seen from beside it. A high score further out is a
/// different answer, which is what a repeating sound produces: a metronome, an
/// alarm, a rotor or a hum matches equally well at every repeat, and the best of
/// those is a coin toss wearing a measurement's clothes. At the envelope rates
/// this crate is handed, eight samples is about a fifth of a second.
pub const RIVAL_GAP: usize = 8;

/// A validated alignment search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlignConfig {
    max_lag: usize,
    min_overlap: usize,
}

impl AlignConfig {
    /// Build a search that looks up to `max_lag` samples either way and only
    /// trusts a lag that leaves at least `min_overlap` samples overlapping.
    ///
    /// A lag with little overlap is the classic false positive: two samples of
    /// one loud moment correlate perfectly and say nothing, so the overlap is a
    /// floor rather than a preference.
    pub fn new(max_lag: usize, min_overlap: usize) -> Result<Self, ConfigError> {
        if max_lag == 0 {
            return Err(ConfigError::new("max_lag", "must search at least one lag"));
        }
        if max_lag > MAX_ALIGN_LAG {
            return Err(ConfigError::new("max_lag", "exceeds the search ceiling"));
        }
        if min_overlap < 2 {
            return Err(ConfigError::new(
                "min_overlap",
                "two samples cannot establish a match",
            ));
        }
        Ok(Self {
            max_lag,
            min_overlap,
        })
    }

    /// The search radius in envelope samples.
    #[must_use]
    pub const fn max_lag(self) -> usize {
        self.max_lag
    }

    /// The smallest overlap a reported lag may rest on.
    #[must_use]
    pub const fn min_overlap(self) -> usize {
        self.min_overlap
    }
}

/// The lag that matched best, and its normalised correlation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alignment {
    /// Where `candidate[0]` sits on the reference's clock, in envelope samples.
    /// Positive means the candidate starts later than the reference.
    pub lag: isize,
    /// Normalised cross-correlation over the overlap, in `-1.0..=1.0`. One is
    /// an exact match of shape, zero is no relation.
    pub score: f32,
    /// How many samples the score was computed over.
    pub overlap: usize,
    /// The best score at a lag further than [`RIVAL_GAP`] from `lag`, or `-1.0`
    /// when the search had no such lag to compare.
    ///
    /// A rival near `score` means the signals match this well in more than one
    /// place, so the reported lag is one answer among several rather than the
    /// answer. Whether that is disqualifying is the caller's to decide, like the
    /// score itself: the sign the caller needs is `score - rival`.
    pub rival: f32,
}

/// The lag at which `candidate` best matches `reference`.
///
/// `None` when no lag in range leaves `min_overlap` samples to compare, when
/// either input is longer than [`MAX_ALIGN_SAMPLES`], or when every candidate
/// window is flat: silence and a constant tone carry no evidence about time,
/// and a lag reported from one would be a guess dressed as a measurement.
///
/// The correlation is computed on mean-removed windows, because an envelope is
/// non-negative: without removing the mean, every lag scores near one and the
/// peak says nothing.
#[must_use]
pub fn align(reference: &[f32], candidate: &[f32], config: AlignConfig) -> Option<Alignment> {
    align_near(reference, candidate, 0, config)
}

/// The lag nearest `center` at which `candidate` best matches `reference`.
///
/// Same measurement as [`align`], searching `center - max_lag ..= center +
/// max_lag` instead of around zero. This is the refinement half of
/// [`align_anywhere`]: a coarse pass names roughly where the match is, and this
/// one looks there at full resolution.
#[must_use]
pub fn align_near(
    reference: &[f32],
    candidate: &[f32],
    center: isize,
    config: AlignConfig,
) -> Option<Alignment> {
    if reference.len() > MAX_ALIGN_SAMPLES || candidate.len() > MAX_ALIGN_SAMPLES {
        return None;
    }
    let max_lag = config.max_lag as isize;
    let lo = center.saturating_sub(max_lag);
    let hi = center.saturating_add(max_lag);
    // Every lag's score, kept so the rival can be read off once the winner is
    // known: which lags count as rivals depends on where the winner landed, and
    // scoring the range twice would double the work of a press.
    let mut scores = vec![f32::NEG_INFINITY; (hi - lo).unsigned_abs() + 1];
    let mut best: Option<Alignment> = None;
    for lag in lo..=hi {
        // The overlap in reference coordinates: candidate sample `i` lines up
        // with reference sample `i + lag`.
        let start = lag.max(0);
        let end = (candidate.len() as isize + lag).min(reference.len() as isize);
        if end - start < config.min_overlap as isize {
            continue;
        }
        let a = &reference[start as usize..end as usize];
        let b = &candidate[(start - lag) as usize..(end - lag) as usize];
        let Some(score) = correlation(a, b) else {
            continue;
        };
        scores[(lag - lo) as usize] = score;
        let overlap = a.len();
        // A strictly better score wins; ties keep the lag nearest the centre, so
        // an identical pair reports zero rather than whichever edge came first.
        let better = match &best {
            None => true,
            Some(current) => {
                score > current.score
                    || (score == current.score
                        && (lag - center).abs() < (current.lag - center).abs())
            }
        };
        if better {
            best = Some(Alignment {
                lag,
                score,
                overlap,
                rival: -1.0,
            });
        }
    }
    let best = best?;
    let rival = scores
        .iter()
        .enumerate()
        .filter(|(index, _)| (lo + *index as isize - best.lag).unsigned_abs() > RIVAL_GAP)
        .fold(f32::NEG_INFINITY, |high, (_, score)| high.max(*score));
    Some(Alignment {
        rival: if rival.is_finite() { rival } else { -1.0 },
        ..best
    })
}

/// The lag at which `candidate` best matches `reference`, with no search radius
/// to choose.
///
/// Two recordings of one event can start any distance apart, so a radius in
/// seconds would be an invented fact about the shoot. Instead the work is
/// bounded: both envelopes are max-pooled down to [`ALIGN_BUDGET`] samples, the
/// whole valid overlap range is searched there, and the answer is refined at
/// full resolution around what the coarse pass found. Max-pooling is the same
/// reduction a waveform uses to draw fewer bars than it decoded, so a transient
/// survives it and the coarse pass locks onto the same moment the fine pass
/// does.
///
/// The overlap floor is a quarter of the shorter envelope. A lag that leaves
/// less than that is where false matches live: two samples of one loud moment
/// correlate perfectly and mean nothing.
///
/// [`Alignment::rival`] is read over the whole range rather than over the fine
/// pass's window. The fine pass looks a few samples either side of the coarse
/// answer, so on its own it can only ever report the winning peak's shoulders,
/// and a sound that repeats every ten seconds would come back looking unique.
#[must_use]
pub fn align_anywhere(reference: &[f32], candidate: &[f32]) -> Option<Alignment> {
    if reference.len() > MAX_ALIGN_SAMPLES || candidate.len() > MAX_ALIGN_SAMPLES {
        return None;
    }
    let shortest = reference.len().min(candidate.len());
    if shortest < 8 {
        return None;
    }
    let overlap_floor = (shortest / 4).max(2);
    let longest = reference.len().max(candidate.len());
    let factor = longest.div_ceil(ALIGN_BUDGET).max(1);
    let (center, coarse_rival) = if factor == 1 {
        (0, f32::NEG_INFINITY)
    } else {
        let coarse_reference = pooled(reference, factor);
        let coarse_candidate = pooled(candidate, factor);
        let coarse_shortest = coarse_reference.len().min(coarse_candidate.len());
        let coarse = AlignConfig::new(
            coarse_reference.len().max(coarse_candidate.len()).max(1),
            (coarse_shortest / 4).max(2),
        )
        .ok()?;
        let found = align(&coarse_reference, &coarse_candidate, coarse)?;
        (found.lag * factor as isize, found.rival)
    };
    // Two coarse samples either way: the coarse lag is exact to within one of
    // its own samples, and the second one is the margin the pooling costs.
    let radius = if factor == 1 {
        reference.len().max(candidate.len()).max(1)
    } else {
        factor * 2
    };
    let fine = align_near(
        reference,
        candidate,
        center,
        AlignConfig::new(radius.min(MAX_ALIGN_LAG), overlap_floor).ok()?,
    )?;
    Some(Alignment {
        rival: fine.rival.max(coarse_rival),
        ..fine
    })
}

/// Max-pool `values` by `factor`, so a transient survives the reduction.
fn pooled(values: &[f32], factor: usize) -> Vec<f32> {
    values
        .chunks(factor)
        .map(|chunk| chunk.iter().copied().fold(f32::MIN, f32::max))
        .collect()
}

/// Normalised cross-correlation of two equal-length windows, or `None` when
/// either window is flat and has nothing to correlate.
fn correlation(a: &[f32], b: &[f32]) -> Option<f32> {
    debug_assert_eq!(a.len(), b.len());
    let n = a.len() as f32;
    let mean_a = a.iter().sum::<f32>() / n;
    let mean_b = b.iter().sum::<f32>() / n;
    let mut covariance = 0.0f32;
    let mut variance_a = 0.0f32;
    let mut variance_b = 0.0f32;
    for (left, right) in a.iter().zip(b) {
        let da = left - mean_a;
        let db = right - mean_b;
        covariance += da * db;
        variance_a += da * da;
        variance_b += db * db;
    }
    if variance_a <= f32::EPSILON || variance_b <= f32::EPSILON {
        return None;
    }
    Some(covariance / (variance_a * variance_b).sqrt())
}

#[cfg(test)]
#[path = "align_tests.rs"]
mod tests;
