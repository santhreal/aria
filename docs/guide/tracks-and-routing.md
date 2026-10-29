# Tracks and routing

## Tracks

Each track header holds a collapse chevron and six toggles: mute, solo (`S`),
lock, visibility, target (`T`), and sync lock (`⇄`). Drag the right border of
the header column to widen or narrow it.

Solo is per kind. While any audio track is soloed, the other audio tracks are
silent in both preview and export; the video tracks are unaffected. While any
video track is soloed, the other video tracks drop out of the composite. Solo
does not override a track's own mute: a muted track stays silent while soloed.

Right-click a track header for the rest:

- **Move Track Up** / **Move Track Down** move the track one row, which is one
  layer in the composite. The main track stays at the bottom of the stack.
- **Duck Source** marks an audio track as the voice that auto-duck listens to,
  and draws a check mark while the track is the voice. Only audio tracks can
  be the source.
- **Open Gap at Playhead** shifts every clip on that track starting at or after
  the playhead one second later. **Close Gap at Playhead** shifts them back. A
  shift that would collide with another clip, or move one before the start of the
  sequence, is refused.
- **Collapse Lane** / **Expand Lane** does what the chevron does.
- **Hide Waveforms** / **Show Waveforms** and **Hide Filmstrips** / **Show
  Filmstrips** turn that lane's strips on or off.
- **Lane Height: Mini**, **Small**, **Medium**, and **Large** set the lane to
  20, 32, 48, and 72 pixels. The lane's current height draws a check mark; a
  lane at any other height draws none.

The palette's **Move Track Up** (`pal-track-up`), **Move Track Down**
(`pal-track-down`), **Solo Track** (`pal-track-solo`), **Open One-Second Gap at
Playhead** (`pal-track-open-gap`), and **Close One-Second Gap at Playhead**
(`pal-track-close-gap`) rows act on the track that holds the selected clip.

Drag a track header onto another header to move that lane to that header's
row. The row is marked while the pointer is over its header, and the release
drops the lane on the marked row wherever the pointer is; over the lane's own
header no row is marked and the release moves nothing. Escape abandons the
drag, and a drop is one edit. The main track stays at the bottom of the stack.

Drag a lane's lower border to resize it, or Shift+scroll over its header to step
through the Mini, Small, Medium, and Large presets. Heights below 20 or above
200 pixels are clamped. The drag is one edit, so one Undo restores the previous
height, and
Escape abandons it. Lane height, the collapse state, and the two strip options
are stored with the sequence: reopening the project gives back the layout you
left.

The chevron collapses a lane to a 20-pixel strip: each clip is drawn as a flat
bar in its own color, so the cut pattern stays readable while the lane takes
almost no room. The height is kept, so expanding restores the lane you sized.

**Shift Selection…**, in a clip's context menu, or **Shift Selection by Frames…** (`pal-shift-selection`) in the command palette, prompts for a
signed frame count. The offset moves the selection on every lane it spans, from
the earliest selected clip on each, shifting everything later on those lanes with
it. A leading `+` is accepted. An offset that would move a clip before the start
of the sequence is refused whole, and anything that is not a whole number of
frames is reported rather than guessed at.

Dropping media on the timeline follows the edit mode, which the chip after the
tool bar's tools shows as **Ripple** or **Overwrite**; click it to switch, or
rest the pointer on it for what each does. In Ripple mode the drop
opens a hole first and pushes the clips at or after that point right on the
patched and sync-locked lanes, so nothing is covered. Otherwise the clip lands
where it was dropped. See [Routing](#routing) for the lanes the picture and the
sound land on.

### Clip bars

A clip bar has a 14-pixel name strip at the top, the content strip under it,
and a line across the bar.

The name strip holds the clip's name, its speed when it is not 100 % (`120%`,
`-100%` for a reversed clip; a speed ramp prints none), and its badges. The fx
badge is gray on a clip with no changes, purple when a clip property such as
Opacity, Position, Crop, Time Remapping, Volume, or Pan differs from a new
clip, yellow when an effect, a grade, a mask, or a key is applied, and yellow
with a purple underline when both hold. Linked clips whose sources have drifted
apart print the offset in frames, such as `+3`, on both bars.

The content strip draws thumbnails on a video clip, as the timeline display
settings choose, and a rectified waveform on an audio clip. A generated clip
draws its name and, for a color generator, a swatch of its color. A clip whose
media file is missing fills red and reads **Media Offline** with the file name.
A disabled clip is drawn at 40 % opacity.

The line draws one property of the clip over time: Opacity on a picture lane
and Volume on an audio lane, until another property is chosen. Click the fx
badge to choose: the menu lists every property of the clip by block
(`Transform: Rotation`, `Gaussian Blur: Radius`, `Mask 1: Feather`), with the
drawn one marked, and **Show in Effect Controls** selects the clip and opens
that panel. Choosing the lane's default property clears the choice, so a clip
moved to the other kind of lane draws that lane's default.

