//! `storage` tests covering layout and validation. A `#[path]` child of the
//! `storage` test binary; the tests reach their fixtures through `super::*`.

use super::*;

#[test]
fn layout_resolves_roots_and_overrides_deterministically() {
    let temporary = TestDirectory::new();
    let ai_models_root = temporary.path.join("custom-ai-models");
    let override_root = temporary.path.join("custom-download");
    let luts_root = temporary.path.join("custom-luts");
    let layout = StorageLayout::with_overrides(
        &temporary.path,
        [
            ("luts", luts_root.clone()),
            ("download", override_root.clone()),
            ("ai_models", ai_models_root.clone()),
        ],
    )
    .unwrap();

    assert_eq!(layout.root(), temporary.path);
    assert_eq!(layout.resolve(CacheId::PreviewFrames), None);
    assert_eq!(
        layout.resolve(CacheId::Proxies),
        Some(temporary.path.join("proxies"))
    );
    assert_eq!(layout.resolve(CacheId::Download), Some(override_root));
    assert_eq!(layout.resolve(CacheId::AiModels), Some(ai_models_root));
    assert_eq!(layout.resolve(CacheId::Luts), Some(luts_root));
    assert_eq!(
        layout.resolve(CacheId::Catalog),
        Some(temporary.path.join("catalog-cache"))
    );

    let override_ids: Vec<_> = layout.overrides().keys().copied().collect();
    assert_eq!(
        override_ids,
        [CacheId::AiModels, CacheId::Download, CacheId::Luts]
    );
    let resolved_ids: Vec<_> = layout
        .resolved_disk_paths()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let disk_backed: Vec<_> = CACHE_REGISTRY
        .iter()
        .filter(|descriptor| descriptor.kind == CacheKind::Disk)
        .map(|descriptor| descriptor.id)
        .collect();
    assert_eq!(resolved_ids, disk_backed);
}

#[test]
fn layout_rejects_invalid_overrides() {
    let temporary = TestDirectory::new();
    let mut layout = StorageLayout::new(&temporary.path).unwrap();

    assert!(matches!(
        StorageLayout::new("relative"),
        Err(StorageError::PathNotAbsolute(_))
    ));
    assert!(matches!(
        layout.set_override(CacheId::PreviewFrames, temporary.path.join("memory")),
        Err(StorageError::CacheIsNotDisk(CacheId::PreviewFrames))
    ));
    assert!(matches!(
        layout.set_override(CacheId::Proxies, "relative"),
        Err(StorageError::PathNotAbsolute(_))
    ));
    assert!(matches!(
        layout.set_override_key("unknown", temporary.path.join("unknown")),
        Err(StorageError::UnknownCacheId)
    ));
    assert!(matches!(
        layout.set_override(CacheId::Download, temporary.path.join("proxies")),
        Err(StorageError::CachePathsOverlap {
            cache: CacheId::Download,
            other: CacheId::Proxies
        })
    ));
    layout
        .set_override(CacheId::Download, temporary.path.join("custom-download"))
        .unwrap();
    assert!(matches!(
        layout.set_override(
            CacheId::Proxies,
            temporary.path.join("custom-download").join("nested")
        ),
        Err(StorageError::CachePathsOverlap {
            cache: CacheId::Proxies,
            other: CacheId::Download
        })
    ));
    assert!(matches!(
        StorageLayout::with_overrides(
            &temporary.path,
            [
                ("download", temporary.path.join("one")),
                ("download", temporary.path.join("two")),
            ],
        ),
        Err(StorageError::DuplicateOverride(CacheId::Download))
    ));
}

#[test]
fn filesystem_validation_accepts_distinct_existing_roots() {
    let temporary = TestDirectory::new();
    let root = temporary.path.join("storage");
    fs::create_dir(&root).unwrap();
    let layout = StorageLayout::new(&root).unwrap();

    for (_, path) in layout.resolved_disk_paths() {
        fs::create_dir(&path).unwrap();
    }

    layout.validate_filesystem().unwrap();
}

#[test]
fn filesystem_validation_allows_missing_leaves_without_creating_directories() {
    let temporary = TestDirectory::new();
    let missing_parent = temporary.path.join("missing-parent");
    let root = missing_parent.join("storage");
    let layout = StorageLayout::new(&root).unwrap();

    assert!(!missing_parent.exists());
    layout.validate_filesystem().unwrap();
    assert!(!missing_parent.exists());
    for (_, path) in layout.resolved_disk_paths() {
        assert!(!path.exists());
    }
}

