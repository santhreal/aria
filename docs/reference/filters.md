# Filters and color grading

Aria provides two color-grading mechanisms that share the same GPU color pipeline (`crates/aria-compositor/shaders/grade.wgsl` and `lut.wgsl`):

1. **Filter presets (`filters.toml`)**: 10 named color-grade recipes (`vivid`, `warm`, `cool`, `mono`, `fade`, `chrome`, `noir`, `sunset`, `forest`, `berry`) scaled by the clip's animatable `filter-intensity` parameter (`0.0..=1.0`, default `1.0`).
2. **Per-clip color adjustments (`ColorAdjustments::SLIDERS`)**: animatable color-grading sliders declared in `crates/aria-models/src/look/adjust_rows.rs`, plus 3D `.cube` LUT application (`lut-intensity`).

## Examples

### Apply a Sunset filter on a Filter track

1. Open **Window > Project**, select **Filters**, and click **Sunset** (`sunset`) with no clip selected to create a 5-second `Filter` generator clip on the filter track (`F1`), or select a video clip first to set its filter directly.
2. Open **Window > Effect Controls > Color** (or **Window > Lumetri Color**) to adjust **Filter intensity** (`filter-intensity`) or keyframe it between `0.0` and `1.0`.

### Override `filters.toml` to tune the Noir recipe

