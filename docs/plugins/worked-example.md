# Worked example

Build, test, and package a `com.example.color-tools` plugin that ships a
declared WGSL effect, a setting, a command with a menu row and default key
chord, and a docked panel rendered from a `UiNode` tree.

## 1. Scaffold the package

Create the package directory with `aria plugin new` (pass `--tier native` to
generate a native `aria-sdk` crate instead of the default WebAssembly
component):

```text
aria plugin new com.example.color-tools
```

The command writes:

```text
com.example.color-tools/
  .gitignore
  Cargo.toml
  plugin.toml
  shaders/
    color-tools.wgsl
  src/
    lib.rs
  wit/
    plugin.wit
```

The generated `plugin.toml` declares the package identity, one WGSL effect
(`com.example.color-tools.color-tools`), and one palette command placed on the
`Sequence` menu:

```toml
[plugin]
id = "com.example.color-tools"
name = "Color Tools"
version = "0.1.0"
api = 1
publisher = ""
description = ""
license = ""

[[effects]]
id = "color-tools"
name = "Color Tools"
shader = "shaders/color-tools.wgsl"

[[effects.params]]
name = "gain"
label = "Gain"
default = 1.0
min = 0.0
max = 2.0

[[commands]]
id = "com.example.color-tools.run"
title = "Run Color Tools"
menu = "Sequence"
```

The generated fragment shader at `shaders/color-tools.wgsl` samples `input_tex`
with `aria_sampler` and multiplies the RGB channels by `param_gain()`:

```wgsl
@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let px = textureSample(input_tex, aria_sampler, in.uv);
    return vec4<f32>(px.rgb * param_gain(), px.a);
}
```

## 2. Add a key chord, menu row, panel, setting, and activation triggers

Edit `plugin.toml` to declare `[permissions]`, `[activation]`, a default key
chord and enablement rule on `com.example.color-tools.run`, an additional
`[[menus]]` placement, a sidebar entry in `[[panels]]`, and a `[[settings]]`
field:

```toml
[plugin]
id = "com.example.color-tools"
name = "Color Tools"
version = "0.1.0"
api = 1
publisher = "Example"
description = "Gain shader, command, and sidebar panel"
license = "MIT OR Apache-2.0"

[permissions]
project = "read"

[activation]
on = [
  "command:com.example.color-tools.run",
  "panel:com.example.color-tools.panel",
  "effect:com.example.color-tools.color-tools",
  "setting:default_gain",
]

[[effects]]
id = "color-tools"
name = "Color Tools"
shader = "shaders/color-tools.wgsl"

[[effects.params]]
name = "gain"
label = "Gain"
default = 1.0
min = 0.0
max = 2.0

[[commands]]
id = "com.example.color-tools.run"
title = "Run Color Tools"
key = "Ctrl+Alt+L"
menu = "Sequence"
enablement = "!playing"

[[menus]]
command = "com.example.color-tools.run"
menu = "Clip"

[[keys]]
command = "com.example.color-tools.run"
key = "Ctrl+Shift+L"
route = "home"

[[panels]]
id = "com.example.color-tools.panel"
title = "Color Tools"
location = "sidebar"

[[settings]]
key = "default_gain"
label = "Default Gain"
type = "number"
default = 1.0
min = 0.0
max = 2.0
step = 0.1
```

When Aria starts, `PluginContext::defer_package` registers the WGSL effect,
command palette entry, `Sequence` and `Clip` menu rows, `Ctrl+Alt+L` and
`Ctrl+Shift+L` key chords, `default_gain` setting field, and waiting sidebar
panel from `plugin.toml` without loading the plugin's code. Invoking the
command, opening the panel, opening a project that uses the effect, or editing
the setting loads and mounts the package.

## 3. Implement the command and panel

### WebAssembly tier (`--tier wasm`)

In `src/lib.rs`, register the command and sidebar panel during `mount`, read
the `com.example.color-tools.default_gain` setting in `panel_view`, and handle
button presses in `panel_event`:

```rust
use std::sync::atomic::{AtomicU32, Ordering};

wit_bindgen::generate!({
    world: "plugin",
    path: "wit",
});

const COMMAND_ID: &str = "com.example.color-tools.run";
const COMMAND_TITLE: &str = "Run Color Tools";
const PANEL_ID: &str = "com.example.color-tools.panel";
const SETTING_KEY: &str = "com.example.color-tools.default_gain";

static RUNS: AtomicU32 = AtomicU32::new(0);

struct ColorToolsPlugin;

impl Guest for ColorToolsPlugin {
    fn mount() -> Result<(), String> {
        log(LogLevel::Info, "mounted com.example.color-tools");
        register_command(COMMAND_ID, COMMAND_TITLE)?;
        register_panel(&PanelSpec {
            id: PANEL_ID.to_owned(),
            title: "Color Tools".to_owned(),
            location: PanelLocation::Sidebar,
        })?;
        Ok(())
    }

    fn unmount() -> Result<(), String> {
        Ok(())
    }

    fn command_run(id: String, _args_json: String) -> Result<(), String> {
        match id.as_str() {
            COMMAND_ID => {
                let count = RUNS.fetch_add(1, Ordering::Relaxed) + 1;
                storage_set("last_run", count.to_string().as_bytes())?;
                refresh_panel(PANEL_ID)?;
                Ok(())
            }
            other => Err(format!("unknown command: {other}")),
        }
    }

    fn event(_kind: String, _event_json: String) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn import(importer_id: String, _path: String) -> Result<ImportResult, String> {
        Err(format!("unknown importer: {importer_id}"))
    }

    fn export(
        exporter_id: String,
        _project_json: String,
        _path: String,
    ) -> Result<Report, String> {
        Err(format!("unknown exporter: {exporter_id}"))
    }

    fn audio_create(effect_id: String, _sample_rate: u32) -> Result<Instance, String> {
        Err(format!("unknown audio effect: {effect_id}"))
    }

    fn audio_process(
        instance: Instance,
        _params: Vec<AudioParam>,
        _block: AudioBlock,
    ) -> Result<StereoBuffers, String> {
        Err(format!("unknown audio instance: {instance}"))
    }

    fn audio_reset(instance: Instance) -> Result<(), String> {
        Err(format!("unknown audio instance: {instance}"))
    }

    fn audio_drop(_instance: Instance) -> Result<(), String> {
        Ok(())
    }

    fn panel_view(id: String) -> Vec<UiNode> {
        if id != PANEL_ID {
            return Vec::new();
        }
        let gain = setting_get(SETTING_KEY)
            .ok()
            .flatten()
            .unwrap_or_else(|| "1.0".to_owned());
        let runs = RUNS.load(Ordering::Relaxed);
        vec![
            UiNode::Column(vec![1, 2, 3, 4]),
            UiNode::Heading("Color Tools".to_owned()),
            UiNode::Field(UiField {
                label: "Default gain".to_owned(),
                value: gain,
            }),
            UiNode::Field(UiField {
                label: "Runs".to_owned(),
                value: runs.to_string(),
            }),
            UiNode::Button(UiButton {
                id: "run".to_owned(),
                label: "Run Color Tools".to_owned(),
                disabled: false,
            }),
        ]
    }

    fn panel_event(id: String, event: PanelEvent) -> Result<(), String> {
        if id != PANEL_ID {
            return Err(format!("unknown panel: {id}"));
        }
        if let PanelEvent::Pressed(widget) = event
            && widget == "run"
        {
            Self::command_run(COMMAND_ID.to_owned(), String::new())?;
        }
        Ok(())
    }

    fn overlay_scene(_id: String) -> Vec<OverlayShape> {
        Vec::new()
    }

    fn overlay_event(id: String, _event: OverlayEvent) -> Result<(), String> {
        Err(format!("unknown overlay: {id}"))
    }

    fn encoder_open(
        encoder_id: String,
        _path: String,
        _config: EncoderConfig,
    ) -> Result<Instance, String> {
        Err(format!("unknown encoder: {encoder_id}"))
    }

    fn encoder_push_frame(instance: Instance, _frame: EncoderFrame) -> Result<(), String> {
        Err(format!("unknown encoder instance: {instance}"))
    }

    fn encoder_push_audio(instance: Instance, _audio: EncoderAudio) -> Result<(), String> {
        Err(format!("unknown encoder instance: {instance}"))
    }

    fn encoder_finish(instance: Instance) -> Result<(), String> {
        Err(format!("unknown encoder instance: {instance}"))
    }

    fn encoder_drop(_instance: Instance) -> Result<(), String> {
        Ok(())
    }
}

export!(ColorToolsPlugin);
```

