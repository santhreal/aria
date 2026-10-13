# Panels

Every panel docks, tabs, and floats ([layout.md](layout.md#frames-and-panels)),
has a panel menu, and keeps its state per project. The Viewer and Timeline have
their own chapters; Audio Meters and Audio Track Mixer are in
[audio.md](audio.md).

## Project

The media bin. Views: list (columns: name, label, frame rate, media start,
media end, duration, video info, audio info, codec, path, date, comment;
columns resize, reorder, and sort) and icon (thumbnails, with hover scrub:
moving the pointer across a thumbnail scrubs through the clip). The slider
in the panel footer sets thumbnail size.

- Bins nest. Double-click opens a bin in a new tab of the panel, Ctrl+double-click
  opens it in place.
- A search field filters by any column; the filter menu adds searches by type
  (video, audio, still, sequence, offline, unused).
- New Item menu: Sequence, Bin, Color Matte, Adjustment Clip, Title, and every
  generator a plugin contributes.
- Dragging an item to the Timeline drops it with the source patching; dragging
  onto the New Item button creates a sequence matching the clip.
- Items not used in any sequence draw their name in `text_muted` when the
  "Show usage" option is on; the usage count column counts uses.
- Offline items have a red icon and offer Link Media.

## Effect Controls

The properties of the selected clip, one group per property group, in the
order of the timeline's property rows. To the right of the value column, a
small time area spans the clip's range with its own playhead and keyframes, as
in Premiere's Effect Controls. Dragging an effect from Effects & Presets onto
this panel adds it to the selected clip at the drop position in the stack.

Each effect header shows a bypass toggle (fx), the effect name, a reset button,
and a menu with Rename, Duplicate, Save Preset, Copy, and Remove. Effects
reorder by dragging their headers. With several clips selected, the panel edits
the shared properties of all of them; differing values show as a dash.

## Effects & Presets

A searchable tree of every effect, transition, generator, and animation preset,
grouped by category, including plugin contributions with their plugin's name
in the tooltip. Typing filters the tree and opens the matching categories.
Double-click applies to the selection; dragging applies to the drop target (a
clip on the timeline, an edit point for transitions, Effect Controls).
Favourites (star) appear in a top group. A contribution whose plugin failed to
load is listed with its failure.

## Info

The pixel colour and position under the pointer in the Viewer, the selected
clip's in, out, duration, and source, and while dragging, the drag offset and
the duration being trimmed. Errors from drivers and plugins show here with the
property or effect they came from.

## Scopes

Waveform (luma, RGB parade, YCbCr parade), vectorscope (with skin tone line and
75 % and 100 % targets), histogram, and RGB parade, each sized to the panel.
Scopes analyse the presented frame on the GPU after presentation and update at
most at 30 Hz during playback, off the UI thread.

## Color

Basic correction (white balance, tone: exposure, contrast, highlights, shadows,
whites, blacks, saturation), creative (look LUT with intensity, faded film,
sharpen, vibrance), curves (RGB and hue-vs curves), colour wheels (shadows,
midtones, highlights), HSL secondary, and vignette. Every control is a
keyframeable property of the clip's colour effect.

## Text

Character and Paragraph panels edit the selected text clip or text range:
font, style, size, tracking, leading, kerning, fill, stroke, shadow,
background, alignment, and justification. Text is edited in the Viewer with the
Type tool.

## Align

Aligns and distributes the selected clips' bounds to the frame, the selection,
or the first selected clip.

## Markers

A list of sequence and clip markers with time, duration, name, comment, and
colour. Clicking a row moves the playhead; editing a cell edits the marker.

## History

Every undo step with its name. Clicking a step undoes or redoes to it; steps
after it remain until the next edit.

## Jobs

Background work: exports, proxies, analysis, plugin jobs. Each row shows
progress, elapsed time, estimate, cancel, and on completion a reveal action.

## Export

The export dialog sets format, codec, preset, range (work area, whole
sequence, custom), frame size, rate, bitrate, and audio settings, shows the
estimated file size, and either exports now or queues. Formats come from
encoder plugin contributions. The shipped set is H.264, HEVC, ProRes on macOS,
AV1 where the platform encoder exists, image sequences (PNG, TIFF, EXR), WAV,
and AAC; exporters for EDL, FCPXML, and OTIO export the timeline itself.

## Plugins

Every installed plugin with its version, tier (native or WASM), state (active,
inactive, failed with reason), permissions, contributions, and measured cost
(mount time, per-frame render time, per-block audio time). Rows offer enable,
disable, reload, uninstall, and open folder. Dropping an `.ariaplugin` package
on the window or on this panel installs it after showing its permissions.

## Assistant

The assistant is a plugin with a panel. It reads the project and proposes edits
through the same command path as every other plugin; each proposal shows the
commands it will apply and is applied as one undo step.
