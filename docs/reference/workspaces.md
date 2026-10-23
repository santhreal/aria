# Workspaces Reference

A workspace is a named arrangement of dock frames, split ratios, panel tabs, and pane dimensions (`crates/aria-ui/src/layouts.rs` and `crates/aria-ui/src/dock/mod.rs`). Switching workspaces updates the dock tree, the sidebar and inspector widths, the timeline height, the scope strip, the timeline graph editor, the focused panel, and the Effects & Presets category filter. A workspace with **Graph open** set to `false` closes the graph editor.

## Worked example

Switch to the **Audio** workspace, adjust a splitter, save the arrangement, and reset to the saved layout:

1. Press `Shift+Alt+4` or choose **Window > Workspaces > Audio** (`win-workspace-audio`). The dock switches to `Audio`, focuses the Audio Track Mixer, and filters Effects & Presets to `Audio Effects`.
2. Choose **Window > Workspaces > Save Workspace** (`win-workspace-save`). Aria writes `<data dir>/workspaces/audio.json` and stores the arrangement in the `Custom` slot (`win-workspace-custom` in `ui-settings.json`).
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

Each shipped workspace builds a two-row vertical `DockTree` (`DOCK_VERSION = 2`):

- **Editing (`win-workspace-editing`)**:
  - Upper row (`65%` height): `Left` frame (`320 px` at `1440 px` window width) with `aria.panel.media` and `aria.panel.inspector`; `Center` frame with `aria.panel.viewer`; `Right` frame (`300 px`) with `aria.panel.effects`.
  - Lower row (`35%` height): `Bottom` frame (focused) with `aria.panel.timeline`; `Meters` frame (`64 px`) with `aria.panel.audio_meters`.
- **Animation (`win-workspace-animation`)**:
  - Upper row (`50%` height): `Left` frame with `aria.panel.inspector` (active) and `aria.panel.media`; `Center` frame with `aria.panel.viewer`, `aria.panel.mixer`, and `aria.panel.assistant`; `Right` frame with `aria.panel.info`, `aria.panel.effects` (active), and `aria.panel.align`.
  - Lower row (`50%` height): `Bottom` frame with `aria.panel.timeline` (timeline graph editor open) and `Meters` frame (`64 px`) with `aria.panel.audio_meters`.
- **Color (`win-workspace-colour`)**:
  - Upper row (`65%` height): `Center` frame (`38%` width, focused) with `aria.panel.viewer`, `aria.panel.mixer`, and `aria.panel.assistant`; `Left` frame (`32%` width) with `aria.panel.scopes` (active) and `aria.panel.media`; `Right` frame (`30%` width) with `aria.panel.color` (active), `aria.panel.inspector`, and `aria.panel.effects`.
  - Lower row (`35%` height): `Bottom` frame with `aria.panel.timeline` and `Meters` frame (`64 px`) with `aria.panel.audio_meters`.
- **Audio (`win-workspace-audio`)**:
  - Upper row (`65%` height): `Left` frame (`22%` width) with `aria.panel.viewer` and `aria.panel.media`; `Center` frame (`50%` width, focused) with `aria.panel.mixer` (active) and `aria.panel.assistant`; `Right` frame (`28%` width) with `aria.panel.audio_meters`, `aria.panel.effects` (active, filtered to `Audio Effects`), and `aria.panel.inspector`.
  - Lower row (`35%` height): `Bottom` frame with `aria.panel.timeline`.
- **Effects (`win-workspace-effects`)**:
  - Upper row (`60%` height): `Left` frame (`32%` width) with `aria.panel.effects` (active) and `aria.panel.media`; `Center` frame (`36%` width) with `aria.panel.viewer`, `aria.panel.mixer`, and `aria.panel.assistant`; `Right` frame (`32%` width, focused) with `aria.panel.inspector` (active) and `aria.panel.info`.
  - Lower row (`40%` height): `Bottom` frame with `aria.panel.timeline` and `Meters` frame (`64 px`) with `aria.panel.audio_meters`.

## Custom and plugin workspaces

- **Per-workspace persistence**: splitter drags, tab reorders, docks, and undocks are saved per workspace ID in `workspaceTrees` inside `ui-settings.json`.
- **Custom workspace (`win-workspace-custom`)**: created by **Window > Workspaces > Save Workspace** (`win-workspace-save`) or the **Save current** button in **Settings > Workspaces**. Named workspace files are written to `<data dir>/workspaces/<name>.json`. **Reset to Saved Layout** (`win-workspace-reset`) restores the active workspace to its saved or shipped arrangement.
- **Plugin workspaces (`win-workspace-plugin:<id>`)**: plugins register workspaces on `PluginContext::register_workspace`. A plugin workspace opens with the timeline graph editor closed. Shortcut slots `Shift+Alt+1` through `Shift+Alt+9` address the first nine rows in `AppState::workspace_rows()`: the shipped workspaces in the order of the table above, then `Custom` when present, then plugin workspaces in registration order.
