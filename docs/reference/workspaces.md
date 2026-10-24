# Workspaces Reference

A workspace is a named arrangement of dock frames, split ratios, panel tabs, and pane dimensions (`crates/aria-ui/src/layouts.rs` and `crates/aria-ui/src/dock/mod.rs`). Switching workspaces updates the dock tree, the sidebar and inspector widths, the timeline height, the scope strip, the timeline graph editor, the focused panel, and the Effects & Presets category filter. A workspace with **Graph open** set to `false` closes the graph editor.

## Worked example

Switch to the **Audio** workspace, adjust a splitter, save the arrangement, and reset to the saved layout:

1. Press `Shift+Alt+4` or choose **Window > Workspaces > Audio** (`win-workspace-audio`). The dock switches to `Audio`, focuses the Timeline, and filters Effects & Presets to `Audio Effects`.
2. Choose **Window > Workspaces > Save Workspace** (`win-workspace-save`), type `audio`, and press Enter. Aria stores the arrangement in the `Custom` slot (`win-workspace-custom` in `ui-settings.json`) and writes `<data dir>/workspaces/audio.json`.
3. Choose **Window > Workspaces > Reset to Saved Layout** (`win-workspace-reset`) to restore the active workspace to its saved or shipped frame tree and split ratios.

## Shipped workspaces

| ID | Name | Shortcut | Sidebar width | Sidebar collapsed | Inspector width | Inspector collapsed | Timeline height | Scopes open | Graph open | Focused panel | Route |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `win-workspace-editing` | Editing | `Shift+Alt+1` | `256` | `false` | `520` | `false` | `280` | `false` | `false` | `Timeline` | `Editor` |
| `win-workspace-animation` | Animation | `Shift+Alt+2` | `280` | `false` | `480` | `false` | `360` | `false` | `true` | `Timeline` | `Editor` |
| `win-workspace-colour` | Color | `Shift+Alt+3` | `208` | `true` | `700` | `false` | `200` | `false` | `false` | `Preview` | `Editor` |
| `win-workspace-audio` | Audio | `Shift+Alt+4` | `240` | `false` | `440` | `false` | `420` | `false` | `false` | `Timeline` | `Editor` |
| `win-workspace-effects` | Effects | `Shift+Alt+5` | `208` | `true` | `760` | `false` | `240` | `false` | `false` | `Inspector` | `Editor` |

## Default dock trees

Each shipped workspace builds a `DockTree` (`DOCK_VERSION = 2`) of an upper and a
lower row, each a share of the dock area's height. A frame's width is a share of
its row, a width in pixels kept at every window width wide enough for it, or
`Rest`, the part of the row the other frames leave. Focused marks the frame that
has keyboard focus when the workspace is chosen.

| Workspace | Row | Height | Frame | Width | Tabs | Visible tab | Focused |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `win-workspace-editing` | Upper | 55% | Left | 320 px | `aria.panel.media`, `aria.panel.inspector` | `aria.panel.media` | no |
| `win-workspace-editing` | Upper | 55% | Center | Rest | `aria.panel.viewer` | `aria.panel.viewer` | no |
| `win-workspace-editing` | Upper | 55% | Right | 300 px | `aria.panel.effects` | `aria.panel.effects` | no |
| `win-workspace-editing` | Lower | 45% | Bottom | Rest | `aria.panel.timeline` | `aria.panel.timeline` | yes |
| `win-workspace-editing` | Lower | 45% | Meters | 64 px | `aria.panel.audio_meters` | `aria.panel.audio_meters` | no |
| `win-workspace-animation` | Upper | 50% | Left | 320 px | `aria.panel.inspector`, `aria.panel.media` | `aria.panel.inspector` | no |
| `win-workspace-animation` | Upper | 50% | Center | Rest | `aria.panel.viewer`, `aria.panel.mixer`, `aria.panel.assistant` | `aria.panel.viewer` | no |
| `win-workspace-animation` | Upper | 50% | Right | 300 px | `aria.panel.info`, `aria.panel.effects`, `aria.panel.align` | `aria.panel.effects` | no |
| `win-workspace-animation` | Lower | 50% | Bottom | 100% | `aria.panel.timeline` | `aria.panel.timeline` | yes |
| `win-workspace-colour` | Upper | 65% | Center | 38% | `aria.panel.viewer`, `aria.panel.mixer`, `aria.panel.assistant` | `aria.panel.viewer` | yes |
| `win-workspace-colour` | Upper | 65% | Left | 32% | `aria.panel.scopes`, `aria.panel.media` | `aria.panel.scopes` | no |
| `win-workspace-colour` | Upper | 65% | Right | 30% | `aria.panel.color`, `aria.panel.inspector`, `aria.panel.effects` | `aria.panel.color` | no |
| `win-workspace-colour` | Lower | 35% | Bottom | 100% | `aria.panel.timeline` | `aria.panel.timeline` | no |
| `win-workspace-audio` | Upper | 66% | Left | 22% | `aria.panel.viewer`, `aria.panel.media` | `aria.panel.viewer` | no |
| `win-workspace-audio` | Upper | 66% | Center | 50% | `aria.panel.mixer`, `aria.panel.assistant` | `aria.panel.mixer` | no |
| `win-workspace-audio` | Upper | 66% | Right | 28% | `aria.panel.audio_meters`, `aria.panel.effects`, `aria.panel.inspector` | `aria.panel.audio_meters` | no |
| `win-workspace-audio` | Lower | 34% | Bottom | 100% | `aria.panel.timeline` | `aria.panel.timeline` | yes |
| `win-workspace-effects` | Upper | 60% | Left | 32% | `aria.panel.effects`, `aria.panel.media` | `aria.panel.effects` | no |
| `win-workspace-effects` | Upper | 60% | Center | 36% | `aria.panel.viewer`, `aria.panel.mixer`, `aria.panel.assistant` | `aria.panel.viewer` | no |
| `win-workspace-effects` | Upper | 60% | Right | 32% | `aria.panel.inspector`, `aria.panel.info` | `aria.panel.inspector` | yes |
| `win-workspace-effects` | Lower | 40% | Bottom | 100% | `aria.panel.timeline` | `aria.panel.timeline` | no |

## Custom and plugin workspaces

- **Per-workspace persistence**: splitter drags, tab reorders, docks, and undocks are saved per workspace ID in `workspaceTrees` inside `ui-settings.json`. A resized window lays out a shipped workspace with no saved edits as the Default dock trees table states for the new width; a workspace with saved edits keeps the shares of its frames.
- **Custom workspace (`win-workspace-custom`)**: created by **Window > Workspaces > Save Workspace** (`win-workspace-save`) or the **Save current** button in **Settings > Workspaces**. Named workspace files are written to `<data dir>/workspaces/<name>.json`. **Reset to Saved Layout** (`win-workspace-reset`) restores the active workspace to its saved or shipped arrangement.
- **Plugin workspaces (`win-workspace-plugin:<id>`)**: plugins register workspaces on `PluginContext::register_workspace`. A plugin workspace opens with the timeline graph editor closed. Shortcut slots `Shift+Alt+1` through `Shift+Alt+9` address the first nine rows in `AppState::workspace_rows()`: the shipped workspaces in the order of the table above, then `Custom` when present, then plugin workspaces in registration order.
