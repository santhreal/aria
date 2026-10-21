# Generators

Aria provides two layers of generated visual content:

1. **Clip content generators (`Generator` / `generator_specs()`)**: the eight `ClipSource::Generated` variants that define what a generated timeline clip holds (`text`, `solid`, `shape`, `sticker`, `lottie`, `effect`, `filter`, `adjustment`).
2. **GPU effect generators (`effects.toml` entries with `generator = true`)**: seven procedural shader passes (`color_matte`, `linear_gradient`, `radial_gradient`, `checkerboard`, `grid`, `fractal_noise`, `color_bars`) that synthesize pixels without reading source media.

## Examples

### Place an animated Fractal Noise background clip

1. Open **Window > Effects & Presets**, expand **Generators**, and drag **Fractal Noise** onto `V1` (or click **Fractal Noise** in **Window > Project > Effects > Generators** to place an `effect` generator clip at the playhead).
2. Select the generated clip and open **Window > Effect Controls > Effects**.
3. Set **Fractal Noise > Scale** to `160.0`, **Complexity** to `5.0`, and **Contrast** to `1.4`.
4. Move the playhead to the start of the clip, set **Evolution** to `0.0`, and click `◆`.
5. Move the playhead to the end of the clip and set **Evolution** to `4.0`.

### Override `effects.toml` to customize the Color Matte default

In `<data dir>/catalogs/effects.toml`, keep `generator = true` on `color_matte` and update its `default_color`:

```toml
[[entry]]
id = "color_matte"
label = "Color Matte"
generator = true
params = [{ name = "color", label = "Color", kind = "color", default_color = [18, 22, 28, 255] }]
```

## Where to place and edit generators

- **Effects & Presets (`Window > Effects & Presets > Generators`)**: lists the 7 GPU effect generators (`generator = true`). Double-clicking with no clip selected creates a 5-second `Generator::Effect` clip at the playhead with that generator as its first effect; double-clicking with a video clip selected appends the generator to that clip's effect chain.
- **Project panel (`Window > Project`)**:
  - **Effects > Generators**: lists plugin clip types (`clip_types()`) and the 7 GPU effect generators (`color_matte`, `linear_gradient`, `radial_gradient`, `checkerboard`, `grid`, `fractal_noise`, `color_bars`).
  - **Graphics > Titles**: places a `text` generator clip (`Generator::text("Title")`).
  - **Graphics > Shapes**: places a `shape` generator clip (`Rectangle` or `Ellipse`, `200 x 200` reference pixels).
  - **Graphics > Stickers**: places a `sticker` generator clip from `stickers.toml` (see [Stickers and masks](stickers-and-masks.md)).
- **Viewport tools (`T` Type, `R` Rectangle, `E` Ellipse, `P` Pen)**: clicking or dragging in the Program Monitor creates a `text` or `shape` generator clip at the playhead (see [Tools](tools.md)).
- **Sequence menu (`Sequence > Add Adjustment Layer`)**: places a 5-second `adjustment` generator clip at the playhead.

## Clip content generators

Every `ClipSource::Generated` clip resolves through `generator_specs()` (`crates/aria-models/src/clip/generator_catalog.rs`):

| ID | Label | Track kind | Renders with | Paints pixels | Parameter time | Animates |
| --- | --- | --- | --- | --- | --- | --- |
| `text` | Text | `text` | `text` | `yes` | `clip_local` | `yes` |
| `solid` | Color | `sticker` | `solid` | `yes` | `clip_local` | `no` |
| `shape` | Shape | `sticker` | `shape`, `path-shape` | `yes` | `clip_local` | `yes` |
| `sticker` | Sticker | `sticker` | `sticker` | `yes` | `clip_local` | `no` |
| `lottie` | Animation | `sticker` | `lottie` | `yes` | `clip_local` | `no` |
| `effect` | Effect | `effect` | `canvas-pass` | `no` | `sequence` | `no` |
| `filter` | Filter | `filter` | `canvas-pass` | `no` | `sequence` | `no` |
| `adjustment` | Adjustment | `adjustment` | `canvas-pass` | `no` | `sequence` | `no` |

