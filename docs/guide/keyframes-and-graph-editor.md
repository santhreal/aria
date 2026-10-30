# Keyframes and the graph editor

## Keyframes

The Transform and Shape tabs end in a Keyframes block with one row per
animated property of the selected clip: each property that holds a keyframe
or a driver. With none, the block reads None. Each row shows how many
keyframes the property holds, and its buttons stand on the line under it. A
constant property is keyed with the stopwatch or ◇ on its own row, or on its
timeline property row.

- ◇ writes a keyframe at the playhead, pinning the value the property already
  has there. ◆ means one is already there; clicking it removes that keyframe.
- ‹ and › move the playhead to the previous and next keyframe.
- **ƒ Driver** adds a driver to the property and **ƒ Remove** removes it; see
  [Drivers](../plugins/drivers.md).
- **Flatten** drops every keyframe and holds the value at the playhead.
- With a keyframe under the playhead, the easing chips shape the segment leaving
  it: Snappy, Overshoot, and Anticipate are single curves; Bounce out, Elastic
  out, and Back out expand the segment into several keyframes.

Once a property is animated, moving a Transform slider writes the keyframe under
the playhead instead of replacing the curve. The reset glyph at the foot of the
tab clears the animation.

An expanded clip draws its keyframe ticks as marks in the strip.

### Graph block

While a property of the selected clip holds a keyframe, the Transform tab draws
a Graph block under the Keyframes block. It plots one property against time:
the one chosen on its chips or by clicking a timeline property row's name, and
otherwise the first property that holds a keyframe. The chips list the
properties that hold keyframes, and the chosen property when it holds none. A
point property such as Position adds X and Y chips, which select the axis
drawn.

- Drag a keyframe point to move it in time and value. A keyframe dragged past
  the graph's right edge lands on the clip's last frame.
- Drag a handle to bend the curve of the segment it shapes. A handle is drawn
  as a lever from its keyframe to its tip, in the color the timeline graph
  draws its handles in, and lies on the curve's tangent at its keyframe. A
  linear segment draws its handles a third of the way along it. A lever that
  runs past the graph's edge is cut there.
- A press grabs the nearest keyframe or handle. A keyframe wins over a handle
  at the same distance.
- While the button is held, the graph draws the curve, the keyframes and the
  handles where the release writes them. The drag follows the pointer past the
  graph's edge, held to the clip and to the property's range, and is written
  wherever the button comes up.
- Esc drops a drag unwritten. Each drag is one undo step.

### Property controls

A property that can be animated has these controls, in Effect Controls and on a
timeline property row:

- The value prints in the same units on both: opacity and scale in percent,
  rotation in degrees, volume in decibels, and a volume of zero as Silent.
  Drag the value to scrub it, or click it and type a number in the units it
  prints.
- In Effect Controls, a property bounded at both ends draws a track under its
  name and value, filled to where the value stands between the bounds.
  Volume's track is even in decibels from -60 dB to the +20 dB ceiling, as an
  Audio Track Mixer fader is, so 0 dB stands three quarters along.
- The stopwatch, left of the name, turns animation on and off. A click on a
  constant property writes a keyframe at the playhead. A click on an animated
  property deletes every keyframe and keeps the value at the playhead, as one
  undo step. Alt+click adds a driver, or removes the one the property has; see
  [Drivers](../plugins/drivers.md).
- Editing an animated value writes a keyframe at the playhead. With the
  playhead off the clip, the keyframe lands on the clip's nearest frame.
- ‹ and › move the playhead to the previous and next keyframe. ◆ marks a
  keyframe under the playhead, and a click deletes it. ◇ writes a keyframe
  there holding the value the property has.
- The reset button returns the property to its default. On an animated
  property it writes the default as a keyframe at the playhead and keeps the
  rest of the curve. It is dimmed when the property holds its default there.

In Effect Controls, a time area follows the reset button. It spans the selected
clip and draws the playhead and the property's keyframes. In a narrow Effect
Controls panel the time area narrows, and is left blank below 32 px, before the
property's name is shortened; hover a shortened name to read it in full.

- Click a keyframe to select it; Shift+click adds it to the selection. Drag to
  move every selected keyframe. The drag snaps to the playhead and to the
  clip's other keyframes; hold Alt to move freely. The move is one undo step,
  and the timeline's property row draws it while it is in flight.
- Press anywhere else to move the playhead to that frame, and drag to scrub.
- Escape drops a keyframe drag unwritten, and returns a scrubbed playhead to
  where the press found it.

### Several selected clips

