# Layout

## Window

```
+------------------------------------------------------------------------------+
| Menu bar                                                          28 px      |
| Tool bar                                                          36 px      |
+----------------------+--------------------------------+----------------------+
| Project | Effect     | Program | Source               | Info                 |
| Controls             |                                | Effects & Presets    |
|                      |           Viewer               | Character            |
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

## Frames and panels

A frame holds one or more panels. Each panel has a tab. The tab row is 24 px
high and shows the panel name, a close cross on hover, and a panel menu
button at the right end of the frame's tab row. Tabs wider than the frame
narrow together and end their names with an ellipsis; the close cross and the
panel menu button keep their width.

| Gesture | Result |
| --- | --- |
| Click a tab | That panel becomes the frame's visible panel. |
| Drag a tab inside its row | Reorders the tabs. |
| Drag a tab over another frame | Shows drop zones: centre (add as tab), top, bottom, left, right (split the frame). Releasing on a zone docks the panel there. |
| Drag a tab outside the window | Opens the panel in a floating window. |
| Drag a floating window's tab back over a frame | Docks it, with the same drop zones. |
| Grave accent (`` ` ``) | Maximizes the frame under the pointer to the whole area between the tool bar and the status bar. Pressed again, restores it. |
| Double-click a tab | Same as grave accent. |
| Middle-click a tab | Closes the panel. |
| Ctrl+W on a focused panel | Closes the panel, or the sequence tab when the Timeline has focus. |

A drop zone is a translucent accent rectangle covering the half or the centre
of the target frame. It appears 80 ms after the pointer enters the frame and
follows the pointer between zones with a 120 ms fade.

Splitters between frames are 1 px drawn and 6 px hit. Hovering a splitter
shows the resize cursor and brightens the line to the accent colour.
Double-clicking a splitter restores the workspace's saved ratio for it. A frame
never shrinks below its panel's minimum size: 160 px wide and 80 px high for
most panels, 320 x 180 for the Viewer, and 400 x 120 for the Timeline.

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
frames over 180 ms; panel contents are not rebuilt. A workspace remembers
changes made to it. Window > Workspaces > Reset to Saved Layout restores the
saved arrangement, and Save as New Workspace stores the current arrangement
under a new name. Workspaces are stored in `<data dir>/workspaces/<name>.json`
and can be imported and exported.

Plugins contribute panels and workspaces (see [plugins.md](plugins.md)). A
plugin panel docks, floats, and tabs the same way a shipped panel does.

## Menu bar

