//! WHY: `aria plugin pack --out` wrote the archive to any name, so
//! `--out <dir>/plugin.toml` replaced the package's manifest with the archive,
//! `--out x.zip` wrote a package archive no install reads by its name, and
//! `--out <folder>` failed with the OS text after every check had run. Each
//! is now a wrong command line, refused before the package is read, with the
//! package left as it was.
//!
//! Not caught: a default output, `<dir>/<id>-<version>.ariaplugin`, that is a
//! folder.

use std::fs;

use crate::args::Args;
use crate::fail::Failure;

const MANIFEST: &str =
    "[plugin]\nid = \"com.example.q\"\nname = \"Q\"\nversion = \"0.1.0\"\napi = 1\n";

fn run(words: &[&str]) -> Result<(), Failure> {
    let args = Args::parse(words.iter().map(|word| (*word).to_owned())).expect("parse");
    crate::plugin::run(&args)
}

#[test]
fn an_output_that_is_not_an_archive_file_is_a_wrong_command_line() {
    let root = tempfile::tempdir().expect("tempdir");
    let dir = root.path().join("q");
    fs::create_dir_all(&dir).expect("package dir");
    fs::write(dir.join("plugin.toml"), MANIFEST).expect("manifest");
    fs::create_dir_all(root.path().join("folder.ariaplugin")).expect("folder");
    let at = |name: &str| root.path().join(name).to_string_lossy().into_owned();
    let package = dir.to_string_lossy().into_owned();
    let manifest = dir.join("plugin.toml").to_string_lossy().into_owned();
    let renamed = |out: &str, to: &str| {
        format!(
            "plugin pack writes a .ariaplugin file; name the output \"{to}\" rather than \"{out}\""
        )
    };
    let mut cases = vec![
        (
            manifest.clone(),
            renamed(&manifest, &dir.join("plugin.ariaplugin").to_string_lossy()),
        ),
        (at("q.zip"), renamed(&at("q.zip"), &at("q.ariaplugin"))),
        (at("q"), renamed(&at("q"), &at("q.ariaplugin"))),
        (
            at("folder.ariaplugin"),
            format!(
                "\"{}\" is a directory; name a file to write",
                at("folder.ariaplugin")
            ),
        ),
    ];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&manifest, at("link.ariaplugin")).expect("symlink");
        cases.push((
            at("link.ariaplugin"),
            format!(
                "\"{}\" is a symbolic link; name a file to write",
                at("link.ariaplugin")
            ),
        ));
    }
    for (out, refusal) in cases {
        let ran = run(&["plugin", "pack", &package, "--out", &out]);
        assert_eq!(ran, Err(Failure::Usage(refusal)), "--out {out}");
        assert_eq!(
            fs::read_to_string(dir.join("plugin.toml")).expect("manifest"),
            MANIFEST,
            "--out {out}"
        );
        assert_eq!(
            fs::read_dir(&dir).expect("package").count(),
            1,
            "--out {out} wrote into the package"
        );
    }
    assert!(!root.path().join("q.zip").exists());

    let out = at("q.ariaplugin");
    assert_eq!(run(&["plugin", "pack", &package, "--out", &out]), Ok(()));
    assert!(root.path().join("q.ariaplugin").is_file());
}
