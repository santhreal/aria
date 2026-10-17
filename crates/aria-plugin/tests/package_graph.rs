//! Package TOML effect graphs and textures, end to end: a data package with
//! no code declares an image under `[[textures]]` and an effect whose
//! `[[effects.passes]]` tables run a compute pass and a fragment pass reading
//! it and the texture. Mounting registers both, the effect draws on the GPU,
//! and unmounting removes both. An image the host does not register fails the
//! mount naming the file, before any effect registers.
//!
//! Defect class closed: a graph field, texture, or effect trait that
//! `plugin.toml` declares and registration drops or reads differently, and a
//! texture image decoded before its size is checked. Manifest parsing rules
//! are rows of the package unit tests; graph planning and per-pass behaviour
//! are covered by `aria-models` and `aria-compositor`.

use std::path::Path;

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, GpuContext, LayerPlacement, PassInstance,
    RgbaImage, pass_id, plugin_texture, renders_effect,
};
use aria_models::{EffectBounds, effect_spec};
use aria_plugin::package::{DataPlugin, MANIFEST_FILE, Package};
use aria_plugin::{PluginContext, PluginError};
use parking_lot::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

const W: u32 = 32;
const H: u32 = 16;

/// A 1x1 straight-alpha PNG of opaque blue.
const BLUE_PNG: [u8; 70] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x60, 0x60, 0xf8, 0xff,
    0x1f, 0x00, 0x03, 0x02, 0x01, 0xff, 0x39, 0x29, 0x19, 0xbe, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// An 8193x1 transparent PNG: one pixel wider than a texture may be.
const WIDE_PNG: [u8; 111] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x20, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x99, 0x89, 0x4b,
    0x5e, 0x00, 0x00, 0x00, 0x36, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0xed, 0xc1, 0x01, 0x0d, 0x00,
    0x00, 0x00, 0xc2, 0xa0, 0xf7, 0x4f, 0xed, 0x66, 0x0e, 0xa0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x1b, 0x80, 0x05, 0x00, 0x01, 0xac,
    0x06, 0xf1, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

const HALVE_COMPUTE: &str = "@compute @workgroup_size(8, 8, 1)
fn cs(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(aria.size);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let c = textureLoad(input_tex, vec2<i32>(id.xy), 0);
    textureStore(output_tex, vec2<i32>(id.xy), vec4<f32>(c.rgb * 0.5, c.a));
}
";

const MIX_FRAGMENT: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let h = textureSample(pass_half, aria_sampler, in.uv);
    let t = textureSample(texture_com_test_pkggraph_lut, aria_sampler, in.uv);
    return vec4<f32>(h.r, h.g, t.b, 1.0);
}
";

const GRAPH_EFFECT: &str = "
[[textures]]
id = \"lut\"
file = \"textures/lut.png\"

[[effects]]
id = \"fx\"
name = \"Package Graph\"
category = \"Stylize\"
time_dependent = true
reads_outside = true
bounds = { left = 2, top = 3, right = 4, bottom = 5 }

[[effects.passes]]
name = \"half\"
file = \"shaders/half.wgsl\"
stage = { compute = { workgroup = [8, 8] } }

