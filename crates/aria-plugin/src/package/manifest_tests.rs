use aria_models::{EffectParamKind, EffectParamSpec};
use semver::VersionReq;

use super::*;
use crate::activation::Activation;
use crate::keys::{Chord, KeyDefault, KeyRoute};
use crate::menus::{MenuCommand, MenuLocation, MenuPath};
use crate::settings_schema::{SettingKind, SettingSpec};

#[test]
fn a_full_manifest_reads_as_written() {
    let absolute = std::env::current_dir().expect("cwd").join("luts");
    let manifest = PackageManifest::parse(&format!(
        r#"
[plugin]
id = "com.example.color-tools"
name = "Color Tools"
version = "1.4.0"
api = "1"
publisher = "Example"
description = "Split-tone and a wipe."
license = "MIT"
requires = ["com.example.base >=1.2, <2", "com.example.any"]

[activation]
on = ["startup", "effect:com.example.color-tools.split-tone", "command:color-tools.reset", "file:.CUBE", "startup"]

[permissions]
project = "read-write"
media = "read"
gpu = true
net = ["api.example.com"]
fs = ["project", "pick", {absolute:?}]
process = ["ffprobe"]
clipboard = true

[[effects]]
id = "split-tone"
name = "Split Tone"
category = "Color"
shader = "shaders/split_tone.wgsl"

[[effects.params]]
name = "amount"
label = "Amount"
default = 0.5
min = 0.0
max = 1.0

[[effects]]
id = "com.example.color-tools.glow"
name = "Glow"
passes = ["shaders/blur.wgsl", "./shaders/add.wgsl"]

[[generators]]
id = "noise"
name = "Noise"
shader = "shaders/noise.wgsl"

[[transitions]]
id = "wipe"
name = "Wipe"
shader = "shaders/wipe.wgsl"

[[commands]]
id = "color-tools.reset"
title = "Reset"
key = "Ctrl+Alt+L"
menu = "Sequence/Color Tools"
icon = "./icons/reset.svg"
enablement = "selection && !playing"

[[commands]]
id = "color-tools.export"
title = "Export LUT"

[[menus]]
command = "color-tools.export"
menu = "media"

[[keys]]
command = "color-tools.export"
key = "shift+alt+e"
route = "home"

[[settings]]
key = "lut-directory"
label = "LUT folder"
placeholder = "~/luts"

[[settings]]
key = "com.example.color-tools.token"
label = "Token"
type = "secret"

[[settings]]
key = "strength"
label = "Strength"
type = "number"
min = 0
max = 2.5
step = 0.5
default = 1

[[settings]]
key = "preview"
label = "Preview"
type = "toggle"
default = true

[[settings]]
key = "space"
label = "Space"
type = "choice"
options = ["Rec.709", "ACEScct"]
default = "ACEScct"
"#,
        absolute = absolute.display().to_string()
    ))
    .expect("parses");

    assert_eq!(
        manifest,
        PackageManifest {
            id: "com.example.color-tools".into(),
            name: "Color Tools".into(),
            version: "1.4.0".into(),
            api: 1,
            publisher: "Example".into(),
            description: "Split-tone and a wipe.".into(),
            license: "MIT".into(),
            requires: vec![
                Requirement {
                    id: "com.example.base".into(),
                    versions: VersionReq::parse(">=1.2, <2").expect("range"),
                },
                Requirement {
                    id: "com.example.any".into(),
                    versions: VersionReq::STAR,
                },
            ],
            activation: vec![
                Activation::Startup,
                Activation::Effect("com.example.color-tools.split-tone".into()),
                Activation::Command("color-tools.reset".into()),
                Activation::File("cube".into()),
            ],
            permissions: Permissions {
                project: Access::ReadWrite,
                media: Access::Read,
                gpu: true,
                net: vec!["api.example.com".into()],
                fs: vec![FsScope::Project, FsScope::Pick, FsScope::Path(absolute)],
                process: vec!["ffprobe".into()],
                clipboard: true,
            },
            effects: vec![
                DeclaredEffect {
                    id: "com.example.color-tools.split-tone".into(),
                    name: "Split Tone".into(),
                    category: Some("Color".into()),
                    passes: vec![DeclaredPass::file("shaders/split_tone.wgsl")],
                    params: vec![EffectParamSpec {
                        name: "amount".into(),
                        label: "Amount".into(),
                        kind: EffectParamKind::Scalar,
                        default: 0.5,
                        min: 0.0,
                        max: 1.0,
                        default_color: [0; 4],
                        default_vec2: [0.0; 2],
                        ..Default::default()
                    }],
                    groups: Vec::new(),
                    stateful: false,
                    bounds: aria_models::EffectBounds::ZERO,
                    reads_outside: false,
                    time_dependent: false,
                },
                DeclaredEffect {
                    id: "com.example.color-tools.glow".into(),
                    name: "Glow".into(),
                    category: None,
                    passes: vec![
                        DeclaredPass::file("shaders/blur.wgsl"),
                        DeclaredPass::file("shaders/add.wgsl"),
                    ],
                    params: Vec::new(),
                    groups: Vec::new(),
                    stateful: false,
                    bounds: aria_models::EffectBounds::ZERO,
                    reads_outside: false,
                    time_dependent: false,
                },
            ],
            transitions: vec![DeclaredTransition {
                id: "com.example.color-tools.wipe".into(),
                name: "Wipe".into(),
                shader: "shaders/wipe.wgsl".into(),
                params: Vec::new(),
            }],
            generators: vec![DeclaredEffect {
                id: "com.example.color-tools.noise".into(),
                name: "Noise".into(),
                category: None,
                passes: vec![DeclaredPass::file("shaders/noise.wgsl")],
                params: Vec::new(),
                groups: Vec::new(),
                stateful: false,
                bounds: aria_models::EffectBounds::ZERO,
                reads_outside: false,
                time_dependent: false,
            }],
            commands: vec![
                DeclaredCommand {
                    id: "color-tools.reset".into(),
                    title: "Reset".into(),
                    key: Some("Ctrl+Alt+L".into()),
                    menu: Some("Sequence/Color Tools".into()),
                    icon: Some("icons/reset.svg".into()),
                    enablement: Some(
                        crate::Enablement::parse("selection && !playing").expect("parses"),
                    ),
                },
                DeclaredCommand {
                    id: "color-tools.export".into(),
                    title: "Export LUT".into(),
                    key: None,
                    menu: None,
                    icon: None,
                    enablement: None,
                },
            ],
            panels: Vec::new(),
            menus: vec![
                MenuCommand {
                    command: "color-tools.reset".into(),
                    path: MenuPath {
                        location: MenuLocation::Sequence,
                        group: Some("Color Tools".into()),
                    },
                },
                MenuCommand {
                    command: "color-tools.export".into(),
                    path: MenuPath {
                        location: MenuLocation::Media,
                        group: None,
                    },
                },
            ],
            keys: vec![
                KeyDefault {
                    command: "color-tools.reset".into(),
                    chord: Chord::ctrl_alt("l"),
                    route: KeyRoute::Editor,
                },
                KeyDefault {
                    command: "color-tools.export".into(),
                    chord: Chord::shift_alt("e"),
                    route: KeyRoute::Home,
                },
            ],
            settings: vec![
                setting(
                    "lut-directory",
                    "LUT folder",
                    "~/luts",
                    SettingKind::Text,
                    None
                ),
                setting("token", "Token", "", SettingKind::Secret, None),
                setting(
                    "strength",
                    "Strength",
                    "",
                    SettingKind::Number {
                        min: 0.0,
                        max: 2.5,
                        step: 0.5,
                    },
                    Some("1"),
                ),
                setting("preview", "Preview", "", SettingKind::Toggle, Some("true")),
                setting(
                    "space",
                    "Space",
                    "",
                    SettingKind::Choice(vec!["Rec.709".into(), "ACEScct".into()]),
                    Some("ACEScct"),
                ),
            ],
            workspaces: Vec::new(),
            clip_types: Vec::new(),
            textures: Vec::new(),
        }
    );
}

