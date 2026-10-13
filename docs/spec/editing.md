# Editing

## Tools

| Tool | Key | On a clip body | On a clip edge | On empty track |
| --- | --- | --- | --- | --- |
| Selection | V | Select, drag to move | Trim (overwrite) | Marquee select |
| Track Select Forward | A | Select this and every later clip on all tracks | - | Same from the pointer |
| Track Select Backward | Shift+A | Select this and every earlier clip | - | Same |
| Ripple Edit | B | - | Trim and shift later clips | - |
| Rolling Edit | N | - | Move the cut between two clips | - |
| Rate Stretch | R | - | Change speed to fit the new length | - |
| Razor | C | Split at the pointer; Shift splits all tracks | - | - |
| Slip | Y | Shift the source under fixed edges | - | - |
| Slide | U | Move the clip, trimming its neighbours | - | - |
| Pen | P | Add volume or opacity keyframes on the clip's line; in the Viewer, draw masks | - | - |
| Hand | H | Pan | - | Pan |
| Zoom | Z | Zoom in; Alt zooms out | - | Same |
| Type | T | In the Viewer, create a text clip at the click | - | - |

Holding a tool's key switches to it while the key is down and returns to the
previous tool on release. With the Selection tool, Ctrl over an edge switches
it to Ripple Edit, and Ctrl+Alt to Rolling Edit, while the modifiers are held.

The cursor names the edit a drag will make before the drag starts: trim
bracket, ripple bracket, roll bracket, slip, slide, rate, or razor.

## Selection

| Input | Result |
| --- | --- |
| Click | Selects the clip and its linked clips. |
| Alt+click | Selects the clip without its links. |
| Shift+click | Adds or removes the clip. |
| Drag on empty track area | Marquee; Shift adds. |
| Ctrl+A / Ctrl+Shift+A | Select all / deselect all. |
| Click an edit point | Selects the edit (the edge). Shift adds edges on other tracks. |
| D | Selects the clip at the playhead on the targeted tracks. |

Selected edges draw a 3 px accent bracket. With edges selected, Ctrl+T applies
the default transition, Ctrl+Left / Ctrl+Right trim them by one frame, and
Ctrl+Shift+Left / Ctrl+Shift+Right by five frames.

## Moving and trimming

- A drag shows a tooltip with the offset (`+00:00:01:04`) and, for a trim, the
  new duration. Edges in the Viewer show the outgoing and incoming frames
  side by side during a trim (two-up display).
- A move drops in overwrite mode. Ctrl while releasing drops in insert mode and
  pushes later clips right on every track with sync lock on.
- A trim cannot extend past the source's media; the edge stops and turns red at
  the limit.
- Alt+drag duplicates.
- Dragging vertically moves a clip to another track of the same kind. Shift
  restricts the move to vertical.
- Numeric entry: with clips selected, typing `+` or `-` on the numeric keypad
  and a duration moves them; with an edge selected it trims.

Every drag is one undo step with a name in Edit > Undo ("Undo Ripple Trim").

## Gaps and deletion

| Command | Key | Result |
| --- | --- | --- |
| Clear | Delete | Removes the selection, leaves the gap |
| Ripple Delete | Shift+Delete | Removes the selection, closes the gap on sync-locked tracks |
| Select a gap | Click it | The gap highlights; Delete closes it |
| Lift | `;` | Removes the In to Out range on targeted tracks, leaves the gap |
| Extract | `'` | Removes the In to Out range on targeted tracks, closes the gap |
| Ripple trim to playhead | Q / W | Trims the clip's head / tail to the playhead and closes the gap |
| Trim to playhead | Alt+[ / Alt+] | Same without closing the gap |
| Add Edit | Ctrl+K | Splits targeted tracks at the playhead |
| Add Edit to All Tracks | Ctrl+Shift+K | Splits every unlocked track at the playhead |

## Three-point editing

A Source tab holds In and Out on the source. The sequence holds In and Out on
the timeline. Any three of the four points define an edit; the fourth comes
from the source's length or the gap.

| Command | Key | Result |
| --- | --- | --- |
| Insert | `,` | Places the source range at the sequence In (or the playhead) on the patched tracks and pushes later clips right on sync-locked tracks |
| Overwrite | `.` | Places the source range over what is there |
| Replace | Clip > Replace with Clip > From Source | Replaces the clip under the playhead, aligning the Source playhead frame with the sequence playhead frame |
| Fit to Fill | With four points set | Retimes the source range to the sequence range and shows the new speed |

Source patching (the V and A indicators in the track headers) chooses which
tracks receive the source's video and audio. Dragging a Source tab's frame onto
the timeline drops it with the same patching, in overwrite mode, Ctrl for
insert.

## Snapping

Snapping (S) snaps a moving edge, clip, playhead, or keyframe to: clip edges,
the playhead, In and Out, markers, keyframes on visible property rows, beat
markers, and the work area. The snap distance is 8 px. A snap draws a vertical
`warning` line through every track at the snap point while it holds. Holding
Ctrl while dragging inverts snapping for that drag.

## Linking and grouping

- Linked clips (video with its audio) select, move, and trim together while
  linked selection is on. Ctrl+L links or unlinks the selection.
- Groups (Ctrl+G) select and move together but trim independently.
- Out-of-sync linked clips show their offset on both bars. Right-click the
  offset > Move into Sync or Slip into Sync restores sync.

## Nesting

Clip > Nest replaces the selection with one clip whose source is a new sequence
holding the selection. Double-clicking a nested clip opens its sequence in a new
Timeline tab. Edits inside it appear in every instance. A nested sequence can
be marked as a composition, which opens with one clip per track and the
Animation workspace.

## Markers

M adds a marker at the playhead, on the selected clip if one is selected and
the playhead is over it, otherwise on the sequence. Pressing M twice opens the
marker dialog: name, comment, colour, duration, type (comment, chapter,
segmentation). Markers with a duration draw as a range. Shift+M / Ctrl+Shift+M
go to the next / previous marker. The Markers panel lists every marker with its
time, name, and comment, and filters by colour.
