//! Installed packages through the one mount path: discovery, the disabled
//! list, install, update, reload, removal, the reload on a change on disk and
//! the development package.
//!
//! The class this closes: the plugin folder and the registry disagreeing about
//! what an installed package is doing. Every operation is checked against
//! `ctx.is_mounted` and against what a fresh `open` of the same folder reads,
//! which is what the next start sees. Data and WASM packages are driven here;
//! a native package needs a cdylib build and is driven by `aria-plugin`'s
//! `native_abi` tests through the same `load_library`. Here a native package
//! holds bytes that do not load, which is enough to show which changes the
//! watch acts on.
//!
//! Not caught here: the half-second poll in the shell that calls
//! `reload_changed`, and a file system whose timestamps do not move on a
//! write.

use std::fs;
use std::path::{Path, PathBuf};

use aria_plugin::PluginContext;
use aria_plugin::package;

use super::*;

const ID: &str = "com.example.looks";

fn manifest(version: &str) -> String {
    format!(
        "[plugin]\nid = {ID:?}\nname = \"Looks\"\nversion = {version:?}\napi = 1\n\
         publisher = \"Example\"\n"
    )
}

/// A package folder `<data>/plugins/<folder>` holding `files`.
fn write_package(data: &Path, folder: &str, files: &[(&str, &[u8])]) -> PathBuf {
    write_files(&external_plugin_dir(data).join(folder), files)
}

/// A development package folder outside the plugin folder, holding `files`.
fn write_dev(data: &Path, files: &[(&str, &[u8])]) -> PathBuf {
    write_files(&data.join("source-tree"), files)
}

fn write_files(dir: &Path, files: &[(&str, &[u8])]) -> PathBuf {
    for (name, bytes) in files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap_or(dir)).expect("package dir");
        fs::write(path, bytes).expect("package file");
    }
    dir.to_owned()
}

/// The component the WASM tier tests load.
fn wasm_fixture() -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../aria-plugin-wasm/tests/fixtures/example_plugin.wasm"),
    )
    .expect("the component fixture")
}

/// A poll time past every settle window, so a poll reads the files as they
/// are.
fn later() -> SystemTime {
    SystemTime::now() + Duration::from_secs(60)
}

/// An archive of a data package at version `version`, outside the plugin
/// folder.
fn archive(data: &Path, version: &str) -> PathBuf {
    let source = data.join("source").join(version);
    fs::create_dir_all(&source).expect("source dir");
    fs::write(source.join(package::MANIFEST_FILE), manifest(version)).expect("manifest");
    let out = data.join(format!("looks-{version}.{}", package::ARCHIVE_EXTENSION));
    package::pack(&source, &out).expect("packs");
    out
}

fn mounted_version(ctx: &PluginContext, id: &str) -> Option<String> {
    ctx.mounted()
        .into_iter()
        .find(|info| info.manifest.id == id)
        .map(|info| info.manifest.version.to_string())
}

#[test]
fn a_disabled_package_stays_unmounted_across_a_restart_until_enabled() {
    let data = crate::scratch_pool::scratch("installed-disable");
    write_package(
        &data,
        "looks",
        &[(package::MANIFEST_FILE, manifest("1.0.0").as_bytes())],
    );
    let ctx = PluginContext::new();
    let mut installed = InstalledPlugins::open(&data);
    assert!(installed.mount_enabled(&ctx).is_empty());
    assert!(ctx.is_mounted(ID));

    assert_eq!(installed.set_enabled(&ctx, ID, false), Ok(true));
    assert!(!ctx.is_mounted(ID));

    // The next start reads the choice and mounts nothing.
    let restarted = PluginContext::new();
    let mut reopened = InstalledPlugins::open(&data);
    assert!(reopened.is_disabled(ID));
    assert!(reopened.mount_enabled(&restarted).is_empty());
    assert!(!restarted.is_mounted(ID));

    assert_eq!(reopened.set_enabled(&restarted, ID, true), Ok(true));
    assert!(restarted.is_mounted(ID));
    assert!(!InstalledPlugins::open(&data).is_disabled(ID));
    // Asking for the state it is already in changes nothing.
    assert_eq!(reopened.set_enabled(&restarted, ID, true), Ok(false));
    assert!(
        reopened
            .set_enabled(&restarted, "com.example.absent", false)
            .is_err()
    );
}

