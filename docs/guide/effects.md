# Effects

## Effects & Presets

**Window > Effects & Presets**, or <kbd>Shift+5</kbd> (`key-pane-effects`), brings the Effects & Presets panel
forward, opening it in the right column when it is closed, with its search box
taking keys. The Project panel's **Effects** tab shows the same tree and search.
The tree has one folder per group:

- **Video Effects** and **Audio Effects**, each with one folder per category
  and the Other folder last.
- **Transitions**.
- **Generators**: generators and plugin clip types. Aria ships Color
  Matte, Linear Gradient, Radial Gradient, Checkerboard, Grid, Fractal Noise,
  and Color Bars; see [Generators](#generators).
- **Animation Presets**: saved presets and plugin presets. A saved preset hides
  a plugin preset of the same name.

Click a folder to open or close it. Click an entry to pick it. Double-click an
entry to apply it to the selection:

| Entry | Double-click |
|---|---|
| Video effect | Appends it to the selected clip's chain |
| Audio effect | Appends it to the chain of every selected clip with sound |
| Transition | Sets it at the selected clip's right cut |
| Generator, clip type | Places a clip at the playhead |
| Animation preset | Applies it across the selected clips, anchored at the playhead |

Drag an entry onto a clip to apply it to that clip, a transition onto a cut, and
a generator or clip type onto a lane to place it there. Hover an entry to show
where it comes from (Built in, Saved preset, or the plugin's name) and the two
gestures: double-click and drag.

Drag a video effect onto the Effects tab of Effect Controls to add it to the selected
clip's chain: dropped on an effect, it goes above that effect; dropped anywhere
else in the tab, it goes at the end. The drop is one undo step. Any other entry
that applies to a clip applies to the selected clip. A generator, clip type or
sticker dropped there is refused, with a message to drop it on a lane.

Click the star on an entry to list it under **Favourites**, the first folder.
Click it again to remove it. Favourites are saved with the settings.

Type in the search box to filter the tree. The search ignores case and matches
the name, the id, and the category. Every folder holding a match opens, and a
group with no match is hidden. The `×` at the right of the box empties the
search. Esc returns keys to Aria.

**Failed to Load**, the last folder, lists each plugin that did not mount and
each preset file that did not read, with the reason.

A workspace can list one group alone, as the Audio workspace lists Audio
Effects. A caption over the tree states the group. Favourites from that group
and Failed to Load stay listed. **Show All** lists every group again.

### Generators

A generator draws a picture from its parameters alone. Placing one adds a
transparent solid the size of the frame, named after the generator, with the
generator first in its effect chain. The Effects tab of Effect Controls edits and
keyframes its parameters, and effects added below it process its picture.

See [Generators](../reference/generators.md) for each generator and its parameters.

Points and anchors are fractions of the frame from its top-left corner. The
radius and the noise offset are fractions of the frame height. Cell size,
spacing, line width, and noise scale are pixels of a 1080-pixel-high frame, so a
proxy or a reduced preview draws the same picture.


## Effects

The Effects tab of Effect Controls lists the effect chain in render order. Each effect
has a header: **fx**, the effect's name, ↑ and ↓ to move it one step (hidden
at the ends of the chain), a reset button, a menu button, and a bin to remove
it.

- **fx** switches the effect off and back on, one undo step per press. A
  switched-off effect keeps its parameters and its place in the chain, renders
  nothing, and draws its name dimmed.
- Drag a header onto another effect to move it above that effect, or onto the
  rest of the tab to move it to the end of the chain. The move is one undo
  step.
- The reset button puts every parameter back at its default, in one undo step.
  The effect keeps its name and its **fx** state.
- The menu button, or a right click on the header, opens the effect menu:
  - **Reset** does what the reset button does.
  - **Rename Effect** opens the name field on the effect's name. Enter keeps
    the name; an empty name puts the catalog name back.
  - **Duplicate** places a copy with every value and keyframe below the effect,
    named with the next free number.
  - **Copy** holds the effect. **Paste Below** places the copy below the effect
    the menu was opened on; **Effect > Paste Effect** places it at the end of
    the chain of every selected clip that draws a picture.
  - **Save as Preset…** opens the preset name field over that effect alone.
  - **Remove Effect** takes the effect out of the chain.

  Rows a plugin adds follow, below a rule.

Parameters of the applied effect appear under its header. Each row reads its
value at the playhead.

- A numeric parameter is a value: drag it left or right to scrub it, or click it
  to type one. A point parameter is two rows, x and y, and a 3D point is three.
- A colour parameter prints its value beside the label, a preset's name or a
  hex such as `#ff0000`, with its swatches on the line under it. Click a
  swatch to set the colour.
- A parameter that can be animated has the stopwatch, the keyframe navigator,
  the reset button, and the time area described in
  [Property controls](keyframes-and-graph-editor.md#property-controls). Editing an animated parameter writes
  a keyframe at the playhead; editing a constant one changes the constant.

**Presets** holds the chains you saved. Click a preset to apply it to the
selected clip, replacing that clip's chain in one undo entry. **Delete** beside
a preset removes it.

**Save as preset** opens a name field over the current chain. Type a name and
press Enter, or click **Save**. A name that is empty or already taken is
refused; the field stays open with what you typed. Escape leaves without
saving. Presets are stored with your settings, so they are there on the next
launch.

**Add** lists every effect the compositor can render. Clicking one appends it
to the end of the chain.

The **Effect** menu lists every video effect and every audio effect, one
submenu per category: Blur & Sharpen, Color, Distort, Noise & Grain, and
Stylize for video; Amplitude & Compression, Channels, Delay & Echo, Filter & EQ,
Noise Reduction, Reverb, and Time & Pitch for audio. A plugin effect is listed
under the category its plugin specifies, or under Other or Other Audio. A video
effect row appends the effect to the selected clip's chain. An audio effect row
appends it to the chain of every selected clip with sound, as one undo step.

## Reference

See [Video effects](../reference/effects.md), [Generators](../reference/generators.md), [Controls](../reference/controls.md#inspector), and [Menus](../reference/menus.md#effect-menu) for every built-in effect and generator parameter.
