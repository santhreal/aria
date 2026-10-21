# Effects and transitions

A plugin adds an effect or a transition by supplying a catalog spec and WGSL.
The compositor draws it like a shipped one: on clips, in the effect rack, in
the browser and the palette, in previews and in exports.

```rust
use aria_plugin::{EffectDefinition, EffectParamKind, EffectParamSpec, EffectSpec};

const TINT: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let px = textureSample(input_tex, aria_sampler, in.uv);
    let wave = 0.5 + 0.5 * sin(aria.time * param_speed());
    return vec4<f32>(mix(px.rgb, param_tint().rgb * px.a, wave), px.a);
}";

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_effect(EffectDefinition {
        spec: EffectSpec {
            id: "example.pulse_tint".into(),
            label: "Pulse Tint".into(),
            category: "Stylize".into(),
            params: vec![
                EffectParamSpec::scalar("speed", "Speed", 2.0, 0.0, 20.0),
                EffectParamSpec::color("tint", "Tint", [255, 120, 0, 255]),
            ],
            groups: Vec::new(),
            generator: false,
            stateful: false,
            time_dependent: true,
            ..Default::default()
        },
        passes: vec![TINT.into()],
    })?;
    Ok(())
}
```

`category` is the Effect menu submenu the effect is listed under. An empty
category lists it under Other. A generator is not listed in the Effect menu.

`register_effect` and `register_transition` return a `Registration`. During
`mount` the context keeps it: the entry stays until the plugin unmounts, and a
failed mount removes it. Outside a mount the caller keeps it, and dropping it
removes the entry. Clips that use a removed effect draw their input unchanged,
a clip of a removed generator draws a `Missing generator` slate, and a removed
transition plays as a cut.

Native `aria-sdk` plugins register WGSL effects, generators, and transitions
with `Host::register_effect`, `Host::register_effect_passes`,
`Host::register_effect_graph`, `Host::register_generator`,
`Host::register_generator_passes`, `Host::register_generator_graph`, and
`Host::register_transition`, and plugin textures with `Host::register_texture`.
WebAssembly guests call `register-effect`, `register-generator`,
`register-effect-graph`, `register-texture`, and `register-transition`, and may
update a registered effect's Effect menu submenu with
`set-effect-category(id, category)`. A package declares them in `plugin.toml`
([Packages](packages.md)).

`time_dependent: true` states that the effect reads `aria.time`. A pass of an
effect that does not set it reads `aria.time` as `0.0`, so its frame depends on
its parameters alone and a cached frame of it stays valid at any time.

## Generators

An effect whose spec sets `generator: true` draws a picture of its own. The
sidebar's Generators tab and the command palette list it. Picking it places a
clip at the playhead; dragging it from the sidebar places the clip on the lane
and at the time it lands on. The clip is a transparent solid the size of the
frame, named after the effect, with the effect first in its chain. Placing it
is one undo step.

The generator's `input_tex` holds the transparent solid, so `fs` draws from
`aria.size`, `aria.time`, `in.uv`, and its parameters. The clip's transform,
opacity, mask, blend mode, and the effects after the generator apply to the
generated picture as to any layer. The effect lists also offer a generator, and
on another clip it draws over that clip's picture.

A placed generator saves `generator: true` on its instance. A clip whose
generator is no longer registered keeps the effect, its parameters, and its
keyframes, draws a slate stating `Missing generator` and the effect id, and
shows `Missing plugin: <id>` on the timeline. An effect that is no generator
passes its input through, and a transition plays as a cut.

