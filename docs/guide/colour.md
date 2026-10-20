# Colour

## Look and grade

The Look tab holds the filter catalog and, below it, the grade:

- The colour wheels: Lift, Gamma and Gain. Press anywhere on a wheel to put
  its puck under the pointer, then drag. The drag follows the pointer off the
  wheel and stops at the rim. A wheel moves the balance between its red, green
  and blue channels and leaves their level where the press found it; the
  **Master** value under the wheel sets the level and drags or types like any
  other value. The rim moves a channel a quarter for Lift and a half for Gamma
  and Gain. Releasing keeps the drag as one undo step, and Escape during the
  drag returns the wheel to where the press found it. The reset button beside
  the wheel's name returns its channels and master to neutral as one undo
  step.
- One row per grade property the catalog holds: the basic correction
  (temperature, tint, exposure, contrast, highlights, shadows, whites, blacks,
  brightness, saturation), the creative values (faded film, vibrance,
  sharpness, hue), the RGB curves, the hue curves, the twelve wheel values as
  numbers, the HSL secondary, and the vignette. Drag a row's value to change
  it, or click it to type one. **Reset grade** returns every adjustment to
  neutral.
- A blend mode for the layer. Normal composites source-over. In a matte the
  pair composites with the content clip's mode, and each side of a transition
  composites with its own clip's mode.
- Stabilize, offering the three smoothing levels and Off. The section is
  absent on a clip with no motion to smooth, such as a still.
- Denoise, which runs RNNoise over the clip's audio on export.

**Window > Color** opens the Color panel, which lays the same grade out in
sections: Basic Correction, Creative, RGB Curves, Hue Curves, Color Wheels, HSL
Secondary, and Vignette. Click a section's title to open or close it. Basic
Correction opens first and the rest start closed. **Reset grade** heads the
panel.

The Audio tab toggles pitch lock. Locked (the default), a retimed clip keeps its
original pitch: the sound is stretched in time and the pitch stays put. Unlocked,
pitch follows rate, so a fast clip sounds high and a slow one low. Both preview
and export mix the same way.


## Example

Colour-correct a video clip and apply a filter preset:

1. Select a video clip on `V1` and press <kbd>Shift+Alt+3</kbd> (`key-workspace-3`) or choose **Window > Workspaces > Color** (or open **Window > Color** and press <kbd>Shift+7</kbd> (`key-scopes`) / click [`preview-scopes-toggle`](../reference/controls.md#scopes)). The Color panel and Scopes panel open beside the Viewer.
2. In the Effect Controls **Look** tab, click a filter preset chip ([`look-choice`](../reference/controls.md#inspector)) such as **Warm** or **Cinema** to apply a base look, or click [`lut-load`](../reference/controls.md#inspector) to load a `.cube` 3D LUT file. The Viewer and the Scopes waveform and vectorscope update immediately to reflect the preset or LUT.
3. In the Color panel (**Window > Color**), click [`color-section`](../reference/controls.md#color) to expand **Basic Correction** and **Color Wheels**, drag the **Lift**, **Gamma**, and **Gain** pucks to balance shadows, midtones, and highlights, and scrub **Exposure**, **Contrast**, and **Saturation**. The clip's `fx` badge on the timeline turns yellow and the Scopes parade reflects the new tonal balance.
4. To return a single wheel to neutral, click [`inspector-wheel-reset`](../reference/controls.md#color) beside that wheel; to clear the entire grade back to neutral, click [`color-grade-clear`](../reference/controls.md#color) (or choose **Effect > Reset Grade** / run **Reset grade** (`pal-grade-reset`)). The Viewer shows the graded or reset frame at the playhead.

## Reference

See [Filters and color grading](../reference/filters.md), [Controls](../reference/controls.md#color), and [Menus](../reference/menus.md#effect-menu) for all 16 filter presets and 62 grading sliders.
