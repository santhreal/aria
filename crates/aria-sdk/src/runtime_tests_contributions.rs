//! Visual effects, CPU effects, effect graphs, textures, transitions,
//! generators, clip types, drivers, presets, themes, workspaces, and `f16`
//! frame conversions.

use parking_lot::Mutex;
use std::ffi::c_void;

use aria_plugin_abi::{
    ARIA_API_MAJOR, ARIA_PARAM_COLOR, ARIA_PARAM_RECT, ARIA_PARAM_SCALAR, ARIA_PARAM_VEC2,
    ARIA_STATUS_ERR, ARIA_STATUS_OK, ARIA_STATUS_PANIC, AriaCpuEffectCall, AriaDriverEvalCall,
    AriaParamValue, AriaStr, free_rust_string,
};

use crate::{
    ClipTypeSpec, CpuFrameIn, CpuFrameOut, DriverLanguageSpec, DriverVars, EffectParamSpec,
    EffectSpec, GraphPass, Host, MarkerColor, OutputSize, ParamValue, PassInput, Plugin,
    PluginRuntime, PresetSpec, TransitionSpec, WorkspaceFocus, WorkspacePanes, WorkspaceSpec,
};

use super::{MOCK_HOST, MockHostState};

struct ContribPlugin;

impl Plugin for ContribPlugin {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_effect(
            &EffectSpec::new("example.tint", "Tint")
                .with_category("Color")
                .with_params(vec![EffectParamSpec::scalar(
                    "amount", "Amount", 0.5, 0.0, 1.0,
                )]),
            "@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }",
        )?;
        host.register_cpu_effect(
            EffectSpec::new("example.invert", "Invert").with_category("Color"),
            |_host: &Host, input: &CpuFrameIn<'_>, output: &mut CpuFrameOut<'_>| {
                let delta = input.param_at(0).unwrap_or(0.0);
                for y in 0..input.height() {
                    for x in 0..input.width() {
                        let [r, g, b, a] = input.pixel_f32(x, y);
                        output.set_pixel_f32(x, y, [1.0 - r, 1.0 - g, (1.0 - b) + delta, a]);
                    }
                }
                Ok(())
            },
        )?;
        host.register_transition(
            &TransitionSpec::new("example.wipe", "Wipe"),
            "@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.5); }",
        )?;
        host.register_generator(
            &EffectSpec::new("example.bars", "Bars Generator").with_category("Generate"),
            "@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.25); }",
        )?;
        host.register_cpu_generator(
            EffectSpec::new("example.solid", "Solid Generator").with_category("Generate"),
            |_host: &Host, _input: &CpuFrameIn<'_>, output: &mut CpuFrameOut<'_>| {
                output.fill_f32([0.25, 0.5, 0.75, 1.0]);
                Ok(())
            },
        )?;
        host.register_clip_type(
            &ClipTypeSpec::new(
                "example.card",
                "Title Card",
                "example.solid",
                MarkerColor::Teal,
            )
            .with_handles(vec!["origin".to_owned()]),
        )?;
        host.register_driver_language(
            DriverLanguageSpec::new("example.expr", "Example Expr"),
            |_host: &Host, expr: &str, vars: &DriverVars| match expr {
                "double" => match vars.value {
                    ParamValue::Scalar(v) => Ok(ParamValue::Scalar(v * 2.0)),
                    other => Ok(other),
                },
                "vec2" => Ok(ParamValue::Vec2([vars.time as f32, vars.clip_time as f32])),
                "color" => Ok(ParamValue::Color([1, 2, 3, 255])),
                "rect" => Ok(ParamValue::Rect([0.1, 0.2, 0.3, 0.4])),
                "err" => Err("bad expr".to_owned()),
                "panic" => panic!("driver exploded"),
                _ => Ok(vars.value),
            },
        )?;
        host.register_preset(
            &PresetSpec::new("Soft Fade")
                .with_effect("vignette")
                .with_scalar_property("opacity", 0.75),
        )?;
        host.register_theme("midnight", "[dark]\nbg = \"#101216\"\n")?;
        host.register_workspace(&WorkspaceSpec::new("color", "Color Grading").with_panes(
            WorkspacePanes {
                scopes: Some(true),
                focus: Some(WorkspaceFocus::Preview),
                ..WorkspacePanes::default()
            },
        ))?;
        Ok(())
    }

    fn cpu_effect_process(
        &self,
        _host: &Host,
        id: &str,
        _input: &CpuFrameIn<'_>,
        _output: &mut CpuFrameOut<'_>,
    ) -> Result<(), String> {
        if id == "example.panic" {
            panic!("cpu effect exploded");
        }
        Err(format!("no cpu effect {id}"))
    }
}

