# Masks and keys

## Mask and chroma key

Below the grade, the Look tab shows the masks of every visual clip: footage,
stills, titles, shapes, stickers, animations, and adjustment layers. The masks
cut what the clip draws before its effects, LUT, and layer styles, so a shadow
or glow follows the masked edge. On an adjustment layer the masks limit the
layer's look to the masked area. The Look tab holds:

- A stack of up to 32 masks. The seven shape buttons under **Add a mask**
  (linear, mirror, circle, rectangle, heart, star, path) each add a mask at the
  bottom of the stack and make it the active mask. The palette's **Add linear mask** (`pal-mask-add-linear`) through **Add path mask** (`pal-mask-add-path`)
  rows do the same.
- One header per mask: its number, its shape, and **(inverted)** when it keeps
  the outside. Click the header to make that mask the active one. The active
  mask is marked with a dot and shows its rows: feather, roundness on a
  rectangle, rotation, width and height (not on a path), the two offsets,
  opacity, and expansion. The Viewer handles, the Pen, and mask tracking act on
  the active mask.
- A mode per mask: None, Add, Subtract, Intersect, Lighten, Darken, or
  Difference. Masks apply top to bottom. The stack starts empty, or full when
  the first mask with a mode other than None subtracts, intersects, or darkens;
  each mask then replaces the coverage with its mode's result, mixed by its
  opacity. Expansion grows the mask outward by that many pixels, or shrinks it
  when negative, and scales with the clip.
- Invert, Move up, Move down, and Remove on each header. Each press is one undo
  step. The palette's **Invert mask** (`pal-mask-invert`) and **Remove mask** (`pal-mask-clear`) rows and the **Effect > Remove Mask**
  row act on the active mask; removing a mask makes the
  one above it active.
- Mask handles on the preview for the active mask: a dot at the centre moves the
  mask, one on its right edge resizes it, and one above it rotates it. The
  handles sit where the mask is at the playhead. The outline follows the
  pointer; the composite catches up on release, which commits centre, size, and
  rotation as one edit. A keyframed centre, size, or rotation that the drag
  changed gains a keyframe at the playhead and keeps its other keyframes; one
  the drag left alone keeps its curve. Escape abandons the drag.
- Path masks, drawn with the Pen tool (P) on the selected clip in the preview. A
  Pen press off the active path starts a new path mask; closing it adds the mask
  to the bottom of the stack. Each click adds a corner vertex, and a click-drag
  pulls a smooth one with mirrored tangents. Click the first vertex or press
  Enter to close the path; Escape drops it. A path needs three vertices and
  holds at most 1024. On an existing path the Pen inserts a vertex where it
  clicks a segment, Ctrl+click deletes a vertex, and Alt+click turns one between
  corner and smooth; an Alt+drag pulls new tangents. The Pen's Add Vertex,
  Delete Vertex, and Convert Vertex variants do the same without the modifiers.
  Any tool drags a vertex or a tangent handle. Each gesture is one undo step.
- Path keyframes. A path mask has a **Mask N Path** row at the head of its
  mask's timeline rows: a stopwatch, the vertex count at the playhead, and the
  keyframe navigator. A Path keyframe holds the whole path. The stopwatch keys
  the path at the playhead; pressed again it deletes every Path keyframe and
  keeps the path shown at the playhead. While the path is keyframed, a vertex
  or tangent drag in the Viewer keys the path at the playhead and leaves the
  other keyframes alone, and inserting or deleting a vertex changes every
  keyframe so the shape keeps moving between them. Between two keyframes with
  different vertex counts the earlier path holds. Drag a Path keyframe to move
  it in time; it snaps to the playhead and the path's other keyframes, and Alt
  moves it freely. Click one to bring the playhead to it. Right-click one for
  Linear, Ease In and Out, Ease In, Ease Out, Hold or Continuous, and Delete
  Keyframe. Each press is one undo step.
- Mask tracking from the command palette: **Track mask forward from the
  playhead** (`pal-track-mask`). It follows the picture under the active mask through the rest of
  the clip and keyframes that mask's centre for every frame it followed. The
  pass is a job, so Aria keeps drawing while it runs; the jobs board
  counts the frames it has read against the frames it will read, and Cancel
  stops it and keeps what it tracked. A pass that loses the feature stops there
  and states the frame. Undo removes the whole track in one step. Masks added,
  removed, or reordered while it runs do not move the keyframes off the tracked
  mask; if the tracked mask is removed, dragged, or reshaped meanwhile, nothing
  is written and Aria reports it.
- A chroma key. Pick Green, Blue, or Cyan to key on that colour at a
  starting tolerance, then adjust Strength and Shadow. Re-keying keeps the
  tolerance. Remove key clears it.
- Four layer styles: shadow, glow, outline, background. Click one to add it
  at its default, then adjust its knobs and pick a colour. Clear styles
  removes all four.
- A `.cube` LUT. **Load .cube…** picks a file; Intensity blends it over the
  ungraded image; Remove LUT clears it. Loading another LUT keeps the
  intensity you dialled in.
- In, Out, and Loop animations. Chips list the presets for that slot; the
  applied one is marked. Speed, Intensity, and Stagger appear only when the
  preset exposes them. A loop animation replaces the In and Out pair and vice
  versa. Text-only presets appear on text clips.

The Audio tab tags a clip as Music, Sound FX, Voiceover, or Extracted.
**Untag** clears it.

## Reference

See [Stickers and masks](../reference/stickers-and-masks.md), [Animations](../reference/animations.md), [Controls](../reference/controls.md#inspector), and [Commands](../reference/commands.md) for all mask shapes, stack modes, and keyer commands.
