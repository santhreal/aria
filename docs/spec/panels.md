# Panels

Every panel docks, tabs, and floats ([layout.md](layout.md#frames-and-panels)),
and a right-click on its tab opens the frame menu. The tab states the panel's
title, and the panel's body does not repeat it. The Viewer and Timeline have
their own chapters; Audio Meters and Audio Track Mixer are in
[audio.md](audio.md).

Down to its frame's minimum width (160 px for most panels), a panel keeps its
text inside the frame: a row of buttons or chips wraps, a label or field ends
in an ellipsis, and a paragraph wraps. The Audio Track Mixer's strips scroll
sideways instead, and down in a frame shorter than the strips. In Effect
Controls, the tab's body and the panels stacked under it scroll below the tab
row. A tab opens at the top of its body.

## Project

The media pool, one bin at a time. A click on a bin in the bin list opens it
in place; a double-click renames it. Bins nest: New bin creates one inside the
open bin, and dragging a bin onto another moves it there. Deleting a bin moves
its media and its bins up to its parent.

- The search field matches an item's name, path, kind (`video`, `audio`),
  label, and frame size (`1920x1080` or `1920×1080`). A search covers the open
  bin and every bin under it.
- In the views that draw pictures, moving the pointer across an item's picture
  scrubs through the item.
- Dragging an item to the Timeline places it.
- An item whose file is missing draws its caption in `warning` and a Relink
  button, which prompts for the file's new location. The reason the last
  Relink All left it missing is drawn under it.
- The item menu offers Remove from Project, or Select Clips in Timeline while a
  clip uses the item, and Decode from Proxy or Decode from Original File.

The panel's rows, from the top: the group chips (Media, Graphics, Effects,
Library, Analyze) on one line, where the active group reads whole and the
others shorten with an ellipsis in a narrow panel; and the open group's tab
chips on one line, when it has more than one and they are not the pool's.

The Media tab draws one row above its items and one below them. Above: the
search field, with the placeholder `Search`; Audio only, which lists the
pool's audio sources; and Import. Below: the item count at the left; at the
right the four views (Tiles, List, Icons, Freeform) as icon toggles, the order
as a button reading the current order (`Name`, `Duration`, `Resolution`,
`Date added`) that opens the list of orders above itself, New bin, and New
multicam. Every icon control states its name in its tip. In a narrow panel
the active view icon stands for the four views and cycles through them on
click, and the count shortens with an ellipsis so the footer stays on one line.
An item's name with
no room ends in an ellipsis. In Tiles an item draws its 120 × 68 px picture at
the left and its name and facts beside it, and in a panel too narrow for both
the name and facts wrap under the picture. The facts take two lines, the frame
size and length over the codecs, and List draws them as one line beside the
name. A facts line never wraps: with no room it ends in an ellipsis. In every
view that draws pictures, an item with no frame to show (an audio file, a file
that is gone) draws a glyph in a slot the size of the others' pictures.

Between the two rows: the breadcrumb, while a bin is open, with Rename and
Delete bin at the right, which move to a line of their own when the
breadcrumbs would be narrower than 96 px; the bins inside the open one; the
multicam groups, when the project has any; and the items, which scroll. A
field narrower than its placeholder ends it in an ellipsis.

## Effect Controls

The properties of the selected clip, one group per property group, in the
order of the timeline's property rows. To the right of the value column, a
time area spans the clip's range with its own playhead and keyframes, as in
Premiere's Effect Controls. Dragging an effect from Effects & Presets onto
this panel adds it to the selected clip at the drop position in the stack.

A property row stays on one line. Past the width its controls need, a row
gives three parts of its spare width to the time area for each part it gives
the property's name. In a narrow panel the time area narrows first. Below
32 px it is left blank and a press there does nothing, and once it has no width
left the property's name is shortened with an ellipsis and shown in full on
hover. Once the name has no width left, the value column narrows, down to the
width of the value it shows. The tabs above the properties share the panel's
width in proportion to the width of their names and shorten a long name the
same way.

A row that cannot be keyed, such as a fade, a switch, or a
transition or insert parameter, stands in the same columns as a property row:
a blank where the stopwatch stands, its name in the name column, its value or
switch in the value column, and a blank where the navigator, the reset button
and the time area stand. A row with a set of choices, such as a filter, an
audio role or an effect's choice slot, shows the current choice in the value
column, cut to the column's width with the whole of it on hover, and the
choices on the line under it, as a color row shows its palette. The value
column is 76 px wide.

Each effect header shows a bypass toggle (fx), the effect name, a reset button,
and a menu with Reset, Rename Effect, Duplicate, Copy, Paste Below, Save as
Preset…, and Remove Effect. Effects
reorder by dragging their headers. With several clips selected, the panel edits
the shared properties of all of them; differing values show as a dash.

## Effects & Presets

A searchable tree of every effect, transition, generator, and animation preset,
grouped by category, including plugin contributions with their plugin's name
in the tooltip. Typing filters the tree and opens the matching categories;
the `×` at the right of the search field clears it. Double-click applies to
the selection; dragging applies to the drop target (a clip on the timeline, an
edit point for transitions, Effect Controls). An entry's tooltip states both.
Favorites (star) appear in a top group. A contribution whose plugin failed to
load is listed with its failure. Docked on its own, the panel's contents stand
12 px inside the frame, as on the Project panel's Effects tab.

## Info

The pixel color and position under the pointer in the Viewer, the selected
clip's in, out, duration, and source, and while dragging, the drag offset and
the duration being trimmed. Errors from drivers and plugins show here with the
property or effect they came from.

## Scopes

Waveform (luma, RGB parade, YCbCr parade), vectorscope (with skin tone line and
75 % and 100 % targets), and RGB histogram, each sized to the panel. Scopes
measure the presented frame off the UI thread: on the GPU when the frame is a
texture on the window's device and no plugin scope is registered, and on the
CPU from a readback otherwise. During playback they measure at most 30 frames
a second.

## Color

Sections, each opened and closed by its header: Basic Correction
(temperature, tint, exposure, contrast, highlights, shadows, whites, blacks,
brightness, saturation), Creative (faded film, vibrance, sharpness, hue), RGB
Curves (shadows, midtones, and highlights of the master, red, green, and blue
curves), Hue Curves (saturation, hue, and luma of six hues), Color Wheels
(lift, gamma, gain), HSL Secondary, and Vignette. Every control is a
keyframeable property of the clip's grade, the same rows as the Effect
Controls Look tab. A grade property no section lists is drawn under Other.

## Text

Character and Paragraph panels edit the selected text clip or text range:
font, style, size, tracking, leading, kerning, fill, stroke, shadow,
background, alignment, and justification. Text is edited in the Viewer with the
Type tool.

## Align

Aligns and distributes the selected clips' bounds to the frame, the selection,
or the first selected clip.

## Markers

Every ruler marker as a row, in time order: time, label, color, and note. A
click on a row moves the playhead. The swatch cycles the marker's color, Edit…
opens the marker dialog, Rename edits the label and Note the comment on the
row, and Remove deletes the marker. The color filter above the rows shows the
markers of one color.

## History

Every undo step with its name. Clicking a step undoes or redoes to it; steps
after it remain until the next edit.

## Jobs

Background work: exports, proxies, analysis, plugin jobs. Each row shows
progress, elapsed time, estimate, cancel, and on completion a reveal action.

## Export

The export dialog shows the destination and the encoder, and sets the format,
scale (with the resulting frame size), frame rate, codec, bitrate, keyframe
interval, audio (Included or Video only), and range (the whole timeline or the
marked range). Format offers the platform encoder, which writes `.mp4`, and
every encoder a plugin contributes. Codec offers H.264 and H.265 as the
platform encoder reports them; a backend that sets its own bitrate shows the
bitrate as a fact. Export frames… writes the range as one PNG per frame to a
folder. Exports run one at a time, in the order they were started.
**File > Export > Timeline…** writes the timeline itself through a timeline
exporter plugin. Each setting is one row with its label in the left column and
its choices or value beside the label; a click on a choice chip selects that
value.

## Plugins

**Settings > Plugins** lists every built-in plugin and installed package with
its version, tier, state, contributions, permissions, and measured cost (mount
time, render time per frame, time per audio block), and the failure of one that
did not load. A plugin that can be turned off ends its row in a switch. An
installed package offers Reload, Uninstall, and Open folder, and a plugin that
logged offers Log. Dropping an `.ariaplugin` package on the window, or
**File > Install Plugin…**, installs it after showing its permissions.

## Assistant

The Assistant panel sends a prompt to the configured provider as a job, which
the Jobs board can cancel. Its edits go through the session's project like any
other edit, and a turn's edits undo as one step. With planning on, a turn
validates its edits and shows them as a plan, which Apply replays and Discard
drops.