| Menu | Contents |
| --- | --- |
| File | New Project, New Sequence, Open, Use Template, Open Recent (one row per remembered project, then All Recent Projects), Close, Rename Project, Save, Save As, Save a Copy, Save as Template, Revert, Import, Import Recent (one row per recently imported file, then Clear Recent Imports), Import Timeline, Relink All, Export (Media, Frame, Timeline, then one row per timeline exporter: EDL, OTIO, FCPXML), Install Plugin, Project Settings, Quit |
| Edit | Undo, Redo, Cut, Copy, Paste, Paste Insert, Paste Attributes, Remove Attributes, Paste To Fit, Clear, Ripple Delete, Duplicate, Select All, Deselect All, Find, Label (one row per label colour, then None), Keyboard Shortcuts, Preferences |
| Clip | Rename, Make Subclip, Speed/Duration, Time Remapping, Enable, Link, Unlink, Group, Ungroup, Nest, Replace with Clip (From Source Monitor), Freeze Frame, Render and Replace, Restore Unrendered, Audio Gain, Audio Channels (Stereo, Swap Channels, Left to Both, Right to Both, Mono), Extract Audio, Transform (Flip Horizontal, Flip Vertical, Reset Framing), Track Matte (Luma, Alpha, Remove) |
| Sequence | Sequence Settings, Render In to Out, Render Selection, Delete Render Files, Add Edit, Add Edit to All Tracks, Trim Edit, Extend Edit, Apply Default Transitions, Lift, Extract, Snap, Linked Selection, Add Tracks (Video Track, Audio Track), Delete Empty Tracks, Add Adjustment Layer, Match Frame, Source (Insert, Overwrite, Insert Pool Selection), Ripple Trim In, Ripple Trim Out, Add Cross Fade at Cut, Remove Transition at Cut, lane shifts, Ducking, Nested Sequence |
| Markers | Mark In, Mark Out, Mark Clip, Go to In, Go to Out, Clear In, Clear Out, Clear In and Out, Add Marker, Go to Next Marker, Go to Previous Marker, Clear Marker, Clear All Markers, Edit Marker, Move Nearest Marker to Playhead |
| Effect | Effect Controls, Apply (the last applied effect), Paste Effect, Remove All Effects, Reset Effect Parameters, Reset Grade, Remove Mask, Remove Chroma Key, then one submenu per effect category |
| Animation | Add Keyframe, Toggle Hold Keyframe, Keyframe Interpolation (Linear, Bezier, Hold), Keyframe Velocity, Keyframe Assistant (Easy Ease, Easy Ease In, Easy Ease Out, Time-Reverse Keyframes), Add Expression, Reveal Properties with Keyframes, Reveal Modified Properties, Graph Editor |
| View | Zoom In, Zoom Out, Fit, Fit Selection, Resolution (Playback, Paused), Show Rulers, Show Guides, Clear Guides, Safe Margins (Title Safe, Action Safe), Snap to Guides, Transparency Grid, Proxies, Grid, Split Viewer, Compare (Current Frame, Still, Source Tab, Vertical Wipe, Horizontal Wipe, Side by Side, Stop Comparing), Scopes, Scope Waveform (Luma, RGB Parade, YCbCr Parade), Fit Preview, Preview 100%, Canvas Handles, Fullscreen Preview, Loop Playback, Timeline Display Settings (Head Thumbnails, Head and Tail Thumbnails, Continuous Thumbnails, Logarithmic Waveforms, Linear Waveforms, Hide Waveforms, Page Scroll, Smooth Scroll) |
| Window | Workspaces (one row per workspace, then Save Workspace), then one item per panel, checked when open, Command Palette, Home, Project Report |
| Help | Keyboard Shortcuts, Plugin Development, Open Data Folder, Check for Updates, About |

A row with a parenthesised list is a submenu. Plugin menu rows follow the
shipped rows of their menu; a plugin row placed in a group sits in a submenu
named after the group ([plugins/menus.md](../plugins/menus.md)).

Every menu item shows its current chord, right-aligned, and a toggle shows a
check mark while it is on. A disabled item is drawn at 40 % opacity and states
why in its tooltip; chosen from the keyboard or the palette anyway, it runs
nothing and the reason goes to the status line, and a press on it leaves the
drop open. Pointing at a submenu entry or pressing it opens its flyout. A
press outside the bar and the open drop closes the drop and lands where it was
aimed. F10 or a lone Alt opens File with its first row lit. Up and Down move
the light, a letter lights the next row starting with it, Right opens a lit
submenu and Left closes it, Left and Right on
a plain row move to the neighbouring menu, Enter runs the lit row, and Escape
closes the drop. On macOS the bar is the native menu bar, with the same
submenus, check marks and disabled rows. After the menu titles, centred, the
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

## Tool bar

Left to right: the tools ([editing.md](editing.md#tools)), a separator, the
snapping and linked-selection toggles, the Timeline Display Settings button,
the edit-mode button, the workspace switcher centred, then the Export button on
the right. The Timeline Display Settings button opens the View menu's Timeline
Display Settings choices under itself; a second press, a choice, Escape, or a
press elsewhere closes them.
The transport and the playhead timecode are in the Viewer's control
row ([viewer.md](viewer.md#anatomy)). Settings opens from Edit > Preferences
and Ctrl+,; panels open from the Window menu.

Each tool button is 28 x 28 px with a 16 px icon. The active tool has the
`element_active` plate and an accent icon. A tool with variants (the trim
tools, the pen tools) shows a 3 px corner triangle; pressing and holding for
300 ms, or clicking the triangle, opens its variants.

## Status bar

Left: the message line (the last action, an error in `danger`, a refusal
reason). Centre: background jobs with progress. Right: the dropped-frame
indicator, the playback resolution, and the GPU device name. Clicking a job
opens the Jobs panel.
