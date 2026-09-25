//! The three names a stored moment is addressed by: its kind, the analyzer that
//! produced it, and that analyzer's result version.
//!
//! All three are storage-stable, which is why the character sets are closed and
//! validated on construction rather than on the way to the database. A kind or
//! an identity that reached a row unvalidated would be a value no later build
//! could parse, and a version exists so a payload change is a new scope instead
//! of a silent reinterpretation of old rows.

use super::*;

/// A validated, storage-stable moment kind.
///
/// Names use lowercase ASCII identifiers: the first byte is `a-z`, and later
/// bytes may additionally be digits, `.`, `_`, or `-`. Dots allow custom
/// analyzers to namespace kinds without expanding a closed enum. Built-in
/// values are stored as borrowed static strings; custom values are owned.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MomentKind(Cow<'static, str>);

impl MomentKind {
    /// Stable storage name for a detected shot boundary.
    pub const SHOT_BOUNDARY: Self = Self(Cow::Borrowed("shot_boundary"));

    /// Stable storage name for a silent span.
    pub const SILENCE: Self = Self(Cow::Borrowed("silence"));

    /// Stable storage name for a beat or onset point.
    pub const BEAT: Self = Self(Cow::Borrowed("beat"));

    /// Stable storage name for a transcript segment.
    pub const TRANSCRIPT_SEGMENT: Self = Self(Cow::Borrowed("transcript_segment"));

    /// Stable storage name for a transcript word.
    pub const TRANSCRIPT_WORD: Self = Self(Cow::Borrowed("transcript_word"));

    /// Stable storage name for a semantically detected visual moment.
    pub const VISION_MOMENT: Self = Self(Cow::Borrowed("vision_moment"));

    /// Validates a built-in or custom storage name.
    ///
    /// Known built-in names reuse static storage. Other valid names are copied
    /// into an owned string.
    ///
    /// # Errors
    ///
    /// Returns [`MomentKindError`] when `name` is empty, too long, or outside
    /// the documented lowercase ASCII grammar.
    pub fn new(name: &str) -> Result<Self, MomentKindError> {
        validate_moment_kind(name)?;
        Ok(match name {
            "shot_boundary" => Self::SHOT_BOUNDARY,
            "silence" => Self::SILENCE,
            "beat" => Self::BEAT,
            "transcript_segment" => Self::TRANSCRIPT_SEGMENT,
            "transcript_word" => Self::TRANSCRIPT_WORD,
            "vision_moment" => Self::VISION_MOMENT,
            _ => Self(Cow::Owned(name.to_owned())),
        })
    }

    /// Returns the stable name used by storage and query adapters.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }

    /// Returns whether this value is one of the built-in kinds.
    #[must_use]
    pub fn is_builtin(&self) -> bool {
        matches!(
            self.as_str(),
            "shot_boundary"
                | "silence"
                | "beat"
                | "transcript_segment"
                | "transcript_word"
                | "vision_moment"
        )
    }
}

impl FromStr for MomentKind {
    type Err = MomentKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<&str> for MomentKind {
    type Error = MomentKindError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl AsRef<str> for MomentKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for MomentKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Validation errors for a [`MomentKind`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MomentKindError {
    /// The name was empty.
    Empty,
    /// The UTF-8 representation exceeded [`MAX_MOMENT_KIND_BYTES`].
    TooLong {
        /// Actual UTF-8 byte length.
        actual: usize,
    },
    /// The first byte was not a lowercase ASCII letter.
    InvalidStart {
        /// Invalid first byte.
        byte: u8,
    },
    /// A later byte was outside the accepted identifier grammar.
    InvalidCharacter {
        /// Zero-based byte position in the name.
        index: usize,
        /// Invalid byte found at `index`.
        byte: u8,
    },
}

impl fmt::Display for MomentKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("moment kind must not be empty"),
            Self::TooLong { actual } => write!(
                formatter,
                "moment kind has {actual} bytes; maximum is {MAX_MOMENT_KIND_BYTES}"
            ),
            Self::InvalidStart { byte } => write!(
                formatter,
                "moment kind must start with a lowercase ASCII letter, found 0x{byte:02x}"
            ),
            Self::InvalidCharacter { index, byte } => write!(
                formatter,
                "moment kind contains invalid byte 0x{byte:02x} at byte index {index}"
            ),
        }
    }
}

impl Error for MomentKindError {}

