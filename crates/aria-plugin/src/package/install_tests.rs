//! Tests for installing and removing package archives.
//!
//! Closes the class "an install changes the plugin folder other than as
//! confirmed": a staged install is invisible to discovery until committed and
//! leaves nothing when dropped, an update replaces the installed package in
//! its own folder and asks only for what it adds, and a removal takes the
//! package and keeps its storage. It does not cover mounting the installed
//! package; the host does that after a commit.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::*;
use crate::package::{Access, Tier, library_name};

const ID: &str = "com.example.tools";

fn manifest(version: &str, permissions: &str) -> String {
    format!(
        "[plugin]\nid = {ID:?}\nname = \"Tools\"\nversion = {version:?}\napi = 1\n\
         publisher = \"Example\"\n{permissions}"
    )
}

/// An archive at `path` holding `files` (name, contents) at its root.
fn archive(path: &Path, files: &[(&str, &str)]) {
    let mut zip = ZipWriter::new(File::create(path).expect("create archive"));
    for (name, contents) in files {
        zip.start_file(*name, SimpleFileOptions::default())
            .expect("start file");
        zip.write_all(contents.as_bytes()).expect("write entry");
    }
    zip.finish().expect("finish");
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

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().expect("scratch"),
        }
    }

    fn plugins(&self) -> std::path::PathBuf {
        self.root.path().join("plugins")
    }

    fn archive(&self, name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let path = self.root.path().join(name);
        archive(&path, files);
        path
    }

    fn install(&self, name: &str, files: &[(&str, &str)]) -> Package {
        stage(&self.archive(name, files), &self.plugins())
            .expect("stages")
            .commit()
            .expect("commits")
    }
}

#[test]
fn a_new_install_asks_for_everything_and_is_hidden_until_committed() {
    let fixture = Fixture::new();
    let asked = "[permissions]\nproject = \"read\"\nnet = [\"api.example.com\"]\n";
    let path = fixture.archive(
        "tools.ariaplugin",
        &[
            ("plugin.toml", &manifest("1.0.0", asked)),
            ("shaders/a.wgsl", "// a\n"),
        ],
    );

    let staged = stage(&path, &fixture.plugins()).expect("stages");
    assert!(staged.needs_confirmation());
    assert!(staged.replaces().is_none());
    assert!(!staged.is_native());
    assert_eq!(staged.asks().project, Access::Read);
    assert_eq!(staged.asks().net, vec!["api.example.com".to_owned()]);
    assert_eq!(staged.package().manifest.publisher, "Example");
    assert!(
        discover(&fixture.plugins()).is_empty(),
        "a staged package is not discovered"
    );

    let installed = staged.commit().expect("commits");
    let target = fixture.plugins().join(ID);
    assert_eq!(installed.dir, target);
    assert_eq!(installed.tier, Tier::Data);
    assert_eq!(entries(&fixture.plugins()), vec![ID.to_owned()]);
    assert_eq!(
        fs::read_to_string(target.join("shaders/a.wgsl")).expect("installed file"),
        "// a\n"
    );
    let found: Vec<_> = discover(&fixture.plugins())
        .into_iter()
        .map(|package| package.expect("reads").manifest.id)
        .collect();
    assert_eq!(found, vec![ID.to_owned()]);
}

#[test]
fn a_dropped_install_leaves_the_plugin_folder_as_it_was() {
    let fixture = Fixture::new();
    fixture.install("v1.ariaplugin", &[("plugin.toml", &manifest("1.0.0", ""))]);
    let before = entries(&fixture.plugins());

    let path = fixture.archive("v2.ariaplugin", &[("plugin.toml", &manifest("2.0.0", ""))]);
    drop(stage(&path, &fixture.plugins()).expect("stages"));

    assert_eq!(entries(&fixture.plugins()), before);
    let installed = Package::open(&fixture.plugins().join(ID)).expect("still installed");
    assert_eq!(installed.manifest.version, "1.0.0");
}