struct GraphPlugin;

impl Plugin for GraphPlugin {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_texture("example.lut", 2, 1, &[255, 0, 0, 255, 0, 0, 255, 128])?;
        host.register_effect_graph(
            &EffectSpec::new("example.graph", "Graph").with_category("Stylize"),
            &[
                GraphPass::compute("compute-src", [8, 4])
                    .named("half")
                    .sized(OutputSize::Scale([0.5, 0.5])),
                GraphPass::fragment("fragment-src").reading([
                    PassInput::Pass("half".to_owned()),
                    PassInput::Source,
                    PassInput::Layer("matte".to_owned()),
                    PassInput::Texture("example.lut".to_owned()),
                ]),
            ],
        )
    }
}

/// The graph reaches the host as the JSON array `register_effect` reads in
/// place of WGSL, field for field, and a texture reaches `register_texture`
/// with its size and bytes. A host table that ends before `register_texture`
/// refuses the texture without calling past its end.
#[test]
fn an_effect_graph_and_a_texture_cross_the_abi_as_declared() {
    let runtime = PluginRuntime::new(GraphPlugin);
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);
    assert_eq!(runtime.handle_mount(ctx).status, ARIA_STATUS_OK);
    {
        let st = state.lock();
        assert_eq!(
            st.textures,
            [(
                "example.lut".to_owned(),
                2,
                1,
                vec![255, 0, 0, 255, 0, 0, 255, 128]
            )]
        );
        assert_eq!(st.effects.len(), 1);
        let (spec, graph) = &st.effects[0];
        let spec: serde_json::Value = serde_json::from_str(spec).expect("spec JSON");
        assert_eq!(spec["id"], "example.graph");
        let graph: serde_json::Value = serde_json::from_str(graph).expect("graph JSON");
        assert_eq!(
            graph,
            serde_json::json!([
                {
                    "name": "half",
                    "stage": { "compute": { "workgroup": [8, 4] } },
                    "inputs": [],
                    "output": { "scale": [0.5, 0.5] },
                    "source": "compute-src",
                },
                {
                    "name": "",
                    "stage": "fragment",
                    "inputs": [
                        { "pass": "half" },
                        "source",
                        { "layer": "matte" },
                        { "texture": "example.lut" },
                    ],
                    "output": "same",
                    "source": "fragment-src",
                },
            ])
        );
    }

    let mut short_host = MOCK_HOST;
    short_host.struct_size = aria_plugin_abi::ARIA_HOST_V1_QUERIES_SIZE;
    assert!(short_host.has_project_queries());
    assert!(!short_host.has_textures());
    let legacy = PluginRuntime::new(GraphPlugin);
    #[allow(unsafe_code)]
    let bound = unsafe { legacy.bind_host(&short_host, ARIA_API_MAJOR) };
    assert!(bound);
    let refused = legacy.handle_mount(ctx);
    assert_eq!(refused.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let refused = unsafe { refused.value.into_string_with(free_rust_string) };
    assert!(refused.contains("texture registration"), "{refused}");
    assert_eq!(
        state.lock().textures.len(),
        1,
        "a host without register_texture is never called"
    );
}

