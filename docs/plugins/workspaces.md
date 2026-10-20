# Workspaces

A workspace is a named arrangement of the editor: pane sizes, which panes are
collapsed, whether the scope strip is open, and which pane has the keyboard.
A plugin workspace is a row in the Window menu and in Settings > Workspaces,
after the shipped Editing, Animation, Colour, Audio, and Effects rows. Settings
shows the registering plugin's name beside the row's measurements.

A package declares workspaces in `plugin.toml`, which serves every tier,
including a package with no code:

```toml
[plugin]
id = "com.example.workspaces"
name = "Grading Workspaces"
version = "0.1.0"
api = 1

[[workspaces]]
id = "grading"
name = "Grading Room"
sidebar_collapsed = true
inspector_width = 520
timeline_height = 230
scopes = true
focus = "preview"
```

An in-process plugin registers one with `PluginContext::register_workspace`
(or `Host::register_workspace` in `aria-sdk` and `register-workspace` in
WebAssembly):

```rust
use aria_plugin::{PluginContext, PluginError, WorkspaceFocus, WorkspacePanes, WorkspaceSpec};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_workspace(WorkspaceSpec {
        id: "grading".into(),
        name: "Grading Room".into(),
        panes: WorkspacePanes {
            sidebar_collapsed: Some(true),
            inspector_width: Some(520.0),
            timeline_height: Some(230.0),
            scopes: Some(true),
            focus: Some(WorkspaceFocus::Preview),
            ..WorkspacePanes::default()
        },
    })?;
    Ok(())
}
```

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | string | required | Workspace identifier: 1 to `WORKSPACE_ID_MAX_BYTES` (64) bytes of ASCII letters, digits, `-`, `_`, and `.`, not starting with `.`, unique within the plugin. |
| `name` | string | required | Display name: 1 to `WORKSPACE_NAME_MAX_BYTES` (64) bytes after trimming, without line breaks (`\n` or `\r`). |
| `sidebar_width` | float (`f32`) | none (Editing width) | Left sidebar width in pixels, finite and in `(0.0, 16384.0]` (`WORKSPACE_MAX_SIZE`). |
| `sidebar_collapsed` | boolean | none (`false`, shown) | Whether the left sidebar starts collapsed. |
| `inspector_width` | float (`f32`) | none (Editing width) | Right inspector width in pixels, finite and in `(0.0, 16384.0]` (`WORKSPACE_MAX_SIZE`). |
| `inspector_collapsed` | boolean | none (`false`, shown) | Whether the right inspector starts collapsed. |
| `timeline_height` | float (`f32`) | none (Editing height) | Timeline height in pixels, finite and in `(0.0, 16384.0]` (`WORKSPACE_MAX_SIZE`). |
| `scopes` | boolean | none (`false`, closed) | Whether the video scope strip is open. |
| `focus` | `"media"`, `"preview"`, `"timeline"`, `"inspector"`, `"assistant"`, or `"mixer"` (`WorkspaceFocus`) | none (`"timeline"`) | Pane receiving keyboard focus when the workspace is applied. |
| `tree` | string | none (derived from pane sizes and visibility) | Serialized `DockTree` JSON string overriding the default dock split tree. |

Applying a workspace sets the panes, opens or closes the scope strip, moves
the keyboard to the focus pane, and goes to the editor. It never changes the
project and adds no undo step. Sizes are clamped to the range each pane
accepts. The row reads as in use from the moment it is applied until another
workspace is chosen.

`register_workspace` returns `PluginError::WorkspaceRefused(message)` when the
id is empty, over `WORKSPACE_ID_MAX_BYTES` (64), starts with `.`, or holds a
byte other than an ASCII letter, digit, `-`, `_` or `.`; when the plugin
already registered the id; when the name is blank, over
`WORKSPACE_NAME_MAX_BYTES` (64), or holds a line break; and when a size is not
finite, not above 0, or over `WORKSPACE_MAX_SIZE` (16384). A package whose
`[[workspaces]]` entry fails these checks, contains an unknown field, or
repeats an id fails to load with the entry identified.

The returned `Registration` is held on the terms of `register_theme`. A
workspace whose registration drops leaves the Window menu and Settings.