#[test]
fn an_update_replaces_the_package_in_its_own_folder() {
    let fixture = Fixture::new();
    // Installed by hand under a folder that is not its id.
    let by_hand = fixture.plugins().join("tools-dev");
    fs::create_dir_all(by_hand.join("shaders")).expect("mkdir");
    fs::write(by_hand.join("plugin.toml"), manifest("1.0.0", "")).expect("write");
    fs::write(by_hand.join("shaders/old.wgsl"), "// old\n").expect("write");

    let path = fixture.archive(
        "v2.ariaplugin",
        &[
            ("plugin.toml", &manifest("2.0.0", "")),
            ("shaders/new.wgsl", "// new\n"),
        ],
    );
    let staged = stage(&path, &fixture.plugins()).expect("stages");
    assert_eq!(
        staged.replaces().map(|old| old.version.as_str()),
        Some("1.0.0")
    );
    assert!(staged.asks().is_empty());
    assert!(!staged.needs_confirmation(), "nothing new to grant");

    let installed = staged.commit().expect("commits");
    assert_eq!(installed.dir, by_hand);
    assert_eq!(installed.manifest.version, "2.0.0");
    assert_eq!(entries(&fixture.plugins()), vec!["tools-dev".to_owned()]);
    assert_eq!(
        entries(&by_hand.join("shaders")),
        vec!["new.wgsl".to_owned()]
    );
}

#[test]
fn an_update_asks_only_for_what_it_adds() {
    let fixture = Fixture::new();
    fixture.install(
        "v1.ariaplugin",
        &[(
            "plugin.toml",
            &manifest(
                "1.0.0",
                "[permissions]\nproject = \"read\"\nnet = [\"a.example.com\"]\n",
            ),
        )],
    );
    let path = fixture.archive(
        "v2.ariaplugin",
        &[(
            "plugin.toml",
            &manifest(
                "1.1.0",
                "[permissions]\nproject = \"read\"\nnet = [\"a.example.com\", \"b.example.com\"]\n\
                 clipboard = true\n",
            ),
        )],
    );
    let staged = stage(&path, &fixture.plugins()).expect("stages");
    assert!(staged.needs_confirmation());
    assert_eq!(
        staged.asks().describe(),
        vec![
            "Connect to b.example.com".to_owned(),
            "Read the clipboard".to_owned()
        ]
    );
}

#[test]
fn a_native_package_is_reported_as_native() {
    let fixture = Fixture::new();
    let library = format!("lib/{}", library_name());
    let path = fixture.archive(
        "native.ariaplugin",
        &[
            ("plugin.toml", &manifest("1.0.0", "")),
            (&library, "not loaded"),
        ],
    );
    let staged = stage(&path, &fixture.plugins()).expect("stages");
    assert!(staged.is_native());
    let installed = staged.commit().expect("commits");
    assert_eq!(
        installed.tier,
        Tier::Native {
            library: fixture.plugins().join(ID).join(&library)
        }
    );
}

#[test]
fn a_folder_in_the_way_refuses_the_install_and_is_left_alone() {
    let fixture = Fixture::new();
    let blocker = fixture.plugins().join(ID);
    fs::create_dir_all(&blocker).expect("mkdir");
    fs::write(blocker.join("notes.txt"), "mine").expect("write");

    let path = fixture.archive(
        "tools.ariaplugin",
        &[("plugin.toml", &manifest("1.0.0", ""))],
    );
    let err = stage(&path, &fixture.plugins()).expect_err("in the way");
    assert_eq!(err.path, blocker);
    assert_eq!(
        err.reason,
        format!(
            "is in the way of installing {ID}: it holds no readable package; move or delete it"
        )
    );
    assert_eq!(entries(&fixture.plugins()), vec![ID.to_owned()]);
    assert_eq!(entries(&blocker), vec!["notes.txt".to_owned()]);
}

#[test]
fn a_refused_archive_stages_nothing() {
    let fixture = Fixture::new();
    let path = fixture.archive("bad.ariaplugin", &[("../escape", "x")]);
    stage(&path, &fixture.plugins()).expect_err("refused");
    assert!(entries(&fixture.plugins()).is_empty());
    assert!(!fixture.root.path().join("escape").exists());
}

#[test]
fn uninstall_removes_the_package_and_keeps_its_storage() {
    let fixture = Fixture::new();
    let installed = fixture.install("v1.ariaplugin", &[("plugin.toml", &manifest("1.0.0", ""))]);
    let storage = installed.data_dir(fixture.root.path());
    fs::create_dir_all(&storage).expect("mkdir");
    fs::write(storage.join("state.json"), "{}").expect("write");

    let removed = uninstall(&fixture.plugins(), ID).expect("uninstalls");
    assert_eq!(removed, fixture.plugins().join(ID));
    assert!(entries(&fixture.plugins()).is_empty());
    assert!(storage.join("state.json").is_file());

    let err = uninstall(&fixture.plugins(), ID).expect_err("gone");
    assert_eq!(err.reason, format!("no installed package is {ID}"));
}