#[test]
fn a_package_that_does_not_read_fails_under_its_folder_name() {
    let data = crate::scratch_pool::scratch("installed-broken");
    write_package(
        &data,
        "broken",
        &[(package::MANIFEST_FILE, b"this is [not toml")],
    );
    let ctx = PluginContext::new();
    let failures = InstalledPlugins::open(&data).mount_enabled(&ctx);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].id, "broken");
    assert_eq!(failures[0].name, "broken");
    assert!(!failures[0].reason.is_empty());
    assert!(ctx.failures().iter().any(|failure| failure.id == "broken"));
}

#[test]
fn an_install_mounts_and_an_update_replaces_the_mounted_version_in_place() {
    let data = crate::scratch_pool::scratch("installed-update");
    let ctx = PluginContext::new();
    let mut installed = InstalledPlugins::open(&data);

    let staged = installed.stage(&archive(&data, "1.0.0")).expect("stages");
    assert!(!ctx.is_mounted(ID), "a staged package runs nothing");
    installed.commit(&ctx, staged).expect("installs");
    assert_eq!(mounted_version(&ctx, ID).as_deref(), Some("1.0.0"));
    let folder = installed.package(ID).expect("installed").dir.clone();

    // A disabled package that is updated is enabled again: installing is
    // asking for it to run.
    assert_eq!(installed.set_enabled(&ctx, ID, false), Ok(true));
    let staged = installed.stage(&archive(&data, "2.0.0")).expect("stages");
    assert_eq!(
        staged.replaces().map(|old| old.version.as_str()),
        Some("1.0.0")
    );
    installed.commit(&ctx, staged).expect("updates");
    assert_eq!(mounted_version(&ctx, ID).as_deref(), Some("2.0.0"));
    assert!(!InstalledPlugins::open(&data).is_disabled(ID));

    let packages: Vec<_> = installed
        .packages()
        .iter()
        .filter_map(|entry| entry.as_ref().ok())
        .collect();
    assert_eq!(packages.len(), 1, "the update left a second package");
    assert_eq!(packages[0].dir, folder);
    assert_eq!(packages[0].manifest.version, "2.0.0");
}

#[test]
fn uninstall_unmounts_deletes_the_folder_and_keeps_the_storage() {
    let data = crate::scratch_pool::scratch("installed-uninstall");
    let ctx = PluginContext::new();
    let mut installed = InstalledPlugins::open(&data);
    let staged = installed.stage(&archive(&data, "1.0.0")).expect("stages");
    let package = installed.commit(&ctx, staged).expect("installs");
    let storage = package.data_dir(&data);
    fs::create_dir_all(&storage).expect("storage");
    fs::write(storage.join("state.json"), b"{}").expect("stored");
    assert_eq!(installed.set_enabled(&ctx, ID, false), Ok(true));

    let removed = installed.uninstall(&ctx, ID).expect("uninstalls");
    assert_eq!(removed, package.dir);
    assert!(!package.dir.exists());
    assert!(!ctx.is_mounted(ID));
    assert!(installed.package(ID).is_none());
    assert!(storage.join("state.json").exists(), "storage was deleted");
    let reopened = InstalledPlugins::open(&data);
    assert!(!reopened.is_disabled(ID), "the disabled list kept a ghost");
    assert!(reopened.packages().is_empty());
    assert!(installed.uninstall(&ctx, ID).is_err());
}

