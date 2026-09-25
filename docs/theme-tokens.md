# Theme tokens

Source: `crates/aria-ui/src/theme.rs`, ported from
[Comet](https://github.com/zeronsh/comet). Two appearances, one token set.

Helpers: `ink(alpha)` for fills, `hairline(alpha)` for borders,
`wash(alpha)` for hover plates. Selection is an inset ring
(`glass_selected_shadows`), not a colored fill.

## Surfaces

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `bg` | `#060606` | white | Content plane (preview, clip lane) |
| `surface` | `#0d0d0d` | `neutral(0.968)` | Sidebar, inspector, timeline chrome |
| `surface_raised` | `neutral(0.235)` | `neutral(0.940)` | Raised controls |
| `surface_card` | `#0e0e0e` | white | Clip bodies |
| `surface_dialog` | `#101010` | white | Dialogs |
| `surface_overlay` | `#161616` | white | Context menu, popovers |
| `element_hover` | ink 0.11 | ink 0.06 | The plate a control fades to under the pointer |
| `element_active` | ink 0.16 | ink 0.10 | The plate a control shows while it is pressed, and a selected tool |

## Text

| Token | Dark | Light | Contrast target |
| --- | --- | --- | --- |
| `text` | `neutral(0.922)` | `neutral(0.25)` | ~16–17.5:1 body |
| `text_muted` | `neutral(0.708)` | `neutral(0.439)` | ~7.5–8:1 |
| `text_faint` | `neutral(0.556)` | `neutral(0.535)` | ~4.5:1 captions |
| `text_dim` | `#989898` | `neutral(0.50)` | Context rather than content: file paths |
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

Track tints are chroma 0.03 and paint only the 2 px left clip border.

| Token | Value | Use |
| --- | --- | --- |
| `track_video` | oklch 0.65 / 0.03 / 270 | Video clip border |
| `track_audio` | oklch 0.65 / 0.03 / 140 | Audio clip border |
| `track_text` | oklch 0.65 / 0.03 / 60 | Text clip border |
| `track_shape` | oklch 0.65 / 0.03 / 200 | Shape clip border |
| `track_generator` | oklch 0.65 / 0.03 / 0 | Generator clip border |
| `playhead` | `accent` dark / black light | 1 px playhead |
| `snap_guide` | `warning` | 1 px magnet line |
| `waveform_peak` | `text_muted` @ 0.6 | Waveform peaks |
| `waveform_rms` | `text_faint` @ 0.4 | Waveform RMS |
| `recording_dot` | `danger` | Live-work dot in the status strip |
| `cursor` | `accent` | Drop insertion cursor in the timeline |
| `selection` | `accent` @ 0.14 | Rubber-band fill |
| `band` | ink 0.16 dark / 0.045 light | Every other row of a long list |

## Layout constants

`HEADER_HEIGHT = 44`, `TITLEBAR_HEIGHT = 38`, `STATUS_STRIP_HEIGHT = 24`,
`BUBBLE_RADIUS = 16`, `PANEL_RADIUS = 10`, `CONTROL_RADIUS = 6`,
`SPACE_XS = 4`, `SPACE_SM = 8`, `SPACE_MD = 12`, `SPACE_LG = 16`.

Glass is 0.80 alpha on macOS and opaque elsewhere.
