//! The folder sync that makes a committed save survive a crash.

use super::*;

/// WHY: a save is on disk only once the folder that records its rename is.
/// Without the folder sync, a crash shortly after a reported save brings back
/// the replaced project, or no file at all for a first save. Held for the
/// atomic rename, the fallback swap through a backup, and a first save.
///
/// Not covered: a crash itself, which no test here can stage, so a folder sync
/// that writes nothing is not seen; the call, its folder and its place after
/// the install are what is checked.
#[test]
fn a_committed_save_syncs_its_folder_after_the_install() {
    for (existing, failed_renames, renames) in
        [(true, vec![], 1), (true, vec![1], 3), (false, vec![], 1)]
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("durable.aria");
        if existing {
            std::fs::write(&path, b"old project").unwrap();
        }
        let fs = FaultFs {
            failed_renames,
            ..FaultFs::default()
        };

        persist_serialized(&path, b"new project", &fs).unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"new project");
        assert_eq!(fs.rename_calls.get(), renames);
        let operations = fs.operations.borrow();
        assert_eq!(operations.last(), Some(&"sync_dir"), "{operations:?}");
        assert_eq!(
            operations.iter().filter(|op| **op == "sync_dir").count(),
            1,
            "{operations:?}"
        );
        assert_eq!(*fs.synced_dirs.borrow(), [dir.path().to_path_buf()]);
        assert_no_transaction_artifacts(dir.path());
    }
}

/// A folder that cannot be synced fails the save with that reason, though the
/// new project is already in place: the save is not known to be on disk.
#[test]
fn a_folder_that_cannot_be_synced_fails_the_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unsynced.aria");
    std::fs::write(&path, b"old project").unwrap();
    let fs = FaultFs {
        fail_sync_dir: true,
        ..FaultFs::default()
    };

    let error = persist_serialized(&path, b"new project", &fs).unwrap_err();

    assert_eq!(
        error.to_string(),
        "could not sync the project's folder: injected folder sync failure #1"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"new project");
    assert_no_transaction_artifacts(dir.path());
}
