//! Native C ABI clip types that declare their fields and draw themselves: an
//! `aria-sdk` guest bound to the host's own table defines one type drawn by
//! its CPU callback and one drawn by WGSL passes. Each type names its own
//! generator, the generator lists the declared fields, each draws its field
//! value, and unmount removes both entries of each. A type with fields that
//! names another generator fails the mount.
//!
//! Defect class closed: a `schema` the sdk sends and the host table drops,
//! misparses, or keeps past unmount, and a CPU type whose callback is not
//! reached by the type's id or cannot read its fields by name. The rules of a
//! definition itself are covered by `aria-plugin`'s unit tests.

#![allow(unsafe_code)]

use std::sync::LazyLock;

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, F16_ONE, GpuContext, LayerPlacement,
    PassInstance, RgbaImage, f32_to_f16, pass_id, registered_effect, renders_effect,
};
use aria_models::{clip_type_spec, effect_spec};
use aria_plugin::PluginContext;
use aria_plugin::external::{from_table, host_table};
use aria_plugin::package::{MANIFEST_FILE, Package};
use aria_plugin_abi::ARIA_API_MAJOR;
use aria_sdk::{
    ClipTypeSchema, ClipTypeSpec, CpuFrameIn, CpuFrameOut, EffectParamSpec, GraphPass, Host,
    MarkerColor, Plugin as SdkPlugin, PluginRuntime,
};
use parking_lot::Mutex;
use tempfile::TempDir;

#[macro_use]
#[path = "support/plugin_table.rs"]
mod plugin_table;

static SERIAL: Mutex<()> = Mutex::new(());

const W: u32 = 32;
const H: u32 = 16;

/// Grey at the `level` field, opaque.
const METER_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(vec3<f32>(param_level()), 1.0);
}
";

const TYPES: [&str; 2] = ["native.meter.cpu", "native.meter.wgsl"];

fn schema() -> ClipTypeSchema {
    ClipTypeSchema::new(vec![EffectParamSpec::scalar(
        "level", "Level", 0.25, 0.0, 1.0,
    )])
}

/// Fill the frame with opaque grey at the `level` field.
fn grey(_host: &Host, input: &CpuFrameIn<'_>, output: &mut CpuFrameOut<'_>) -> Result<(), String> {
    let level = input.scalar("level").ok_or("no level field")?;
    for y in 0..output.height() {
        for x in 0..output.width() {
            output.set_pixel_f32(x, y, [level, level, level, 1.0]);
        }
    }
    Ok(())
}

struct MeterGuest;

impl SdkPlugin for MeterGuest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.define_cpu_clip_type(
            &ClipTypeSpec::new(TYPES[0], "CPU Meter", "", MarkerColor::Green),
            &schema(),
            grey,
        )?;
        host.define_clip_type(
            &ClipTypeSpec::new(TYPES[1], "WGSL Meter", TYPES[1], MarkerColor::Blue),
            &schema(),
            &[GraphPass::fragment(METER_FS)],
        )
    }
}

struct ForeignGuest;

impl SdkPlugin for ForeignGuest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.define_clip_type(
            &ClipTypeSpec::new(
                "native.meter.foreign",
                "Foreign Meter",
                "native.meter.other",
                MarkerColor::Red,
            ),
            &schema(),
            &[GraphPass::fragment(METER_FS)],
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
name = "Native Meter"
version = "0.1.0"
api = 1
publisher = "Aria Tests"
description = "Native C-ABI clip type test package"
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
fn native_clip_types_list_their_fields_draw_them_and_leave_on_unmount() {
    let _serial = SERIAL.lock();
    static RT: LazyLock<PluginRuntime<MeterGuest>> = LazyLock::new(|| bound(MeterGuest));
    let temp = TempDir::new().expect("temp dir");
    let plugin_id = "test.native.meter";
    let (ctx, plugin) = loaded(&temp, plugin_id, &plugin_table!(RT));
    ctx.mount(plugin.as_ref()).expect("mount meter plugin");

    for id in TYPES {
        let listed = clip_type_spec(id).expect("the type is listed");
        assert_eq!(listed.generator, id, "{id} names another generator");
        let drawn = effect_spec(id).expect("the type's generator is in the effect catalog");
        assert!(drawn.generator, "{id}");
        let fields: Vec<&str> = drawn.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(fields, ["level"], "{id}");
        assert!(renders_effect(id), "{id}");
    }

    let cpu = registered_effect(TYPES[0]).expect("the CPU type renders");
    let blank = [0u16; 16];
    let mut out = [0u16; 16];
    assert!(
        cpu.run_cpu(
            aria_compositor::CpuFrameIn::new(&blank, 2, 2, 8),
            &mut aria_compositor::CpuFrameOut::new(&mut out, 2, 2, 8),
            &[0.5],
            0.0,
        ),
        "the callback failed: {:?}",
        ctx.faults()
    );
    let half = f32_to_f16(0.5);
    assert_eq!(out[..4], [half, half, half, F16_ONE]);
    assert_eq!(out[12..], [half, half, half, F16_ONE]);
    drop(cpu);

    let gpu = GpuContext::new_headless_blocking().expect("lavapipe headless GPU");
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(W, H).with_background([0, 0, 0, 255]);
    // The generator draws over the whole layer: the plate's colour is gone.
    let plate = RgbaImage::new(W, H, [200, 100, 50, 255].repeat((W * H) as usize));
    let pass = PassInstance::new(pass_id(TYPES[1]).expect("pass id"), vec![0.5]);
    let layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&pass));
    let img = comp
        .render(&gpu, &config, &[layer])
        .expect("render the WGSL type");
    assert_px(&img, 16, 8, [128, 128, 128, 255], 2);

    ctx.unmount(plugin_id).expect("unmount meter plugin");
    for id in TYPES {
        assert!(clip_type_spec(id).is_none(), "{id} outlived its plugin");
        assert!(effect_spec(id).is_none(), "{id} left its generator");
        assert!(!renders_effect(id), "{id} still renders");
    }
}

#[test]
fn a_native_type_with_fields_naming_another_generator_fails_the_mount() {
    let _serial = SERIAL.lock();
    static RT: LazyLock<PluginRuntime<ForeignGuest>> = LazyLock::new(|| bound(ForeignGuest));
    let temp = TempDir::new().expect("temp dir");
    let (ctx, plugin) = loaded(&temp, "test.native.meter.foreign", &plugin_table!(RT));
    let error = ctx
        .mount(plugin.as_ref())
        .expect_err("a type with fields naming another generator mounts")
        .to_string();
    assert!(error.contains("draws itself"), "{error}");
    assert!(clip_type_spec("native.meter.foreign").is_none());
    assert!(effect_spec("native.meter.foreign").is_none());
}
