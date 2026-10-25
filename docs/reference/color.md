# Color Spaces, YUV Matrices, Starter LUT Pack, and Grading Controls Reference

Frame colorimetry metadata, CPU RGBA/BGRA-to-YUV 4:2:0 conversion matrices, the baked `.cube` starter LUT pack, and Color panel sections and wheels defined in `crates/aria-core/src/color.rs`, `crates/aria-encoder/src/yuv.rs`, `crates/aria-render/src/lut_pack.rs`, `crates/aria-ui/src/panels/color_panel.rs`, and `crates/aria-ui/src/app/grade_controls.rs`. For the color filter presets (`filters.toml`) and the individual `ColorAdjustments::SLIDERS` rows, see [filters.md](filters.md).

## Frame colorimetry (`ColorSpace`)

Every `VideoFrame` and `SourceInfo` carries a `ColorSpace` (`crates/aria-core/src/color.rs`) composed of four H.273-aligned enums: `ColorPrimaries`, `TransferFunction`, `MatrixCoefficients`, and `ColorRange`. `ColorSpace::resolved(self)` maps any `Unspecified` field to its `Bt709` SDR fallback while preserving `range`.

### Color primaries (`ColorPrimaries`)

| Variant | Default | Wide gamut (`is_wide_gamut`) | Resolved fallback | Summary |
| --- | --- | --- | --- | --- |
| `Bt709` | `yes` | `no` | `Bt709` | Rec.709 / sRGB primaries. |
| `Bt601_525` | `no` | `no` | `Bt601_525` | Rec.601 525-line (SMPTE 170M / NTSC). |
| `Bt601_625` | `no` | `no` | `Bt601_625` | Rec.601 625-line (BT.470BG / PAL). |
| `Bt2020` | `no` | `yes` | `Bt2020` | Rec.2020 / Rec.2100 wide-gamut primaries. |
| `DisplayP3` | `no` | `no` | `DisplayP3` | DCI-P3 / Display P3 primaries. |
| `Unspecified` | `no` | `no` | `Bt709` | Unsignalled container primaries; resolves to `Bt709`. |

### Transfer functions (`TransferFunction`)

| Variant | Default | HDR (`is_hdr`) | Resolved fallback | Summary |
| --- | --- | --- | --- | --- |
| `Bt709` | `yes` | `no` | `Bt709` | Rec.709 opto-electronic transfer function. |
| `Srgb` | `no` | `no` | `Srgb` | sRGB transfer function for stills and screen captures. |
| `Pq` | `no` | `yes` | `Pq` | SMPTE ST 2084 Perceptual Quantizer (HDR10). |
| `Hlg` | `no` | `yes` | `Hlg` | ITU-R BT.2100 Hybrid Log-Gamma. |
| `Unspecified` | `no` | `no` | `Bt709` | Unsignalled transfer function; resolves to `Bt709`. |

### YUV-to-RGB matrix coefficients (`MatrixCoefficients`)

| Variant | Default | Resolved fallback | Encoder `YuvMatrix` | Summary |
| --- | --- | --- | --- | --- |
| `Bt709` | `yes` | `Bt709` | `Bt709` | Rec.709 luma/chroma matrix. |
| `Bt601` | `no` | `Bt601` | `Bt601` | Rec.601 luma/chroma matrix. |
| `Bt2020Ncl` | `no` | `Bt2020Ncl` | `Bt2020` | Rec.2020 non-constant luminance matrix. |
| `Identity` | `no` | `Identity` | `Bt709` | Direct RGB/GBR samples with no matrix conversion. |
| `Unspecified` | `no` | `Bt709` | `Bt709` | Unsignalled matrix; resolves to `Bt709`. |

### Sample range (`ColorRange`)

| Variant | Default | `is_full()` | 8-bit luma / chroma range |
| --- | --- | --- | --- |
| `Limited` | `yes` | `false` | Luma `16..=235`, chroma `16..=240`. |
| `Full` | `no` | `true` | Luma and chroma `0..=255`. |

