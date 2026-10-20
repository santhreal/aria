# Video effects

Video effects run as ordered post-transform GPU shader passes on a clip's composited image or on an adjustment layer (`RGBA8Unorm` ping-pong textures, or `RGBA16Float` when the project's GPU effect path runs in half-float). The built-in catalog is embedded from `crates/aria-models/data/effects.toml` and overridden at `<data dir>/catalogs/effects.toml` (see [Catalogs](../catalogs.md)). Generator entries in `effects.toml` (`generator = true`) are documented in [Generators](generators.md).

## Examples

### Keyframed glow and chromatic aberration on a title

1. Select a clip on a video track.
2. Open **Window > Effects & Presets**, expand **Video Effects > Stylize**, and double-click **Glow**.
3. Expand **Video Effects > Distort** and double-click **Chromatic Aberration**.
4. Open **Window > Effect Controls** and switch to the **Effects** tab.
5. Move the playhead to the start of the clip, set **Glow > Intensity** to `0.0` and **Chromatic Aberration > Amount** to `0.0`, and click `◆` on both rows.
6. Move the playhead 12 frames forward, set **Glow > Intensity** to `1.8` and **Threshold** to `0.65`, and set **Chromatic Aberration > Amount** to `0.45`.

### Override `effects.toml` to change default blur radius

Place an override file at `~/.aria/catalogs/effects.toml` (or `%APPDATA%\aria\catalogs\effects.toml` on Windows) that includes the `[[entry]]` array and sets a new default on `gaussian_blur`:

```toml
[[entry]]
id = "gaussian_blur"
label = "Gaussian Blur"
category = "Blur & Sharpen"
params = [{ name = "radius", label = "Radius", default = 12.0, min = 0.0, max = 64.0, unit = "px" }]
```

## Where to apply and edit video effects

- **Effects & Presets panel (`Window > Effects & Presets`)**: expand **Video Effects**, filter by name or category in the search field, and double-click an entry or drag it onto a timeline clip.
- **Project panel (`Window > Project`)**: select the **Effects > Effects** chip, then click an entry to append it to the selected clip or drag it onto a clip on the timeline.
- **Top menu bar (`Effect`)**: select a category submenu (**Blur & Sharpen**, **Color**, **Distort**, **Noise & Grain**, **Stylize**) and click an effect to append it to the selected clip.
- **Effect Controls (`Window > Effect Controls > Effects` tab)**:
  - Toggle the bypass button (`●` active, `○` bypassed) to skip an instance during rendering without removing its parameters or keyframes.
  - Click `↑` or `↓` to reorder passes in the chain, or `×` to remove the instance.
  - Drag a numeric readout horizontally or click `−` / `+` to step the value; click `◆` to insert or update a keyframe at the playhead (`◇` when no keyframe sits at the playhead).
  - Use **Save Preset** to store the clip's current effect chain to `<data dir>/effect-presets/<name>.toml`, **Apply Preset** to append a saved chain, or **Clear All** to empty the stack.
- **Adjustment layers (`Sequence > Add Adjustment Layer`)**: effects placed on an adjustment clip process the composite of all video tracks below that adjustment track over the clip's timeline span.

## Catalog entries

| ID | Label | Category | Shader pass | Summary |
| --- | --- | --- | --- | --- |
| `gaussian_blur` | Gaussian Blur | Blur & Sharpen | `effect_blur_h.wgsl` + `effect_blur_v.wgsl` | Two-pass separable box blur using bilinear midpoint pairing (`MAX_RADIUS = 32` texels per pass). |
| `vignette` | Vignette | Stylize | `effect_vignette.wgsl` | Radial edge darkening via `1.0 - amount * smoothstep(0.35, 1.0, length(uv - 0.5) * 1.41421356)`. |
| `sharpen` | Sharpen | Blur & Sharpen | `effect_sharpen.wgsl` | 5-tap cross Laplacian unsharp mask (`centre.rgb + (4 * centre.rgb - neighbours) * amount`). |
| `pixelate` | Pixelate | Stylize | `effect_pixelate.wgsl` | Quantizes UV coordinates into square cells of `max(size, 1.0)` pixels. |
| `glitch` | Glitch | Stylize | `effect_glitch.wgsl` | 24-band horizontal scanline tear (`MAX_SLIDE = 0.08`) with `35%` red/blue channel slip. |
| `chromatic_aberration` | Chromatic Aberration | Distort | `effect_chromatic_aberration.wgsl` | Radial red/blue channel separation from frame center (`MAX_SHIFT = 0.02`). |
| `grain` | Film Grain | Noise & Grain | `effect_grain.wgsl` | Zero-mean per-pixel hash noise scaled by `amount * 0.25 * src.a`. |
| `glow` | Glow | Stylize | `effect_glow.wgsl` | Two 8-tap radial rings (`4` and `8` texels) collecting Rec.709 luminance above `threshold`. |
| `zoom_blur` | Zoom Blur | Blur & Sharpen | `effect_zoom_blur.wgsl` | 12-tap radial streak toward `(0.5, 0.5)` covering up to `30%` of the distance to center. |
| `mirror` | Mirror | Distort | `effect_mirror.wgsl` | Reflects one half of the frame onto the other along `mode` (`0` left, `1` right, `2` top, `3` bottom). |
| `color_overlay` | Color Overlay | Color | `effect_color_overlay.wgsl` | Samples at `uv - offset` and mixes straight RGB toward `color.rgb` by `amount * color.a`. |
| `duotone` | Duotone | Color | `effect_duotone.wgsl` | Maps BT.601 luminance (`0.299 R + 0.587 G + 0.114 B`) from `shadow_color` to `highlight_color`, mixed by `intensity`. |

