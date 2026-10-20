# Animations, text effects, easings, and speed presets

Aria ships four motion, text-style, and timing catalogs under `crates/aria-models/data/`:

1. **Clip and per-character animations (`animations.toml`)**: 31 entrance (`in`), exit (`out`), and looping (`combo`) presets evaluated in `crates/aria-render/src/animation.rs` (whole-layer) and `crates/aria-render/src/render/text_anim.rs` (`text_only = true` per-cluster glyph animations).
2. **Text style presets (`text_effects.toml`)**: 6 stroke and drop-shadow combinations (`neon`, `shadow`, `outline`, `glow`, `retro`, `chrome`) applied to `TextStyle` (`crates/aria-models/src/text.rs`).
3. **Easing curves (`easings.toml` and `preset_catalog.rs`)**: 3 named cubic-bezier curves (`snappy`, `overshoot`, `anticipate`), 6 core `Easing` modes (`linear`, `ease_in`, `ease_out`, `ease_in_out`, `hold`, `cubic_bezier`), and 3 multi-keyframe piecewise presets (`bounce_out`, `elastic_out`, `back_out`).
4. **Speed-ramp presets (`speed_presets.toml`)**: 8 normalized speed multiplier curves (`ramp_up`, `ramp_down`, `montage`, `hero`, `bullet`, `jump_cut`, `flash_in`, `flash_out`).

## Examples

### Apply an Entrance and Exit animation to a clip

1. Select a video, text, or sticker clip on the timeline.
2. Open **Window > Effect Controls** and switch to the **Motion** tab (or **Window > Project > Animations**).
3. In the **Animation** block:
   - Set **In** (`slot = "in"`) to **Slide up** (`slide_up`), **Speed** (`speed`) to `1.2`, and **Intensity** (`intensity`) to `1.0`.
   - Set **Out** (`slot = "out"`) to **Fade out** (`fade_out`) and **Speed** to `1.0`.

### Apply a Neon text style preset and a per-character Typewriter entrance

1. Select a `Text` clip on `T1` and open **Window > Effect Controls > Text**.
2. Click **Neon** (`neon`) in the **Text Effects** preset bar (or **Window > Project > Graphics > Text Effects**) to apply a `#39FF14` (`4 px`) stroke and `#39FF14C8` (`blur = 0.35`, `distance = 0 px`) glow shadow.
3. In the **Animation** block, set **In** to **Typewriter** (`char_typewriter`) and adjust **Stagger** (`stagger`, `0.05..=2.0`, default `1.0`).

### Override `speed_presets.toml` to customize the Hero ramp

Place an override file at `<data dir>/catalogs/speed_presets.toml`:

```toml
[[entry]]
id = "hero"
label = "Hero"
points = [
  { at = 0.0, value = 1.5, easing = "ease_in_out" },
  { at = 0.5, value = 0.25, easing = "ease_in_out" },
  { at = 1.0, value = 1.5 },
]
```

## Animations (`animations.toml`)

Each entry in `crates/aria-models/data/animations.toml` specifies `id`, `label`, `slot` (`in`, `out`, or `combo`), `text_only` (`false` for whole-layer transforms, `true` for per-cluster glyph animations on text clips), and `knobs`:
- **Whole-layer presets (`text_only = false`)**: expose `speed` (`0.25..=4.0`, default `1.0`, scaling the base window `look_animation_window_ticks(duration, rate)`) and `intensity` (`0.0..=2.0`, default `1.0`).
- **Per-character text presets (`text_only = true`)**: expose `speed` (`0.25..=4.0`, default `1.0`), `intensity` (`0.0..=2.0`, default `1.0`), and `stagger` (`0.05..=2.0`, default `1.0`, scaling the `0.55` per-cluster stagger window).

UI location: **Window > Effect Controls > Motion > Animation**, **Window > Effects & Presets > Animations**, and **Window > Project > Animations**.

### Whole-layer animations (`text_only = false`, `crates/aria-render/src/animation.rs`)

