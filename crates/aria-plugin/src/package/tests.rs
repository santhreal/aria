//! Tests for plugin packages.
//!
//! Closes the class "a package is read differently from what its manifest and
//! files state": every manifest rule is a row with its exact refusal, tier
//! detection runs against real directories, and the declared WGSL registers and
//! unregisters through a real mount. It does not cover loading code; the tier
//! crates own that.

use std::path::Path;

use aria_models::{EffectParamKind, EffectParamSpec};
use semver::{Version, VersionReq};

use super::*;
use crate::activation::Activation;
use crate::context::PluginContext;
use crate::error::PluginError;
use crate::keys::{Chord, KeyDefault, KeyRoute};
use crate::menus::{MenuCommand, MenuLocation, MenuPath};
use crate::settings_schema::{SettingKind, SettingSpec};

const ID: &str = "com.example.tools";

fn text(id: &str, name: &str, version: &str, api: &str, rest: &str) -> String {
    format!("[plugin]\nid = {id:?}\nname = {name:?}\nversion = {version:?}\napi = {api}\n{rest}")
}

fn with(rest: &str) -> String {
    text(ID, "Tools", "1.0.0", "1", rest)
}

fn refusal(text: &str) -> String {
    PackageManifest::parse(text).expect_err("refused")
}

fn write(dir: &Path, relative: &str, contents: &str) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("create parent");
    std::fs::write(path, contents).expect("write");
}

fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().expect("scratch dir")
}

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
fn every_manifest_rule_refuses_with_its_reason() {
    let semver_error = Version::parse("1.0").expect_err("partial").to_string();
    let range_error = VersionReq::parse(">=banana")
        .expect_err("not a range")
        .to_string();
    let effect = |fields: &str| with(&format!("[[effects]]\nid = \"a\"\nname = \"A\"\n{fields}"));
    let command = |fields: &str| {
        with(&format!(
            "[[commands]]\nid = \"tools.reset\"\ntitle = \"Reset\"\n{fields}"
        ))
    };
    let setting = |fields: &str| {
        with(&format!(
            "[[settings]]\nkey = \"a\"\nlabel = \"A\"\n{fields}"
        ))
    };
    let cases: Vec<(&str, String, String)> =
        vec![
        (
            "one label",
            text("tools", "Tools", "1.0.0", "1", ""),
            "plugin id \"tools\" is not a reverse-domain id such as `com.example.tools`".into(),
        ),
        (
            "upper case",
            text("Com.example", "Tools", "1.0.0", "1", ""),
            "plugin id \"Com.example\" is not a reverse-domain id such as `com.example.tools`"
                .into(),
        ),
        (
            "empty label",
            text("com..tools", "Tools", "1.0.0", "1", ""),
            "plugin id \"com..tools\" is not a reverse-domain id such as `com.example.tools`"
                .into(),
        ),
        (
            "edge hyphen",
            text("com.-tools", "Tools", "1.0.0", "1", ""),
            "plugin id \"com.-tools\" is not a reverse-domain id such as `com.example.tools`"
                .into(),
        ),
        (
            "reserved",
            text("aria.tools", "Tools", "1.0.0", "1", ""),
            "plugin id \"aria.tools\": `aria.` is reserved for shipped plugins".into(),
        ),
        (
            "no name",
            text(ID, " ", "1.0.0", "1", ""),
            "com.example.tools: the plugin needs a name".into(),
        ),
        (
            "partial version",
            text(ID, "Tools", "1.0", "1", ""),
            format!("com.example.tools: version \"1.0\" is not semantic: {semver_error}"),
        ),
        (
            "future api",
            text(ID, "Tools", "1.0.0", "2", ""),
            "com.example.tools: built for plugin API 2; this Aria loads API 1".into(),
        ),
        (
            "worded api",
            text(ID, "Tools", "1.0.0", "\"one\"", ""),
            "com.example.tools: api is not a major version number".into(),
        ),
        (
            "negative api",
            text(ID, "Tools", "1.0.0", "-1", ""),
            "com.example.tools: api is not a major version number".into(),
        ),
        (
            "requires a name",
            text(ID, "Tools", "1.0.0", "1", "requires = [\"Tools\"]"),
            "com.example.tools: requires \"Tools\", which is not a plugin id".into(),
        ),
        (
            "requires itself",
            text(ID, "Tools", "1.0.0", "1", "requires = [\"com.example.tools >=1\"]"),
            "com.example.tools: requires itself".into(),
        ),
        (
            "requires a bad range",
            text(ID, "Tools", "1.0.0", "1", "requires = [\"com.other.x >=banana\"]"),
            format!("com.example.tools: requires \"com.other.x >=banana\": {range_error}"),
        ),
        (
            "unknown activation",
            with("[activation]\non = [\"boot\"]"),
            "com.example.tools: activation event \"boot\" is not `startup` or one of command: \
             file: effect: transition: generator: panel: setting:"
                .into(),
        ),
        (
            "service activation",
            with("[activation]\non = [\"service:com.example.Clock\"]"),
            "com.example.tools: activation event \"service:com.example.Clock\" is not `startup` \
             or one of command: file: effect: transition: generator: panel: setting:"
                .into(),
        ),
        (
            "panel without a location",
            with("[[panels]]\nid = \"tools.panel\"\ntitle = \"Tools\"\nlocation = \"viewer\""),
            "tools.panel: location \"viewer\" is not one of sidebar, inspector, timeline".into(),
        ),
        (
            "panel declared twice",
            with(
                "[[panels]]\nid = \"tools.panel\"\ntitle = \"Tools\"\nlocation = \"sidebar\"\n\
                 [[panels]]\nid = \"tools.panel\"\ntitle = \"More\"\nlocation = \"timeline\"",
            ),
            "tools.panel: the panel is declared twice".into(),
        ),
        (
            "panel without a title",
            with("[[panels]]\nid = \"tools.panel\"\ntitle = \" \"\nlocation = \"sidebar\""),
            "tools.panel: the panel needs a title".into(),
        ),
        (
            "empty activation",
            with("[activation]\non = [\"command: \"]"),
            "com.example.tools: activation event \"command: \" names nothing".into(),
        ),
        (
            "net url",
            with("[permissions]\nnet = [\"https://api.example.com\"]"),
            "com.example.tools: net host \"https://api.example.com\" is not a host name, such as \
             `api.example.com`"
                .into(),
        ),
        (
            "relative fs",
            with("[permissions]\nfs = [\"home\"]"),
            "com.example.tools: fs scope \"home\" is not `project`, `pick`, or an absolute path"
                .into(),
        ),
        (
            "process true",
            with("[permissions]\nprocess = true"),
            "com.example.tools: process = true names no executable; list the executables instead"
                .into(),
        ),
        (
            "process blank",
            with("[permissions]\nprocess = [\"ffprobe\", \" \"]"),
            "com.example.tools: process lists an empty name \" \"".into(),
        ),
        (
            "shader and passes",
            effect("shader = \"a.wgsl\"\npasses = [\"b.wgsl\"]"),
            "com.example.tools.a: declares both `shader` and `passes`; use one".into(),
        ),
        (
            "no shader",
            effect(""),
            "com.example.tools.a: declares no `shader` or `passes`".into(),
        ),
        (
            "parent path",
            effect("shader = \"../a.wgsl\""),
            "com.example.tools.a: \"../a.wgsl\" is not a path inside the package".into(),
        ),
        (
            "rooted path",
            effect("shader = \"/a.wgsl\""),
            "com.example.tools.a: \"/a.wgsl\" is not a path inside the package".into(),
        ),
        (
            "nested parent path",
            effect("passes = [\"ok.wgsl\", \"shaders/../../a.wgsl\"]"),
            "com.example.tools.a: \"shaders/../../a.wgsl\" is not a path inside the package"
                .into(),
        ),
        (
            "no file",
            effect("shader = \".\""),
            "com.example.tools.a: \".\" names no file".into(),
        ),
        (
            "a pass table without a file",
            effect("passes = [{ name = \"x\" }]"),
            "com.example.tools.a: pass 0: missing field `file`".into(),
        ),
        (
            "a misspelled pass key",
            effect("passes = [{ file = \"a.wgsl\", inptus = [] }]"),
            "com.example.tools.a: pass 0: unknown field `inptus`, expected one of `file`, \
             `name`, `stage`, `inputs`, `output`"
                .into(),
        ),
        (
            "a pass that is neither a path nor a table",
            effect("passes = [\"a.wgsl\", 3]"),
            "com.example.tools.a: pass 1: expected a WGSL file path or a pass table, found integer"
                .into(),
        ),
        (
            "a pass table outside the package",
            effect("passes = [{ file = \"../a.wgsl\" }]"),
            "com.example.tools.a: \"../a.wgsl\" is not a path inside the package".into(),
        ),
        (
            "a pass reading an undeclared texture",
            effect("passes = [{ file = \"a.wgsl\", inputs = [{ texture = \"lut\" }] }]"),
            "com.example.tools.a: pass 0 reads texture `lut`, which no [[textures]] entry declares"
                .into(),
        ),
        (
            "a texture twice",
            with(
                "[[textures]]\nid = \"lut\"\nfile = \"a.png\"\n\
                 [[textures]]\nid = \"com.example.tools.lut\"\nfile = \"b.png\"",
            ),
            "com.example.tools.lut: the texture is declared twice".into(),
        ),
        (
            "a texture outside the package",
            with("[[textures]]\nid = \"lut\"\nfile = \"/a.png\""),
            "com.example.tools.lut: \"/a.png\" is not a path inside the package".into(),
        ),
        (
            "a spaced texture id",
            with("[[textures]]\nid = \"l u t\"\nfile = \"a.png\""),
            "com.example.tools: id \"l u t\" holds characters other than ASCII letters, digits, \
             `-`, `_` and `.`"
                .into(),
        ),
        (
            "spaced id",
            with("[[effects]]\nid = \"a b\"\nname = \"A\"\nshader = \"a.wgsl\""),
            "com.example.tools: id \"a b\" holds characters other than ASCII letters, digits, \
             `-`, `_` and `.`"
                .into(),
        ),
        (
            "effect without a name",
            with("[[effects]]\nid = \"a\"\nname = \"\"\nshader = \"a.wgsl\""),
            "com.example.tools.a: the effect needs a name".into(),
        ),
        (
            "an effect and a generator share an id",
            with(
                "[[effects]]\nid = \"a\"\nname = \"A\"\nshader = \"a.wgsl\"\n\
                 [[generators]]\nid = \"com.example.tools.a\"\nname = \"A\"\nshader = \"b.wgsl\"",
            ),
            "com.example.tools.a: declared twice".into(),
        ),
        (
            "a transition shares an effect id",
            with(
                "[[effects]]\nid = \"a\"\nname = \"A\"\nshader = \"a.wgsl\"\n\
                 [[transitions]]\nid = \"a\"\nname = \"A\"\nshader = \"b.wgsl\"",
            ),
            "com.example.tools.a: declared twice".into(),
        ),
        (
            "transition without a name",
            with("[[transitions]]\nid = \"w\"\nname = \" \"\nshader = \"w.wgsl\""),
            "com.example.tools.w: the transition needs a name".into(),
        ),
        (
            "command without an id",
            with("[[commands]]\nid = \"\"\ntitle = \"Reset\""),
            "com.example.tools: a command needs an id".into(),
        ),
        (
            "command without a title",
            with("[[commands]]\nid = \"tools.reset\"\ntitle = \" \""),
            "tools.reset: the command needs a title".into(),
        ),
        (
            "command twice",
            with(
                "[[commands]]\nid = \"tools.reset\"\ntitle = \"Reset\"\n\
                 [[commands]]\nid = \"tools.reset\"\ntitle = \"Again\"",
            ),
            "tools.reset: the command is declared twice".into(),
        ),
        (
            "a menu outside every menu",
            command("menu = \"Effects/Color Tools\""),
            "tools.reset: menu \"Effects/Color Tools\" names no menu; it starts with one of \
             File, Edit, Sequence, Markers, Effect, Animation, View, Window, Help, Clip, Track, \
             Media, Viewer, Transition, Effect Header, Clip Badge"
                .into(),
        ),
        (
            "a menu nested twice",
            command("menu = \"Sequence/Color/Tools\""),
            "tools.reset: menu \"Sequence/Color/Tools\" nests deeper than one group, such as \
             `Sequence/Color Tools`"
                .into(),
        ),
        (
            "a menu with an empty group",
            command("menu = \"Sequence/ \""),
            "tools.reset: menu \"Sequence/ \" has an empty group".into(),
        ),
        (
            "a command in one menu twice",
            command("menu = \"Sequence\"\n[[menus]]\ncommand = \"tools.reset\"\nmenu = \"sequence\""),
            "tools.reset: the menu Sequence is declared twice".into(),
        ),
        (
            "a menu row for no command",
            command("[[menus]]\ncommand = \"tools.gone\"\nmenu = \"File\""),
            "tools.gone: a menu names a command no [[commands]] entry declares".into(),
        ),
        (
            "a modifier Aria does not have",
            command("key = \"Ctrl+Hyper+L\""),
            "tools.reset: key \"Ctrl+Hyper+L\" is not a chord such as `Ctrl+Alt+L` on a key \
             Aria can bind"
                .into(),
        ),
        (
            "a key Aria cannot bind",
            command("key = \"Ctrl+Banana\""),
            "tools.reset: key \"Ctrl+Banana\" is not a chord such as `Ctrl+Alt+L` on a key \
             Aria can bind"
                .into(),
        ),
        (
            "a route that is not a surface",
            command("[[keys]]\ncommand = \"tools.reset\"\nkey = \"x\"\nroute = \"inspector\""),
            "tools.reset: route \"inspector\" is not `editor`, `timeline`, `viewer`, or `home`"
                .into(),
        ),
        (
            "two keys on one route",
            command("key = \"Ctrl+Alt+L\"\n[[keys]]\ncommand = \"tools.reset\"\nkey = \"x\""),
            "tools.reset: the command has two Editor keys".into(),
        ),
        (
            "a key for no command",
            command("[[keys]]\ncommand = \"tools.gone\"\nkey = \"x\""),
            "tools.gone: a key names a command no [[commands]] entry declares".into(),
        ),
        (
            "an icon outside the package",
            command("icon = \"../shared/reset.svg\""),
            "tools.reset: \"../shared/reset.svg\" is not a path inside the package".into(),
        ),
        (
            "an icon at an absolute path",
            command("icon = \"/usr/share/reset.svg\""),
            "tools.reset: \"/usr/share/reset.svg\" is not a path inside the package".into(),
        ),
        (
            "an icon naming no file",
            command("icon = \".\""),
            "tools.reset: \".\" names no file".into(),
        ),
        (
            "an enablement condition with an unknown key",
            command("enablement = \"selected\""),
            format!(
                "tools.reset: the enablement condition names an unknown key `selected` at column 1; the keys are {}",
                crate::ContextKey::ALL.map(crate::ContextKey::name).join(", ")
            ),
        ),
        (
            "a malformed enablement condition",
            command("enablement = \"selection &&\""),
            "tools.reset: the enablement condition `selection &&` expects a key, `!`, or `(` at column 13"
                .into(),
        ),
        (
            "an empty enablement condition",
            command("enablement = \" \""),
            "tools.reset: the enablement condition is empty".into(),
        ),
        (
            "a setting of no known type",
            setting("type = \"color\""),
            "com.example.tools.a: type \"color\" is not text, secret, toggle, number or choice"
                .into(),
        ),
        (
            "a range on text",
            setting("min = 1"),
            "com.example.tools.a: min, max and step belong to a number".into(),
        ),
        (
            "options on a toggle",
            setting("type = \"toggle\"\noptions = [\"on\"]"),
            "com.example.tools.a: options belong to a choice".into(),
        ),
        (
            "a default that is a list",
            setting("default = [1]"),
            "com.example.tools.a: default [1] is not text, a number or true or false".into(),
        ),
        (
            "a setting without a label",
            with("[[settings]]\nkey = \"a\"\nlabel = \" \""),
            "com.example.tools.a: the setting needs a label".into(),
        ),
        (
            "a setting key with a space",
            with("[[settings]]\nkey = \"a b\"\nlabel = \"A\""),
            "com.example.tools: id \"a b\" holds characters other than ASCII letters, digits, \
             `-`, `_` and `.`"
                .into(),
        ),
        (
            "a range that holds nothing",
            setting("type = \"number\"\nmin = 2\nmax = 1"),
            "com.example.tools.a: the range 2 to 1 holds nothing".into(),
        ),
        (
            "a step that never moves",
            setting("type = \"number\"\nstep = 0"),
            "com.example.tools.a: the step 0 is not above zero".into(),
        ),
        (
            "a choice with nothing to choose",
            setting("type = \"choice\""),
            "com.example.tools.a: the choice has no options".into(),
        ),
        (
            "a default the kind refuses",
            setting("type = \"number\"\nmax = 1\ndefault = 3"),
            "com.example.tools.a: the default is refused: 3 is above 1".into(),
        ),
        (
            "a setting twice",
            with(
                "[[settings]]\nkey = \"a\"\nlabel = \"A\"\n\
                 [[settings]]\nkey = \"com.example.tools.a\"\nlabel = \"Again\"",
            ),
            "com.example.tools.a: the setting is declared twice".into(),
        ),
    ];
    for (case, text, want) in cases {
        assert_eq!(refusal(&text), want, "{case}");
    }
}

