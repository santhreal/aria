# Timeline

The Timeline panel shows one tab per open sequence. Tracks hold many clips
each, as in Premiere Pro. Any clip opens into property rows under its track,
as a layer does in After Effects, so cutting and animating happen in one panel.

## Anatomy

```
+-------------------------------+---------------------------------------------+
| 00:00:12:04          [search] | ruler  :00f   :12f   01:00f  01:12f        |  header
|               [S][L][M][D][G] | work area  [=========]                       |  49 px
|                               | render bar ====yellow===red===green          |
+-------------------------------+---------------------------------------------+
| V3 | Titles    | eye lock sync | [Title 01      ]                             |
| V2 | Graphics  | eye lock sync |        [logo.png  fx]                       |
|>V1 | Video 1   | eye lock sync | [interview_02.mov fx][b-roll_04 120%]         |
|     v interview_02.mov        |  property rows for the opened clip          |
|       v Transform            |                                             |
|         Position  960, 540  o |    <>              <>                       |
|         Scale     100 %     o |         <>                                   |
|         Opacity   100 %       |                                             |
|       > Effects (2)           |                                             |
|       > Masks (1)             |                                             |
|       > Audio                 |                                             |
+-------------------------------+---------------------------------------------+
|>A1 | Audio 1   | M S lock sync | [~~~~waveform~~~~][~~~~~~]                   |
| A2 | Music     | M S lock sync | [~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~]             |
| Mix|           | meter         |                                             |
+-------------------------------+---------------------------------------------+
| zoom scroll bar with handles                                                 |
+------------------------------------------------------------------------------+
```

The left part is the outline column, 300 px wide by default and resizable. The
right part is the time area. Video tracks are above the divider and number
upward from V1; audio tracks are below it and number downward from A1. The
divider between them is draggable and sets how much height each side gets.
Tracks that take less height than their side sit against the divider: the
spare height is above the top video track and below the last audio track. A
media drag over the spare height above the video tracks lands on the top one.
A side with no tracks is one 22 px strip labeled "No video tracks" or "No
audio tracks", and the divider above or below it does not drag.

A Timeline narrower than the outline column plus 160 px draws the outline
column narrower, down to its 240 px minimum, so the time area keeps 160 px.
The saved width is unchanged and returns when the panel widens.

## Header

The header over the outline column is a grid of two 24 px rows and two
columns, 49 px tall with its bottom border, the height of the ruler beside it.

| Row | Left column | Right column, 120 px |
| --- | --- | --- |
| 1 | Sequence timecode, centered on both rows | Property search |
| 2 | | Toggles |

- Sequence timecode: 18 px, or 14 px when the outline column is narrower than
  272 px. It is scrubbable and editable like the Viewer's. At rest it is drawn
  as figures; the field border and fill fade in under the pointer, and the
  accent border and caret show while it is open. A press anywhere else closes
  it without applying the typed value. Its first digit stands on the column's
  text edge.
- Property search: typing filters the property rows of every opened clip to
  properties whose names match, and opens the groups that contain them. The
  field and the toggles under it share both edges.
- Toggles, 24 px each: snapping (S), linked selection, add marker (M),
  timeline display settings, and graph editor (Shift+F3). A lit toggle holds
  the pressed plate and an accent icon, as the tool bar's tools do. The
  window draws these toggles here only. The timeline display settings button
  opens the View menu's Timeline Display Settings choices under itself; a
  second press, a choice, Escape, or a press elsewhere closes them.
- Ruler: ticks adapt to zoom so labels never overlap. Labels read in the
  sequence's display format (timecode, frames, or feet+frames). Clicking or
  dragging in the ruler moves the playhead; the playhead follows the pointer
  without easing.
- Markers: a point marker is a flag 6 px wide and a range marker a bar across
  its duration, both 10 px tall and standing on the bottom edge of the ticks,
  below the labels. Clicking a marker moves the playhead to it.
- Work area: the In to Out range, drawn as a bar under the ruler. Dragging the
  bar moves both points; dragging an end moves one; double-clicking it sets it
  to the sequence length.
- Render bar: one color per frame range. Green: rendered or cached at the
  current resolution. Yellow: expected to play at the sequence rate without a
  render. Red: expected to drop frames. None: empty.

## Track headers

| Column | Video track | Audio track |
| --- | --- | --- |
| Source patch | V source indicator: click to patch a Source tab's video to this track, click the lit one to leave video out | A source indicator, for the audio |
| Target | Track targeted for paste, match frame, and up/down navigation | Same |
| Name | Double-click to rename | Same |
| Toggles | Visibility (eye), lock, sync lock | Mute (M), solo (S), lock, sync lock |
| Height | Drag the bottom edge; Ctrl+= and Ctrl+- resize all tracks | Same; tall audio tracks show a level meter and a volume knob |

A track header is one row of 20 px controls: the source indicator, the
track badge, the name, then the toggles. Mute and solo show at all times on
audio tracks. Every other toggle shows while the pointer is over the header,
and at all times while its track departs from a new track: collapsed,
locked, hidden, sync lock off, muted or soloed. Target shows at all times
while the track is targeted. A hidden toggle keeps its place, so no control
moves when the pointer enters the header. A lit toggle is drawn in the accent
color.

Right-click on a header offers Add Track, Delete Track, Rename, Duplicate,
Collapse, and track output routing for audio. Tracks are reordered by dragging
the header.

Track heights: Mini (20 px), Small (32 px), Medium (48 px, default), Large
(72 px). Shift+scroll over the headers changes the height of the track under
the pointer.

The header column, with the property rows under each track, is 300 px wide by
default. Dragging its right border sets the width between 240 px and 480 px;
Escape during the drag restores the width it started from. The width is saved
with the UI settings.