#[test]
fn reload_mounts_what_is_on_disk_now_and_refuses_a_disabled_package() {
    let data = crate::scratch_pool::scratch("installed-reload");
    let manifest_path = write_package(
        &data,
        "looks",
        &[(package::MANIFEST_FILE, manifest("1.0.0").as_bytes())],
    )
    .join(package::MANIFEST_FILE);
    let ctx = PluginContext::new();
    let mut installed = InstalledPlugins::open(&data);
    assert!(installed.mount_enabled(&ctx).is_empty());

    fs::write(&manifest_path, manifest("1.1.0")).expect("edit in place");
    installed.reload(&ctx, ID).expect("reloads");
    assert_eq!(mounted_version(&ctx, ID).as_deref(), Some("1.1.0"));

    assert_eq!(installed.set_enabled(&ctx, ID, false), Ok(true));
    let refused = installed.reload(&ctx, ID).expect_err("disabled");
    assert!(refused.contains("disabled"), "{refused}");
    assert!(!ctx.is_mounted(ID));
}

/// One test for every WASM step: the component registers an audio effect in a
/// catalog the process shares, so two tests mounting it at once collide.
#[test]
fn a_wasm_package_mounts_reloads_and_follows_its_component_on_disk() {
    let wasm = wasm_fixture();
    let id = "com.example.wasm-tools";
    let toml = format!(
        "[plugin]\nid = {id:?}\nname = \"Example WASM Tools\"\nversion = \"0.1.0\"\napi = 1\n\
         publisher = \"Example\"\n"
    );
    let data = crate::scratch_pool::scratch("installed-wasm");
    let dir = write_package(
        &data,
        "wasm-tools",
        &[
            (package::MANIFEST_FILE, toml.as_bytes()),
            (package::COMPONENT_FILE, &wasm),
        ],
    );
    let configured = PluginLimitSettings {
        wasm_fuel_per_call: 2_000_000_000,
        wasm_memory_mib: 128,
    };
    let mut settings = aria_settings::Settings::default();
    settings.plugin_limits = configured;
    aria_settings::save(&aria_settings::config_in(&data), &settings).expect("write config");
    let ctx = PluginContext::new();
    let mut installed = InstalledPlugins::open(&data);
    assert_eq!(installed.limits(), configured);
    assert!(matches!(
        installed.package(id).map(|package| &package.tier),
        Some(package::Tier::Wasm { .. })
    ));
    let failures = installed.mount_enabled(&ctx);
    assert!(failures.is_empty(), "{failures:?}");
    assert!(ctx.is_mounted(id));
    let registered = ctx
        .mounted()
        .into_iter()
        .find(|info| info.manifest.id == id)
        .map(|info| info.contributions.len())
        .unwrap_or_default();
    assert!(registered > 0, "the component registered nothing");

    installed.reload(&ctx, id).expect("reloads");
    assert!(ctx.is_mounted(id));

    fs::write(dir.join(package::COMPONENT_FILE), b"not a component").expect("break");
    let reloaded = installed.reload_changed(&ctx, later());
    assert_eq!(reloaded.len(), 1, "{reloaded:?}");
    assert_eq!(reloaded[0].id, id);
    assert!(reloaded[0].result.is_err(), "{reloaded:?}");
    assert!(!ctx.is_mounted(id));
    assert!(ctx.failures().iter().any(|failure| failure.id == id));

    fs::write(dir.join(package::COMPONENT_FILE), &wasm).expect("fix");
    let fixed = Reloaded {
        id: id.to_owned(),
        name: "Example WASM Tools".to_owned(),
        result: Ok(()),
    };
    assert_eq!(installed.reload_changed(&ctx, later()), [fixed]);
    assert!(ctx.is_mounted(id));
    assert!(!ctx.failures().iter().any(|failure| failure.id == id));

    // A change of limits remounts the mounted component under the new ones: a
    // memory too small to instantiate it fails the remount.
    assert!(
        installed.set_limits(&ctx, configured).is_empty(),
        "unchanged limits remount nothing"
    );
    let starved = PluginLimitSettings {
        wasm_memory_mib: 0,
        ..configured
    };
    let remounted = installed.set_limits(&ctx, starved);
    assert_eq!(remounted.len(), 1, "{remounted:?}");
    assert_eq!(remounted[0].id, id);
    assert!(remounted[0].result.is_err(), "{remounted:?}");
    assert!(!ctx.is_mounted(id));
    assert!(
        installed.set_limits(&ctx, configured).is_empty(),
        "a package that is not mounted is not remounted"
    );
    installed
        .reload(&ctx, id)
        .expect("mounts under the restored limits");
    assert!(ctx.is_mounted(id));

    // Listing an activation event defers the component: its declared command
    // is listed while none of it runs, and the first matching file loads it.
    fs::write(
        dir.join(package::MANIFEST_FILE),
        format!(
            "{toml}[activation]\non = [\"file:lazy\"]\n[[commands]]\n\
             id = \"example.add-marker\"\ntitle = \"Add Example Marker\"\n"
        ),
    )
    .expect("defer");
    installed.reload(&ctx, id).expect("reloads");
    assert!(ctx.is_mounted(id), "a mounted package mounts again at once");
    assert_eq!(installed.set_enabled(&ctx, id, false), Ok(true));
    assert_eq!(installed.set_enabled(&ctx, id, true), Ok(true));
    assert!(!ctx.is_mounted(id));
    assert!(ctx.is_waiting(id));
    assert!(
        ctx.commands()
            .iter()
            .any(|spec| spec.id == "example.add-marker")
    );
    let activated = ctx.activate_for_file(Path::new("take.LAZY"));
    assert_eq!(activated.len(), 1, "{activated:?}");
    assert_eq!(activated[0].result, Ok(()));
    assert!(ctx.is_mounted(id));
    assert!(!ctx.is_waiting(id));

    assert_eq!(installed.set_enabled(&ctx, id, false), Ok(true));
    assert!(!ctx.is_mounted(id));
}