### Native tier (`--tier native`)

If the package was created with `aria plugin new com.example.color-tools --tier
native`, implement `aria_sdk::Plugin` and `aria_sdk::Panel` in `src/lib.rs`:

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use aria_sdk::{
    Host, LogLevel, Panel, PanelEvent, PanelLocation, PanelSpec, Plugin, UiNode,
};

const COMMAND_ID: &str = "com.example.color-tools.run";
const COMMAND_TITLE: &str = "Run Color Tools";
const PANEL_ID: &str = "com.example.color-tools.panel";
const SETTING_KEY: &str = "com.example.color-tools.default_gain";

#[derive(Debug, Default)]
pub struct ColorToolsPlugin {
    runs: Arc<AtomicU32>,
}

struct ColorToolsPanel {
    runs: Arc<AtomicU32>,
}

impl Panel for ColorToolsPanel {
    fn view(&self, host: &Host) -> UiNode {
        let gain = host
            .setting_get_json(SETTING_KEY)
            .ok()
            .flatten()
            .unwrap_or_else(|| "1.0".to_owned());
        let runs = self.runs.load(Ordering::Relaxed);
        UiNode::column([
            UiNode::heading("Color Tools"),
            UiNode::field("Default gain", gain),
            UiNode::field("Runs", runs.to_string()),
            UiNode::button("run", "Run Color Tools"),
        ])
    }

    fn event(&self, host: &Host, event: &PanelEvent) -> Result<(), String> {
        match event {
            PanelEvent::Pressed { id } if id.as_ref() == "add" || id.as_ref() == "run" => {
                let count = self.runs.fetch_add(1, Ordering::Relaxed) + 1;
                host.storage_set("last_run", count.to_string().as_bytes())?;
                Ok(())
            }
            other => Err(format!("unexpected control event on {}", other.id())),
        }
    }
}

impl Plugin for ColorToolsPlugin {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.log(LogLevel::Info, "mounted com.example.color-tools");
        host.register_command(COMMAND_ID, COMMAND_TITLE)?;
        host.register_panel(
            PanelSpec::new(PANEL_ID, "Color Tools", PanelLocation::Sidebar),
            ColorToolsPanel {
                runs: Arc::clone(&self.runs),
            },
        )?;
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args_json: &str) -> Result<(), String> {
        match id {
            COMMAND_ID => {
                let count = self.runs.fetch_add(1, Ordering::Relaxed) + 1;
                host.storage_set("last_run", count.to_string().as_bytes())?;
                host.refresh_panel(PANEL_ID)?;
                Ok(())
            }
            other => Err(format!("unknown command: {other}")),
        }
    }
}

aria_sdk::export!(ColorToolsPlugin);
```

## 4. Run, validate, pack, and install

A WASM-tier package builds for `wasm32-unknown-unknown`. Install the target
once:

```text
rustup target add wasm32-unknown-unknown
```

Start the live development loop against an isolated profile directory:

```text
aria plugin dev com.example.color-tools --data-dir /var/tmp/aria-dev-profile
```

`aria plugin dev` builds the crate, stages `plugin.wasm` (or
`lib/<os>-<arch>.<ext>`), launches the editor with `ARIA_PLUGIN_DEV` pointing at
`com.example.color-tools`, and rebuilds whenever `Cargo.toml`, `src/`, or
`wit/` changes. After a rebuild, click **Reload** on the `Color Tools` row in
**Settings > Plugins**.

Run the four offline validation checks (`manifest`, `shaders`, `permissions`,
`abi`):

```text
aria plugin check com.example.color-tools
```

Build the release binary and write the `.ariaplugin` package archive:

```text
aria plugin pack com.example.color-tools
```

This writes `com.example.color-tools/com.example.color-tools-0.1.0.ariaplugin`.
Install the archive into Aria with **File > Install Plugin...**, **Settings >
Plugins > Install from File...**, or by dropping the `.ariaplugin` file onto
the editor window, then confirm the `project = "read"` permission prompt.
