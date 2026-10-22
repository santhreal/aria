//! ABI table compatibility, package directory mounting, and dynamic library
//! loading tests for `aria_plugin::external`.

use aria_plugin_abi::ARIA_PLUGIN_V1_MIN_SIZE;
use tempfile::TempDir;

use super::*;
use crate::{DropAction, Origin, ResolveDropped};

fn find_compiled_cdylib(stem: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("test executable path");
    let deps_dir = exe.parent().expect("deps dir");
    let target_dir = deps_dir.parent().unwrap_or(deps_dir);
    let prefix = if cfg!(target_os = "windows") {
        ""
    } else {
        "lib"
    };
    let ext = std::env::consts::DLL_EXTENSION;
    let exact_name = format!("{prefix}{stem}.{ext}");

    for dir in [target_dir, deps_dir] {
        let candidate = dir.join(&exact_name);
        if candidate.is_file() {
            return candidate;
        }
    }
    let dashed_prefix = format!("{prefix}{stem}-");
    let suffix = format!(".{ext}");
    if let Ok(entries) = std::fs::read_dir(deps_dir) {
        let mut matches: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&dashed_prefix) && n.ends_with(&suffix))
            })
            .collect();
        matches.sort();
        if let Some(found) = matches.pop() {
            return found;
        }
    }
    panic!(
        "compiled cdylib `{exact_name}` not found in {} or {}",
        target_dir.display(),
        deps_dir.display()
    );
}

#[test]
fn table_compatibility_checks_api_major_and_struct_size() {
    let exact_size = std::mem::size_of::<AriaPluginV1>();
    assert_eq!(check_compat(ABI_VERSION, exact_size), Ok(()));
    assert_eq!(check_compat(ABI_VERSION, ARIA_PLUGIN_V1_MIN_SIZE), Ok(()));
    assert_eq!(check_compat(ABI_VERSION, exact_size + 64), Ok(()));

    for wrong_major in [0, 2, 8, u32::MAX] {
        let err = check_compat(wrong_major, exact_size).expect_err("wrong ABI major must fail");
        assert!(err.contains(&wrong_major.to_string()), "{err}");
        assert!(err.contains(&ABI_VERSION.to_string()), "{err}");
    }

    for short_size in [0, 1, ARIA_PLUGIN_V1_MIN_SIZE - 1] {
        let err = check_compat(ABI_VERSION, short_size).expect_err("short table must fail");
        assert!(err.contains(&short_size.to_string()), "{err}");
        assert!(err.contains(&ARIA_PLUGIN_V1_MIN_SIZE.to_string()), "{err}");
    }
}

#[test]
fn missing_directory_and_bare_library_and_non_plugin_library_are_handled() {
    let temp = TempDir::new().expect("temp dir");
    let absent = temp.path().join("absent");
    let ctx = PluginContext::new();
    assert!(mount_dir(&ctx, &absent).is_empty());

    // A bare shared library outside a package directory is refused with an actionable message.
    let plugins_dir = temp.path().join("plugins");
    std::fs::create_dir_all(&plugins_dir).expect("create plugins dir");
    let bare_name = format!("broken.{LIBRARY_EXTENSION}");
    std::fs::write(plugins_dir.join(&bare_name), b"not a shared object").expect("write bare file");
    let failures = mount_dir(&ctx, &plugins_dir);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].id, bare_name);
    assert!(
        failures[0].reason.contains(MANIFEST_FILE),
        "{}",
        failures[0].reason
    );
    assert_eq!(ctx.failures().len(), 1);

    // A real shared library that does not export `aria_plugin_entry` is refused by symbol check.
    if let Some(sys_lib) = [
        "/lib/x86_64-linux-gnu/libc.so.6",
        "/lib64/libc.so.6",
        "/usr/lib/libSystem.B.dylib",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
    {
        let pkg_root = temp.path().join("sys_pkg");
        let manifest = basic_manifest("test.syslib", "SysLib", "none");
        let pkg_dir = write_package(&pkg_root, "test.syslib", &manifest, None);
        let lib_dir = pkg_dir.join(LIBRARY_DIR);
        std::fs::create_dir_all(&lib_dir).expect("create lib dir");
        std::fs::copy(&sys_lib, lib_dir.join(library_name())).expect("copy sys lib");
        let pkg = Package::open(&pkg_dir).expect("open package");
        let Err(err) = load(&pkg) else {
            panic!("system library without aria_plugin_entry must be refused");
        };
        assert_eq!(err, "not an aria plugin: no aria_plugin_entry symbol");
    }
}

