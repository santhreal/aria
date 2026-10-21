# Presets

A preset (`aria_models::Preset`) is a named set of visual effects, clip audio
effects, and clip property curves. Plugin presets appear beside saved presets
in the Effects Rack preset bar. Applying a preset appends its effects and audio
effects to every selected clip and writes its property curves with tick `0` at
the playhead, or at the clip start when the playhead is outside the clip, as one
undo step.

## Package files

A package declares presets as `.ariapreset` files in `presets/`:

```text
my-plugin/
  plugin.toml
  presets/
    soft-vignette.ariapreset
```

Each file is UTF-8 JSON in the `Preset` format. `examples/data-plugin` ships
this one:

```json
{
  "version": 1,
  "name": "Soft Vignette",
  "effects": [
    {
      "effect_id": "vignette"
    }
  ],
  "audio_effects": [],
  "properties": []
}
```

### `Preset` root object

Unknown fields are rejected (`deny_unknown_fields`). At least one of `effects`,
`audio_effects`, or `properties` must be non-empty.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `version` | integer (`u32`) | required | Schema version. Must equal `PRESET_VERSION` (`1`). |
| `name` | string | required | Display name. Trimmed on load; must be 1 to `PRESET_MAX_NAME_CHARS` (`128`) Unicode scalar values without control characters. |
| `effects` | array of `EffectInstance` | `[]` | Visual effect instances appended to the target clip, at most `MAX_PRESET_EFFECTS` (`64`). |
| `audio_effects` | array of `AudioEffectInstance` | `[]` | Clip audio effect instances appended to the target clip, at most `MAX_PRESET_AUDIO_EFFECTS` (`64`). |
| `properties` | array of `PresetProperty` | `[]` | Animatable clip property curves, at most `MAX_PRESET_PROPERTIES` (`256`). |

### `effects[]` (`EffectInstance`)

Only parameters that differ from their catalog default need to be stored;
omitted parameters use the effect's catalog default.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `effect_id` | string | required | Non-empty catalog effect ID (for example `"vignette"` or `"com.example.fx.warm-tint"`), without control characters. |
| `params` | array of `[string, Param<f32>]` pairs | `[]` | Scalar, integer, and angle parameter curves as `[name, value]` pairs, sorted by key on save. |
| `color_params` | array of `[string, Param<[u8; 4]>]` pairs | `[]` | 8-bit sRGB `[R, G, B, A]` color parameter curves (`EffectParamKind::Color`). |
| `linear_color_params` | array of `[string, Param<[f32; 4]>]` pairs | `[]` | Linear RGBA `[r, g, b, a]` color parameter curves (`EffectParamKind::Color`), each channel finite. |
| `vec2_params` | array of `[string, Param<[f32; 2]>]` pairs | `[]` | 2D vector and size parameter curves (`EffectParamKind::Vec2`, `Size`), each component finite. |
| `vec3_params` | array of `[string, Param<[f32; 3]>]` pairs | `[]` | 3D point parameter curves (`EffectParamKind::Point3`), each component finite. |
| `bool_params` | array of `[string, bool]` pairs | `[]` | Boolean parameter values (`EffectParamKind::Bool`). |
| `choice_params` | array of `[string, string]` pairs | `[]` | Selected option IDs (`EffectParamKind::Choice`). |
| `curve_params` | array of `[string, array of { "x": f32, "y": f32 }]` pairs | `[]` | 1D curve control points (`EffectParamKind::Curve`), coordinates finite in `0.0..=1.0` with non-decreasing `x`. |
| `gradient_params` | array of `[string, array of { "offset": f32, "color": [f32; 4] }]` pairs | `[]` | Gradient color stops (`EffectParamKind::Gradient`), `offset` (or alias `position`) finite in `0.0..=1.0` with non-decreasing offsets. |
| `text_params` | array of `[string, string]` pairs | `[]` | String values for `EffectParamKind::Text`, `Font`, and `File`. |
| `layer_params` | array of `[string, u64]` pairs | `[]` | Referenced raw `ClipId` values for `EffectParamKind::Layer`. |
| `generator` | boolean | `false` | Whether `effect_id` was placed as a generator. |
| `bypass` | boolean | `false` | Whether the effect instance starts switched off. |
| `name` | string | none | Custom instance label: 1 to `EFFECT_NAME_MAX_CHARS` (`64`) printable characters without leading or trailing whitespace. |

