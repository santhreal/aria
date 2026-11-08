//! The scratch protocol: a held claim survives every sweep, a dead one is
//! cleared once, and nothing outside the scratch names is touched.
//!
//! WHY: a long write (an export, a plugin update, a model download, a cache
//! move) fills a scratch path beside its destination, and a writer killed part
//! way left that path, unfinished data included, where nothing deleted it.
//! These tests hold the protocol every such writer shares; each writer's own
//! tests check that it claims through [`ScratchLock`] and sweeps its folder.
//! Not caught: a live writer from a build that made no lock file, whose
//! scratch path reads as dead.

use super::*;

/// Scratch names in these tests: `.w-` and digits.
fn is_scratch(name: &str) -> bool {
    name.strip_prefix(".w-")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The names in `dir`, sorted, hidden ones included.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// Everything `sweep` handed to `clear`, by name, sorted.
fn swept(dir: &Path) -> Vec<String> {
    let mut cleared = Vec::new();
    sweep(dir, is_scratch, |path| {
        cleared.push(
            path.file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned(),
        );
        remove(path).expect("clear");
    });
    cleared.sort();
    cleared
}

#[test]
fn a_held_claim_survives_and_a_dead_one_is_cleared_once() {
    let root = tempfile::tempdir().expect("scratch");
    let dir = root.path();
    let live = ScratchLock::claim(&dir.join(".w-1"))
        .expect("claim")
        .expect("free name");
    fs::create_dir(dir.join(".w-1")).expect("mkdir");

    // Dead: a folder beside an unheld lock file, a file with no lock file (its
    // writer made one first, so none means dead), and a lock file alone.
    fs::create_dir(dir.join(".w-2")).expect("mkdir");
    fs::write(dir.join(".w-2/part"), b"unfinished").expect("write");
    File::create(lock_path(&dir.join(".w-2"))).expect("lock file");
    fs::write(dir.join(".w-3"), b"unfinished").expect("write");
    File::create(lock_path(&dir.join(".w-4"))).expect("lock file");
    // Not scratch names.
    fs::create_dir(dir.join(".w-notes")).expect("mkdir");
    fs::write(dir.join("keep.txt"), b"unrelated").expect("write");
    fs::write(dir.join("keep.txt.lock"), b"unrelated").expect("write");

    assert_eq!(swept(dir), vec![".w-2", ".w-3"]);
    assert_eq!(
        entries(dir),
        vec![".w-1", ".w-1.lock", ".w-notes", "keep.txt", "keep.txt.lock"]
    );
    assert_eq!(
        swept(dir),
        Vec::<String>::new(),
        "a cleared path is not cleared again"
    );

    fs::remove_dir(dir.join(".w-1")).expect("the writer deletes its path");
    drop(live);
    assert_eq!(entries(dir), vec![".w-notes", "keep.txt", "keep.txt.lock"]);
}

#[test]
fn a_held_name_cannot_be_claimed_and_a_dead_one_can() {
    let root = tempfile::tempdir().expect("scratch");
    let path = root.path().join(".w-5");
    let first = ScratchLock::claim(&path)
        .expect("claim")
        .expect("free name");
    assert!(
        ScratchLock::claim(&path).expect("claim").is_none(),
        "a held name is taken"
    );
    assert_eq!(first.lock_path(), Some(lock_path(&path).as_path()));
    drop(first);
    assert!(!lock_path(&path).exists(), "the drop unlinks the lock file");

    // A lock file a dead writer left is claimed again.
    File::create(lock_path(&path)).expect("lock file");
    let again = ScratchLock::claim(&path).expect("claim");
    assert!(again.is_some(), "a dead writer's name is free");
}

#[test]
fn remove_deletes_a_folder_or_a_file_and_accepts_nothing() {
    let root = tempfile::tempdir().expect("scratch");
    let folder = root.path().join("folder");
    fs::create_dir_all(folder.join("inner")).expect("mkdir");
    fs::write(folder.join("inner/file"), b"x").expect("write");
    let file = root.path().join("file");
    fs::write(&file, b"x").expect("write");
    remove(&folder).expect("folder");
    remove(&file).expect("file");
    remove(&root.path().join("absent")).expect("nothing");
    assert!(entries(root.path()).is_empty());
}
