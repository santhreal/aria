//! Tests for `.ariaplugin` archives.
//!
//! Closes the class "unpacking an archive writes outside its folder, or
//! writes something other than the package's files": every hostile entry
//! shape is a row that must refuse the whole archive and leave nothing on
//! disk, and a packed package unpacks byte for byte. It does not cover what
//! the unpacked code does; the tier loaders own that.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::*;
use crate::package::Tier;

const MANIFEST: &str = "[plugin]\nid = \"com.example.tools\"\nname = \"Tools\"\n\
                        version = \"1.2.0\"\napi = 1\n";

fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().expect("scratch dir")
}

fn write(dir: &Path, relative: &str, contents: &[u8]) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().expect("a parent")).expect("create parent");
    fs::write(path, contents).expect("write");
}

/// An archive holding `files` (name, bytes) and the symbolic links `links`
/// (name, target), in that order.
fn archive(path: &Path, files: &[(&str, &[u8])], links: &[(&str, &str)]) {
    let mut zip = ZipWriter::new(File::create(path).expect("create archive"));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in files {
        if let Some(dir) = name.strip_suffix('/') {
            zip.add_directory(dir, options).expect("add directory");
        } else {
            zip.start_file(*name, options).expect("start file");
            zip.write_all(bytes).expect("write entry");
        }
    }
    for (name, target) in links {
        zip.add_symlink(*name, *target, options).expect("add link");
    }
    zip.finish().expect("finish archive");
}

/// Every regular file under `dir`, relative and sorted, with its bytes.
fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let relative = path.strip_prefix(root).expect("under root");
                let name = relative.to_string_lossy().replace('\\', "/");
                out.push((name, fs::read(&path).expect("read file")));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn a_packed_package_unpacks_byte_for_byte_without_hidden_files() {
    let root = scratch();
    let source = root.path().join("tools");
    write(&source, "plugin.toml", MANIFEST.as_bytes());
    write(&source, "shaders/tint.wgsl", b"// tint\n");
    write(&source, "data/table.bin", &(0..=255).collect::<Vec<u8>>());
    write(&source, ".git/HEAD", b"ref: refs/heads/main\n");
    write(&source, "shaders/.swap", b"editor state");
    let out = root.path().join("tools.ariaplugin");

    let packed = pack(&source, &out).expect("packs");
    assert_eq!(packed.manifest.id, "com.example.tools");
    assert!(!root.path().join("tools.ariaplugin.partial").exists());

    let into = root.path().join("unpacked");
    let unpacked = unpack(&out, &into).expect("unpacks");
    assert_eq!(unpacked.manifest, packed.manifest);
    assert_eq!(unpacked.tier, Tier::Data);
    assert_eq!(unpacked.dir, into);
    let expected: Vec<_> = tree(&source)
        .into_iter()
        .filter(|(name, _)| !name.starts_with(".git/") && name != "shaders/.swap")
        .collect();
    assert_eq!(tree(&into), expected);
}

