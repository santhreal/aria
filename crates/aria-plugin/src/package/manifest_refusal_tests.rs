use semver::{Version, VersionReq};

use super::*;

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