### Gaussian Blur (`gaussian_blur`)

Applies a two-pass separable box blur: horizontal pass (`effect_blur_h.wgsl`) followed by vertical pass (`effect_blur_v.wgsl`). When `radius <= 0.0`, the shader returns the center sample unchanged. For `radius > 0.0`, `r = min(i32(radius), 32)` pairs adjacent texels `(2k - 1, 2k)` at bilinear midpoint `2k - 0.5` so each filtered read averages two texels, plus an unpaired edge tap when `r` is odd.

UI location: **Effects & Presets > Video Effects > Blur & Sharpen > Gaussian Blur**, **Project > Effects > Effects > Gaussian Blur**, and **Effect > Blur & Sharpen > Gaussian Blur**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `radius` | Radius | `scalar` | px | `0.0` | `64.0` | `4.0` | `yes` |

### Vignette (`vignette`)

Computes radial distance from the frame center `dist = length(uv - (0.5, 0.5)) * 1.41421356` and multiplies premultiplied `color.rgb` by `1.0 - clamp(amount, 0.0, 1.0) * smoothstep(0.35, 1.0, dist)`, preserving `color.a` (`effect_vignette.wgsl`).

UI location: **Effects & Presets > Video Effects > Stylize > Vignette**, **Project > Effects > Effects > Vignette**, and **Effect > Stylize > Vignette**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `amount` | Amount | `scalar` | normalized | `0.0` | `1.0` | `0.6` | `yes` |

### Sharpen (`sharpen`)

Evaluates a 5-tap cross Laplacian unsharp mask (`effect_sharpen.wgsl`) over the center texel and its four cardinal neighbors (`±step.x`, `±step.y`): `laplacian = centre.rgb * 4.0 - (l.rgb + r.rgb + u.rgb + d.rgb)`. Clamps `centre.rgb + laplacian * max(amount, 0.0)` to `0.0..=centre.a` so premultiplied channels stay within alpha coverage.

UI location: **Effects & Presets > Video Effects > Blur & Sharpen > Sharpen**, **Project > Effects > Effects > Sharpen**, and **Effect > Blur & Sharpen > Sharpen**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `amount` | Amount | `scalar` | multiplier | `0.0` | `3.0` | `0.5` | `yes` |

### Pixelate (`pixelate`)

Quantizes UV coordinates to square cell centers of `cell = max(size, 1.0)` pixels: `px = floor(uv * dims / cell) * cell + cell * 0.5` and samples `input_tex` at `px / dims` (`effect_pixelate.wgsl`).

UI location: **Effects & Presets > Video Effects > Stylize > Pixelate**, **Project > Effects > Effects > Pixelate**, and **Effect > Stylize > Pixelate**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `size` | Cell Size | `scalar` | px | `1.0` | `256.0` | `8.0` | `yes` |

### Glitch (`glitch`)

Cuts the frame into `BANDS = 24.0` horizontal rows (`band = floor(uv.y * 24.0)`). When `hash((band, seed)) < amount`, shifts the band horizontally by `slide = (hash((band + 1.0, seed * 1.7 + 3.0)) - 0.5) * 2.0 * 0.08 * amount` and slips red (`base + slide * 0.35`) and blue (`base - slide * 0.35`) against green (`effect_glitch.wgsl`).

UI location: **Effects & Presets > Video Effects > Stylize > Glitch**, **Project > Effects > Effects > Glitch**, and **Effect > Stylize > Glitch**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `amount` | Amount | `scalar` | normalized | `0.0` | `1.0` | `0.5` | `yes` |
| `seed` | Seed | `scalar` | seed | `0.0` | `1000.0` | `0.0` | `yes` |

### Chromatic Aberration (`chromatic_aberration`)

Splits channels radially around the frame center `(0.5, 0.5)` (`effect_chromatic_aberration.wgsl`): computes `shift = (uv - (0.5, 0.5)) * (clamp(amount, 0.0, 1.0) * 0.04)`, samples red at `uv + shift`, green and alpha at `uv`, and blue at `uv - shift`.

UI location: **Effects & Presets > Video Effects > Distort > Chromatic Aberration**, **Project > Effects > Effects > Chromatic Aberration**, and **Effect > Distort > Chromatic Aberration**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `amount` | Amount | `scalar` | normalized | `0.0` | `1.0` | `0.4` | `yes` |

