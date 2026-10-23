# Tools Reference

The editor toolbar offers thirteen tools (`crates/aria-ui/src/tool_catalog.rs`). Each tool sets how pointer clicks and drags behave on the timeline and in the Viewer. Tool shortcuts fire on the editor route whenever no text input, search field, or modal dialog holds focus.

## Worked example

Trim a cut between two adjacent clips without changing the sequence duration, then slip the incoming clip's source window:

1. Press `N` to select **Rolling Edit** (`tool-roll`).
2. Drag the cut between the outgoing and incoming clips left or right by 12 frames. The outgoing clip's out-point and the incoming clip's in-point move together as one undo step.
3. Press `Y` to select **Slip** (`tool-slip`).
4. Drag the body of the incoming clip left or right to shift its source in-point and out-point by the same delta while keeping its timeline start and duration fixed.
5. Press `V` to return to **Select** (`tool-pointer`).

## Built-in tools

| ID | Label | Key | Group | Body drag | Edge drag | Track select | Summary |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `tool-pointer` | Select | `v` | `Select` | `none` | `none` | `none` | Selects clips, starts a marquee selection on empty lane space, drags clip bodies to move (`Shift` for ripple move), and drags clip edges to trim (`Shift` for ripple trim). `Esc` returns to this tool. |
| `tool-track-forward` | Track Select Forward | `a` | `TrackSelect` | `none` | `none` | `Forward` | Selects the clicked clip and every clip starting at or after it on unlocked tracks (`Shift+click` restricts to the clicked track). |
| `tool-track-backward` | Track Select Backward | `Shift+a` | `TrackSelect` | `none` | `none` | `Backward` | Selects the clicked clip and every clip ending at or before it on unlocked tracks (`Shift+click` restricts to the clicked track). |
| `tool-ripple` | Ripple Trim | `b` | `Trim` | `none` | `Ripple` | `none` | Trims the dragged clip edge and shifts subsequent clips on the edited lane and sync-locked lanes so no gap is left. |
| `tool-roll` | Rolling Edit | `n` | `Trim` | `none` | `Roll` | `none` | Moves the shared cut between two abutting clips in one undo step without changing their combined span. |
| `tool-rate` | Rate Stretch | `r` | `Trim` | `none` | `Rate` | `none` | Changes how long a clip plays by dragging its edge while keeping the same source window (`speed` scales inversely with length). |
| `tool-slip` | Slip | `y` | `Trim` | `Slip` | `none` | `none` | Moves the take (`source_in` and `source_out`) inside a clip while keeping the clip's timeline edges fixed. |
| `tool-slide` | Slide | `u` | `Trim` | `Slide` | `none` | `none` | Moves a clip along its lane and gives the difference to its adjacent outgoing and incoming neighbours. |
| `tool-razor` | Razor | `c` | `Razor` | `none` | `none` | `none` | Splits the clicked clip at the pointer frame (`Shift+click` cuts every unlocked track at that frame). |
| `tool-pen` | Pen | `p` | `Pen` | `none` | `none` | `none` | Adds a keyframe on a clip's rubber-band line in the timeline, or draws and edits Bézier masks and motion-path keyframes in the Viewer. |
| `tool-hand` | Hand | `h` | `Navigate` | `none` | `none` | `none` | Scrolls the timeline horizontally on drag, or pans the Viewer frame when zoomed in. |
| `tool-zoom` | Zoom | `z` | `Navigate` | `none` | `none` | `none` | Changes the timeline horizontal zoom centered on the clicked position (`Alt+click` zooms out). |
| `tool-type` | Type | `t` | `Type` | `none` | `none` | `none` | Clicks in the Viewer to create a text clip at the playhead or place the caret inside an existing text clip. |

## Tool bar groups

The toolbar draws one button per `ToolGroup` (`ToolGroup::ALL` in `crates/aria-ui/src/tool_catalog.rs`):

- `Select`: `tool-pointer`
- `TrackSelect`: `tool-track-forward`, `tool-track-backward`
- `Trim`: `tool-ripple`, `tool-roll`, `tool-rate`, `tool-slip`, `tool-slide`
- `Razor`: `tool-razor`
- `Pen`: `tool-pen`
- `Navigate`: `tool-hand`, `tool-zoom`
- `Type`: `tool-type`

Groups with more than one tool display the active member on the button (or the group's first tool when another group's tool is armed) and list the remaining variants in the button's popover.

## Pen tool vertex modes

When `tool-pen` is active, four vertex sub-modes (`PenVariant::ALL` in `crates/aria-ui/src/types_editor.rs`) control how clicks interact with a path mask in the Viewer:

| Variant | Label | Behavior |
| --- | --- | --- |
| `Standard` | Pen Tool | Clicks away from the active path start a new path mask or append a vertex (`click` for a corner vertex, `click-drag` for smooth mirrored tangents); clicking a segment inserts a vertex; `Ctrl+click` on a vertex deletes it; `Alt+click` or `Alt+drag` converts a vertex between corner and smooth. |
| `AddVertex` | Add Vertex Tool | Inserts a vertex on a path segment without holding a modifier. |
| `DeleteVertex` | Delete Vertex Tool | Removes a clicked vertex from a path mask without holding `Ctrl` (refuses when fewer than 3 vertices would remain). |
| `ConvertVertex` | Convert Vertex Tool | Toggles a clicked vertex between corner and smooth tangents without holding `Alt`. |
