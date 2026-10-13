//! `begin_commit` cases.

use super::*;

#[test]
fn catalog_has_verified_base_en_constants() {
    assert_eq!(WhisperModel::catalog(), &[WhisperModel::BaseEn]);
    let spec = WhisperModel::BaseEn.spec();
    assert_eq!(spec.id(), "base.en");
    assert_eq!(spec.filename(), "ggml-base.en.bin");
    assert_eq!(
        spec.url(),
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"
    );
    assert_eq!(spec.exact_bytes(), 147_964_211);
    assert_eq!(
        spec.sha256(),
        "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002"
    );
    assert!(spec.is_english_only());
    assert!(spec.resource_description().contains("148 MB"));
    assert!(validate_spec(spec).is_ok());
}

#[test]
fn validates_root_and_catalog_paths_without_traversal() {
    // `/` is the filesystem root on unix but root-relative (hence not even
    // absolute) on Windows; each platform spells its own roots here.
    let fs_root: &Path = if cfg!(windows) {
        Path::new("C:/")
    } else {
        Path::new("/")
    };
    let traversal: &Path = if cfg!(windows) {
        Path::new("C:/tmp/../escape")
    } else {
        Path::new("/tmp/../escape")
    };
    assert!(matches!(
        ModelManager::new("relative/models"),
        Err(ModelManagerError::RootNotAbsolute { .. })
    ));
    assert!(matches!(
        ModelManager::new(fs_root),
        Err(ModelManagerError::FilesystemRoot { .. })
    ));
    assert!(matches!(
        ModelManager::new(traversal),
        Err(ModelManagerError::RootTraversal { .. })
    ));

    let temp = TempDir::new().expect("temporary directory");
    let manager = ModelManager::new(temp.path()).expect("valid absolute root");
    let traversal = ModelSpec::new(
        "test.traversal",
        "../escape.bin",
        "https://example.invalid/escape.bin",
        &[],
        5,
        HELLO_SHA256,
        true,
        "invalid test entry",
    );
    assert!(matches!(
        manager.path_for_spec(&traversal),
        Err(ModelManagerError::InvalidCatalogEntry { .. })
    ));
    assert!(!temp.path().join("../escape.bin").exists());
}

#[test]
fn pre_cancelled_install_creates_no_root_or_download() {
    let temp = TempDir::new().expect("temporary directory");
    let root = temp.path().join("models");
    let manager = ModelManager::new(&root).expect("valid manager");
    let downloader = CountingDownloader::new(b"unused");

    let error = manager
        .ensure_with_cancellation(WhisperModel::BaseEn, &downloader, &|| true)
        .expect_err("pre-cancelled installation must stop");

    assert!(matches!(&error, ModelManagerError::Cancelled));
    assert_eq!(error.to_string(), "model installation cancelled");
    assert!(!root.exists());
    assert_eq!(downloader.calls(), 0);
}

#[test]
fn panicking_cancellation_callback_fails_closed() {
    let temp = TempDir::new().expect("temporary directory");
    let root = temp.path().join("models");
    let manager = ModelManager::new(&root).expect("valid manager");
    let downloader = CountingDownloader::new(b"unused");

    let error = manager
        .ensure_with_cancellation(WhisperModel::BaseEn, &downloader, &|| {
            panic!("injected cancellation callback panic")
        })
        .expect_err("callback panic must be cancellation");

    assert!(matches!(error, ModelManagerError::Cancelled));
    assert!(!root.exists());
    assert_eq!(downloader.calls(), 0);
}

#[test]
fn reuses_a_verified_existing_model_without_downloading() {
    let temp = TempDir::new().expect("temporary directory");
    let manager = ModelManager::new(temp.path()).expect("valid manager");
    let path = manager.path_for_spec(&TINY_SPEC).expect("known path");
    fs::write(&path, b"hello").expect("write fixture");
    let downloader = |_spec: &ModelSpec| -> Result<DownloadReader, DownloadError> {
        panic!("verified model must not be downloaded")
    };

    assert_eq!(
        manager
            .ensure_spec(&TINY_SPEC, &downloader)
            .expect("reuse succeeds"),
        path
    );
    assert!(matches!(
        manager.status_spec(&TINY_SPEC).expect("status"),
        ModelStatus::Ready { .. }
    ));
}

#[test]
fn redownloads_wrong_size_and_corrupt_existing_models() {
    for existing in [&b"bad"[..], &b"jello"[..]] {
        let temp = TempDir::new().expect("temporary directory");
        let manager = ModelManager::new(temp.path()).expect("valid manager");
        let path = manager.path_for_spec(&TINY_SPEC).expect("known path");
        fs::write(&path, existing).expect("write corrupt fixture");
        let downloader = CountingDownloader::new(b"hello");

        manager
            .ensure_spec(&TINY_SPEC, &downloader)
            .expect("redownload succeeds");

        assert_eq!(downloader.calls(), 1);
        assert_eq!(fs::read(path).expect("read installed model"), b"hello");
        assert_no_temporary_files(temp.path());
    }
}