/// A package with code that lists activation events waits for the first of
/// them. One that a package mounting at start requires, and the development
/// package, mount at start. Disable, enable, reload and uninstall stop or
/// restart the wait as they unmount or mount a package.
#[test]
fn a_package_with_activation_events_waits_for_the_first() {
    let data = crate::scratch_pool::scratch("installed-activation");
    let library = format!("{}/{}", package::LIBRARY_DIR, package::library_name());
    let manifest_of = |id: &str, rest: &str| {
        format!("[plugin]\nid = {id:?}\nname = {id:?}\nversion = \"1.0.0\"\napi = 1\n{rest}")
    };
    let lazy = manifest_of("com.example.lazy", "[activation]\non = [\"file:lazy\"]");
    let needed = manifest_of("com.example.needed", "[activation]\non = [\"file:needed\"]");
    let needs = manifest_of("com.example.needs", "requires = [\"com.example.needed\"]");
    write_package(
        &data,
        "lazy",
        &[
            (package::MANIFEST_FILE, lazy.as_bytes()),
            (&library, b"not a library"),
        ],
    );
    write_package(
        &data,
        "needed",
        &[
            (package::MANIFEST_FILE, needed.as_bytes()),
            (&library, b"not a library"),
        ],
    );
    write_package(
        &data,
        "a-needs",
        &[(package::MANIFEST_FILE, needs.as_bytes())],
    );
    let ctx = PluginContext::new();
    let mut installed = InstalledPlugins::open_developing(&data, None);

    // The data package mounts at start, after the package it requires, which
    // mounts at start for it; the bytes do not load, so both fail.
    let failures = installed.mount_enabled(&ctx);
    let failed: Vec<&str> = failures.iter().map(|failure| failure.id.as_str()).collect();
    assert_eq!(
        failed,
        ["com.example.needed", "com.example.needs"],
        "{failures:?}"
    );
    assert!(!ctx.is_waiting("com.example.needed"));
    assert!(ctx.is_waiting("com.example.lazy"));
    assert!(
        !ctx.failures()
            .iter()
            .any(|failure| failure.id == "com.example.lazy")
    );
    let package = installed.package("com.example.lazy").expect("read");
    assert!(installed.waits(package));

    assert_eq!(
        installed.set_enabled(&ctx, "com.example.lazy", false),
        Ok(true)
    );
    assert!(!ctx.is_waiting("com.example.lazy"));
    assert_eq!(
        installed.set_enabled(&ctx, "com.example.lazy", true),
        Ok(true)
    );
    assert!(ctx.is_waiting("com.example.lazy"));
    assert_eq!(
        installed.set_enabled(&ctx, "com.example.lazy", true),
        Ok(false),
        "a waiting package is enabled"
    );
    installed
        .reload(&ctx, "com.example.lazy")
        .expect("defers again");
    assert!(ctx.is_waiting("com.example.lazy"));

    // The first matching file loads it, and its failure is recorded as a
    // mount's is.
    let activated = ctx.activate_for_file(Path::new("take.LAZY"));
    assert_eq!(activated.len(), 1, "{activated:?}");
    assert_eq!(activated[0].id, "com.example.lazy");
    assert!(activated[0].result.is_err(), "{activated:?}");
    assert!(!ctx.is_waiting("com.example.lazy"));
    assert!(
        ctx.failures()
            .iter()
            .any(|failure| failure.id == "com.example.lazy")
    );
    assert!(ctx.activate_for_file(Path::new("take.lazy")).is_empty());

    installed
        .reload(&ctx, "com.example.lazy")
        .expect("defers again");
    assert!(ctx.is_waiting("com.example.lazy"));
    let removed = installed
        .uninstall(&ctx, "com.example.lazy")
        .expect("uninstalls");
    assert!(!removed.exists());
    assert!(!ctx.is_waiting("com.example.lazy"));

    // The development package mounts at start whatever it lists.
    let dev_lazy = manifest_of("com.example.dev-lazy", "[activation]\non = [\"file:dev\"]");
    let dev = write_dev(
        &data,
        &[
            (package::MANIFEST_FILE, dev_lazy.as_bytes()),
            (&library, b"not a library"),
        ],
    );
    let restarted = PluginContext::new();
    let failures = InstalledPlugins::open_developing(&data, Some(dev)).mount_enabled(&restarted);
    assert!(
        failures
            .iter()
            .any(|failure| failure.id == "com.example.dev-lazy"),
        "{failures:?}"
    );
    assert!(!restarted.is_waiting("com.example.dev-lazy"));
}

