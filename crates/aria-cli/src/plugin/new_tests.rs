//! WHY: `aria plugin new` took `--sdk` with a wasm plugin, checked that the
//! path existed, and wrote a package that never read it: a wasm package
//! depends on wit-bindgen. And an id the manifest rules reject exited 1, the
//! code for a command that failed, where every other malformed argument exits
//! 2. `--sdk` with a wasm plugin is now a wrong command line, and so is a
//! malformed id; neither writes anything.
//!
//! A parent folder that cannot be written printed the OS text with its code,
//! "Permission denied (os error 13)". Every plugin command states a failed
//! read or write through `Failure::io`, as the path and the reason alone.
//!
//! Not caught: a flag a later tier reads that wasm ignores, unless it is
//! added to the cases here; a write failure under root, which writes anyway.

use super::NewTier;
use crate::args::Args;
use crate::fail::Failure;

fn run(words: &[&str]) -> Result<(), Failure> {
    let args = Args::parse(words.iter().map(|word| (*word).to_owned())).expect("parse");
    crate::plugin::run(&args)
}

#[test]
fn an_sdk_is_a_wrong_command_line_for_every_tier_but_native() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parent = dir.path().to_string_lossy().into_owned();
    let sdk = concat!(env!("CARGO_MANIFEST_DIR"), "/../aria-sdk");
    let mut refused = 0;
    for tier in NewTier::ALL {
        let id = format!("com.example.sdk-{}", tier.key());
        let words = [
            "plugin",
            "new",
            &id,
            "--tier",
            tier.key(),
            "--dir",
            &parent,
            "--sdk",
            sdk,
        ];
        let ran = run(&words);
        if tier == NewTier::Native {
            assert_eq!(ran, Ok(()), "--tier native --sdk");
            assert!(
                dir.path().join(&id).is_dir(),
                "the native package is written"
            );
            continue;
        }
        refused += 1;
        assert_eq!(
            ran,
            Err(Failure::Usage(
                "--sdk is the aria-sdk path for a --tier native plugin; a wasm plugin builds \
                 against wit-bindgen"
                    .into()
            )),
            "--tier {} --sdk",
            tier.key()
        );
        assert!(
            !dir.path().join(&id).exists(),
            "--tier {} --sdk wrote a package",
            tier.key()
        );
    }
    assert_eq!(refused, NewTier::ALL.len() - 1, "every tier but native");

    let id = "com.example.sdk-default";
    let ran = run(&["plugin", "new", id, "--dir", &parent, "--sdk", sdk]);
    assert_eq!(
        ran.map_err(|err| err.code()),
        Err(2),
        "the default tier with --sdk"
    );
    assert!(
        !dir.path().join(id).exists(),
        "the default tier with --sdk wrote a package"
    );
}

#[test]
fn a_malformed_id_is_a_wrong_command_line_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parent = dir.path().to_string_lossy().into_owned();
    for (id, why) in [
        (
            "tools",
            "is not a reverse-domain id such as `com.example.tools`",
        ),
        (
            "../evil",
            "is not a reverse-domain id such as `com.example.tools`",
        ),
        ("aria.tools", ": `aria.` is reserved for shipped plugins"),
    ] {
        let ran = run(&["plugin", "new", id, "--dir", &parent]);
        let separator = if why.starts_with(':') { "" } else { " " };
        assert_eq!(
            ran,
            Err(Failure::Usage(format!("plugin id {id:?}{separator}{why}"))),
            "{id}"
        );
        assert_eq!(
            std::fs::read_dir(dir.path())
                .expect("read the parent")
                .count(),
            0,
            "{id}: nothing is written"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_parent_that_cannot_be_written_is_stated_with_its_reason() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let mode = |bits| std::fs::Permissions::from_mode(bits);
    std::fs::set_permissions(dir.path(), mode(0o555)).expect("make read-only");
    let probe = std::fs::write(dir.path().join("probe"), b"");
    let parent = dir.path().to_string_lossy().into_owned();
    let ran = run(&["plugin", "new", "com.example.q", "--dir", &parent]);
    std::fs::set_permissions(dir.path(), mode(0o755)).expect("restore");
    if probe.is_ok() {
        return;
    }
    assert_eq!(
        ran,
        Err(Failure::Failed(format!(
            "{parent}/com.example.q: permission denied"
        )))
    );
}