#[test]
fn rejects_an_overlong_stream_and_cleans_temporary_file() {
    let temp = TempDir::new().expect("temporary directory");
    let manager = ModelManager::new(temp.path()).expect("valid manager");
    let downloader = CountingDownloader::new(b"hello!");

    let error = manager
        .ensure_spec(&TINY_SPEC, &downloader)
        .expect_err("overlong download must fail");
    assert!(matches!(
        error,
        ModelManagerError::DownloadTooLarge {
            expected: 5,
            observed: 6
        }
    ));
    assert!(!manager.path_for_spec(&TINY_SPEC).expect("path").exists());
    assert_directory_empty(temp.path());
}

#[test]
fn rejects_checksum_mismatch_and_cleans_temporary_file() {
    let temp = TempDir::new().expect("temporary directory");
    let manager = ModelManager::new(temp.path()).expect("valid manager");
    let downloader = CountingDownloader::new(b"jello");

    let error = manager
        .ensure_spec(&TINY_SPEC, &downloader)
        .expect_err("checksum mismatch must fail");
    assert!(matches!(
        error,
        ModelManagerError::Integrity {
            reason: ModelIntegrityError::ChecksumMismatch { .. },
            ..
        }
    ));
    assert_directory_empty(temp.path());
}

#[test]
fn interrupted_reader_cleans_temporary_file() {
    let temp = TempDir::new().expect("temporary directory");
    let manager = ModelManager::new(temp.path()).expect("valid manager");
    let downloader = |_spec: &ModelSpec| -> Result<DownloadReader, DownloadError> {
        Ok(Box::new(InterruptedReader { emitted: false }))
    };

    let error = manager
        .ensure_spec(&TINY_SPEC, &downloader)
        .expect_err("interrupted download must fail");
    assert!(matches!(error, ModelManagerError::Io { .. }));
    assert_directory_empty(temp.path());
}

#[test]
fn mid_stream_cancellation_cleans_temp_and_preserves_invalid_target() {
    let temp = TempDir::new().expect("temporary directory");
    let manager = ModelManager::new(temp.path()).expect("valid manager");
    let path = manager.path_for_spec(&TINY_SPEC).expect("known path");
    fs::write(&path, b"bad").expect("write invalid existing model");

    let cancelled = Arc::new(AtomicBool::new(false));
    let downloader_calls = Arc::new(AtomicUsize::new(0));
    let cancelled_for_reader = Arc::clone(&cancelled);
    let calls_for_downloader = Arc::clone(&downloader_calls);
    let downloader = move |_spec: &ModelSpec| -> Result<DownloadReader, DownloadError> {
        calls_for_downloader.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(CancellingReader {
            read_count: 0,
            cancelled: Arc::clone(&cancelled_for_reader),
        }))
    };

    let error = manager
        .ensure_spec_with_cancellation(&TINY_SPEC, &downloader, &|| {
            cancelled.load(Ordering::SeqCst)
        })
        .expect_err("mid-stream cancellation must stop");

    assert!(matches!(error, ModelManagerError::Cancelled));
    assert_eq!(downloader_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fs::read(&path).expect("read invalid target"), b"bad");
    assert_no_temporary_files(temp.path());
}

#[test]
fn begin_commit_is_called_once_for_fresh_install_and_replacement() {
    for existing in [None, Some(&b"jello"[..])] {
        let temp = TempDir::new().expect("temporary directory");
        let manager = ModelManager::new(temp.path()).expect("valid manager");
        let path = manager.path_for_spec(&TINY_SPEC).expect("known path");
        if let Some(bytes) = existing {
            fs::write(&path, bytes).expect("write invalid existing model");
        }
        let downloader = CountingDownloader::new(b"hello");
        let begin_calls = AtomicUsize::new(0);

        let installed = manager
            .ensure_spec_with_cancellation_and_commit(
                &TINY_SPEC,
                &downloader,
                &never_cancelled,
                &|| {
                    begin_calls.fetch_add(1, Ordering::SeqCst);
                    true
                },
            )
            .expect("installation succeeds");

        assert_eq!(installed, path);
        assert_eq!(begin_calls.load(Ordering::SeqCst), 1);
        assert_eq!(downloader.calls(), 1);
        assert_eq!(fs::read(&path).expect("read installed model"), b"hello");
        assert_no_temporary_files(temp.path());
    }
}

#[test]
fn rejected_begin_commit_preserves_target_and_cleans_temp() {
    for existing in [None, Some(&b"bad"[..])] {
        let temp = TempDir::new().expect("temporary directory");
        let manager = ModelManager::new(temp.path()).expect("valid manager");
        let path = manager.path_for_spec(&TINY_SPEC).expect("known path");
        if let Some(bytes) = existing {
            fs::write(&path, bytes).expect("write invalid existing model");
        }
        let downloader = CountingDownloader::new(b"hello");
        let begin_calls = AtomicUsize::new(0);

        let error = manager
            .ensure_spec_with_cancellation_and_commit(
                &TINY_SPEC,
                &downloader,
                &never_cancelled,
                &|| {
                    begin_calls.fetch_add(1, Ordering::SeqCst);
                    false
                },
            )
            .expect_err("rejected commit must cancel");

        assert!(matches!(error, ModelManagerError::Cancelled));
        assert_eq!(begin_calls.load(Ordering::SeqCst), 1);
        match existing {
            Some(bytes) => {
                assert_eq!(fs::read(&path).expect("read preserved target"), bytes);
            }
            None => assert!(!path.exists()),
        }
        assert_no_temporary_files(temp.path());
    }
}

