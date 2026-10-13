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
button at the right end of the frame's tab row.

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
| File | New Project, New Sequence, Open, Open Recent, Close, Save, Save As, Save a Copy, Revert, Import, Import Recent, Export (Media, Frame, EDL, OTIO), Project Settings, Quit |
| Edit | Undo, Redo, Cut, Copy, Paste, Paste Insert, Paste Attributes, Remove Attributes, Clear, Ripple Delete, Duplicate, Select All, Deselect All, Find, Label, Keyboard Shortcuts, Preferences |
| Clip | Rename, Make Subclip, Speed/Duration, Enable, Link, Unlink, Group, Ungroup, Nest, Replace with Clip, Freeze Frame, Render and Replace, Restore Unrendered, Audio Gain, Audio Channels |
| Sequence | Sequence Settings, Render In to Out, Render Selection, Delete Render Files, Add Edit, Add Edit to All Tracks, Trim Edit, Extend Edit, Apply Default Transitions, Lift, Extract, Snap, Linked Selection, Add Tracks, Delete Tracks |
| Markers | Mark In, Mark Out, Mark Clip, Go to In, Go to Out, Clear In and Out, Add Marker, Go to Next Marker, Go to Previous Marker, Clear Marker, Clear All Markers, Edit Marker |
| Effect | Effect Controls, the last applied effect, Remove All, then one submenu per effect category |
| Animation | Add Keyframe, Toggle Hold Keyframe, Keyframe Interpolation, Keyframe Velocity, Keyframe Assistant (Easy Ease, Easy Ease In, Easy Ease Out, Time-Reverse Keyframes), Add Expression, Reveal Properties with Keyframes, Reveal Modified Properties |
| View | Zoom In, Zoom Out, Fit, Resolution, Show Rulers, Show Guides, Safe Margins, Transparency Grid, Snap to Guides, Proxies, Timeline Display Settings |
| Window | Workspaces, then one item per panel, checked when open |
| Help | Keyboard Shortcuts, Plugin Development, Open Data Folder, About |

Every menu item shows its current chord, right-aligned. A disabled item is
drawn at 40 % opacity and states why in its tooltip. On macOS the bar is the
native menu bar.

## Tool bar

Left to right: the tools ([editing.md](editing.md#tools)), a separator, the
snapping and linked-selection toggles, the workspace switcher centred, then on
the right the Export button and the project name with its unsaved-changes dot.

Each tool button is 28 x 28 px with a 16 px icon. The active tool has the
`element_active` plate and an accent icon. A tool with variants (the trim
tools, the pen tools) shows a 3 px corner triangle; pressing and holding for
300 ms, or clicking the triangle, opens its variants.

## Status bar

Left: the message line (the last action, an error in `danger`, a refusal
reason). Centre: background jobs with progress. Right: the dropped-frame
indicator, the playback resolution, and the GPU device name. Clicking a job
opens the Jobs panel.