/// A key the format does not define is refused by name rather than ignored,
/// so a misspelled permission is not silently absent.
#[test]
fn unknown_and_missing_keys_are_refused_by_name() {
    for (text, want) in [
        (
            with("[permissions]\nnetwork = [\"a.b\"]"),
            "unknown field `network`",
        ),
        (
            text(ID, "Tools", "1.0.0", "1", "homepage = \"x\""),
            "unknown field `homepage`",
        ),
        (with("[[docks]]\nid = \"p\""), "unknown field `docks`"),
        (with("[[panels]]\nid = \"p\""), "missing field `title`"),
        (
            "[plugin]\nid = \"com.example.tools\"\n".to_owned(),
            "missing field `name`",
        ),
        (String::new(), "missing field `plugin`"),
        (
            with("[permissions]\nproject = \"write\""),
            "unknown variant `write`",
        ),
    ] {
        let reason = refusal(&text);
        assert!(reason.contains(want), "{want} not in {reason}");
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

/// Shipped as plain text so a test can create a package without a toolchain.
const EFFECT_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(input_tex, aria_sampler, in.uv) * param_gain();
}";

const GENERATOR_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.uv, 0.0, 1.0);
}";

const TRANSITION_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let a = textureSample(outgoing_tex, aria_sampler, in.uv);
    let b = textureSample(incoming_tex, aria_sampler, in.uv);
    return mix(a, b, aria.progress);
}";