### Preset `ColorSpace` constants

| Constant | Primaries | Transfer | Matrix | Range | `is_hdr()` | `is_wide_gamut()` | Default |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `BT709` | `Bt709` | `Bt709` | `Bt709` | `Limited` | `false` | `false` | `yes` |
| `BT601` | `Bt601_625` | `Bt709` | `Bt601` | `Limited` | `false` | `false` | `no` |
| `BT2020_PQ` | `Bt2020` | `Pq` | `Bt2020Ncl` | `Limited` | `true` | `true` | `no` |
| `SRGB` | `Bt709` | `Srgb` | `Identity` | `Full` | `false` | `false` | `no` |

## YUV 4:2:0 conversion matrices (`YuvMatrix`)

`crates/aria-encoder/src/yuv.rs` converts packed 32-bit `ChannelOrder::Rgba` or `ChannelOrder::Bgra` frames into planar I420 (`rgba_to_i420_into`) or semi-planar NV12 (`rgba_to_nv12_into`) using fixed-point (`/256`) limited-range integer arithmetic:

```text
Y = clamp(((cr * R + cg * G + cb * B + 128) >> 8) + 16, 0, 255)
U = clamp(((ur * R + ug * G + ub * B + 128) >> 8) + 128, 0, 255)
V = clamp(((vr * R + vg * G + vb * B + 128) >> 8) + 128, 0, 255)
```

Chroma `(R, G, B)` inputs are 2x2 box-averaged (`(p00 + p01 + p10 + p11) >> 2`) across each 2x2 pixel block, and frame byte length is `yuv420_len(width, height) = (w & !1) * (h & !1) + 2 * ((w & !1) / 2) * ((h & !1) / 2)`.

| Variant | Luma `[cr, cg, cb]` | Chroma `U` `[ur, ug, ub]` | Chroma `V` `[vr, vg, vb]` | Selected by `matrix_for` when `color.matrix` is |
| --- | --- | --- | --- | --- |
| `Bt601` | `[66, 129, 25]` | `[-38, -74, 112]` | `[112, -94, -18]` | `Bt601` |
| `Bt709` | `[47, 157, 16]` | `[-26, -87, 113]` | `[112, -102, -10]` | `Bt709`, `Identity`, `Unspecified` |
| `Bt2020` | `[58, 149, 13]` | `[-31, -81, 112]` | `[112, -103, -9]` | `Bt2020Ncl` |

## Starter LUT pack (`StarterLutEntry` and `StarterLut`)

`crates/aria-render/src/lut_pack.rs` generates a 3D `.cube` LUT (`STARTER_LUT_SIZE = 33`, `33^3 = 35937` lattice points over the unit domain) for every preset in `filter_catalog()` via `ColorGrade::apply`. **Install Starter LUT Pack** (`pal-look-install`) writes the pack into the LUT cache (`<data dir>/luts`). The same presets apply as filters from the Look tab's **Filter** row.

| Pack ID | Label | Filter ID | Cube Grid Size |
| --- | --- | --- | --- |
| `aria-vivid` | Vivid | `vivid` | `33` |
| `aria-warm` | Warm | `warm` | `33` |
| `aria-cool` | Cool | `cool` | `33` |
| `aria-mono` | Mono | `mono` | `33` |
| `aria-fade` | Fade | `fade` | `33` |
| `aria-chrome` | Chrome | `chrome` | `33` |
| `aria-noir` | Noir | `noir` | `33` |
| `aria-sunset` | Sunset | `sunset` | `33` |
| `aria-forest` | Forest | `forest` | `33` |
| `aria-berry` | Berry | `berry` | `33` |

## Color panel sections and grading wheels (`SECTIONS` and `WheelControl`)

`crates/aria-ui/src/panels/color_panel.rs` partitions the rows of the clip parameter catalog's `grade` group into the collapsible sections below (`SECTIONS`, bitmask capacity `COLOR_SECTIONS = 32`). Section `0` (`Basic Correction`) starts open by default (`GradeControls::default()`); sections `1..=6` start closed.