- **Renders with**:
  - `text`, `solid`, `shape`, `path-shape`, `sticker`, `lottie`: rasterize or evaluate their own layer texture before clip effects run (`Shape::Path` resolves to `path-shape`; parametric shapes resolve to `shape`).
  - `canvas-pass`: reprocesses the composite beneath the track (`filter` and `adjustment`), or runs a generator effect chain over the canvas (`effect`).
- **Parameter time**: `clip_local` addresses keyframes in ticks from the clip's own start; `sequence` addresses keyframes in sequence ticks.
- **Animates**: when `yes`, the generator's own content fields carry clip-relative keyframes that rebase on split or trim (`TextStyle` on `text`, `Shape` / `ShapeStroke` on `shape`).

## Parametric shapes and animatable shape properties

A `shape` generator (`Generator::Shape`) draws one of eight geometry variants (`crates/aria-models/src/clip/generator.rs`, evaluated in `crates/aria-compositor/shaders/shape.wgsl` or rasterized as a cubic-bezier path):

| Shape variant | Geometry and signed-distance evaluation |
| --- | --- |
| `Rectangle` | Axis-aligned box of `width x height` reference pixels with corner radius `clamp(corner_radius, 0, min(half_w, half_h))`. |
| `Ellipse` | Axis-aligned ellipse fit to `width x height` (`length(p / radii) - 1.0` scaled by `min(rx, ry)`). |
| `Polygon { sides }` | Regular `sides`-gon (`3..=20` sides, `MAX_STAR_POINTS = 20`) inscribed in `min(width, height) * 0.5`. |
| `Star { points, inner_ratio }` | Symmetric `points`-spike star (`3..=20` points) with outer radius `min(width, height) * 0.5` and animatable `inner_ratio` (`0.0..=1.0`, default `0.5`). |
| `Line` | Horizontal round-capped capsule spanning `width` with thickness `height`. |
| `Arrow` | Right-pointing arrow filling `width x height` (shaft height `38%` of half-height, head base at `x = +10%` of half-width). |
| `Heart` | Upright symmetric heart fit to `width x height`. |
| `Path(ShapePath)` | Cubic-bezier outline (`2` or more `ShapePathPoint` anchors within `±8192.0` reference pixels); closed paths fill and stroke, open paths draw stroke only. |

### Animatable shape properties (`ClipParam::Shape`)

Configured in **Effect Controls > Shape** (`crates/aria-models/src/clip/param_catalog/rows.rs`):

| ID | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `shape-width` | Shape width | `scalar` | reference px | `> 0.0` | `8192.0` | `200.0` | `yes` |
| `shape-height` | Shape height | `scalar` | reference px | `> 0.0` | `8192.0` | `200.0` | `yes` |
| `shape-corner-radius` | Corner radius | `scalar` | reference px | `0.0` | `8192.0` | `0.0` | `yes` |
| `shape-inner-ratio` | Star inner ratio | `scalar` | ratio | `0.0` | `1.0` | `0.5` | `yes` |
| `shape-fill` | Shape fill | `color` | RGBA (`0..=255`) | `0` | `255` | `[255, 255, 255, 255]` | `yes` |
| `shape-stroke-width` | Shape stroke width | `scalar` | reference px | `0.0` | `512.0` | `4.0` | `yes` |
| `shape-stroke-color` | Shape stroke colour | `color` | RGBA (`0..=255`) | `0` | `255` | `[0, 0, 0, 255]` | `yes` |

## GPU effect generators

The 7 entries in `crates/aria-models/data/effects.toml` marked `generator = true` synthesize a full frame in the GPU effect pass:

