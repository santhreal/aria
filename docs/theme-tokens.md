# Theme tokens

Source: `crates/aria-ui/src/theme.rs`, ported from
[Comet](https://github.com/zeronsh/comet). Two appearances, one token set.

Helpers: `ink(alpha)` for fills, `hairline(alpha)` for borders,
`wash(alpha)` for hover plates. Selection is an inset ring
(`glass_selected_shadows`), not a colored fill.

## Surfaces

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `bg` | `#0e0e10` | `#ebebeb` | Viewer pasteboard, timeline track area, the gaps between docked frames |
| `surface` | `#161618` | `#e3e3e3` | Panel bodies |
| `surface_header` | `#19191b` | `#dcdcdc` | Tab rows, track headers, property name column |
| `surface_raised` | `#232326` | `#d5d5d5` | Solid plates: a card, a key cap |
| `surface_raised_hover` | `#2b2b2e` | `#cccccc` | A raised plate under the pointer |
| `surface_card` | `#131315` | `#e7e7e7` | Cards and tooltips |
| `surface_dialog` | `#1c1c1f` | `#d9d9d9` | Dialogs |
| `surface_overlay` | `#202023` | `#d1d1d1` | Menus, popovers |
| `element_hover` | ink 0.08 | ink 0.06 | The wash a control fades to under the pointer |
| `element_active` | ink 0.13 | ink 0.10 | The wash a control shows while it is pressed |
| `input_bg` | black 32 % | `#ffffff` | Text box fill, recessed into the surface under it |
| `solid` | `neutral(0.922)` | `neutral(0.205)` | High-contrast primary button fill |

## Lines

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `border` | `#050506` | `#f1f1f1` | Splitters and frame edges |
| `border_strong` | white 14 % | black 17 % | Control outlines |
| `hairline` | white 6 % | black 6 % | Row separators |

## Text

| Token | Dark | Light | Contrast target |
| --- | --- | --- | --- |
| `text` | `#d8d8db` | `#262626` | Body text |
| `text_muted` | `#9e9ea4` | `#525252` | Labels, column headers, a control at rest |
| `text_faint` | `#67676d` | `#7d7d7d` | Disabled, hints |
| `text_dim` | `#97979d` | `neutral(0.50)` | Context rather than content: file paths |
| `value` | `#5aa8ff` | `#1a73d9` | Editable numbers |
| `caret` | accent family | accent family | The caret in a text box, solid, and in the rename bar, blinking |
| `on_solid` | `#0e0e0e` | `neutral(0.985)` | Glyph and label color on `solid` |
| `on_accent` | `#141414` or `#f4f4f4` | `#141414` or `#f4f4f4` | Glyph and label color on `accent` and `accent_strong` |

Fonts: Inter (Regular, Medium, SemiBold, Bold, embedded) for all UI text, with
proportional figures; readouts (the playhead fields, the ruler, scrubbable
values, the Info panel, the playback rate and drop count) are shaped with
tabular figures (`tnum`) so digits keep one width. `font_mono` is the system
monospace face, used for expressions and plugin logs. Sizes are
28 px (display), 18 px (timeline timecode), 16 px, 14 px (headings), 12 px
(panel titles), 11 px (UI text), 10 px, and 9 px (micro labels).

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `font_sans` | `"Inter"` | `"Inter"` | Primary UI sans-serif family |
| `font_mono` | system monospace | system monospace | Monospace family for expressions and plugin logs |
| `font_sans_fallback` | system sans-serif | system sans-serif | Fallback sans-serif family |
| `font_mono_fallback` | system monospace | system monospace | Fallback monospace family |

## Accents

Use each hue only in its role. No sixth accent.

| Token | Dark (oklch) | Light (oklch) | Role |
| --- | --- | --- | --- |
| `accent` | accent family | accent family | Focus, links, playhead, active tool |
| `danger` | 0.704, 0.191, 22.216 | 0.577, 0.245, 27.325 | Errors, destructive, recording |
| `warning` | 0.828, 0.189, 84.429 | 0.555, 0.163, 48.998 | Snap guide, missing media |
| `success` | 0.765, 0.177, 163.223 | 0.596, 0.145, 163.225 | Export done, AI approve |
| `busy` | 0.718, 0.202, 349.761 | 0.592, 0.249, 0.584 | Working indicator (`loaders::pulse_loader`) |
| `accent_strong` | accent family | accent family | Pressed or emphasized accent fill |
| `danger_strong` | 0.58, 0.16, 25.0 | 0.51, 0.20, 25.0 | Destructive button fill |
| `danger_muted` | 0.808, 0.114, 19.571 | 0.505, 0.213, 27.518 | Error status strip and failed job row fill |
| `warning_muted` | 0.924, 0.12, 95.746 | 0.473, 0.137, 46.201 | Moved-file warning banner fill |
| `success_muted` | 0.845, 0.143, 164.978 | 0.508, 0.118, 165.612 | Completed job row fill |

Muted variants back a whole surface rather than a glyph: `danger_muted` the
status strip while an error stands and a failed job row, `warning_muted` a
project whose file has moved, `success_muted` a job that finished. `on_accent`
is the glyph color on an accent fill, as `on_solid` is on the solid one.

Primary buttons use `solid` / `on_solid` (neutral maximum contrast), not
`accent`.

### Accent families

Settings > Appearance > **Accent** selects one family. A family sets
`accent`, `accent_strong`, `caret`, `selection`, and `playhead` for each
appearance; `on_accent` is near black or near white, whichever contrasts more
with `accent`. Neutrals, track tints, and status colors do not change. Values
are oklch `l, c, h`; `selection` adds an alpha.

| Family | Appearance | `accent` | `accent_strong` | `caret` | `selection` | `playhead` |
| --- | --- | --- | --- | --- | --- | --- |
| Blue (default) | Dark | 0.71, 0.15, 253 | 0.62, 0.17, 255 | 0.80, 0.11, 253 | 0.62, 0.15, 253 @ 0.35 | `accent` |
| Blue (default) | Light | 0.47, 0.17, 257 | 0.41, 0.17, 258 | 0.45, 0.17, 257 | 0.65, 0.14, 253 @ 0.28 | `accent` |
| Ember | Dark | 0.74, 0.15, 52 | 0.66, 0.17, 45 | 0.82, 0.11, 55 | 0.66, 0.14, 52 @ 0.32 | `accent` |
| Ember | Light | 0.49, 0.15, 42 | 0.43, 0.14, 40 | 0.47, 0.15, 42 | 0.72, 0.12, 55 @ 0.28 | `accent` |
| Teal | Dark | 0.77, 0.12, 190 | 0.68, 0.12, 192 | 0.84, 0.09, 190 | 0.66, 0.11, 190 @ 0.32 | `accent` |
| Teal | Light | 0.45, 0.09, 202 | 0.39, 0.08, 204 | 0.43, 0.09, 202 | 0.72, 0.09, 195 @ 0.28 | `accent` |
| Graphite | Dark | 0.93, 0, 0 | 0.82, 0, 0 | 0.95, 0, 0 | 0.80, 0, 0 @ 0.22 | 0.66, 0.21, 27 |
| Graphite | Light | 0.26, 0, 0 | 0.20, 0, 0 | 0.22, 0, 0 | 0.40, 0, 0 @ 0.18 | 0.57, 0.21, 27 |

In every family, `accent` text contrasts at least 4.5:1 with `bg`, `surface`,
and `surface_header`, the playhead at least 3:1 with `bg`, `on_accent` at
least 4.5:1 with `accent` and `accent_strong`, the caret at least 3:1 with an
input field, and `text` at least 4.5:1 on `selection`. No family token has a
hue between 232 and 335 degrees (HSL).

## Editor tokens

Track tints are chroma 0.03 and color the badge label in a track header. Each
reaches 4.5:1 against every surface in its appearance.

| Token | Value | Use |
| --- | --- | --- |
| `track_video` | oklch 0.65 / 0.03 / 250 dark, 0.45 / 0.03 / 250 light | Video track badge |
| `track_audio` | oklch 0.65 / 0.03 / 140 dark, 0.45 / 0.03 / 140 light | Audio and submix track badge |
| `track_text` | oklch 0.65 / 0.03 / 60 dark, 0.45 / 0.03 / 60 light | Text track badge |
| `track_generator` | oklch 0.65 / 0.03 / 0 dark, 0.45 / 0.03 / 0 light | Generator and adjustment track badge |
| `playhead` | accent family | 1 px playhead |
| `snap_guide` | `warning` | 1 px magnet line |
| `waveform_peak` | `text_muted` @ 0.6 | Waveform peaks |
| `waveform_rms` | `text_faint` @ 0.4 | Waveform RMS |
| `recording_dot` | `danger` | Live-work dot in the status strip |
| `cursor` | white 35 % dark / black 55 % light | Drop insertion cursor in the timeline |
| `selection` | accent family | Rubber-band fill, selected text in a title and in a name field |
| `band` | ink 0.16 dark / 0.045 light | Every other row of a long list |

## Labels

A clip bar is filled with its label color, and a selected clip with the
label's selected fill: each channel moved a third of the way to white. Both are
the same in either appearance. The name, icon, and waveform on a fill are drawn
in near black (`#141414`) or near white (`#f4f4f4`), whichever contrasts more.
Labels are not theme tokens; a theme file does not set them. A clip with no
label takes its kind's: Teal for video, Forest for audio, Mango for generators,
Tan for nested sequences, Yellow for adjustment clips.

| Label | Fill | Label | Fill |
| --- | --- | --- | --- |
| Violet | `#a58cde` | Purple | `#9839a6` |
| Iris | `#7180dd` | Blue | `#2e52d6` |
| Caribbean | `#1dc297` | Teal | `#199191` |
| Lavender | `#d88fdb` | Magenta | `#e33baf` |
| Cerulean | `#1ba2e4` | Tan | `#c9b086` |
| Forest | `#418f37` | Green | `#3ca83c` |
| Rose | `#f14a75` | Brown | `#8b5220` |
| Mango | `#f19e1f` | Yellow | `#e3d32a` |

## Theme files

A theme file sets token values over the shipped theme. Every `*.toml` file in
`~/.aria/themes` (`$ARIA_DATA_DIR/themes` when that is set), and every theme a
mounted plugin registers, is read each time Settings opens and listed as a
**Theme** row in the Appearance section. A plugin theme's row reads
`Theme: <name> from <plugin>`. **Theme: Shipped** removes the file's values.
The choice is saved in `ui-settings.json` as `theme_file`, the file name
without `.toml` or `<plugin id>/<theme id>`, and applied at launch.

```toml
name = "Dusk"

[dark]
accent = "#7aa2f7"
bg = "oklch(0.18 0.02 260)"
font_mono = "JetBrains Mono"

[light]
accent = "oklch(0.5 0.2 260 / 1)"
```

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `name` | string | file stem or theme id | Display label shown in Settings. When omitted or whitespace-only, defaults to the file name without `.toml` (or `<theme id>` for a plugin theme). |
| `dark` | table (`[dark]`) | `{}` (shipped dark tokens) | Token overrides applied when the active appearance is Dark. |
| `light` | table (`[light]`) | `{}` (shipped light tokens) | Token overrides applied when the active appearance is Light. |

- At least one of `[dark]` or `[light]` must set at least one token. Unknown
  top-level keys are rejected.
- `[dark]` and `[light]` map token names to string values: every color and font
  token in the tables above (`COLOR_TOKENS` and `FONT_TOKENS`, 49 tokens total).
  A token omitted from a table keeps its shipped value, and an appearance with
  no table uses the shipped theme.
- Color strings are `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, or `oklch(l c h)`
  with an optional `/ alpha`. Lightness `l` and `alpha` must be finite in
  `0.0..=1.0`, chroma `c` finite and `>= 0.0`, and hue `h` finite in degrees.
  Font tokens take a non-blank font family string.
- The accent family selected in Settings applies first; a theme file that sets
  `accent`, `accent_strong`, `caret`, `selection`, or `playhead` overrides it.
- A theme file larger than `MAX_THEME_BYTES` (`THEME_MAX_BYTES` = 64 KiB), an
  unknown key or token, or a value that fails to parse is rejected and listed in
  Settings with its failure reason. A saved choice that fails to load falls back
  to the shipped theme and logs a warning.

To reload a file after editing it, open Settings and click its row. A plugin
ships theme files at `themes/<id>.toml` in its package (where `<id>` is 1 to
`THEME_ID_MAX_BYTES` = 64 bytes of ASCII letters, digits, `-`, `_`, and `.`, not
starting with `.`) or registers them from code with
`PluginContext::register_theme` (see [Packages](plugins/packages.md)).

## Layout constants

`HEADER_HEIGHT = 44`, `TITLEBAR_HEIGHT = 38`, `STATUS_STRIP_HEIGHT = 22`,
`PANEL_RADIUS = 4`, `CONTROL_RADIUS = 2`,
`SPACE_XS = 4`, `SPACE_SM = 8`, `SPACE_MD = 12`, `SPACE_LG = 16`.

A menu, a popover, a dialog and a card take `PANEL_RADIUS`. A button, a
field, a tab, a chip, a toggle, a bar and a clip take `CONTROL_RADIUS`. A dot
and a handle are circles.

A region that scrolls fades each edge its content continues past across
`SCROLL_FADE = 24` px (`crates/aria-ui/src/edge_fade.rs`). An edge with nothing
past it does not fade.

Glass is 0.80 alpha on macOS and opaque elsewhere.
