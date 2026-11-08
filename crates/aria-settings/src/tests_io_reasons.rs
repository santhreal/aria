//! A configuration file that cannot be read or written states the path and
//! the reason in the words `aria_core::io_reason` gives.
//!
//! WHY: loading, saving and replacing `config.toml` formatted the
//! `std::io::Error` itself, so a failure read "Is a directory (os error 21)"
//! or "File exists (os error 17)". Not covered: the install and rollback
//! steps, whose OS failures the fault-injecting file system here cannot
//! raise; the workspace census in `aria-ui` reads their format arguments.

use super::*;

#[test]
fn a_configuration_path_that_is_a_folder_states_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::create_dir(&path).unwrap();
    let want = format!("could not read {}: is a directory", path.display());

    assert_eq!(load(&path), Err(want.clone()), "load");
    assert_eq!(refused(save(&path, &Settings::default())), want, "save");
}

#[test]
fn a_configuration_folder_blocked_by_a_file_states_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("aria");
    std::fs::write(&blocker, b"not a folder").unwrap();
    let path = blocker.join("config.toml");

    assert_eq!(
        refused(save(&path, &Settings::default())),
        format!("could not read {}: not a directory", path.display()),
        "save"
    );
    assert_eq!(
        replace_file(&path, b"[appearance]\n"),
        Err(format!(
            "could not create {}: file exists",
            blocker.display()
        )),
        "replace"
    );
    assert_eq!(std::fs::read(&blocker).unwrap(), b"not a folder");
}
