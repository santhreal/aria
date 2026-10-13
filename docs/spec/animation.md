# Animation

## Properties

Every value in a property row ([timeline.md](timeline.md#property-rows)) and in
Effect Controls is a property. A property has a type (scalar, angle, 2D point,
3D point, colour, boolean, choice, text, path), a default, a range, and a
display unit. Scalar properties carry a soft range (the slider's span) and a
hard range (the clamp); typing a value outside the soft range is allowed up to
the hard range.

Plugin effects declare their parameters with the same types, so a plugin
parameter animates, scrubs, and draws in the graph editor the same as a
built-in one.

## Keyframes

A keyframe has a time, a value, and for each side (incoming and outgoing):

| Field | Values |
| --- | --- |
| Temporal interpolation | Linear, Bezier, Hold |
| Temporal ease | Speed (units per second) and influence (0.1 % to 100 % of the segment) |
| Spatial interpolation (point properties) | Linear, Bezier, Auto Bezier, Continuous Bezier, with an in and out tangent in property units |
| Roving | A roving keyframe has no fixed time; its time is solved so the speed through it is constant |

The keyframe glyph states its interpolation on each half:

| Glyph | Meaning |
| --- | --- |
| Diamond | Linear |
| Hourglass | Bezier (eased) |
| Circle | Auto Bezier |
| Square | Hold |

A half-and-half glyph means the two sides differ. Selected keyframes fill with
the accent colour.

### Keyframe gestures

| Input | Result |
| --- | --- |
| Click | Selects the keyframe. Shift adds. Drag a marquee across rows to select many. |
| Drag | Moves the selected keyframes in time. Shift snaps to the playhead and other keyframes. |
| Alt+drag the first or last of a selection | Scales the selection's timing around the opposite end. |
| Ctrl+click | Cycles Linear and Auto Bezier (Hold with Ctrl+Alt). |
| Double-click | Opens Keyframe Velocity: incoming and outgoing speed and influence, with Continuous to keep both sides equal. |
| F9 | Easy Ease: speed 0, influence 33.33 % on both sides. |
| Shift+F9 / Ctrl+Shift+F9 | Easy Ease In / Out. |
| Ctrl+Alt+H | Toggle Hold. |
| Ctrl+C / Ctrl+V | Copies keyframes; pasting places the first at the playhead on the selected property, or on the same property of the selected clip. |
| Delete | Deletes the selected keyframes. |
| Alt+Shift+Left / Alt+Shift+Right | Playhead to the previous / next keyframe on the visible property rows. |

Keyframe edits are commands like any other edit: each gesture is one undo step,
and a plugin's `CommandProposed` handler sees it.

### Migration of existing projects

Current projects store a CSS-style cubic bezier per segment. A segment from
value `v0` at `t0` to `v1` at `t1` with control points `(x1, y1, x2, y2)`
converts without loss: outgoing influence `x1`, outgoing speed
`(y1 / x1) * (v1 - v0) / (t1 - t0)`, incoming influence `1 - x2`, incoming speed
`((1 - y2) / (1 - x2)) * (v1 - v0) / (t1 - t0)`. Linear, Hold, and the named
presets convert to the matching interpolation. The project schema version is
bumped and a project from an older version is converted on load.

## Graph editor

Shift+F3 replaces the keyframe rows of the opened clips with a graph of the
selected properties. One property is drawn per colour; the outline column shows
the colour next to each property name.

| Control | Effect |
| --- | --- |
| Value graph | Draws the value over time. Bezier handles on each keyframe set the curve. |
| Speed graph | Draws the rate of change. Handle length is influence, handle height is speed. |
| Auto-select graph type | Value graph for scalar properties, speed graph for spatial ones. |
| Separate dimensions | Splits a point property into X, Y (and Z) properties with independent keyframes. |
| Fit selection, fit all | Zooms the graph vertically and horizontally to the keyframes. |
| Show reference graph | Draws the other graph type faintly behind the active one. |
| Transform box | With several keyframes selected, a box with handles scales and moves them in time and value. |
| Snap | Snaps keyframes and handles to frames, other keyframes, and the playhead. |

The graph is drawn with antialiased paths at the display's refresh rate while
handles are dragged; the Viewer updates on each frame of the drag at the paused
resolution.

## Motion paths

Position and anchor point keyframes draw a path in the Viewer (see
[viewer.md](viewer.md#on-canvas-controls)). Dragging a path keyframe moves its
value; dragging a tangent changes its spatial interpolation; Alt+click on a
keyframe converts it between corner and smooth. Ctrl+click on the path with the
Pen tool adds a keyframe at that point.

## Parenting

The Transform group has a Parent row. Dragging its pick whip onto another clip,
or choosing a clip from its menu, makes this clip's transform relative to the
parent's. The parent must overlap the child in time; outside the overlap the
child uses its own transform. Parenting keeps the child's current world
position: setting or clearing a parent rewrites the child's transform so the
frame does not change. Shift+parent snaps the child to the parent's transform
instead.

## Drivers

A driver replaces a property's keyframed value with a value computed per frame.
Alt+click on a stopwatch adds one. The row then shows the driver's source text
under the property, and the value in `value` colour turns red when the driver
fails, with the error in a tooltip and in the Info panel.

Drivers are a plugin contribution point
([plugins.md](plugins.md#property-drivers)). Aria ships one driver language
plugin, `aria.expressions`, a JavaScript subset with the After Effects
expression library: `time`, `value`, `thisLayer`, `thisComp`, `wiggle`,
`loopOut`, `loopIn`, `linear`, `ease`, `valueAtTime`, `velocityAtTime`, and the
vector math functions. A driver reads other properties through the pick whip,
which inserts a reference such as `thisComp.layer("Logo").transform.position`.
Drivers run at render time on every frame, on a worker thread, with a time
budget per frame; a driver that exceeds it is stopped and reported.

## Presets

Effects & Presets > Save Animation Preset stores the selected properties,
effects, and keyframes as an `.ariapreset` file. Applying a preset to a clip
adds its effects and keyframes relative to the playhead. Presets live in
`<data dir>/presets/` and plugins can ship them.

## Time

- Time Remap (Ctrl+Alt+T) adds a property that maps sequence time to source
  time with keyframes; its graph is the speed ramp editor.
- Speed/Duration (Ctrl+R) sets a constant speed, reverse, and whether audio
  pitch is held.
- Frame blending: off, frame mix, or optical flow, per clip.
- Motion blur: a per-clip switch and a sequence master switch, with shutter
  angle and samples in Sequence Settings.