| ID | Label | Category | Shader pass | Summary |
| --- | --- | --- | --- | --- |
| `color_matte` | Color Matte | Other | `effect_color_matte.wgsl` | Fills the frame with premultiplied `color`. |
| `linear_gradient` | Linear Gradient | Other | `effect_linear_gradient.wgsl` | Premultiplied linear ramp from `start_color` at `start` to `end_color` at `end` in pixel space. |
| `radial_gradient` | Radial Gradient | Other | `effect_radial_gradient.wgsl` | Premultiplied circular ramp from `inner_color` at `center` to `outer_color` at `radius * height`. |
| `checkerboard` | Checkerboard | Other | `effect_checkerboard.wgsl` | Alternates `color_a` and `color_b` on square cells of `size` reference pixels (at 1080p) around `anchor`. |
| `grid` | Grid | Other | `effect_grid.wgsl` | Draws anti-aliased `color` grid lines of `width` reference pixels every `spacing` reference pixels from `anchor`. |
| `fractal_noise` | Fractal Noise | Other | `effect_fractal_noise.wgsl` | Multi-octave (`1..=8`) 3D value fBm noise mapped between `dark_color` and `light_color`. |
| `color_bars` | Color Bars | Other | `effect_color_bars.wgsl` | SMPTE test pattern with top 7 bars at `level`, reverse blue castellations, and bottom `-I` / white / `+Q` / PLUGE section. |

### Color Matte (`color_matte`)

Clamps `color` to `0.0..=1.0` and outputs premultiplied `(color.rgb * color.a, color.a)` at every fragment (`effect_color_matte.wgsl`).

UI location: **Effects & Presets > Generators > Color Matte** and **Project > Effects > Generators > Color Matte**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `color` | Color | `color` | RGBA (`0..=255`) | `0` | `255` | `[128, 128, 128, 255]` | `yes` |

### Linear Gradient (`linear_gradient`)

Converts `start` and `end` from frame fractions to pixel coordinates (`start * dims`, `end * dims`), projects `uv * dims - start_px` onto `axis = end_px - start_px` (`t = clamp(dot(uv * dims - start_px, axis) / dot(axis, axis), 0.0, 1.0)`), and mixes premultiplied `start_color` and `end_color` by `t` (`effect_linear_gradient.wgsl`).

UI location: **Effects & Presets > Generators > Linear Gradient** and **Project > Effects > Generators > Linear Gradient**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `start_color` | Start Color | `color` | RGBA (`0..=255`) | `0` | `255` | `[0, 0, 0, 255]` | `yes` |
| `end_color` | End Color | `color` | RGBA (`0..=255`) | `0` | `255` | `[255, 255, 255, 255]` | `yes` |
| `start` | Start | `vec2` | frame fraction | `-1.0` | `2.0` | `[0.0, 0.5]` | `yes` |
| `end` | End | `vec2` | frame fraction | `-1.0` | `2.0` | `[1.0, 0.5]` | `yes` |

### Radial Gradient (`radial_gradient`)

Computes pixel distance from `center * dims` normalized by `r = max(radius * dims.y, 0.001)` (`t = clamp(length(uv * dims - center_px) / r, 0.0, 1.0)`) and mixes premultiplied `inner_color` and `outer_color` by `t` (`effect_radial_gradient.wgsl`).

UI location: **Effects & Presets > Generators > Radial Gradient** and **Project > Effects > Generators > Radial Gradient**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `inner_color` | Inner Color | `color` | RGBA (`0..=255`) | `0` | `255` | `[255, 255, 255, 255]` | `yes` |
| `outer_color` | Outer Color | `color` | RGBA (`0..=255`) | `0` | `255` | `[0, 0, 0, 255]` | `yes` |
| `center` | Center | `vec2` | frame fraction | `-1.0` | `2.0` | `[0.5, 0.5]` | `yes` |
| `radius` | Radius | `scalar` | frame height fraction | `0.0` | `4.0` | `0.5` | `yes` |

### Checkerboard (`checkerboard`)

Scales `size` by `dims.y / 1080.0` (`cell = max(size * dims.y / 1080.0, 0.001)`), computes cell indices `p = floor((uv - anchor) * dims / cell)`, and outputs premultiplied `color_a` when `(i32(p.x) + i32(p.y)) & 1 == 0` or `color_b` when odd (`effect_checkerboard.wgsl`).