/// A package directory that is also its plugin's crate packs the package and
/// none of the crate: every source entry, at the top level only, and any
/// archive a previous pack wrote into it. Without a `Cargo.toml` the same
/// names are package files and stay.
#[test]
fn pack_leaves_out_the_crate_and_earlier_archives_of_a_source_directory() {
    let root = scratch();
    let source = root.path().join("tools");
    write(&source, "plugin.toml", MANIFEST.as_bytes());
    write(&source, "shaders/tint.wgsl", b"// tint\n");
    write(
        &source,
        "data/src/kept.txt",
        b"nested src is package data\n",
    );
    write(
        &source,
        "com.example.tools-1.1.0.ariaplugin",
        b"an earlier pack",
    );
    let crate_files = [
        "Cargo.toml",
        "Cargo.lock",
        "src/lib.rs",
        "wit/plugin.wit",
        "target/debug/libtools.so",
    ];
    for name in crate_files {
        write(&source, name, b"crate file");
    }
    assert_eq!(
        crate_files.map(|name| name.split('/').next().unwrap_or(name)),
        SOURCE_ENTRIES,
        "every source entry has a row"
    );
    let out = root.path().join("tools.ariaplugin");
    pack(&source, &out).expect("packs");
    let into = root.path().join("unpacked");
    unpack(&out, &into).expect("unpacks");
    let names: Vec<String> = tree(&into).into_iter().map(|(name, _)| name).collect();
    assert_eq!(
        names,
        ["data/src/kept.txt", "plugin.toml", "shaders/tint.wgsl"]
    );

    fs::remove_file(source.join("Cargo.toml")).expect("remove the crate marker");
    let data_out = root.path().join("data.ariaplugin");
    pack(&source, &data_out).expect("packs");
    let data_into = root.path().join("data-unpacked");
    unpack(&data_out, &data_into).expect("unpacks");
    let names: Vec<String> = tree(&data_into).into_iter().map(|(name, _)| name).collect();
    assert!(names.contains(&"src/lib.rs".to_owned()), "{names:?}");
    assert!(names.contains(&"Cargo.lock".to_owned()), "{names:?}");
    assert!(
        !names.iter().any(|name| name.ends_with(".ariaplugin")),
        "an archive is never packed: {names:?}"
    );
}

#[test]
fn pack_refuses_what_is_not_a_package_and_a_linked_file() {
    let root = scratch();
    let empty = root.path().join("empty");
    fs::create_dir_all(&empty).expect("mkdir");
    let out = root.path().join("out.ariaplugin");
    let err = pack(&empty, &out).expect_err("no manifest");
    assert_eq!(err.path, empty);
    assert!(!out.exists());

    #[cfg(unix)]
    {
        let source = root.path().join("linked");
        write(&source, "plugin.toml", MANIFEST.as_bytes());
        std::os::unix::fs::symlink("/etc/hostname", source.join("host")).expect("symlink");
        let err = pack(&source, &out).expect_err("a link");
        assert_eq!(err.path, source.join("host"));
        assert_eq!(
            err.reason,
            "a package archive holds no symbolic links; copy the target in"
        );
        assert!(!out.exists());
        assert!(!root.path().join("out.ariaplugin.partial").exists());
    }
}

#[test]
fn a_package_in_one_top_folder_unpacks_to_its_contents() {
    let root = scratch();
    let path = root.path().join("nested.ariaplugin");
    archive(
        &path,
        &[
            ("tools/", b""),
            ("tools/plugin.toml", MANIFEST.as_bytes()),
            ("tools/shaders/a.wgsl", b"// a\n"),
        ],
        &[],
    );
    let into = root.path().join("into");
    let package = unpack(&path, &into).expect("unpacks");
    assert_eq!(package.manifest.version, "1.2.0");
    assert_eq!(
        tree(&into),
        vec![
            ("plugin.toml".to_owned(), MANIFEST.as_bytes().to_vec()),
            ("shaders/a.wgsl".to_owned(), b"// a\n".to_vec()),
        ]
    );
}