const GAIN_PARAM: &str = "[[effects.params]]
name = \"gain\"
label = \"Gain\"
default = 1.0
min = 0.0
max = 2.0
";

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

fn data_package(root: &Path, id: &str, rest: &str) -> Package {
    let dir = root.join(id);
    write(
        &dir,
        MANIFEST_FILE,
        &text(id, "Declared", "1.0.0", "1", rest),
    );
    write(&dir, "shaders/gain.wgsl", EFFECT_FS);
    write(&dir, "shaders/uv.wgsl", GENERATOR_FS);
    write(&dir, "shaders/mix.wgsl", TRANSITION_FS);
    Package::open(&dir).expect("opens")
}

/// Declared shaders register when the package mounts, are listed and drawn
/// like any registered entry, and leave when it unmounts.
#[test]
fn declared_shaders_last_as_long_as_the_mount() {
    let root = scratch();
    let package = data_package(
        root.path(),
        "com.test.declared",
        &format!(
            "[[effects]]\nid = \"gain\"\nname = \"Gain\"\ncategory = \"Color\"\n\
             shader = \"shaders/gain.wgsl\"\n\
             {GAIN_PARAM}\
             [[generators]]\nid = \"uv\"\nname = \"UV\"\npasses = [\"shaders/uv.wgsl\"]\n\
             [[transitions]]\nid = \"mix\"\nname = \"Mix\"\nshader = \"shaders/mix.wgsl\""
        ),
    );
    let ctx = PluginContext::new();
    ctx.mount(&DataPlugin::new(package)).expect("mounts");

    let gain = aria_models::effect_spec("com.test.declared.gain").expect("the effect is listed");
    assert_eq!(gain.label, "Gain");
    assert_eq!(gain.category(), "Color", "the declared category is listed");
    assert!(!gain.generator);
    assert_eq!(
        gain.params
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["gain"]
    );
    let uv = aria_models::effect_spec("com.test.declared.uv").expect("the generator is listed");
    assert!(uv.generator, "a declared generator is a generator");
    assert_eq!(uv.category(), aria_models::OTHER_VIDEO_CATEGORY);
    assert!(aria_compositor::renders_effect("com.test.declared.gain"));
    assert!(aria_compositor::renders_effect("com.test.declared.uv"));
    assert_eq!(
        aria_models::transition_spec("com.test.declared.mix").map(|spec| spec.label.clone()),
        Some("Mix".to_owned())
    );
    assert!(aria_compositor::renders_transition("com.test.declared.mix"));

    ctx.unmount("com.test.declared").expect("unmounts");
    for id in ["com.test.declared.gain", "com.test.declared.uv"] {
        assert!(
            aria_models::effect_spec(id).is_none(),
            "{id} outlived its package"
        );
        assert!(!aria_compositor::renders_effect(id));
    }
    assert!(aria_models::transition_spec("com.test.declared.mix").is_none());
}

