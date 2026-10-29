# Audio

## Audio Track Mixer

<kbd>Shift+6</kbd> (`key-mixer`), or **Window > Audio Track Mixer**, shows the Audio Track Mixer in place
of the Viewer. It holds one strip per lane that has sound, plus one per submix
bus, drawn in the order the mixdown folds them: a bus sits after everything that
feeds it. The Master strip comes last and holds the master's **Inserts**. An
audio lane always has a strip. A picture lane has one while a clip on it holds
its sound in the picture; a lane whose clips' sound is on an audio lane, or
whose clips have none, has no strip until such a clip lands on it. With no
strip to draw, the mixer shows a line in place of the Master strip.

Each strip holds:

- The lane's name. A bus is marked `· bus`.
- **Inserts**: the lane's audio effects, applied to its sum before the fader
  and the sends ([Audio effects](#audio-effects)).
- **Out**: one chip per destination. `Master` is the default; the others are the
  buses. Click a chip to route the lane there. A route that would loop back into
  the lane is refused and the routing is left as it was.
- **Sends**: one chip per bus. Click it once to send a copy of the lane to that
  bus after its fader, again to take the copy before the fader, and a third time
  to drop the send. A send reads its level beside the bus name.
- The fader, read in decibels below it. The faders of every strip stand level
  and take the height of the panel. Drag one: up is louder, the travel runs from
  silence to +12 dB over the fader's drawn height, and it snaps to 0 dB near
  unity. The fader follows the pointer off the strip, and the project is
  written once on release wherever the button comes up, so riding a fader is
  one undo step.

**+ Submix bus** at the top right adds a bus, named `Bus 1`, `Bus 2`, and so on.
**Mixer: Add a Submix Bus** (`pal-mixer-add-bus`) in the palette does the same. Deleting a bus returns
everything that fed it to the master; undo restores both the bus and the routing.

A strip whose signal cannot arrive at the master, behind a muted bus or with
its fader at silence, is bordered and reads `not in the mix`. Those lanes are
not decoded during export.

The assistant routes too: it adds a submix lane, moves a fader, points a lane's
output at a bus, and adds, changes or drops a send. `describe_project` includes
each lane's fader, the bus its output feeds, and the sends it holds in the order
`set_send` addresses them. A level outside silence to +12 dB, a destination that
is not a bus, a route that would loop, and a second send from one lane to one
bus are refused with what to send instead.


## Audio effects

An audio effect processes sound in a chain: on one clip, on a lane, or on the
master after every lane and bus is summed. Effects in a chain run top to
bottom. Aria ships Gain, Parametric EQ (eight bands with low and high cuts),
Compressor, Limiter, De-esser, Noise Reduction, Reverb, Delay, Pitch Shifter,
Swap Channels, Fill Left with Right, Fill Right with Left, Mono, and Loudness
Normalizer; a plugin adds more.

A clip's chain is the **Audio effects** section of the Audio tab, shown for a
clip with sound. A lane's chain and the master's are the **Inserts** row of each
mixer strip and of the Master strip. A lane and the master hold five effects
each; a clip holds any number.

- **+ Audio effect** opens the list of effects; clicking one appends it and
  closes the list.
- Each effect row holds its name, a chip that reads **On** or **Bypassed**
  (click it to switch), the up and down chevrons that move it one place, absent
  at the ends of the chain, and a bin to remove it.
- Parameters step within their range: left click raises a value, right click
  lowers it. In the Audio tab every effect shows its parameters; on a mixer
  strip, click an effect's name to show or hide them. A keyframed parameter
  reads `keyed`, and a step writes a keyframe at the playhead instead of
  replacing the curve.
- An effect that reads a key, such as the Compressor, shows **Key from** on a
  mixer strip: **No key** or one chip per lane that may key it.
- An effect whose plugin is not installed reads `(not installed)`. Its values
  stay in the project and the sound passes through it unchanged.

Every change is one undo step. Preview and export run the same chain; the
mixer delays every other path by the latency of the Limiter, Noise Reduction,
and Pitch Shifter so lanes stay in time.


## Example

Route dialogue through a submix bus, insert an audio effect chain, and normalize clip peaks:

1. Select the dialogue clips on `A1` and press <kbd>G</kbd> (`key-audio-gain`) or choose **Clip > Audio Gain…**. The Audio Gain dialog opens and reports the measured peak in dBFS across the selection.
2. Choose **Normalize Max Peak to**, set the target level, and click [`audio-gain-apply`](../reference/controls.md#modal-dialogs). The clip waveforms on `A1` scale to the normalized level in one undo step.
3. Press <kbd>Shift+6</kbd> (`key-mixer`) or choose **Window > Audio Track Mixer**, then click **+ Submix bus** or run **Mixer: Add a Submix Bus** (`pal-mixer-add-bus`) in the Command Palette. A `Bus 1 · bus` strip appears in the mixer before the `Master` strip.
4. On the `A1` strip, click the `Bus 1` chip under **Out** to route `A1` into `Bus 1`, then add **Parametric EQ** and **Compressor** under **Inserts** on `Bus 1` and reorder them with [`insert-up`](../reference/controls.md#audio-mixer) or [`insert-down`](../reference/controls.md#audio-mixer) (or remove an insert with [`insert-remove`](../reference/controls.md#audio-mixer)). Press <kbd>Shift+9</kbd> (`key-pane-audio-meters`) or choose **Window > Audio Meters** to watch the summed peak levels in the Audio Meters panel and [`preview-meter`](../reference/controls.md#audio-meters) during playback.

## Reference

See [Audio effects](../reference/audio-effects.md), [Controls](../reference/controls.md#audio-mixer), [Menus](../reference/menus.md#clip-menu), and [Commands](../reference/commands.md) for all audio processors and mixer controls.
