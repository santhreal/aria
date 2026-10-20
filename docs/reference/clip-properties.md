# Clip Properties, Transform, Alignment, Text, and Shapes Reference

Every animatable clip property in Aria is registered in `crates/aria-models/src/clip/param_catalog.rs` (`clip_param_catalog()`) and dispatched through `ClipParam` (`crates/aria-models/src/clip/transform.rs` and `crates/aria-models/src/clip/param_axis.rs`). Structural clip properties and UI alignment/framing controls are defined in `crates/aria-models/src/clip/crop.rs`, `crates/aria-models/src/look/motion_blur.rs`, `crates/aria-models/src/clip/text.rs`, `crates/aria-models/src/clip/generator.rs`, `crates/aria-models/src/clip/generator_catalog.rs`, `crates/aria-ui/src/app/clip_frame.rs`, and `crates/aria-ui/src/app/align.rs`.

## `ClipParam` variants and classification (`ClipParam::CENSUS` and `ClipParamClass`)

`ClipParam::CENSUS` lists one representative value for each of the 15 `ClipParam` variants in declaration order. `ClipParam::class()` classifies each variant as:
- `Direct(&'static str)` for a single clip property with one static row in `clip_param_catalog()`.
- `Delegated(ParamAxis)` for a wrapper over a sub-enum (`ParamAxis::Look`, `ParamAxis::Style`, `ParamAxis::Text`, or `ParamAxis::Shape`), where each member of the sub-enum (`LookParam::ALL`, `StyleParam::ALL`, `TextParam::ALL`, `ShapeParam::ALL`) has its own row in `clip_param_catalog()`.
- `Excluded(ParamExclusion)` for a parameter with no static row in `clip_param_catalog()`: `ParamExclusion::SpeedCurve` (edited via `EditCommand::SetClipSpeedCurve` on `0..=SPEED_CURVE_SCALE` with `SPEED_CURVE_SCALE = 1000` and speed bounds `MIN_SPEED = 0.05` to `MAX_SPEED = 100.0`), `ParamExclusion::EffectSlot` (built per effect instance via `clip_params_for`), and `ParamExclusion::MaskSlot` (built per mask via `clip_params_for` over the 7 `MaskParam::ALL` variants).

| Variant | Class | Scope | Kind | Easing Preset (`supports_easing_preset`) |
| --- | --- | --- | --- | --- |
| `Position` | `Direct("position")` | `Visual` | `Vec2` | `true` |
| `AnchorPoint` | `Direct("anchor-point")` | `Visual` | `Vec2` | `true` |
| `Scale` | `Direct("scale")` | `Visual` | `Vec2` | `true` |
| `Rotation` | `Direct("rotation")` | `Visual` | `Scalar` | `true` |
| `Opacity` | `Direct("opacity")` | `Visual` | `Scalar` | `true` |
| `Crop` | `Direct("crop")` | `Visual` | `Rect` | `false` |
| `Speed` | `Excluded(SpeedCurve)` | `Visual` | `Scalar` | `false` |
| `Volume` | `Direct("volume")` | `Audio` | `Scalar` | `true` |
| `Pan` | `Direct("pan")` | `Audio` | `Scalar` | `true` |
| `Effect` | `Excluded(EffectSlot)` | `Visual` | `None` | `false` |
| `Shape` | `Delegated(Shape)` | `Visual` | `Scalar` | `false` |
| `Text` | `Delegated(Text)` | `Visual` | `Scalar` | `false` |
| `Look` | `Delegated(Look)` | `Visual` | `Scalar` | `true` |
| `Mask` | `Excluded(MaskSlot)` | `Visual` | `Scalar` | `true` |
| `Style` | `Delegated(Style)` | `Visual` | `Color` | `false` |

## Static animatable clip parameter catalog (`clip_param_catalog()`)

`clip_param_catalog()` returns all 104 static `ParamSpec` rows in browse order: 8 direct rows, 67 `LookParam` rows (`filter`, `lut`, `grade`, `chroma`, `matte`), 11 `StyleParam` rows (`shadow`, `glow`, `outline`, `background`), 11 `TextParam` rows (`text`), and 7 `ShapeParam` rows (`shape`).

