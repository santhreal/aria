//! Plugin clip types: what `register_clip_type` lists, how long it lasts, what
//! it does to an icon, and how a package's `[[clip_types]]` reach the catalog.
//!
//! The class these close: a clip type that outlives its plugin, an icon that
//! reaches the catalog without being parsed and rewritten, and a declared table
//! read with a fault in it. The catalog's own rules are tested in aria-models;
//! placing a typed clip is tested in aria-ui.

use aria_models::{ClipTypeIcon, ClipTypeSpec, MarkerColor, clip_type_spec};

use super::*;
use crate::package::{DataPlugin, MANIFEST_FILE, Package};
use crate::plugin::{Plugin, PluginManifest};

/// An icon with an element the parser drops: drawn text would need a font.
const ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 2h12v12H2z"/><text>t</text></svg>"#;

struct TypePlugin {
    manifest: PluginManifest,
    spec: ClipTypeSpec,
}

impl Plugin for TypePlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register_clip_type(self.spec.clone())?;
        Ok(())
    }
}

fn spec(id: &str) -> ClipTypeSpec {
    ClipTypeSpec::new(
        id,
        "Lower third",
        "com.example.bars.bar",
        MarkerColor::Orange,
    )
}

fn refusal(result: Result<Registration, PluginError>) -> String {
    match result {
        Err(PluginError::ClipTypeRefused(reason)) => reason,
        Err(other) => panic!("refused as {other:?}, not a clip type refusal"),
        Ok(_) => panic!("registered"),
    }
}

#[test]
fn a_mounted_type_is_listed_until_the_plugin_unmounts() {
    let ctx = PluginContext::new();
    let plugin = TypePlugin {
        manifest: PluginManifest::new("com.example.bars", "Bars", "1.0.0", &[], &[]),
        spec: spec("com.example.bars.mounted"),
    };
    ctx.mount(&plugin).expect("mounts");
    let listed = clip_type_spec("com.example.bars.mounted").expect("listed");
    assert_eq!(listed.label, "Lower third");
    let plugins = ctx.mounted();
    let info = plugins
        .iter()
        .find(|info| info.manifest.id == "com.example.bars")
        .expect("mounted");
    assert!(
        info.contributions.iter().any(|c| {
            c.kind == ContributionKind::ClipType && c.id == "com.example.bars.mounted"
        }),
        "the Plugins list does not show the clip type"
    );
    ctx.unmount("com.example.bars").expect("unmounts");
    assert_eq!(clip_type_spec("com.example.bars.mounted"), None);
}

#[test]
fn an_icon_is_parsed_and_rewritten_before_it_is_listed() {
    let ctx = PluginContext::new();
    let mut typed = spec("com.example.bars.iconic");
    typed.icon = Some(ClipTypeIcon {
        svg: std::sync::Arc::from(ICON),
        key: 0,
    });
    let held = ctx.register_clip_type(typed).expect("registers");
    let listed = clip_type_spec("com.example.bars.iconic").expect("listed");
    let expected = crate::CommandIcon::from_svg(ICON.as_bytes()).expect("parses");
    assert_eq!(listed.icon, Some(expected.to_clip_type_icon()));
    let icon = listed.icon.as_ref().expect("an icon");
    assert!(!icon.svg.contains("<text"), "text reached the catalog");
    assert_ne!(icon.key, 0, "the key a plugin passed was kept");
    drop(held);

    let mut broken = spec("com.example.bars.broken");
    broken.icon = Some(ClipTypeIcon {
        svg: std::sync::Arc::from("<svg"),
        key: 1,
    });
    let reason = refusal(ctx.register_clip_type(broken));
    assert!(reason.starts_with("com.example.bars.broken: "), "{reason}");
    assert_eq!(clip_type_spec("com.example.bars.broken"), None);
}

#[test]
fn a_catalog_refusal_is_a_clip_type_refusal() {
    let ctx = PluginContext::new();
    let reason = refusal(ctx.register_clip_type(spec("unnamespaced")));
    assert!(reason.contains("needs a namespace"), "{reason}");
    let _held = ctx
        .register_clip_type(spec("com.example.bars.taken"))
        .expect("registers");
    let reason = refusal(ctx.register_clip_type(spec("com.example.bars.taken")));
    assert!(reason.contains("already in the catalog"), "{reason}");
}