/// A setting of `com.example.color-tools` as the manifest reads it.
fn setting(
    key: &str,
    label: &str,
    placeholder: &str,
    kind: SettingKind,
    default: Option<&str>,
) -> SettingSpec {
    SettingSpec {
        key: format!("com.example.color-tools.{key}"),
        label: label.into(),
        placeholder: placeholder.into(),
        kind,
        default: default.map(str::to_owned),
    }
}

#[test]
fn a_minimal_manifest_asks_for_nothing() {
    for api in ["1", "\"1\"", "\" 1 \""] {
        let manifest =
            PackageManifest::parse(&text(ID, "Tools", "1.0.0-beta.2", api, "")).expect(api);
        assert_eq!(manifest.api, API_MAJOR);
        assert_eq!(manifest.version, "1.0.0-beta.2");
        assert_eq!(manifest.permissions, Permissions::default());
        assert!(manifest.activation.is_empty(), "loads at start");
        assert!(manifest.requires.is_empty());
        assert_eq!(
            (
                manifest.publisher.as_str(),
                manifest.description.as_str(),
                manifest.license.as_str()
            ),
            ("", "", "")
        );
        assert!(
            manifest.effects.is_empty()
                && manifest.generators.is_empty()
                && manifest.transitions.is_empty()
                && manifest.commands.is_empty()
                && manifest.menus.is_empty()
                && manifest.keys.is_empty()
                && manifest.settings.is_empty()
        );
    }
}

#[test]
fn a_requirement_admits_the_versions_in_its_range() {
    let manifest = PackageManifest::parse(&text(
        ID,
        "Tools",
        "1.0.0",
        "1",
        "requires = [\"com.example.base >=1.2, <2\", \"com.example.any\"]",
    ))
    .expect("parses");
    let [ranged, any] = manifest.requires.as_slice() else {
        panic!("two requirements");
    };
    for (version, admitted) in [
        ("1.2.0", true),
        ("1.9.4", true),
        ("1.1.9", false),
        ("2.0.0", false),
        ("1.2", false),
        ("", false),
    ] {
        assert_eq!(ranged.admits(version), admitted, "{version}");
    }
    assert!(any.admits("0.0.1") && any.admits("not semantic"));
}