#[test]
fn the_development_package_replaces_an_installed_one_and_reloads_on_change() {
    let data = crate::scratch_pool::scratch("installed-dev");
    let installed_dir = write_package(
        &data,
        "looks",
        &[(package::MANIFEST_FILE, manifest("1.0.0").as_bytes())],
    );
    let dev = write_dev(
        &data,
        &[(package::MANIFEST_FILE, manifest("2.0.0").as_bytes())],
    );
    let ctx = PluginContext::new();
    let mut installed = InstalledPlugins::open_developing(&data, Some(dev.clone()));
    let with_id: Vec<&Path> = installed
        .packages()
        .iter()
        .filter_map(|entry| entry.as_ref().ok())
        .filter(|package| package.manifest.id == ID)
        .map(|package| package.dir.as_path())
        .collect();
    assert_eq!(with_id, [dev.as_path()]);
    assert!(installed.is_development(ID));
    assert!(installed.mount_enabled(&ctx).is_empty());
    assert_eq!(mounted_version(&ctx, ID).as_deref(), Some("2.0.0"));
    assert!(installed.reload_changed(&ctx, later()).is_empty());

    fs::write(dev.join(package::MANIFEST_FILE), manifest("2.0.10")).expect("edit");
    let looks = Reloaded {
        id: ID.to_owned(),
        name: "Looks".to_owned(),
        result: Ok(()),
    };
    assert_eq!(installed.reload_changed(&ctx, later()), [looks]);
    assert_eq!(mounted_version(&ctx, ID).as_deref(), Some("2.0.10"));
    assert!(
        installed.reload_changed(&ctx, later()).is_empty(),
        "the reload was not recorded, so it repeats"
    );

    let staged = installed.stage(&archive(&data, "3.0.0")).expect("stages");
    let refused = installed.commit(&ctx, staged).expect_err("development id");
    assert!(refused.contains("aria plugin dev"), "{refused}");
    let refused = installed.uninstall(&ctx, ID).expect_err("development id");
    assert!(refused.contains("nothing to uninstall"), "{refused}");
    assert!(dev.join(package::MANIFEST_FILE).is_file());
    assert!(installed_dir.join(package::MANIFEST_FILE).is_file());
    assert_eq!(mounted_version(&ctx, ID).as_deref(), Some("2.0.10"));
}

