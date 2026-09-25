//! What a moment carries besides its span: how sure the analyzer is, and the
//! bounded blob it attached.
//!
//! Confidence is a finite `[0, 1]` float with a total order, so results sort
//! deterministically even though `f32` does not. A payload is small on purpose:
//! it rides along in every query result and every tool response, and an
//! analyzer artifact too large for that belongs outside the index.

use super::*;

/// A finite confidence value in the inclusive range `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MomentConfidence(f32);

impl MomentConfidence {
    /// Lowest possible confidence.
    pub const ZERO: Self = Self(0.0);

    /// Highest possible confidence.
    pub const CERTAIN: Self = Self(1.0);

    /// Constructs a validated confidence value.
    ///
    /// Negative zero is canonicalized to positive zero.
    ///
    /// # Errors
    ///
    /// Returns [`MomentConfidenceError::NonFinite`] for NaN or infinity and
    /// [`MomentConfidenceError::OutOfRange`] for finite values outside
    /// `[0, 1]`.
    pub fn new(value: f32) -> Result<Self, MomentConfidenceError> {
        if !value.is_finite() {
            return Err(MomentConfidenceError::NonFinite);
        }
        if !(0.0..=1.0).contains(&value) {
            return Err(MomentConfidenceError::OutOfRange);
        }
        Ok(Self(if value == 0.0 { 0.0 } else { value }))
    }

    /// Returns the finite value in `[0, 1]`.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Default for MomentConfidence {
    fn default() -> Self {
        Self::ZERO
    }
}

impl Eq for MomentConfidence {}

impl PartialOrd for MomentConfidence {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MomentConfidence {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl fmt::Display for MomentConfidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(formatter)
    }
}

/// Validation errors for [`MomentConfidence`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MomentConfidenceError {
    /// The supplied value was NaN or positive/negative infinity.
    NonFinite,
    /// The finite value was below zero or above one.
    OutOfRange,
}

impl fmt::Display for MomentConfidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => formatter.write_str("moment confidence must be finite"),
            Self::OutOfRange => {
                formatter.write_str("moment confidence must be in the inclusive range [0, 1]")
            }
        }
    }
}

impl Error for MomentConfidenceError {}

/// A bounded opaque text or metadata payload attached to a moment.
///
/// Payloads are valid UTF-8 (as guaranteed by [`String`]), non-empty, and at
/// most [`MAX_MOMENT_PAYLOAD_BYTES`] bytes. Tab, line-feed, and carriage-return
/// are accepted; other Unicode control characters are rejected. The contents
/// are deliberately not interpreted as JSON or any other format.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MomentPayload(String);

impl MomentPayload {
    /// Validates and copies a payload.
    ///
    /// # Errors
    ///
    /// Returns [`MomentPayloadError`] when `value` is empty, too long, or
    /// contains a disallowed control character.
    pub fn new(value: &str) -> Result<Self, MomentPayloadError> {
        validate_moment_payload(value)?;
        Ok(Self(value.to_owned()))
    }

    /// Returns the uninterpreted payload text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the payload's bounded UTF-8 byte length.
    #[must_use]
    pub fn len_bytes(&self) -> usize {
        self.0.len()
    }
}

impl FromStr for MomentPayload {
    type Err = MomentPayloadError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<&str> for MomentPayload {
    type Error = MomentPayloadError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl AsRef<str> for MomentPayload {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for MomentPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Validation errors for a [`MomentPayload`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MomentPayloadError {
    /// The payload was empty; use `None` when a moment has no payload.
    Empty,
    /// The UTF-8 representation exceeded [`MAX_MOMENT_PAYLOAD_BYTES`].
    TooLong {
        /// Actual UTF-8 byte length.
        actual: usize,
    },
    /// The payload contained a control character other than tab, LF, or CR.
    DisallowedControl {
        /// Zero-based UTF-8 byte position of the character.
        index: usize,
        /// Disallowed character found at `index`.
        character: char,
    },
}

impl fmt::Display for MomentPayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("moment payload must not be empty; use no payload"),
            Self::TooLong { actual } => write!(
                formatter,
                "moment payload has {actual} bytes; maximum is {MAX_MOMENT_PAYLOAD_BYTES}"
            ),
            Self::DisallowedControl { index, character } => write!(
                formatter,
                "moment payload contains disallowed control U+{:04X} at byte index {index}",
                u32::from(*character)
            ),
        }
    }
}

impl Error for MomentPayloadError {}

fn validate_moment_payload(value: &str) -> Result<(), MomentPayloadError> {
    if value.is_empty() {
        return Err(MomentPayloadError::Empty);
    }
    if value.len() > MAX_MOMENT_PAYLOAD_BYTES {
        return Err(MomentPayloadError::TooLong {
            actual: value.len(),
        });
    }

    for (index, character) in value.char_indices() {
        if character.is_control() && !matches!(character, '\t' | '\n' | '\r') {
            return Err(MomentPayloadError::DisallowedControl { index, character });
        }
    }
    Ok(())
}
