//! The content key: 32 bytes a caller supplies, and its hexadecimal form.
//!
//! This crate hashes nothing and names no digest algorithm. The key is the
//! caller's promise that two identical byte strings mean the same media, so the
//! only rules here are the length, the lowercase hexadecimal encoding, and the
//! parse that refuses anything else by position.

use super::*;

const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";

/// A caller-supplied, fixed-size media content digest.
///
/// The key stores exactly 32 bytes inline and performs no allocation. This
/// crate does not choose a digest algorithm or hash media; callers must supply
/// the digest and use one algorithm consistently within an index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaContentKey([u8; MEDIA_CONTENT_KEY_BYTES]);

impl MediaContentKey {
    /// Constructs a key from an exactly sized byte array.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; MEDIA_CONTENT_KEY_BYTES]) -> Self {
        Self(bytes)
    }

    /// Copies a key from a byte slice of exactly 32 bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MediaContentKeyError::InvalidByteLength`] when `bytes` does
    /// not contain exactly [`MEDIA_CONTENT_KEY_BYTES`] bytes.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, MediaContentKeyError> {
        if bytes.len() != MEDIA_CONTENT_KEY_BYTES {
            return Err(MediaContentKeyError::InvalidByteLength {
                actual: bytes.len(),
            });
        }

        let mut key = [0_u8; MEDIA_CONTENT_KEY_BYTES];
        key.copy_from_slice(bytes);
        Ok(Self(key))
    }

    /// Returns the exact digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; MEDIA_CONTENT_KEY_BYTES] {
        &self.0
    }

    /// Consumes the key and returns the exact digest bytes.
    #[must_use]
    pub const fn into_bytes(self) -> [u8; MEDIA_CONTENT_KEY_BYTES] {
        self.0
    }

    /// Encodes the key as 64 lowercase hexadecimal ASCII bytes.
    ///
    /// The returned array is stack/inline data and requires no heap
    /// allocation. [`fmt::Display`] emits the same canonical representation.
    #[must_use]
    pub fn encode_hex(self) -> [u8; MEDIA_CONTENT_KEY_HEX_BYTES] {
        let mut encoded = [0_u8; MEDIA_CONTENT_KEY_HEX_BYTES];
        for (index, byte) in self.0.iter().copied().enumerate() {
            encoded[index * 2] = LOWER_HEX[usize::from(byte >> 4)];
            encoded[index * 2 + 1] = LOWER_HEX[usize::from(byte & 0x0f)];
        }
        encoded
    }
}

impl From<[u8; MEDIA_CONTENT_KEY_BYTES]> for MediaContentKey {
    fn from(bytes: [u8; MEDIA_CONTENT_KEY_BYTES]) -> Self {
        Self::from_bytes(bytes)
    }
}

impl TryFrom<&[u8]> for MediaContentKey {
    type Error = MediaContentKeyError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Self::from_slice(bytes)
    }
}

impl fmt::Display for MediaContentKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let encoded = self.encode_hex();
        let text = std::str::from_utf8(&encoded).map_err(|_| fmt::Error)?;
        formatter.write_str(text)
    }
}

impl FromStr for MediaContentKey {
    type Err = MediaContentKeyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let encoded = value.as_bytes();
        if encoded.len() != MEDIA_CONTENT_KEY_HEX_BYTES {
            return Err(MediaContentKeyError::InvalidHexLength {
                actual: encoded.len(),
            });
        }

        let mut bytes = [0_u8; MEDIA_CONTENT_KEY_BYTES];
        for (index, pair) in encoded.chunks_exact(2).enumerate() {
            let high = decode_hex_digit(pair[0], index * 2)?;
            let low = decode_hex_digit(pair[1], index * 2 + 1)?;
            bytes[index] = (high << 4) | low;
        }
        Ok(Self(bytes))
    }
}

/// Errors produced while constructing or parsing a [`MediaContentKey`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaContentKeyError {
    /// A binary digest did not contain exactly 32 bytes.
    InvalidByteLength {
        /// Actual number of supplied bytes.
        actual: usize,
    },
    /// A hexadecimal digest did not contain exactly 64 ASCII bytes.
    InvalidHexLength {
        /// Actual UTF-8 byte length of the supplied string.
        actual: usize,
    },
    /// A hexadecimal digest contained a byte outside `0-9`, `a-f`, and `A-F`.
    InvalidHexCharacter {
        /// Zero-based byte position in the supplied string.
        index: usize,
        /// Invalid byte found at `index`.
        byte: u8,
    },
}

impl fmt::Display for MediaContentKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidByteLength { actual } => write!(
                formatter,
                "media content key has {actual} bytes; expected {MEDIA_CONTENT_KEY_BYTES}"
            ),
            Self::InvalidHexLength { actual } => write!(
                formatter,
                "hex media content key has {actual} bytes; expected \
                 {MEDIA_CONTENT_KEY_HEX_BYTES}"
            ),
            Self::InvalidHexCharacter { index, byte } => write!(
                formatter,
                "invalid hexadecimal byte 0x{byte:02x} at byte index {index}"
            ),
        }
    }
}

impl Error for MediaContentKeyError {}

fn decode_hex_digit(byte: u8, index: usize) -> Result<u8, MediaContentKeyError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(MediaContentKeyError::InvalidHexCharacter { index, byte }),
    }
}