#[test]
fn panicking_begin_commit_preserves_target_and_cleans_temp() {
    for existing in [None, Some(&b"bad"[..])] {
        let temp = TempDir::new().expect("temporary directory");
        let manager = ModelManager::new(temp.path()).expect("valid manager");
        let path = manager.path_for_spec(&TINY_SPEC).expect("known path");
        if let Some(bytes) = existing {
            fs::write(&path, bytes).expect("write invalid existing model");
        }
        let downloader = CountingDownloader::new(b"hello");
        let begin_calls = AtomicUsize::new(0);

        let error = manager
            .ensure_spec_with_cancellation_and_commit(
                &TINY_SPEC,
                &downloader,
                &never_cancelled,
                &|| {
                    begin_calls.fetch_add(1, Ordering::SeqCst);
                    panic!("injected begin-commit panic")
                },
            )
            .expect_err("panicking commit callback must cancel");

        assert!(matches!(error, ModelManagerError::Cancelled));
        assert_eq!(begin_calls.load(Ordering::SeqCst), 1);
        match existing {
            Some(bytes) => {
                assert_eq!(fs::read(&path).expect("read preserved target"), bytes);
            }
            None => assert!(!path.exists()),
        }
        assert_no_temporary_files(temp.path());
    }
}

#[test]
fn begin_commit_observes_complete_verified_and_synced_temporary_file() {
    let temp = TempDir::new().expect("temporary directory");
    let manager = ModelManager::new(temp.path()).expect("valid manager");
    let path = manager.path_for_spec(&TINY_SPEC).expect("known path");
    let read_calls = Arc::new(AtomicUsize::new(0));
    let saw_eof = Arc::new(AtomicBool::new(false));
    let read_calls_for_downloader = Arc::clone(&read_calls);
    let saw_eof_for_downloader = Arc::clone(&saw_eof);
    let downloader = move |_spec: &ModelSpec| -> Result<DownloadReader, DownloadError> {
        Ok(Box::new(CompletionTrackingReader {
            cursor: Cursor::new(b"hello"),
            read_calls: Arc::clone(&read_calls_for_downloader),
            saw_eof: Arc::clone(&saw_eof_for_downloader),
        }))
    };

    manager
        .ensure_spec_with_cancellation_and_commit(
            &TINY_SPEC,
            &downloader,
            &never_cancelled,
            &|| {
                assert!(saw_eof.load(Ordering::SeqCst));
                assert!(read_calls.load(Ordering::SeqCst) >= 2);
                assert!(!path.exists());

                let temporary_paths: Vec<_> = fs::read_dir(temp.path())
                    .expect("read model directory")
                    .map(|entry| entry.expect("directory entry").path())
                    .filter(|entry| {
                        entry
                            .extension()
                            .is_some_and(|extension| extension == "tmp")
                    })
                    .collect();
                assert_eq!(temporary_paths.len(), 1);
                assert_eq!(
                    fs::read(&temporary_paths[0]).expect("read synced temporary model"),
                    b"hello"
                );
                true
            },
        )
        .expect("installation succeeds");

    assert_eq!(fs::read(&path).expect("read installed model"), b"hello");
    assert_no_temporary_files(temp.path());
}

#[test]
fn cancellation_before_commit_skips_begin_commit() {
    let temp = TempDir::new().expect("temporary directory");
    let manager = ModelManager::new(temp.path()).expect("valid manager");
    let path = manager.path_for_spec(&TINY_SPEC).expect("known path");
    fs::write(&path, b"bad").expect("write invalid existing model");
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_for_downloader = Arc::clone(&cancelled);
    let downloader = move |_spec: &ModelSpec| -> Result<DownloadReader, DownloadError> {
        Ok(Box::new(CancelAtEofReader {
            cursor: Cursor::new(b"hello"),
            cancelled: Arc::clone(&cancelled_for_downloader),
        }))
    };
    let begin_calls = AtomicUsize::new(0);

    let error = manager
        .ensure_spec_with_cancellation_and_commit(
            &TINY_SPEC,
            &downloader,
            &|| cancelled.load(Ordering::SeqCst),
            &|| {
                begin_calls.fetch_add(1, Ordering::SeqCst);
                true
            },
        )
        .expect_err("pre-commit cancellation must win");

    assert!(matches!(error, ModelManagerError::Cancelled));
    assert_eq!(begin_calls.load(Ordering::SeqCst), 0);
    assert_eq!(fs::read(&path).expect("read preserved target"), b"bad");
    assert_no_temporary_files(temp.path());
}
