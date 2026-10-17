//! `storage` tests covering cache contracts. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `storage` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn registry_is_exact_unique_and_round_trips() {
    let expected = [
        (
            "preview_frames",
            "Preview frames",
            CacheKind::Memory,
            CacheTier::Disposable,
            None,
        ),
        (
            "library_thumbnails",
            "Library thumbnails",
            CacheKind::Disk,
            CacheTier::Disposable,
            Some("library-thumbnails"),
        ),
        (
            "timeline_filmstrips",
            "Timeline filmstrips",
            CacheKind::Memory,
            CacheTier::Disposable,
            None,
        ),
        (
            "timeline_waveforms",
            "Timeline waveforms",
            CacheKind::Memory,
            CacheTier::Disposable,
            None,
        ),
        (
            "proxies",
            "Proxies",
            CacheKind::Disk,
            CacheTier::Disposable,
            Some("proxies"),
        ),
        (
            "analysis",
            "Media analysis",
            CacheKind::Disk,
            CacheTier::Disposable,
            Some("analysis"),
        ),
        (
            "render_files",
            "Render files",
            CacheKind::Disk,
            CacheTier::Disposable,
            Some("render-files"),
        ),
        (
            "ai_models",
            "AI models",
            CacheKind::Disk,
            CacheTier::Redownloadable,
            Some("ai-models"),
        ),
        (
            "download",
            "Downloads",
            CacheKind::Disk,
            CacheTier::Redownloadable,
            Some("download-cache"),
        ),
        (
            "catalog",
            "Catalog",
            CacheKind::Disk,
            CacheTier::Redownloadable,
            Some("catalog-cache"),
        ),
        (
            "luts",
            "LUTs",
            CacheKind::Disk,
            CacheTier::Redownloadable,
            Some("luts"),
        ),
        (
            "lottie",
            "Lottie assets",
            CacheKind::Disk,
            CacheTier::Redownloadable,
            Some("lottie"),
        ),
        (
            "templates",
            "Templates",
            CacheKind::Disk,
            CacheTier::Redownloadable,
            Some("templates"),
        ),
    ];

    let actual: Vec<_> = CACHE_REGISTRY
        .iter()
        .map(|descriptor| {
            (
                descriptor.id.as_str(),
                descriptor.label,
                descriptor.kind,
                descriptor.tier,
                descriptor.default_relative,
            )
        })
        .collect();
    assert_eq!(actual, expected);
    let registry_ids: Vec<_> = CACHE_REGISTRY
        .iter()
        .map(|descriptor| descriptor.id)
        .collect();
    assert_eq!(CacheId::ALL.as_slice(), registry_ids.as_slice());

    let unique: HashSet<_> = CACHE_REGISTRY
        .iter()
        .map(|descriptor| descriptor.id.as_str())
        .collect();
    assert_eq!(unique.len(), CACHE_REGISTRY.len());

    for descriptor in CACHE_REGISTRY {
        assert_eq!(CacheId::parse(descriptor.id.as_str()), Ok(descriptor.id));
        assert_eq!(descriptor.id.as_str().parse::<CacheId>(), Ok(descriptor.id));
        assert_eq!(
            cache_descriptor_by_key(descriptor.id.as_str()),
            Some(&descriptor)
        );
        assert_ne!(descriptor.tier, CacheTier::UserData);
    }

    for forbidden in ["projects", "config", "agent_sessions"] {
        assert!(CacheId::parse(forbidden).is_err());
        assert!(cache_descriptor_by_key(forbidden).is_none());
    }
    assert!(CacheId::parse("Proxies").is_err());
    assert!(CacheId::parse("proxies ").is_err());
}

#[test]
fn analysis_cache_has_a_stable_disk_contract() {
    let temporary = TestDirectory::new();
    let layout = StorageLayout::new(&temporary.path).unwrap();
    let descriptor = CacheId::Analysis.descriptor();

    assert_eq!(CacheId::parse("analysis"), Ok(CacheId::Analysis));
    assert_eq!("analysis".parse::<CacheId>(), Ok(CacheId::Analysis));
    assert_eq!(CacheId::Analysis.to_string(), "analysis");
    assert_eq!(cache_descriptor_by_key("analysis"), Some(descriptor));
    assert_eq!(descriptor.label, "Media analysis");
    assert_eq!(descriptor.kind, CacheKind::Disk);
    assert_eq!(descriptor.tier, CacheTier::Disposable);
    assert_eq!(descriptor.default_relative, Some("analysis"));
    assert_eq!(
        layout.resolve(CacheId::Analysis),
        Some(temporary.path.join("analysis"))
    );
}

