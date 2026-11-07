use std::os::unix::fs::PermissionsExt;

use super::*;

/// WHY: a new project was written mode 0600 while every other file Aria
/// creates takes the umask default, so a project saved to a shared folder was
/// unreadable to the group that reads its exports. A new project gets the mode
/// of any new file in its folder, and a replaced one keeps its own mode; the
/// save leaves no temporary or backup beside them. Not caught under a umask of
/// 077, where both modes are 0600.
#[test]
fn save_gives_a_new_project_the_umask_default_and_keeps_an_existing_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mode = |name: &str| {
        let meta = std::fs::metadata(dir.path().join(name)).unwrap();
        meta.permissions().mode() & 0o777
    };
    std::fs::File::create(dir.path().join("reference")).unwrap();
    let existing = dir.path().join("existing.aria");
    std::fs::write(&existing, b"old project").unwrap();
    std::fs::set_permissions(&existing, Permissions::from_mode(0o640)).unwrap();

    for name in ["new.aria", "existing.aria"] {
        let project = Project::new("permissions", crate::time::Rational::FPS_24);
        project.save_to_file(&dir.path().join(name)).unwrap();
    }

    assert_eq!(mode("new.aria"), mode("reference"));
    assert_eq!(mode("existing.aria"), 0o640);
    let mut names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["existing.aria", "new.aria", "reference"]);
}
