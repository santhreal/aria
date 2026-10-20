# Transitions

## Transitions

A transition is a blend at the cut between two clips that touch. Select the
clip on the left of the cut; the Look tab, or the Audio tab on an audio lane,
then shows the Transition block. It is absent when the clip has no clip
abutting it on the right, and on lanes that cannot hold a transition.

- On a picture lane the catalog lists cross fade, dip to black, dip to white,
  the four wipes, and slide. On an audio lane it lists the audio crossfades.
  The applied blend is marked. Picking another replaces it.
- Duration sets the window length in ticks.
- **Center at Cut**, **Start at Cut**, and **End at Cut** position the window across the cut, after the cut, or before the cut.
- **Remove transition** clears it.

Ctrl+T puts the default picture transition, the first in the catalog, at the
selected edge or at the cut nearest the playhead on the targeted picture lanes.
**Sequence > Apply Default Transitions** runs the same command. Ctrl+Shift+T
puts Constant Power at the selected edge or at the cut nearest the playhead on
the targeted audio lanes. A selected edge on the other kind of lane is refused
with a status line stating the other chord. The Effects panel lists both sets,
under **Video Transitions** and **Audio Transitions**.

The timeline draws the window as a band at the cut according to its alignment.
Drag either edge of the band to set the length: a centered band moves both
edges symmetrically around the cut, and a start-aligned or end-aligned band
changes length 1:1 with the dragged edge. Right-click the band for **Remove
transition**, **Center at Cut**, **Start at Cut**, **End at Cut**, and the
blend catalog. All act on that cut whatever is selected.

Deleting or moving either clip so the two no longer touch drops the transition;
undo restores it.

### Audio crossfades

- **Constant Power** fades by `sin` and `cos`. The summed power stays constant,
  so two unrelated sounds keep their loudness through the cut. Ctrl+Shift+T
  applies it.
- **Constant Gain** fades linearly in amplitude. The two gains sum to one, so a
  sound continued across the cut keeps its level.
- **Exponential Fade** moves each level linearly in decibels between -60 dB
  and 0 dB.

The outgoing clip plays past the cut into its unused source media and fades
out across the window; the incoming clip starts early from its unused source
media and fades in. A clip with less unused media than half the window fades
over the part of the window it covers, reaching silence where its media ends. A
retimed clip fades inside its own extent. The crossfade replaces the clip's own
fade on that side, in playback and in export.


## Example

Add a video transition and an audio crossfade across an edit cut:

1. Select the [`tool-pointer`](../reference/controls.md#tool-strip) tool (<kbd>V</kbd> (`key-tool-pointer`)), place two clips with unused source handles side by side on `V1` and `A1`, and click the outgoing clip on `V1` (or park the playhead at the cut). The cut between the two clips is ready on both lanes.
2. Press <kbd>Ctrl+T</kbd> (`key-transition`) or choose **Sequence > Apply Default Transitions** (or choose **Sequence > Add Cross Fade at Cut** / run **Add cross fade at cut** (`pal-transition-crossfade`)). Aria attaches a `Cross Fade` transition band across the cut on `V1`.
3. Press <kbd>Ctrl+Shift+T</kbd> (`key-audio-transition`) to apply the default `Constant Power` audio crossfade on `A1`. The timeline draws both the video transition pill on `V1` and the audio crossfade band across the cut on `A1`.
4. With the outgoing clip selected, open the Effect Controls **Look** tab (**Window > Effect Controls**), click [`transition-choice`](../reference/controls.md#inspector) to switch the video blend to **Dip to Black** or **Wipe Left**, and click [`transition-align`](../reference/controls.md#inspector) to choose **Center at Cut**, **Start at Cut**, or **End at Cut** (or click [`transition-clear`](../reference/controls.md#inspector) / choose **Sequence > Remove Transition at Cut** to remove it). Playback across the cut blends the outgoing and incoming frames and crossfades the audio smoothly in the Viewer.

## Reference

See [Transitions](../reference/transitions.md), [Controls](../reference/controls.md#inspector), and [Menus](../reference/menus.md#sequence-menu) for all 8 video transitions and 3 audio crossfade curves.