fn package_with(scratch: &std::path::Path, tables: &str) -> std::path::PathBuf {
    let dir = scratch.join("com.example.titles");
    std::fs::create_dir_all(&dir).expect("package dir");
    std::fs::write(
        dir.join(MANIFEST_FILE),
        format!(
            "[plugin]\nid = \"com.example.titles\"\nname = \"Titles\"\nversion = \"0.1.0\"\napi = 1\n{tables}"
        ),
    )
    .expect("manifest");
    dir
}

#[test]
fn a_package_registers_its_declared_clip_types() {
    let scratch = tempfile::tempdir().expect("scratch");
    let dir = package_with(
        scratch.path(),
        "[[clip_types]]\nid = \"lower-third\"\nname = \"Lower third\"\ngenerator = \"bar\"\ncolour = \"orange\"\nicon = \"icons/third.svg\"\nhandles = [\"anchor\"]\n\n[[clip_types]]\nid = \"com.example.titles.card\"\nname = \"Card\"\ngenerator = \"com.example.titles.card-gen\"\ncolour = \"blue\"\n",
    );
    std::fs::create_dir_all(dir.join("icons")).expect("icons dir");
    std::fs::write(dir.join("icons/third.svg"), ICON).expect("icon");
    let package = Package::open(&dir).expect("opens");
    let ctx = PluginContext::new();
    ctx.mount(&DataPlugin::new(package)).expect("mounts");

    let third = clip_type_spec("com.example.titles.lower-third").expect("listed");
    assert_eq!(third.label, "Lower third");
    assert_eq!(third.generator, "com.example.titles.bar");
    assert_eq!(third.colour, MarkerColor::Orange);
    assert_eq!(third.handles, ["anchor"]);
    let expected = crate::CommandIcon::from_svg(ICON.as_bytes()).expect("parses");
    assert_eq!(third.icon, Some(expected.to_clip_type_icon()));
    let card = clip_type_spec("com.example.titles.card").expect("listed");
    assert_eq!(card.generator, "com.example.titles.card-gen");
    assert_eq!(card.icon, None);

    ctx.unmount("com.example.titles").expect("unmounts");
    assert_eq!(clip_type_spec("com.example.titles.lower-third"), None);
    assert_eq!(clip_type_spec("com.example.titles.card"), None);
}

#[test]
fn a_declared_clip_type_with_a_fault_refuses_the_package() {
    let scratch = tempfile::tempdir().expect("scratch");
    for (tables, says) in [
        (
            "[[clip_types]]\nid = \"a\"\nname = \"A\"\ngenerator = \"g\"\ncolour = \"mauve\"\n",
            "mauve",
        ),
        (
            "[[clip_types]]\nid = \"a\"\nname = \"A\"\ngenerator = \"g\"\n",
            "colour",
        ),
        (
            "[[clip_types]]\nid = \"a\"\nname = \"A\"\ngenerator = \"g\"\ncolour = \"red\"\ncolor = \"red\"\n",
            "color",
        ),
        (
            "[[clip_types]]\nid = \"a\"\nname = \" \"\ngenerator = \"g\"\ncolour = \"red\"\n",
            "has no name",
        ),
        (
            "[[clip_types]]\nid = \"a b\"\nname = \"A\"\ngenerator = \"g\"\ncolour = \"red\"\n",
            "holds characters",
        ),
        (
            "[[clip_types]]\nid = \"a\"\nname = \"A\"\ngenerator = \"g\"\ncolour = \"red\"\nhandles = [\"p\", \"p\"]\n",
            "declared twice",
        ),
        (
            "[[clip_types]]\nid = \"a\"\nname = \"A\"\ngenerator = \"g\"\ncolour = \"red\"\nicon = \"../x.svg\"\n",
            "not a path inside the package",
        ),
        (
            "[[clip_types]]\nid = \"a\"\nname = \"A\"\ngenerator = \"g\"\ncolour = \"red\"\n[[clip_types]]\nid = \"a\"\nname = \"B\"\ngenerator = \"g\"\ncolour = \"red\"\n",
            "the clip type is declared twice",
        ),
        (
            "[[clip_types]]\nid = \"a\"\nname = \"A\"\ngenerator = \"g\"\ncolour = \"red\"\nicon = \"missing.svg\"\n",
            "the icon of com.example.titles.a",
        ),
    ] {
        let dir = package_with(scratch.path(), tables);
        let err = Package::open(&dir).expect_err(tables);
        assert!(err.to_string().contains(says), "{tables}: {err}");
    }
}

