# Catalogs

A catalog is the set of things you can pick from: transitions, effects, filters,
animations, text effects, easings, speed presets, stickers, mask shapes. Each one
is a TOML file. The shipped copy is compiled into the binary, so the app always
has a full set, and a file of the same name in your catalog directory replaces
it.

## Where the files go

```text
~/.aria/catalogs/transitions.toml
~/.aria/catalogs/effects.toml
~/.aria/catalogs/audio_effects.toml
~/.aria/catalogs/filters.toml
~/.aria/catalogs/animations.toml
~/.aria/catalogs/text_effects.toml
~/.aria/catalogs/easings.toml
~/.aria/catalogs/speed_presets.toml
~/.aria/catalogs/stickers.toml
~/.aria/catalogs/masks.toml
~/.aria/catalogs/stickers/           # sticker art
```

The directory is `<data dir>/catalogs`, shown in **Settings > Catalogs**. It is
not created for you; make it and drop a file in.

## Format

One array of entries under `entry`:

```toml
[[entry]]
id = "crossfade"
label = "Cross Fade"
```

`id` is what a project file stores. Changing an id orphans every clip that used
it, so add an entry rather than rename one. `label` is what the UI shows.

## Transitions

`id` must be a transition the compositor renders, and an id with no shader
behind it cross-dissolves. Each entry may also declare `[[entry.params]]` rows
(`name`, `label`, `min`, `max`, `default`, `step`). For the shipped transitions
and their parameters, see [Transitions](reference/transitions.md).

## Filters

A filter carries its own grade, so a new filter needs no rebuild. Every key
except `id` and `label` is a grade offset at full intensity, and a key left out
states no change. A key the grade does not carry refuses the file, so a
misspelled offset is reported rather than dropped:

```toml
[[entry]]
id = "teal"
label = "Teal"
temperature = -0.3
tint = -0.1
saturation = 0.2
```

The keys are the inspector's grade sliders, in panel order:

| key | range | effect |
| --- | --- | --- |
| `brightness` | -1 … 1 | overall lift of every channel |
| `contrast` | -1 … 1 | spread of the tonal range around mid grey |
| `saturation` | -1 … 1 | -1 is monochrome |
| `exposure` | -1 … 1 | stops, applied before the tone curve |
| `temperature` | -1 … 1 | negative is cooler |
| `tint` | -1 … 1 | negative is greener |
| `hue` | -1 … 1 | rotation; ±1 is ±30° |
| `highlights` | -1 … 1 | tones above mid luma |
| `shadows` | -1 … 1 | tones below mid luma |
| `whites` | -1 … 1 | white point; +1 takes 0.75 to white |
| `blacks` | -1 … 1 | black point; +1 lifts black to 0.2 |
| `sharpness` | 0 … 1 | unsharp-mask strength |
| `vignette` | 0 … 1 | radial darkening from the centre |
| `vibrance` | -1 … 1 | saturation, weighted toward muted colours |
| `faded_film` | 0 … 1 | lifts black toward grey, white stays |
| `lift_r` | -1 … 1 | shadow offset, red |
| `lift_g` | -1 … 1 | shadow offset, green |
| `lift_b` | -1 … 1 | shadow offset, blue |
| `lift_master` | -1 … 1 | shadow offset, every channel |
| `gamma_r` | 0 … 4 | midtone power, red |
| `gamma_g` | 0 … 4 | midtone power, green |
| `gamma_b` | 0 … 4 | midtone power, blue |
| `gamma_master` | 0 … 4 | midtone power, every channel |
| `gain_r` | 0 … 4 | highlight scale, red |
| `gain_g` | 0 … 4 | highlight scale, green |
| `gain_b` | 0 … 4 | highlight scale, blue |
| `gain_master` | 0 … 4 | highlight scale, every channel |
| `curve_master_shadows` | -1 … 1 | RGB curve, every channel, at 1/4 input |
| `curve_master_midtones` | -1 … 1 | RGB curve, every channel, at 1/2 input |
| `curve_master_highlights` | -1 … 1 | RGB curve, every channel, at 3/4 input |
| `curve_red_shadows` | -1 … 1 | RGB curve, red, at 1/4 input |
| `curve_red_midtones` | -1 … 1 | RGB curve, red, at 1/2 input |
| `curve_red_highlights` | -1 … 1 | RGB curve, red, at 3/4 input |
| `curve_green_shadows` | -1 … 1 | RGB curve, green, at 1/4 input |
| `curve_green_midtones` | -1 … 1 | RGB curve, green, at 1/2 input |
| `curve_green_highlights` | -1 … 1 | RGB curve, green, at 3/4 input |
| `curve_blue_shadows` | -1 … 1 | RGB curve, blue, at 1/4 input |
| `curve_blue_midtones` | -1 … 1 | RGB curve, blue, at 1/2 input |
| `curve_blue_highlights` | -1 … 1 | RGB curve, blue, at 3/4 input |
| `hue_sat_red` | -1 … 1 | saturation of reds |
| `hue_sat_yellow` | -1 … 1 | saturation of yellows |
| `hue_sat_green` | -1 … 1 | saturation of greens |
| `hue_sat_cyan` | -1 … 1 | saturation of cyans |
| `hue_sat_blue` | -1 … 1 | saturation of blues |
| `hue_sat_magenta` | -1 … 1 | saturation of magentas |
| `hue_hue_red` | -1 … 1 | hue of reds; ±1 is ±60° |
| `hue_hue_yellow` | -1 … 1 | hue of yellows; ±1 is ±60° |
| `hue_hue_green` | -1 … 1 | hue of greens; ±1 is ±60° |
| `hue_hue_cyan` | -1 … 1 | hue of cyans; ±1 is ±60° |
| `hue_hue_blue` | -1 … 1 | hue of blues; ±1 is ±60° |
| `hue_hue_magenta` | -1 … 1 | hue of magentas; ±1 is ±60° |
| `hue_luma_red` | -1 … 1 | luma of reds |
| `hue_luma_yellow` | -1 … 1 | luma of yellows |
| `hue_luma_green` | -1 … 1 | luma of greens |
| `hue_luma_cyan` | -1 … 1 | luma of cyans |
| `hue_luma_blue` | -1 … 1 | luma of blues |
| `hue_luma_magenta` | -1 … 1 | luma of magentas |
| `hsl_key_hue` | -1 … 1 | keyed hue centre; 0 is red, ±1 is cyan |
| `hsl_key_width` | -1 … 1 | keyed hue half-width; 30° at 0 |
| `hsl_hue` | -1 … 1 | hue of the keyed colours; ±1 is ±60° |
| `hsl_saturation` | -1 … 1 | saturation of the keyed colours |
| `hsl_luma` | -1 … 1 | luma of the keyed colours |

