//! Native C ABI effect graphs and plugin textures, end to end: an `aria-sdk`
//! guest bound to the host's own table registers a texture and a compute plus
//! fragment graph reading it, the effect draws on the GPU, and unmount removes
//! both. A graph the host refuses fails the mount with the planner's reason.
//!
//! Defect class closed: a graph or texture that the sdk sends and the host
//! table drops, misparses, or keeps past unmount. Graph planning and the
//! runtime's per-pass behaviour are covered by `aria-models` and
//! `aria-compositor`.

#![allow(unsafe_code)]

use std::sync::LazyLock;

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, GpuContext, LayerPlacement, PassInstance,
    RgbaImage, pass_id, plugin_texture, renders_effect,
};
use aria_models::effect_spec;
use aria_plugin::PluginContext;
use aria_plugin::external::{from_table, host_table};
use aria_plugin::package::{MANIFEST_FILE, Package};
use aria_plugin_abi::ARIA_API_MAJOR;
use aria_sdk::{EffectSpec, GraphPass, Host, PassInput, Plugin as SdkPlugin, PluginRuntime};
use parking_lot::Mutex;
use tempfile::TempDir;

#[macro_use]
#[path = "support/plugin_table.rs"]
mod plugin_table;

static SERIAL: Mutex<()> = Mutex::new(());

const W: u32 = 32;
const H: u32 = 16;

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
    let t = textureSample(texture_native_graph_lut, aria_sampler, in.uv);
    return vec4<f32>(h.r, h.g, t.b, 1.0);
}
";

const ZERO_FRAGMENT: &str =
    "@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }";

struct GraphGuest;

impl SdkPlugin for GraphGuest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_texture("native.graph.lut", 1, 1, &[0, 0, 255, 255])?;
        host.register_effect_graph(
            &EffectSpec::new("native.graph.fx", "Native Graph").with_category("Stylize"),
            &[
                GraphPass::compute(HALVE_COMPUTE, [8, 8]).named("half"),
                GraphPass::fragment(MIX_FRAGMENT).reading([
                    PassInput::Pass("half".to_owned()),
                    PassInput::Texture("native.graph.lut".to_owned()),
                ]),
            ],
        )
    }
}

struct CycleGuest;

impl SdkPlugin for CycleGuest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_effect_graph(
            &EffectSpec::new("native.graph.cycle", "Native Cycle").with_category("Stylize"),
            &[
                GraphPass::fragment(ZERO_FRAGMENT)
                    .named("a")
                    .reading([PassInput::Pass("b".to_owned())]),
                GraphPass::fragment(ZERO_FRAGMENT)
                    .named("b")
                    .reading([PassInput::Pass("a".to_owned())]),
                GraphPass::fragment(ZERO_FRAGMENT).reading([PassInput::Pass("a".to_owned())]),
            ],
        )
    }
}

fn bound<P: SdkPlugin>(guest: P) -> PluginRuntime<P> {
    let runtime = PluginRuntime::new(guest);
    // SAFETY: `host_table()` is the host's static `AriaHostV1`.
    assert!(unsafe { runtime.bind_host(host_table(), ARIA_API_MAJOR) });
    runtime
}

fn loaded(
    temp: &TempDir,
    id: &str,
    table: &aria_plugin_abi::AriaPluginV1,
) -> (PluginContext, Box<dyn aria_plugin::Plugin>) {
    let dir = temp.path().join(id);
    std::fs::create_dir_all(&dir).expect("create package dir");
    let manifest = format!(
        r#"[plugin]
id = "{id}"
name = "Native Graph"
version = "0.1.0"
api = 1
publisher = "Aria Tests"
description = "Native C-ABI effect graph test package"
license = "MIT"
"#
    );
    std::fs::write(dir.join(MANIFEST_FILE), manifest).expect("write plugin.toml");
    let pkg = Package::open(&dir).expect("open package");
    let plugin = from_table(&pkg, *table, Some(temp.path())).expect("from_table");
    (PluginContext::new(), plugin)
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
fn a_native_graph_reads_its_texture_on_the_gpu_and_unmount_removes_both() {
    let _serial = SERIAL.lock();
    static RT: LazyLock<PluginRuntime<GraphGuest>> = LazyLock::new(|| bound(GraphGuest));
    let temp = TempDir::new().expect("temp dir");
    let plugin_id = "test.native.graph";
    let (ctx, plugin) = loaded(&temp, plugin_id, &plugin_table!(RT));
    ctx.mount(plugin.as_ref()).expect("mount graph plugin");

    let texture = plugin_texture("native.graph.lut").expect("texture registered");
    assert_eq!((texture.width(), texture.height()), (1, 1));
    drop(texture);
    let spec = effect_spec("native.graph.fx").expect("graph effect in catalog");
    assert_eq!(spec.category, "Stylize");
    assert!(renders_effect("native.graph.fx"));

    let gpu = GpuContext::new_headless_blocking().expect("lavapipe headless GPU");
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(W, H).with_background([0, 0, 0, 255]);
    let plate = RgbaImage::new(W, H, [200, 100, 50, 255].repeat((W * H) as usize));
    let pass = PassInstance::new(pass_id("native.graph.fx").expect("pass id"), Vec::new());
    let layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&pass));
    let img = comp.render(&gpu, &config, &[layer]).expect("render graph");
    // Red and green halved by the compute pass, blue from the texture.
    assert_px(&img, 16, 8, [100, 50, 255, 255], 2);

    ctx.unmount(plugin_id).expect("unmount graph plugin");
    assert!(effect_spec("native.graph.fx").is_none());
    assert!(!renders_effect("native.graph.fx"));
    assert!(
        plugin_texture("native.graph.lut").is_none(),
        "unmount leaves the texture registered"
    );
}

#[test]
fn a_native_graph_with_a_cycle_fails_the_mount_naming_it() {
    let _serial = SERIAL.lock();
    static RT: LazyLock<PluginRuntime<CycleGuest>> = LazyLock::new(|| bound(CycleGuest));
    let temp = TempDir::new().expect("temp dir");
    let (ctx, plugin) = loaded(&temp, "test.native.graph.cycle", &plugin_table!(RT));
    let error = ctx
        .mount(plugin.as_ref())
        .expect_err("a cyclic graph mounts")
        .to_string();
    assert!(error.contains("cycle"), "{error}");
    assert!(effect_spec("native.graph.cycle").is_none());
    assert!(!renders_effect("native.graph.cycle"));
}
