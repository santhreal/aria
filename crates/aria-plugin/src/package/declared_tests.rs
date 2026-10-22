use super::*;
use crate::context::PluginContext;
use crate::error::PluginError;

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