[[effects.passes]]
file = \"shaders/mix.wgsl\"
inputs = [{ pass = \"half\" }, { texture = \"lut\" }]
";

fn write(dir: &Path, relative: &str, contents: &[u8]) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("create parent");
    std::fs::write(path, contents).expect("write");
}

/// The data package `id` declaring [`GRAPH_EFFECT`], with `image` as its
/// texture file.
fn package(root: &Path, id: &str, image: &[u8]) -> Package {
    let dir = root.join(id);
    let manifest = format!(
        "[plugin]\nid = {id:?}\nname = \"Graph\"\nversion = \"1.0.0\"\napi = 1\n{GRAPH_EFFECT}"
    );
    write(&dir, MANIFEST_FILE, manifest.as_bytes());
    write(&dir, "shaders/half.wgsl", HALVE_COMPUTE.as_bytes());
    write(&dir, "shaders/mix.wgsl", MIX_FRAGMENT.as_bytes());
    write(&dir, "textures/lut.png", image);
    Package::open(&dir).expect("opens")
}

#[track_caller]
fn assert_px(img: &RgbaImage, x: u32, y: u32, expect: [u8; 4], tol: i32) {
    let got = img.pixel(x, y);
    for ch in 0..4 {
        let d = i32::from(got[ch]) - i32::from(expect[ch]);
        assert!(
            d.abs() <= tol,
            "pixel({x},{y}) = {got:?}, expected ~{expect:?} (channel {ch} off by {d}, tol {tol})"
        );
    }
}

#[test]
fn a_declared_graph_reads_its_declared_texture_on_the_gpu_and_unmount_removes_both() {
    let _serial = SERIAL.lock();
    let root = tempfile::tempdir().expect("scratch dir");
    let id = "com.test.pkggraph";
    let ctx = PluginContext::new();
    ctx.mount(&DataPlugin::new(package(root.path(), id, &BLUE_PNG)))
        .expect("mounts");

    let texture = plugin_texture("com.test.pkggraph.lut").expect("texture registered");
    assert_eq!((texture.width(), texture.height()), (1, 1));
    drop(texture);
    let spec = effect_spec("com.test.pkggraph.fx").expect("graph effect in catalog");
    assert_eq!(spec.category, "Stylize");
    assert!(spec.time_dependent, "time_dependent was dropped");
    assert!(spec.reads_outside, "reads_outside was dropped");
    assert_eq!(spec.bounds, EffectBounds::new(2.0, 3.0, 4.0, 5.0));
    assert!(renders_effect("com.test.pkggraph.fx"));

    let gpu = GpuContext::new_headless_blocking().expect("lavapipe headless GPU");
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(W, H).with_background([0, 0, 0, 255]);
    let plate = RgbaImage::new(W, H, [200, 100, 50, 255].repeat((W * H) as usize));
    let pass = PassInstance::new(
        pass_id("com.test.pkggraph.fx").expect("pass id"),
        Vec::new(),
    );
    let layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&pass));
    let img = comp.render(&gpu, &config, &[layer]).expect("render graph");
    // Red and green halved by the compute pass, blue from the texture.
    assert_px(&img, 16, 8, [100, 50, 255, 255], 2);

    ctx.unmount(id).expect("unmounts");
    assert!(effect_spec("com.test.pkggraph.fx").is_none());
    assert!(!renders_effect("com.test.pkggraph.fx"));
    assert!(
        plugin_texture("com.test.pkggraph.lut").is_none(),
        "unmount leaves the texture registered"
    );
}

#[test]
fn a_texture_image_the_host_does_not_register_fails_the_mount_by_name() {
    let _serial = SERIAL.lock();
    let root = tempfile::tempdir().expect("scratch dir");
    for (id, image, reason) in [
        (
            "com.test.pkgwide",
            &WIDE_PNG[..],
            Some("8193x1 is larger than 8192 pixels on a side"),
        ),
        ("com.test.pkgtext", &b"not an image"[..], None),
    ] {
        let ctx = PluginContext::new();
        let err = ctx
            .mount(&DataPlugin::new(package(root.path(), id, image)))
            .expect_err("an unregistered texture fails the mount");
        let PluginError::MountFailed(_, detail) = &err else {
            panic!("{id}: unexpected error: {err:?}");
        };
        let prefix = format!("{id}.lut: textures/lut.png: ");
        assert!(detail.starts_with(&prefix), "{id}: {detail}");
        if let Some(reason) = reason {
            assert_eq!(&detail[prefix.len()..], reason, "{id}");
        }
        assert!(plugin_texture(&format!("{id}.lut")).is_none(), "{id}");
        assert!(
            effect_spec(&format!("{id}.fx")).is_none(),
            "{id}: the effect registered without its texture"
        );
    }
}
