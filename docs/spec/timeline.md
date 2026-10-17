# Timeline

The Timeline panel shows one tab per open sequence. Tracks hold many clips
each, as in Premiere Pro. Any clip opens into property rows under its track,
as a layer does in After Effects, so cutting and animating happen in one panel.

## Anatomy

```
+-------------------------------+---------------------------------------------+
| 00:00:12:04          [search] | ruler  :00f   :12f   01:00f  01:12f        |  header
| 292 frames    [snap][link][M] | work area  [=========]                       |  42 px
|  [wrench] [graph]             | render bar ====yellow===red===green          |
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

## Header

- Sequence timecode, 18 px, scrubbable and editable like the Viewer's. The frame
  count is shown under it.
- Property search: typing filters the property rows of every opened clip to
  properties whose names match, and opens the groups that contain them.
- Toggles: snapping (S), linked selection, add marker (M), timeline display
  settings, and graph editor (Shift+F3).
- Ruler: ticks adapt to zoom so labels never overlap. Labels read in the
  sequence's display format (timecode, frames, or feet+frames). Clicking or
  dragging in the ruler moves the playhead; the playhead follows the pointer
  without easing.
- Work area: the In to Out range, drawn as a bar under the ruler. Dragging the
  bar moves both points; dragging an end moves one; double-clicking it sets it
  to the sequence length.
- Render bar: one colour per frame range. Green: rendered or cached at the
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
| Fill | The clip's label colour. Selected clips use the label's selected fill and a 1 px white border. |
| Name strip | Clip name, then the speed when it is not 100 % (`120%`, `-100%` reversed), then badges. |
| fx badge | Grey: no effects. Yellow: effects applied. Purple: intrinsic properties changed. Yellow with purple underline: both. Clicking it opens a menu of the clip's effects and properties. |
| Content strip | Video: thumbnails, per the display setting (head, head and tail, continuous). Audio: rectified waveform, logarithmic by default. Generators: their name and a solid swatch. |
| Volume line | The clip volume as a horizontal line with keyframe dots. Dragging the line changes the volume; Ctrl+click adds a keyframe. |
| Keyframe dots | Shown when the track is Medium or taller and the clip has keyframes on the property chosen in the clip's badge menu. |
| Transition | A bar over the cut with its name, aligned to the cut (centre, start, or end). Dragging its edge changes its length. |
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
  keyframe. The centre diamond is filled when a keyframe exists at the playhead.
- Reset: returns the property to its default (a keyframe when animated).

The time area of a property row draws its keyframes. In graph editor mode the
time area of the opened clip shows the value or speed graph of the selected
properties instead (see [animation.md](animation.md#graph-editor)).

A path mask's group starts with its Path row (`Mask N Path`). A Path keyframe
holds every vertex of the path, so the row has a stopwatch, the vertex count
at the playhead in place of a value, and the navigator, and no driver, reset,
or graph. The stopwatch and the navigator work as on a property row. A Path
keyframe drags in time within the clip, snapping to the playhead and the path's
other keyframes unless Alt is held; a click on one moves the playhead to it.
Its right-click menu sets the easing of the segment leaving it (Linear, Ease In
and Out, Ease In, Ease Out, Hold or Continuous) or deletes it. Deleting the last
Path keyframe keeps its path. Each press is one `SetClipMask` edit. Alt+M opens
the Path rows with the mask rows, the first Alt+U opens the Path rows of
keyframed paths, and the second opens every Path row.

## Navigation

| Input | Result |
| --- | --- |
| Scroll | Vertical scroll of the tracks |
| Shift+scroll | Horizontal scroll |
| Alt+scroll, Ctrl+scroll | Zoom around the pointer |
| `=` / `-` | Zoom in or out around the playhead |
| `\` | Fit the sequence |
| Shift+F | Fit the selection |
| Zoom scroll bar | Dragging the bar scrolls; dragging its ends zooms |
| Middle-button drag | Pan |
| Up / Down | Playhead to the previous or next edit on targeted tracks |
| Shift+Up / Shift+Down | Same on all tracks |

During playback the view follows the playhead by pages (Premiere's page scroll)
or smoothly, per the timeline display setting. A scroll by hand during
playback stops following until the playhead next leaves the view.