### `audio_effects[]` (`AudioEffectInstance`)

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `effect_id` | string | required | Non-empty audio effect ID (for example `"eq_3band"`, `"compressor"`, or a plugin audio effect ID), without control characters. |
| `params` | array of `[string, Param<f32>]` pairs | `[]` | Audio parameter curves as `[name, value]` pairs, each value finite. |
| `bypass` | boolean | `false` | Whether the audio insert starts bypassed. |
| `sidechain` | integer (`u64`) | none | Optional `TrackId` of the sidechain key track. |

### `properties[]` (`PresetProperty`)

Unknown fields on `PresetProperty` are rejected (`deny_unknown_fields`). Each
property `id` may appear at most once in `properties`.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | string | required | Property row ID resolved by `property_row`: a clip property row from `clip_param_catalog()` (`"position"`, `"scale"`, `"rotation"`, `"opacity"`, `"anchor_point"`, `"crop"`, or a `look-*`, `style-*`, `text-*`, or `shape-*` row) or a mask property row `mask-<index>-<property>` (for example `"mask-0-feather"`). |
| `curve` | `Param<ParamValue>` | required | Constant `ParamValue` or keyframed `ParamValue` curve (`{"kf": [...]}`) matching the target property's value kind. |

`ParamValue` JSON representation in `properties[].curve`:

| Variant | JSON form | Target properties |
| --- | --- | --- |
| `Scalar` | `{ "scalar": 0.8 }` | Scalar properties (`opacity`, `rotation`, uniform `scale`, `look-*`, scalar `style-*`, `text-*`, `shape-*`, `mask-*-*`). `opacity` requires `0.0..=1.0`; `scale` requires `> 0.0`. |
| `Vec2` | `{ "vec2": [0.0, 0.0] }` | 2D properties (`position`, `anchor_point`, per-axis `scale`, `style-shadow-offset`, `text-position`, `shape-position`). Per-axis `scale` requires both components `> 0.0`. |
| `Vec3` | `{ "vec3": [0.0, 0.0, 0.0] }` | 3D vector properties. |
| `Color` | `{ "color": [255, 255, 255, 255] }` | 8-bit RGBA color properties (`style-*-color`, `text-*-color`, `shape-*-color`). |
| `Rect` | `{ "rect": [0.0, 0.0, 1.0, 1.0] }` | Normalized `[x, y, w, h]` rectangle (`crop`, with `0.0 <= x, y <= 1.0`, `w, h > 0.0`, `x + w <= 1.0`, `y + h <= 1.0`). |

### `Param<T>` and `Keyframe<T>`

Every `Param<T>` (in `effects[].params`, `effects[].*_params`,
`audio_effects[].params`, and `properties[].curve`) serializes as an untagged
union: either a bare constant `T` or an object `{"kf": [...]}` containing 1 to
`MAX_PRESET_KEYFRAMES` (`4096`) keyframes sorted by strictly increasing `t`.

`Keyframe<T>` object keys:

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `t` | integer (`i64`) | required | Anchor-relative offset in timeline ticks (`0` = anchor / playhead). Strictly increasing across `kf`. |
| `v` | `T` | required | Value at tick `t` (`f32`, `[u8; 4]`, `[f32; 4]`, `[f32; 2]`, `[f32; 3]`, or `ParamValue`). |
| `e` | `Easing` | `"linear"` | Outgoing segment interpolation: `"linear"`, `"ease_in"`, `"ease_out"`, `"ease_in_out"`, `"hold"`, `{ "bezier": { "points": [x1, y1, x2, y2] } }` (`x1, x2` in `0.0..=1.0`), or `{ "temporal": { "in_ease": { "speed": f32, "influence": f32 }, "out_ease": { "speed": f32, "influence": f32 }, "roving": bool, "hold": bool, "points": [x1, y1, x2, y2] } }` (`influence` in `0.001..=1.0`, `x1, x2` in `0.0..=1.0`). |
| `s` | `SpatialTangents` (`{ "o": [dx, dy], "i": [dx, dy] }`) | none | Optional 2D cubic Bezier motion-path handles (`o` = outgoing offset, `i` = incoming offset), each component finite in `-4.0..=4.0`. Accepted on `[f32; 2]` position curves (`position`, `vec2_params`) and rejected on non-position properties. |

