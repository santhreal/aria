# Layout

## Window

```
+------------------------------------------------------------------------------+
| Menu bar                                                          28 px      |
| Tool bar                                                          36 px      |
+----------------------+--------------------------------+----------------------+
| Project | Effect     | Viewer                         | Effects & Presets    |
| Controls             |                                |                      |
|                      |                                |                      |
|   left frame         |                                |   right column       |
|   320 px             |                                |   300 px             |
+----------------------+--------------------------------+----------------------+
| Timeline: Sequence 01 | Sequence 02                              | Audio     |
|                                                                  | meters    |
|                                                                  | 64 px     |
+------------------------------------------------------------------------------+
| Status bar                                                        22 px      |
+------------------------------------------------------------------------------+
```

The window opens maximized on the display it last closed on. The first launch
opens maximized on the primary display. Size, position, and maximized state
are restored per display arrangement. A window smaller than 1024 x 640 shows
the Viewer and the Timeline only; the other frames return when the window
grows past that size.

The upper area takes 55 % of the height between the tool bar and the status
bar. The Timeline takes the rest.

The Editing workspace opens with six panels: Project, Effect Controls, Viewer,
Effects & Presets, Timeline, and Audio Meters. Every other panel opens from
the Window menu as a tab in its default frame: Markers and Scopes on the left,
Audio Track Mixer and Assistant beside the Viewer, and Info, Character, Jobs,
History, and the rest in the right column. Audio Meters opens right of the
Timeline. When a panel's default frame has closed, such as the right column
after its last panel floats, opening the panel re-creates that frame at its
shipped edge: the left or right column at that end of the Viewer's row, at
320 px or 300 px of a 1440 px window; the Timeline frame as a lower row, left
of Audio Meters when docked; Audio Meters at 64 px right of the Timeline. A
panel opens as a tab of the largest frame when its default frame is the
Viewer's, or when its column has no docked Viewer frame to sit beside.

## Frames and panels

A frame holds one or more panels. Each panel has a tab. The tab row is 24 px
high and shows the panel name, a 16 px close cross, and a panel menu button at
its right end. The cross shows on the visible tab and under the pointer, and keeps its place while hidden. Tabs wider than the frame narrow, the menu button
does not: when the tabs have no room for their whole names, every cross is hidden, then the names take an ellipsis, and a tab's tooltip shows its full name. A tab narrows no further than the first three letters of its name and an ellipsis.
When the frame's tabs do not fit at that width, the row draws as many as fit, always including the visible tab, and ends in a chevron button, More panels, whose menu lists the other tabs on 22 px rows; picking one makes it the visible panel.
In a frame narrower than 120 px the tabs and the menu button take 4 px of padding instead of 8.
The menu button and a right-click on a tab open the frame menu under the tab row, inside the window: Close Panel, Close Other Tabs in Frame, Undock Panel, Maximize Frame, and Add Panel, whose flyout lists the panels the frame does not hold. Its rows are 22 px, as the menu bar's are.

