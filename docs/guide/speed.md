# Speed

## Speed

The Speed tab retimes a media clip. It is absent on generated clips, which have
nothing to retime.

- The preset row lists every built-in ramp. The active one is marked, matched by
  the ramp's shape, so a hand-edited ramp shows none as active.
- The graph draws the ramp. Time runs left to right across the clip; speed runs
  up, logarithmically, so 1x sits near the middle and the guide line marks it.
  Every point of the ramp is a dot.
- Drag a dot to move it. The first and last dot keep their position in time and
  change only their speed. The graph follows the pointer; the edit is written on
  release, and Esc abandons the drag.
- Click the graph away from a dot to add a point there. Right-click a dot to
  remove it. A ramp keeps its last two points; removing one of those is refused.
  Use **Clear ramp** to go back to flat speed.
- **Length** under the graph is the length the ramp implies, updated while you
  drag.
- Start, Middle, and End set the speed multiplier at those points of the clip. A
  flat clip gains a 1x ramp on the first edit, so only the point you touch moves.
- **Clear ramp** returns the clip to flat speed and its original length.

A ramp changes the clip's timeline length: the engine re-derives it from the
curve. A ramp that would push the clip into its neighbour is refused.

**Clip > Speed/Duration…** sets constant speed (`5%` to `10000%`), duration,
**Reverse Speed**, and **Maintain Audio Pitch** across the selected clips as one
undo step. Editing speed updates the resulting duration; editing duration
updates the speed. Values outside the supported range are rejected with the
reason shown in the dialog.

**Clip > Time Remapping** toggles keyframed speed mapping (`Clip::speed_curve`) on
the selected media clip and opens the Speed tab. While Time Remap is active, the
curve maps sequence time to source time by integrating the keyframed speed
multipliers across the clip.

## Reference

See [Animations, text effects, easings, and speed presets](../reference/animations.md#speed-ramp-presets-speed_presetstoml), [Controls](../reference/controls.md#modal-dialogs), and [Menus](../reference/menus.md#clip-menu) for all retiming controls.