With several clips selected, Effect Controls draws the rows of the first selected
clip. An edit there reaches every other selected clip that has the property, as
one undo step:

- A step, a scrub, or a typed value gives every clip the value the first clip
  lands on. On a point row it gives only that axis; each clip keeps its own
  value on the other axes.
- The reset button returns each clip to its own default.
- The stopwatch, the keyframe navigator's diamond, **Flatten**, and the easing
  chips act on every clip as the first clip's row drew them: a diamond drawn ◆
  deletes the keyframe at the playhead on every clip, and one drawn ◇ writes
  one on every clip. Alt+click on the stopwatch adds a driver to the clips that
  have none when the first clip has none, and removes it from the clips that
  have one when the first clip has one.
- A chip or a switch reaches the clips of the first clip's kind that read what
  the first clip reads there.
- The color wheels' drag and reset reach every clip that grades.
- An effect added with **Add**, or dragged onto the Effects tab, lands on every
  clip on the same kind of lane; a shorter chain takes it at its end. A
  parameter edit, the effect's switch, a move, the reset button, and the effect menu's
  **Reset**, **Rename Effect**, **Duplicate**, **Paste Below**, and **Remove
  Effect** reach every clip holding the same effect at the same place in its
  chain. A menu opened on an effect of another clip acts on that clip alone.

A dash replaces a value the selected clips read differently at the playhead.
On a point row, a dash on one axis means the clips differ on that axis.

The time area, ‹, and › act on the first selected clip, and a timeline property
row acts on its own clip. While a title is being typed into in the Viewer, an
edit reaches that title alone.

### Property rows

The arrow at the left of a clip bar opens the clip into property rows under its
track, grouped under Transform, Effects, Masks, and Audio headers. A header
arrow opens or closes its group; a clip on an audio lane shows only Effects and
Audio. Each row shows the property's value at the playhead and the clip's
keyframes on that property.

- Drag a row's value left or right to scrub it. On an animated property the
  scrub writes the keyframe under the playhead.
- A keyframe's icon shows its interpolation: a diamond for linear, an hourglass
  for Easy Ease, a half-hourglass for an ease on one side, a square for Hold,
  and a circle for a roving keyframe.
- Click a keyframe to select it; Shift+click adds or removes it. Drag across an
  empty part of a row to select every keyframe inside the band; Shift-drag
  adds them to the selection. Drawn back past a keyframe, the band deselects
  it, and Escape during the drag puts back the selection the press left.
- Drag a selected keyframe to move every selected keyframe in time. The drag
  snaps to the playhead and to the clip's other keyframes; hold Alt to move
  freely. The move is one undo step, and Escape during the drag moves nothing.
- Right-click a keyframe, on its row or in the graph editor, for Linear, Easy
  Ease, Easy Ease In, Easy Ease Out, Hold, Rove Across Time (on a keyframe
  between two others), Copy Keyframes, Paste Keyframes at Playhead, and Delete
  Keyframes. Hold and Rove Across Time show a check mark while they apply.
  Linear and Rove Across Time act on the keyframe under the pointer; the other
  rows act on the selected keyframes, and a right-click on a keyframe outside
  the selection selects it first.

