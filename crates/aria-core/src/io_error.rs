//! The words for a [`std::io::Error`] in a message: the reason without the
//! code the OS numbers it by, and the path or directory it is about.
//!
//! Every crate that states a file or process failure words it here, so no
//! message reads "Permission denied (os error 13)".

use std::io;
use std::path::Path;

/// The reason an I/O error states, as a clause that follows a path: an OS
/// error reads in lower case, without the code the OS numbers it by and
/// without a closing period, so "Permission denied (os error 13)" reads as
/// "permission denied" and "Is a directory (os error 21)" as "is a
/// directory". An error built with a message of its own reads as that
/// message.
pub fn io_reason(err: &io::Error) -> String {
    if err.raw_os_error().is_none() {
        return err.to_string();
    }
    if err.kind() == io::ErrorKind::PermissionDenied {
        return "permission denied".to_owned();
    }
    let text = err.to_string();
    let reason = text
        .split_once(" (os error ")
        .map_or(text.as_str(), |(reason, _)| reason)
        .trim_end_matches('.');
    let mut chars = reason.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if first.is_uppercase() && !second.is_uppercase() => first
            .to_lowercase()
            .chain(reason[first.len_utf8()..].chars())
            .collect(),
        _ => reason.to_owned(),
    }
}

/// An error writing a new file in `dir` that states the directory: creating
/// a file reports only the OS error, so a folder that cannot be written read
/// as "Permission denied (os error 13)" with no path. The kind is kept.
pub fn unwritable_directory(dir: &Path, err: &io::Error) -> io::Error {
    io::Error::new(
        err.kind(),
        format!("cannot write in \"{}\": {}", dir.display(), io_reason(err)),
    )
}

/// `err` as a failure of `path`: the path, then the reason as [`io_reason`]
/// states it, so "No such file or directory (os error 2)" reads as "<path>: no
/// such file or directory". The kind is kept.
pub fn path_error(path: &Path, err: &io::Error) -> io::Error {
    io::Error::new(
        err.kind(),
        format!("{}: {}", path.display(), io_reason(err)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY: a folder that cannot be written failed `aria new`, `aria frame`,
    /// `aria export` and a save as "Permission denied (os error 13)", naming
    /// no path. An error writing in a directory states the directory and the
    /// reason without its OS code, keeps its kind, and an error that already
    /// has a message keeps the message rather than the bare kind.
    #[test]
    fn an_unwritable_directory_is_stated_with_its_reason() {
        let dir = Path::new("renders/out");
        let mut cases = vec![
            (
                io::Error::from(io::ErrorKind::PermissionDenied),
                "permission denied".to_owned(),
            ),
            (
                io::Error::new(io::ErrorKind::PermissionDenied, "read-only share"),
                "read-only share".to_owned(),
            ),
        ];
        #[cfg(unix)]
        cases.extend([
            (io::Error::from_raw_os_error(13), "permission denied".into()),
            (io::Error::from_raw_os_error(5), "input/output error".into()),
            (io::Error::from_raw_os_error(21), "is a directory".into()),
            (
                io::Error::from_raw_os_error(30),
                "read-only file system".into(),
            ),
        ]);
        for (err, reason) in cases {
            assert_eq!(io_reason(&err), reason, "{err:?}");
            let stated = unwritable_directory(dir, &err);
            assert_eq!(stated.kind(), err.kind());
            assert_eq!(
                stated.to_string(),
                format!("cannot write in \"renders/out\": {reason}")
            );
            let failed = path_error(Path::new("cut.aria"), &err);
            assert_eq!(failed.kind(), err.kind());
            assert_eq!(failed.to_string(), format!("cut.aria: {reason}"));
        }
    }
}
