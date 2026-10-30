# Packages

A plugin package is a directory or `.ariaplugin` archive with `plugin.toml` at
its root, optional WGSL shaders, SVG icons, `.ariapreset` files, and theme
files, and at most one code artifact.

```text
com.example.color-tools/
  plugin.toml
  plugin.wasm                  # WASM tier, or
  lib/
    linux-x86_64.so            # Native tier (per target OS and CPU), or neither for Data tier
  shaders/
    color-tools.wgsl
  presets/
    warm-fade.ariapreset
  themes/
    dusk.toml
  icons/
    color-tools.svg
```

## Layout and tiers

`Package::open` inspects the package root to select one of three execution
tiers:

| Tier | Files in package root | Capabilities |
| --- | --- | --- |
| `Tier::Data` | Neither `plugin.wasm` nor `lib/` | Declared WGSL effects, generators, and transitions, plugin textures, clip types, workspaces, menu rows, key chords, settings, `.ariapreset` files, and themes. Cannot declare `[[commands]]`. Mounts at startup without running guest code. |
| `Tier::Wasm` | `plugin.wasm` | All declared contributions plus sandboxed guest code targeting `aria:plugin/plugin@1.0.0`: commands, panels, viewer overlays, scopes, jobs, AI tools, events, project edits, importers, exporters, encoders, decoders, audio effects, WGSL effect graphs and CPU effects, generators, transitions, plugin textures, clip types, driver languages, presets, themes, workspaces, settings, and storage. See [WebAssembly plugins](wasm.md). |
| `Tier::Native` | `lib/<os>-<arch>.<so\|dylib\|dll>` | All declared contributions plus in-process C ABI code (`aria_plugin_entry`): commands, panels, viewer overlays, scopes, jobs, AI tools, events, project edits, importers, exporters, encoders, decoders, audio effects, WGSL effect graphs and CPU effects, generators, transitions, plugin textures, clip types, driver languages, presets, themes, workspaces, settings, and storage. See [Native plugins](native.md). |

A package that contains both `plugin.wasm` and `lib/` fails to open. A package
with neither is `Tier::Data`; if its manifest declares `[[commands]]`, opening
it fails with instructions to add `plugin.wasm` or `lib/<os>-<arch>.<ext>`.

File limits enforced when opening or mounting a package:

- `plugin.toml`: UTF-8 TOML at the package root, at most `MAX_MANIFEST_BYTES`
  (1 MiB).
- Shader files (`shaders/*.wgsl`): relative paths without `..` segments,
  resolving inside the package directory after canonicalization, each at most
  `MAX_SHADER_BYTES` (1 MiB).
- Icons (`icons/*.svg`): relative SVG paths without `..` segments for
  `[[commands]]` and `[[clip_types]]`, each at most `ICON_MAX_BYTES` (64 KiB),
  parsed by `CommandIcon::from_svg`.
- Textures (`textures/*.png`): relative image paths without `..` segments,
  resolving inside the package directory after canonicalization, each at most
  `MAX_TEXTURE_BYTES` (64 MiB) and at most `MAX_TEXTURE_SIDE` (8192) pixels on
  a side. Each decodes to straight-alpha RGBA and registers through
  `PluginContext::register_texture` before the package's effects.
- Presets (`presets/*.ariapreset`): optional `presets/` directory of UTF-8 JSON
  files validated by `Preset::parse`, each at most `PRESET_MAX_BYTES` (1 MiB),
  sorted by path. Preset names must be unique within the package without regard
  to ASCII case.