### Film Grain (`grain`)

Computes deterministic zero-mean per-pixel hash noise `noise = (hash(floor(uv * dims) + (seed, seed * 0.37)) - 0.5) * 2.0`, scales by `swing = noise * clamp(amount, 0.0, 1.0) * 0.25 * src.a`, and clamps `src.rgb + swing` to `0.0..=src.a` so transparent areas remain transparent (`effect_grain.wgsl`).

UI location: **Effects & Presets > Video Effects > Noise & Grain > Film Grain**, **Project > Effects > Effects > Film Grain**, and **Effect > Noise & Grain > Film Grain**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `amount` | Amount | `scalar` | normalized | `0.0` | `1.0` | `0.3` | `yes` |
| `seed` | Seed | `scalar` | seed | `0.0` | `1000.0` | `0.0` | `yes` |

### Glow (`glow`)

Samples two concentric 8-tap rings at `4.0` and `8.0` texels around `uv` (`effect_glow.wgsl`). Each tap extracts Rec.709 luminance above `threshold` (`over = max(luma - threshold, 0.0) / max(1.0 - threshold, 0.001)`), averages the 16 taps into `spill`, and clamps `src.rgb + spill * max(intensity, 0.0)` to `0.0..=src.a`.

UI location: **Effects & Presets > Video Effects > Stylize > Glow**, **Project > Effects > Effects > Glow**, and **Effect > Stylize > Glow**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `threshold` | Threshold | `scalar` | luminance | `0.0` | `1.0` | `0.7` | `yes` |
| `intensity` | Intensity | `scalar` | multiplier | `0.0` | `4.0` | `0.8` | `yes` |

### Zoom Blur (`zoom_blur`)

Averages `SAMPLES = 12` evenly spaced taps along the radial vector from `uv` toward `(0.5, 0.5)` (`toward = ((0.5, 0.5) - uv) * (clamp(amount, 0.0, 1.0) * 0.3)`, `effect_zoom_blur.wgsl`).

UI location: **Effects & Presets > Video Effects > Blur & Sharpen > Zoom Blur**, **Project > Effects > Effects > Zoom Blur**, and **Effect > Blur & Sharpen > Zoom Blur**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `amount` | Amount | `scalar` | normalized | `0.0` | `1.0` | `0.5` | `yes` |

### Mirror (`mirror`)

Rounds `mode` to an integer in `0..=3` (`effect_mirror.wgsl`):
- `0`: mirrors the left half onto the right (`uv.x = min(uv.x, 1.0 - uv.x)`).
- `1`: mirrors the right half onto the left (`uv.x = max(uv.x, 1.0 - uv.x)`).
- `2`: mirrors the top half onto the bottom (`uv.y = min(uv.y, 1.0 - uv.y)`).
- `3`: mirrors the bottom half onto the top (`uv.y = max(uv.y, 1.0 - uv.y)`).

UI location: **Effects & Presets > Video Effects > Distort > Mirror**, **Project > Effects > Effects > Mirror**, and **Effect > Distort > Mirror**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `mode` | Mode | `scalar` | mode index (`0..=3`) | `0.0` | `3.0` | `0.0` | `yes` |

### Color Overlay (`color_overlay`)

Samples the input texture at `uv - offset`, divides out alpha (`straight = src.rgb / src.a`), mixes toward `color.rgb` by `clamp(amount * color.a, 0.0, 1.0)`, and repremultiplies by `src.a` (`effect_color_overlay.wgsl`).

UI location: **Effects & Presets > Video Effects > Color > Color Overlay**, **Project > Effects > Effects > Color Overlay**, and **Effect > Color > Color Overlay**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `color` | Color | `color` | RGBA (`0..=255`) | `0` | `255` | `[255, 0, 0, 255]` | `yes` |
| `offset` | Offset | `vec2` | UV fraction | `-1.0` | `1.0` | `[0.0, 0.0]` | `yes` |
| `amount` | Amount | `scalar` | normalized | `0.0` | `1.0` | `0.5` | `yes` |

### Duotone (`duotone`)

Unpremultiplies `src.rgb / src.a`, computes luminance `luma = dot(straight, (0.299, 0.587, 0.114))`, interpolates `duo = mix(shadow_color.rgb, highlight_color.rgb, clamp(luma, 0.0, 1.0))`, mixes `straight` toward `duo` by `clamp(intensity, 0.0, 1.0)`, and repremultiplies by `src.a` (`effect_duotone.wgsl`).

UI location: **Effects & Presets > Video Effects > Color > Duotone**, **Project > Effects > Effects > Duotone**, and **Effect > Color > Duotone**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `shadow_color` | Shadow | `color` | RGBA (`0..=255`) | `0` | `255` | `[20, 16, 60, 255]` | `yes` |
| `highlight_color` | Highlight | `color` | RGBA (`0..=255`) | `0` | `255` | `[255, 220, 160, 255]` | `yes` |
| `intensity` | Intensity | `scalar` | normalized | `0.0` | `1.0` | `1.0` | `yes` |
