//! How a command fails, and what the shell sees when it does.
//!
//! Two kinds, because a script has to tell them apart: a wrong command line is
//! the caller's mistake (exit 2, print the command's synopsis), and a failed command is
//! the machine's (exit 1, print the reason). Collapsing them into one code makes
//! a typo indistinguishable from a missing codec.

use crate::args::ArgsError;

#[derive(Debug, PartialEq)]
pub(crate) enum Failure {
    /// The command line was wrong.
    Usage(String),
    /// The command ran and could not finish.
    Failed(String),
    /// A specific non-zero status: a child process's, or 128 plus the number
    /// of a signal that stopped the command.
    Exit { code: i32, message: String },
}

impl Failure {
    /// The process exit code for this failure.
    pub(crate) fn code(&self) -> i32 {
        match self {
            Failure::Usage(_) => 2,
            Failure::Failed(_) => 1,
            Failure::Exit { code, .. } => *code,
        }
    }

    /// The one-line reason, as printed on stderr.
    pub(crate) fn message(&self) -> &str {
        match self {
            Failure::Usage(message) | Failure::Failed(message) | Failure::Exit { message, .. } => {
                message
            }
        }
    }

    /// A failed read or write of `path`: the path, then the reason as
    /// [`aria_models::io_reason`] states it, so "Permission denied (os error
    /// 13)" reads as "<path>: permission denied".
    pub(crate) fn io(path: &std::path::Path, err: &std::io::Error) -> Self {
        Failure::Failed(format!(
            "{}: {}",
            path.display(),
            aria_models::io_reason(err)
        ))
    }

    /// A media path that is missing, a folder, or one of Aria's own files,
    /// stated before a decode opens it: the decoder's own wording names
    /// GStreamer, and reads an Aria file as an unknown text file. `None` when
    /// the path is any other file.
    pub(crate) fn not_media(path: &std::path::Path) -> Option<Self> {
        let why = match std::fs::metadata(path) {
            Err(_) => "no such file".to_owned(),
            Ok(meta) if meta.is_dir() => "is a directory; name a media file".to_owned(),
            Ok(_) => {
                let ext = path.extension()?.to_str()?;
                let (_, kind) = ARIA_FILES
                    .iter()
                    .find(|(aria, _)| ext.eq_ignore_ascii_case(aria))?;
                format!("is {kind}; name a media file")
            }
        };
        Some(Failure::Failed(format!("{}: {why}", path.display())))
    }
}

/// The extension of each file kind Aria writes, and what a media argument
/// that names one is told it named.
pub(crate) const ARIA_FILES: [(&str, &str); 5] = [
    (aria_models::PROJECT_FILE_EXTENSION, "an Aria project"),
    (aria_models::TEMPLATE_FILE_EXTENSION, "an Aria template"),
    (
        aria_models::template_bundle::BUNDLE_FILE_EXTENSION,
        "an Aria template bundle",
    ),
    (aria_models::PRESET_FILE_EXTENSION, "an Aria preset"),
    (
        aria_plugin::package::ARCHIVE_EXTENSION,
        "an Aria plugin archive",
    ),
];
impl From<ArgsError> for Failure {
    fn from(err: ArgsError) -> Self {
        match err {
            ArgsError::Usage(message) => Failure::Usage(message),
            // `--help` and a bare invocation are handled before a command runs;
            // reaching here at all is a parser mistake, and it is reported as
            // one rather than becoming a silent success.
            ArgsError::Help(_) => Failure::Usage("--help is not a command".into()),
            ArgsError::Version => Failure::Usage("--version is not a command".into()),
            ArgsError::NoVerb => Failure::Usage("no command given".into()),
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::Failed(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The class closed here is "every failure exits 1", which makes a typo
    /// indistinguishable from a real failure in a script.
    #[test]
    fn a_usage_mistake_and_a_failed_run_exit_differently() {
        assert_eq!(Failure::from(ArgsError::Usage("bad".into())).code(), 2);
        assert_eq!(Failure::from("device lost".to_string()).code(), 1);
        assert_eq!(
            Failure::from("device lost".to_string()).message(),
            "device lost"
        );
    }

    /// WHY: `aria analyze cut.aria` and `aria new out.aria cut.aria` failed
    /// with "decode failed: GStreamer: This appears to be a text file. Pick
    /// another file.", which does not state that the path was a project rather
    /// than media. Each file kind Aria writes is stated by kind, in any letter
    /// case, and any other file reaches the decoder. Not caught: an Aria file
    /// renamed to a media extension.
    #[test]
    fn an_aria_file_named_as_media_is_stated_by_its_kind() {
        let dir = tempfile::tempdir().expect("scratch directory");
        for (ext, kind) in ARIA_FILES {
            for spelled in [ext.to_owned(), ext.to_uppercase()] {
                let path = dir.path().join(format!("cut.{spelled}"));
                std::fs::write(&path, b"{}").expect("write file");
                let want = format!("{}: is {kind}; name a media file", path.display());
                assert_eq!(Failure::not_media(&path), Some(Failure::Failed(want)));
            }
        }
        for media in ["cut.wav", "cut.aria.mp4", "cut"] {
            let path = dir.path().join(media);
            std::fs::write(&path, b"RIFF").expect("write file");
            assert_eq!(Failure::not_media(&path), None, "{media}");
        }
    }
}