| Section Index | Section Title | Default Open | Row Count | Catalog Row IDs |
| --- | --- | --- | --- | --- |
| `0` | Basic Correction | `yes` | `10` | `adjust-temperature`, `adjust-tint`, `adjust-exposure`, `adjust-contrast`, `adjust-highlights`, `adjust-shadows`, `adjust-whites`, `adjust-blacks`, `adjust-brightness`, `adjust-saturation` |
| `1` | Creative | `no` | `4` | `adjust-faded-film`, `adjust-vibrance`, `adjust-sharpness`, `adjust-hue` |
| `2` | RGB Curves | `no` | `12` | `adjust-curve-master-shadows`, `adjust-curve-master-midtones`, `adjust-curve-master-highlights`, `adjust-curve-red-shadows`, `adjust-curve-red-midtones`, `adjust-curve-red-highlights`, `adjust-curve-green-shadows`, `adjust-curve-green-midtones`, `adjust-curve-green-highlights`, `adjust-curve-blue-shadows`, `adjust-curve-blue-midtones`, `adjust-curve-blue-highlights` |
| `3` | Hue Curves | `no` | `18` | `adjust-hue-sat-red`, `adjust-hue-sat-yellow`, `adjust-hue-sat-green`, `adjust-hue-sat-cyan`, `adjust-hue-sat-blue`, `adjust-hue-sat-magenta`, `adjust-hue-hue-red`, `adjust-hue-hue-yellow`, `adjust-hue-hue-green`, `adjust-hue-hue-cyan`, `adjust-hue-hue-blue`, `adjust-hue-hue-magenta`, `adjust-hue-luma-red`, `adjust-hue-luma-yellow`, `adjust-hue-luma-green`, `adjust-hue-luma-cyan`, `adjust-hue-luma-blue`, `adjust-hue-luma-magenta` |
| `4` | Color Wheels | `no` | `12` | `adjust-lift-master`, `adjust-lift-r`, `adjust-lift-g`, `adjust-lift-b`, `adjust-gamma-master`, `adjust-gamma-r`, `adjust-gamma-g`, `adjust-gamma-b`, `adjust-gain-master`, `adjust-gain-r`, `adjust-gain-g`, `adjust-gain-b` |
| `5` | HSL Secondary | `no` | `5` | `adjust-hsl-key-hue`, `adjust-hsl-key-width`, `adjust-hsl-hue`, `adjust-hsl-saturation`, `adjust-hsl-luma` |
| `6` | Vignette | `no` | `1` | `adjust-vignette` |

### Grading wheels (`WheelControl`)

`crates/aria-ui/src/app/grade_controls.rs` and `crates/aria-ui/src/panels/inspector/blocks/wheels.rs` render three circular color wheels (`WHEEL_RADIUS = 36.0` px) with a 36-dot hue ring and a 9 px puck. Dragging the puck writes zero-sum RGB offsets (`dr = x`, `dg = -0.5 * x + 0.8660254 * y`, `db = -0.5 * x - 0.8660254 * y`) scaled by `reach()` around the three channels' mean at press start.

| Variant | ID | Label | Neutral Master | Reach | Master `LookParam` | RGB `LookParam` Triple | History Drag Label | History Reset Label |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `Lift` | `lift` | Lift | `0.0` | `0.25` | `AdjustLiftMaster` | `AdjustLiftR, AdjustLiftG, AdjustLiftB` | Lift wheel | Reset Lift |
| `Gamma` | `gamma` | Gamma | `1.0` | `0.5` | `AdjustGammaMaster` | `AdjustGammaR, AdjustGammaG, AdjustGammaB` | Gamma wheel | Reset Gamma |
| `Gain` | `gain` | Gain | `1.0` | `0.5` | `AdjustGainMaster` | `AdjustGainR, AdjustGainG, AdjustGainB` | Gain wheel | Reset Gain |
