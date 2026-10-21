# Clip types

A clip type is a named kind of clip whose picture a generator draws. It has a
name, a timeline colour, an icon, and Viewer handles bound to the generator's
vec2 parameters. The Generators tab lists each type above the plain
generators, and the palette offers it by name. Clicking a type places it at
the playhead; dragging it places it on a lane.

Placing a type adds a transparent solid with the generator first in its
effect chain, names the clip after the type, and records the type's id on the
clip, as one undo step. The generator's parameters are the type's data: they
are stored in the project with the clip, keyframe like any effect parameter,
and show as the clip's rows in the Effects block of Effect Controls and in the
timeline's Effects group.

A type is registered in one of two forms. A listed type names a generator
registered on its own. A defined type declares its fields and its renderer
together: the host registers a generator under the type's id whose
parameters are the fields, and one registration holds both. See
[Defined types](#defined-types).

A package declares clip types in `plugin.toml`, which serves every tier,
including a package with no code. The generator is one the package declares:

```toml
[[generators]]
id = "card-gen"
name = "Card"
shader = "card.wgsl"
params = [
  { name = "anchor", label = "Anchor", kind = "vec2", default_vec2 = [0.1, 0.8], min = 0.0, max = 1.0 },
]

[[clip_types]]
id = "lower-third"
name = "Lower third"
generator = "card-gen"
colour = "orange"
icon = "icons/third.svg"
handles = ["anchor"]
```

| Field | Value |
| --- | --- |
| `id` | The type's id, under the package id |
| `name` | The name in the Generators tab, the palette, Effect Controls, and on the clip |
| `generator` | A generator the package declares, under the package id |
| `colour` | `teal`, `blue`, `purple`, `pink`, `red`, `orange`, `yellow`, or `green` |
| `icon` | A path to an SVG in the package, optional |
| `handles` | Names of the generator's vec2 parameters drawn on the Viewer, optional |

An in-process plugin registers one with `PluginContext::register_clip_type`
(or `Host::register_clip_type` in `aria-sdk` and `register-clip-type` in
WebAssembly):

```rust
use aria_models::{ClipTypeSpec, MarkerColor};
use aria_plugin::{PluginContext, PluginError};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    let mut spec = ClipTypeSpec::new(
        "com.example.titles.lower-third",
        "Lower third",
        "com.example.titles.card-gen",
        MarkerColor::Orange,
    );
    spec.handles = vec!["anchor".into()];
    ctx.register_clip_type(spec)?;
    Ok(())
}
```

`aria_sdk::ClipTypeSpec` holds `id`, `label` (the `name` field above),
`generator`, `colour`, `icon_svg` (the SVG source of the icon, or `None`), and
`handles`. `ClipTypeSpec::new(id, label, generator, colour)` builds one with no
icon and no handles; `with_icon_svg(svg)` and `with_handles(handles)` set
them. `aria_sdk::MarkerColor` is `Teal`, `Blue`, `Purple`, `Pink`, `Red`,
`Orange`, `Yellow`, or `Green`, the variants of the `colour` names above.

## Handles

A handle value is a point on the clip's layer, as fractions of its width and
height: `[0, 0]` is the top left corner and `[1, 1]` the bottom right. With
the clip selected and the Viewer in Transform mode, each handle is a dot in
the type's colour above the move surface and the scale corners. Dragging one
shows the value in the Viewer as it moves and writes one keyframe at the
playhead on release, held inside the parameter's range. A press with no
movement writes nothing. A type declares at most `CLIP_TYPE_MAX_HANDLES` (8)
handles.

## The clip

The timeline draws a typed clip in the label of the type's colour unless the
clip has a label of its own, with the type's icon before its name. `pink` draws
as Magenta, `red` as Rose, and `orange` as Mango; the other five names are
labels of the same name. Effect Controls shows the
type's name and icon above the tabs with Detach, which removes the type from
the clip and keeps its chain and name.

A project keeps a clip's type when no plugin lists it. The clip keeps its
chain, Effect Controls shows the id with "no plugin lists the clip type", and
the timeline shows `Missing plugin: <id>` on the clip. A type whose generator is not registered,
is an effect rather than a generator, or has no vec2 parameter a handle names
is listed with the reason, and placing it reports the same reason.

## Refusals

`register_clip_type` returns `PluginError::ClipTypeRefused(message)` when the
id has no `.` namespace, is already in the catalog, is over
`CLIP_TYPE_ID_MAX_BYTES` (128), or holds a space or control character; when the
name is blank or over `CLIP_TYPE_NAME_MAX_BYTES` (64); when the generator is
empty; when a handle is empty or declared twice; and when the icon is not an
SVG the host reads. A package whose `[[clip_types]]` entry fails these checks,
names an unknown field, or repeats an id fails to load with the entry named.

The returned `Registration` is held on the terms of `register_effect`. A type
whose registration drops leaves the Generators tab and the palette; clips that
hold it keep its id.

## Defined types

A defined type declares its fields as effect parameters of any kind: scalar,
angle, int, bool, choice, colour, vec2, size, point3, curve, gradient, text,
file, font, and layer. Each field is a row in Effect Controls and on the
timeline, saved with the clip, edited as one undo step per change, and
keyframed where its kind keyframes. A handle names a `vec2` field.

The generator registered for the type has the type's id and name, and the
Generators tab, the Effects panel, and the palette list the type only. A
defined type is drawn in one of two ways:

- **Passes**: WGSL passes on the terms of an effect graph (see
  [Effects](effects.md)), reading the fields as `aria.params`.
- **CPU**: a callback handed the field values, the time in seconds into the
  clip, and a transparent premultiplied `RGBA16F` frame of the clip's layer
  size. The time is 0 unless the type is `time_dependent`. Returning an error
  leaves the clip transparent for that frame and records a fault; a panic
  fails the plugin for the session.

In `plugin.toml`, a `[[clip_types]]` entry with `shader` or `passes` instead
of `generator` is a defined type. `params`, `groups`, and `time_dependent`
take the `[[effects]]` forms:

```toml
[[clip_types]]
id = "meter"
name = "Meter"
colour = "green"
shader = "meter.wgsl"
params = [
  { name = "level", label = "Level", kind = "scalar", default = 0.25, min = 0.0, max = 1.0 },
]
```

An in-process plugin calls `PluginContext::define_clip_type`:

```rust
use aria_compositor::{CpuFrameOut, EffectParams};
use aria_models::{ClipTypeSpec, EffectParamKind, EffectParamSpec, MarkerColor};
use aria_plugin::{ClipTypeDefinition, ClipTypeRender, PluginContext, PluginError};

fn grey(fields: &EffectParams<'_>, _time: f32, frame: &mut CpuFrameOut<'_>) -> Result<(), String> {
    let level = fields.scalar("level").ok_or("no level field")?;
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            frame.set_pixel_f32(x, y, [level, level, level, 1.0]);
        }
    }
    Ok(())
}

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    let level = EffectParamSpec {
        name: "level".into(),
        label: "Level".into(),
        kind: EffectParamKind::Scalar,
        default: 0.25,
        max: 1.0,
        ..Default::default()
    };
    ctx.define_clip_type(ClipTypeDefinition::new(
        ClipTypeSpec::new("com.example.meters.meter", "Meter", "", MarkerColor::Green),
        vec![level],
        ClipTypeRender::cpu(grey),
    ))?;
    Ok(())
}
```

`ClipTypeRender::Passes(passes)` draws the type with WGSL instead.
`ClipTypeDefinition::with_groups` and `with_time_dependent` set the field
groups and the time dependence.

A native plugin calls `Host::define_clip_type(&spec, &schema, &passes)` or
`Host::define_cpu_clip_type(&spec, &schema, render)` in `aria-sdk`, where
`schema` is a `ClipTypeSchema`; the CPU form's `CpuFrameIn` reads the fields
by name. A WebAssembly plugin imports `define-clip-type` with a
`clip-type-schema` and a `clip-type-render` of `cpu` or `passes`; a `cpu` type
is drawn by its `cpu-effect-process` export called with the type's id. See
[Native plugins](native.md) and [WebAssembly plugins](wasm.md).

`define_clip_type` returns `PluginError::ClipTypeRefused` when the spec names
another generator, a handle names no `vec2` field, or the spec fails the
checks in [Refusals](#refusals), and `PluginError::RegisterRefused` when a
field or a pass fails the checks of `register_effect`. A `[[clip_types]]`
entry that declares `generator` together with `shader` or `passes`, declares
none of the three, or sets `params`, `groups`, or `time_dependent` beside a
`generator` fails to load with the entry named. A defined type's id is also an
effect id, so no `[[effects]]` or `[[generators]]` entry may repeat it.