Place an override file at `<data dir>/catalogs/filters.toml` using any subset of the recipe keys documented in [Catalogs](../catalogs.md#filters):

```toml
[[entry]]
id = "noir"
label = "Noir"
saturation = -1.0
contrast = 0.45
brightness = -0.05
vignette = 0.30
```

## Where to apply and edit filters and color grades

- **Effects & Presets (`Window > Effects & Presets > Filters`)**: double-click a filter preset or drag it onto a clip.
- **Project panel (`Window > Project > Filters`)**: click an entry to apply it to the selected clip (or create a `Filter` track clip when no clip is selected).
- **Lumetri Color panel (`Window > Lumetri Color`)** and **Effect Controls (`Window > Effect Controls > Color` tab)**:
  - Pick a filter preset and drag **Filter intensity** (`0.0..=1.0`).
  - Load a `.cube` 3D LUT file and set **LUT intensity** (`0.0..=1.0`).
  - Adjust or keyframe any `ColorAdjustments::SLIDERS` color slider across `Basic`, `Tone`, `Detail`, `Lift / Gamma / Gain`, `Master / Red / Green / Blue Curve`, `Hue vs Sat / Hue / Luma`, and `HSL Secondary`.

## Filter presets (`filters.toml`)

Every entry in `crates/aria-models/data/filters.toml` defines a `FilterGrade` recipe. A clip's `filter-intensity` parameter (`0.0..=1.0`, default `1.0`) linearly scales additive recipe keys from `0.0` and multiplicative keys (`gamma_*`, `gain_*`) from `1.0`:

| ID | Label | Recipe keys (`filters.toml`) |
| --- | --- | --- |
| `vivid` | Vivid | `saturation = 0.45`, `contrast = 0.15` |
| `warm` | Warm | `temperature = 0.4`, `brightness = 0.05` |
| `cool` | Cool | `temperature = -0.4` |
| `mono` | Mono | `saturation = -1.0`, `contrast = 0.1` |
| `fade` | Fade | `contrast = -0.3`, `brightness = 0.1`, `saturation = -0.2` |
| `chrome` | Chrome | `contrast = 0.25`, `saturation = 0.1`, `brightness = 0.05` |
| `noir` | Noir | `saturation = -1.0`, `contrast = 0.35`, `brightness = -0.05` |
| `sunset` | Sunset | `temperature = 0.5`, `tint = 0.1`, `saturation = 0.15` |
| `forest` | Forest | `tint = -0.25`, `saturation = 0.2`, `brightness = 0.03` |
| `berry` | Berry | `tint = 0.35`, `temperature = -0.1`, `saturation = 0.25` |

### Vivid (`vivid`)

Boosts saturation (`+0.45`) and contrast (`+0.15`).

UI location: **Effects & Presets > Filters > Vivid**, **Project > Filters > Vivid**, and **Lumetri Color**.

### Warm (`warm`)

Shifts white balance toward amber (`temperature = 0.4`) with a slight brightness lift (`+0.05`).

UI location: **Effects & Presets > Filters > Warm**, **Project > Filters > Warm**, and **Lumetri Color**.

### Cool (`cool`)

Shifts white balance toward blue (`temperature = -0.4`).

UI location: **Effects & Presets > Filters > Cool**, **Project > Filters > Cool**, and **Lumetri Color**.

### Mono (`mono`)

Desaturates completely to monochrome (`saturation = -1.0`) and increases contrast (`+0.1`).

UI location: **Effects & Presets > Filters > Mono**, **Project > Filters > Mono**, and **Lumetri Color**.

### Fade (`fade`)

Lowers contrast (`-0.3`) and saturation (`-0.2`) while lifting brightness (`+0.1`).

UI location: **Effects & Presets > Filters > Fade**, **Project > Filters > Fade**, and **Lumetri Color**.

### Chrome (`chrome`)

Increases contrast (`+0.25`), saturation (`+0.1`), and brightness (`+0.05`).

UI location: **Effects & Presets > Filters > Chrome**, **Project > Filters > Chrome**, and **Lumetri Color**.

### Noir (`noir`)

Desaturates to monochrome (`saturation = -1.0`) with high contrast (`+0.35`) and darkened brightness (`-0.05`).

UI location: **Effects & Presets > Filters > Noir**, **Project > Filters > Noir**, and **Lumetri Color**.

### Sunset (`sunset`)

Applies warm golden-hour white balance (`temperature = 0.5`, `tint = 0.1`) and increases saturation (`+0.15`).

UI location: **Effects & Presets > Filters > Sunset**, **Project > Filters > Sunset**, and **Lumetri Color**.

### Forest (`forest`)

Shifts tint toward green (`tint = -0.25`) while raising saturation (`+0.2`) and brightness (`+0.03`).

UI location: **Effects & Presets > Filters > Forest**, **Project > Filters > Forest**, and **Lumetri Color**.

### Berry (`berry`)

Pushes tint toward magenta (`tint = 0.35`, `temperature = -0.1`) and increases saturation (`+0.25`).

UI location: **Effects & Presets > Filters > Berry**, **Project > Filters > Berry**, and **Lumetri Color**.

## Color grading sliders (`ColorAdjustments::SLIDERS`)

Declared in `crates/aria-models/src/look/adjust_rows.rs` (`ColorAdjustments::SLIDERS`) and evaluated in `crates/aria-compositor/shaders/grade.wgsl`:

| Catalog ID | Recipe key | Label | Kind | Min | Max | Default | Animates | Stage formula (`grade.wgsl`) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `adjust-brightness` | `brightness` | Brightness | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Adds `brightness * 0.25` to RGB. |
| `adjust-contrast` | `contrast` | Contrast | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Scales around mid-gray: `(c - 0.5) * (1.0 + contrast) + 0.5`. |
| `adjust-saturation` | `saturation` | Saturation | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Mixes Rec.709 luma (`0.2126 R + 0.7152 G + 0.0722 B`) with `c` by `1.0 + saturation`. |
| `adjust-exposure` | `exposure` | Exposure | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Multiplies RGB by `2^(2.0 * exposure)` (`±2` stops across `-1..=1`). |
| `adjust-temperature` | `temperature` | Temperature | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Adds `temperature * 0.25` to red and subtracts `temperature * 0.25` from blue. |
| `adjust-tint` | `tint` | Tint | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Adds `tint * 0.25` to green. |
| `adjust-hue` | `hue` | Hue | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Rotates YIQ chroma by `-hue * pi / 6` (`±30°` across `-1..=1`). |
| `adjust-highlights` | `highlights` | Highlights | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Applies `smoothstep(0.5, 1.0, luma) * highlights * 0.35` weighted by `1 - c` (positive) or `c` (negative). |
| `adjust-shadows` | `shadows` | Shadows | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Applies `(1.0 - smoothstep(0.0, 0.5, luma)) * shadows * 0.35` weighted by `1 - c` or `c`. |
| `adjust-whites` | `whites` | Whites | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Remaps white point `white_point = 1.0 - 0.25 * whites` (`(c - black_point) / (white_point - black_point)`). |
| `adjust-blacks` | `blacks` | Blacks | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Remaps black point `black_point = -0.25 * blacks`. |
| `adjust-sharpness` | `sharpness` | Sharpness | `scalar` | `0.0` | `1.0` | `0.0` | `yes` | 4-tap neighbor Laplacian unsharp pass applied after `apply_color_grade`. |
| `adjust-vignette` | `vignette` | Vignette | `scalar` | `0.0` | `1.0` | `0.0` | `yes` | Radial edge darkening applied after `apply_color_grade`. |
| `adjust-vibrance` | `vibrance` | Vibrance | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Scales chroma by `1.0 + vibrance * (1.0 - saturation_held)`, weighting low-chroma pixels higher. |
| `adjust-faded-film` | `faded_film` | Faded Film | `scalar` | `0.0` | `1.0` | `0.0` | `yes` | Lifts dark values via `c += faded_film * 0.3 * (1 - c)^2`, keeping `1.0` pinned. |
| `adjust-lift-r` | `lift_r` | Lift Red | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Red channel shadow lift (`c.r + (lift_r + lift_master) * (1 - c.r)`). |
| `adjust-lift-g` | `lift_g` | Lift Green | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Green channel shadow lift. |
| `adjust-lift-b` | `lift_b` | Lift Blue | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Blue channel shadow lift. |
| `adjust-lift-master` | `lift_master` | Lift Master | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Master shadow offset added to `lift_r`, `lift_g`, `lift_b`. |
| `adjust-gamma-r` | `gamma_r` | Gamma Red | `scalar` | `0.0` | `4.0` | `1.0` | `yes` | Red channel midtone power multiplier (`c^(1 / gamma)`). |
| `adjust-gamma-g` | `gamma_g` | Gamma Green | `scalar` | `0.0` | `4.0` | `1.0` | `yes` | Green channel gamma multiplier. |
| `adjust-gamma-b` | `gamma_b` | Gamma Blue | `scalar` | `0.0` | `4.0` | `1.0` | `yes` | Blue channel gamma multiplier. |
| `adjust-gamma-master` | `gamma_master` | Gamma Master | `scalar` | `0.0` | `4.0` | `1.0` | `yes` | Master midtone power multiplier (`gamma = vec3(gamma_r, gamma_g, gamma_b) * gamma_master`). |
| `adjust-gain-r` | `gain_r` | Gain Red | `scalar` | `0.0` | `4.0` | `1.0` | `yes` | Red channel highlight multiplier. |
| `adjust-gain-g` | `gain_g` | Gain Green | `scalar` | `0.0` | `4.0` | `1.0` | `yes` | Green channel highlight multiplier. |
| `adjust-gain-b` | `gain_b` | Gain Blue | `scalar` | `0.0` | `4.0` | `1.0` | `yes` | Blue channel highlight multiplier. |
| `adjust-gain-master` | `gain_master` | Gain Master | `scalar` | `0.0` | `4.0` | `1.0` | `yes` | Master highlight multiplier (`gain = vec3(gain_r, gain_g, gain_b) * gain_master`). |
| `adjust-curve-master-shadows` | `curve_master_shadows` | Master Curve Shadows | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Shifts the `x = 0.25` Catmull-Rom spline knot by `0.25 * v` across all RGB channels. |
| `adjust-curve-master-midtones` | `curve_master_midtones` | Master Curve Midtones | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Shifts the `x = 0.50` Catmull-Rom spline knot by `0.25 * v` across all RGB channels. |
| `adjust-curve-master-highlights` | `curve_master_highlights` | Master Curve Highlights | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Shifts the `x = 0.75` Catmull-Rom spline knot by `0.25 * v` across all RGB channels. |
| `adjust-curve-red-shadows` | `curve_red_shadows` | Red Curve Shadows | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Shifts the red channel `x = 0.25` spline knot by `0.25 * v`. |
| `adjust-curve-red-midtones` | `curve_red_midtones` | Red Curve Midtones | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Shifts the red channel `x = 0.50` spline knot by `0.25 * v`. |
| `adjust-curve-red-highlights` | `curve_red_highlights` | Red Curve Highlights | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Shifts the red channel `x = 0.75` spline knot by `0.25 * v`. |
| `adjust-curve-green-shadows` | `curve_green_shadows` | Green Curve Shadows | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Shifts the green channel `x = 0.25` spline knot by `0.25 * v`. |
| `adjust-curve-green-midtones` | `curve_green_midtones` | Green Curve Midtones | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Shifts the green channel `x = 0.50` spline knot by `0.25 * v`. |
| `adjust-curve-green-highlights` | `curve_green_highlights` | Green Curve Highlights | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Shifts the green channel `x = 0.75` spline knot by `0.25 * v`. |
| `adjust-curve-blue-shadows` | `curve_blue_shadows` | Blue Curve Shadows | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Shifts the blue channel `x = 0.25` spline knot by `0.25 * v`. |
| `adjust-curve-blue-midtones` | `curve_blue_midtones` | Blue Curve Midtones | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Shifts the blue channel `x = 0.50` spline knot by `0.25 * v`. |
| `adjust-curve-blue-highlights` | `curve_blue_highlights` | Blue Curve Highlights | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Shifts the blue channel `x = 0.75` spline knot by `0.25 * v`. |
| `adjust-hue-sat-red` | `hue_sat_red` | Red Saturation | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Saturation offset at the Red (`0°`) periodic hue knot. |
| `adjust-hue-sat-yellow` | `hue_sat_yellow` | Yellow Saturation | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Saturation offset at the Yellow (`60°`) hue knot. |
| `adjust-hue-sat-green` | `hue_sat_green` | Green Saturation | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Saturation offset at the Green (`120°`) hue knot. |
| `adjust-hue-sat-cyan` | `hue_sat_cyan` | Cyan Saturation | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Saturation offset at the Cyan (`180°`) hue knot. |
| `adjust-hue-sat-blue` | `hue_sat_blue` | Blue Saturation | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Saturation offset at the Blue (`240°`) hue knot. |
| `adjust-hue-sat-magenta` | `hue_sat_magenta` | Magenta Saturation | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Saturation offset at the Magenta (`300°`) hue knot. |
| `adjust-hue-hue-red` | `hue_hue_red` | Red Hue | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Hue rotation offset at the Red (`0°`) hue knot (`±60°` at `±1`). |
| `adjust-hue-hue-yellow` | `hue_hue_yellow` | Yellow Hue | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Hue rotation offset at the Yellow (`60°`) hue knot. |
| `adjust-hue-hue-green` | `hue_hue_green` | Green Hue | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Hue rotation offset at the Green (`120°`) hue knot. |
| `adjust-hue-hue-cyan` | `hue_hue_cyan` | Cyan Hue | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Hue rotation offset at the Cyan (`180°`) hue knot. |
| `adjust-hue-hue-blue` | `hue_hue_blue` | Blue Hue | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Hue rotation offset at the Blue (`240°`) hue knot. |
| `adjust-hue-hue-magenta` | `hue_hue_magenta` | Magenta Hue | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Hue rotation offset at the Magenta (`300°`) hue knot. |
| `adjust-hue-luma-red` | `hue_luma_red` | Red Luma | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Luma offset at the Red (`0°`) hue knot (`±0.25` at `±1`). |
| `adjust-hue-luma-yellow` | `hue_luma_yellow` | Yellow Luma | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Luma offset at the Yellow (`60°`) hue knot. |
| `adjust-hue-luma-green` | `hue_luma_green` | Green Luma | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Luma offset at the Green (`120°`) hue knot. |
| `adjust-hue-luma-cyan` | `hue_luma_cyan` | Cyan Luma | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Luma offset at the Cyan (`180°`) hue knot. |
| `adjust-hue-luma-blue` | `hue_luma_blue` | Blue Luma | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Luma offset at the Blue (`240°`) hue knot. |
| `adjust-hue-luma-magenta` | `hue_luma_magenta` | Magenta Luma | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Luma offset at the Magenta (`300°`) hue knot. |
| `adjust-hsl-key-hue` | `hsl_key_hue` | Key Hue | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Selects the target hue center (`centre = 0.5 * hsl_key_hue`, wrapping across `0..1`). |
| `adjust-hsl-key-width` | `hsl_key_width` | Key Range | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Sets the hue key half-width (`reach = (1.0 + hsl_key_width) * (30 / 360)` with `0.04` feather). |
| `adjust-hsl-hue` | `hsl_hue` | Secondary Hue | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Rotates hue inside the HSL secondary key window (`±60°` at `±1`). |
| `adjust-hsl-saturation` | `hsl_saturation` | Secondary Saturation | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Scales saturation inside the HSL secondary key window. |
| `adjust-hsl-luma` | `hsl_luma` | Secondary Luma | `scalar` | `-1.0` | `1.0` | `0.0` | `yes` | Offsets luminance inside the HSL secondary key window (`±0.25` at `±1`). |
