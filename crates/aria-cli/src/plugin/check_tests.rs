//! WHY: `aria plugin check <file.ariaplugin>` unpacks the archive into a
//! scratch directory and checks that, so every failure stated the scratch
//! path, `/tmp/.tmpXXXX/package/plugin.toml: TOML parse error`, which is
//! deleted before the message prints. Each failure of an archive check now
//! states the archive the command was given, with the path inside it where
//! the failure has one.
//!
//! Rows cover a failure from unpacking (a manifest over its size limit), from
//! parsing (a manifest that is not TOML), from opening the package (one top
//! folder, commands with no code), and from the checks that run after it (a
//! library that is not one). Not caught: a failure whose message
//! states the scratch path in another form than `Path::display`.

use std::fs::File;
use std::io::Write as _;
use std::path::Path;

use aria_plugin::package::library_name;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::args::Args;

/// The manifest of a package that declares one command.
const MANIFEST: &str = "[plugin]\nid = \"com.example.demo\"\nname = \"Demo\"\n\
                        version = \"0.1.0\"\napi = 1\n\n[[commands]]\n\
                        id = \"com.example.demo.run\"\ntitle = \"Run Demo\"\nmenu = \"Sequence\"\n";

/// Write an archive at `path` holding `entries`.
fn archive(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = ZipWriter::new(File::create(path).expect("create archive"));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default())
            .expect("start entry");
        zip.write_all(bytes).expect("write entry");
    }
    zip.finish().expect("finish archive");
}

#[test]
fn every_failure_of_an_archive_check_states_the_archive() {
    let root = tempfile::tempdir().expect("scratch dir");
    let oversized = vec![b'#'; 2 * 1024 * 1024];
    let library = format!("lib/{}", library_name());
    let unreadable = format!("/{library}: invalid ");
    let cases: [(&str, Vec<(&str, &[u8])>, &str); 5] = [
        (
            "big",
            vec![("plugin.toml", oversized.as_slice())],
            ": plugin.toml is larger than ",
        ),
        (
            "toml",
            vec![("plugin.toml", b"not toml [[[".as_slice())],
            "/plugin.toml: TOML parse error at line 1, column 5",
        ),
        (
            "code",
            vec![("plugin.toml", MANIFEST.as_bytes())],
            ": declares commands but holds no code: add plugin.wasm or lib/",
        ),
        (
            "folder",
            vec![("demo/plugin.toml", MANIFEST.as_bytes())],
            ": declares commands but holds no code: add plugin.wasm or lib/",
        ),
        (
            "library",
            vec![
                ("plugin.toml", MANIFEST.as_bytes()),
                (library.as_str(), b"not a shared library".as_slice()),
            ],
            unreadable.as_str(),
        ),
    ];
    for (name, entries, after) in cases {
        let path = root.path().join(format!("{name}.ariaplugin"));
        archive(&path, &entries);
        let args = Args::parse(["plugin", "check", &path.to_string_lossy()].map(str::to_owned))
            .expect("parse");
        let failure = crate::plugin::run(&args).expect_err(name);
        assert_eq!(failure.code(), 1, "{name}: {}", failure.message());
        let expected = format!("{}{after}", path.display());
        assert!(
            failure.message().starts_with(&expected),
            "{name}: expected {expected:?}, got {:?}",
            failure.message()
        );
    }
}
