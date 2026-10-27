# Look

## Themes

Two themes from one token set: Dark (default) and Light. Settings >
Appearance switches between them, or follows the operating system, and picks
the accent family: Blue (default), Ember, Teal, or Graphite. A theme is a
TOML file of token values; plugins and users add themes by dropping a file in
`<data dir>/themes/`. [theme-tokens.md](../theme-tokens.md) lists the current
tokens and each family's values.

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
| `accent` | Blue: oklch 0.71 0.15 253 (`#57a5fc`) | Playhead, focus outline, selection, active tool |

Light inverts the neutral ramp and keeps the hues. No accent, label default, or
track tint is violet. Graphite has no accent hue; its playhead is red.

Clip colors come from the label set (Violet, Iris, Caribbean, Lavender,
Cerulean, Forest, Rose, Mango, Purple, Blue, Teal, Magenta, Tan, Green, Brown,
Yellow), each with a fill and a selected fill. Video clips default to Teal,
audio clips to Forest, generators to Mango, nested sequences to Tan,
adjustment clips to Yellow.

## Density

| Element | Size |
| --- | --- |
| UI text | 11 px |
| Panel tab and title text | 12 px |
| Timecode in the timeline header | 18 px tabular figures; 14 px under a 272 px outline column |
| Row height (lists, properties, menus, palette, picker and export rows) | 22 px |
| Tab row | 24 px |
| Icon | 16 px glyph in a 24 px hit box; 28 px in the tool bar; 20 px in a 22 px row |
| Worded control in a 24 px control row | 24 px high, 4 px side padding |
| Text boxes and solid buttons | 24 px high |
| Spacing grid | 4 px; 2 px for icon-to-text gaps |
| Panel inset | 12 px |

A row of ghost buttons, chips or clickable rows steps out by its controls'
side padding, so the first label stands on the text edge of the column that
holds the row. Menu and palette rows stand with no gap between them.

An active tab's underline and a scrubbable value's hover underline draw over
their box and take no layout, so a tab label stands on its neighbors' center
line and a property value on the line of its label and keyframe navigator.
Docked frames stand 6 px apart and 6 px in from the bars and the window's
edges. Two parallel gray rules of different elements never touch or stand
1 px apart. The workspace switcher, the breadcrumb and the transport stand on
their bar's center line while both side groups fit their halves. A round
control, such as a transform handle on a fitted picture's edge, is drawn
whole inside the box that clips it.

Text uses Inter with proportional figures. Readouts are shaped with tabular
figures, so timecode, values, and frame counts keep their digit widths while
they change and align in columns; names and labels keep proportional figures,
because Inter's tabular one and hyphen are as wide as a zero. A monospace face is used only for expression text and plugin logs.

## Controls

A scrubbable value is drawn in `value` color with a dotted underline on hover.
Dragging it horizontally changes the value: 1 unit per 2 px, Shift multiplies
by 10, Ctrl divides by 10. Clicking without dragging opens a text field;
Enter commits, Escape reverts, Tab commits and moves to the next value. The
pointer is hidden and confined during the drag and reappears where the drag
started.

A button press dims the button to 92 % opacity and darkens its plate. A
disabled control is drawn at 40 % opacity, keeps the arrow cursor, and states
the reason in its tooltip. Every icon-only control has a tooltip naming the
action and its chord, shown after 400 ms and hidden on press. After a press the
tooltip stays hidden until the pointer leaves the control. A tooltip that
states more than a name sets it on a dimmed line under the name, and wraps at
280 px.

A track header toggle that is on is drawn in the accent color, with no plate;
a Timeline header toggle lights as an armed tool in the tool bar does. A selected row in a list of choices holds the selected
fill and shadow. A control that is shown only under the pointer keeps its
place while hidden, so nothing moves when the pointer arrives.

A choice from a short set is a row of chips. The chip in force has a faint
wash and full-strength text; the others have muted text on no plate. A solid
button is an action, such as Done or Export, and never marks a choice.

Keyboard focus draws a 1 px accent ring around the focused control. A pointer
click does not leave a focus ring.

## Icons

One stroke icon set at 16 px, 1.5 px stroke, drawn in the text color of the
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