The reveal chords open rows without the arrow: Alt+P, Alt+S, Alt+R, Alt+T, and
Alt+A show only Position, Scale, Rotation, Opacity, or Anchor Point; Alt+E,
Alt+M, and Alt+L show the effect, mask, or audio rows; Alt+M includes each path
mask's Path row. Alt+U shows every animated or driven property and every
keyframed path, and a second Alt+U also shows every property whose value
differs from its default and every Path row. Adding Shift adds the property to
the rows already open instead of replacing them. Alt+` closes every row.

### Graph editor

Shift+F3 opens the graph editor under the timeline. It draws the curves of the
properties chosen by clicking a row's name or a keyframe (Shift+click adds a
property). With none chosen, it draws the property chosen in the Graph block,
else the clip's first keyframed property, while that row is open or no row is,
and otherwise the clip's first open row. The rows of the drawn properties are
highlighted.

- **Value** plots the property's value; **Speed** plots its rate of change.
- Drag a keyframe to change its value and time. The drag follows the pointer
  past the graph's edges, and a release above or below the graph writes the
  top or bottom value of the graph. Escape during the drag writes nothing.
- Each keyframe has incoming and outgoing influence handles labeled with their
  influence. Dragging a handle sets the ease speed and influence of that side.
  A press on a keyframe's icon takes the keyframe even where its handles lie
  under it, as a linear keyframe's do; a press past the icon, within 8 pixels,
  pulls out the nearest handle.
- **Easy Ease** and **Hold** apply to the selected keyframes, or to the keyframe
  under the playhead when none is selected.

### Animation menu

- **Add Keyframe** writes a keyframe at the playhead on every selected property
  row, holding the value the property has there.
- **Add Expression** adds a driver to the selected property row and opens it.
  A row that already has a driver opens that driver.
- **Keyframe Velocity…**, or a double-click on a keyframe, sets the incoming and
  outgoing speed (property units per second) and influence (per cent of the
  segment) of the selected keyframes, or of the keyframes under the playhead.
  **Continuous** keeps both sides at one speed. A Hold keyframe stays Hold.
- **Animation > Keyframe Assistant > Time-Reverse Keyframes** mirrors two or more selected
  keyframes of one property across their span, so the animation between the
  first and the last plays backwards.
- **Reveal Modified Properties** opens every property row of the selected clip
  whose value differs from its default, as the second Alt+U does.

The palette writes a keyframe on the selected clip at the playhead, holding the
value the property has there, one property per row: **Keyframe Opacity at
Playhead** (`pal-key-opacity`), **Keyframe Position at Playhead**
(`pal-key-position`), **Keyframe Scale at Playhead** (`pal-key-scale`),
**Keyframe Rotation at Playhead** (`pal-key-rotation`), **Keyframe Anchor Point
at Playhead** (`pal-key-anchor-point`), **Keyframe Crop at Playhead**
(`pal-key-crop`), **Keyframe Volume at Playhead** (`pal-key-volume`), and
**Keyframe Pan at Playhead** (`pal-key-pan`). **Remove Opacity Keyframe at
Playhead** (`pal-key-remove`) deletes the opacity keyframe there.

Each edit is one undo step.

**Edit > Remove Attributes…** lists the attribute groups the selected clips
hold, each ticked: Motion, Opacity, Crop, Time Remapping, Volume, Pan, Effects,
Audio Effects, Grade, Mask, and Chroma Key. Remove resets the ticked groups to a
new clip's values as one undo step. Clearing Time Remapping re-derives the
clip's duration; clearing Volume keeps the fades.


## Example

Animate a title's position with keyframes and an easing curve:

1. Select a title clip on `T1`, move the playhead to the first frame of the clip, and press <kbd>Alt+P</kbd> (`key-reveal-position`) to reveal the **Position** row under the clip on the timeline (or open **Window > Effect Controls**). The **Position** property row opens beneath the track header.
2. Click the stopwatch [`prop-watch`](../reference/controls.md#keyframes) (or click [`kf-toggle`](../reference/controls.md#keyframes) / choose **Animation > Add Keyframe** / run **Keyframe Position at Playhead** (`pal-key-position`)) and drag the title to its starting off-center position in the Viewer. A diamond keyframe marker appears on the **Position** row at the clip's start frame.
3. Move the playhead 24 frames forward, drag the title to the center of the frame in the Viewer, and use [`kf-prev`](../reference/controls.md#keyframes) and [`kf-next`](../reference/controls.md#keyframes) (or press <kbd>Shift+Alt+Left</kbd> (`key-keyframe-prev`) and <kbd>Shift+Alt+Right</kbd> (`key-keyframe-next`)) to step between the two keyframes. Aria writes a second keyframe at the playhead and draws the motion path between the two positions.
4. Select the second keyframe and press <kbd>F9</kbd> (`key-easy-ease`) or choose **Animation > Keyframe Assistant > Easy Ease** (or click [`graph-btn-easy-ease`](../reference/controls.md#timeline) in the timeline header). The keyframe icon changes from a linear diamond to an Easy Ease hourglass.
5. Press <kbd>Shift+F3</kbd> (`key-graph-editor`) or click [`timeline-graph`](../reference/controls.md#timeline) (or choose **Animation > Graph Editor**) to open the graph editor under the timeline, click the **Position** row label, and drag the incoming Bézier influence handle (or choose **Animation > Keyframe Velocity…** and click [`keyframe-velocity-apply`](../reference/controls.md#modal-dialogs)). Playing the clip in the Viewer moves the title smoothly along the eased curve into its resting position.

## Reference

See [Animations, text effects, easings, and speed presets](../reference/animations.md), [Controls](../reference/controls.md#keyframes), [Menus](../reference/menus.md#animation-menu), and [Keyboard shortcuts](../keyboard-shortcuts.md) for all keyframe and graph editor controls.
