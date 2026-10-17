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
| `Tier::Data` | Neither `plugin.wasm` nor `lib/` | Declared WGSL effects, generators, and transitions, plugin textures, menu rows, key chords, settings, `.ariapreset` files, and themes. Cannot declare `[[commands]]`. Mounts at startup without running guest code. |
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
- Command icons (`icons/*.svg`): relative SVG paths without `..` segments, each
  at most `ICON_MAX_BYTES` (64 KiB), parsed by `CommandIcon::from_svg`.
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

Every table in `plugin.toml` rejects unknown keys (`deny_unknown_fields`).

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
enablement = "clipSelected && !playing"

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
```

### `[plugin]`

Required identity and compatibility table.

| Key | Type | Rules |
| --- | --- | --- |
| `id` | string | Reverse-domain ID with at least two dot-separated labels (`com.example.tools`). Each label contains only `a-z`, `0-9`, or `-`, and cannot start or end with `-`. The `aria.` prefix is reserved for shipped plugins. |
| `name` | string | Non-empty display name after trimming whitespace. |
| `version` | string | Full `major.minor.patch` semantic version parsed by `semver::Version`. |
| `api` | integer or string | Plugin host API major version (`1` or `"1"`). Must equal `API_MAJOR` (`1`). |
| `publisher` | string | Optional publisher name (default `""`). |
| `description` | string | Optional summary (default `""`). |
| `license` | string | Optional SPDX license expression (default `""`). |
| `requires` | array of strings | Optional dependency list (default `[]`). Each entry is `"<plugin-id>"` or `"<plugin-id> <version-req>"` where `<version-req>` parses as `semver::VersionReq`. Target IDs must be valid plugin IDs and cannot match `plugin.id`. |

### `[permissions]`

Optional capability grants (defaults to no access when omitted). `Package::plugin_manifest`
maps these grants to host service names in `PluginManifest::uses`:

| Key | Allowed values | Default | Host services / scope |
| --- | --- | --- | --- |
| `project` | `"none"`, `"read"`, `"read-write"` | `"none"` | `"read"` and `"read-write"` add `ProjectHost` (`"read"` permits snapshots; `"read-write"` also permits transactions and undo). |
| `media` | `"none"`, `"read"`, `"read-write"` | `"none"` | `"read"` and `"read-write"` add `DecoderService` and `AudioHost`. |
| `gpu` | `false`, `true` | `false` | `true` adds `GpuContext` and `RenderHost`. |
| `net` | array of host strings | `[]` | Allowed network hosts (`api.example.com`, `*.cdn.example.com`). Each entry must be non-empty and contain only ASCII alphanumeric characters, `-`, `.`, or `*`. |
| `fs` | array of strings | `[]` | File-system scopes (`FsScope`). Each entry is `"project"`, `"pick"`, or an absolute path (`/path/to/dir`). |
| `process` | `false` or array of strings | `false` | Allowed external executables. `process = true` and empty executable strings are rejected. |
| `clipboard` | `false`, `true` | `false` | System clipboard access. |

`SettingsService` and `SettingsSchema` are included in `uses` without a
permission entry.

### `[activation]`

Optional deferred-loading triggers (empty `on` or omitted `[activation]` loads
the package at startup). See [Activation](activation.md).

- `on`: array of trigger strings: `"startup"`, `"command:<id>"`, `"file:<ext>"`,
  `"effect:<id>"`, `"transition:<id>"`, `"generator:<id>"`, `"panel:<id>"`, or
  `"setting:<key>"`.
- Every `"command:<id>"`, `"panel:<id>"`, and `"setting:<key>"` entry must match
  a `[[commands]]`, `[[panels]]`, or `[[settings]]` entry declared in the same
  `plugin.toml` (`setting:<key>` is qualified under `<plugin.id>.<key>` unless
  already prefixed).
- `"file:<ext>"` strips an optional leading `.` and normalizes `<ext>` to
  lowercase ASCII letters, digits, `_`, and `-`.

### `[[effects]]`, `[[generators]]`, and `[[transitions]]`

Optional declared GPU shaders registered into the compositor when the package
mounts or defers. See [Effects and transitions](effects.md).

- `id`: `<name>` or `<plugin.id>.<name>` containing only ASCII letters, digits,
  `-`, `_`, and `.`. Unprefixed IDs are qualified as `<plugin.id>.<id>`. Every
  effect, generator, and transition in a package shares one catalog ID
  namespace and must be unique.
- `name`: non-empty display label.
- `category` (`[[effects]]` and `[[generators]]` only): the Effect menu submenu
  an effect is listed under. An effect without one is listed under Other.
  Generators are not listed in the Effect menu.
- `shader` or `passes`: effects and generators specify either `shader =
  "path.wgsl"` or `passes`, never both and never neither. Transitions take
  `shader = "path.wgsl"` only. Every path must be relative to the package root
  without `..` segments.
- `passes`: an array whose entries are a WGSL path (a fragment pass with
  default name, inputs, and size) or a pass table, written inline or as
  `[[effects.passes]]` / `[[generators.passes]]`. A pass table takes `file`
  (required), `name`, `stage` (`"fragment"` or
  `{ compute = { workgroup = [x, y] } }`), `inputs` (each `"source"`,
  `{ pass = "<name>" }`, `{ layer = "<param>" }`, or `{ texture = "<id>" }`),
  and `output` (`"same"`, `{ scale = [x, y] }`, or `{ fixed = [w, h] }`), with
  the defaults and rules of [Effect graphs](effects.md#effect-graphs). A
  texture input is qualified like a texture id and must name a `[[textures]]`
  entry of the same package.
- `bounds` (`[[effects]]` and `[[generators]]` only): optional
  `{ left, top, right, bottom }` pixel margins the effect draws past its layer
  (default all `0`).
- `reads_outside` and `time_dependent` (`[[effects]]` and `[[generators]]`
  only): optional booleans (default `false`). See
  [Bounds and traits](effects.md#bounds-and-traits).
- `params`: optional array of `EffectParamSpec` tables (`name`, `label`,
  optional `kind` = `"scalar"` | `"int"` | `"bool"` | `"choice"` | `"color"` |
  `"vec2"` | `"point3"` | `"size"` | `"angle"` | `"curve"` | `"gradient"` |
  `"text"` | `"font"` | `"file"` | `"layer"`, `default`, `min`, `max`,
  `soft_min`, `soft_max`, `unit`, `display`, `default_int`, `int_min`,
  `int_max`, `default_bool`, `choices`, `default_choice`, `default_color`,
  `default_linear_color`, `default_vec2`, `default_vec3`, `default_curve`,
  `default_gradient`, `default_text`, `group`, `visible_when`, `tooltip`).
- `groups` (`[[effects]]` and `[[generators]]` only): optional array of
  `EffectGroupSpec` tables (`id`, `label`, optional `collapsed = true | false`).

### `[[textures]]`

Optional images that effect and generator passes read with
`{ texture = "<id>" }`. See [Plugin textures](effects.md#plugin-textures).

- `id`: `<name>` or `<plugin.id>.<name>` containing only ASCII letters, digits,
  `-`, `_`, and `.`. Unprefixed IDs are qualified as `<plugin.id>.<id>`. Each id
  is declared once.
- `file`: an image path relative to the package root without `..` segments.

Mounting registers every texture before the package's effects. An image that
is missing, does not decode, or is larger than the limits above fails the
mount, naming the texture and the file, and leaves no texture or effect of the
package registered.

### `[[commands]]`

Optional command palette entries implemented by the package's WASM or native
code. See [Commands](commands.md).

- `id`: non-empty command ID, unique within `[[commands]]`.
- `title`: non-empty command palette title.
- `key`: optional default chord on the `editor` route, validated by
  `Chord::parse` (for example `"Ctrl+Alt+L"`).
- `menu`: optional menu path validated by `MenuPath::parse` (for example
  `"Sequence"`, `"Sequence/Color Tools"`, `"Clip"`).
- `icon`: optional relative `.svg` path inside the package.
- `enablement`: optional enablement expression validated by `Enablement::parse`
  (for example `"clipSelected && !playing"`).

### `[[menus]]`

Optional additional menu placements for commands declared in `[[commands]]`.
See [Menus](menus.md).

- `command`: target command ID; must match an entry in `[[commands]]`.
- `menu`: menu path validated by `MenuPath::parse`. Duplicate `(command, menu)`
  placements across `[[commands]]` and `[[menus]]` are rejected.

### `[[keys]]`

Optional additional default key chords for commands declared in `[[commands]]`.
See [Keys](keys.md).

- `command`: target command ID; must match an entry in `[[commands]]`.
- `key`: chord string validated by `Chord::parse`.
- `route`: optional `"editor"` (default) or `"home"` (`KeyRoute::parse`). Each
  command may bind at most one default chord per route across `[[commands]]`
  and `[[keys]]`.

### `[[panels]]`

Optional dockable panels rendered by the package's code. See [Panels](panels.md).

- `id`: non-empty panel ID, unique within `[[panels]]`.
- `title`: non-empty panel tab title.
- `location`: `"sidebar"`, `"inspector"`, or `"timeline"`.

### `[[settings]]`

Optional array of setting field tables registered under the package's section
in Settings. See [Settings](settings.md).

- `key`: `<key>` or `<plugin.id>.<key>`, qualified under `<plugin.id>.` unless
  already prefixed, and unique within `[[settings]]`.
- `label`: non-empty display label.
- `type`: optional `"text"` (default), `"secret"`, `"toggle"`, `"number"`, or
  `"choice"`.
- `placeholder`: optional placeholder string (default `""`).
- `default`: optional string, boolean, integer, or float validated against
  `type`.
- `min`, `max`, `step`: optional `f64` bounds and step for `type = "number"`
  (`min <= max`, `step > 0`). Rejected on other types.
- `options`: required non-empty list of unique non-empty strings for `type =
  "choice"`. Rejected on other types.

### `[[workspaces]]`

Optional named pane arrangements offered in the Window menu and Settings. See
[Workspaces](workspaces.md).

- `id`: 1 to 64 ASCII letters, digits, `-`, `_` and `.`, not starting with `.`,
  unique within `[[workspaces]]`.
- `name`: one line of at most 64 bytes.
- `sidebar_width`, `inspector_width`, `timeline_height`: optional pane sizes in
  pixels, above 0 and at most 16384.
- `sidebar_collapsed`, `inspector_collapsed`, `scopes`: optional booleans.
- `focus`: optional `"media"`, `"preview"`, `"timeline"`, `"inspector"`,
  `"assistant"`, or `"mixer"`.

### `[[clip_types]]`

Optional clip kinds drawn by a generator, offered in the Generators tab and
the palette. See [Clip types](clip-types.md).

- `id`: `<name>` or `<plugin.id>.<name>`, qualified under `<plugin.id>.` unless
  already prefixed, unique within `[[clip_types]]`, at most 128 bytes.
- `name`: non-empty, at most 64 bytes.
- `generator`: a generator id, qualified the same way. Required unless the
  entry sets `shader` or `passes`, and refused beside either.
- `shader`, `passes`: the type's own renderer, on the terms of `[[effects]]`.
  The host registers a generator under the type's id, which no `[[effects]]`
  or `[[generators]]` entry may repeat.
- `params`, `groups`, `time_dependent`: the fields of a type with its own
  renderer, on the terms of `[[effects]]`. Refused beside `generator`.
- `colour`: `"teal"`, `"blue"`, `"purple"`, `"pink"`, `"red"`, `"orange"`,
  `"yellow"`, or `"green"`.
- `icon`: optional relative path to an SVG file inside the package.
- `handles`: optional list of at most 8 unique vec2 parameter names of the
  generator, or of `params` for a type with its own renderer.

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

- **Enable / Disable**: toggles the package ID in `<data dir>/plugins/disabled.json`
  and mounts or unmounts the package immediately.
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
