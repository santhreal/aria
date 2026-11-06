# Transitions

Aria ships the video transition entries in `crates/aria-models/data/transitions.toml` (overridden at `<data dir>/catalogs/transitions.toml`; see [Catalogs](../catalogs.md)) and the audio crossfade transitions (`AudioTransition::ALL`).

A video transition (`Transition`) attaches to a cut between two abutting clips on the same track over `duration` ticks (`DEFAULT_TRANSITION_TICKS = 24`) with window placement `alignment: TransitionAlignment` (`TransitionAlignment::Center` by default). Normalized progress `t` runs from `0.0` at the transition window start to `1.0` at the transition window end.

## Examples

### Apply a feathered Wipe Left between two clips

1. Place two clips side-by-side on `V1`.
2. Select the outgoing (left) clip.
3. Open **Window > Effects & Presets**, expand **Video Transitions**, and double-click **Wipe Left** (or press `Ctrl+T` / choose **Sequence > Apply Default Transitions** and change its kind in **Window > Effect Controls** under **Look > Transition**).
4. In **Effect Controls** (**Look > Transition**), set **Duration (ticks)** (`transition-duration`), **Align** (`transition-align`), and **Feather** (`feather`, `0.10`).

### Override `transitions.toml` to increase default wipe feather

Place an override file at `<data dir>/catalogs/transitions.toml`:

```toml
[[entry]]
id = "wipe_left"
label = "Wipe Left"
[[entry.params]]
name = "feather"
label = "Feather"
min = 0.0
max = 0.5
default = 0.1
```

## Where to apply and edit transitions

- **Keyboard shortcut (`Ctrl+T` / `Cmd+T`, `key-transition`)**: applies the default video transition (`crossfade`, `DEFAULT_TRANSITION_TICKS = 24`) on the selected clip's outgoing cut.
- **Sequence menu (**Sequence > Apply Default Transitions** and **Sequence > Add Cross Fade at Cut**)**: applies `crossfade` (`24` ticks) to the selected clip's outgoing cut.
- **Effects & Presets (**Window > Effects & Presets**, **Video Transitions**)**: double-click an entry or drag it onto a clip on the timeline. While dragging, the lane line marks the cut the release lands on: the clip's cut nearer the pointer, or the clip's one cut from either half. The clip on the left of that cut becomes selected.
- **Project panel (**Window > Project**, **Effects > Transitions**)**: click an entry to apply it to the selected clip's outgoing cut or drag it onto a timeline cut.
- **Effect Controls (**Window > Effect Controls**, **Look > Transition** for picture cuts or **Audio > Transition** for sound cuts)**: selects the transition kind (`transition-choice`), duration in ticks (`transition-duration`), cut alignment (`transition-align`), and per-transition parameters (`feather` on wipes), or removes the transition (`transition-clear` / **Sequence > Remove Transition at Cut**).
- **Timeline cut handle**: drag the transition pill across the cut to resize its duration directly on the track.

## Video transition catalog (`transitions.toml`)

| ID | Label | Shader pass | Summary |
| --- | --- | --- | --- |
| `crossfade` | Cross Fade | `transition_crossfade.wgsl` | Linearly mixes outgoing texture `A` and incoming texture `B` by `t`. |
| `dip_to_black` | Dip to Black | `transition_dip.wgsl` | Fades `A` to opaque black over `t in 0.0..0.5`, then black to `B` over `t in 0.5..1.0`. |
| `dip_to_white` | Dip to White | `transition_dip.wgsl` | Fades `A` to opaque white over `t in 0.0..0.5`, then white to `B` over `t in 0.5..1.0`. |
| `wipe_left` | Wipe Left | `transition_wipe.wgsl` | Reveals `B` from right to left (`coord = 1.0 - uv.x`) with `smoothstep` feathering. |
| `wipe_right` | Wipe Right | `transition_wipe.wgsl` | Reveals `B` from left to right (`coord = uv.x`) with `smoothstep` feathering. |
| `wipe_up` | Wipe Up | `transition_wipe.wgsl` | Reveals `B` from bottom to top (`coord = 1.0 - uv.y`) with `smoothstep` feathering. |
| `wipe_down` | Wipe Down | `transition_wipe.wgsl` | Reveals `B` from top to bottom (`coord = uv.y`) with `smoothstep` feathering. |
| `slide` | Slide | `transition_slide.wgsl` | Pushes `A` left (`uv_a.x = uv.x + t`) while sliding `B` in from the right (`uv_b.x = uv.x - (1.0 - t)`). |

### Cross Fade (`crossfade`)

Evaluates `mix(a, b, clamp(progress, 0.0, 1.0))` on premultiplied RGBA textures `tex_a` and `tex_b` (`transition_crossfade.wgsl`).

UI location: **Effects & Presets > Video Transitions > Cross Fade**, **Project > Effects > Transitions > Cross Fade**, **Sequence > Add Cross Fade at Cut**, and `Ctrl+T`.

Parameters: none (`params = []`).

### Dip to Black (`dip_to_black`)

Evaluates `transition_dip.wgsl` with dip target `dip = vec4(0.0, 0.0, 0.0, 1.0)`. For `progress < 0.5`, mixes `tex_a` toward `dip` by `progress * 2.0`; for `progress >= 0.5`, mixes `dip` toward `tex_b` by `(progress - 0.5) * 2.0`.

