//! Regression tests for `aria plugin dev` editor exit signals and `--editor`
//! candidate path diagnostics.

use std::fs;

use aria_plugin::package::MANIFEST_FILE;

use super::{exit_for_editor_status, resolve_editor_binary, run_dev};
use crate::fail::Failure;

/// WHY: On Unix, when the editor spawned by `aria plugin dev` crashed on a
/// signal (such as SIGABRT = 6 or SIGSEGV = 11 from a faulty native plugin),
/// `status.code()` returned `None` and `exit_for_editor_status` fell back to
/// `unwrap_or(1)`, reporting `editor exited with status 1` with exit code 1
/// instead of `128 + sig`. Passing a directory or broken symbolic link to
/// `--editor` (or having one at sibling `aria-desktop`) also reported
/// `editor binary not found`, while valid symlinks to executables must keep
/// resolving.
/// Not caught: Windows NTSTATUS crash code formatting.
#[test]
fn dev_reports_signal_exit_and_validates_editor_candidate_kind() {
    let scratch = tempfile::tempdir().expect("tempdir");
    let root = scratch.path();

    let editor_dir = root.join("not-a-binary");
    fs::create_dir_all(&editor_dir).expect("mkdir");
    let err = resolve_editor_binary(Some(&editor_dir), None).expect_err("directory --editor");
    assert_eq!(
        err,
        Failure::Failed(format!(
            "{}: editor path is a directory, not an executable file",
            editor_dir.display()
        ))
    );

    let bin_dir = root.join("bin");
    let sib_dir = bin_dir.join(format!("aria-desktop{}", std::env::consts::EXE_SUFFIX));
    fs::create_dir_all(&sib_dir).expect("mkdir sibling aria-desktop");
    let sib_err =
        resolve_editor_binary(None, Some(&bin_dir.join("aria"))).expect_err("sibling dir");
    assert_eq!(
        sib_err,
        Failure::Failed(format!(
            "{}: editor path is a directory, not an executable file",
            sib_dir.display()
        ))
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        use std::os::unix::process::ExitStatusExt;
        use std::process::ExitStatus;

        let dangling = root.join("dangling-editor");
        symlink(root.join("missing-target"), &dangling).expect("dangling symlink");
        let dang_err = resolve_editor_binary(Some(&dangling), None).expect_err("dangling");
        assert_eq!(
            dang_err,
            Failure::Failed(format!(
                "{}: editor path is a broken symbolic link",
                dangling.display()
            ))
        );

        let real_bin = root.join("real-editor.sh");
        fs::write(&real_bin, "#!/bin/sh\nexit 0\n").expect("write real");
        fs::set_permissions(&real_bin, fs::Permissions::from_mode(0o755)).expect("chmod");
        let valid_link = root.join("linked-editor");
        symlink(&real_bin, &valid_link).expect("valid symlink");
        assert_eq!(
            resolve_editor_binary(Some(&valid_link), None).expect("valid symlink resolves"),
            valid_link
        );

        // A wait status of 6 is a child that SIGABRT ended.
        let sig_status = ExitStatus::from_raw(6);
        let sig_err = exit_for_editor_status(sig_status).expect_err("signal exit must fail");
        assert_eq!(sig_err.code(), 128 + 6);
        assert_eq!(sig_err.message(), "editor terminated by signal 6");

        let data_pkg = root.join("com.example.dev-sig");
        fs::create_dir_all(&data_pkg).expect("data_pkg");
        fs::write(
            data_pkg.join(MANIFEST_FILE),
            "[plugin]\nid = \"com.example.dev-sig\"\nname = \"Dev Sig\"\nversion = \"0.1.0\"\napi = 1\n",
        )
        .expect("manifest");
        let crash_editor = root.join("crash-editor.sh");
        fs::write(&crash_editor, "#!/bin/sh\nkill -SEGV $$\n").expect("script");
        fs::set_permissions(&crash_editor, fs::Permissions::from_mode(0o755)).expect("chmod");
        let dev_err =
            run_dev(&data_pkg, Some(&crash_editor), None).expect_err("crashing editor fails");
        assert_eq!(dev_err.code(), 128 + 11);
        assert_eq!(dev_err.message(), "editor terminated by signal 11");
    }
}
