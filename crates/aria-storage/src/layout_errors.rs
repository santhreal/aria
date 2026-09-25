//! The two error types a shared-layout write can produce.
//!
//! A pure move out of `layout.rs` to stay under the file ceiling. Neither type
//! carries a path: a rejected write reports generation numbers only, so an
//! error that reaches a log or a support report cannot leak a storage location.

use super::*;

/// A rejected [`SharedStorageLayout`] write.
///
/// This error contains only generation numbers and never retains or displays
/// storage paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedStorageLayoutError {
    /// Another writer replaced the layout after the caller took its snapshot.
    StaleGeneration {
        /// Generation supplied by the caller.
        expected: u64,
        /// Generation currently installed.
        current: u64,
    },
    /// The current generation is `u64::MAX` and cannot advance safely.
    GenerationExhausted,
}

impl fmt::Display for SharedStorageLayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleGeneration { expected, current } => write!(
                f,
                "stale storage layout generation: expected {expected}, current {current}"
            ),
            Self::GenerationExhausted => f.write_str("storage layout generation cannot advance"),
        }
    }
}

impl Error for SharedStorageLayoutError {}

/// A rejected exclusive [`SharedStorageLayout::transition`].
#[derive(Debug)]
pub enum SharedStorageLayoutTransitionError<E> {
    /// The expected generation was stale or exhausted.
    Layout(SharedStorageLayoutError),
    /// The caller's transition failed before publication.
    Transition(E),
}

impl<E: fmt::Display> fmt::Display for SharedStorageLayoutTransitionError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(error) => error.fmt(f),
            Self::Transition(error) => write!(f, "storage layout transition failed: {error}"),
        }
    }
}

impl<E> Error for SharedStorageLayoutTransitionError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Layout(error) => Some(error),
            Self::Transition(error) => Some(error),
        }
    }
}
