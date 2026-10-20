# Stickers and masks

Aria includes two shape and overlay catalogs under `crates/aria-models/data/`:

1. **Stickers (`stickers.toml`)**: 8 built-in static and animated raster overlays (`heart`, `star`, `smiley`, `bolt`, `bubble`, `flower`, `star_spin`, `heart_beat`) decoded from `crates/aria-models/data/stickers/` or `<data dir>/catalogs/stickers/`.
2. **Mask shapes (`masks.toml`)**: 7 mask shape entries (`linear`, `mirror`, `circle`, `rectangle`, `heart`, `star`, `path`) evaluated on the GPU in `crates/aria-compositor/shaders/mask.wgsl`, `matte.wgsl`, and `mask_stack.wgsl`.

## Examples

### Place an animated Spinning Star sticker

1. Open **Window > Project**, switch to the **Graphics** tab, and click **Stickers** (or open **Window > Effects & Presets > Stickers**).
2. Click **Spinning Star** (`star_spin`) to place a 5-second `Generator::Sticker` clip (`128 x 128` reference pixels, `star_spin.gif`) on the `S1` sticker track at the playhead.
3. Select the clip and open **Window > Effect Controls > Motion** to position, scale, rotate, or animate it.

### Apply a subtracted feathered Circle mask on a clip

1. Select a clip on a video track.
2. Open **Window > Effect Controls** and switch to the **Mask** tab.
3. Click **Circle** (`circle`) to add a circular mask to the clip's mask stack.
4. Set **Mode** to **Subtract** (`subtract`) or enable **Inverted** (`inverted = true`), and drag **Feather** (`mask-0-feather`) to `0.15`.
5. Click `◆` on **Centre** (`mask-0-center`) or **Size** (`mask-0-size`) to keyframe the mask across the clip.

### Override `stickers.toml` to register a custom PNG or GIF sticker

Place the image file in `<data dir>/catalogs/stickers/logo_badge.png` and add an `[[entry]]` block to `<data dir>/catalogs/stickers.toml`:

```toml
[[entry]]
id = "logo_badge"
label = "Logo Badge"
width = 256
height = 256
file = "logo_badge.png"
```

## Stickers (`stickers.toml`)

Defined in `crates/aria-models/data/stickers.toml` (`StickerSpec` in `crates/aria-models/src/clip/generator.rs`). Each entry specifies a permanent `id`, display `label`, intrinsic size `width x height` in reference pixels (at 1080p), `animated` (`true` for multi-frame GIF or APNG art, default `false`), and `file` (under `<data dir>/catalogs/stickers/` or bundled in the binary):

| ID | Label | Size (`width x height`) | Animated | Asset `file` | Parameters |
| --- | --- | --- | --- | --- | --- |
| `heart` | Heart | `256 x 256` | `no` | `heart.png` | none |
| `star` | Star | `256 x 256` | `no` | `star.png` | none |
| `smiley` | Smiley | `256 x 256` | `no` | `smiley.png` | none |
| `bolt` | Lightning Bolt | `256 x 256` | `no` | `bolt.png` | none |
| `bubble` | Speech Bubble | `256 x 256` | `no` | `bubble.png` | none |
| `flower` | Flower | `256 x 256` | `no` | `flower.png` | none |
| `star_spin` | Spinning Star | `128 x 128` | `yes` | `star_spin.gif` | none |
| `heart_beat` | Beating Heart | `128 x 128` | `yes` | `heart_beat.png` | none |

- **Static and animated assets**: multi-frame `.gif` (`star_spin.gif`) and APNG (`heart_beat.png`) assets decode frame-accurately by clip local time; static `.png` assets upload once to the GPU texture cache.
- **Lottie vector animations**: `Generator::Lottie` clips (`lottie` in [Generators](generators.md)) render Bodymovin/Lottie JSON animations on the same `Sticker` track kind (`S1`).
- **UI location**: **Window > Project > Graphics > Stickers** and **Window > Effects & Presets > Stickers**.

## Mask shapes (`masks.toml`)

Defined in `crates/aria-models/data/masks.toml` (`MaskSpec` / `MaskKind` in `crates/aria-models/src/look/mask.rs`) and evaluated in `crates/aria-compositor/shaders/mask.wgsl`, `matte.wgsl`, and `mask_stack.wgsl`:

| Kind (`kind`) | Label | GPU shader / path evaluation |
| --- | --- | --- |
| `linear` | Linear | Half-plane split (`kind = 0` in `mask.wgsl`) keeping the side above the line through `center` rotated by `rotation`. |
| `mirror` | Mirror | Reflected band (`kind = 1` in `mask.wgsl`) of half-width `size.y * 0.5` symmetric across the line through `center` at `rotation`. |
| `circle` | Circle | Ellipse (`kind = 2` in `mask.wgsl`) with radii `size * 0.5` around `center` rotated by `rotation`. |
| `rectangle` | Rectangle | Axis-aligned or rotated box (`kind = 3` in `mask.wgsl`) of extent `size` around `center` with corner rounding `roundness` (`0.0..=1.0`). |
| `heart` | Heart | Symmetric heart (`kind = 5` in `mask.wgsl`) fit to `size` around `center` at `rotation`. |
| `star` | Star | 5-point star (`kind = 4` in `mask.wgsl`, inner vertex ratio `0.42`) fit to `size` around `center` at `rotation`. |
| `path` | Path | Cubic-bezier vertex path (`1..=1024` `ShapePathPoint` vertices within `±10.0` normalized coordinates, `MASK_PATH_VERSION = 1`), rasterized to a stencil texture and feathered in `matte.wgsl` / `mask_stack.wgsl` (`MAX_SOFTNESS_TEXELS = 12.0`). |

UI location: **Window > Effect Controls > Mask**, **Window > Effects & Presets > Masks**, and the **Pen tool (`P`)** when **Mask** path target mode is active in the Program Monitor.

### Multi-mask stack modes (`MaskMode` in `crates/aria-models/src/look/mask.rs`)

A clip can hold up to `MAX_CLIP_MASKS = 32` masks in `clip.masks`, combined in stack order by `MaskMode` and weighted by each mask's `opacity` (`mix(a, combine(op, a, m), opacity)` in `mask_stack.wgsl`):

| ID | Label | Stack formula (`combine(op, a, m)`) |
| --- | --- | --- |
| `none` | None | Skips the mask without altering stack coverage (`a`). |
| `add` | Add | `min(a + m, 1.0)` (default). |
| `subtract` | Subtract | `max(a - m, 0.0)` (starts coverage at `1.0` when first in stack). |
| `intersect` | Intersect | `a * m` (starts coverage at `1.0` when first in stack). |
| `lighten` | Lighten | `max(a, m)`. |
| `darken` | Darken | `min(a, m)` (starts coverage at `1.0` when first in stack). |
| `difference` | Difference | `abs(a - m)`. |

### Animatable mask parameters (`MaskParam` / `mask_rows.rs`)

Each mask at index `<index>` (`0..32`) in `clip.masks` exposes 7 animatable parameters (`mask-<index>-<id>`, with clip-first shorthand aliases `mask-feather`, `mask-expansion`, `mask-roundness`, `mask-opacity` for mask `0` in `crates/aria-models/src/clip/param_catalog/mask_rows.rs`):

| ID (`MaskParam::id`) | Shorthand (mask `0`) | Label | Kind | Unit | Min | Max | Default | Applies to | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `feather` | `mask-feather` | Feather | `scalar` | normalized | `0.0` | `1.0` | `0.0` | all `MaskKind` | `yes` |
| `center` | none (`mask-0-center`) | Centre | `vec2` | layer fraction offset | `-10.0` | `10.0` | `[0.0, 0.0]` | all `MaskKind` | `yes` |
| `size` | none (`mask-0-size`) | Size | `vec2` | layer fraction | `> 0.0` | `f32::MAX` | `[1.0, 1.0]` | all except `path` | `yes` |
| `rotation` | none (`mask-0-rotation`) | Rotation | `scalar` | degrees | `-3600.0` | `3600.0` | `0.0` | all `MaskKind` | `yes` |
| `roundness` | `mask-roundness` | Roundness | `scalar` | corner fraction | `0.0` | `1.0` | `0.0` | `rectangle` only | `yes` |
| `opacity` | `mask-opacity` | Opacity | `scalar` | normalized | `0.0` | `1.0` | `1.0` | all `MaskKind` | `yes` |
| `expansion` | `mask-expansion` | Expansion | `scalar` | reference px | `-4096.0` | `4096.0` | `0.0` | all `MaskKind` | `yes` |