#[test]
fn contributions_register_and_dispatch_cpu_effect_and_driver_across_abi() {
    let runtime = PluginRuntime::new(ContribPlugin);
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);

    assert_eq!(runtime.handle_mount(ctx).status, ARIA_STATUS_OK);
    {
        let st = state.lock();
        assert_eq!(st.effects.len(), 1);
        assert_eq!(st.cpu_effects.len(), 1);
        assert_eq!(st.transitions.len(), 1);
        assert_eq!(st.generators.len(), 2);
        assert_eq!(st.clip_types.len(), 1);
        assert_eq!(st.driver_languages.len(), 1);
        assert_eq!(st.presets.len(), 1);
        assert_eq!(st.themes.len(), 1);
        assert_eq!(st.workspaces.len(), 1);
    }

    // 1. CPU effect processes a 1x1 RGBA16F frame with scalar param (preserving >1.0 HDR values).
    let input_px = [
        crate::f16::from_f32(0.75),
        crate::f16::from_f32(0.25),
        crate::f16::from_f32(-0.5),
        crate::f16::ONE,
    ];
    let mut output_px = [0u16; 4];
    let params = [2.5f32];
    let mut call = AriaCpuEffectCall {
        input: input_px.as_ptr(),
        output: output_px.as_mut_ptr(),
        pixels_len: 4,
        width: 1,
        height: 1,
        params: params.as_ptr(),
        params_len: 1,
        params_json: AriaStr::from_str(""),
        time: 0.5,
    };
    #[allow(unsafe_code)]
    let res = unsafe {
        runtime.handle_cpu_effect_process(ctx, AriaStr::from_str("example.invert"), &raw mut call)
    };
    assert_eq!(res.status, ARIA_STATUS_OK);
    assert_eq!(
        output_px,
        [
            crate::f16::from_f32(0.25),
            crate::f16::from_f32(0.75),
            crate::f16::from_f32(4.0),
            crate::f16::ONE,
        ]
    );

    // 2. Generator fills the output frame.
    let mut gen_px = [0u16; 4];
    call.output = gen_px.as_mut_ptr();
    call.params = std::ptr::null();
    call.params_len = 0;
    #[allow(unsafe_code)]
    let gen_res = unsafe {
        runtime.handle_cpu_effect_process(ctx, AriaStr::from_str("example.solid"), &raw mut call)
    };
    assert_eq!(gen_res.status, ARIA_STATUS_OK);
    assert_eq!(
        gen_px,
        [
            crate::f16::from_f32(0.25),
            crate::f16::from_f32(0.5),
            crate::f16::from_f32(0.75),
            crate::f16::ONE,
        ]
    );

    // 3. Panicking CPU effect catches unwind and returns ARIA_STATUS_PANIC.
    #[allow(unsafe_code)]
    let panic_res = unsafe {
        runtime.handle_cpu_effect_process(ctx, AriaStr::from_str("example.panic"), &raw mut call)
    };
    assert_eq!(panic_res.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let panic_msg = unsafe { panic_res.value.into_string_with(free_rust_string) };
    assert_eq!(panic_msg, "cpu effect exploded");

    // 4. Driver evaluate handles Scalar, Vec2, Color, Rect, Err, and Panic.
    let mut drv_call = AriaDriverEvalCall {
        time: 1.5,
        clip_time: 0.5,
        frame_rate: 24.0,
        duration: 10.0,
        canvas_width: 1920,
        canvas_height: 1080,
        value: AriaParamValue {
            kind: ARIA_PARAM_SCALAR,
            x: 0.4,
            ..AriaParamValue::default()
        },
        seed: 42,
        output: AriaParamValue::default(),
    };
    #[allow(unsafe_code)]
    let drv_ok = unsafe {
        runtime.handle_driver_evaluate(
            ctx,
            AriaStr::from_str("example.expr"),
            AriaStr::from_str("double"),
            &raw mut drv_call,
        )
    };
    assert_eq!(drv_ok.status, ARIA_STATUS_OK);
    assert_eq!(drv_call.output.kind, ARIA_PARAM_SCALAR);
    assert!((drv_call.output.x - 0.8).abs() < 1e-6);

    for (expr, expected_kind) in [
        ("vec2", ARIA_PARAM_VEC2),
        ("color", ARIA_PARAM_COLOR),
        ("rect", ARIA_PARAM_RECT),
    ] {
        #[allow(unsafe_code)]
        let res = unsafe {
            runtime.handle_driver_evaluate(
                ctx,
                AriaStr::from_str("example.expr"),
                AriaStr::from_str(expr),
                &raw mut drv_call,
            )
        };
        assert_eq!(res.status, ARIA_STATUS_OK);
        assert_eq!(drv_call.output.kind, expected_kind);
    }

    #[allow(unsafe_code)]
    let drv_err = unsafe {
        runtime.handle_driver_evaluate(
            ctx,
            AriaStr::from_str("example.expr"),
            AriaStr::from_str("err"),
            &raw mut drv_call,
        )
    };
    assert_eq!(drv_err.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let drv_err_msg = unsafe { drv_err.value.into_string_with(free_rust_string) };
    assert_eq!(drv_err_msg, "bad expr");

    #[allow(unsafe_code)]
    let drv_panic = unsafe {
        runtime.handle_driver_evaluate(
            ctx,
            AriaStr::from_str("example.expr"),
            AriaStr::from_str("panic"),
            &raw mut drv_call,
        )
    };
    assert_eq!(drv_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let drv_panic_msg = unsafe { drv_panic.value.into_string_with(free_rust_string) };
    assert_eq!(drv_panic_msg, "driver exploded");

    // 5. Old host table (SCOPES_SIZE) refuses contribution registrations cleanly.
    let mut scopes_only_host = MOCK_HOST;
    scopes_only_host.struct_size = aria_plugin_abi::ARIA_HOST_V1_SCOPES_SIZE;
    assert!(scopes_only_host.has_scopes());
    assert!(!scopes_only_host.has_contributions());
    let legacy = PluginRuntime::new(ContribPlugin);
    #[allow(unsafe_code)]
    let bound = unsafe { legacy.bind_host(&scopes_only_host, ARIA_API_MAJOR) };
    assert!(bound);
    let refused = legacy.handle_mount(ctx);
    assert_eq!(refused.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let refused_msg = unsafe { refused.value.into_string_with(free_rust_string) };
    assert!(refused_msg.contains("effect registration"), "{refused_msg}");
}

/// Closes the defect class where `aria_sdk::f16::{to_f32, from_f32}` corrupts
/// subnormal, negative, HDR (`> 1.0`), infinity, or NaN IEEE-754 binary16
/// bit patterns.
#[test]
fn f16_helpers_round_trip_every_u16_bit_pattern() {
    for bits in 0u16..=u16::MAX {
        let f = crate::f16::to_f32(bits);
        let back = crate::f16::from_f32(f);
        if (bits & 0x7c00) == 0x7c00 && (bits & 0x03ff) != 0 {
            assert!(f.is_nan(), "NaN f16 {bits:#06x} must decode to NaN");
            assert_eq!(back & 0x8000, bits & 0x8000, "NaN sign preserved");
            assert_eq!(back & 0x7c00, 0x7c00, "NaN exponent preserved");
            assert_ne!(back & 0x03ff, 0, "NaN mantissa non-zero");
        } else {
            assert_eq!(
                back, bits,
                "f16 {bits:#06x} -> {f} -> {back:#06x} failed round-trip"
            );
        }
    }
    assert_eq!(crate::f16::from_f32(1.0), crate::f16::ONE);
    assert_eq!(crate::f16::to_f32(crate::f16::ONE), 1.0);
    assert_eq!(crate::f16::to_f32(crate::f16::from_f32(4.0)), 4.0);
    assert_eq!(crate::f16::to_f32(crate::f16::from_f32(-1.25)), -1.25);
}

/// Closes the defect class where `aria_sdk::f16::from_f32` truncates, rounds
/// ties away from zero, or rounds to the wrong neighbour. Over every pair of
/// adjacent finite binary16 values of either sign, the exact midpoint rounds
/// to the even pattern and one `f32` ulp to either side rounds to the nearer
/// value; past the largest finite value, the overflow midpoint rounds to
/// infinity.
#[test]
fn f16_from_f32_rounds_every_midpoint_to_nearest_even() {
    for lo in (0u16..0x7bff).chain(0x8000..0xfbff) {
        let hi = lo + 1;
        let mid = (crate::f16::to_f32(lo) + crate::f16::to_f32(hi)) / 2.0;
        let even = if lo % 2 == 0 { lo } else { hi };
        assert_eq!(crate::f16::from_f32(mid), even, "tie {lo:#06x}/{hi:#06x}");
        let nearer_lo = f32::from_bits(mid.to_bits() - 1);
        assert_eq!(crate::f16::from_f32(nearer_lo), lo, "below tie {lo:#06x}");
        let nearer_hi = f32::from_bits(mid.to_bits() + 1);
        assert_eq!(crate::f16::from_f32(nearer_hi), hi, "above tie {lo:#06x}");
    }
    assert_eq!(crate::f16::from_f32(65520.0), 0x7c00);
    assert_eq!(crate::f16::from_f32(-65520.0), 0xfc00);
    assert_eq!(
        crate::f16::from_f32(f32::from_bits(65520f32.to_bits() - 1)),
        0x7bff
    );
}