#[test]
fn a_change_younger_than_the_settle_time_waits_for_a_later_poll() {
    let data = crate::scratch_pool::scratch("installed-settle");
    let manifest_path = write_package(
        &data,
        "looks",
        &[(package::MANIFEST_FILE, manifest("1.0.0").as_bytes())],
    )
    .join(package::MANIFEST_FILE);
    let ctx = PluginContext::new();
    let mut installed = InstalledPlugins::open_developing(&data, None);
    assert!(installed.mount_enabled(&ctx).is_empty());

    fs::write(&manifest_path, manifest("1.0.10")).expect("edit");
    let written = fs::metadata(&manifest_path)
        .and_then(|meta| meta.modified())
        .expect("modification time");
    assert!(
        installed
            .reload_changed(&ctx, written + SETTLE / 2)
            .is_empty()
    );
    assert_eq!(mounted_version(&ctx, ID).as_deref(), Some("1.0.0"));
    let reloaded = installed.reload_changed(&ctx, written + SETTLE);
    assert_eq!(reloaded.len(), 1, "{reloaded:?}");
    assert_eq!(reloaded[0].result, Ok(()));
    assert_eq!(mounted_version(&ctx, ID).as_deref(), Some("1.0.10"));

    // A clock ahead of the poll would otherwise hold a reload until the two
    // agree.
    let ahead = Stamp(vec![Some((1, written + Duration::from_secs(5)))]);
    assert!(!ahead.settling(written));
    assert!(Stamp(vec![Some((1, written)), None]).settling(written));
}

#[test]
fn an_installed_native_package_waits_for_a_reload_by_hand_and_a_developed_one_does_not() {
    let data = crate::scratch_pool::scratch("installed-native-watch");
    let library = format!("{}/{}", package::LIBRARY_DIR, package::library_name());
    let installed_dir = write_package(
        &data,
        "looks",
        &[
            (package::MANIFEST_FILE, manifest("1.0.0").as_bytes()),
            (&library, b"not a library"),
        ],
    );
    let ctx = PluginContext::new();
    let mut installed = InstalledPlugins::open_developing(&data, None);
    assert!(matches!(
        installed.package(ID).map(|package| &package.tier),
        Some(Tier::Native { .. })
    ));
    assert_eq!(installed.mount_enabled(&ctx).len(), 1, "the bytes loaded");
    fs::write(
        installed_dir.join(&library),
        b"a rebuilt library, still not one",
    )
    .expect("rebuild");
    assert!(installed.reload_changed(&ctx, later()).is_empty());

    let dev = write_dev(
        &data,
        &[
            (package::MANIFEST_FILE, manifest("2.0.0").as_bytes()),
            (&library, b"not a library"),
        ],
    );
    let mut developing = InstalledPlugins::open_developing(&data, Some(dev.clone()));
    assert_eq!(developing.mount_enabled(&ctx).len(), 1, "the bytes loaded");
    fs::write(dev.join(&library), b"a rebuilt library, still not one").expect("rebuild");
    let reloaded = developing.reload_changed(&ctx, later());
    assert_eq!(reloaded.len(), 1, "{reloaded:?}");
    assert_eq!(reloaded[0].id, ID);
    assert!(reloaded[0].result.is_err(), "{reloaded:?}");
    assert!(ctx.failures().iter().any(|failure| failure.id == ID));
}

