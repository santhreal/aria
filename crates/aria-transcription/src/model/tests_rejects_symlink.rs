//! `rejects_symlink` cases.

use super::*;

#[cfg(unix)]
#[test]
fn rejects_symlink_model_files_without_touching_the_target() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().expect("temporary directory");
    let external = TempDir::new().expect("external directory");
    let external_file = external.path().join("external.bin");
    fs::write(&external_file, b"hello").expect("write external file");
    let manager = ModelManager::new(temp.path()).expect("valid manager");
    let model_path = manager.path_for_spec(&TINY_SPEC).expect("known path");
    symlink(&external_file, &model_path).expect("create symlink");
    let downloader = CountingDownloader::new(b"hello");

    assert!(matches!(
        manager.ensure_spec(&TINY_SPEC, &downloader),
        Err(ModelManagerError::UnsafeModelFile { .. })
    ));
    assert!(matches!(
        manager.remove_spec(&TINY_SPEC),
        Err(ModelManagerError::UnsafeModelFile { .. })
    ));
    assert_eq!(downloader.calls(), 0);
    assert_eq!(
        fs::read(&external_file).expect("external target survives"),
        b"hello"
    );
    assert!(model_path.is_symlink());
}

#[test]
fn remove_deletes_only_the_known_regular_file() {
    let temp = TempDir::new().expect("temporary directory");
    let manager = ModelManager::new(temp.path()).expect("valid manager");
    let model_path = manager.path_for_spec(&TINY_SPEC).expect("known path");
    let sibling = temp.path().join("keep.txt");
    fs::write(&model_path, b"hello").expect("write model");
    fs::write(&sibling, b"keep").expect("write sibling");

    assert!(manager.remove_spec(&TINY_SPEC).expect("remove model"));
    assert!(!model_path.exists());
    assert_eq!(fs::read(&sibling).expect("sibling remains"), b"keep");
    assert!(!manager.remove_spec(&TINY_SPEC).expect("already absent"));
}

#[cfg(windows)]
#[test]
fn windows_reparse_attribute_helper_rejects_all_reparse_points() {
    assert!(windows_attributes_are_reparse_point(
        WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT
    ));
    assert!(windows_attributes_are_reparse_point(
        WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT | 0x10
    ));
    assert!(!windows_attributes_are_reparse_point(0x10));
}