## Clip bars

A clip bar shows, from the top: a 14 px name strip, the content strip, and on
audio clips the volume line.

| Element | Detail |
| --- | --- |
| Fill | The clip's label color. Selected clips use the label's selected fill and a 1 px white border. |
| Name strip | Clip name, then the speed when it is not 100 % (`120%`, `-100%` reversed), then badges. |
| fx badge | Gray: no effects. Yellow: effects applied. Purple: intrinsic properties changed. Yellow with purple underline: both. Clicking it opens a menu of the clip's effects and properties. |
| Content strip | Video: thumbnails, per the display setting (head, head and tail, continuous). Audio: rectified waveform, logarithmic by default. Generators: their name and a solid swatch. |
| Volume line | The clip volume as a horizontal line with keyframe dots. Dragging the line up or down changes the volume, and a drag from it along the lane moves the clip; Ctrl+click adds a keyframe. |
| Keyframe dots | Shown when the track is Medium or taller and the clip has keyframes on the property chosen in the clip's badge menu. |
| Transition | A bar over the cut with its name, aligned to the cut (center, start, or end). Dragging its edge changes its length. |
| Offline | Red fill with "Media Offline" and the file name. |
| Disabled | Clip drawn at 40 % opacity and excluded from render. |
| Linked | Video and audio of one source move, trim, and select together while linked selection is on. A link that has drifted out of sync shows the offset in frames on both bars (`+3`). |

Bars draw as one painted element per track, not as one widget per clip. A
sequence with 10,000 clips scrolls and zooms at the display's refresh rate.

## Property rows

A clip opens into property rows when its twirl arrow is clicked, when a reveal
chord is pressed with it selected (see [keyboard.md](keyboard.md#properties)),
or when a property is changed from Effect Controls with "Reveal in timeline"
on. One clip per track can be open at a time; opening another clip on the same
track closes the first.

Rows are drawn under the track and span the full time area. Inside the opened
clip's range the row background is `surface`; outside it the row is
shaded, since keyframes cannot exist there.

| Group | Rows |
| --- | --- |
| Transform | Anchor Point, Position, Scale (linked or per axis), Rotation, Opacity, Skew, Skew Axis |
| Time | Speed, Time Remap |
| Effects | One group per effect in stack order, each with its parameters |
| Masks | One group per mask: Path, Feather, Opacity, Expansion, mode, inverted |
| Audio | Volume, Pan, and one group per audio effect |
| Text | Source Text and the animatable text properties, on text clips |

Each property row shows in the outline column:

```
[stopwatch] Name ............. value  [<] [<>] [>]  [reset]
```

- Stopwatch: off means the property is constant. Turning it on makes a
  keyframe at the playhead with the current value. Turning it off deletes all
  keyframes of that property and keeps the value at the playhead (one undo
  step). Alt+click adds a driver (see [animation.md](animation.md#drivers)).
- Value: scrubbable; per-axis values are separate fields. Editing a value on an
  animated property sets or adds a keyframe at the playhead.
- Navigator: previous keyframe, add or remove keyframe at the playhead, next
  keyframe. The center diamond is filled when a keyframe exists at the playhead.
- Reset: returns the property to its default (a keyframe when animated).

The time area of a property row draws its keyframes. In graph editor mode the
time area of the opened clip shows the value or speed graph of the selected
properties instead (see [animation.md](animation.md#graph-editor)).

The graph editor band is 168 px tall. Its header column holds the property's
name on the first 24 px row and the graph tools on the second: the Value and
Speed chips, then Easy Ease and Hold. The value axis labels stand on the
column's right edge, in a 40 px gutter, at the top, middle and bottom values'
ys, and the graph draws its three grid lines at the same ys.

A path mask's group starts with its Path row (`Mask N Path`). A Path keyframe
holds every vertex of the path, so the row has a stopwatch, the vertex count
at the playhead in place of a value, and the navigator, and no driver, reset,
or graph. The stopwatch and the navigator work as on a property row. A Path
keyframe drags in time within the clip, snapping to the playhead and the path's
other keyframes unless Alt is held; a click on one moves the playhead to it.
Its right-click menu sets the easing of the segment leaving it (Linear, Easy
Ease, Easy Ease In, Easy Ease Out, or Hold, which a second press returns to
Linear) or deletes it. Deleting the last
Path keyframe keeps its path. Each press is one `SetClipMask` edit. Alt+M opens
the Path rows with the mask rows, the first Alt+U opens the Path rows of
keyframed paths, and the second opens every Path row.

## Navigation

| Input | Result |
| --- | --- |
| Scroll | Vertical scroll of the tracks |
| Shift+scroll, trackpad swipe | Horizontal scroll; down or a swipe left shows later time |
| Alt+scroll, Ctrl+scroll | Zoom around the pointer; up zooms in |
| `=` / `-` | Zoom in or out around the playhead; a playhead off screen moves to the middle of the view |
| `\` | Fit the sequence |
| Shift+F | Fit the selection |
| Zoom scroll bar | Dragging the bar scrolls; dragging its ends zooms; scrolling over it zooms around the playhead |
| Middle-button drag | Pan |
| Up / Down | Playhead to the previous or next edit on targeted tracks |
| Shift+Up / Shift+Down | Same on all tracks |

Zooming out stops where the whole sequence fills the view, or at a tenth of the
base scale when the sequence is shorter than that view. A zoom around a point
inside the sequence does not scroll the sequence's start out of a view that has
room past its end: the point moves right instead, and stays on screen.

During playback the view follows the playhead by pages (Premiere's page scroll)
or smoothly, per the timeline display setting. A scroll by hand during
playback stops following until the playhead next leaves the view.
