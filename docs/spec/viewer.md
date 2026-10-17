# Viewer

The Viewer panel shows the Program (the active sequence at the playhead) and
any number of Source tabs. Program is always the first tab and cannot be
closed.

## Opening a source

| Action | Result |
| --- | --- |
| Double-click a Project item | Opens it in a Source tab and focuses that tab. A second double-click on the same item focuses its existing tab. |
| Double-click a clip on the timeline | Opens the clip's source in a Source tab at the clip's source frame, with the clip's in and out shown. Edits to In and Out in that tab change the clip (a match-frame edit). |
| Shift+2 | Toggles between Program and the most recent Source tab. |
| Viewer menu > Split | Shows Source on the left and Program on the right in the same frame, each with its own transport. Split again returns to tabs. |

A Source tab has its own playhead, In, Out, markers, zoom, and resolution. It
keeps them while it stays open.

## Anatomy

```
+---------------------------------------------------------------+
| Program | Source: take.mov x   Split Rulers Safe Guides Alpha |  tab row
+---------------------------------------------------------------+
|                                                               |
|                  pasteboard (bg)                              |
|          +-------------------------------+                    |
|          |                               |                    |
|          |        frame                  |                    |
|          |                               |                    |
|          +-------------------------------+                    |
|                                                               |
+---------------------------------------------------------------+
| 00:00:12:04 | Fit | Full |  |< < [> > >|  Loop  In Out Scopes |  control row
+---------------------------------------------------------------+
| mini time ruler with In/Out range and markers                  |  12 px
+---------------------------------------------------------------+
```

The control row, left to right:

- Playhead timecode, scrubbable and editable. Typing `+12` moves 12 frames
  forward, `-1.00` moves one second back.
- Magnification: Fit, 10 % to 800 %. Ctrl+scroll zooms around the pointer;
  pressing the middle button pans; Shift+/ (`?`) fits.
- Resolution: Full, 1/2, 1/4, 1/8, Auto. The same list sets the paused
  resolution, Full or 1/2, under its own heading. View > Playback Resolution
  and View > Paused Resolution step through the same choices. See
  [playback.md](playback.md#resolution).
- Proxy: the Viewer reads proxies in place of the original files.
- Transport: go to start, step back, play/stop, step forward, go to end. Each
  acts on the tab in front of the Viewer. Holding step repeats once per frame
  of the tab in front, after the press-and-hold pause.
- Loop, and the shuttle speed while J or L runs above 1x.
- Mark In, Mark Out, while Program is in front. A Source tab marks its take,
  and inserts or overwrites with it, from its own bar under the picture.
- Handles: the set of on-canvas handles that is live.
- Scopes, and the output peak meter.

A Viewer too narrow for one line wraps the row between three groups, each kept
whole: the timecode through Proxy, the transport through Mark Out, and Handles
through the meter. An error after the meter is shortened with an ellipsis and
shown in full on hover.

The view toggles sit at the right end of the tab row: Split, Rulers, Safe
(title-safe and action-safe margins), Guides, and Alpha (the transparency
grid). The dropped-frame indicator is in the status bar.

The mini ruler under the control row spans the whole sequence or source. It
draws the In/Out range, markers, and a draggable playhead. A press puts the tab
in front on the frame under the pointer; the drag continues off the ruler, and
Escape puts the frame back.

Dragging the playhead timecode moves the playhead one frame per two pixels, ten
with Shift and a tenth of one with Ctrl. A click without a drag opens the field
for typing.

## Overlays

| Overlay | Drawn |
| --- | --- |
| Safe margins | Action safe at 93 %, title safe at 90 %, as 1 px lines. |
| Guides | Dragged out of the rulers (Ctrl+Shift+R shows rulers). Snapping to guides is a View toggle. |
| Transparency grid | 8 px checkerboard behind transparent pixels instead of black. |
| Dropped-frame indicator | A dot in the top left of the frame: green while every frame arrives on time, yellow with a count after a drop. Clicking it resets the count. |
| Missing media | A red slate naming the file and the clip. |
| Clip outline | The selected clips' bounds, anchor point, and rotation handle. |

## On-canvas controls

With a clip selected and the Selection tool active, the Viewer draws the clip's
transform bounds:

| Handle | Drag result | Modifiers |
| --- | --- | --- |
| Inside the bounds | Position | Shift locks to one axis |
| Corner square | Scale | Shift keeps aspect; Alt scales from the anchor |
| Edge square | Scale one axis | Shift keeps aspect |
| Outside a corner | Rotation | Shift steps 45 degrees |
| Anchor point (crosshair) | Moves the anchor with the Pan Behind tool (Y in Animation workspace) | Ctrl snaps to bounds |

Arrow keys nudge the selected clip by 1 px, Shift+arrow by 10 px. Each drag or
nudge burst is one undo step. When the property being dragged has keyframes,
the drag sets or adds a keyframe at the playhead, the same as editing the value
in the property row.

Effect parameters of type point, rectangle, and path draw their own handles in
the Viewer while the effect is selected in Effect Controls. Masks draw their
path with vertex and tangent handles, editable with the Pen tool. A keyframed
path draws as it stands at the playhead; moving a vertex or a tangent keys the
path there, and inserting or deleting a vertex changes every Path keyframe.

A position property with keyframes draws its motion path: one dot per frame,
spaced by speed, with the keyframes as squares and their spatial tangents as
handles.

A plugin's viewer overlay ([plugins.md](plugins.md#contribution-points)) draws
lines, rectangles, ellipses, paths, text, and handles over the sequence frame,
placed in canvas pixels through the same fit, zoom, and pan as the frame.
Strokes and text keep their screen size. Dragging an overlay handle sends the
press, each step, and the release to the plugin, which returns the scene drawn
next.

## Comparison

Viewer menu > Compare shows the Program frame split with a reference: another
frame of the sequence, a still, or a Source tab. The divider is draggable and
switches between vertical, horizontal, and side by side.