- Themes (`themes/*.toml`): optional `themes/` directory of UTF-8 theme files
  (see [Theme files](../theme-tokens.md#theme-files)), each at most
  `THEME_MAX_BYTES` (64 KiB). The file name without `.toml` is the theme id:
  ASCII letters, digits, `-`, `_`, and `.`, not starting with `.`, at most
  `THEME_ID_MAX_BYTES` (64) bytes. Each registers through
  `PluginContext::register_theme(id, text)`. Plugins also register themes from
  code with `PluginContext::register_theme`, `aria_sdk::Host::register_theme`,
  or the WebAssembly `register-theme` import. The host reads the tokens when it
  lists themes; a theme that does not read is listed in Settings with the reason.

Each package also gets a persistent data directory outside the package folder
at `<data dir>/plugin-data/<id>` (`Package::data_dir`). Uninstalling or
updating the package leaves `<data dir>/plugin-data/<id>` intact.

## `plugin.toml` reference

Every table in `plugin.toml` rejects unknown fields (`deny_unknown_fields`).

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `plugin` | table | required | Package identity, target API version, metadata, and plugin dependencies. |
| `permissions` | table | no access | Host capability grants for project state, media, GPU, network, file system, subprocesses, and clipboard. |
| `activation` | table | `{ on = [] }` | Deferred-loading event list. Empty loads at startup. |
| `textures` | array of tables | `[]` | Static RGBA images registered before the package's effects, generators, and clip types. |
| `effects` | array of tables | `[]` | Video effects rendered by package WGSL files. |
| `generators` | array of tables | `[]` | Generators rendered by package WGSL files. |
| `transitions` | array of tables | `[]` | Clip transitions rendered by package WGSL files. |
| `commands` | array of tables | `[]` | Command palette entries implemented by WASM or native guest code. |
| `menus` | array of tables | `[]` | Additional menu bar placements for declared commands. |
| `keys` | array of tables | `[]` | Additional default keyboard shortcuts for declared commands. |
| `panels` | array of tables | `[]` | Docked UI panels implemented by WASM or native guest code. |
| `settings` | array of tables | `[]` | Persistent configuration fields shown in the package's Settings section. |
| `workspaces` | array of tables | `[]` | Named editor pane layouts listed in the Window menu and Settings. |
| `clip_types` | array of tables | `[]` | Custom timeline clip kinds drawn by a package generator or by their own WGSL. |

```toml
[plugin]
id = "com.example.color-tools"
name = "Color Tools"
version = "0.1.0"
api = 1
publisher = "Example Studio"
description = "Film grain and warm grade tools"
license = "MIT OR Apache-2.0"
requires = ["com.example.shared-luts >=1.0.0, <2.0.0"]

[permissions]
project = "read-write"
media = "read"
gpu = false
net = ["api.example.com", "*.cdn.example.com"]
fs = ["project", "pick"]
process = ["ffmpeg"]
clipboard = true

[activation]
on = [
  "command:com.example.color-tools.run",
  "panel:com.example.color-tools.inspector",
  "effect:com.example.color-tools.warm-grade",
  "setting:strength",
]

[[textures]]
id = "grain"
file = "textures/grain.png"

[[effects]]
id = "warm-grade"
name = "Warm Grade"
category = "Color"
shader = "shaders/warm-grade.wgsl"
params = [
  { name = "gain", label = "Gain", default = 1.0, min = 0.0, max = 2.0 },
]

[[effects]]
id = "film-grain"
name = "Film Grain"
category = "Stylize"
time_dependent = true

[[effects.passes]]
name = "half"
file = "shaders/grain-half.wgsl"
stage = { compute = { workgroup = [8, 8] } }
output = { scale = [0.5, 0.5] }

[[effects.passes]]
file = "shaders/grain-mix.wgsl"
inputs = ["source", { pass = "half" }, { texture = "grain" }]

[[generators]]
id = "test-card"
name = "Test Card"
shader = "shaders/test-card.wgsl"

[[transitions]]
id = "flash-cut"
name = "Flash Cut"
shader = "shaders/flash-cut.wgsl"

[[commands]]
id = "com.example.color-tools.run"
title = "Run Color Tools"
key = "Ctrl+Alt+L"
menu = "Sequence/Color Tools"
icon = "icons/color-tools.svg"
enablement = "selection && !playing"

[[menus]]
command = "com.example.color-tools.run"
menu = "Clip"

[[keys]]
command = "com.example.color-tools.run"
key = "Ctrl+Shift+L"
route = "home"

[[panels]]
id = "com.example.color-tools.inspector"
title = "Color Tools"
location = "sidebar"

[[settings]]
key = "strength"
label = "Default Strength"
type = "number"
default = 1.0
min = 0.0
max = 2.0
step = 0.1

[[workspaces]]
id = "grading"
name = "Grading Room"
sidebar_collapsed = true
inspector_width = 520
timeline_height = 230
scopes = true
focus = "preview"

[[clip_types]]
id = "slate"
name = "Slate"
generator = "test-card"
colour = "teal"
```

### `[plugin]`

Required identity and compatibility table.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | string | required | Reverse-domain identifier with at least two dot-separated labels (`com.example.tools`). Each label contains only lowercase ASCII letters (`a-z`), digits (`0-9`), and `-`, and cannot start or end with `-`. The `aria.` prefix is reserved for shipped plugins. |
| `name` | string | required | Display name. Must be non-empty after trimming whitespace. |
| `version` | string | required | Full `major.minor.patch` semantic version parsed by `semver::Version`. |
| `api` | integer or string | required | Plugin host API major version (`1` or `"1"`). Must equal `API_MAJOR` (`1`). |
| `publisher` | string | `""` | Publisher display name. |
| `description` | string | `""` | Short package summary. |
| `license` | string | `""` | SPDX license expression. |
| `requires` | array of strings | `[]` | Required plugin dependencies. Each entry is `"<plugin-id>"` (any version) or `"<plugin-id> <version-req>"` where `<version-req>` parses as `semver::VersionReq`. Target IDs must contain a dot, use only `a-z`, `0-9`, `-`, and `.`, and cannot equal `plugin.id`. |

```toml
[plugin]
id = "com.example.minimal"
name = "Minimal Plugin"
version = "1.0.0"
api = 1
```

### `[permissions]`

Optional capability grants. `Package::plugin_manifest` maps these grants to
host service names in `PluginManifest::uses`. `SettingsService` and
`SettingsSchema` are included in `uses` without a permission entry.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `"none"`, `"read"`, or `"read-write"` | `"none"` | Project access level. `"read"` and `"read-write"` add `ProjectHost` (`"read"` permits project snapshots; `"read-write"` also permits transactions and undo). |
| `media` | `"none"`, `"read"`, or `"read-write"` | `"none"` | Media access level. `"read"` and `"read-write"` add `DecoderService` and `AudioHost`. |
| `gpu` | boolean | `false` | GPU compute and render access. `true` adds `GpuContext` and `RenderHost`. |
| `net` | array of strings | `[]` | Allowed network host names (`"api.example.com"`, `"*.cdn.example.com"`). Each entry must be non-empty and contain only ASCII alphanumeric characters, `-`, `.`, or `*`. |
| `fs` | array of strings | `[]` | File-system scopes (`FsScope`) beyond the package directory and `<data dir>/plugin-data/<id>`. Each entry is `"project"` (the open project folder), `"pick"` (paths chosen in a file dialog), or an absolute path. |
| `process` | `false` or array of strings | `false` | Executables the plugin may spawn. `process = true` and empty executable strings are rejected. |
| `clipboard` | boolean | `false` | System clipboard read access. |

```toml
[plugin]
id = "com.example.exporter"
name = "Exporter"
version = "1.0.0"
api = 1

[permissions]
project = "read"
media = "read"
fs = ["pick"]
process = ["ffmpeg"]
```

### `[activation]`

Optional deferred-loading triggers. When `[activation]` is omitted, `on` is
empty, or `on` contains `"startup"`, `PackageManifest::loads_at_start` returns
`true` and the package mounts at startup. See [Activation](activation.md).

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `on` | array of strings | `[]` | Activation events (`Activation` / `ActivationKind::ALL`): `"startup"`, `"command:<id>"`, `"panel:<id>"`, `"setting:<key>"`, `"file:<ext>"`, `"effect:<id>"`, `"transition:<id>"`, or `"generator:<id>"`. Duplicate events are deduplicated in declaration order. |

Every `"command:<id>"`, `"panel:<id>"`, and `"setting:<key>"` entry must match
an entry in `[[commands]]`, `[[panels]]`, or `[[settings]]` of the same
`plugin.toml` (`setting:<key>` is qualified under `<plugin.id>.<key>` unless
already prefixed). `"file:<ext>"` strips an optional leading `.` and normalizes
`<ext>` to lowercase ASCII letters, digits, `_`, and `-`.

```toml
[plugin]
id = "com.example.lazy"
name = "Lazy Loader"
version = "1.0.0"
api = 1

[activation]
on = ["file:braw", "setting:quality"]

[[settings]]
key = "quality"
label = "Decode Quality"
```

### `[[textures]]`

Optional static images registered through `PluginContext::register_texture`
before the package's effects, generators, and clip types. See
[Plugin textures](effects.md#plugin-textures).

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | string | required | Texture identifier containing only ASCII letters, digits, `-`, `_`, and `.`. Unprefixed IDs are qualified as `<plugin.id>.<id>`. Must be unique within `[[textures]]`. |
| `file` | string | required | Image path relative to the package root without `..` segments, at most `MAX_TEXTURE_BYTES` (64 MiB) and `MAX_TEXTURE_SIDE` (8192) pixels per side. |

```toml
[plugin]
id = "com.example.textures"
name = "Texture Pack"
version = "1.0.0"
api = 1

[[textures]]
id = "hald-lut"
file = "textures/hald.png"
```

### `[[effects]]` and `[[generators]]`

Optional declared GPU effects and generators registered into the compositor
when the package mounts. Both array-of-tables sections share the same schema;
entries in `[[generators]]` register with `EffectSpec::generator = true`.
Every `[[effects]]`, `[[generators]]`, `[[transitions]]`, and self-drawn
`[[clip_types]]` entry in a package shares one catalog ID namespace and must be
unique. See [Effects and transitions](effects.md).

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | string | required | Catalog identifier containing only ASCII letters, digits, `-`, `_`, and `.`. Unprefixed IDs are qualified as `<plugin.id>.<id>`. |
| `name` | string | required | Display label. Must be non-empty after trimming whitespace. |
| `category` | string | none (`"Other"` for effects) | Effect menu submenu label. Ignored for generators, which are listed in the Generators tab rather than the Effect menu. |
| `shader` | string | none | Single-pass fragment shader WGSL path relative to the package root without `..` segments. Mutually exclusive with `passes`; one of `shader` or `passes` is required. |
| `passes` | array of strings or pass tables | `[]` | Multi-pass render graph entries (`[[effects.passes]]` / `[[generators.passes]]`). Mutually exclusive with `shader`; between 1 and `MAX_EFFECT_PASSES` (8) passes when used. |
| `params` | array of `EffectParamSpec` tables | `[]` | Parameter declarations in inspector display order. |
| `groups` | array of `EffectGroupSpec` tables | `[]` | Inspector parameter group headings (`{ id, label, collapsed }`). |
| `stateful` | boolean | `false` | Whether the output at tick `t` depends on frames rendered before `t`. |
| `bounds` | `EffectBounds` table | `{ left = 0.0, top = 0.0, right = 0.0, bottom = 0.0 }` | Pixel margin expansion (`> 0`) or contraction (`< 0`) applied to the layer bounding box. |
| `reads_outside` | boolean | `false` | Whether any pass samples source coordinates outside `0.0..=1.0`. |
| `time_dependent` | boolean | `false` | Whether the output varies with playback time (`aria.time`) when parameters are static. When `false`, `aria.time` is `0.0`. |

```toml
[plugin]
id = "com.example.fx"
name = "FX Pack"
version = "1.0.0"
api = 1

[[effects]]
id = "warm-tint"
name = "Warm Tint"
category = "Color"
shader = "shaders/warm-tint.wgsl"
bounds = { left = 4.0, top = 4.0, right = 4.0, bottom = 4.0 }
reads_outside = false
time_dependent = false

[[generators]]
id = "checkerboard"
name = "Checkerboard"
shader = "shaders/checkerboard.wgsl"
time_dependent = true
```

### `[[effects.passes]]`, `[[generators.passes]]`, and parameter tables

Each entry in `passes` is either a relative WGSL path string (equivalent to a
fragment pass with default `name`, `inputs`, and `output`) or a pass table:

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `file` | string | required | Relative WGSL path inside the package without `..` segments, at most `MAX_SHADER_BYTES` (1 MiB). |
| `name` | string | `""` (`pass<index>`) | Pass identifier (`a-z`, `A-Z`, `0-9`, `_`) referenced by later passes as `{ pass = "<name>" }` and bound in WGSL as `pass_<name>`. |
| `stage` | `"fragment"` or `{ compute = { workgroup = [x, y] } }` | `"fragment"` | Pipeline stage (`PassStage`). Compute workgroup dimensions must be non-zero with `x * y <= MAX_WORKGROUP_INVOCATIONS` (256). |
| `inputs` | array of `PassInput` entries | `[]` (previous pass, or `"source"` for pass 0) | Bound inputs in order, at most `MAX_PASS_INPUTS` (8): `"source"`, `{ pass = "<name>" }`, `{ layer = "<param>" }`, or `{ texture = "<id>" }`. Texture IDs are qualified under `<plugin.id>.` and must match a `[[textures]]` entry in the same manifest. |
| `output` | `"same"`, `{ scale = [x, y] }`, or `{ fixed = [w, h] }` | `"same"` | Output target dimensions (`OutputSize`). Scale factors must be finite and in `0.0..=16.0` (`MAX_PASS_SCALE`). Fixed dimensions must be in `1..=8192` (`MAX_PASS_SIDE`). The final pass must use `"same"`. |

`bounds` (`EffectBounds`) inline table keys:

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `left` | float (`f32`) | `0.0` | Left margin offset in canvas pixels. |
| `top` | float (`f32`) | `0.0` | Top margin offset in canvas pixels. |
| `right` | float (`f32`) | `0.0` | Right margin offset in canvas pixels. |
| `bottom` | float (`f32`) | `0.0` | Bottom margin offset in canvas pixels. |

`groups` (`EffectGroupSpec`) table keys:

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | string | required | Group identifier referenced by `params[].group`. |
| `label` | string | required | Inspector section heading. |
| `collapsed` | boolean | `false` | Whether the group starts collapsed in the inspector. |

`params` (`EffectParamSpec`) table keys (used in `[[effects]]`,
`[[generators]]`, `[[transitions]]`, and self-drawn `[[clip_types]]`):

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `name` | string | required | WGSL parameter identifier (`a-z`, `A-Z`, `0-9`, `_`, not starting with a digit), unique within the effect or transition. |
| `label` | string | required | Non-empty inspector display label. |
| `kind` | string (`EffectParamKind`) | `"scalar"` | Parameter type: `"scalar"` (alias `"float"`), `"int"` (alias `"integer"`), `"bool"` (alias `"boolean"`), `"choice"` (alias `"enum"`), `"color"` (alias `"colour"`), `"vec2"` (aliases `"point2d"`, `"point_2d"`), `"point3"` (aliases `"vec3"`, `"point3d"`, `"point_3d"`), `"size"`, `"angle"`, `"curve"`, `"gradient"`, `"text"` (alias `"string"`), `"font"`, `"file"` (alias `"path"`), or `"layer"` (alias `"layer_ref"`). |
| `default` | float (`f32`) | `0.0` | Default value for `"scalar"` and `"angle"`. Must satisfy `min <= default <= max`. |
| `min` | float (`f32`) | `0.0` | Hard minimum for `"scalar"`, `"vec2"`, `"point3"`, `"size"`, and `"angle"` (`min <= max`). |
| `max` | float (`f32`) | `1.0` | Hard maximum for `"scalar"`, `"vec2"`, `"point3"`, `"size"`, and `"angle"` (`min <= max`). |
| `soft_min` | float (`f32`) | none | Optional slider scrub minimum (`min <= soft_min <= soft_max <= max`). |
| `soft_max` | float (`f32`) | none | Optional slider scrub maximum (`min <= soft_min <= soft_max <= max`). |
| `unit` | string | none | Unit suffix shown in the inspector (`"px"`, `"deg"`, `"%"`, `"s"`). |
| `display` | string (`EffectParamDisplay`) | none (`"slider"`) | How the inspector prints the value: `"slider"` prints it with its unit, `"angle_dial"` (aliases `"angle"`, `"dial"`) in degrees to a tenth, and `"percentage"` (alias `"percent"`) prints the stored fraction as a whole percent. Every number row draws a value track between its bounds (`min` and `max`, or `int_min` and `int_max`) when they differ. |
| `default_int` | integer (`i64`) | `0` | Default value for `"int"`. |
| `int_min` | integer (`i64`) | `0` | Minimum allowed value for `"int"` (`int_min <= int_max`). |
| `int_max` | integer (`i64`) | `0` | Maximum allowed value for `"int"` (`int_min <= int_max`). |
| `default_bool` | boolean | `false` | Default value for `"bool"`. |
| `choices` | array of `{ id, label }` tables | `[]` | Options (`EffectChoiceOption`) for `"choice"`. Must be non-empty when `kind = "choice"`. |
| `default_choice` | string | none (first option) | Default option `id` for `"choice"`. An `id` no option has selects the first option. |
| `default_color` | `[u8; 4]` | `[0, 0, 0, 0]` | Default sRGB `[R, G, B, A]` color in `0..=255` for `"color"`. |
| `default_linear_color` | `[f32; 4]` | none | Optional linear RGBA default `[r, g, b, a]` for `"color"` (alias `linear_color`). Overrides `default_color` when set. |
| `linear_color` | `[f32; 4]` | none | Alias for `default_linear_color`. |
| `default_vec2` | `[f32; 2]` | `[0.0, 0.0]` | Default `[x, y]` or `[w, h]` value for `"vec2"` and `"size"`. |
| `default_vec3` | `[f32; 3]` | `[0.0, 0.0, 0.0]` | Default `[x, y, z]` value for `"point3"`. |
| `default_curve` | array of `{ x, y }` tables | `[]` | Default control points (`EffectCurvePoint`, coordinates in `0.0..=1.0`) for `"curve"`. Empty defaults to the identity line `(0, 0)` to `(1, 1)`. |
| `default_gradient` | array of `{ offset, color }` tables | `[]` | Default stops (`EffectGradientStop`, `offset` or alias `position` in `0.0..=1.0`, linear RGBA `color: [f32; 4]`) for `"gradient"`. |
| `default_text` | string | `""` | Default string for `"text"`, `"font"`, or `"file"`. |
| `group` | string | none | Group `id` matching an entry in `groups`. |
| `visible_when` | `EffectVisibilityCondition` table | none | Visibility rule hiding this parameter unless another parameter on the same effect or transition equals a target value. |
| `tooltip` | string | none | Inspector hover description. |

`choices` (`EffectChoiceOption`) table keys:

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | string | required | Option identifier stored on the effect instance. |
| `label` | string | required | Option label shown in the dropdown. |

`default_curve` (`EffectCurvePoint`) table keys:

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `x` | float (`f32`) | required | Input coordinate in `0.0..=1.0`. |
| `y` | float (`f32`) | required | Output value in `0.0..=1.0`. |

`default_gradient` (`EffectGradientStop`) table keys:

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `offset` | float (`f32`) | required | Stop position in `0.0..=1.0` (alias `position`). |
| `position` | float (`f32`) | none | Alias for `offset`. |
| `color` | `[f32; 4]` | required | Linear RGBA color `[r, g, b, a]` at this stop. |

`visible_when` (`EffectVisibilityCondition`) table keys:

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `param` | string | required | Controlling parameter identifier on the same effect or transition. |
| `equals` | string | `""` | Stringified value that reveals this parameter. |
| `equals_bool` | boolean | none | Boolean alias for `equals` (`true` -> `"true"`, `false` -> `"false"`). |
| `equals_int` | integer (`i64`) | none | Integer alias for `equals`. |
| `equals_choice` | string | none | Choice ID alias for `equals`. |

```toml
[plugin]
id = "com.example.bloom"
name = "Bloom"
version = "1.0.0"
api = 1

[[textures]]
id = "dirt"
file = "textures/dirt.png"

[[effects]]
id = "bloom"
name = "Bloom"
category = "Stylize"
bounds = { left = 8.0, top = 8.0, right = 8.0, bottom = 8.0 }
groups = [{ id = "optics", label = "Optics", collapsed = false }]
params = [
  { name = "amount", label = "Amount", kind = "scalar", default = 0.5, min = 0.0, max = 2.0, soft_min = 0.0, soft_max = 1.0, unit = "%", display = "percentage", group = "optics", tooltip = "Bloom intensity" },
]

[[effects.passes]]
name = "downsample"
file = "shaders/downsample.wgsl"
stage = { compute = { workgroup = [8, 8] } }
output = { scale = [0.5, 0.5] }

[[effects.passes]]
file = "shaders/composite.wgsl"
inputs = ["source", { pass = "downsample" }, { texture = "dirt" }]
output = "same"
```

A native plugin builds the same tables with `aria-sdk`. Each
`EffectParamSpec` constructor takes `name` and `label`, then the kind's
default and range: `scalar(name, label, default, min, max)`,
`int(name, label, default_int, min, max)`, `boolean(name, label, default_bool)`,
`choice(name, label, choices, default_choice)`,
`vec2(name, label, default_vec2, min, max)`,
`point3(name, label, default_vec3, min, max)`,
`size(name, label, default_vec2, min, max)`,
`angle(name, label, default, min, max)`, `color(name, label, default_color)`,
`curve(name, label, default_curve)`, `gradient(name, label, default_gradient)`,
`text`, `font`, and `file` with `default_text`, and `layer(name, label)`.
`with_display`, `with_unit`, `with_tooltip`, `with_group`, and
`with_visible_when` set the key of the same name, `with_soft_range(soft_min,
soft_max)` sets both soft bounds, and `flattened_floats()` is the number of
floats the parameter occupies in shader storage. `EffectGroupSpec::new(id,
label)` builds an expanded group and `with_collapsed(collapsed)` sets
`collapsed`. `EffectVisibilityCondition::when_bool(param, value)`,
`when_int(param, value)`, and `when_choice(param, choice)` show the row while
`param` equals the value, setting `equals_bool`, `equals_int`, or
`equals_choice`. `EffectChoiceOption::new(id, label)`,
`EffectCurvePoint::new(x, y)`, and `EffectGradientStop::new(position, color)`
build the entries of `choices`, `default_curve`, and `default_gradient`.

### `[[transitions]]`

Optional single-pass GPU transitions between adjacent timeline clips. See
[Effects and transitions](effects.md).

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | string | required | Catalog identifier containing only ASCII letters, digits, `-`, `_`, and `.`. Unprefixed IDs are qualified as `<plugin.id>.<id>`. |
| `name` | string | required | Non-empty display label. |
| `shader` | string | required | Relative WGSL path inside the package without `..` segments, at most `MAX_SHADER_BYTES` (1 MiB). |
| `params` | array of `EffectParamSpec` tables | `[]` | Transition parameters exposed in the inspector and as `param_<name>()` in WGSL. |

```toml
[plugin]
id = "com.example.transitions"
name = "Transitions"
version = "1.0.0"
api = 1

[[transitions]]
id = "flash-cut"
name = "Flash Cut"
shader = "shaders/flash-cut.wgsl"
params = [
  { name = "softness", label = "Softness", default = 0.25, min = 0.0, max = 1.0 },
]
```

### `[[commands]]`

Optional command palette entries implemented by the package's WASM or native
code. Rejected in `Tier::Data` packages. See [Commands](commands.md).

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | string | required | Non-empty command identifier, unique within `[[commands]]`. |
| `title` | string | required | Non-empty command palette and menu label. |
| `key` | string | none | Default shortcut chord on the `"editor"` route, validated by `Chord::parse` (for example `"Ctrl+Alt+L"`). |
| `menu` | string | none | Menu bar placement validated by `MenuPath::parse` (for example `"Sequence"`, `"Sequence/Color Tools"`, `"Clip"`). |
| `icon` | string | none | Relative `.svg` file path inside the package without `..` segments, at most `ICON_MAX_BYTES` (64 KiB). |
| `enablement` | string | none | Boolean enablement expression over editor context keys (`clips`, `selection`, `single-selection`, `mark-in`, `mark-out`, `in-out`, `playing`, `focus.media`, `focus.viewer`, `focus.timeline`, `focus.inspector`, `focus.assistant`, `focus.mixer`), validated by `Enablement::parse` (for example `"selection && !playing"`). |

```toml
[plugin]
id = "com.example.commands"
name = "Commands"
version = "1.0.0"
api = 1

[[commands]]
id = "com.example.commands.normalize"
title = "Normalize Clip"
key = "Ctrl+Alt+N"
menu = "Clip"
icon = "icons/normalize.svg"
enablement = "selection && !playing"
```

### `[[menus]]`

Optional additional menu bar placements for commands declared in `[[commands]]`.
See [Menus](menus.md).

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `command` | string | required | Target command ID. Must match an `id` declared in `[[commands]]`. |
| `menu` | string | required | Menu path validated by `MenuPath::parse` (top-level menu `File`, `Edit`, `Clip`, `Sequence`, `Markers`, `View`, `Window`, or `Help`, with optional `/Submenu` up to `MAX_MENU_DEPTH` = 3). Duplicate `(command, menu)` pairs across `[[commands]]` and `[[menus]]` are rejected. |

```toml
[plugin]
id = "com.example.menus"
name = "Menus"
version = "1.0.0"
api = 1

[[commands]]
id = "com.example.menus.grade"
title = "Apply Grade"

[[menus]]
command = "com.example.menus.grade"
menu = "Sequence/Color Tools"
```

### `[[keys]]`

Optional additional default keyboard shortcuts for commands declared in
`[[commands]]`. See [Keys](keys.md).

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `command` | string | required | Target command ID. Must match an `id` declared in `[[commands]]`. |
| `key` | string | required | Key chord validated by `Chord::parse` (`Ctrl`, `Alt`/`Option`, `Shift`, `Super`/`Cmd` modifiers plus a bindable key). |
| `route` | `"editor"`, `"timeline"`, `"viewer"`, or `"home"` | `"editor"` | Key dispatch scope (`KeyRoute::parse`). Each command may bind at most one default chord per route across `[[commands]]` and `[[keys]]`. |

```toml
[plugin]
id = "com.example.keys"
name = "Keys"
version = "1.0.0"
api = 1

[[commands]]
id = "com.example.keys.mark"
title = "Mark Beat"

[[keys]]
command = "com.example.keys.mark"
key = "Ctrl+Shift+M"
route = "timeline"
```

### `[[panels]]`

Optional dockable panels rendered by the package's WASM or native code. See
[Panels](panels.md).

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | string | required | Non-empty panel identifier, unique within `[[panels]]`. |
| `title` | string | required | Non-empty tab title. |
| `location` | `"sidebar"`, `"inspector"`, or `"timeline"` | required | Initial dock region (`PanelLocation`). |

```toml
[plugin]
id = "com.example.panels"
name = "Panels"
version = "1.0.0"
api = 1

[[panels]]
id = "com.example.panels.notes"
title = "Production Notes"
location = "inspector"
```

### `[[settings]]`

Optional configuration fields registered under the package's section in
Settings. See [Settings](settings.md).

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `key` | string | required | Setting identifier containing only ASCII letters, digits, `-`, `_`, and `.`, qualified under `<plugin.id>.<key>` unless already prefixed, and unique within `[[settings]]`. |
| `label` | string | required | Non-empty display label in Settings. |
| `type` | `"text"`, `"secret"`, `"toggle"`, `"number"`, or `"choice"` | `"text"` | Control and storage kind (`SettingKind`). |
| `placeholder` | string | `""` | Placeholder hint shown for empty `"text"` and `"secret"` inputs. |
| `default` | string, boolean, integer, or float | none | Initial value validated against `type` (`"true"` / `"false"` for `"toggle"`, finite number in `min..=max` for `"number"`, member of `options` for `"choice"`). |
| `min` | float (`f64`) | `f64::MIN` | Minimum value for `type = "number"` (`min <= max`, finite). Rejected on other types. |
| `max` | float (`f64`) | `f64::MAX` | Maximum value for `type = "number"` (`min <= max`, finite). Rejected on other types. |
| `step` | float (`f64`) | `1.0` | Scrub and spinbox increment for `type = "number"` (`step > 0.0`, finite). Rejected on other types. |
| `options` | array of strings | `[]` | Allowed choices for `type = "choice"`. Must be non-empty with non-blank unique entries when `type = "choice"`; rejected on other types. |

```toml
[plugin]
id = "com.example.settings"
name = "Settings Demo"
version = "1.0.0"
api = 1

[[settings]]
key = "mode"
label = "Render Mode"
type = "choice"
options = ["draft", "full"]
default = "full"
```

### `[[workspaces]]`

Optional named editor pane arrangements offered in the Window menu and
**Settings > Workspaces**. See [Workspaces](workspaces.md).

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | string | required | Workspace identifier: 1 to `WORKSPACE_ID_MAX_BYTES` (64) bytes of ASCII letters, digits, `-`, `_`, and `.`, not starting with `.`, unique within `[[workspaces]]`. |
| `name` | string | required | Display name: 1 to `WORKSPACE_NAME_MAX_BYTES` (64) bytes after trimming, without line breaks (`\n` or `\r`). |
| `sidebar_width` | float (`f32`) | none (Editing width) | Sidebar width in pixels, finite and in `(0.0, 16384.0]` (`WORKSPACE_MAX_SIZE`). |
| `sidebar_collapsed` | boolean | none (`false`) | Whether the left sidebar starts collapsed. |
| `inspector_width` | float (`f32`) | none (Editing width) | Inspector width in pixels, finite and in `(0.0, 16384.0]` (`WORKSPACE_MAX_SIZE`). |
| `inspector_collapsed` | boolean | none (`false`) | Whether the right inspector starts collapsed. |
| `timeline_height` | float (`f32`) | none (Editing height) | Timeline height in pixels, finite and in `(0.0, 16384.0]` (`WORKSPACE_MAX_SIZE`). |
| `scopes` | boolean | none (`false`) | Whether the video scopes strip is open. |
| `focus` | `"media"`, `"preview"`, `"timeline"`, `"inspector"`, `"assistant"`, or `"mixer"` | none (`"timeline"`) | Pane receiving keyboard focus (`WorkspaceFocus`). |
| `tree` | string | none | Optional serialized `DockTree` JSON string overriding the default dock split tree. |

```toml
[plugin]
id = "com.example.workspaces"
name = "Workspaces"
version = "1.0.0"
api = 1

[[workspaces]]
id = "grading"
name = "Grading Room"
sidebar_collapsed = true
inspector_width = 520.0
timeline_height = 230.0
scopes = true
focus = "preview"
```

### `[[clip_types]]`

Optional custom clip kinds offered in the Generators tab and command palette.
Each entry either references an existing generator via `generator` or defines
its own renderer with `shader` or `passes`. See [Clip types](clip-types.md).

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | string | required | Clip type identifier: 1 to `CLIP_TYPE_ID_MAX_BYTES` (128) bytes of ASCII letters, digits, `-`, `_`, and `.`, qualified under `<plugin.id>.<id>` unless already prefixed, unique within `[[clip_types]]`. |
| `name` | string | required | Display label: 1 to `CLIP_TYPE_LABEL_MAX_BYTES` (64) bytes after trimming, without line breaks. |
| `generator` | string | none | Target generator ID (qualified under `<plugin.id>.` unless already prefixed), at most `CLIP_TYPE_ID_MAX_BYTES` (128) bytes. Mutually exclusive with `shader`, `passes`, `params`, `groups`, and `time_dependent`. |
| `colour` | `"teal"`, `"blue"`, `"purple"`, `"pink"`, `"red"`, `"orange"`, `"yellow"`, or `"green"` | required | Timeline clip bar and badge color (`MarkerColor`). |
| `icon` | string | none | Relative `.svg` file path inside the package without `..` segments, at most `ICON_MAX_BYTES` (64 KiB), with at most `CLIP_TYPE_ICON_MAX_PATHS` (32) paths and `CLIP_TYPE_ICON_MAX_VERBS` (1024) path verbs. |
| `handles` | array of strings | `[]` | On-canvas viewer handle parameter names: up to `CLIP_TYPE_MAX_HANDLES` (8) unique non-empty names, each at most `CLIP_TYPE_HANDLE_MAX_BYTES` (64) bytes of ASCII letters, digits, and `_` (not starting with a digit). For a self-drawn clip type, each handle must match a `kind = "vec2"` entry in `params`. |
| `shader` | string | none | Single-pass WGSL generator path relative to the package root without `..` segments. Registers a generator under the clip type's `id`. Mutually exclusive with `generator` and `passes`. |
| `passes` | array of strings or pass tables | `[]` | Multi-pass WGSL generator graph (`[[clip_types.passes]]`) registered under the clip type's `id`. Mutually exclusive with `generator` and `shader`. |
| `params` | array of `EffectParamSpec` tables | `[]` | Generator parameters for a self-drawn clip type (`shader` or `passes`). Rejected when `generator` is set. |
| `groups` | array of `EffectGroupSpec` tables | `[]` | Inspector parameter groups for a self-drawn clip type (`shader` or `passes`). Rejected when `generator` is set. |
| `time_dependent` | boolean | `false` | Whether a self-drawn clip type reads `aria.time`. Rejected when `generator` is set. |

```toml
[plugin]
id = "com.example.cliptypes"
name = "Clip Types"
version = "1.0.0"
api = 1

[[clip_types]]
id = "callout"
name = "Callout Box"
colour = "orange"
shader = "shaders/callout.wgsl"
handles = ["origin", "target"]
time_dependent = true
params = [
  { name = "origin", label = "Origin", kind = "vec2", default_vec2 = [0.25, 0.75], min = 0.0, max = 1.0 },
  { name = "target", label = "Target", kind = "vec2", default_vec2 = [0.5, 0.5], min = 0.0, max = 1.0 },
]
```

## Installing and managing packages

Packages are stored at `<data dir>/plugins/<id>/`.

1. Install an `.ariaplugin` archive from **File > Install Plugin...**, from
   **Settings > Plugins > Install from File...**, or by dragging and dropping the
   `.ariaplugin` file onto the editor window.
2. `stage` unpacks the archive (at most `MAX_ARCHIVE_ENTRIES` = 1,024 entries
   and `MAX_UNPACKED_BYTES` = 64 MiB uncompressed) into a sibling
   `.staging-<pid>-<n>` directory and runs `Package::open` before modifying an
   existing installation.
3. If a new package requests any permission in `[permissions]` or is
   `Tier::Native`, the editor opens a confirmation dialog listing the requested
   capabilities and, for native packages, an unsandboxed-code warning. Data and
   WASM packages with empty `[permissions]` install without a prompt.
4. When installing a newer version of an already-installed package, the editor
   compares the existing `Permissions` against the incoming manifest with
   `Permissions::covers`. If the update requests wider permissions or switches
   from data/WASM to `Tier::Native`, the confirmation dialog prompts again for
   the added capabilities; otherwise the staged directory replaces the existing
   package and reloads in place.

In **Settings > Plugins**, each installed package row provides:

- **The switch** at the right end of the row: toggles the package ID in
  `<data dir>/plugins/disabled.json` and mounts or unmounts the package
  immediately.
- **Reload**: unmounts the package, re-reads its directory on disk, and mounts
  (or defers) the updated package. Shown when files on disk changed since the
  last mount or when the package is in a failed or faulted state.
- **Uninstall**: unmounts the package, removes its ID from `disabled.json`, and
  deletes `<data dir>/plugins/<id>/`. Stored files under
  `<data dir>/plugin-data/<id>/` remain on disk.

## CLI development loop

Use the `aria plugin` subcommands to scaffold, run, validate, and archive
packages. See [Command line](../command-line.md#plugin) for all flags and exit
codes.

```text
aria plugin new com.example.color-tools
aria plugin dev com.example.color-tools
aria plugin check com.example.color-tools
aria plugin pack com.example.color-tools
```

- `aria plugin new <id> [--tier wasm|native] [--dir PARENT] [--sdk PATH]`:
  creates `<id>/` with `plugin.toml`, `shaders/<slug>.wgsl`, `src/lib.rs`,
  `Cargo.toml`, `.gitignore`, and (for WASM) `wit/plugin.wit`.
- `aria plugin dev <dir> [--editor PATH] [--data-dir PATH]`: builds the crate
  in debug mode, stages `plugin.wasm` or `lib/<os>-<arch>.<ext>`, starts the
  editor with `ARIA_PLUGIN_DEV=<dir>`, and polls `Cargo.toml`, `src/`, and
  `wit/` every 500 ms to rebuild on change.
- `aria plugin check <dir|file.ariaplugin>`: validates `manifest`, `shaders`,
  `permissions`, and `abi` without launching the editor.
- `aria plugin pack <dir> [--out FILE] [--no-build]`: builds the crate in
  release mode, runs `check`, and writes `<dir>/<id>-<version>.ariaplugin` with
  source and build entries (`Cargo.toml`, `Cargo.lock`, `src`, `wit`, `target`)
  excluded.

## Failures, faults, cost, and logs

**Settings > Plugins** reports runtime diagnostics per package:

- **Status badge**: `Active` (mounted), `On demand` (deferred until an
  `[activation]` event fires), `Disabled`, `Failed` (manifest, load, or `mount`
  error from `ctx.failures()`), `Faulted` (runtime trap, panic, timeout, or
  budget overrun from `ctx.faults()`), and `Reload needed` (files on disk
  changed since mount).
- **Cost readout**: mount duration, last and peak per-frame render time
  (`frame_cost`), and last and peak per-block audio time (`block_cost`) from
  `ctx.costs()`.
- **Log drawer**: toggling **Log** on a plugin row displays the newest
  `LOG_LINES_PER_PLUGIN` (256) lines (each up to `LOG_LINE_MAX_BYTES` = 4,096
  bytes) with a **Copy** button and forwards every line to `tracing`. See
  [Logging](basics.md#logging).
- **WASM resource limits**: fuel per call and memory limit in MiB for
  `Tier::Wasm` packages, persisted in `[plugin_limits]` and applied across
  mounted WASM plugins when updated.
