# Workspaces

A workspace is a named arrangement of the editor: pane sizes, which panes are
collapsed, whether the scope strip is open, and which pane has the keyboard.
A plugin workspace is a row in the Window menu and in Settings > Workspaces,
after the shipped Editing, Animation, Colour, Audio, and Effects rows. Settings
shows the registering plugin's name beside the row's measurements.

A package declares workspaces in `plugin.toml`, which serves every tier,
including a package with no code:

```toml
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

| Field | Value | Unset |
| --- | --- | --- |
| `sidebar_width` | Pixels | Editing width |
| `sidebar_collapsed` | Boolean | Shown |
| `inspector_width` | Pixels | Editing width |
| `inspector_collapsed` | Boolean | Shown |
| `timeline_height` | Pixels | Editing height |
| `scopes` | Boolean | Closed |
| `focus` | `Media`, `Preview`, `Timeline`, `Inspector`, `Assistant`, or `Mixer` | Timeline |
| `tree` | Serialized `DockTree` JSON string | Derived from pane sizes and visibility |
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
`[[workspaces]]` entry fails these checks, names an unknown field, or repeats
an id fails to load with the entry named.

The returned `Registration` is held on the terms of `register_theme`. A
workspace whose registration drops leaves the Window menu and Settings.