| Catalog ID | Label | Group | `ClipParam` | Kind | Scope | Range |
| --- | --- | --- | --- | --- | --- | --- |
| `position` | Position | `transform` | `Position` | `Vec2` | `Visual` | `any` |
| `anchor-point` | Anchor point | `transform` | `AnchorPoint` | `Vec2` | `Visual` | `any` |
| `scale` | Scale | `transform` | `Scale` | `Vec2` | `Visual` | `> 0` |
| `rotation` | Rotation | `transform` | `Rotation` | `Scalar` | `Visual` | `any` |
| `opacity` | Opacity | `transform` | `Opacity` | `Scalar` | `Visual` | `0..=1` |
| `crop` | Crop | `framing` | `Crop` | `Rect` | `Visual` | `any` |
| `volume` | Volume | `audio` | `Volume` | `Scalar` | `Audio` | `0..=10` |
| `pan` | Pan | `audio` | `Pan` | `Scalar` | `Audio` | `-1..=1` |
| `filter-intensity` | Filter amount | `filter` | `Look(FilterIntensity)` | `Scalar` | `Visual` | `0..=1` |
| `lut-intensity` | LUT amount | `lut` | `Look(LutIntensity)` | `Scalar` | `Visual` | `0..=1` |
| `adjust-brightness` | Brightness | `grade` | `Look(AdjustBrightness)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-contrast` | Contrast | `grade` | `Look(AdjustContrast)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-saturation` | Saturation | `grade` | `Look(AdjustSaturation)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-exposure` | Exposure | `grade` | `Look(AdjustExposure)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-temperature` | Temperature | `grade` | `Look(AdjustTemperature)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-tint` | Tint | `grade` | `Look(AdjustTint)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue` | Hue | `grade` | `Look(AdjustHue)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-highlights` | Highlights | `grade` | `Look(AdjustHighlights)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-shadows` | Shadows | `grade` | `Look(AdjustShadows)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-whites` | Whites | `grade` | `Look(AdjustWhites)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-blacks` | Blacks | `grade` | `Look(AdjustBlacks)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-sharpness` | Sharpness | `grade` | `Look(AdjustSharpness)` | `Scalar` | `Visual` | `0..=1` |
| `adjust-vignette` | Vignette | `grade` | `Look(AdjustVignette)` | `Scalar` | `Visual` | `0..=1` |
| `adjust-vibrance` | Vibrance | `grade` | `Look(AdjustVibrance)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-faded-film` | Faded Film | `grade` | `Look(AdjustFadedFilm)` | `Scalar` | `Visual` | `0..=1` |
| `adjust-lift-r` | Lift Red | `grade` | `Look(AdjustLiftR)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-lift-g` | Lift Green | `grade` | `Look(AdjustLiftG)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-lift-b` | Lift Blue | `grade` | `Look(AdjustLiftB)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-lift-master` | Lift Master | `grade` | `Look(AdjustLiftMaster)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-gamma-r` | Gamma Red | `grade` | `Look(AdjustGammaR)` | `Scalar` | `Visual` | `0..=4` |
| `adjust-gamma-g` | Gamma Green | `grade` | `Look(AdjustGammaG)` | `Scalar` | `Visual` | `0..=4` |
| `adjust-gamma-b` | Gamma Blue | `grade` | `Look(AdjustGammaB)` | `Scalar` | `Visual` | `0..=4` |
| `adjust-gamma-master` | Gamma Master | `grade` | `Look(AdjustGammaMaster)` | `Scalar` | `Visual` | `0..=4` |
| `adjust-gain-r` | Gain Red | `grade` | `Look(AdjustGainR)` | `Scalar` | `Visual` | `0..=4` |
| `adjust-gain-g` | Gain Green | `grade` | `Look(AdjustGainG)` | `Scalar` | `Visual` | `0..=4` |
| `adjust-gain-b` | Gain Blue | `grade` | `Look(AdjustGainB)` | `Scalar` | `Visual` | `0..=4` |
| `adjust-gain-master` | Gain Master | `grade` | `Look(AdjustGainMaster)` | `Scalar` | `Visual` | `0..=4` |
| `adjust-curve-master-shadows` | Master Curve Shadows | `grade` | `Look(AdjustCurveMasterShadows)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-curve-master-midtones` | Master Curve Midtones | `grade` | `Look(AdjustCurveMasterMidtones)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-curve-master-highlights` | Master Curve Highlights | `grade` | `Look(AdjustCurveMasterHighlights)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-curve-red-shadows` | Red Curve Shadows | `grade` | `Look(AdjustCurveRedShadows)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-curve-red-midtones` | Red Curve Midtones | `grade` | `Look(AdjustCurveRedMidtones)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-curve-red-highlights` | Red Curve Highlights | `grade` | `Look(AdjustCurveRedHighlights)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-curve-green-shadows` | Green Curve Shadows | `grade` | `Look(AdjustCurveGreenShadows)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-curve-green-midtones` | Green Curve Midtones | `grade` | `Look(AdjustCurveGreenMidtones)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-curve-green-highlights` | Green Curve Highlights | `grade` | `Look(AdjustCurveGreenHighlights)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-curve-blue-shadows` | Blue Curve Shadows | `grade` | `Look(AdjustCurveBlueShadows)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-curve-blue-midtones` | Blue Curve Midtones | `grade` | `Look(AdjustCurveBlueMidtones)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-curve-blue-highlights` | Blue Curve Highlights | `grade` | `Look(AdjustCurveBlueHighlights)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-sat-red` | Red Saturation | `grade` | `Look(AdjustHueSatRed)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-sat-yellow` | Yellow Saturation | `grade` | `Look(AdjustHueSatYellow)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-sat-green` | Green Saturation | `grade` | `Look(AdjustHueSatGreen)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-sat-cyan` | Cyan Saturation | `grade` | `Look(AdjustHueSatCyan)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-sat-blue` | Blue Saturation | `grade` | `Look(AdjustHueSatBlue)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-sat-magenta` | Magenta Saturation | `grade` | `Look(AdjustHueSatMagenta)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-hue-red` | Red Hue | `grade` | `Look(AdjustHueHueRed)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-hue-yellow` | Yellow Hue | `grade` | `Look(AdjustHueHueYellow)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-hue-green` | Green Hue | `grade` | `Look(AdjustHueHueGreen)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-hue-cyan` | Cyan Hue | `grade` | `Look(AdjustHueHueCyan)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-hue-blue` | Blue Hue | `grade` | `Look(AdjustHueHueBlue)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-hue-magenta` | Magenta Hue | `grade` | `Look(AdjustHueHueMagenta)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-luma-red` | Red Luma | `grade` | `Look(AdjustHueLumaRed)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-luma-yellow` | Yellow Luma | `grade` | `Look(AdjustHueLumaYellow)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-luma-green` | Green Luma | `grade` | `Look(AdjustHueLumaGreen)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-luma-cyan` | Cyan Luma | `grade` | `Look(AdjustHueLumaCyan)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-luma-blue` | Blue Luma | `grade` | `Look(AdjustHueLumaBlue)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hue-luma-magenta` | Magenta Luma | `grade` | `Look(AdjustHueLumaMagenta)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hsl-key-hue` | Key Hue | `grade` | `Look(AdjustHslKeyHue)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hsl-key-width` | Key Range | `grade` | `Look(AdjustHslKeyWidth)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hsl-hue` | Secondary Hue | `grade` | `Look(AdjustHslHue)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hsl-saturation` | Secondary Saturation | `grade` | `Look(AdjustHslSaturation)` | `Scalar` | `Visual` | `-1..=1` |
| `adjust-hsl-luma` | Secondary Luma | `grade` | `Look(AdjustHslLuma)` | `Scalar` | `Visual` | `-1..=1` |
| `chroma-strength` | Key strength | `chroma` | `Look(ChromaStrength)` | `Scalar` | `Visual` | `0..=1` |
| `chroma-shadow` | Key shadow | `chroma` | `Look(ChromaShadow)` | `Scalar` | `Visual` | `0..=1` |
| `matte-softness` | Matte softness | `matte` | `Look(MatteSoftness)` | `Scalar` | `Visual` | `0..=1` |
| `style-shadow-color` | Shadow colour | `shadow` | `Style(ShadowColor)` | `Color` | `Visual` | `any` |
| `style-shadow-offset` | Shadow offset | `shadow` | `Style(ShadowOffset)` | `Vec2` | `Visual` | `any` |
| `style-shadow-blur` | Shadow blur | `shadow` | `Style(ShadowBlur)` | `Scalar` | `Visual` | `>= 0` |
| `style-glow-color` | Glow colour | `glow` | `Style(GlowColor)` | `Color` | `Visual` | `any` |
| `style-glow-radius` | Glow radius | `glow` | `Style(GlowRadius)` | `Scalar` | `Visual` | `>= 0` |
| `style-glow-intensity` | Glow intensity | `glow` | `Style(GlowIntensity)` | `Scalar` | `Visual` | `0..=4` |
| `style-outline-color` | Outline colour | `outline` | `Style(OutlineColor)` | `Color` | `Visual` | `any` |
| `style-outline-width` | Outline width | `outline` | `Style(OutlineWidth)` | `Scalar` | `Visual` | `>= 0` |
| `style-background-color` | Background colour | `background` | `Style(BackgroundColor)` | `Color` | `Visual` | `any` |
| `style-background-padding` | Background padding | `background` | `Style(BackgroundPadding)` | `Scalar` | `Visual` | `>= 0` |
| `style-background-radius` | Background radius | `background` | `Style(BackgroundRadius)` | `Scalar` | `Visual` | `>= 0` |
| `text-size` | Text size | `text` | `Text(Size)` | `Scalar` | `Visual` | `> 0..=4096` |
| `text-fill` | Text colour | `text` | `Text(Fill)` | `Color` | `Visual` | `any` |
| `text-letter-spacing` | Letter spacing | `text` | `Text(LetterSpacing)` | `Scalar` | `Visual` | `-4096..=4096` |
| `text-line-spacing` | Line spacing | `text` | `Text(LineSpacing)` | `Scalar` | `Visual` | `> 0..=100` |
| `text-stroke-width` | Text stroke width | `text` | `Text(StrokeWidth)` | `Scalar` | `Visual` | `0..=512` |
| `text-stroke-color` | Text stroke colour | `text` | `Text(StrokeColor)` | `Color` | `Visual` | `any` |
| `text-shadow-blur` | Text shadow blur | `text` | `Text(ShadowBlur)` | `Scalar` | `Visual` | `0..=1` |
| `text-shadow-distance` | Text shadow distance | `text` | `Text(ShadowDistance)` | `Scalar` | `Visual` | `0..=4096` |
| `text-shadow-color` | Text shadow colour | `text` | `Text(ShadowColor)` | `Color` | `Visual` | `any` |
| `text-background-color` | Text card colour | `text` | `Text(BackgroundColor)` | `Color` | `Visual` | `any` |
| `text-background-radius` | Text card radius | `text` | `Text(BackgroundRadius)` | `Scalar` | `Visual` | `0..=1` |
| `shape-width` | Shape width | `shape` | `Shape(Width)` | `Scalar` | `Visual` | `> 0..=8192` |
| `shape-height` | Shape height | `shape` | `Shape(Height)` | `Scalar` | `Visual` | `> 0..=8192` |
| `shape-corner-radius` | Corner radius | `shape` | `Shape(CornerRadius)` | `Scalar` | `Visual` | `0..=8192` |
| `shape-inner-ratio` | Star inner ratio | `shape` | `Shape(InnerRatio)` | `Scalar` | `Visual` | `0..=1` |
| `shape-fill` | Shape fill | `shape` | `Shape(Fill)` | `Color` | `Visual` | `any` |
| `shape-stroke-width` | Shape stroke width | `shape` | `Shape(StrokeWidth)` | `Scalar` | `Visual` | `0..=512` |
| `shape-stroke-color` | Shape stroke colour | `shape` | `Shape(StrokeColor)` | `Color` | `Visual` | `any` |