#[test]
fn every_hostile_archive_is_refused_and_leaves_nothing() {
    let manifest = MANIFEST.as_bytes();
    #[rustfmt::skip]
    let cases: &[(&str, &[(&str, &[u8])], &[(&str, &str)], &str)] = &[
        ("parent", &[("plugin.toml", manifest), ("../escape", b"x")], &[],
            "\"../escape\" is not a relative path inside the package"),
        ("inner parent", &[("plugin.toml", manifest), ("a/../../escape", b"x")], &[],
            "\"a/../../escape\" is not a relative path inside the package"),
        ("absolute", &[("plugin.toml", manifest), ("/tmp/escape", b"x")], &[],
            "\"/tmp/escape\" is not a relative path inside the package"),
        ("current", &[("plugin.toml", manifest), ("./x", b"x")], &[],
            "\"./x\" is not a relative path inside the package"),
        ("backslash", &[("plugin.toml", manifest), ("..\\escape", b"x")], &[],
            "\"..\\\\escape\" is not a relative path inside the package"),
        ("drive", &[("plugin.toml", manifest), ("C:/escape", b"x")], &[],
            "\"C:/escape\" is not a relative path inside the package"),
        ("double slash", &[("plugin.toml", manifest), ("a//b", b"x")], &[],
            "\"a//b\" is not a relative path inside the package"),
        ("link", &[("plugin.toml", manifest)], &[("lib", "/usr/lib")],
            "lib: an archive holds no symbolic links"),
        ("no manifest", &[("shaders/a.wgsl", b"x")], &[],
            "holds no plugin.toml at its root or in one top folder"),
        ("two top folders", &[("a/plugin.toml", manifest), ("b/x", b"x")], &[],
            "holds no plugin.toml at its root or in one top folder"),
        ("manifest too deep", &[("a/b/plugin.toml", manifest)], &[],
            "holds no plugin.toml at its root or in one top folder"),
        ("empty", &[], &[],
            "holds no plugin.toml at its root or in one top folder"),
    ];
    for (label, files, links, reason) in cases {
        let root = scratch();
        let path = root.path().join("hostile.ariaplugin");
        archive(&path, files, links);
        let into = root.path().join("into");
        let err = unpack(&path, &into).expect_err(label);
        assert_eq!(err.path, path, "{label}");
        assert_eq!(err.reason, *reason, "{label}");
        assert!(!into.exists(), "{label}: the unpack folder is removed");
        assert!(
            !root.path().join("escape").exists(),
            "{label}: nothing escaped"
        );
    }
}

#[test]
fn a_manifest_the_host_refuses_removes_the_unpacked_files() {
    let root = scratch();
    let path = root.path().join("bad.ariaplugin");
    archive(
        &path,
        &[("plugin.toml", b"[plugin]\nid = \"Not An Id\"\n")],
        &[],
    );
    let into = root.path().join("into");
    let err = unpack(&path, &into).expect_err("bad manifest");
    assert_eq!(err.path, into.join("plugin.toml"));
    assert!(!into.exists());
}

#[test]
fn an_archive_that_inflates_past_the_limit_is_refused() {
    let root = scratch();
    let path = root.path().join("bomb.ariaplugin");
    let zeros = vec![0_u8; 64 * 1024];
    archive(
        &path,
        &[("plugin.toml", MANIFEST.as_bytes()), ("zeros", &zeros)],
        &[],
    );
    let limit = (MANIFEST.len() + zeros.len()) as u64;

    let into = root.path().join("at");
    unpack_within(&path, &into, limit).expect("exactly at the limit unpacks");

    let into = root.path().join("over");
    let err = unpack_within(&path, &into, limit - 1).expect_err("one byte over");
    assert_eq!(
        err.reason,
        format!("unpacks to more than {} bytes", limit - 1)
    );
    assert!(!into.exists());
}

#[test]
fn unpack_never_writes_into_an_existing_folder() {
    let root = scratch();
    let path = root.path().join("ok.ariaplugin");
    archive(&path, &[("plugin.toml", MANIFEST.as_bytes())], &[]);
    let into = root.path().join("into");
    write(&into, "keep", b"mine");
    let err = unpack(&path, &into).expect_err("exists");
    assert_eq!(err.reason, "the unpack folder already exists");
    assert_eq!(fs::read(into.join("keep")).expect("kept"), b"mine");
}

#[test]
fn not_a_zip_is_refused_by_name() {
    let root = scratch();
    let path = root.path().join("text.ariaplugin");
    fs::write(&path, b"plain text").expect("write");
    let err = unpack(&path, &root.path().join("into")).expect_err("not a zip");
    assert!(
        err.reason.starts_with("not a zip archive: "),
        "{}",
        err.reason
    );
}

#[test]
fn the_archive_extension_matches_in_any_case() {
    assert!(is_archive(Path::new("a/tools.ariaplugin")));
    assert!(is_archive(Path::new("TOOLS.ARIAPLUGIN")));
    assert!(!is_archive(Path::new("tools.zip")));
    assert!(!is_archive(Path::new("ariaplugin")));
}