| ID | Label | Slot | Knobs (`speed`, `intensity`, `stagger`) | Transform / opacity delta (`SLIDE_OFFSET = 0.18` of canvas height) |
| --- | --- | --- | --- | --- |
| `fade_in` | Fade in | `in` | `speed`, `intensity` | Multiplies opacity by `t` (`EaseOut` over entrance window). |
| `slide_up` | Slide up | `in` | `speed`, `intensity` | Offsets `y` by `(1 - t) * 0.18` and multiplies opacity by `t`. |
| `zoom_in` | Zoom in | `in` | `speed`, `intensity` | Scales by `0.25 + 0.75 * t` and multiplies opacity by `t`. |
| `spin_in` | Spin in | `in` | `speed`, `intensity` | Rotates by `(1 - t) * -360°`, scales by `0.5 + 0.5 * t`, and multiplies opacity by `t`. |
| `bounce` | Bounce | `in` | `speed`, `intensity` | Scales with `bounce_scale(t)`, offsets `y` by `(1 - t) * 0.18 * 0.35`, and multiplies opacity by `t`. |
| `fade_out` | Fade out | `out` | `speed`, `intensity` | Multiplies opacity by `1 - t` (`EaseIn` over exit window). |
| `slide_down` | Slide down | `out` | `speed`, `intensity` | Offsets `y` by `t * 0.18` and multiplies opacity by `1 - t`. |
| `zoom_out` | Zoom out | `out` | `speed`, `intensity` | Scales by `1.0 - 0.75 * t` and multiplies opacity by `1 - t`. |
| `spin_out` | Spin out | `out` | `speed`, `intensity` | Rotates by `t * 360°`, scales by `1.0 - 0.5 * t`, and multiplies opacity by `1 - t`. |
| `drop` | Drop | `out` | `speed`, `intensity` | Offsets `y` by `t * 0.18 * 1.4`, scales by `1.0 - 0.35 * t`, and multiplies opacity by `1 - t`. |
| `pulse` | Pulse | `combo` | `speed`, `intensity` | Scales by `1.0 + 0.08 * sin(2 * pi * phase)`. |
| `rock` | Rock | `combo` | `speed`, `intensity` | Rotates by `6.0° * sin(2 * pi * phase)`. |
| `swing` | Swing | `combo` | `speed`, `intensity` | Rotates by `12.0° * sin(pi * phase)`. |
| `flicker` | Flicker | `combo` | `speed`, `intensity` | Steps opacity between `1.0` and `0.35` at `8` cycles per period (`fract(phase * 8.0) < 0.5`). |
| `breathe` | Breathe | `combo` | `speed`, `intensity` | Scales by `1.0 + 0.05 * sin(2 * pi * phase)` and varies opacity between `0.85` and `1.0`. |

### Per-character text animations (`text_only = true`, `crates/aria-render/src/render/text_anim.rs`)

| ID | Label | Slot | Knobs (`speed`, `intensity`, `stagger`) | Per-cluster delta (`local = stagger_progress(t, stagger_idx, window)`) |
| --- | --- | --- | --- | --- |
| `char_typewriter` | Typewriter | `in` | `speed`, `intensity`, `stagger` | Steps cluster opacity from `0.0` to `1.0` when `local > 0.0`. |
| `char_fade_in` | Fade in | `in` | `speed`, `intensity`, `stagger` | Ramps cluster opacity linearly with `local`. |
| `char_bounce_in` | Bounce in | `in` | `speed`, `intensity`, `stagger` | Offsets `y` by `(1 - local) * 18 px`, scales by `0.4 + 0.6 * bounce_out(local)`, and fades by `local`. |
| `char_slide_in` | Slide in | `in` | `speed`, `intensity`, `stagger` | Offsets `x` by `(1 - local) * 24 px` and fades by `local`. |
| `char_pop_in` | Pop in | `in` | `speed`, `intensity`, `stagger` | Scales from `0.2` to `1.0` (`0.2 + 0.8 * local`) and fades by `local`. |
| `char_fade_out` | Fade out | `out` | `speed`, `intensity`, `stagger` | Fades cluster opacity in reverse cluster order (`1 - local`). |
| `char_fall_away` | Fall away | `out` | `speed`, `intensity`, `stagger` | Drops `y` by `local * 28 px`, shrinks scale by `1.0 - 0.35 * local`, and fades by `1 - local` in reverse cluster order. |
| `char_typewriter_out` | Type out | `out` | `speed`, `intensity`, `stagger` | Hides clusters in reverse order once `local >= 1.0`. |
| `typewriter` | Typewriter | `combo` | `speed`, `intensity`, `stagger` | Types clusters in over `phase in 0.0..0.85`, holds, then repeats. |
| `text_fade` | Fade | `combo` | `speed`, `intensity`, `stagger` | Oscillates cluster opacity between `0.55` and `1.0` (`0.55 + 0.45 * (sin(pi * phase + stagger * 2) * 0.5 + 0.5)`). |
| `text_bounce` | Bounce | `combo` | `speed`, `intensity`, `stagger` | Bounces cluster `y` by `|sin(2 * pi * (phase + stagger))| * 6 px` and scale by `1.0 + 0.08 * |wave|`. |
| `text_slide` | Slide | `combo` | `speed`, `intensity`, `stagger` | Slides cluster `x` by `sin(2 * pi * (phase + stagger)) * 5 px`. |
| `pop` | Pop | `combo` | `speed`, `intensity`, `stagger` | Pulses cluster scale by `1.0 + 0.18 * max(sin(pi * phase + pi * stagger), 0)`. |
| `wave` | Wave | `combo` | `speed`, `intensity`, `stagger` | Oscillates cluster `y` by `wave * 8 px` and rotation by `wave * 0.12 rad`. |
| `char_jitter` | Jitter | `combo` | `speed`, `intensity`, `stagger` | Steps deterministic per-cluster noise (`±2.5 px` X/Y, `±0.05 rad` rotation) across `12` phase buckets per cycle. |
| `char_pulse` | Pulse | `combo` | `speed`, `intensity`, `stagger` | Oscillates cluster scale by `1.0 + 0.12 * sin(2 * pi * phase + stagger * 3)`. |

