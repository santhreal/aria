# Look

## Themes

Two themes from one token set: Dark (default) and Light. Settings >
Appearance switches between them, or follows the operating system. A theme is a
TOML file of token values; plugins and users add themes by dropping a file in
`<data dir>/themes/`. [theme-tokens.md](../theme-tokens.md) lists the current
tokens.

### Dark surfaces

| Token | Value | Use |
| --- | --- | --- |
| `bg` | `#141414` | Viewer pasteboard, timeline track area |
| `surface` | `#1c1c1c` | Panel bodies |
| `surface_header` | `#232323` | Tab rows, track headers, property name column |
| `surface_raised` | `#2a2a2a` | Inputs, buttons at rest |
| `surface_overlay` | `#2e2e2e` | Menus, popovers, tooltips |
| `border` | `#0e0e0e` | Splitters and frame edges |
| `hairline` | `#ffffff` at 6 % | Row separators |
| `text` | `#d6d6d6` | Body text |
| `text_muted` | `#9a9a9a` | Labels, column headers |
| `text_faint` | `#6b6b6b` | Disabled, hints |
| `value` | `#5aa8ff` | Editable numbers (scrubbable values) |
| `accent` | indigo, oklch 0.673 0.182 277 | Playhead, focus outline, selection, active tool |

Light inverts the neutral ramp and keeps the hues. Clip colours come from the
label set (Violet, Iris, Caribbean, Lavender, Cerulean, Forest, Rose, Mango,
Purple, Blue, Teal, Magenta, Tan, Green, Brown, Yellow), each with a fill
and a selected fill. Video clips default to Violet, audio clips to Forest,
generators to Mango, nested sequences to Tan, adjustment clips to Lavender.

## Density

| Element | Size |
| --- | --- |
| UI text | 11 px |
| Panel tab and title text | 12 px |
| Timecode in the timeline header | 18 px tabular figures |
| Row height (lists, properties, menus) | 22 px |
| Tab row | 24 px |
| Icon | 16 px in a 24 or 28 px hit box |
| Inputs | 20 px high |
| Spacing grid | 4 px; 2 px for icon-to-text gaps |

Text uses Geist; timecode, values, and frame counts use Geist Mono with
tabular figures, so digits do not shift while a value changes.

## Controls

A scrubbable value is drawn in `value` colour with a dotted underline on hover.
Dragging it horizontally changes the value: 1 unit per 2 px, Shift multiplies
by 10, Ctrl divides by 10. Clicking without dragging opens a text field;
Enter commits, Escape reverts, Tab commits and moves to the next value. The
pointer is hidden and confined during the drag and reappears where the drag
started.

A button press dims the button to 92 % opacity and darkens its plate. A
disabled control is drawn at 40 % opacity, keeps the arrow cursor, and states
the reason in its tooltip. Every icon-only control has a tooltip naming the
action and its chord, shown after 400 ms and hidden on press.

Keyboard focus draws a 1 px accent ring around the focused control. A pointer
click does not leave a focus ring.

## Icons

One stroke icon set at 16 px, 1.5 px stroke, drawn in the text colour of the
control that holds it. Each icon has one meaning; the same glyph is never used
for two actions. [icons.md](../icons.md) lists the set.

## Motion

Motion uses `gpui::motion` from santh-gpui, with the curve set and specs in
[animation-catalog.md](../animation-catalog.md). The rules:

- Entrances and exits are one-shots: menus and popovers 140 ms in and 100 ms
  out on `ease`, dialogs 180 ms, panel docking and workspace changes 180 ms on
  `ease_out`.
- Anything that can be interrupted is a chased value: hover plates (150 ms
  `standard`), selection rings, splitter positions, track heights (180 ms
  `ease_out`), twirl-down expansion (180 ms `ease_out`), timeline zoom and
  fit (200 ms `ease_out`), and clips displaced by a ripple (200 ms `ease_out`).
- A value under the pointer follows the pointer with no easing. Easing applies
  from the release.
- During playback nothing animates except the playhead, the meters, and the
  dropped-frame indicator. Hover fades and glides are suspended so the frame
  budget belongs to the picture.
- Settings > Appearance offers Full, Brisk, Relaxed, and No motion. No motion
  draws every end state immediately and follows the operating system's
  reduced-motion preference by default.