The gamma and gain keys multiply, so `1` is no change and the value a preset
states is the multiplier at full intensity. A `0`, or a key left out, is read
as no change rather than as a black frame.

Clip intensity scales the whole recipe, so keep the numbers at what the preset
looks like at full strength. An additive key scales toward zero and a
multiplying key scales toward one.

For the shipped filter presets and color grading sliders, see
[Filters and color grading](reference/filters.md).

## Effects

`id` must be an effect the compositor (`effects.toml`) or audio mixer
(`audio_effects.toml`) dispatches. `params` is the uniform or parameter slot
order, so append a parameter rather than reorder one. In `effects.toml`,
`generator = true` marks a procedural generator, and each parameter takes
`name`, `label`, and a `kind` of `scalar` (`default`, `min`, `max`), `vec2`
(`default_vec2`, `min`, `max`), or `color` (`default_color` as RGBA 0 … 255).
In `audio_effects.toml`, every parameter is a scalar (`default`, `min`, `max`),
`latency` is in 48 kHz frames, and `sidechain = true` enables sidechain routing.
For the shipped entries, see [Video effects](reference/effects.md),
[Generators](reference/generators.md), and
[Audio effects](reference/audio-effects.md).

## Animations, text effects, easings, speed presets

An animation entry names a `slot` (`in`, `out`, or `combo`), an easing entry
carries four bezier control `points`, and a speed preset carries the `points` of
its ramp. A speed curve is validated the way one you draw in the editor is, so a
file cannot introduce a ramp the editor would refuse.

An animation id needs a sampler behind it. An entry the renderer has no
implementation for is still listed, labelled `(no renderer)`, and applying it
animates nothing. An effect id with no pass behind it is skipped rather than
dispatched, so it changes nothing.

For the shipped animations, text style presets, easings, and speed ramps, see
[Animations, text effects, easings, and speed presets](reference/animations.md).

## Stickers

A sticker entry names `width`, `height`, and the `file` its art is read from,
plus `animated = true` for multi-frame art:

```toml
[[entry]]
id = "logo"
label = "Logo"
width = 256
height = 256
file = "logo.png"
```

Art is read from `<data dir>/catalogs/stickers/<file>`, and falls back to the
copy shipped in the binary. PNG, APNG, and GIF are accepted. `width` and
`height` are the intrinsic size in pixels; a sticker is placed at that size
relative to a 1080p canvas.

The file name is a name, not a path: a `/` or a `..` in it is refused. An entry
whose art is found in neither place is dropped from the catalog, and the reason
appears in **Settings > Catalogs**.

For the shipped sticker entries, see
[Stickers and masks](reference/stickers-and-masks.md#stickers-stickerstoml).

## Masks

A mask entry names the `kind` it labels rather than an `id`:

```toml
[[entry]]
kind = "circle"
label = "Round"
```

A `kind` is a shape the renderer draws (`MaskKind`), so the set is fixed: a file
may relabel or reorder the shapes, and a file that names an unknown kind, or
leaves one out, is refused whole. For the shipped mask shapes, modes, and
parameters, see
[Stickers and masks](reference/stickers-and-masks.md#mask-shapes-maskstoml).

## A file that will not load

A catalog file is refused whole. An unparseable file, an empty one, a duplicate
id, or an entry with no label leaves the shipped catalog in effect rather than
starting the app with no transitions.

**Settings > Catalogs** lists every catalog, how many entries are in effect,
whether the source is your file or the shipped copy, and the reason a file on disk
was refused. A refused file is the case that otherwise looks exactly like having
installed nothing.

Catalogs are read once, at startup. Restart after editing one.