A clip type that declares its fields registers a generator under the type's
id whose parameters are the fields. The Generators tab, the Effects panel, and
the palette list such a generator through its type only. See
[Defined types](clip-types.md#defined-types).

## CPU effects and generators

A plugin registers a CPU effect or generator (`spec.generator == true`) with
`ctx.register_cpu_effect(spec, effect)` or
`ctx.register_effect(EffectDefinition::cpu(spec, effect))`. The effect
implements `CpuEffect`:

```rust
use aria_plugin::{
    CpuFrameIn, CpuFrameOut, EffectParamKind, EffectParamSpec, EffectParams, EffectSpec,
    PluginContext, PluginError,
};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_cpu_effect(
        EffectSpec {
            id: "example.cpu_invert".into(),
            label: "CPU Invert".into(),
            category: "Color".into(),
            params: vec![EffectParamSpec::scalar("amount", "Amount", 1.0, 0.0, 1.0)],
            groups: Vec::new(),
            generator: false,
            stateful: false,
            ..Default::default()
        },
        |input: CpuFrameIn<'_>,
         output: &mut CpuFrameOut<'_>,
         params: &EffectParams<'_>,
         _time: f32| {
            let amount = params.scalar("amount").unwrap_or(1.0);
            for y in 0..input.height() {
                for x in 0..input.width() {
                    let [r, g, b, a] = input.pixel_f32(x, y);
                    output.set_pixel_f32(
                        x,
                        y,
                        [
                            r + (1.0 - 2.0 * r) * amount,
                            g + (1.0 - 2.0 * g) * amount,
                            b + (1.0 - 2.0 * b) * amount,
                            a,
                        ],
                    );
                }
            }
            Ok(())
        },
    )?;
    Ok(())
}
```

`CpuFrameIn` and `CpuFrameOut` hold premultiplied `RGBA16F` pixels (`[u16; 4]`
IEEE-754 binary16 channels, or `[f32; 4]` through `pixel_f32` and
`set_pixel_f32`) with `width()`, `height()`, and `row_stride()`. `EffectParams`
reads evaluated parameter values by name (`scalar`, `angle`, `int`, `boolean`,
`choice`, `vec2`, `size`, `point3`, `color`, `curve_lut`, `gradient_lut`,
`text`, `font`, `file`, `layer`) or slot index (`scalar_at`, `vec2_at`,
`point3_at`, `color_at`, `values`).

The compositor reuses its readback and upload staging buffers across frames of
the same dimensions. CPU effects are not cached. Returning `Err(reason)` leaves
the input frame as passthrough for that frame and records a fault in
`ctx.faults()`. A panic in `process` is caught, leaves the frame as passthrough
so the rest of the chain renders, and fails the plugin for the session through
`ctx.fail`.

Native `aria-sdk` plugins register CPU effects and generators with
`Host::register_cpu_effect(spec, effect)` or `Host::register_cpu_generator(spec,
effect)`, or implement `Plugin::cpu_effect_process` with
`Host::register_cpu_effect_spec` / `Host::register_cpu_generator_spec`.
WebAssembly guests call `register-cpu-effect(spec)` (with `spec.generator =
true` for a CPU generator) and export `cpu-effect-process(id, params, frame) ->
result<video-frame, string>`. Both external tiers exchange packed `width *
height * 4` premultiplied `RGBA16F` (`u16` IEEE-754 binary16) channels directly
with the guest without `u8` quantization; `aria_sdk::f16::to_f32` and
`aria_sdk::f16::from_f32` convert individual channels on the native guest side.

The SDK `CpuFrameIn` has `width()`, `height()`, `pixels()` (the packed
channels, top row first, without row padding), `pixel(x, y)` and
`pixel_f32(x, y)` (zero outside the frame), and `time()` (clip seconds). It
reads parameters by name with the `EffectParams` names above, as flattened
floats with `params()` and `param_at(index)`, and as the effect instance JSON
with `params_json()` (empty when there is none). The SDK `CpuFrameOut` has
`width()`, `height()`, `pixels()`, `pixels_mut()`, `set_pixel(x, y, rgba)` and
`set_pixel_f32(x, y, rgba)` (ignored outside the frame), and `fill(rgba)` and
`fill_f32(rgba)`, which set every pixel.

`aria_sdk::EffectSpec::new` takes the id and label, and `with_category`,
`with_params`, `with_groups`, `with_generator`, `with_stateful`,
`with_bounds`, `with_reads_outside`, and `with_time_dependent` set the field of
the same name. `TransitionSpec::new` takes the id and label, and
`with_params` sets its parameters. `EffectParamKind::flattened_floats()` is the
storage float count in the table under [Shader source](#shader-source).

## Effect graphs

An effect is a list of `GraphPass`es. Each pass has a name, a stage, the
textures it reads, an output size, and its WGSL. A plain list of fragment
passes is a chain: a pass with no inputs reads the pass declared before it, or
the effect's input for the first pass.

```rust
use aria_plugin::{
    EffectBounds, EffectDefinition, EffectSpec, GraphPass, OutputSize, PassInput, RgbaImage,
};

const HALVE: &str = "@compute @workgroup_size(8, 8, 1)
fn cs(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(aria.size);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let c = textureLoad(input_tex, vec2<i32>(id.xy) * 2, 0);
    textureStore(output_tex, vec2<i32>(id.xy), c * 0.5);
}";

const MIX: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let small = textureSample(pass_half, aria_sampler, in.uv);
    let grain = textureSample(texture_example_grain, aria_sampler, in.uv * 8.0);
    return textureSample(source_tex, aria_sampler, in.uv) + small * grain.r;
}";

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    let grain = RgbaImage::new(64, 64, grain_pixels());
    ctx.register_texture("example.grain", &grain)?;
    ctx.register_effect(EffectDefinition {
        spec: EffectSpec {
            id: "example.bloom".into(),
            label: "Bloom".into(),
            bounds: EffectBounds::uniform(4.0),
            ..Default::default()
        },
        passes: vec![
            GraphPass::compute(HALVE, [8, 8])
                .named("half")
                .sized(OutputSize::Scale([0.5, 0.5])),
            GraphPass::fragment(MIX).reading([
                PassInput::Source,
                PassInput::pass("half"),
                PassInput::texture("example.grain"),
            ]),
        ],
    })?;
    Ok(())
}
```

| Field | Values | Default |
| --- | --- | --- |
| `name` | ASCII letters, digits, `_`; unique in the effect | `pass<index>` |
| `stage` | `PassStage::Fragment`, or `PassStage::Compute { workgroup: [x, y] }` | `Fragment` |
| `inputs` | `PassInput::Source`, `Pass(name)`, `Layer(param)`, `Texture(id)`, in binding order | the previous pass, or `Source` for the first |
| `output` | `OutputSize::Same`, `Scale([x, y])`, `Fixed([w, h])` | `Same` |
| `source` | WGSL | |

The passes form an acyclic graph. Aria orders them so every pass runs after
the passes it reads, keeping declaration order wherever the graph allows. The
last declared pass writes the effect's frame, uses `OutputSize::Same`, and
reads every other pass, directly or through other passes.

A pass writes `Scale([x, y])` as the input size times the factors, rounded,
at least one pixel, at most `MAX_PASS_SIDE` (8192) per side. A pass reading a
pass of another size samples it over the same `0..1` UV range.

### Inputs

| Input | Bound as | Content |
| --- | --- | --- |
| `Source` | `source_tex` | The effect's input frame |
| `Pass(name)` | `pass_<name>` | The named pass's output |
| `Layer(param)` | `layer_<param>` | The clip the `EffectParamKind::Layer` parameter `param` references |
| `Texture(id)` | `texture_<id>` | The plugin texture `id`; each character other than an ASCII letter, digit, or `_` becomes `_` |

The first input is also bound as `input_tex`.

A layer input holds the referenced clip's frame at the same instant, drawn with
its transform, effects, and LUT on a transparent canvas-sized frame. A
parameter that references no clip, a clip that draws nothing at that instant,
and an adjustment clip read transparent. The model refuses a
reference to the clip itself and a reference that closes a cycle through other
clips' layer parameters; the effect row in Effect Controls shows the refusal.

### Plugin textures

`ctx.register_texture(id, &image)` registers a straight-alpha `RgbaImage` as a
texture. Aria premultiplies it once and uploads it the first time a frame
reads it. A pass reads it with `PassInput::Texture(id)`. Register a texture
before the effects that read it: registration refuses an effect reading a
texture that is not registered. During `mount` the context keeps the
registration until the plugin unmounts. An effect keeps the image it was
registered with while the effect is registered.

Registration refuses an id that is empty, holds characters other than ASCII
letters, digits, `_`, `.`, and `-`, or is already registered; an image with a
zero side or a side above `MAX_TEXTURE_SIDE` (8192); and a pixel buffer that is
not `width * height * 4` bytes. The refusal is
`PluginError::TextureRefused(TextureError)`.

### Bounds and traits

`EffectSpec` states three traits of the frame the effect writes:

- `bounds: EffectBounds { left, top, right, bottom }`: pixels the effect draws
  past each edge of its layer. A click in the viewer on that margin selects
  the layer. Margins of effects in one chain add.
- `reads_outside`: the effect samples its input outside `0..1`. Every pass
  reads its whole input, and `aria_sampler` clamps to the edge, so such a read
  returns the edge texel. The trait is part of the effect's frame cache key.
- `time_dependent`: the effect reads `aria.time`. Without it, `aria.time` is
  `0.0` in every pass.

## Shader source

Each pass is the plugin's WGSL followed by a host prelude. The prelude declares
`struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }`
and the vertex stage `vs`. A fragment pass writes `@fragment fn fs` returning
`@location(0) vec4<f32>`. A compute pass writes
`@compute @workgroup_size(x, y, 1) fn cs` with the workgroup its stage declares,
and stores its result with `textureStore(output_tex, ...)`; Aria dispatches it
over the pass's output in workgroup-sized tiles. A pass declares no bindings of
its own. Line numbers in a refusal are lines of the plugin's source.

Textures hold premultiplied RGBA, and a pass writes premultiplied RGBA.

Effect bindings, group 0:

| Binding | Name | Content |
| --- | --- | --- |
| 0 | `input_tex` | The pass's first input |
| 1 | `source_tex` | The effect's input |
| 2 | `aria_sampler` | Linear sampler, clamped to edge |
| 3 | `aria` | Read-only storage buffer `AriaPass` (`var<storage, read>`) |
| 4.. | `pass_<name>`, `layer_<param>`, `texture_<id>` | Each input other than `Source`, in input order (`texture_2d<f32>`) |
| last | `output_tex` | Compute passes only: `texture_storage_2d<rgba8unorm, write>` |

`AriaPass` fields:

| Field | WGSL type | Description |
| --- | --- | --- |
| `size` | `vec2<f32>` | Output dimensions `[width, height]` of the current pass in pixels. |
| `texel` | `vec2<f32>` | Reciprocal pixel dimensions `[1.0 / width, 1.0 / height]`. |
| `time` | `f32` | Seconds since the start of the clip, or `0.0` when `time_dependent` is `false`. |
| `pass_index` | `u32` | Zero-based execution index of the current pass. |
| `pass_count` | `u32` | Total number of passes in the effect (`1..=8`). |
| `pad` | `u32` | Alignment padding word. |
| `params` | `array<f32>` | Flattened parameter storage (`MAX_EFFECT_FLOATS` = 256 floats max). |

Each shader-visible parameter gets a generated WGSL accessor `param_<name>()`:

| `EffectParamKind` | `param_<name>()` return type | Storage floats |
| --- | --- | --- |
| `Scalar`, `Int`, `Bool`, `Choice`, `Angle`, `Layer` | `f32` | 1 |
| `Vec2`, `Size` | `vec2<f32>` | 2 |
| `Point3` | `vec3<f32>` | 3 |
| `Color` | `vec4<f32>` (linear RGBA in `0..1`) | 4 |
| `Curve` | `array<f32, 64>` (64-sample LUT over `0..1`) | 64 |
| `Gradient` | `array<vec4<f32>, 64>` (64-sample linear RGBA LUT over `0..1`) | 256 |
| `Text`, `Font`, `File` | CPU-only (`EffectParams` / `CpuFrameIn`) | 0 |

`Scalar`, `Int`, `Color`, `Vec2`, `Point3`, `Size`, and `Angle` parameters are
keyframeable and drivable; `Vec2` and `Point3` parameters also show draggable
on-canvas handles in the Viewer. An `EffectParamSpec` may set `soft_min`,
`soft_max`, `unit`, `display` (`Slider`, `AngleDial`, `Percentage`), `tooltip`,
`group` (referencing an `EffectGroupSpec { id, label, collapsed }` in
`EffectSpec::groups`), and `visible_when` (`EffectVisibilityCondition { param,
equals }`).

In `aria-sdk`, `EffectParamSpec::display` is an `EffectParamDisplay`, each
entry of `choices` is an `EffectChoiceOption { id, label }`, each entry of
`default_curve` is an `EffectCurvePoint { x, y }` with `x` in `0..1`, and each
entry of `default_gradient` is an `EffectGradientStop { position, color }` with
`position` in `0..1` and `color` linear RGBA. An empty `default_curve` is the
identity from `(0, 0)` to `(1, 1)`, and an empty `default_gradient` is black to
white. `EFFECT_CURVE_LUT_SAMPLES` and `EFFECT_GRADIENT_LUT_SAMPLES` are the
sample counts of the `Curve` and `Gradient` lookup tables in the table above.

Fragment effect pass example (`shaders/warm-grade.wgsl`, with scalar parameter
`gain`):

```wgsl
@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let px = textureSample(input_tex, aria_sampler, in.uv);
    let g = param_gain();
    return vec4<f32>(px.rgb * vec3<f32>(g, g * 0.95, g * 0.85), px.a);
}
```

Compute pass example (`shaders/grain-half.wgsl`, with `stage = { compute =
{ workgroup = [8, 8] } }`):

```wgsl
@compute @workgroup_size(8, 8, 1)
fn cs(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(aria.size);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let c = textureLoad(input_tex, vec2<i32>(id.xy) * 2, 0);
    textureStore(output_tex, vec2<i32>(id.xy), c);
}
```

Transition bindings, group 0:

| Binding | Name | WGSL type | Content |
| --- | --- | --- | --- |
| 0 | `outgoing_tex` | `texture_2d<f32>` | Outgoing clip frame (premultiplied RGBA) |
| 1 | `incoming_tex` | `texture_2d<f32>` | Incoming clip frame (premultiplied RGBA) |
| 2 | `aria_sampler` | `sampler` | Linear sampler, clamped to edge |
| 3 | `aria` | `var<storage, read> aria: AriaTransition` | Transition uniform header and parameters |

`AriaTransition` fields:

| Field | WGSL type | Description |
| --- | --- | --- |
| `size` | `vec2<f32>` | Output frame dimensions `[width, height]` in pixels. |
| `texel` | `vec2<f32>` | Reciprocal pixel dimensions `[1.0 / width, 1.0 / height]`. |
| `progress` | `f32` | Normalized transition progress in `0.0..=1.0`. |
| `pad0`, `pad1`, `pad2` | `f32` | Alignment padding words. |
| `params` | `array<f32>` | Flattened transition parameter storage. |

Each parameter declared in `TransitionSpec::params` gets a `param_<name>()`
accessor with the same return type as an effect parameter.

Transition shader example (`shaders/flash-cut.wgsl`, with scalar parameter
`softness`):

```wgsl
@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let a = textureSample(outgoing_tex, aria_sampler, in.uv);
    let b = textureSample(incoming_tex, aria_sampler, in.uv);
    let edge = clamp(param_softness(), 0.01, 1.0);
    let t = smoothstep(0.5 - edge * 0.5, 0.5 + edge * 0.5, aria.progress);
    return mix(a, b, t);
}
```

## Limits and refusals

Registration parses and validates every pass with naga and returns
`PluginError::RegisterRefused(RegisterError)` when:

- the id has no `.` namespace, or is already in the catalog;
- the label is empty, a parameter name repeats or is not a WGSL identifier, or
  a range is inverted;
- an effect has no passes, or more than `MAX_EFFECT_PASSES` (8);
- a pass name is not an identifier or repeats; an input names no pass, the
  pass itself, a parameter that is not an `EffectParamKind::Layer` parameter,
  or a texture id with characters other than ASCII letters, digits, `_`, `.`,
  and `-`; a pass binds one name twice or reads more than `MAX_PASS_INPUTS`
  (8) inputs;
- a workgroup has a zero side or more than `MAX_WORKGROUP_INVOCATIONS` (256)
  invocations; a scale factor is not finite, not positive, or above
  `MAX_PASS_SCALE` (16); a fixed size has a zero side or a side above
  `MAX_PASS_SIDE` (8192);
- the passes form a cycle (the message spells it out, as in
  ``pass `a` is on a cycle: a reads b reads a``); a pass feeds nothing the last
  pass reads; the last pass does not use `OutputSize::Same`;
- a pass reads a texture that is not registered;
- the WGSL does not parse or validate, does not declare the entry point its
  stage needs (`fs`, or `cs` with the declared workgroup size), or declares a
  binding.

`RegisterError` holds the `id`, the `pass` index, the `location` as
`(line, column)` in the pass source, and the `message`. A refusal of a named
pass's WGSL starts with ``pass `<name>`:``.

A device that rejects a validated pipeline logs the error once; the effect
then draws its input unchanged.

The compositor records each mounted plugin's per-frame render cost in
`ctx.costs()` and `MountedPluginInfo::frame_cost`, and the Plugins section in
Settings displays it beside the plugin's mount time. For a `CpuEffect`, frame
cost measures the `process` callback together with its `u8 <-> f16` staging
conversion and texture upload. For a WGSL effect or transition, frame cost
measures the CPU-side uniform preparation, bind-group lookup, and render-pass
command encoding across the effect's passes (the compositor device does not
enable GPU timestamp queries). When an effect or transition pass exceeds the
sequence or preview frame budget (`1 / fps`), the host records the latest
over-budget measurement as a `PluginFault` in `ctx.faults()` without unmounting
the plugin.