/// A shader that is not there fails the mount naming the file, and what
/// registered before it leaves with the mount.
#[test]
fn a_missing_shader_fails_the_mount_by_name() {
    let root = scratch();
    let package = data_package(
        root.path(),
        "com.test.missing",
        &format!(
            "[[effects]]\nid = \"gain\"\nname = \"Gain\"\nshader = \"shaders/gain.wgsl\"\n\
             {GAIN_PARAM}\
             [[transitions]]\nid = \"gone\"\nname = \"Gone\"\nshader = \"shaders/gone.wgsl\""
        ),
    );
    let os_error = package
        .dir
        .join("shaders/gone.wgsl")
        .canonicalize()
        .expect_err("absent")
        .to_string();
    let ctx = PluginContext::new();
    let err = ctx.mount(&DataPlugin::new(package)).expect_err("fails");
    assert_eq!(
        err.to_string(),
        format!(
            "plugin com.test.missing failed to mount: com.test.missing.gone: shaders/gone.wgsl: \
             {os_error}"
        )
    );
    assert!(
        aria_models::effect_spec("com.test.missing.gain").is_none(),
        "the effect registered before the failure stayed"
    );
}

/// A shader path that resolves outside the package through a link is refused.
#[cfg(unix)]
#[test]
fn a_shader_linked_from_outside_the_package_is_refused() {
    let root = scratch();
    let outside = root.path().join("outside.wgsl");
    std::fs::write(&outside, EFFECT_FS).expect("write outside");
    let package = data_package(
        root.path(),
        "com.test.linked",
        &format!(
            "[[effects]]\nid = \"gain\"\nname = \"Gain\"\nshader = \"shaders/link.wgsl\"\n{GAIN_PARAM}"
        ),
    );
    std::os::unix::fs::symlink(&outside, package.dir.join("shaders/link.wgsl")).expect("link");
    let ctx = PluginContext::new();
    let err = ctx.mount(&DataPlugin::new(package)).expect_err("fails");
    assert_eq!(
        err.to_string(),
        "plugin com.test.linked failed to mount: com.test.linked.gain: shaders/link.wgsl: \
         resolves outside the package"
    );
    assert!(aria_models::effect_spec("com.test.linked.gain").is_none());
}