## Clip transform, framing, and motion blur (`ClipTransform`, `CropRect`, `MotionBlur`)

### `ClipTransform` and `Scale2`

`ClipTransform::IDENTITY` (`crates/aria-models/src/clip/transform.rs`) initializes:
- `position = [0.0, 0.0]` (offset of the anchor point from canvas center in canvas-normalized coordinates, `+x` right, `+y` down)
- `anchor_point = [0.5, 0.5]` (pivot inside placed content bounds, `[0.0, 0.0]` top-left, `[0.5, 0.5]` center)
- `scale = Scale2::ONE` (`Scale2 { x: 1.0, y: 1.0 }`; both axes must be finite and `> 0.0`, with `isotropic() = (x.abs() * y.abs()).sqrt()`)
- `rotation = 0.0` (clockwise degrees about `anchor_point`)
- `opacity = 1.0` (`0.0..=1.0`)

### `CropRect` and `CROP_EDGES`

`CropRect` (`crates/aria-models/src/clip/crop.rs`) stores the kept content window `{ x, y, w, h }` in normalized content fractions (`CropRect::FULL = { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }`). Both `w` and `h` must be at least `MIN_CROP_FRACTION = 0.01`, with `x >= 0.0`, `y >= 0.0`, `x + w <= 1.0`, and `y + h <= 1.0`.

