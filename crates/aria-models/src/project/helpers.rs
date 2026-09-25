//! Project-level helpers that belong to no single domain module.

use std::path::Path;

/// Whether two paths name the same file on disk. Falls back to a literal
/// comparison when either path cannot be canonicalized (a media file that has
/// since moved still matches itself).
pub(super) fn paths_refer_to_same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}
