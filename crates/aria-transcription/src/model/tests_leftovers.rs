//! Scratch files a killed download left.

use aria_core::scratch::{self, ScratchLock};

use super::*;

/// WHY: a model download killed part way (SIGKILL, a crash, a power cut)
/// left its hidden `.download-…tmp` file, up to the model's full size, beside
/// the model for good. The next download in that folder deletes every
/// scratch file no live download holds, with its lock file, and leaves a live
/// one and every other entry alone. Not caught: a live download from a build
/// that made no lock file, whose scratch file reads as dead.
#[test]
fn the_next_download_deletes_what_a_killed_download_left() {
    let temp = TempDir::new().expect("temporary directory");
    let manager = ModelManager::new(temp.path()).expect("valid manager");
    let path = manager.path_for_spec(&TINY_SPEC).expect("known path");
    let folder = path.parent().expect("model folder").to_owned();
    fs::create_dir_all(&folder).expect("model folder");

    let scratch_name = |pid: u32| format!(".tiny-model.bin.download-{pid}-0.tmp");
    let live = folder.join(scratch_name(6));
    let held = ScratchLock::claim(&live)
        .expect("claim")
        .expect("free name");
    fs::write(&live, b"he").expect("live download");
    // Dead: a file beside an unheld lock file, and one with no lock file.
    fs::write(folder.join(scratch_name(7)), b"hel").expect("dead download");
    fs::File::create(scratch::lock_path(&folder.join(scratch_name(7)))).expect("lock file");
    fs::write(folder.join(scratch_name(8)), b"hell").expect("dead download");
    // Not scratch files.
    fs::write(folder.join(".tiny-model.bin.download-x.tmp"), b"x").expect("write");
    fs::write(folder.join("notes.tmp"), b"x").expect("write");

    let installed = manager
        .ensure_spec_with_cancellation_and_commit(
            &TINY_SPEC,
            &CountingDownloader::new(b"hello"),
            &|| false,
            &|| true,
        )
        .expect("install");
    assert_eq!(installed, path);

    let names = |folder: &Path| {
        let mut names: Vec<String> = fs::read_dir(folder)
            .expect("read model folder")
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
    };
    assert_eq!(
        names(&folder),
        vec![
            ".tiny-model.bin.download-6-0.tmp".to_owned(),
            ".tiny-model.bin.download-6-0.tmp.lock".to_owned(),
            ".tiny-model.bin.download-x.tmp".to_owned(),
            "notes.tmp".to_owned(),
            "tiny-model.bin".to_owned(),
        ]
    );
    assert_eq!(fs::read(&live).expect("live download kept"), b"he");
    drop(held);
}