In `aria-ui`, `CROP_EDGES` (`crates/aria-ui/src/app/clip_frame.rs`) defines the four inspector inset controls:

| Control ID | Label | Inset Formula |
| --- | --- | --- |
| `crop-left` | Left | `crop.x` |
| `crop-top` | Top | `crop.y` |
| `crop-right` | Right | `1.0 - (crop.x + crop.w)` |
| `crop-bottom` | Bottom | `1.0 - (crop.y + crop.h)` |

### `MotionBlur`

`MotionBlur` (`crates/aria-models/src/look/motion_blur.rs`) configures temporal supersampling of animated clip transforms:
- `enabled: bool` (default `false`)
- `shutter_deg: f32` (default `180.0`, validated in `0.0..=720.0`)
- `samples: u32` (default `8`, validated in `2..=32`)

## Alignment and distribution (`AlignTo`, `AlignEdge`, `AlignAxis`)

`crates/aria-ui/src/app/align.rs` aligns or distributes selected visual clips at the playhead by writing to each clip's `Position` property. Distribution requires at least `DISTRIBUTE_MIN = 3` selected clips.

### Alignment reference (`AlignTo::ALL`)

| Variant | Label | Default |
| --- | --- | --- |
| `Selection` | Selection | `yes` |
| `Frame` | Frame | `no` |
| `FirstClip` | First clip | `no` |