`SpatialTangents` (`s`) object keys:

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `o` | `[f32; 2]` | `[0.0, 0.0]` | Outgoing tangent offset `[dx, dy]` relative to the keyframe value, each component finite in `-4.0..=4.0`. |
| `i` | `[f32; 2]` | `[0.0, 0.0]` | Incoming tangent offset `[dx, dy]` relative to the keyframe value, each component finite in `-4.0..=4.0`. |

Worked `.ariapreset` example with keyframed effect parameters, an audio insert,
and a keyframed clip property curve:

```json
{
  "version": 1,
  "name": "Punch In and Warm Fade",
  "effects": [
    {
      "effect_id": "vignette",
      "name": "Edge Vignette",
      "params": [
        [
          "amount",
          {
            "kf": [
              { "t": 0, "v": 0.0, "e": "ease_out" },
              { "t": 24, "v": 0.45 }
            ]
          }
        ]
      ]
    }
  ],
  "audio_effects": [
    {
      "effect_id": "compressor",
      "params": [
        ["threshold", -18.0],
        ["ratio", 3.0]
      ]
    }
  ],
  "properties": [
    {
      "id": "scale",
      "curve": {
        "kf": [
          { "t": 0, "v": { "vec2": [1.0, 1.0] }, "e": { "bezier": { "points": [0.25, 0.1, 0.25, 1.0] } } },
          { "t": 24, "v": { "vec2": [1.15, 1.15] } }
        ]
      }
    },
    {
      "id": "opacity",
      "curve": { "scalar": 0.95 }
    }
  ]
}
```

When a preset property targets a mask row (`mask-<index>-<property>`) and a
selected clip has no mask at `<index>`, applying the preset fails and leaves
the clip unchanged. The **Save as preset** button in Effect Controls writes
selected effects, audio effects, and keyframed clip properties in this format.

`Package::open` and `aria plugin check` scan `presets/` for `*.ariapreset`
files in path order (skipping entries with other extensions) without running
plugin code, and reject:

- a `presets` entry that is not a directory;
- a `.ariapreset` file larger than `PRESET_MAX_BYTES` (1 MiB);
- a `.ariapreset` file that fails `Preset::parse` or `Preset::validate`;
- two presets in one package whose trimmed names match without regard to ASCII
  case.

Mounting the package registers each preset under the package's registration.
Unmounting or deactivating the plugin removes them.

## Registering from code

A Rust plugin registers a preset in `mount`:

```rust
use aria_models::{EffectInstance, Preset};
use aria_plugin::{PluginContext, PluginError};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    let preset = Preset::new(
        "Soft Vignette",
        vec![EffectInstance::new("vignette")],
        Vec::new(),
        Vec::new(),
    );
    ctx.register_preset(preset)?;
    Ok(())
}
```

A registration made during `mount` lasts until the plugin unmounts.

Native `aria-sdk` plugins register presets with `Host::register_preset(&spec)`
or `Host::register_preset_json(json)`. `aria_sdk::PresetSpec` holds `version`
(`1`), `name`, `effects`, `audio_effects`, and `properties`; the three lists
hold JSON values with the shapes under
[`Preset` root object](#preset-root-object). `PresetSpec::new(name)` builds a
version 1 preset with empty lists, `with_effect(effect_id)` appends an effect
instance with default parameters, and `with_scalar_property(id, value)` appends
a constant curve of `value` for the clip property `id`, such as `opacity`.
WebAssembly guests pass the preset JSON string to
`register-preset(preset-json)`.

`PluginContext::register_preset` trims the name, validates the preset, and
fails with `PluginError::PresetRefused` when the preset is invalid or the same
plugin already registered a preset of that name, compared without regard to
ASCII case. Dropping the returned `Registration` removes the preset.

`PluginContext::presets()` lists the registered presets in registration order
with the name of the plugin that registered each. `presets_revision()`
increases on every registration and removal.