Drag the line up or down to set a property that is one number. The value reads
out as it moves, Escape puts it back, and the drag is one undo step. A drag
between two keyframes moves both; a drag on a keyframe dot moves that keyframe.
Ctrl+click on the line adds a keyframe at that time. On Medium and taller lanes
the bar draws a dot at each keyframe of the property. A vector property draws
its first axis, and a color or a rectangle draws a level line; neither is
dragged.

### Routing

Source patching picks which lanes a source's picture and sound land on: Insert
and Overwrite from a Source tab or for the selected pool items, a drag from a
Source tab, and a media drop. Each video and audio lane header has a source
indicator, V or A. The lit one is where that channel lands; click a dark one to
move the channel there, and click the lit one to leave the channel out. A
channel nobody patched goes to the topmost targeted lane of its kind, else the
kind's first lane, and a lane of its kind is created if the sequence has none.
The sound lands as a clip of its own on the patched audio lane, linked to the
picture, so the two move and trim together. The edit and any lane it creates
are one undo step. A picture placed with its sound patched off is silent. A
drop aims at the lane under the pointer: that lane takes the channel of its
kind, whatever the patch, and the other channel follows the patching.

Sync lock sets which other lanes follow a ripple edit. A lane with sync lock
set moves by the same amount as the lane being edited, so picture and sound stay
in step. Clearing it pins that lane: a ripple edit elsewhere then slides past it.
The lane being edited always moves, regardless of its own sync lock, and the
whole ride-along is one Undo.

An insert opens a hole and keeps every clip: a sync-locked clip lying across the
insert point is cut there and both halves survive. A ripple delete removes a
stretch of timeline, so a sync-locked lane loses that stretch as well: a clip of
its own inside the deleted span goes, and a clip lying across either edge is cut
so only the part inside the span is lost. One Undo brings the lane back whole.
A ripple move does both halves on every sync-locked lane at once: it closes the
span the clip left and opens the span where it lands, so the material between
the old and new position slides with the move. Clear sync lock on a lane to
keep any of these edits elsewhere from touching it.

A cover moves nothing, so sync lock has no effect on an Overwrite-mode drop or
on Fit to Fill without ripple. Closing a gap does not consult it either.

Both flags are set on every new lane, and both are stored with the sequence.

## Nested sequences

A nest is a stretch of timeline that plays as one clip. **Clip > Nest…** takes the
selected clips, moves them onto a sequence of their own, and leaves a single
clip in their place on the lane the first of them came from.

Select that clip and use **Sequence > Nested Sequence > Open Nested Sequence** to work inside it.
The timeline, the lanes, the markers and the preview are then the nest's own,
and every edit lands there rather than on the sequence you came from. The
status strip holds the nest's name while it is open; clicking that chip
leaves it, as does **Sequence > Nested Sequence > Leave Nested Sequence**.

**Sequence > Nested Sequence > Place Nest at Playhead** puts a second clip playing the same nest
at the playhead, on the lane a media drop would land on. Editing inside the
nest changes both clips, because both play the one sequence.

A nest clip draws the whole frame of its sequence: every visible lane, with
its transitions, mattes, adjustment layers, and nests, over a transparent
background. That frame is fitted into the canvas the clip is placed on, as
footage is, and then takes the clip's transform, crop, effects, mask, color,
LUT, layer styles, blend mode, and motion blur as one picture. A sequence of
another shape fills the middle of the canvas; a larger one draws at the
canvas size.

A sequence that shows itself, directly or through another nest, cannot be
placed. A project edited outside Aria that holds one fails the frame with a
sequence cycle error stating the sequence.

Nesting nothing, opening a clip that is not a nest, and leaving when the root
timeline is already open are each refused with what is missing.


## Example

Nest a sequence of clips into a single compound clip, edit inside the nest, and return to the parent sequence:

1. Click [`add-track`](../reference/controls.md#timeline) at the bottom-left of the timeline (or choose **Sequence > Add Tracks > Video Track**) and arrange the clips to group across `V1` and `V2`. The timeline shows the layered clips in the composite.
2. Drag a marquee across the clips to group (or press <kbd>Ctrl+A</kbd> (`key-select-all`) / choose **Edit > Select All**), then choose **Clip > Nest…**. Aria moves the selected clips into a new sequence and replaces them on the parent timeline with a single Tan-colored nested sequence clip.
3. Select the nested sequence clip and choose **Sequence > Nested Sequence > Open Nested Sequence**. The timeline switches to the nested sequence's own tracks and ruler, and the status bar shows the nest breadcrumb [`status-nest-breadcrumb`](../reference/controls.md#window--dock-chrome).
4. Trim or reorder the clips inside the nested sequence, then click [`status-nest-breadcrumb`](../reference/controls.md#window--dock-chrome) in the status bar or choose **Sequence > Nested Sequence > Leave Nested Sequence** (or press <kbd>Ctrl+W</kbd> (`key-pane-close`) with the Timeline focused). The parent timeline returns to view and the Viewer composites the updated nested sequence as one clip.

## Reference

See [Menus](../reference/menus.md#sequence-menu), [Commands](../reference/commands.md), and [Controls](../reference/controls.md#timeline) for track, routing, and nested sequence commands.