/// A shader the compositor refuses is reported with its pass and line.
#[test]
fn a_refused_declared_shader_reports_its_line() {
    let root = scratch();
    let package = data_package(
        root.path(),
        "com.test.refused",
        "[[effects]]\nid = \"bad\"\nname = \"Bad\"\npasses = [\"shaders/uv.wgsl\", \"shaders/bad.wgsl\"]",
    );
    write(
        &package.dir,
        "shaders/bad.wgsl",
        "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    return nope;\n}",
    );
    let ctx = PluginContext::new();
    let Err(PluginError::RegisterRefused(refusal)) = register_declared(&ctx, &package) else {
        panic!("a shader naming an unknown identifier registered");
    };
    assert_eq!(refusal.id, "com.test.refused.bad");
    assert_eq!(refusal.pass, Some(1));
    assert_eq!(refusal.location.map(|(line, _)| line), Some(3));
    assert!(aria_models::effect_spec("com.test.refused.bad").is_none());

    // Through a mount, the refusal is the plugin's mount failure.
    let err = ctx.mount(&DataPlugin::new(package)).expect_err("fails");
    assert!(
        err.to_string().starts_with(
            "plugin com.test.refused failed to mount: registration refused: \
             com.test.refused.bad pass 1 at 3:"
        ),
        "{err}"
    );
}

/// Outside a mount the caller holds what registered, and dropping it removes
/// the entries.
#[test]
fn outside_a_mount_the_caller_holds_the_declared_shaders() {
    let root = scratch();
    let package = data_package(
        root.path(),
        "com.test.held",
        "[[generators]]\nid = \"uv\"\nname = \"UV\"\nshader = \"shaders/uv.wgsl\"",
    );
    let ctx = PluginContext::new();
    let held = register_declared(&ctx, &package).expect("registers");
    assert_eq!(held.len(), 1);
    assert!(aria_models::effect_spec("com.test.held.uv").is_some());
    drop(held);
    assert!(aria_models::effect_spec("com.test.held.uv").is_none());
}