## Text style presets (`text_effects.toml`)

Defined in `crates/aria-models/data/text_effects.toml` (`TextEffectPreset` in `crates/aria-models/src/text.rs`). Selecting a preset copies its `stroke` (`rgba`, `width` in px) and `shadow` (`rgba`, `blur` in `0.0..=1.0`, `distance` in px) fields onto the clip's `TextStyle`:

| ID | Label | Stroke (`rgba`, `width`) | Shadow (`rgba`, `blur`, `distance`) |
| --- | --- | --- | --- |
| `neon` | Neon | `[57, 255, 20, 255]`, `width = 4.0` | `[57, 255, 20, 200]`, `blur = 0.35`, `distance = 0.0` |
| `shadow` | Shadow | none | `[0, 0, 0, 230]`, `blur = 0.15`, `distance = 8.0` |
| `outline` | Outline | `[0, 0, 0, 255]`, `width = 8.0` | none |
| `glow` | Glow | none | `[255, 255, 255, 220]`, `blur = 0.4`, `distance = 0.0` |
| `retro` | Retro | `[255, 140, 60, 255]`, `width = 5.0` | `[120, 40, 160, 255]`, `blur = 0.05`, `distance = 10.0` |
| `chrome` | Chrome | `[230, 230, 240, 255]`, `width = 3.0` | `[40, 60, 90, 200]`, `blur = 0.2`, `distance = 6.0` |

UI location: **Window > Effect Controls > Text** and **Window > Project > Graphics > Text Effects**.

## Easing curves (`easings.toml`, `Easing`, and `PiecewiseEasingPreset`)

### Named cubic-bezier presets (`easings.toml`)

Defined in `crates/aria-models/data/easings.toml` (`EASING_PRESETS` in `crates/aria-models/src/param/mod.rs`). Selecting a preset writes `Easing::CubicBezier { x1, y1, x2, y2 }` onto the keyframe or transition:

| ID | Label | Control `points` (`[x1, y1, x2, y2]`) | Behavior |
| --- | --- | --- | --- |
| `snappy` | Snappy | `[0.0, 0.0, 0.2, 1.0]` | Fast initial acceleration with a crisp decelerating settle. |
| `overshoot` | Overshoot | `[0.34, 1.56, 0.64, 1.0]` | Overshoots past `1.0` (`y1 = 1.56`) before settling onto the target value. |
| `anticipate` | Anticipate | `[0.36, 0.0, 0.66, -0.56]` | Winds up below `0.0` (`y2 = -0.56`) before accelerating into the target value. |

### Built-in `Easing` interpolation modes (`crates/aria-models/src/param/mod.rs`)

