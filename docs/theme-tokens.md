# Theme tokens

Source: `crates/aria-ui/src/theme.rs`, ported from
[Comet](https://github.com/zeronsh/comet). Two appearances, one token set.

Helpers: `ink(alpha)` for fills, `hairline(alpha)` for borders,
`wash(alpha)` for hover plates. Selection is an inset ring
(`glass_selected_shadows`), not a colored fill.

## Surfaces

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `bg` | `#141414` | `#ebebeb` | Viewer pasteboard, timeline track area |
| `surface` | `#1c1c1c` | `#e3e3e3` | Panel bodies |
| `surface_header` | `#232323` | `#dcdcdc` | Tab rows, track headers, property name column |
| `surface_raised` | `#2a2a2a` | `#d5d5d5` | Inputs, buttons at rest |
| `surface_raised_hover` | `#333333` | `#cccccc` | A raised control under the pointer |
| `surface_card` | `#181818` | `#e7e7e7` | Cards and tooltips |
| `surface_dialog` | `#262626` | `#d9d9d9` | Dialogs |
| `surface_overlay` | `#2e2e2e` | `#d1d1d1` | Menus, popovers |
| `element_hover` | ink 0.11 | ink 0.06 | The plate a control fades to under the pointer |
| `element_active` | ink 0.16 | ink 0.10 | The plate a control shows while it is pressed, and a selected tool |

## Lines

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `border` | `#0e0e0e` | `#f1f1f1` | Splitters and frame edges |
| `border_strong` | white 14 % | black 17 % | Control outlines |
| `hairline` | white 6 % | black 6 % | Row separators |

## Text

| Token | Dark | Light | Contrast target |
| --- | --- | --- | --- |
| `text` | `#d6d6d6` | `#262626` | Body text |
| `text_muted` | `#9a9a9a` | `#525252` | Labels, column headers |
| `text_faint` | `#6b6b6b` | `#7d7d7d` | Disabled, hints |
| `text_dim` | `#989898` | `neutral(0.50)` | Context rather than content: file paths |
| `value` | `#5aa8ff` | `#1a73d9` | Editable numbers |
| `caret` | oklch blue | oklch blue | The blinking caret in a rename bar |

Fonts: Geist and Geist Mono. Sizes in the UI are 14 px (titles), 13 px
(body), 11 px (captions / timecode).

## Accents

Use each hue only in its role. No sixth accent.

| Token | Dark (oklch) | Light (oklch) | Role |
| --- | --- | --- | --- |
| `accent` | 0.673, 0.182, 276.935 | 0.511, 0.262, 276.966 | Focus, links, dark playhead |
| `danger` | 0.704, 0.191, 22.216 | 0.577, 0.245, 27.325 | Errors, destructive, recording |
| `warning` | 0.828, 0.189, 84.429 | 0.555, 0.163, 48.998 | Snap guide, missing media |
| `success` | 0.765, 0.177, 163.223 | 0.596, 0.145, 163.225 | Export done, AI approve |
| `busy` | 0.718, 0.202, 349.761 | 0.592, 0.249, 0.584 | Working indicator (`loaders::pulse_loader`) |

Muted variants back a whole surface rather than a glyph: `danger_muted` the
status strip while an error stands and a failed job row, `warning_muted` a
project whose file has moved, `success_muted` a job that finished. `on_accent`
is the glyph colour on an accent fill, as `on_solid` is on the solid one.

Primary buttons use `solid` / `on_solid` (neutral maximum contrast), not
`accent`.

## Editor tokens

Track tints are chroma 0.03 and paint the kind badge in a track header.

| Token | Value | Use |
| --- | --- | --- |
| `track_video` | oklch 0.65 / 0.03 / 270 | Video track badge |
| `track_audio` | oklch 0.65 / 0.03 / 140 | Audio and submix track badge |
| `track_text` | oklch 0.65 / 0.03 / 60 | Text track badge |
| `track_generator` | oklch 0.65 / 0.03 / 0 | Generator and adjustment track badge |
| `playhead` | `accent` dark / black light | 1 px playhead |
| `snap_guide` | `warning` | 1 px magnet line |
| `waveform_peak` | `text_muted` @ 0.6 | Waveform peaks |
| `waveform_rms` | `text_faint` @ 0.4 | Waveform RMS |
| `recording_dot` | `danger` | Live-work dot in the status strip |
| `cursor` | `accent` | Drop insertion cursor in the timeline |
| `selection` | `accent` @ 0.14 | Rubber-band fill, selected text in a title and in a name field |
| `band` | ink 0.16 dark / 0.045 light | Every other row of a long list |

## Labels

A clip bar is filled with its label colour, and a selected clip with the
label's selected fill: each channel moved a third of the way to white. Both are
the same in either appearance. The name, icon, and waveform on a fill are drawn
in near black (`#141414`) or near white (`#f4f4f4`), whichever contrasts more.
Labels are not theme tokens; a theme file does not set them.

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

- `name` is the label in Settings. Without it, the file name is the label.
- `[dark]` and `[light]` map tokens to values: every token in the tables above,
  plus `font_sans`, `font_mono`, `font_sans_fallback`, and
  `font_mono_fallback`, which take a font family name. A token left out keeps
  its shipped value, and an appearance with no table is the shipped one.
- Colours are `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, or `oklch(l c h)` with
  an optional `/ alpha`. `l` and `alpha` are 0 to 1, `c` is 0 or more, and `h`
  is in degrees.
- The accent choice applies first, so a file that sets `accent` replaces it.
- A file larger than 64 KiB, an unknown key or token, or a value that does not
  parse makes the file unreadable. Settings lists it with the reason. A saved
  choice of an unreadable file shows the shipped theme and logs a warning.

To reload a file after editing it, open Settings and click its row. A plugin
adds a theme with `themes/<id>.toml` in its package or
`PluginContext::register_theme` (see [Packages](plugins/packages.md)).

## Layout constants

`HEADER_HEIGHT = 44`, `TITLEBAR_HEIGHT = 38`, `STATUS_STRIP_HEIGHT = 24`,
`BUBBLE_RADIUS = 16`, `PANEL_RADIUS = 10`, `CONTROL_RADIUS = 6`,
`SPACE_XS = 4`, `SPACE_SM = 8`, `SPACE_MD = 12`, `SPACE_LG = 16`.

Glass is 0.80 alpha on macOS and opaque elsewhere.
