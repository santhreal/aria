# Speed

## Speed

The Speed tab retimes a media clip. It is absent on generated clips, which have
nothing to retime.

- The **Ramp** row shows the active preset, Custom for a hand-edited ramp, or
  None. The chips under it list every built-in ramp, the active one lit,
  matched by the ramp's shape.
- The graph draws the ramp. Time runs left to right across the clip; speed runs
  up, logarithmically, so 100% sits near the middle and the guide line marks it.
  Every point of the ramp is a dot.
- Drag a dot to move it. The first and last dot keep their position in time and
  change only their speed. The dot follows the pointer past the graph's edge,
  held to the ramp's span and to the speed bounds. The edit is written wherever
  the button comes up, and Esc abandons the drag.
- Click the graph away from a dot to add a point there. Right-click a dot to
  remove it. A ramp keeps its last two points; removing one of those is refused.
  Use **Clear ramp** to go back to flat speed.
- **Length** under the graph is the length the ramp implies, updated while you
  drag.
- Start, Middle, and End set the speed at those points of the clip. A flat clip
  gains a 100% ramp on the first edit, so only the point you touch moves.
- **Clear ramp** returns the clip to flat speed and its original length.

A ramp changes the clip's timeline length: the engine re-derives it from the
curve. A ramp or a speed that lengthens a clip followed by a neighbor follows
the edit mode. In Overwrite mode the clip ends at the neighbor's start. In
Ripple mode the clips from the clip's old end on move right by the added
length on its lane, on the lanes of its linked clips, and on every sync-locked
lane; a clip lying across that point on a sync-locked lane stays where it is. A
ramp or a speed that shortens a clip leaves a gap after it in either mode.

A retimed clip splits at any frame. The tail starts on the source frame shown
at the cut. A cut inside one source frame's hold, as on a slowed clip, shows
that frame on both sides of the cut.

Every speed prints as a percent to a tenth: the ramp rows, the note above the
graph, the Speed/Duration dialog, the clip's name strip, and the Rate Stretch
readout. A drag moves a speed one percent per unit, and a typed value is read in
percent.

**Clip > Speed/Duration…** sets constant speed (`5%` to `10000%`), duration,
**Reverse Speed**, and **Maintain Audio Pitch** across the selected clips as one
undo step. Editing speed updates the resulting duration; editing duration
updates the speed. Drag the speed or the duration to scrub it, or click it and
type a value; the duration takes a frame count or timecode. A drag stops at
either end of the range. A typed value outside the range is rejected with the
reason shown in the dialog, and its field stays open until Esc puts back the
value it opened on.

**Clip > Time Remapping** toggles keyframed speed mapping (`Clip::speed_curve`) on
the selected media clip and opens the Speed tab. While Time Remap is active, the
curve maps sequence time to source time by integrating the keyframed speed
multipliers across the clip.


## Example

Retime a clip with constant Speed/Duration and a keyframed speed ramp:

1. Select a video clip on `V1` and press <kbd>Ctrl+R</kbd> (`key-speed-duration`) or choose **Clip > Speed/Duration…**. The Speed/Duration dialog opens with the clip's current speed percentage and duration.
2. Set the speed to `150%`, toggle [`speed-duration-pitch`](../reference/controls.md#modal-dialogs) to preserve audio pitch (or toggle [`speed-duration-reverse`](../reference/controls.md#modal-dialogs) to play the take in reverse), and click [`speed-duration-apply`](../reference/controls.md#modal-dialogs). The clip shortens on the timeline and its name strip displays `150.0%`.
3. Press <kbd>Ctrl+Alt+T</kbd> (`key-time-remap`) or choose **Clip > Time Remapping** to open the Effect Controls **Speed** tab, then click the **Hero** speed-ramp preset or drag dots on the logarithmic speed graph. The clip's timeline length updates to match the integrated speed curve and the Viewer plays fast into a slow-motion middle before ramping back up.
4. Click [`speed-clear`](../reference/controls.md#inspector) (**Clear ramp**) in the **Speed** tab to return the clip to flat speed at its Speed/Duration setting, `150%`. The speed ramp clears and the clip takes back the length it had before the ramp.

## Reference

See [Animations, text effects, easings, and speed presets](../reference/animations.md#speed-ramp-presets-speed_presetstoml), [Controls](../reference/controls.md#modal-dialogs), and [Menus](../reference/menus.md#clip-menu) for all retiming controls.
