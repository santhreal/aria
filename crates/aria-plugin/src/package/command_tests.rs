//! A declared command's icon and enablement condition reach the command the
//! package's code registers.
//!
//! Closes the class of a package whose declared icon is read lazily, so a
//! missing or hostile file passes `aria plugin check` and fails in the editor,
//! and of a runtime registration that drops what the manifest declared. Does
//! not cover how a tier calls `register_command`; the tier crates do.

use std::path::Path;

use super::*;
use crate::{Enablement, ICON_MAX_BYTES};

const ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 2h12v12H2z"/></svg>"#;

fn package(dir: &Path, commands: &str, files: &[(&str, &[u8])]) -> Result<Package, PackageError> {
    std::fs::create_dir_all(dir).expect("package dir");
    std::fs::write(
        dir.join(MANIFEST_FILE),
        format!(
            "[plugin]\nid = \"com.example.icons\"\nname = \"Icons\"\nversion = \"1.0.0\"\napi = 1\n{commands}"
        ),
    )
    .expect("manifest");
    std::fs::write(dir.join(COMPONENT_FILE), b"\0asm").expect("component");
    for (relative, bytes) in files {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("icon dir");
        std::fs::write(path, bytes).expect("icon");
    }
    Package::open(dir)
}

const DECLARED: &str = "[[commands]]\nid = \"icons.reset\"\ntitle = \"Reset\"\n\
    icon = \"icons/reset.svg\"\nenablement = \"single-selection\"\n\
    [[commands]]\nid = \"icons.plain\"\ntitle = \"Plain\"\n";

#[test]
fn a_registered_command_takes_the_declared_icon_and_condition() {
    let root = tempfile::tempdir().expect("scratch");
    let opened = package(
        &root.path().join("icons"),
        DECLARED,
        &[("icons/reset.svg", ICON.as_bytes())],
    )
    .expect("opens");

    let spec = opened.command_spec("icons.reset".into(), "Reset Now".into());
    assert_eq!(spec.id, "icons.reset");
    assert_eq!(spec.title, "Reset Now", "the registered title wins");
    let icon = spec.icon.expect("the declared icon");
    assert_eq!(
        icon,
        crate::CommandIcon::from_svg(ICON.as_bytes()).expect("parses")
    );
    assert_eq!(
        spec.enablement,
        Some(Enablement::parse("single-selection").expect("parses"))
    );

    let plain = opened.command_spec("icons.plain".into(), "Plain".into());
    assert_eq!(plain, crate::CommandSpec::new("icons.plain", "Plain"));
    let undeclared = opened.command_spec("icons.late".into(), "Late".into());
    assert_eq!(undeclared, crate::CommandSpec::new("icons.late", "Late"));
}

#[test]
fn a_bad_icon_refuses_the_package_when_it_is_read() {
    let root = tempfile::tempdir().expect("scratch");
    let oversized = vec![b' '; ICON_MAX_BYTES + 1];
    let oversized_files = [("icons/reset.svg", oversized.as_slice())];
    let cases: Vec<(&str, &[(&str, &[u8])], String)> = vec![
        ("missing", &[], "the icon of icons.reset: ".into()),
        (
            "not svg",
            &[("icons/reset.svg", b"plain text")],
            "the icon of icons.reset: the icon is not an SVG document: ".into(),
        ),
        (
            "oversized",
            &oversized_files,
            format!("the icon of icons.reset: the icon is larger than {ICON_MAX_BYTES} bytes"),
        ),
        (
            "draws nothing",
            &[(
                "icons/reset.svg",
                br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"/>"#,
            )],
            "the icon of icons.reset: the icon draws nothing".into(),
        ),
    ];
    for (name, files, want) in cases {
        let dir = root.path().join(name);
        let err = package(&dir, DECLARED, files).expect_err(name);
        assert_eq!(err.path, dir.join("icons/reset.svg"), "{name}");
        assert!(err.reason.starts_with(&want), "{name}: {}", err.reason);
    }
}