UI location: **Effects & Presets > Video Transitions > Dip to Black** and **Project > Effects > Transitions > Dip to Black**.

Parameters: none (`params = []`).

### Dip to White (`dip_to_white`)

Evaluates `transition_dip.wgsl` with dip target `dip = vec4(1.0, 1.0, 1.0, 1.0)`. For `progress < 0.5`, mixes `tex_a` toward `dip` by `progress * 2.0`; for `progress >= 0.5`, mixes `dip` toward `tex_b` by `(progress - 0.5) * 2.0`.

UI location: **Effects & Presets > Video Transitions > Dip to White** and **Project > Effects > Transitions > Dip to White**.

Parameters: none (`params = []`).

### Wipe Left (`wipe_left`)

Evaluates `transition_wipe.wgsl` with `direction = 0` (`coord = 1.0 - uv.x`). For `feather = max(params[0].z, 0.0005)`, expands the edge sweep across `-feather..=(1.0 + feather)` (`edge = progress * (1.0 + 2.0 * feather) - feather`) and blends `mix(b, a, smoothstep(edge - feather, edge + feather, coord))` so `t = 0.0` is pure `A` and `t = 1.0` is pure `B`.

UI location: **Effects & Presets > Video Transitions > Wipe Left** and **Project > Effects > Transitions > Wipe Left**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `feather` | Feather | `scalar` | UV fraction | `0.0` | `0.5` | `0.0` | `no` |

### Wipe Right (`wipe_right`)

Evaluates `transition_wipe.wgsl` with `direction = 1` (`coord = uv.x`) and the same `smoothstep(edge - feather, edge + feather, coord)` blend as `wipe_left`.

UI location: **Effects & Presets > Video Transitions > Wipe Right** and **Project > Effects > Transitions > Wipe Right**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `feather` | Feather | `scalar` | UV fraction | `0.0` | `0.5` | `0.0` | `no` |

### Wipe Up (`wipe_up`)

Evaluates `transition_wipe.wgsl` with `direction = 2` (`coord = 1.0 - uv.y`) and the same `smoothstep(edge - feather, edge + feather, coord)` blend as `wipe_left`.

UI location: **Effects & Presets > Video Transitions > Wipe Up** and **Project > Effects > Transitions > Wipe Up**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `feather` | Feather | `scalar` | UV fraction | `0.0` | `0.5` | `0.0` | `no` |

### Wipe Down (`wipe_down`)

Evaluates `transition_wipe.wgsl` with `direction = 3` (`coord = uv.y`) and the same `smoothstep(edge - feather, edge + feather, coord)` blend as `wipe_left`.

UI location: **Effects & Presets > Video Transitions > Wipe Down** and **Project > Effects > Transitions > Wipe Down**.

| Name | Label | Kind | Unit | Min | Max | Default | Animates |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `feather` | Feather | `scalar` | UV fraction | `0.0` | `0.5` | `0.0` | `no` |

### Slide (`slide`)

Evaluates `transition_slide.wgsl`: slides incoming texture `tex_b` in from the right (`uv_b.x = uv.x - (1.0 - t)`) when `uv.x >= 1.0 - t` while pushing outgoing texture `tex_a` to the left (`uv_a.x = uv.x + t`).

UI location: **Effects & Presets > Video Transitions > Slide** and **Project > Effects > Transitions > Slide**.

Parameters: none (`params = []`).

## Transition cut alignment (`TransitionAlignment::ALL`)

Defined in `crates/aria-models/src/transition.rs` (`TransitionAlignment::ALL`, `DEFAULT_TRANSITION_TICKS = 24`):

| ID | Label | Variant | Default | Window `[start, end)` |
| --- | --- | --- | --- | --- |
| `center` | Center at Cut | `Center` | `yes` | `[cut - duration / 2, cut - duration / 2 + duration)` |
| `start` | Start at Cut | `Start` | `no` | `[cut, cut + duration)` |
| `end` | End at Cut | `End` | `no` | `[cut - duration, cut)` |

## Audio transitions (`AudioTransition::ALL`)

Defined in `crates/aria-models/src/audio_transition.rs` (`AudioTransition::ALL`, `EXPONENTIAL_FLOOR_DB = -60.0`):

| ID | Label | Variant | Default | Gain curve (`p` in `0.0..=1.0`) |
| --- | --- | --- | --- | --- |
| `constant_power` | Constant Power | `ConstantPower` | `yes` | Incoming `sin(p * pi / 2)`, outgoing `cos(p * pi / 2)` |
| `constant_gain` | Constant Gain | `ConstantGain` | `no` | Incoming `p`, outgoing `1.0 - p` |
| `exponential_fade` | Exponential Fade | `ExponentialFade` | `no` | Level linear in dB from `-60.0` dBFS (`EXPONENTIAL_FLOOR_DB`) to `0.0` dB |

Where to apply: `Ctrl+Shift+T` (`key-audio-transition`), **Window > Effect Controls** (**Audio > Transition**), or **Window > Effects & Presets** (**Audio Transitions**).
