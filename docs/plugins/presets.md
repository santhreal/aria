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

| Field | Contents |
| --- | --- |
| `version` | `PRESET_VERSION` (1). |
| `name` | Display name, trimmed, at most `PRESET_MAX_NAME_CHARS` characters. |
| `effects` | Effect instances, at most `MAX_PRESET_EFFECTS` (64). |
| `audio_effects` | Audio effect instances, at most `MAX_PRESET_AUDIO_EFFECTS` (64). |
| `properties` | `{ "id": <clip property row id>, "curve": <constant or keyframes> }`, at most `MAX_PRESET_PROPERTIES` (256), each curve at most `MAX_PRESET_KEYFRAMES` (4096) keyframes. |

Unknown fields are rejected. Keyframe ticks are relative to the anchor. A
property `id` is a row id from `clip_param_catalog()` or a mask row id,
`mask-<index>-<property>` (`mask-0-feather`), which writes onto the mask at that
index of each target clip; a target without that mask refuses the preset and is
left unchanged. Effect parameters travel with their effects, and an effect saved
with `"bypass": true` is added switched off. The
**Save as preset** button in Effect Controls writes property curves in this format; copy one from a saved
preset rather than writing keyframes by hand.

`Package::open` and `aria plugin check` validate every file without running
plugin code, and reject:

- a file in `presets/` without the `.ariapreset` extension;
- a file larger than `PRESET_MAX_BYTES` (1 MiB);
- a file that fails `Preset::parse` or `Preset::validate`;
- two presets in one package whose names differ only in ASCII case.

Mounting the package registers each preset under the package's registration.
Unmounting or deactivating the plugin removes them.

## Registering from code

A native plugin registers a preset in `activate`:

```rust
use aria_models::{EffectInstance, Preset};
use aria_plugin::{Plugin, PluginContext, PluginError};

impl Plugin for MyPlugin {
    fn activate(&mut self, ctx: &PluginContext) -> Result<(), PluginError> {
        let preset = Preset::new(
            "Soft Vignette",
            vec![EffectInstance::new("vignette")],
            Vec::new(),
            Vec::new(),
        );
        self.handles.push(ctx.register_preset(preset)?);
        Ok(())
    }
}
```

Native `aria-sdk` plugins register presets with `Host::register_preset(&spec)`
or `Host::register_preset_json(json)`. WebAssembly guests pass the preset JSON
string to `register-preset(preset-json)`.

`PluginContext::register_preset` trims the name, validates the preset, and
fails with `PluginError::PresetRefused` when the preset is invalid or the same
plugin already registered a preset of that name, compared without regard to
ASCII case. Dropping the returned `Registration` removes the preset.

`PluginContext::presets()` lists the registered presets in registration order
with the name of the plugin that registered each. `presets_revision()`
increases on every registration and removal.