#[cfg(unix)]
#[test]
fn filesystem_validation_rejects_same_target_parent_aliases_deterministically() {
    use std::os::unix::fs::symlink;

    let temporary = TestDirectory::new();
    let physical_parent = temporary.path.join("physical");
    let shared = physical_parent.join("shared");
    let alias_parent = temporary.path.join("alias");
    fs::create_dir(&physical_parent).unwrap();
    fs::create_dir(&shared).unwrap();
    symlink(&physical_parent, &alias_parent).unwrap();

    let layout = StorageLayout::with_overrides(
        temporary.path.join("defaults"),
        [
            ("download", alias_parent.join("shared")),
            ("proxies", shared),
        ],
    )
    .unwrap();
    let error = layout.validate_filesystem().unwrap_err();

    assert!(matches!(
        &error,
        StorageError::CachePathsOverlap {
            cache: CacheId::Proxies,
            other: CacheId::Download,
        }
    ));
    assert!(
        !error
            .to_string()
            .contains(temporary.path.to_string_lossy().as_ref())
    );
}

#[cfg(unix)]
#[test]
fn filesystem_validation_rejects_alias_nested_roots() {
    use std::os::unix::fs::symlink;

    let temporary = TestDirectory::new();
    let physical_parent = temporary.path.join("physical");
    let cache = physical_parent.join("cache");
    let nested = cache.join("nested");
    let alias_parent = temporary.path.join("alias");
    fs::create_dir(&physical_parent).unwrap();
    fs::create_dir(&cache).unwrap();
    fs::create_dir(&nested).unwrap();
    symlink(&physical_parent, &alias_parent).unwrap();

    let layout = StorageLayout::with_overrides(
        temporary.path.join("defaults"),
        [
            ("download", alias_parent.join("cache").join("nested")),
            ("proxies", cache),
        ],
    )
    .unwrap();

    assert!(matches!(
        layout.validate_filesystem(),
        Err(StorageError::CachePathsOverlap {
            cache: CacheId::Proxies,
            other: CacheId::Download,
        })
    ));
}

#[cfg(unix)]
#[test]
fn filesystem_validation_rejects_symlink_leaf() {
    use std::os::unix::fs::symlink;

    let temporary = TestDirectory::new();
    let target = temporary.path.join("target");
    let cache_link = temporary.path.join("cache-link");
    fs::create_dir(&target).unwrap();
    symlink(&target, &cache_link).unwrap();
    let layout =
        StorageLayout::with_overrides(temporary.path.join("defaults"), [("proxies", cache_link)])
            .unwrap();

    assert!(matches!(
        layout.validate_filesystem(),
        Err(StorageError::SymlinkRoot)
    ));
}

#[test]
fn filesystem_validation_rejects_non_directory_leaf_and_ancestor() {
    let temporary = TestDirectory::new();
    let file_leaf = temporary.path.join("file-leaf");
    fs::write(&file_leaf, b"not a directory").unwrap();
    let leaf_layout = StorageLayout::with_overrides(
        temporary.path.join("leaf-defaults"),
        [("proxies", file_leaf)],
    )
    .unwrap();
    assert!(matches!(
        leaf_layout.validate_filesystem(),
        Err(StorageError::NotDirectory)
    ));

    let file_ancestor = temporary.path.join("file-ancestor");
    fs::write(&file_ancestor, b"not a directory").unwrap();
    let ancestor_layout = StorageLayout::with_overrides(
        temporary.path.join("ancestor-defaults"),
        [("proxies", file_ancestor.join("missing-cache"))],
    )
    .unwrap();
    assert!(matches!(
        ancestor_layout.validate_filesystem(),
        Err(StorageError::NotDirectory)
    ));
}

#[cfg(unix)]
#[test]
fn filesystem_validation_reports_alias_resolution_io_without_paths() {
    use std::os::unix::fs::symlink;

    let temporary = TestDirectory::new();
    let alias = temporary.path.join("dangling-alias");
    symlink(temporary.path.join("missing-target"), &alias).unwrap();
    let layout = StorageLayout::with_overrides(
        temporary.path.join("defaults"),
        [("proxies", alias.join("cache"))],
    )
    .unwrap();
    let error = layout.validate_filesystem().unwrap_err();

    match &error {
        StorageError::Io { operation, .. } => {
            assert_eq!(*operation, "canonicalize cache layout ancestor");
            assert!(operation.len() < 64);
        }
        other => panic!("expected alias resolution I/O error, got {other:?}"),
    }
    assert!(
        !error
            .to_string()
            .contains(temporary.path.to_string_lossy().as_ref())
    );
}

#[cfg(unix)]
#[test]
fn filesystem_root_is_rejected_by_destructive_operations() {
    assert!(matches!(
        StorageLayout::new("/"),
        Err(StorageError::DangerousPath(_))
    ));
    assert!(matches!(
        clear_cache(Path::new("/"), &NeverCancelled),
        Err(StorageError::DangerousPath(_))
    ));

    let temporary = TestDirectory::new();
    let source = temporary.path.join("source");
    fs::create_dir(&source).unwrap();
    assert!(matches!(
        relocate_cache(&source, Path::new("/"), &NeverCancelled, |_| Ok(())),
        Err(StorageError::DangerousPath(_))
    ));
}