/// A scalar field `level`, 0 to 1, 0.25 by default.
fn level() -> EffectParamSpec {
    EffectParamSpec {
        name: "level".into(),
        label: "Level".into(),
        kind: aria_models::EffectParamKind::Scalar,
        default: 0.25,
        min: 0.0,
        max: 1.0,
        ..Default::default()
    }
}

/// Fill the frame with opaque grey at the `level` field.
fn grey(fields: &EffectParams<'_>, _time: f32, frame: &mut CpuFrameOut<'_>) -> Result<(), String> {
    let level = fields
        .scalar("level")
        .ok_or_else(|| "no level field".to_owned())?;
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            frame.set_pixel_f32(x, y, [level, level, level, 1.0]);
        }
    }
    Ok(())
}

/// A type with the `level` field drawn grey on the CPU.
fn meter(id: &str) -> ClipTypeDefinition {
    ClipTypeDefinition::new(
        ClipTypeSpec::new(id, "Meter", "", MarkerColor::Green),
        vec![level()],
        ClipTypeRender::cpu(grey),
    )
}

/// The first pixel the generator `id` draws on a 2x2 frame at `level`.
fn drawn_pixel(id: &str, level: f32) -> [u16; 4] {
    let pass = crate::registered_effect(id).expect("the generator renders");
    let blank = [0u16; 16];
    let mut out = [0u16; 16];
    assert!(
        pass.run_cpu(
            CpuFrameIn::new(&blank, 2, 2, 8),
            &mut CpuFrameOut::new(&mut out, 2, 2, 8),
            &[level],
            0.0,
        ),
        "the renderer failed"
    );
    [out[0], out[1], out[2], out[3]]
}

struct DefinedPlugin {
    manifest: PluginManifest,
    definition: ClipTypeDefinition,
}

impl Plugin for DefinedPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.define_clip_type(self.definition.clone())?;
        Ok(())
    }
}

#[test]
fn a_defined_type_is_drawn_by_its_own_generator_until_the_plugin_unmounts() {
    let ctx = PluginContext::new();
    let id = "com.example.meters.meter";
    let plugin = DefinedPlugin {
        manifest: PluginManifest::new("com.example.meters", "Meters", "1.0.0", &[], &[]),
        definition: meter(id),
    };
    ctx.mount(&plugin).expect("mounts");
    let listed = clip_type_spec(id).expect("listed");
    assert_eq!(listed.generator, id, "the type names another generator");
    assert_eq!(listed.unavailable(), None);
    let drawn = aria_models::effect_spec(id).expect("the generator is in the effect catalog");
    assert!(drawn.generator);
    assert_eq!(drawn.label, "Meter");
    assert_eq!(drawn.params, [level()]);
    assert!(!drawn.time_dependent);
    let plugins = ctx.mounted();
    let info = plugins
        .iter()
        .find(|info| info.manifest.id == "com.example.meters")
        .expect("mounted");
    assert_eq!(info.contributions.len(), 1, "{:?}", info.contributions);
    assert!(
        info.contributions[0].kind == ContributionKind::ClipType && info.contributions[0].id == id,
        "the Plugins list does not show one clip type"
    );
    let half = crate::f32_to_f16(0.5);
    assert_eq!(drawn_pixel(id, 0.5), [half, half, half, crate::F16_ONE]);

    ctx.unmount("com.example.meters").expect("unmounts");
    assert_eq!(clip_type_spec(id), None);
    assert_eq!(
        aria_models::effect_spec(id),
        None,
        "unmount left the generator"
    );
}

#[test]
fn a_defined_type_outside_a_mount_lasts_as_long_as_its_registration() {
    let ctx = PluginContext::new();
    let id = "com.example.meters.ticking";
    let held = ctx
        .define_clip_type(meter(id).with_time_dependent(true))
        .expect("defines");
    let pass = crate::registered_effect(id).expect("the generator renders");
    assert!(pass.is_time_dependent(), "time_dependent was dropped");
    drop(pass);
    assert!(clip_type_spec(id).is_some());
    drop(held);
    assert_eq!(clip_type_spec(id), None);
    assert_eq!(aria_models::effect_spec(id), None);
}