#[test]
fn ai_models_cache_has_a_stable_redownloadable_disk_contract() {
    let temporary = TestDirectory::new();
    let layout = StorageLayout::new(&temporary.path).unwrap();
    let descriptor = CacheId::AiModels.descriptor();
    let root = temporary.path.join("ai-models");

    assert_eq!(CacheId::parse("ai_models"), Ok(CacheId::AiModels));
    assert_eq!("ai_models".parse::<CacheId>(), Ok(CacheId::AiModels));
    assert_eq!(CacheId::AiModels.to_string(), "ai_models");
    assert_eq!(cache_descriptor_by_key("ai_models"), Some(descriptor));
    assert_eq!(descriptor.label, "AI models");
    assert_eq!(descriptor.kind, CacheKind::Disk);
    assert_eq!(descriptor.tier, CacheTier::Redownloadable);
    assert_eq!(descriptor.default_relative, Some("ai-models"));
    assert_eq!(layout.resolve(CacheId::AiModels), Some(root.clone()));

    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("ggml-base.en.bin"), b"model-weights").unwrap();
    let usage = measure_disk_usage(&root, &NeverCancelled).unwrap();
    assert_eq!((usage.bytes, usage.files), (13, 1));
}

#[test]
fn analysis_cache_participates_in_overlap_validation() {
    let temporary = TestDirectory::new();
    let mut layout = StorageLayout::new(&temporary.path).unwrap();

    assert!(matches!(
        layout.set_override(
            CacheId::Analysis,
            temporary.path.join("proxies").join("nested")
        ),
        Err(StorageError::CachePathsOverlap {
            cache: CacheId::Analysis,
            other: CacheId::Proxies,
        })
    ));
    assert!(matches!(
        layout.set_override(
            CacheId::Download,
            temporary.path.join("analysis").join("nested")
        ),
        Err(StorageError::CachePathsOverlap {
            cache: CacheId::Download,
            other: CacheId::Analysis,
        })
    ));
}

#[test]
fn ai_models_cache_participates_in_overlap_validation() {
    let temporary = TestDirectory::new();
    let mut layout = StorageLayout::new(&temporary.path).unwrap();

    assert!(matches!(
        layout.set_override(
            CacheId::AiModels,
            temporary.path.join("analysis").join("models")
        ),
        Err(StorageError::CachePathsOverlap {
            cache: CacheId::AiModels,
            other: CacheId::Analysis,
        })
    ));
    assert!(matches!(
        layout.set_override(
            CacheId::Download,
            temporary.path.join("ai-models").join("nested")
        ),
        Err(StorageError::CachePathsOverlap {
            cache: CacheId::Download,
            other: CacheId::AiModels,
        })
    ));
}

#[test]
fn render_files_cache_has_a_stable_disposable_disk_contract() {
    let temporary = TestDirectory::new();
    let layout = StorageLayout::new(&temporary.path).unwrap();
    let descriptor = CacheId::RenderFiles.descriptor();
    let root = temporary.path.join("render-files");

    assert_eq!(CacheId::parse("render_files"), Ok(CacheId::RenderFiles));
    assert_eq!("render_files".parse::<CacheId>(), Ok(CacheId::RenderFiles));
    assert_eq!(CacheId::RenderFiles.to_string(), "render_files");
    assert_eq!(cache_descriptor_by_key("render_files"), Some(descriptor));
    assert_eq!(descriptor.label, "Render files");
    assert_eq!(descriptor.kind, CacheKind::Disk);
    assert_eq!(descriptor.tier, CacheTier::Disposable);
    assert_eq!(descriptor.default_relative, Some("render-files"));
    assert_eq!(layout.resolve(CacheId::RenderFiles), Some(root));
}