### Alignment edges (`AlignEdge::ALL`)

| Variant | Label | Axis |
| --- | --- | --- |
| `Left` | Left | Horizontal (`x`) |
| `CenterX` | Centre | Horizontal (`x`) |
| `Right` | Right | Horizontal (`x`) |
| `Top` | Top | Vertical (`y`) |
| `CenterY` | Middle | Vertical (`y`) |
| `Bottom` | Bottom | Vertical (`y`) |

### Distribution axes (`AlignAxis::ALL`)

| Variant | Label |
| --- | --- |
| `Horizontal` | Across |
| `Vertical` | Down |

## Text and title styling (`TextStyle`, `TextCase`, `TextAlignH`, `TextAlignV`)

`TextStyle` (`crates/aria-models/src/clip/text.rs` and `crates/aria-models/src/clip/text_runs.rs`) defines the typography and decoration of `Generator::Text` clips in 1080p reference pixels:
- `size`: default `90.0`, validated in `f32::EPSILON..=4096.0` (`MAX_TEXT_SIZE = 4096.0`)
- `fill`: default `[255, 255, 255, 255]`
- `letter_spacing`: default `0.0`, validated in `-4096.0..=4096.0` (`MAX_TEXT_LETTER_SPACING = 4096.0`)
- `kerning`: default `true`
- `line_spacing`: default `1.2`, validated in `f32::EPSILON..=100.0` (`MAX_TEXT_LINE_SPACING = 100.0`)
- `wrap`: default `true`
- `TextStroke`: default `rgba = [0, 0, 0, 255]`, `width = 6.0` in `0.0..=512.0` (`MAX_TEXT_STROKE_WIDTH = 512.0`)
- `TextBackground`: default `rgba = [0, 0, 0, 255]`, `radius = 0.0` in `0.0..=1.0`
- `TextShadow`: default `rgba = [0, 0, 0, 230]`, `blur = 0.15` in `0.0..=1.0`, `distance = 5.0` in `0.0..=4096.0` (`MAX_TEXT_SHADOW_DISTANCE = 4096.0`)
- `runs`: up to `MAX_TEXT_RUNS = 1024` per-character-range `TextStyleRun` entries at schema version `TEXT_RUN_VERSION = 1`