#[test]
fn a_refused_definition_lists_neither_the_type_nor_its_generator() {
    let ctx = PluginContext::new();
    let mut other = meter("com.example.meters.other");
    other.spec.generator = "com.example.meters.bar".into();
    let reason = refusal(ctx.define_clip_type(other));
    assert!(reason.contains("draws itself"), "{reason}");

    let mut handled = meter("com.example.meters.handled");
    handled.spec.handles = vec!["level".into()];
    let reason = refusal(ctx.define_clip_type(handled));
    assert!(reason.contains("names no vec2 parameter"), "{reason}");

    let reason = refusal(ctx.define_clip_type(meter("unnamespaced")));
    assert!(reason.contains("needs a namespace"), "{reason}");

    let broken = ClipTypeDefinition::new(
        ClipTypeSpec::new("com.example.meters.broken", "Broken", "", MarkerColor::Red),
        vec![level()],
        vec![GraphPass::fragment("fn fs(")],
    );
    match ctx.define_clip_type(broken).map(drop) {
        Err(PluginError::RegisterRefused(err)) => assert_eq!(err.pass, Some(0), "{err}"),
        other => panic!("refused as {other:?}, not a register refusal"),
    }

    for id in [
        "com.example.meters.other",
        "com.example.meters.handled",
        "unnamespaced",
        "com.example.meters.broken",
    ] {
        assert_eq!(clip_type_spec(id), None, "{id} is listed");
        assert_eq!(
            aria_models::effect_spec(id),
            None,
            "{id} left its generator"
        );
    }
}

const METER_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(vec3<f32>(param_level()), 1.0);
}
";

#[test]
fn a_package_clip_type_with_its_own_shader_draws_itself() {
    let scratch = tempfile::tempdir().expect("scratch");
    let dir = package_with(
        scratch.path(),
        "[[clip_types]]\nid = \"meter\"\nname = \"Meter\"\ncolour = \"green\"\nshader = \"meter.wgsl\"\ntime_dependent = true\nhandles = [\"at\"]\nparams = [\n  { name = \"level\", label = \"Level\", kind = \"scalar\", default = 0.25, min = 0.0, max = 1.0 },\n  { name = \"at\", label = \"At\", kind = \"vec2\", default_vec2 = [0.5, 0.5], min = 0.0, max = 1.0 },\n]\n",
    );
    std::fs::write(dir.join("meter.wgsl"), METER_FS).expect("shader");
    let package = Package::open(&dir).expect("opens");
    let ctx = PluginContext::new();
    ctx.mount(&DataPlugin::new(package)).expect("mounts");

    let id = "com.example.titles.meter";
    let listed = clip_type_spec(id).expect("listed");
    assert_eq!(listed.generator, id);
    assert_eq!(listed.unavailable(), None);
    let drawn = aria_models::effect_spec(id).expect("the generator is in the effect catalog");
    assert!(drawn.generator && drawn.time_dependent);
    let names: Vec<&str> = drawn
        .params
        .iter()
        .map(|param| param.name.as_str())
        .collect();
    assert_eq!(names, ["level", "at"]);
    assert!(aria_compositor::renders_effect(id));

    ctx.unmount("com.example.titles").expect("unmounts");
    assert_eq!(clip_type_spec(id), None);
    assert_eq!(aria_models::effect_spec(id), None);
}

#[test]
fn a_package_clip_type_draws_with_a_generator_or_its_own_shader_not_both() {
    let scratch = tempfile::tempdir().expect("scratch");
    for (tables, says) in [
        (
            "[[clip_types]]\nid = \"a\"\nname = \"A\"\ngenerator = \"g\"\ncolour = \"red\"\nshader = \"a.wgsl\"\n",
            "declares `generator` and its own `shader` or `passes`",
        ),
        (
            "[[clip_types]]\nid = \"a\"\nname = \"A\"\ncolour = \"red\"\n",
            "declares no `generator`, `shader` or `passes`",
        ),
        (
            "[[clip_types]]\nid = \"a\"\nname = \"A\"\ngenerator = \"g\"\ncolour = \"red\"\ntime_dependent = true\n",
            "belong to a clip type with its own",
        ),
        (
            "[[clip_types]]\nid = \"a\"\nname = \"A\"\ncolour = \"red\"\nshader = \"a.wgsl\"\nhandles = [\"level\"]\nparams = [{ name = \"level\", label = \"Level\" }]\n",
            "the handle `level` names no vec2 param",
        ),
        (
            "[[generators]]\nid = \"a\"\nname = \"A\"\nshader = \"a.wgsl\"\n\n[[clip_types]]\nid = \"a\"\nname = \"A\"\ncolour = \"red\"\nshader = \"a.wgsl\"\n",
            "declared twice",
        ),
    ] {
        let dir = package_with(scratch.path(), tables);
        let err = Package::open(&dir).expect_err(tables);
        assert!(err.to_string().contains(says), "{tables}: {err}");
    }
}
