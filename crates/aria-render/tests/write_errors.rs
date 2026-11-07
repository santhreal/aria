//! A file the render path cannot write is stated by its path and the reason.
//!
//! WHY: `RenderError::Io` printed "i/o error: " before the raw OS error, and
//! the errors under it named no path: `aria frame` into a path under a regular
//! file failed as "i/o error: Not a directory (os error 20)", and a PNG
//! sequence into one as "failed to start encoder: Not a directory (os error
//! 20)". Each case here drives a public write entry point without a GPU and
//! asserts the whole message. Not caught: a failure while publishing a
//! finished file, such as a full disk, which needs a filesystem that fails on
//! demand.

use std::path::{Path, PathBuf};

use aria_core::RgbaImage;
use aria_render::{PngSequenceEncoder, RenderError, write_png};

/// A scratch folder holding `file.txt`, a regular file a path can run under.
fn folder_with_a_file() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("scratch dir");
    let file = dir.path().join("file.txt");
    std::fs::write(&file, b"not a folder").expect("a regular file");
    (dir, file)
}

fn png_error(path: &Path) -> String {
    let image = RgbaImage::new(2, 2, vec![255; 16]);
    let error = write_png(&image, path).expect_err("the write must fail");
    assert!(
        matches!(error, RenderError::Io(_)),
        "expected an I/O error, got {error:?}"
    );
    error.to_string()
}

#[test]
fn a_frame_under_a_regular_file_is_stated_by_its_path() {
    let (_dir, file) = folder_with_a_file();
    let path = file.join("still.png");
    assert_eq!(
        png_error(&path),
        format!("{}: not a directory", path.display())
    );
}

#[test]
fn a_frame_over_a_folder_is_stated_without_an_io_prefix() {
    let (dir, _file) = folder_with_a_file();
    let path = dir.path().join("frames");
    std::fs::create_dir(&path).expect("a folder in the way");
    assert_eq!(
        png_error(&path),
        format!(
            "{} is a folder, not a regular file; export to a file name of its own",
            path.display()
        )
    );
}

#[cfg(unix)]
#[test]
fn a_frame_in_a_folder_that_cannot_be_written_states_the_folder() {
    use std::os::unix::fs::PermissionsExt;

    let (dir, _file) = folder_with_a_file();
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).expect("a folder");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).expect("lock");
    // A process that may write anywhere (root) cannot see the refusal.
    let writable = std::fs::write(locked.join("probe"), b"").is_ok();
    let message = (!writable).then(|| png_error(&locked.join("still.png")));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("unlock");
    if let Some(message) = message {
        assert_eq!(
            message,
            format!(
                "cannot write in \"{}\": permission denied",
                locked.display()
            )
        );
    }
}

#[test]
fn a_sequence_folder_under_a_regular_file_is_stated_by_its_path() {
    let (_dir, file) = folder_with_a_file();
    let folder = file.join("frames");
    let error = PngSequenceEncoder::new(&folder).expect_err("no folder under a file");
    assert_eq!(
        RenderError::from(error).to_string(),
        format!(
            "encode error: failed to start encoder: {}: not a directory",
            folder.display()
        )
    );
}
