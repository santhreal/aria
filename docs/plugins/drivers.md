# Driver languages

A driver replaces one property's keyframed value with a value computed per
frame ([spec/animation.md](../spec/animation.md#drivers)). A plugin adds a
language for driver code with `PluginContext::register_driver_language`. Preview,
export, and the audio mix evaluate drivers written in it.

```rust
use std::sync::Arc;

use aria_plugin::{
    DriverInput, DriverLanguage, DriverLanguageSpec, DriverProgram, DriverScope, InputClip,
    InputProperty, ParamValue, PluginContext, PluginError,
};

/// `follow NAME`: the property takes the opacity of the clip named NAME.
struct Follow;

struct FollowProgram {
    inputs: Vec<DriverInput>,
}

impl DriverLanguage for Follow {
    fn compile(&self, source: &str) -> Result<Arc<dyn DriverProgram>, String> {
        let name = source
            .strip_prefix("follow ")
            .ok_or("write `follow <clip name>`")?;
        Ok(Arc::new(FollowProgram {
            inputs: vec![DriverInput {
                clip: InputClip::Named(name.trim().to_owned()),
                property: InputProperty::Id("opacity".into()),
            }],
        }))
    }

    fn reference(&self, input: &DriverInput) -> Option<String> {
        match (&input.clip, &input.property) {
            (InputClip::Named(name), InputProperty::Id(id)) if id == "opacity" => {
                Some(format!("follow {name}"))
            }
            _ => None,
        }
    }
}

impl DriverProgram for FollowProgram {
    fn inputs(&self) -> &[DriverInput] {
        &self.inputs
    }

    fn evaluate(&self, scope: &mut dyn DriverScope) -> Result<ParamValue, String> {
        scope.input(0)
    }
}

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_driver_language(
        DriverLanguageSpec {
            id: "example.follow".into(),
            name: "Follow".into(),
        },
        Follow,
    )?;
    Ok(())
}
```

The returned `Registration` is held on the terms of `register_effect`.

Native `aria-sdk` plugins register a driver language with
`Host::register_driver_language(spec, impl DriverLanguage)` or
`Host::register_driver_language_spec(&spec)` paired with
`Plugin::driver_evaluate`. WebAssembly guests call
`register-driver-language(spec)` and export `driver-evaluate(id, expr, vars) ->
result<param-value, string>`. Both external tiers pass `time`, `clip_time`,
`frame_rate`, `duration`, `canvas`, `value`, and `seed` to the guest evaluator;
in `aria-sdk` they are the fields of `DriverVars`: `time` (sequence seconds),
`clip_time` (seconds since the clip's first frame), `frame_rate` (sequence
frames per second), `duration` (sequence length in seconds), `canvas`
(`[width, height]` in pixels), `value` (the property's keyframed value at
`time`, a `ParamValue`), and `seed` (a number fixed per clip and property).
The SDK `ParamValue` is `Scalar(f32)`, `Vec2([x, y])`, `Vec3([x, y, z])`,
`Color([r, g, b, a])` in RGBA8, or `Rect([x, y, width, height])`.

## Language

`compile` turns source text into a `DriverProgram`. Its error is the driver's
fault. Aria caches compiled programs by language and
source and compiles again after any language is registered or removed.

`reference` returns source that reads one input. The pick whip in Effect Controls
inserts it at the end of the code being edited. The default returns `None`,
and the pick whip then inserts nothing.

`template` returns the source a new driver starts with. Alt+click on a
stopwatch attaches it. Source that returns the property's own value, such as
`value` in `aria.expressions`, leaves the property unchanged until the code is
edited. The default is empty.

The id contains ASCII letters, digits, `.`, `-` and `_`. A second registration
of an id in one host is refused. When two hosts in one process register the
same id, the latest live registration compiles the code.

## Program

`inputs` lists every property the program reads. Aria checks the graph these
inputs form when code is attached and refuses code that reads its own value
back, directly or through other drivers. Evaluation refuses a cycle a clip
rename creates later.

An input names a clip, `InputClip::This` or `InputClip::Named` with a clip's
title, and a property, by catalog id (`InputProperty::Id`) or as an effect
parameter by the effect's label or id and the parameter's label or name
(`InputProperty::Effect`). Labels and names match without regard to ASCII
case.

`evaluate` reads the scope and returns a value of the property's kind. The
scope supplies:

| Call | Value |
| --- | --- |
| `time` | Sequence seconds being evaluated |
| `clip_time` | Seconds since the clip's first frame |
| `frame_rate`, `duration`, `canvas` | Sequence rate, length in seconds, canvas pixels |
| `value`, `value_at` | The property's keyframed value |
| `key_count`, `key_time` | The property's keyframes |
| `input`, `input_at` | A declared input, after its own driver |
| `clip_info` | A clip's name, in point, and out point |
| `seed` | A number fixed per clip and property |
| `expired` | Whether the evaluation is past its budget |

Values are in model units: position is the anchor's offset from the canvas
center as a fraction of the canvas, scale and opacity are fractions, rotation
is in degrees, a color is four bytes.

## Failure

An error, a panic, or a value of another kind leaves the property at its
keyframed value and records a fault for that driver alone. Effect Controls shows
the value in the danger color with the fault in a tooltip;
`aria_render::driver_fault` and `driver_faults` read faults back. A value
outside the property's range is clamped.

An evaluation that takes longer than the driver budget (20 ms by default) is
stopped and does not run again until its code changes. A long loop checks
`expired` and returns an error. Time spent compiling and evaluating is the
plugin's frame cost in the Plugins list.