#[test]
fn a_package_that_stops_reading_fails_under_its_folder_and_mounts_once_fixed() {
    let data = crate::scratch_pool::scratch("installed-broken-watch");
    let manifest_path = write_package(
        &data,
        "looks",
        &[(package::MANIFEST_FILE, manifest("1.0.0").as_bytes())],
    )
    .join(package::MANIFEST_FILE);
    let ctx = PluginContext::new();
    let mut installed = InstalledPlugins::open_developing(&data, None);
    assert!(installed.mount_enabled(&ctx).is_empty());

    fs::write(&manifest_path, "this is [not toml").expect("break");
    let reloaded = installed.reload_changed(&ctx, later());
    assert_eq!(reloaded.len(), 1, "{reloaded:?}");
    assert_eq!(reloaded[0].id, "looks");
    assert!(reloaded[0].result.is_err(), "{reloaded:?}");
    assert!(!ctx.is_mounted(ID));
    assert!(ctx.failures().iter().any(|failure| failure.id == "looks"));
    assert!(installed.reload_changed(&ctx, later()).is_empty());

    fs::write(&manifest_path, manifest("1.0.10")).expect("fix");
    let reloaded = installed.reload_changed(&ctx, later());
    assert_eq!(reloaded.len(), 1, "{reloaded:?}");
    assert_eq!(reloaded[0].result, Ok(()));
    assert_eq!(mounted_version(&ctx, ID).as_deref(), Some("1.0.10"));
    assert!(ctx.failures().is_empty(), "{:?}", ctx.failures());
}

#[test]
fn a_disabled_package_is_not_watched_and_enabling_mounts_it_as_on_disk() {
    let data = crate::scratch_pool::scratch("installed-disabled-watch");
    let manifest_path = write_package(
        &data,
        "looks",
        &[(package::MANIFEST_FILE, manifest("1.0.0").as_bytes())],
    )
    .join(package::MANIFEST_FILE);
    let ctx = PluginContext::new();
    let mut installed = InstalledPlugins::open_developing(&data, None);
    assert!(installed.mount_enabled(&ctx).is_empty());
    assert_eq!(installed.set_enabled(&ctx, ID, false), Ok(true));

    fs::write(&manifest_path, manifest("1.0.10")).expect("edit");
    assert!(installed.reload_changed(&ctx, later()).is_empty());
    assert!(!ctx.is_mounted(ID));

    assert_eq!(installed.set_enabled(&ctx, ID, true), Ok(true));
    assert_eq!(mounted_version(&ctx, ID).as_deref(), Some("1.0.10"));
    assert!(
        installed.reload_changed(&ctx, later()).is_empty(),
        "enabling did not record what it mounted"
    );
}

#[test]
fn a_removed_package_unmounts_and_is_reported_once() {
    let data = crate::scratch_pool::scratch("installed-removed-watch");
    let dir = write_package(
        &data,
        "looks",
        &[(package::MANIFEST_FILE, manifest("1.0.0").as_bytes())],
    );
    let ctx = PluginContext::new();
    let mut installed = InstalledPlugins::open_developing(&data, None);
    assert!(installed.mount_enabled(&ctx).is_empty());

    fs::remove_dir_all(&dir).expect("remove");
    let removed = Reloaded {
        id: ID.to_owned(),
        name: ID.to_owned(),
        result: Err(format!("{} was removed", dir.display())),
    };
    assert_eq!(installed.reload_changed(&ctx, later()), [removed]);
    assert!(!ctx.is_mounted(ID));
    assert!(installed.package(ID).is_none());
    assert!(installed.reload_changed(&ctx, later()).is_empty());
}

/// The settings defaults restate the WASM host's, so the two cannot drift
/// apart unseen; a config that does not read keeps the host's defaults rather
/// than failing every WASM mount.
#[test]
fn wasm_limits_map_the_settings_and_an_unreadable_config_keeps_the_defaults() {
    assert_eq!(
        wasm_limits(PluginLimitSettings::default()),
        Limits::default()
    );
    assert_eq!(
        wasm_limits(PluginLimitSettings {
            wasm_fuel_per_call: 7_000_000,
            wasm_memory_mib: 32,
        }),
        Limits {
            fuel_per_call: 7_000_000,
            memory_bytes: 32 * 1024 * 1024,
            call_deadline: aria_plugin_wasm::DEFAULT_CALL_DEADLINE,
        }
    );
    let data = crate::scratch_pool::scratch("installed-limits-config");
    fs::create_dir_all(&data).expect("data dir");
    fs::write(aria_settings::config_in(&data), "[plugin_limits\n").expect("config");
    assert_eq!(
        InstalledPlugins::open_developing(&data, None).limits(),
        PluginLimitSettings::default()
    );
}
