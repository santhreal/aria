# Animation

## Properties

Every value in a property row ([timeline.md](timeline.md#property-rows)) and in
Effect Controls is a property. A property has a type (scalar, angle, 2D point,
3D point, color, boolean, choice, text, path), a default, a range, and a
display unit. Scalar properties have a soft range (the slider's span) and a
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
the accent color.

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
and a plugin's `CommandProposed` handler receives it.

## Graph editor

Shift+F3, or the graph button in the timeline header, opens the graph editor
band under the property rows of the opened clips. It draws the selected
properties over the time area, with a value axis at the right of the header
column. The header's tools row holds the Value and Speed chips, Easy Ease, and
Hold.

| Control | Effect |
| --- | --- |
| Value graph | Draws the value over time. |
| Speed graph | Draws the rate of change, in units per second. |
| Keyframe handles | Dragging a keyframe's incoming or outgoing handle sets that side's speed and influence. |
| Easy Ease | Speed 0 and influence 33.33 % on both sides of the selected keyframes. |
| Hold | Toggles Hold on the selected keyframes. |

## Motion paths

Position and anchor point keyframes draw a path in the Viewer (see
[viewer.md](viewer.md#on-canvas-controls)). Dragging a path keyframe moves its
value; dragging a tangent changes its spatial interpolation; Alt+click on a
keyframe converts it between corner and smooth. Ctrl+click on the path with the
Pen tool adds a keyframe at that point.

## Parenting

The Transform group has a Parent row. It shows the parent in the value column,
and on the line under it None, a chip for each clip the model accepts as the
parent, and the pick whip, a link icon lit while armed. A parent is named by
its lane badge and clip name, as in `V2 Interview`. Dragging the pick whip
onto another clip, clicking it and then a clip, or choosing a clip's chip makes
this clip's transform relative to the parent's. The parent must overlap the
child in time; outside the overlap the child uses its own transform. Parent
and child are both clips on picture lanes: a clip on an audio lane has no
transform, so the row never offers one and the pick whip rejects one. Parenting
keeps the child's current world position: setting or clearing a parent
rewrites the child's transform so the frame does not change. Shift+parent
snaps the child to the parent's transform instead.

## Drivers

A driver replaces a property's keyframed value with a value computed per frame.
Alt+click on a stopwatch adds one. The row then shows the driver's source text
under the property, and the value in `value` color turns red when the driver
fails, with the error in a tooltip and in the Info panel.

Drivers are a plugin contribution point
([plugins.md](plugins.md#property-drivers)). Aria ships one driver language
plugin, `aria.expressions`, a JavaScript subset with the After Effects
expression library: `time`, `value`, `thisLayer`, `thisComp`, `wiggle`,
`loopOut`, `loopIn`, `linear`, `ease`, `valueAtTime`, `velocityAtTime`, and the
vector math functions. A driver reads other properties through the pick whip,
the link icon beside the source being edited, which inserts a reference such as
`thisComp.layer("Logo").transform.position`.
Drivers run at render time on every frame, on a worker thread, with a time
budget per frame; a driver that exceeds it is stopped and reported.

## Presets

The effect context menu's Save as Preset… and the inspector's Save as preset
store the selected properties, effects, and keyframes as an `.ariapreset`
file. Applying a preset to a clip adds its effects and keyframes relative to
the playhead. Presets are stored in `<data dir>/presets/` and plugins can ship
them.

## Time

- Time Remap (Ctrl+Alt+T) toggles the clip's keyframed speed curve
  (`Clip::speed_curve` via `EditCommand::SetSpeedCurve`), which maps sequence
  time to source time (`Clip::source_time_at`); its graph is the speed ramp
  editor in the Effect Controls panel's Speed tab.
- Speed/Duration (Ctrl+R) opens the Speed/Duration dialog
  (`DialogKind::SpeedDuration`), setting constant speed, resulting duration,
  reverse, and whether audio pitch is held across the selected clips in one undo
  step.
- Motion blur: a per-clip switch and a sequence master switch, with shutter
  angle and samples in Sequence Settings.