### Text case (`TextCase::ALL`)

| Variant | Label | Default |
| --- | --- | --- |
| `Normal` | As typed | `yes` |
| `Upper` | Upper | `no` |
| `Lower` | Lower | `no` |
| `Title` | Title | `no` |

### Horizontal text alignment (`TextAlignH::ALL`)

| Variant | Label | Default |
| --- | --- | --- |
| `Left` | Left | `no` |
| `Center` | Center | `yes` |
| `Right` | Right | `no` |
| `Justify` | Justify | `no` |

### Vertical text alignment (`TextAlignV::ALL`)

| Variant | Label | Default |
| --- | --- | --- |
| `Top` | Top | `no` |
| `Middle` | Middle | `yes` |
| `Bottom` | Bottom | `no` |

## Generators and vector shapes (`generator_specs()` and `Shape`)

`generator_specs()` (`crates/aria-models/src/clip/generator_catalog.rs`) defines the 8 `Generator` variants (`crates/aria-models/src/clip/generator.rs`). Freshly created `Generator::Shape` clips use `SHAPE_DROP_WIDTH = 200.0` and `SHAPE_DROP_HEIGHT = 200.0` (legacy default `960.0` x `540.0`), with dimensions and path coordinates bounded by `MAX_SHAPE_DIM = 8192.0`, polygon sides and star points in `3..=20` (`MAX_STAR_POINTS = 20`), star `inner_ratio` default `0.5` in `0.0..=1.0`, and `ShapeStroke` default `rgba = [0, 0, 0, 255]`, `width = 4.0` bounded by `MAX_STROKE_WIDTH = 512.0`.

| Wire ID | Label | Target Lane (`TrackKind`) | Layer Topologies (`renders_with`) | Text Block | Shape Block | Needs Asset | Paints | Param Time | Animatable |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `text` | Text | `Text` | `text` | `true` | `false` | `false` | `true` | `ClipLocal` | `true` |
| `solid` | Color | `Sticker` | `solid` | `false` | `false` | `false` | `true` | `ClipLocal` | `false` |
| `shape` | Shape | `Sticker` | `shape, path-shape` | `false` | `true` | `false` | `true` | `ClipLocal` | `true` |
| `sticker` | Sticker | `Sticker` | `sticker` | `false` | `false` | `true` | `true` | `ClipLocal` | `false` |
| `lottie` | Animation | `Sticker` | `lottie` | `false` | `false` | `true` | `true` | `ClipLocal` | `false` |
| `effect` | Effect | `Effect` | `canvas-pass` | `false` | `false` | `false` | `false` | `Sequence` | `false` |
| `filter` | Filter | `Filter` | `canvas-pass` | `false` | `false` | `false` | `false` | `Sequence` | `false` |
| `adjustment` | Adjustment | `Adjustment` | `canvas-pass` | `false` | `false` | `false` | `false` | `Sequence` | `false` |