#[test]
fn notes_plugin_cdylib_loads_and_executes_end_to_end() {
    // Ensure the rlib link keeps `aria-notes-plugin` built by Cargo before this test runs.
    let _ = aria_notes_plugin::seen_commands();
    let cdylib_path = find_compiled_cdylib("aria_notes_plugin");

    let temp = TempDir::new().expect("temp dir");
    let data_root = temp.path().join("data");
    let plugins_dir = data_root.join("plugins");
    let pkg_dir = plugins_dir.join("example.notes");
    let lib_dir = pkg_dir.join(LIBRARY_DIR);
    std::fs::create_dir_all(&lib_dir).expect("create package lib dir");

    let manifest_src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/notes-plugin/plugin.toml");
    std::fs::copy(&manifest_src, pkg_dir.join(MANIFEST_FILE)).expect("copy plugin.toml");
    std::fs::copy(&cdylib_path, lib_dir.join(library_name())).expect("copy notes cdylib");

    let pkg = Package::open(&pkg_dir).expect("open example.notes package");
    assert!(matches!(pkg.tier, Tier::Native { .. }));

    let ctx = PluginContext::new();
    let _settings_reg = ctx.register_arc::<dyn SettingsService>(Arc::new(TestSettings));
    let host = make_project_host(&ctx);

    let plugin = load_with_data_root(&pkg, &data_root).expect("load example.notes cdylib");
    ctx.mount(plugin.as_ref()).expect("mount example.notes");
    assert!(ctx.is_mounted("example.notes"));
    let logs = ctx.plugin_logs("example.notes");
    assert!(
        logs.iter().any(|line| {
            line.level == crate::LogLevel::Info && &*line.text == "mounted example.notes"
        }),
        "expected Info log 'mounted example.notes', got: {logs:?}"
    );

    // 1. ResolveDropped claims `.srt` and ignores `.mp4`.
    let srt_claim = ctx
        .events()
        .serial(&ResolveDropped::path(PathBuf::from("captions.SRT")));
    assert_eq!(srt_claim, Some(DropAction::ImportMedia));

    let mp4_claim: Option<DropAction> = ctx
        .events()
        .serial(&ResolveDropped::path(PathBuf::from("video.mp4")));
    assert_eq!(mp4_claim, None);

    // 2. CommandProposed allows an edit touching <= 8 tracks.
    let outcome = host
        .apply(
            &Origin::Editor,
            Command::Edit(EditCommand::AddTrack {
                kind: TrackKind::Video,
                name: "V2".to_owned(),
                index: None,
                pinned: true,
            }),
        )
        .expect("single-track edit should pass");
    let _ = outcome;

    // 3. Running `notes.count` writes the seen command count to `<data_root>/plugin-data/example.notes/seen_commands`.
    ctx.run_command("notes.count")
        .expect("run notes.count command");
    let stored = std::fs::read_to_string(
        data_root
            .join("plugin-data")
            .join("example.notes")
            .join("seen_commands"),
    )
    .expect("read stored seen_commands");
    let parsed_count: usize = stored.parse().expect("valid integer count");
    assert!(
        parsed_count >= 1,
        "expected at least 1 seen command, got {parsed_count}"
    );

    ctx.unmount("example.notes").expect("unmount example.notes");
    assert!(!ctx.is_mounted("example.notes"));
}