| ID | Variant | Formula (`Easing::apply(t)`) |
| --- | --- | --- |
| `linear` | `Easing::Linear` | `t` |
| `ease_in` | `Easing::EaseIn` | `t^2` (quadratic ease-in) |
| `ease_out` | `Easing::EaseOut` | `1.0 - (1.0 - t)^2` (quadratic ease-out) |
| `ease_in_out` | `Easing::EaseInOut` | Smoothstep `t^2 * (3.0 - 2.0 * t)` |
| `hold` | `Easing::Hold` | Step hold: returns the outgoing keyframe value until the next keyframe tick. |
| `cubic_bezier` | `Easing::CubicBezier { x1, y1, x2, y2 }` | Solves `B_x(s) = t` via 8 Newton-Raphson iterations and evaluates `B_y(s)`. |

### Multi-keyframe piecewise easing presets (`crates/aria-models/src/param/preset_catalog.rs`)

Expanding a `PiecewiseEasingPreset` across a keyframe pair inserts intermediate keyframes along the normalized `(t_frac, v_frac, easing)` waypoints in `SHIPPED`:

| ID | Label | Waypoints (`(t_frac, v_frac, easing)`) |
| --- | --- | --- |
| `bounce_out` | Bounce out | `(0.0, 0.0, EaseIn)`, `(0.3636, 1.0, EaseOut)`, `(0.5454, 0.75, EaseIn)`, `(0.7272, 1.0, EaseOut)`, `(0.8181, 0.9375, EaseIn)`, `(0.9090, 1.0, EaseOut)`, `(0.9545, 0.984375, EaseIn)`, `(1.0, 1.0, Linear)` |
| `elastic_out` | Elastic out | `(0.0, 0.0, EaseInOut)`, `(0.3, 1.25, EaseInOut)`, `(0.55, 0.875, EaseInOut)`, `(0.75, 1.0625, EaseInOut)`, `(0.9, 0.96875, EaseInOut)`, `(1.0, 1.0, Linear)` |
| `back_out` | Back out | `(0.0, 0.0, EaseOut)`, `(0.6, 1.1, EaseInOut)`, `(1.0, 1.0, Linear)` |

## Speed-ramp presets (`speed_presets.toml`)

Defined in `crates/aria-models/data/speed_presets.toml` (`SpeedPreset` in `crates/aria-models/src/clip/speed_curve.rs`). Each preset defines a normalized curve of speed multipliers (`value` in `0.01..=100.0`) at clip positions `at` in `0.0..=1.0` with segment `easing` (`linear` when omitted):

| ID | Label | Points (`at -> value (easing)`) |
| --- | --- | --- |
| `ramp_up` | Ramp up | `0.0 -> 0.4 (ease_in)`, `1.0 -> 2.5 (linear)` |
| `ramp_down` | Ramp down | `0.0 -> 2.5 (ease_out)`, `1.0 -> 0.4 (linear)` |
| `montage` | Montage | `0.0 -> 2.0 (ease_in_out)`, `0.5 -> 0.5 (ease_in_out)`, `1.0 -> 2.0 (linear)` |
| `hero` | Hero | `0.0 -> 1.5 (ease_in_out)`, `0.5 -> 0.3 (ease_in_out)`, `1.0 -> 1.5 (linear)` |
| `bullet` | Bullet | `0.0 -> 3.0 (ease_in_out)`, `0.4 -> 0.25 (ease_in_out)`, `0.6 -> 0.25 (ease_in_out)`, `1.0 -> 3.0 (linear)` |
| `jump_cut` | Jump cut | `0.0 -> 1.0 (linear)`, `0.25 -> 3.0 (linear)`, `0.5 -> 1.0 (linear)`, `0.75 -> 3.0 (linear)`, `1.0 -> 1.0 (linear)` |
| `flash_in` | Flash in | `0.0 -> 3.0 (ease_out)`, `0.3 -> 1.0 (linear)`, `1.0 -> 1.0 (linear)` |
| `flash_out` | Flash out | `0.0 -> 1.0 (linear)`, `0.7 -> 1.0 (ease_in)`, `1.0 -> 3.0 (linear)` |

UI location: **Window > Effect Controls > Speed** and **Window > Project > Effects > Speed Presets**.