| Gesture | Result |
| --- | --- |
| Click a tab | That panel becomes the frame's visible panel. |
| Drag a tab inside its row | Reorders the tabs. |
| Drag a tab over another frame | Shows drop zones: centre (add as tab), top, bottom, left, right (split the frame). Releasing on a zone docks the panel there. |
| Drag a tab outside the window | Opens the panel in a floating window. |
| Drag a floating window's tab back over a frame | Docks it, with the same drop zones. |
| Close a floating window (Close Window, or the window manager's close) | Closes its panels. The Window menu opens each one again in its default frame. |
| Grave accent (`` ` ``) | Maximizes the frame under the pointer to the whole area between the tool bar and the status bar. Pressed again, restores it. |
| Double-click a tab | Same as grave accent. |
| Middle-click a tab | Closes the panel. |
| Ctrl+W on a focused panel | Closes the panel, or the sequence tab when the Timeline has focus. |

A drop zone is a translucent accent rectangle covering the half or the centre
of the target frame. It appears 80 ms after the pointer enters the frame and
follows the pointer between zones with a 120 ms fade.

Frames stand 6 px apart and 6 px in from the bars and the window edges. The
gap between two frames is the splitter: it draws no line at rest, and hovering
it shows the resize cursor and a 1 px accent line. Double-clicking a splitter
restores the workspace's saved ratio for it. A frame never shrinks below its
panel's minimum size: 160 px wide and 80 px high for most panels, 320 x 180 for
the Viewer, and 400 x 120 for the Timeline. Project, Effect Controls, Effects &
Presets, Assistant, Audio Track Mixer, and Audio Meters are laid out at least
as tall as their fixed rows and a few lines of their lists (112 to 280 px). A
frame shorter than that scrolls the panel with the wheel.

The focused frame draws a 1 px accent outline. Keyboard shortcuts resolve
against the focused panel first, then the window (see
[keyboard.md](keyboard.md)). Shift+1 to Shift+9 focus a panel
([keyboard.md](keyboard.md#panels)); Ctrl+Tab and Ctrl+Shift+Tab move focus to
the next and previous frame.

## Workspaces

A workspace is a saved arrangement of frames, panels, ratios, and floating
windows. The shipped workspaces are:

| Workspace | Arrangement |
| --- | --- |
| Editing | The window diagram above. |
| Animation | Effect Controls and Project on the left, Viewer centre, Info, Effects & Presets, and Align on the right, Timeline at 50 % with the graph editor open. |
| Color | Viewer and Scopes side by side above the Timeline, Color panel on the right. |
| Audio | Audio Track Mixer above the Timeline, Viewer reduced, Audio Meters and Effects & Presets filtered to audio effects on the right. |
| Effects | Effects & Presets and Effect Controls enlarged, Viewer centre. |

The workspace switcher sits in the tool bar. Changing workspace rearranges the
frames over 180 ms; panel contents are not rebuilt. A workspace that does not
open the graph editor closes it. A workspace keeps
changes made to it. Window > Workspaces > Reset to Saved Layout restores the
saved arrangement, and Save as New Workspace stores the current arrangement
under a new name. Workspaces are stored in `<data dir>/workspaces/<name>.json`
and can be imported and exported.

Plugins contribute panels and workspaces (see [plugins.md](plugins.md)). A
plugin panel docks, floats, and tabs the same way a shipped panel does.

## Menu bar

| Menu | Contents |
| --- | --- |
| File | New Project, New Sequence, Open, Use Template, Open Recent (one row per recent project, then All Recent Projects), Close, Rename Project, Save, Save As, Save a Copy, Save as Template, Revert, Import, Import Recent (one row per recently imported file, then Clear Recent Imports), Import Timeline, Relink All, Export (Media, Frame, Timeline, then one row per timeline exporter: EDL, OTIO, FCPXML), Install Plugin, Project Settings, Quit |
| Edit | Undo, Redo, Cut, Copy, Paste, Paste Insert, Paste Attributes, Remove Attributes, Paste To Fit, Clear, Ripple Delete, Duplicate, Select All, Deselect All, Find, Label (one row per label colour, then None), Keyboard Shortcuts, Preferences |
| Clip | Rename, Make Subclip, Speed/Duration, Time Remapping, Enable, Link, Unlink, Group, Ungroup, Nest, Replace with Clip (From Source Monitor), Freeze Frame, Render and Replace, Restore Unrendered, Audio Gain, Audio Channels (Stereo, Swap Channels, Left to Both, Right to Both, Mono), Extract Audio, Transform (Flip Horizontal, Flip Vertical, Reset Framing), Track Matte (Luma, Alpha, Remove) |
| Sequence | Sequence Settings, Render In to Out, Render Selection, Delete Render Files, Add Edit, Add Edit to All Tracks, Trim Edit, Extend Edit, Apply Default Transitions, Lift, Extract, Snap, Linked Selection, Add Tracks (Video Track, Audio Track), Delete Empty Tracks, Add Adjustment Layer, Match Frame, Source (Insert, Overwrite, Insert Pool Selection), Ripple Trim In, Ripple Trim Out, Add Cross Fade at Cut, Remove Transition at Cut, lane shifts, Ducking, Nested Sequence |
| Markers | Mark In, Mark Out, Mark Clip, Go to In, Go to Out, Clear In, Clear Out, Clear In and Out, Add Marker, Go to Next Marker, Go to Previous Marker, Clear Marker, Clear All Markers, Edit Marker, Move Nearest Marker to Playhead |
| Effect | Effect Controls, Apply (the last applied effect), Paste Effect, Remove All Effects, Reset Effect Parameters, Reset Grade, Remove Mask, Remove Chroma Key, then one submenu per effect category |
| Animation | Add Keyframe, Toggle Hold Keyframe, Keyframe Interpolation (Linear, Bezier, Hold), Keyframe Velocity, Keyframe Assistant (Easy Ease, Easy Ease In, Easy Ease Out, Time-Reverse Keyframes), Add Expression, Reveal Properties with Keyframes, Reveal Modified Properties, Graph Editor |
| View | Zoom In, Zoom Out, Fit, Fit Selection, Resolution (Playback, Paused), Viewer Display Settings (Split Viewer, Show Rulers, Show Guides, Snap to Guides, Clear Guides, Title Safe, Action Safe, Transparency Grid, Grid), Proxies, Compare (Current Frame, Still, Source Tab, Vertical Wipe, Horizontal Wipe, Side by Side, Stop Comparing), Scopes, Scope Waveform (Luma, RGB Parade, YCbCr Parade), Fit Preview, Preview 100%, Canvas Handles, Fullscreen Preview, Loop Playback, Timeline Display Settings (Head Thumbnails, Head and Tail Thumbnails, Continuous Thumbnails, Logarithmic Waveforms, Linear Waveforms, Hide Waveforms, Page Scroll, Smooth Scroll) |
| Window | Workspaces (one row per workspace, then Save Workspace), then one item per panel, checked when open, Command Palette, Home, Project Report |
| Help | Keyboard Shortcuts, Plugin Development, Open Data Folder, Check for Updates, About |

A row with a parenthesised list is a submenu. Plugin menu rows follow the
shipped rows of their menu; a plugin row placed in a group sits in a submenu
named after the group ([plugins/menus.md](../plugins/menus.md)).

Every menu row is 22 px, in the menu bar and in a context menu. A 16 px lead
column holds a toggle's check mark while it is on, or a plugin row's icon, and
the caption starts one gap past it; the row's current chord is right-aligned.
A context menu row that switches a state, such as Enable or a lane height,
reads the same label in both states. A disabled item is drawn at 40 % opacity and states
why in its tooltip; chosen from the keyboard or the palette anyway, it runs
nothing and the reason goes to the status line, and a press on it leaves the
drop open. Pointing at a submenu entry or pressing it opens its flyout. A
press outside the bar and the open drop closes the drop and lands where it was
aimed. F10 or a lone Alt opens File with its first row lit. Up and Down move
the light, a letter lights the next row starting with it, Right opens a lit
submenu and Left closes it, Left and Right on
a plain row move to the neighbouring menu, Enter runs the lit row, and Escape
closes the drop. Opening a dialog, the command palette, or the unsaved-changes
prompt, or returning to Home, closes an open drop, flyout, or popover, so the
keys typed next reach the dialog. On macOS the bar is the native menu bar, with
the same submenus, check marks and disabled rows. After the menu titles, centred, the
bar shows `Home / ` and the project name, with a dot before the name while the
project has unsaved changes. Clicking Home returns to the Home screen.

The platform window title is the project name and `Aria`, with a leading `*`
while the project has unsaved changes, or `Aria` on the Home screen. A floating
panel window is titled with its panel's name.

On Linux the menu bar is the window's title bar when the compositor draws no
window decorations. Dragging an empty stretch of the bar either side of the
project name moves the window, double-clicking it maximizes or restores the
window, and right-clicking it opens the compositor's window menu. The bar ends
in minimize, maximize, and close buttons, each shown only when the compositor
supports it. A hairline edges every side the window is not maximized or tiled
against. A press within 5 px of such a side resizes the window from it, and a
press in a 12 px square at a corner between two such sides resizes from the
corner. The bands lie over the edge of the window's content, so the content
keeps the window's full size. A floating panel window moves from the empty
stretch of its tab row. Under a window manager
that draws its own frame, the bar draws no window buttons and the frame moves
and resizes the window. On Windows the empty stretches are the window caption,
with the system's drag, snap, and double-click behaviour.

Home has no menu bar. When the application draws the window's top edge (Linux
without server-side decorations, Windows, and macOS, whose traffic lights sit
over that edge), Home's first row is an empty bar: its stretch moves the window
as the menu bar's empty stretches do, and it ends in the window buttons the
menu bar shows. The rail under it shows the application's name. Under a window
manager's frame Home has no bar and starts at the window's top edge.

## Tool bar

Left to right: the tools ([editing.md](editing.md#tools)), a separator, the
edit-mode button, the workspace switcher centred, then the Export button on
the right. Snapping, linked selection, and Timeline Display Settings are in
the Timeline header ([timeline.md](timeline.md)), not in the tool bar.
The transport and the playhead timecode are in the Viewer's control
row ([viewer.md](viewer.md#anatomy)). Settings opens from Edit > Preferences
and Ctrl+,; panels open from the Window menu.

Each tool button is 28 x 28 px with a 16 px icon. Tools share a button by
group: Selection; Track Select Forward and Backward; the trim tools (Ripple
Edit, Rolling Edit, Rate Stretch, Slip, Slide); Razor; Pen and its vertex
variants; Hand and Zoom; Type. A group's button shows the active tool when it
belongs to the group, otherwise the group's first tool. The active tool has
the `element_active` plate and an accent icon. A group with more than one
entry shows a 3 px corner triangle; pressing and holding for 300 ms, or
clicking the triangle, opens its list. A tool's key selects it whether or not
its group's button shows it.

## Status bar

Left: the message line (the last action, an error in `danger`, a refusal
reason). Centre: background jobs with progress. Clicking a job opens the Jobs
panel. Right, during playback: the achieved frame rate against the sequence's,
the playback resolution, and the GPU device name. Once a frame drops, the
dropped-frame count shows in `warning` until it is clicked. At rest with no
dropped frame, the right side is empty.