UI location: **Effects & Presets > Generators > Checkerboard** and **Project > Effects > Generators > Checkerboard**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `color_a` | Color A | `color` | RGBA (`0..=255`) | `0` | `255` | `[255, 255, 255, 255]` | `yes` |
| `color_b` | Color B | `color` | RGBA (`0..=255`) | `0` | `255` | `[0, 0, 0, 255]` | `yes` |
| `size` | Cell Size | `scalar` | reference px | `1.0` | `1080.0` | `60.0` | `yes` |
| `anchor` | Anchor | `vec2` | frame fraction | `-1.0` | `2.0` | `[0.5, 0.5]` | `yes` |

### Grid (`grid`)

Scales `spacing` and `width` by `dims.y / 1080.0`, measures the pixel distance from `(uv - anchor) * dims` to the nearest multiple of `spacing` along X or Y, and anti-aliases line coverage via `clamp(width * 0.5 + 0.5 - distance, 0.0, 1.0) * min(width, 1.0)` over a transparent background (`effect_grid.wgsl`).

UI location: **Effects & Presets > Generators > Grid** and **Project > Effects > Generators > Grid**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `color` | Color | `color` | RGBA (`0..=255`) | `0` | `255` | `[255, 255, 255, 255]` | `yes` |
| `spacing` | Spacing | `scalar` | reference px | `2.0` | `1080.0` | `100.0` | `yes` |
| `width` | Line Width | `scalar` | reference px | `0.0` | `100.0` | `2.0` | `yes` |
| `anchor` | Anchor | `vec2` | frame fraction | `-1.0` | `2.0` | `[0.5, 0.5]` | `yes` |

### Fractal Noise (`fractal_noise`)

Evaluates up to `MAX_OCTAVES = 8` octaves of 3D Hermite-interpolated value noise (`effect_fractal_noise.wgsl`) at `((uv * (dims.x / dims.y, 1.0) + offset) * 1080.0 / scale, evolution)` with lacunarity `2.0`, persistence `0.5`, and fractional octave fade `clamp(complexity - octave, 0.0, 1.0)`. Maps `value = clamp((sum / norm - 0.5) * contrast + 0.5, 0.0, 1.0)` between premultiplied `dark_color` and `light_color`.

UI location: **Effects & Presets > Generators > Fractal Noise** and **Project > Effects > Generators > Fractal Noise**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `dark_color` | Dark Color | `color` | RGBA (`0..=255`) | `0` | `255` | `[0, 0, 0, 255]` | `yes` |
| `light_color` | Light Color | `color` | RGBA (`0..=255`) | `0` | `255` | `[255, 255, 255, 255]` | `yes` |
| `scale` | Scale | `scalar` | reference px | `4.0` | `2000.0` | `200.0` | `yes` |
| `evolution` | Evolution | `scalar` | phase | `0.0` | `1000.0` | `0.0` | `yes` |
| `complexity` | Complexity | `scalar` | octaves | `1.0` | `8.0` | `4.0` | `yes` |
| `contrast` | Contrast | `scalar` | multiplier | `0.0` | `4.0` | `1.0` | `yes` |
| `offset` | Offset | `vec2` | frame heights | `-10.0` | `10.0` | `[0.0, 0.0]` | `yes` |

### Color Bars (`color_bars`)

Synthesizes the three-band SMPTE color bar test pattern (`effect_color_bars.wgsl`):
- Top `2/3` (`uv.y < 2/3`): 7 vertical bars (`Grey`, `Yellow`, `Cyan`, `Green`, `Magenta`, `Red`, `Blue`) scaled by `clamp(level, 0.0, 1.0)`.
- Middle strip (`2/3 <= uv.y < 3/4`): reverse blue castellations (`Blue`, `Black`, `Magenta`, `Black`, `Cyan`, `Black`, `White`) scaled by `level`.
- Bottom `1/4` (`uv.y >= 3/4`): `-I` (`[0.0, 0.129, 0.298]`), `100%` white, `+Q` (`[0.196, 0.0, 0.416]`), black, and a `+4%` PLUGE step (`0.04` for `16/21 <= uv.x < 17/21`).

UI location: **Effects & Presets > Generators > Color Bars** and **Project > Effects > Generators > Color Bars**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `level` | Bar Level | `scalar` | normalized | `0.0` | `1.0` | `0.75` | `yes` |
