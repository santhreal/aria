//! `fs-read`, `fs-read-at`, `fs-len` and `fs-write` against `permissions.fs`.
//!
//! Closes the class where a scope the manifest declares and the consent
//! dialog lists grants nothing to a component: every scope a WebAssembly
//! plugin can declare is checked here through `check_fs_path`, the one check
//! all four imports call, against the folder of a saved project, an unsaved
//! project, and paths outside the scope or escaping it with `..`. It does not
//! catch a symbolic link inside a granted folder that points outside it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use aria_engine::{Engine, EngineConfig};
use aria_plugin::package::Package;
use aria_plugin::{Origin, PluginContext, ProjectHost, Registration};
use parking_lot::Mutex;
use wasmtime::StoreLimitsBuilder;

use super::super::{CallPhase, HostState};

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    ctx: PluginContext,
    host: Arc<ProjectHost>,
    _registration: Registration,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let ctx = PluginContext::new();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let host = Arc::new(ProjectHost::new(Arc::new(Mutex::new(engine)), &ctx));
    let registration = ctx.register_arc(Arc::clone(&host));
    Fixture {
        _dir: dir,
        root,
        ctx,
        host,
        _registration: registration,
    }
}

impl Fixture {
    fn state(&self, fs: &str) -> HostState {
        let package_dir = self.root.join("package");
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::write(
            package_dir.join("plugin.toml"),
            format!(
                "[plugin]\nid = \"com.example.fs\"\nname = \"Fs\"\nversion = \"0.1.0\"\napi = 1\n\
                 publisher = \"Test\"\ndescription = \"Test\"\nlicense = \"MIT\"\n\n\
                 [permissions]\nfs = [{fs}]\n"
            ),
        )
        .unwrap();
        HostState {
            shared: Weak::new(),
            package: Package::open(&package_dir).unwrap(),
            limiter: StoreLimitsBuilder::new().build(),
            phase: CallPhase::Command,
            active_ctx: Some(self.ctx.clone()),
            active_read_path: None,
            active_export: None,
            active_encode_path: None,
        }
    }

    /// Save the open project into `<root>/project/edit.aria` and return the
    /// folder.
    fn save_project(&self) -> PathBuf {
        let folder = self.root.join("project");
        std::fs::create_dir_all(&folder).unwrap();
        self.host
            .save_as(&Origin::Editor, &folder.join("edit.aria"))
            .unwrap();
        folder
    }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn the_project_scope_reads_and_writes_files_in_the_project_folder() {
    let fixture = fixture();
    let folder = fixture.save_project();
    let lut = folder.join("looks").join("warm.cube");
    std::fs::create_dir_all(lut.parent().unwrap()).unwrap();
    std::fs::write(&lut, b"LUT_3D_SIZE 2\n").unwrap();
    let mut state = fixture.state("\"project\"");

    assert_eq!(state.host_fs_read(text(&lut)).unwrap(), b"LUT_3D_SIZE 2\n");
    assert_eq!(state.host_fs_len(text(&lut)).unwrap(), 14);
    assert_eq!(state.host_fs_read_at(text(&lut), 4, 3).unwrap(), b"3D_");
    let written = folder.join("renders").join("notes.txt");
    state
        .host_fs_write(text(&written), b"kept".to_vec())
        .unwrap();
    assert_eq!(std::fs::read(&written).unwrap(), b"kept");
}

#[test]
fn the_project_scope_covers_nothing_outside_the_project_folder() {
    let fixture = fixture();
    let folder = fixture.save_project();
    let outside = fixture.root.join("outside.txt");
    std::fs::write(&outside, b"secret").unwrap();
    let mut state = fixture.state("\"project\"");

    for path in [
        outside.clone(),
        folder.join("..").join("outside.txt"),
        PathBuf::from("outside.txt"),
    ] {
        let error = state.host_fs_read(text(&path)).unwrap_err();
        assert!(error.starts_with("permission denied"), "{path:?}: {error}");
    }
    let error = state
        .host_fs_write(text(&outside), b"overwritten".to_vec())
        .unwrap_err();
    assert!(error.starts_with("permission denied"), "{error}");
    assert_eq!(std::fs::read(&outside).unwrap(), b"secret");
}

#[test]
fn the_project_scope_covers_nothing_before_the_project_is_saved() {
    let fixture = fixture();
    let file = fixture.root.join("anywhere.txt");
    std::fs::write(&file, b"x").unwrap();
    let mut state = fixture.state("\"project\"");

    assert!(state.host_fs_read(text(&file)).is_err());
}

#[test]
fn every_declarable_scope_is_checked() {
    let fixture = fixture();
    let folder = fixture.save_project();
    let inside = folder.join("clip.txt");
    std::fs::write(&inside, b"in").unwrap();
    let shared = fixture.root.join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    let granted = shared.join("grain.png");
    std::fs::write(&granted, b"png").unwrap();
    let path_scope = format!("{:?}", text(&shared));

    for (scopes, path, readable) in [
        (String::new(), &inside, false),
        ("\"pick\"".to_owned(), &inside, false),
        ("\"project\"".to_owned(), &inside, true),
        (path_scope.clone(), &granted, true),
        (path_scope, &inside, false),
        ("\"project\"".to_owned(), &granted, false),
    ] {
        let mut state = fixture.state(&scopes);
        assert_eq!(
            state.host_fs_read(text(path)).is_ok(),
            readable,
            "fs = [{scopes}] reading {path:?}"
        );
    }
}
