# Blend Modes and Mask Compositing Reference

Layer blend modes, mask stack combination modes, and mask shapes defined in `crates/aria-models/src/look/blend.rs`, `crates/aria-models/src/look/mask.rs`, `crates/aria-models/data/masks.toml`, `crates/aria-compositor/src/layer.rs`, and `crates/aria-compositor/shaders/blend_composite.wgsl`.

## Layer blend modes (`BlendMode`)

Each visual clip stores a `blend_mode: BlendMode` field (`BlendMode::Normal` by default; omitted from project JSON when equal to `Normal`). `BlendMode::Normal` runs the fixed-function premultiplied source-over pipeline. Every other mode runs the destination-sampling composite pass in `crates/aria-compositor/shaders/blend_composite.wgsl` with pipeline-override constant `BLEND_MODE` set to `BlendMode::shader_id`.

| ID | Label | Variant | Shader ID | Default | WGSL channel formula | Summary |
| --- | --- | --- | --- | --- | --- | --- |
| `normal` | Normal | `Normal` | `0` | `yes` | `cs` | Source color replaces backdrop without backdrop mixing. |
| `darken` | Darken | `Darken` | `1` | `no` | `min(cb, cs)` | Per-channel minimum of backdrop and source. |
| `multiply` | Multiply | `Multiply` | `2` | `no` | `cb * cs` | Per-channel product of backdrop and source. |
| `color_burn` | Color Burn | `ColorBurn` | `3` | `no` | `1.0 - min(1.0, (1.0 - cb) / cs)` | Divides inverted backdrop by source (`1.0` when `cb == 1.0`, `0.0` when `cs == 0.0`). |
| `lighten` | Lighten | `Lighten` | `4` | `no` | `max(cb, cs)` | Per-channel maximum of backdrop and source. |
| `screen` | Screen | `Screen` | `5` | `no` | `cb + cs - cb * cs` | Per-channel complement product (`1 - (1 - cb)(1 - cs)`). |
| `color_dodge` | Color Dodge | `ColorDodge` | `6` | `no` | `min(1.0, cb / (1.0 - cs))` | Divides backdrop by inverted source (`0.0` when `cb == 0.0`, `1.0` when `cs == 1.0`). |
| `add` | Add | `Add` | `7` | `no` | `min(vec3<f32>(1.0), cb + cs)` | Clamped per-channel linear dodge (`min(1, cb + cs)`). |
| `overlay` | Overlay | `Overlay` | `8` | `no` | `hard_light(cs, cb)` | Multiplies where `cb <= 0.5` and screens where `cb > 0.5`. |
| `soft_light` | Soft Light | `SoftLight` | `9` | `no` | `cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)` | W3C soft light curve (`cb - (1 - 2 * cs) * cb * (1 - cb)` for `cs <= 0.5`; `cb + (2 * cs - 1) * (soft_light_d(cb) - cb)` for `cs > 0.5`). |
| `hard_light` | Hard Light | `HardLight` | `10` | `no` | `hard_light(cb, cs)` | Multiplies (`cb * 2 * cs`) where `cs <= 0.5` and screens (`screen(cb, 2 * cs - 1)`) where `cs > 0.5`. |
| `difference` | Difference | `Difference` | `11` | `no` | `abs(cb - cs)` | Per-channel absolute difference between backdrop and source. |
| `exclusion` | Exclusion | `Exclusion` | `12` | `no` | `cb + cs - 2.0 * cb * cs` | Lower-contrast difference that maps midtones (`0.5`) to `0.5`. |

### Premultiplied W3C composite equations

`blend_composite.wgsl` samples premultiplied RGBA backdrop `dst_pm = (cb * ab, ab)` and premultiplied RGBA source `src_pm = (cs * as_, as_)`, unpremultiplies both where alpha is positive (`c.rgb / c.a`, or `vec3(0.0)` when `c.a <= 0.0`), evaluates `b = blend_fn(BLEND_MODE, cb, cs)`, and writes premultiplied output `(co, ao)` per W3C Compositing and Blending Level 1:

```text
cs_mixed = (1.0 - ab) * cs + ab * b
co       = cs_mixed * as_ + cb * ab * (1.0 - as_)
ao       = as_ + ab * (1.0 - as_)
```

For `soft_light_ch(cb, cs)` when `cs > 0.5`, the W3C auxiliary function `soft_light_d(cb)` evaluates `((16.0 * cb - 12.0) * cb + 4.0) * cb` when `cb <= 0.25` and `sqrt(cb)` when `cb > 0.25`.

## Mask combination modes (`MaskMode` and `MaskOp`)

A clip's `masks: Vec<Mask>` stack holds up to `MAX_CLIP_MASKS = 32` masks and evaluates in index order. Each mask combines its coverage `m` (after optional `invert` complement) with the running stack coverage `a` using `mode: MaskMode`, then mixes the result with `a` by the mask's `opacity` (`mix(a, op(a, m), opacity)`). When the first active mask (`mode != MaskMode::None`) uses `Subtract`, `Intersect`, or `Darken`, the stack starts at full coverage (`1.0`); when it uses `Add`, `Lighten`, or `Difference`, the stack starts at empty coverage (`0.0`).

| ID | Label | Variant | Default | Shader Op ID | Initial Stack Coverage | Coverage Formula |
| --- | --- | --- | --- | --- | --- | --- |
| `none` | None | `None` | `no` | `none` | `none` | `a` |
| `add` | Add | `Add` | `yes` | `0` | `0.0` | `min(1, a + m)` |
| `subtract` | Subtract | `Subtract` | `no` | `1` | `1.0` | `max(0, a - m)` |
| `intersect` | Intersect | `Intersect` | `no` | `2` | `1.0` | `a * m` |
| `lighten` | Lighten | `Lighten` | `no` | `3` | `0.0` | `max(a, m)` |
| `darken` | Darken | `Darken` | `no` | `4` | `1.0` | `min(a, m)` |
| `difference` | Difference | `Difference` | `no` | `5` | `0.0` | `abs(a - m)` |

## Mask shapes (`MaskKind` and `masks.toml`)

`mask_catalog()` loads the shipped mask shape order and labels from `crates/aria-models/data/masks.toml`. The six parametric shapes (`Linear` through `Star`) evaluate in the GPU mask shader using `aria_compositor::layer::mask_kind` constants `0..=5`. `MaskKind::Path` rasterizes a cubic Bezier vertex list (`MASK_PATH_VERSION = 1`, `1..=1024` vertices via `MAX_MASK_PATH_POINTS = 1024`) in normalized layer coordinates (`[0, 1]` across the layer quad, validated within `[-10.0, 10.0]`). Mask expansion is bounded to `[-4096.0, 4096.0]` reference pixels (`MAX_MASK_EXPANSION = 4096.0`).

| ID | Label | Variant | Shader Kind ID | Supports Size | Supports Roundness |
| --- | --- | --- | --- | --- | --- |
| `linear` | Linear | `Linear` | `0` | `yes` | `no` |
| `mirror` | Mirror | `Mirror` | `1` | `yes` | `no` |
| `circle` | Circle | `Circle` | `2` | `yes` | `no` |
| `rectangle` | Rectangle | `Rectangle` | `3` | `yes` | `yes` |
| `heart` | Heart | `Heart` | `4` | `yes` | `no` |
| `star` | Star | `Star` | `5` | `yes` | `no` |
| `path` | Path | `Path` | `cpu` | `no` | `no` |

## On-canvas mask handles (`MaskHandle::ALL`)

In the Viewer (`crates/aria-ui/src/app/canvas_overlay.rs`), parametric masks draw three handles (`MaskHandle::ALL`, with minimum extent `MIN_MASK_FRACTION = 0.02`):

| Variant | Element ID (`id()`) | Anchor (`mask_handle_anchor`) |
| --- | --- | --- |
| `Center` | `mask-handle-center` | `[x + w * 0.5, y + h * 0.5]` |
| `Size` | `mask-handle-size` | `[x + w, y + h]` |
| `Rotate` | `mask-handle-rotate` | `[x + w * 0.5, (y - h * 0.25).max(0.0)]` |
