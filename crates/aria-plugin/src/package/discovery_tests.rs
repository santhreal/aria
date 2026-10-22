use super::*;

#[test]
fn the_files_present_choose_the_tier() {
    let root = scratch();
    let wasm = root.path().join("wasm");
    write(
        &wasm,
        MANIFEST_FILE,
        &text("com.test.wasm", "W", "1.0.0", "1", ""),
    );
    write(&wasm, COMPONENT_FILE, "\0asm");
    let native = root.path().join("native");
    write(
        &native,
        MANIFEST_FILE,
        &text("com.test.native", "N", "1.0.0", "1", ""),
    );
    let library = format!("{LIBRARY_DIR}/{}", library_name());
    write(&native, &library, "");
    let data = root.path().join("data");
    write(
        &data,
        MANIFEST_FILE,
        &text("com.test.data", "D", "1.0.0", "1", ""),
    );

    assert_eq!(
        Package::open(&wasm).expect("wasm").tier,
        Tier::Wasm {
            component: wasm.join(COMPONENT_FILE)
        }
    );
    assert_eq!(
        Package::open(&native).expect("native").tier,
        Tier::Native {
            library: native.join(&library)
        }
    );
    assert_eq!(Package::open(&data).expect("data").tier, Tier::Data);
}

#[test]
fn a_package_that_is_not_one_tier_is_refused() {
    let root = scratch();
    let platform = library_name();
    let cases: Vec<(&str, Vec<(String, String)>, String)> = vec![
        (
            "both",
            vec![
                (COMPONENT_FILE.into(), String::new()),
                (format!("{LIBRARY_DIR}/{platform}"), String::new()),
            ],
            "holds both plugin.wasm and lib/; a package is one tier".into(),
        ),
        (
            "elsewhere",
            vec![
                (format!("{LIBRARY_DIR}/plan9-mips.so"), String::new()),
                (format!("{LIBRARY_DIR}/beos-ppc.so"), String::new()),
            ],
            format!(
                "no lib/{platform} for this platform; the package holds beos-ppc.so, plan9-mips.so"
            ),
        ),
        (
            "commands without code",
            vec![(
                MANIFEST_FILE.into(),
                text(
                    "com.test.commands",
                    "C",
                    "1.0.0",
                    "1",
                    "[[commands]]\nid = \"c.run\"\ntitle = \"Run\"",
                ),
            )],
            format!("declares commands but holds no code: add plugin.wasm or lib/{platform}"),
        ),
    ];
    for (case, files, want) in cases {
        let dir = root.path().join(case);
        write(
            &dir,
            MANIFEST_FILE,
            &text("com.test.tier", "T", "1.0.0", "1", ""),
        );
        for (path, contents) in &files {
            write(&dir, path, contents);
        }
        assert_eq!(
            Package::open(&dir).expect_err(case),
            PackageError {
                path: dir.clone(),
                reason: want,
            },
            "{case}"
        );
    }

    let empty = root.path().join("empty-lib");
    write(
        &empty,
        MANIFEST_FILE,
        &text("com.test.empty", "E", "1.0.0", "1", ""),
    );
    std::fs::create_dir_all(empty.join(LIBRARY_DIR)).expect("lib dir");
    assert_eq!(
        Package::open(&empty).expect_err("empty lib").reason,
        format!("lib/ is empty; this platform loads lib/{platform}")
    );
}

#[test]
fn discovery_reads_every_package_in_path_order() {
    let root = scratch();
    let dir = root.path();
    write(
        &dir.join("b-tools"),
        MANIFEST_FILE,
        &text("com.test.b", "B", "1.0.0", "1", ""),
    );
    write(
        &dir.join("a-tools"),
        MANIFEST_FILE,
        &text("com.test.a", "A", "2.0.0", "1", ""),
    );
    write(&dir.join("c-broken"), MANIFEST_FILE, "[plugin");
    std::fs::create_dir_all(dir.join("d-empty")).expect("empty dir");
    write(
        &dir.join(".hidden"),
        MANIFEST_FILE,
        &text("com.test.hidden", "H", "1.0.0", "1", ""),
    );
    write(dir, "README.txt", "not a plugin");
    write(dir, "old.so", "");
    write(dir, "packed.ariaplugin", "");

    let found = discover(dir);
    let summary: Vec<Result<&str, &Path>> = found
        .iter()
        .map(|result| match result {
            Ok(package) => Ok(package.manifest.id.as_str()),
            Err(err) => Err(err.path.as_path()),
        })
        .collect();
    let manifest_path = dir.join("c-broken").join(MANIFEST_FILE);
    let empty = dir.join("d-empty");
    let library = dir.join("old.so");
    let archive = dir.join("packed.ariaplugin");
    assert_eq!(
        summary,
        [
            Ok("com.test.a"),
            Ok("com.test.b"),
            Err(manifest_path.as_path()),
            Err(empty.as_path()),
            Err(library.as_path()),
            Err(archive.as_path()),
        ]
    );
    let reasons: Vec<&str> = found
        .iter()
        .filter_map(|result| result.as_ref().err())
        .map(|err| err.reason.as_str())
        .collect();
    assert_eq!(reasons[1], "no plugin.toml");
    assert_eq!(
        reasons[2],
        format!(
            "a plugin library loads from a package: put it at <package>/lib/{} beside a plugin.toml",
            library_name()
        )
    );
    assert_eq!(
        reasons[3],
        "an archive is not loaded in place; install it from the Plugins panel"
    );

    assert!(
        discover(&dir.join("absent")).is_empty(),
        "no directory, no packages"
    );
}

#[test]
fn a_package_mounts_under_the_services_its_permissions_grant() {
    let root = scratch();
    let cases = [
        ("", vec!["SettingsService", "SettingsSchema"]),
        (
            "[permissions]\nproject = \"read\"",
            vec!["SettingsService", "SettingsSchema", "ProjectHost"],
        ),
        (
            "[permissions]\nproject = \"read-write\"\nmedia = \"read\"\ngpu = true",
            vec![
                "SettingsService",
                "SettingsSchema",
                "ProjectHost",
                "DecoderService",
                "AudioHost",
                "GpuContext",
                "RenderHost",
            ],
        ),
    ];
    for (index, (permissions, uses)) in cases.into_iter().enumerate() {
        let dir = root.path().join(index.to_string());
        let rest = format!("requires = [\"com.test.base >=1\", \"com.test.other\"]\n{permissions}");
        write(
            &dir,
            MANIFEST_FILE,
            &text("com.test.granted", "Granted", "1.2.3", "1", &rest),
        );
        let package = Package::open(&dir).expect("opens");
        let manifest = package.plugin_manifest();
        assert_eq!(
            manifest,
            PluginManifest {
                id: "com.test.granted".into(),
                name: "Granted".into(),
                version: "1.2.3".into(),
                requires: Names::Owned(vec!["com.test.base".into(), "com.test.other".into()]),
                uses: Names::Owned(uses.iter().map(|name| (*name).to_owned()).collect()),
            },
            "{permissions}"
        );
        assert_eq!(
            package.data_dir(Path::new("profile")),
            Path::new("profile")
                .join("plugin-data")
                .join("com.test.granted")
        );
    }
}