fn validate_moment_kind(name: &str) -> Result<(), MomentKindError> {
    let bytes = name.as_bytes();
    let Some((&first, remaining)) = bytes.split_first() else {
        return Err(MomentKindError::Empty);
    };
    if bytes.len() > MAX_MOMENT_KIND_BYTES {
        return Err(MomentKindError::TooLong {
            actual: bytes.len(),
        });
    }
    if !first.is_ascii_lowercase() {
        return Err(MomentKindError::InvalidStart { byte: first });
    }

    for (offset, byte) in remaining.iter().copied().enumerate() {
        if !(byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(MomentKindError::InvalidCharacter {
                index: offset + 1,
                byte,
            });
        }
    }
    Ok(())
}

/// A validated, stable analyzer or source identity.
///
/// Identities use lowercase ASCII. The first byte is a letter or digit; later
/// bytes may additionally be `.`, `_`, `-`, or `/`. This permits namespaced
/// custom analyzers while keeping database and tool inputs bounded and
/// unambiguous.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnalyzerIdentity(String);

impl AnalyzerIdentity {
    /// Validates and copies an analyzer identity.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzerIdentityError`] when `identity` is empty, too long,
    /// or outside the documented lowercase ASCII grammar.
    pub fn new(identity: &str) -> Result<Self, AnalyzerIdentityError> {
        validate_analyzer_identity(identity)?;
        Ok(Self(identity.to_owned()))
    }

    /// Returns the stable storage identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for AnalyzerIdentity {
    type Err = AnalyzerIdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<&str> for AnalyzerIdentity {
    type Error = AnalyzerIdentityError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl AsRef<str> for AnalyzerIdentity {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for AnalyzerIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Validation errors for an [`AnalyzerIdentity`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalyzerIdentityError {
    /// The identity was empty.
    Empty,
    /// The UTF-8 representation exceeded [`MAX_ANALYZER_IDENTITY_BYTES`].
    TooLong {
        /// Actual UTF-8 byte length.
        actual: usize,
    },
    /// The first byte was not a lowercase ASCII letter or digit.
    InvalidStart {
        /// Invalid first byte.
        byte: u8,
    },
    /// A later byte was outside the accepted identifier grammar.
    InvalidCharacter {
        /// Zero-based byte position in the identity.
        index: usize,
        /// Invalid byte found at `index`.
        byte: u8,
    },
}

impl fmt::Display for AnalyzerIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("analyzer identity must not be empty"),
            Self::TooLong { actual } => write!(
                formatter,
                "analyzer identity has {actual} bytes; maximum is \
                 {MAX_ANALYZER_IDENTITY_BYTES}"
            ),
            Self::InvalidStart { byte } => write!(
                formatter,
                "analyzer identity must start with a lowercase ASCII letter or digit, found \
                 0x{byte:02x}"
            ),
            Self::InvalidCharacter { index, byte } => write!(
                formatter,
                "analyzer identity contains invalid byte 0x{byte:02x} at byte index {index}"
            ),
        }
    }
}

impl Error for AnalyzerIdentityError {}

fn validate_analyzer_identity(identity: &str) -> Result<(), AnalyzerIdentityError> {
    let bytes = identity.as_bytes();
    let Some((&first, remaining)) = bytes.split_first() else {
        return Err(AnalyzerIdentityError::Empty);
    };
    if bytes.len() > MAX_ANALYZER_IDENTITY_BYTES {
        return Err(AnalyzerIdentityError::TooLong {
            actual: bytes.len(),
        });
    }
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(AnalyzerIdentityError::InvalidStart { byte: first });
    }

    for (offset, byte) in remaining.iter().copied().enumerate() {
        if !(byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'.' | b'_' | b'-' | b'/'))
        {
            return Err(AnalyzerIdentityError::InvalidCharacter {
                index: offset + 1,
                byte,
            });
        }
    }
    Ok(())
}

/// A non-zero analyzer result schema/version.
///
/// Adapters can store this value as a positive 32-bit integer. An analyzer
/// must change the version whenever persisted payload or result semantics
/// change incompatibly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnalyzerVersion(NonZeroU32);

impl AnalyzerVersion {
    /// Constructs a positive analyzer version.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzerVersionError::Zero`] when `value` is zero.
    pub fn new(value: u32) -> Result<Self, AnalyzerVersionError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(AnalyzerVersionError::Zero)
    }

    /// Returns the positive integer version.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl fmt::Display for AnalyzerVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(formatter)
    }
}

/// Validation errors for an [`AnalyzerVersion`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalyzerVersionError {
    /// Analyzer versions must not be zero.
    Zero,
}

impl fmt::Display for AnalyzerVersionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("analyzer version must be non-zero"),
        }
    }
}

impl Error for AnalyzerVersionError {}
