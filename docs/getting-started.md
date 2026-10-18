# Getting started

Aria opens on Home. Start a new project, open a `.aria` file, or drop
media onto the window.

## New project

1. Click **New Project**, or press Ctrl+Alt+N (Cmd+Option+N on macOS).
2. The editor opens with an empty timeline.
3. Click **Import** in the Project panel, or drag files onto the window.
4. Drag a media tile onto a lane, or select it and press `,` to insert it at
   the playhead.
5. Titles, Shapes, and Stickers tabs place a generator at the playhead.

Supported import types: `mp4`, `mov`, `mkv`, `webm`, `wav`, `mp3`, `png`,
`jpg`, `jpeg`, `webp`. Linux video and audio decode through the host
GStreamer plugins. Stills work without them.

## Dropped files

What a dropped file becomes follows its type, on Home and in the editor alike:

| File | Result |
| --- | --- |
| `.aria` | Opens as a project, replacing the session |
| `.edl`, `.fcpxml`, `.otio`, `.srt`, `.vtt`, or another extension a timeline plugin reads | Read into the open timeline, one undo step per file |
| Media (see the list above) | Imported into the media pool |
| `.cube` | Applied to the selected clip; with no clip selected, read as a LUT filter over the sequence |
| `SKILL.md` | Installed as an assistant skill |
| Anything else | Imported as media, which reports why it cannot be |

A project file dropped alongside other files wins: opening one replaces the
session, so the rest would be discarded.

## Timeline files

**File > Import Timeline…** reads another editor's cut into the open project,
and **File > Export Timeline…** writes the open timeline as one. The shipped
Timeline interchange plugin reads and writes these formats:

| Format | Extension |
| --- | --- |
| CMX 3600 EDL | `.edl` |
| Final Cut Pro XML | `.fcpxml` |
| OpenTimelineIO | `.otio` |
| SubRip subtitles | `.srt` |
| WebVTT subtitles | `.vtt` |

A plugin can add more. The dialogs list every format a mounted plugin reads or
writes. **File > Import…** and a drop onto the window also read a timeline file
into the timeline rather than the media pool.

**File > Import Recent** lists the last ten files imported through
**File > Import…**, newest first, and imports the chosen one again. A file that
no longer exists is listed disabled. **Clear Recent Imports** empties the list.
The list is saved as `import-recents.json` in the data folder.

An import adds lanes and clips to the open project as one undo step, and
imports the media files listed in it. The export format follows the extension of the
name you give; a name with no extension takes the first format listed. An
export that fails leaves an existing file at that path as it was.

A format holds less than an Aria timeline. An EDL holds one picture track and
four sound channels, for example, and no format holds a generated clip. What a
run places or writes the nearest way, or leaves out, is listed in the
**Timeline file** panel in Effect Controls, one line per item. The panel stays
until you click **Dismiss**, and the next import or export replaces it.

A subtitle file adds a text track named after the file, with one text clip per
cue on the nearest frame. Cues that overlap go to further text tracks, numbered
`(2)`, `(3)`, and so on. Styling tags and WebVTT cue settings are dropped.
Exporting subtitles writes the text clips of every shown text track as cues in
start order; hidden text tracks and clip styling are left out and listed.

**File > Import Timeline…** also lists **Cube LUT** (`.cube`), read by the
Effects plugin. It adds a filter track named after the file with one LUT
filter bar across the sequence, or five seconds long on an empty sequence. A
file that is not a 3D Cube LUT is refused and the project is unchanged.

With the Timeline interchange plugin turned off in **Settings > Plugins** and no
other timeline plugin installed, both rows report that no importer or exporter
is mounted.

## Missing media

If a media file moves after save, its pool row is marked missing. Click
**Relink** on the row to point it at the file, or **Relink All** to pick one
folder and repair every missing row from it.

Relink All searches the folder and up to four levels below it, matching by file
name, then checks each same-named file against what the project recorded for
that row: frame size, duration, and whether it has audio. A row is repaired
only when one file matches. Anything else leaves the row missing and
states why on it:

| Row text | Meaning |
| --- | --- |
| not in that folder | No file there has that name |
| N files there match | Several files match everything recorded; relink that row by hand |
| the file there has a different frame size / duration / audio | The name matched, the file is another source |
| could not read the file there | The file exists and did not open |

## Project panel tabs

The Project panel's top row holds five groups: **Media**, **Graphics**, **Effects**,
**Library**, and **Analyze**. Choosing a group opens its first tab, and a group
with more than one tab lists its tabs in a smaller row underneath:

- **Media**: every imported file. **Audio only**, the icon beside the search
  box, lists the audio files only; see [Media pool](#media-pool).
- **Graphics**: **Titles**, **Shapes**, and **Stickers** place a generated
  clip at the playhead on a click. **Masks** applies a mask to the selected
  clip.
- **Effects**: **Effects** is every effect, transition, generator, clip type,
  and animation preset as one tree; see [Effects & Presets](#effects--presets).
  **Transitions** applies a transition at the selected clip's right cut,
  **Generators** places a generator or plugin clip type at the playhead, and
  **Animations** applies an animation preset to the selected clip.
- **Library**: **Stock** and **SFX** are the cloud libraries; see
  [Stock and sound effects](#stock-and-sound-effects). **Generate** sends a
  prompt to fal.ai; see [Generate](#generate).
- **Analyze**: **Captions** transcribes the selected clip offline; see
  [Captions](#captions). **Analysis** detects shots, silences, and beats on the
  selected clip; see [Analysis](#analysis).

With no clip selected, or on a clip that rejects the entry, a click that
applies an entry reports why. Markers are listed in the Markers panel; see
[Markers](#markers).

Drag instead of clicking to choose the target. Drag a sticker onto a lane to place
it at that lane and time. Drag an animation, mask, or transition onto a clip to
apply it there; the clip becomes selected, so Effect Controls shows what landed. A
sticker dropped on a lane that cannot hold it keeps the time and moves to the
sticker lane.

Hover an **Animations** entry to see it on the selected clip before you choose it.
The preview is not an edit: it makes no undo step, and moving the pointer off the
row restores the clip. With no clip selected there is nothing to preview.

## Effects & Presets

Window > Effects & Presets, or Shift+5, brings the Effects & Presets panel
forward, opening it in the right column when it is closed, with its search box
taking keys. The Project panel's **Effects** tab shows the same tree and search.
The tree has one folder per group:

- **Video Effects** and **Audio Effects**, each with one folder per category
  and the Other folder last.
- **Transitions**.
- **Generators**: generators and plugin clip types. The editor ships Color
  Matte, Linear Gradient, Radial Gradient, Checkerboard, Grid, Fractal Noise,
  and Color Bars; see [Generators](#generators).
- **Animation Presets**: saved presets and plugin presets. A saved preset hides
  a plugin preset of the same name.

Click a folder to open or close it. Click an entry to pick it. Double-click an
entry to apply it to the selection:

| Entry | Double-click |
|---|---|
| Video effect | Appends it to the selected clip's chain |
| Audio effect | Appends it to the chain of every selected clip with sound |
| Transition | Sets it at the selected clip's right cut |
| Generator, clip type | Places a clip at the playhead |
| Animation preset | Applies it across the selected clips, anchored at the playhead |

Drag an entry onto a clip to apply it to that clip, a transition onto a cut, and
a generator or clip type onto a lane to place it there. Hover an entry to show
where it comes from (Built in, Saved preset, or the plugin's name) and the two
gestures: double-click and drag.

Drag a video effect onto the Effects tab of Effect Controls to add it to the selected
clip's chain: dropped on an effect, it goes above that effect; dropped anywhere
else in the tab, it goes at the end. The drop is one undo step. Any other entry
that applies to a clip applies to the selected clip. A generator, clip type or
sticker dropped there is refused, with a message to drop it on a lane.

Click the star on an entry to list it under **Favourites**, the first folder.
Click it again to remove it. Favourites are saved with the settings.

Type in the search box to filter the tree. The search ignores case and matches
the name, the id, and the category. Every folder holding a match opens, and a
group with no match is hidden. The `×` at the right of the box empties the
search. Esc returns keys to the editor.

**Failed to Load**, the last folder, lists each plugin that did not mount and
each preset file that did not read, with the reason.

A workspace can list one group alone, as the Audio workspace lists Audio
Effects. A caption over the tree states the group. Favourites from that group
and Failed to Load stay listed. **Show All** lists every group again.

### Generators

A generator draws a picture from its parameters alone. Placing one adds a
transparent solid the size of the frame, named after the generator, with the
generator first in its effect chain. The Effects tab of Effect Controls edits and
keyframes its parameters, and effects added below it process its picture.

| Generator | Draws |
|---|---|
| Color Matte | One colour over the frame |
| Linear Gradient | A ramp between two colours from a start point to an end point |
| Radial Gradient | A ramp from an inner colour at the centre to an outer colour at the radius |
| Checkerboard | Square cells of two colours, anchored at a point |
| Grid | Lines of one colour at an even spacing, anchored at a point |
| Fractal Noise | Value noise between a dark and a light colour; keyframe Evolution to animate it |
| Color Bars | SMPTE bars: seven bars at the bar level, the reversed blue row, and a strip of -I, white, +Q, black, and a pluge step |

Points and anchors are fractions of the frame from its top-left corner. The
radius and the noise offset are fractions of the frame height. Cell size,
spacing, line width, and noise scale are pixels of a 1080-pixel-high frame, so a
proxy or a reduced preview draws the same picture.

## Text

Select a title. The Text tab shows its content and **Edit text…**, which renames
the text in place. Below it, the Effect chips are the text presets: neon, shadow,
outline, glow, retro, and chrome. Click a chip to apply it, click the active chip
to clear it. A preset writes stroke, shadow, and background onto the style, so
clearing the chip stops marking the preset but leaves what it wrote; edit those
rows to change them. A preset writes only when chosen: an edited stroke stays
when the title is retyped or the same preset is sent again.

Under **Font**, click a family to set it; **Next font** and **Previous font**
step through the installed families. **Style** holds Bold, Italic, Underline,
and Kerning, then the case: Normal, Upper, Lower, Title. **Treatments** switches
the stroke, the shadow, and the card on and off; switching one clears the
preset mark. **Size and spacing** holds size, tracking, leading, and the
treatment sizes, and **Colour** holds a swatch row per colour: fill, then the
stroke, shadow, and card colours while those are on. A colour keyframed at the
playhead takes a new keyframe; any other colour is set for the whole clip.
**Alignment** sets the lines Left, Center, Right, or Justify across the frame
and the block Top, Middle, or Bottom down it; **Wrap** breaks long lines at the
frame edge.

**Window > Character** and **Window > Paragraph** dock the same controls as
panels, in the right frame beside Effects & Presets in the Editing workspace.
They edit whichever title is selected.

With the Type tool (T), click empty picture to place a new title; its
placeholder text is selected, so typing replaces it. Click a title to place the
caret between two characters, drag across it to select, and Shift+click to
extend the selection. The arrow keys, Home, and End move the caret, Shift
extends the selection, Enter starts a new line, and Esc stops typing. Each
burst of typing is one undo step. With characters selected, Bold, Italic,
Underline, the font, size, tracking, and fill colour apply to those characters
only, and the controls show their values; with no characters selected they
apply to the whole title. Leading, case, alignment, and the treatments always
apply to the whole title.

Retyping a title keeps its font, size, colour, alignment, and effect.

## Edit

- Space plays and pauses.
- Ctrl+K splits the selected clip at the playhead; Ctrl+Shift+K splits every
  track.
- Drag a clip to move it. Drag a clip edge to trim. Nearby clips magnet
  at 8 px. The magnet also takes the playhead, the sequence start and end, the
  In and Out points, markers, and detected beats. While a drag is in flight the
  guide line states what it locked onto. Hold Alt to suspend the magnet for
  that one drag; the Timeline
  header's magnet stays on.
- Drag on empty timeline space to rubber-band: every unlocked clip the band
  touches is selected, in either direction, across every lane between the two
  corners. A press with no drag clears the selection. Esc abandons the band and
  leaves the selection as it was.
- Arrow keys nudge one frame. Shift+Arrow nudges ten.
- Delete or Backspace ripple-deletes the selection.
- Ctrl+Z / Ctrl+Y undo and redo.
- Double-click a media clip to open its take in a Source tab linked to the
  clip (see [Previewing a source](#previewing-a-source)). Double-click a nested
  sequence to open it, and a title, shape, or other generated clip to open its
  property rows.
- A clip with no name of its own shows its source's: a media clip the file name
  the pool lists, a multicam clip its group's name, a title its text, and a
  placed generator its generator's name. Double-click a clip's name to rename
  it. The name is stored on the clip, so it survives a relink or a media swap;
  committing an empty name gives the card the source's own name back.
- Renaming a clip, track, marker, effect, or the project opens a name field in
  the middle of the window with the whole name selected: typing replaces it,
  and an arrow key keeps it and moves the caret to that end. Shift with an
  arrow, Home, or End selects; Ctrl with an arrow moves by word; Ctrl+Z puts
  back the name the field opened with. Enter applies the name and Esc cancels.
- `\` fits the timeline to the sequence, Shift+F to the selection. + / - zoom.
- Right-click a clip, track, or the preview for a context menu.
- **Clip > Enable** enables or disables the selected clips. A disabled clip
  draws at 40 per cent opacity on its lane and produces no picture or sound in
  preview or export.
- **Clip > Group** joins two or more selected clips into a clip group, marked
  with `[G]` on each card. Clicking or moving one member selects or moves the
  group, while trimming an edge trims only that clip (or its linked A/V pair).
  Hold Alt while clicking to select one member alone. **Clip > Ungroup**
  dissolves the group.
- **Sequence > Linked Selection**, or the chain button in the Timeline header,
  turns linked selection on or off. On, a click selects a clip with the clips
  linked to it, and a move or a trim moves them together. Off, a click, a move, a
  nudge, and an edge trim apply to the one clip; grouped clips still move
  together.
- **Clip > Audio Gain…** opens Audio Gain for the selected audio-bearing clips.
  It reports the measured peak in dBFS across the selection and offers four
  modes: **Set Gain to** sets clip volume in dB (`-60` to `+12` dB), **Adjust
  Gain by** adds a dB offset (`-60` to `+60` dB), **Normalize Max Peak to**
  shifts every selected clip by the same offset so the loudest clip reaches the
  target (`-60` to `0` dBFS), and **Normalize All Peaks to** scales each
  selected clip to the target (`-60` to `0` dBFS) individually. Applying the
  dialog is one undo step.
- **Clip > Audio Channels** sets how the selected clips' left and right channels
  play: **Stereo**, **Swap Channels**, **Left to Both**, **Right to Both**, or
  **Mono**. A choice other than Stereo places the matching channel tool first in
  each clip's audio effects, replacing any channel tool already there; Stereo
  removes it. The check mark shows the choice every selected clip shares. Each
  choice is one undo step.
- F is Match Frame: it opens the source of the clip under the playhead in the
  monitor, on the same frame, with that clip's take marked. The selection wins
  when it is under the playhead; otherwise the topmost clip does.
- With a source loaded in the monitor, `,` inserts its marked range and `.`
  overwrites with it. The sequence's own In point is where the clip lands, and
  the playhead stands in when there is none; a sequence Out alone backtimes the
  take to end there. With In and Out on the sequence and only an In or an Out on
  the source, the sequence span sets how much of the source is taken. With
  all four points set and the two lengths different, the edit is Fit to Fill:
  the take is retimed to the sequence span and the status line shows the new
  speed. A still takes the sequence span without a retime.

### Trim tools

The tool bar picks what a drag means. Five tools trim without moving a clip's
edges the way a plain drag does. Each keeps something fixed:

- **Slip (Y)** keeps the clip where it is and moves the take inside it. Drag the
  clip body: the content follows the pointer and the timeline does not change.
  A slip stops at either end of the source. A still, a freeze frame, a generated
  clip, and a clip already using its whole take have nothing to slip.
- **Slide (U)** keeps the clip's length and gives the difference to the clips
  either side of it. Drag the clip body. A neighbour keeps at least one frame, so
  the slide stops rather than deleting it. Sliding into empty space is a move.
- **Rolling edit (N)** moves the cut between two abutting clips. Drag the edge
  that is the cut: one clip grows by what the other gives up, and the pair's
  outer edges stay put. An edge with no clip against it has no cut to roll.
- **Rate stretch (R)** changes how long a clip plays without dropping any of its
  source. Drag an edge: the speed follows from the new length. A clip already
  retimed is stretched from where it is, so two drags compose.
- **Ripple trim (B)** trims an edge and moves the rest of the lane by the same
  amount, so the trim leaves no gap. Drag either edge: every clip later on that
  lane follows. Trimming the head slides the clip back to where it started, so
  the lane keeps its own start.

The five share one tool bar button, which shows the armed trim tool, or Ripple
trim when none is armed. Hold the button for 300 ms, or click its corner
triangle, to list the five; each key selects its tool directly.

Slide, a rolling edit, and a ripple trim each issue two edits and cost one undo.
If the second edit is refused the first is rolled back, so the timeline never
keeps half a gesture.

Ctrl+C copies the selected clip, and there are four pastes:

- **Ctrl+V** drops the copy at the playhead, on the copied clip's own lane. What
  is already there is not moved, so a paste over a clip is refused.
- **Ctrl+Shift+V** (Paste Insert) first slides that lane right by the copy's
  length from the playhead on, then fills the hole. Nothing is overwritten.
- **Ctrl+Alt+V** (Paste Attributes) writes the copied clip's transform, filter,
  and whole effect chain onto the selected clip. Its position, length, media, and
  audio levels are unchanged. The target's existing effects are replaced, not
  added to, so pasting from a clip with no effects clears them.
- **Paste To Fit**, on the Edit menu, drops the copy into the marked range and
  retimes it to fill the range. The copy lands at the In point, not at
  the playhead. Mark In and Out first: with no range marked the row reports it and
  changes nothing.

The clipboard holds a reference, not a copy: deleting the copied clip makes the
pastes do nothing rather than paste a stale version of it.

The Effect Controls panel shows transform, look, effects, audio, text, and
info for the selected clip. Tabs hide when they do not apply.

The strip along the bottom shows the timecode, the frame rate, the track count,
and the last message. A status is a receipt for the last action and clears
after four seconds. An error stays until an action succeeds: an edit, an undo, or
a redo replaces it.

The playhead timecode at the left of the Viewer's control row is a field, and so
is the timecode at the top left of the timeline. Each reads in the sequence's
time display (see [Canvas](#canvas)): timecode, a frame count, or feet and
frames.
Drag across it to scrub the playhead: two pixels move one frame, Shift moves ten
frames, Ctrl a tenth of one, and Esc during the drag puts the playhead back.
Click it to type a position: a bare number is a frame count, and one to four
colon-separated fields fill from the right, so `12` is twelve frames, `5:12` is
five seconds and twelve frames, and `01:00:05:12` is the full clock. `;` and `.`
work where `:` does, so `1.00` is one second. `feet+frames` reads 35 mm feet of
sixteen frames, so `12+08` is frame 200. Enter seeks. Anything that is not a
time is refused by name and the playhead stays where it was. Esc leaves the
field without seeking. A position past the end of the sequence lands on the end.

A timecode second holds the rate rounded up: 24 frames at 23.976 and 30 at
29.97.

The field opens without a click: a digit typed over the Timeline starts the
timeline's timecode with that digit, and one typed over the Program view starts
the Viewer's. A leading `+` or `-` (the keypad keys open the
field with the sign) makes the typed value a duration. With clips selected,
`+12` Enter moves them twelve frames later as one undo step, together with
their grouped clips and, with linked selection on, their linked clips, and stops
at the sequence start. With an edge
selected it trims the edge. With nothing selected it moves the playhead.

## Timeline header

The header over the track headers has two columns. The left one holds the
sequence timecode, a playhead field like the Viewer's (see above), drawn as
figures until the pointer is over it. The right one holds, top to bottom:

- The property search. Typing in it narrows the property rows of every opened
  clip to the properties whose names contain the text, ignoring case, and
  opens the groups that hold them; a group with no match is hidden. Enter,
  Esc, or a press anywhere else leaves the field and keeps the filter; the
  **×** button clears it, and each clip shows the groups it had open before.
- Five buttons: snapping (`S`), linked selection, add a marker at the playhead
  or edit the one there (`M`), timeline display settings (see
  [Timeline display settings](#timeline-display-settings)), and the graph
  editor (Shift+F3).

The ruler to the right labels its ticks in the sequence's time display. The
spacing follows the zoom: labels sit as close as their width allows and never
overlap, on whole frames that divide a second, whole seconds, minutes, or hours
for timecode, and on round counts for frames or feet. Click or drag in the ruler
to move the playhead; it stays under the pointer with no easing, and stops at
the end of the sequence when the pointer passes it.

Under the ticks, the work area bar spans `In` to `Out`. It draws in the accent
color when either point is set and faint when the work area is the whole
sequence. Drag the bar to move both points, drag either end to move that point,
and double-click the bar to set the work area to the whole sequence. An end
stops one frame short of the other, and the bar stops at the sequence ends. Esc
during the drag puts both points back. The render bar runs along the bottom of
the ruler (see [Render files](#render-files)).

The zoom bar along the bottom of the timeline maps the whole sequence. Drag the
window to scroll, drag either of its handles to zoom, and click outside the
window to centre the view there. At its left, **+** adds a video track and the
zoom readout fits the sequence on a click.

## Tracks

Each track header holds a collapse chevron and six toggles: mute, solo (`S`),
lock, visibility, target (`T`), and sync lock (`⇄`). Drag the right border of
the header column to widen or narrow it.

Solo is per kind. While any audio track is soloed, the other audio tracks are
silent in both preview and export; the video tracks are unaffected. While any
video track is soloed, the other video tracks drop out of the composite. Solo
does not override a track's own mute: a muted track stays silent while soloed.

Right-click a track header for the rest:

- **Move Track Up** / **Move Track Down** move the track one row, which is one
  layer in the composite. The main track stays at the bottom of the stack.
- **Duck Source** marks an audio track as the voice that auto-duck listens to.
  Only audio tracks can be the source.
- **Open Gap at Playhead** shifts every clip on that track starting at or after
  the playhead one second later. **Close Gap at Playhead** shifts them back. A
  shift that would collide with another clip, or move one before the start of the
  sequence, is refused.
- **Collapse Lane** / **Expand Lane** does what the chevron does.
- **Hide Waveforms** / **Show Waveforms** and **Hide Filmstrips** / **Show
  Filmstrips** turn that lane's strips on or off.
- **Lane Height: Mini**, **Small**, **Medium**, and **Large** set the lane to
  20, 32, 48, and 72 pixels.

Drag a track header onto another header to move that lane to the row you release
over. The target row is marked while the pointer is over it, Escape abandons the
drag, and a drop is one edit. The main track stays at the bottom of the stack.

Drag a lane's lower border to resize it, or Shift+scroll over its header to step
through the Mini, Small, Medium, and Large presets. Heights below 20 or above
200 pixels are clamped. The drag is one edit, so one Undo restores the previous
height, and
Escape abandons it. Lane height, the collapse state, and the two strip options
are stored with the sequence: reopening the project gives back the layout you
left.

The chevron collapses a lane to a 20-pixel strip: each clip is drawn as a flat
bar in its own colour, so the cut pattern stays readable while the lane takes
almost no room. The height is kept, so expanding restores the lane you sized.

**Shift selection…**, in a clip's context menu or the command palette, prompts for a
signed frame count. The offset moves the selection on every lane it spans, from
the earliest selected clip on each, carrying everything later on those lanes with
it. A leading `+` is accepted. An offset that would move a clip before the start
of the sequence is refused whole, and anything that is not a whole number of
frames is reported rather than guessed at.

Dropping media on the timeline follows the edit mode, which the chip after the
tool bar's tools shows as **Ripple** or **Overwrite**; click it to switch, or
rest the pointer on it for what each does. In Ripple mode the drop
opens a hole first and pushes the clips at or after that point right on the
patched and sync-locked lanes, so nothing is covered. Otherwise the clip lands
where it was dropped. See [Routing](#routing) for the lanes the picture and the
sound land on.

### Clip bars

A clip bar has a 14-pixel name strip at the top, the content strip under it,
and a line across the bar.

The name strip holds the clip's name, its speed when it is not 100 % (`120%`,
`-100%` for a reversed clip; a speed ramp prints none), and its badges. The fx
badge is grey on a clip with no changes, purple when a clip property such as
Opacity, Position, Crop, Time Remapping, Volume, or Pan differs from a new
clip, yellow when an effect, a grade, a mask, or a key is applied, and yellow
with a purple underline when both hold. Linked clips whose sources have drifted
apart print the offset in frames, such as `+3`, on both bars.

The content strip draws thumbnails on a video clip, as the timeline display
settings choose, and a rectified waveform on an audio clip. A generated clip
draws its name and, for a colour generator, a swatch of its colour. A clip whose
media file is missing fills red and reads **Media Offline** with the file name.
A disabled clip is drawn at 40 % opacity.

The line draws one property of the clip over time: Opacity on a picture lane
and Volume on an audio lane, until another property is chosen. Click the fx
badge to choose: the menu lists every property of the clip by block
(`Transform: Rotation`, `Gaussian Blur: Radius`, `Mask 1: Feather`), with the
drawn one marked, and **Show in Effect Controls** selects the clip and opens
that panel. Choosing the lane's default property clears the choice, so a clip
moved to the other kind of lane draws that lane's default.

Drag the line up or down to set a property that is one number. The value reads
out as it moves, Escape puts it back, and the drag is one undo step. A drag
between two keyframes moves both; a drag on a keyframe dot moves that keyframe.
Ctrl+click on the line adds a keyframe at that time. On Medium and taller lanes
the bar draws a dot at each keyframe of the property. A vector property draws
its first axis, and a colour or a rectangle draws a level line; neither is
dragged.

### Routing

Source patching picks which lanes a source's picture and sound land on: Insert
and Overwrite from a Source tab or for the selected pool items, a drag from a
Source tab, and a media drop. Each video and audio lane header has a source indicator, V or A. The lit
one is where that channel lands; click a dark one to move the channel there, and
click the lit one to leave the channel out. A channel nobody patched goes to the
topmost targeted lane of its kind, else the kind's first lane, and a video lane
is created if the sequence has none. The sound lands as a clip of its own on the
patched audio lane, linked to the picture, so the two move and trim together.
With no audio lane in the sequence, the sound stays in the picture clip. A
picture placed with its sound patched off is silent. A drop aims at the lane
under the pointer: that lane takes the channel of its kind, whatever the patch,
and the other channel follows the patching.

Sync lock sets which other lanes follow a ripple edit. A lane with sync lock
set moves by the same amount as the lane being edited, so picture and sound stay
in step. Clearing it pins that lane: a ripple edit elsewhere then slides past it.
The lane being edited always moves, regardless of its own sync lock, and the
whole ride-along is one Undo.

An insert opens a hole and keeps every clip: a sync-locked clip lying across the
insert point is cut there and both halves survive. A ripple delete removes a
stretch of timeline, so a sync-locked lane loses that stretch as well: a clip of
its own inside the deleted span goes, and a clip lying across either edge is cut
so only the part inside the span is lost. One Undo brings the lane back whole.
A ripple move does both halves on every sync-locked lane at once: it closes the
span the clip left and opens the span where it lands, so the material between
the old and new position slides with the move. Clear sync lock on a lane to
keep any of these edits elsewhere from touching it.

A cover moves nothing, so sync lock has no effect on an Overwrite-mode drop or
on Fit to Fill without ripple. Closing a gap does not consult it either.

Both flags are set on every new lane, and both are stored with the sequence.

## Nested sequences

A nest is a stretch of timeline that plays as one clip. **Sequence > Nest
Selection**, or `Nest selection into a sequence` in the palette, takes the
selected clips, moves them onto a sequence of their own, and leaves a single
clip in their place on the lane the first of them came from.

Select that clip and use **Sequence > Open Nested Sequence** to work inside it.
The timeline, the lanes, the markers and the preview are then the nest's own,
and every edit lands there rather than on the sequence you came from. The
status strip holds the nest's name while it is open; clicking that chip
leaves it, as does **Sequence > Leave Nested Sequence**.

**Sequence > Place Nest At Playhead** puts a second clip playing the same nest
at the playhead, on the lane a media drop would land on. Editing inside the
nest changes both clips, because both play the one sequence.

A nest clip draws the whole frame of its sequence: every visible lane, with
its transitions, mattes, adjustment layers, and nests, over a transparent
background. That frame is fitted into the canvas the clip is placed on, as
footage is, and then takes the clip's transform, crop, effects, mask, color,
LUT, layer styles, blend mode, and motion blur as one picture. A sequence of
another shape fills the middle of the canvas; a larger one draws at the
canvas size.

A sequence that shows itself, directly or through another nest, cannot be
placed. A project edited outside Aria that holds one fails the frame with a
sequence cycle error naming the sequence.

Nesting nothing, opening a clip that is not a nest, and leaving when the root
timeline is already open are each refused with what is missing.

## Waveforms

An audio clip draws its own waveform. Peaks are decoded once per source file,
off the interactive thread, so a clip appears immediately and its wave fills in
when the decode lands. Two clips cut from one file share that decode.

A clip shows the part of the source it plays: trimming, moving, or splitting it
redraws from the same peaks and costs no decode. A source with no audio stream
draws no wave and is not read again.

The wave is drawn at the level the mix will play: a volume change scales it, and
a fade in or out ramps it. A muted clip draws a flat line.

### Fade handles

Each clip card has a small square in its top left and top right corner. The
square sits where the fade currently ends, so its position is the fade length.
Drag it along the card to set the fade in or the fade out; Effect Controls shows
the same numbers. A drag past either end of the card stops at zero or at the
whole clip. One drag is one undo entry, and a drag that ends where it started
changes nothing. A locked clip draws no handles.

## Filmstrips

A media clip draws a filmstrip along the bottom of its card. Each cell is a
rendered frame of the sequence at that point, so the strip shows the picture
with its transform, grade, and effects applied rather than the raw file.

Cells render one at a time in the background. A picture edit invalidates cells
inside its changed timeline spans; audio, labels, lane display settings, and
other picture-neutral edits retain them. Work started before an edit is rejected
when it finishes. A cell that is not ready leaves its space empty rather than
borrowing a neighbouring frame.

## Timeline display settings

**View > Timeline Display Settings**, or the wrench button in the timeline
header, sets how the timeline draws clips and follows the
playhead. A wrench opens the choices under itself, and a second press closes
them. The choices are saved with the app settings, apply to every project, and
are not edits.

- **Head Thumbnails** draws the first frame of each video clip at its left edge.
- **Head and Tail Thumbnails** adds the last frame at the right edge. A clip
  narrower than two cells draws its head only.
- **Continuous Thumbnails**, the default, draws the filmstrip described above.
- **Logarithmic Waveforms**, the default, draws bar height in decibels down to
  -48 dB, so quiet passages stay visible.
- **Linear Waveforms** draws bar height proportional to the sample peak.
- **Hide Waveforms** draws no waveform and decodes no peaks.
- **Page Scroll**, the default, moves the view a page when the playhead nears
  an edge during playback, leaving the playhead near the left.
- **Smooth Scroll** scrolls the view with the playhead held at its centre.

A scroll by hand during playback stops the follow until the playhead leaves the
view.

## Viewing the preview

The control row under the Viewer holds, left to right: the playhead timecode
field, the zoom controls, the resolution menu, the Proxies toggle, the transport
(go to start, step back, play, step forward, go to end), the Loop toggle, the
shuttle readout, **In** and **Out** while Program is in front, the Transform,
Crop and Mask handle toggles, the Scopes toggle, and the peak meter. The toggles
are icons; hover one for its name, and a lit icon is on. The transport acts on
the tab in front of the Viewer: with a Source tab in front, the buttons move its
take and the sequence playhead stays where it was. **In** and **Out** mark the
sequence at the playhead; a Source tab marks its take from its own bar. In a
narrow Viewer the row wraps onto a second line before the transport or before
the handle toggles.

The Viewer fits the frame to the panel by default. The control row holds
`−`, the zoom readout, and `+`. The readout shows `Fit` or a whole percentage;
clicking it returns to fit. The buttons walk a fixed ladder: 25, 50, 75, 100,
150, 200, 400 per cent, and stop at either end.

Above 100 per cent the frame can be panned: the cursor becomes an open hand and
dragging moves the picture. Returning to fit, or zooming back to 100 per cent or
below, re-centres it.

The Scopes toggle opens a strip beside the Viewer with the waveform, the vectorscope,
and the histogram of the frame on screen; the Scopes panel lays the same
readings across its frame. The chips over the waveform choose what it counts:
**Luma**, **RGB** for a red, green, and blue parade, or **YCbCr** for a luma,
Cb, and Cr parade. **View > Scope Waveform** makes the same choice, and the
choice is saved. The vectorscope draws a skin tone line and a target box for
each primary and secondary at 75 and 100 per cent.

**View > Viewer Display Settings** holds the overlays, each independent of
the others. The wrench at the right of the Viewer's tab row opens the same
submenu under itself: **Split Viewer**, **Show Rulers**, **Show Guides**, **Snap
to Guides**, **Clear Guides**, **Title Safe**, **Action Safe**, **Transparency
Grid**, and **Grid**. A check mark shows each overlay that is on.

- **Title safe** marks the inner 90 per cent of the frame.
- **Action safe** marks the inner 93 per cent.
- **Grid** draws thirds.
- **Transparency Grid** draws a grid of two greys, in 8-pixel squares of the
  rendered frame, where no layer covers the canvas, in place of the sequence's
  background colour. A partly transparent layer shows the grid through it.
  Export, Export Frame, and timeline thumbnails keep the background colour.
  While the grid is on, each Viewer frame is read back from the GPU, as with
  Scopes open. The setting is saved.

**Show Rulers** under the wrench or **View > Viewer Display Settings**, or
Ctrl+Shift+R, draws rulers along the top and left of the Program view, numbered in
canvas pixels and following the zoom and pan. Drag from the top ruler for a
horizontal guide and from the left ruler for a vertical one; drag a guide to move
it, and drop it back on its ruler or off the frame to delete it. With **Snap to Guides** on, a layer moved by its transform handle snaps its edges or its
centre onto a guide within 6 pixels. **Show Guides** off hides the guides
and stops snapping to them without deleting them; pulling a new guide shows them
again. **Clear Guides** deletes every guide. Guides are not saved with the
project and are not edits.

**View > Compare** holds a reference beside the Program frame. **Current
Frame** holds the sequence frame at the playhead; move the playhead to compare
another frame with it. **Still…** prompts for an image file. **Source Tab** shows
the Source monitor's frame at its own playhead, and is disabled with no source
open. **Vertical Wipe** draws the reference left of a divider and Program right
of it, **Horizontal Wipe** draws the reference above and Program below, and
**Side by Side** draws the reference in a pane of its own left of Program. Drag
the divider to move it. The reference keeps the Program view's zoom and pan, and
a caption states its name. **Stop Comparing** returns to Program alone. A still that
cannot be read stops the comparison with an error that states the file.

**View > Fullscreen Preview** fills the window with the viewer, hiding the
other panels. Escape gives them back. A dialog opened by a
shortcut still appears over the picture, and the status line stays visible so a
refusal remains readable.

### Resolution

The resolution chip in the control row lists two settings. **Playback** is
the size frames render at during playback: **Full**, **1/2**, **1/4**, **1/8**,
or **Auto**. Auto starts at Full, drops one level after three dropped frames
within a second, and rises one level after five seconds of playback with no
drop. **Paused** is the size a stopped or scrubbed frame renders at: **Full** or
**1/2**. A reduced frame is rendered small and scaled up to the pane, so it
costs less to render and shows softer.

The chip and the status line show the playback setting, and under Auto the
level it is at, such as `Auto 1/2`. **View > Playback Resolution** steps through
the playback choices and **View > Paused Resolution** switches between Full and
1/2. Both settings are saved with the other UI settings. Export always renders
at the sequence size.

### Proxies

Right-click a pool item and choose **Decode through a proxy** to render a
smaller intra-frame copy as a background job; the command palette's **Proxy
every pool item** does the same for the whole pool. The Proxies toggle in the
control row, and **View > Proxies**, switch the viewer between proxies and the
original files. The toggle is lit while proxies are on. The
choice is saved with the other UI settings. Export always reads the original
files, and a proxy missing from disk reads the original.

### Render files

**Sequence > Render In to Out** (`Enter`) renders every uncached frame in the
work area (`In` to `Out`, or the whole sequence when unset) at full sequence
resolution with motion blur enabled and writes each frame as a content-addressed
QOI file in the `render-files` cache. Preview reads a matching render file from
disk and scales it to the preview size instead of running the compositor; export
renders directly from the timeline and ignores render files. **Sequence > Delete
Render Files** removes the cached render files for the open sequence.

A `3px` render bar along the bottom of the timeline ruler colors frame runs
green when a matching render file is on disk, red when unrendered and the frame
holds a CPU or stateful effect, a nested sequence, motion blur, or more than
four picture layers, and yellow otherwise. Editing a clip changes its frame keys
and turns its span back to yellow or red until rendered again.

### Output meter

Two bars at the right of the control row show the peak level of what the
device is playing, left above right. A full bar is 0 dBFS. The bars turn red and
stay red once a sample reaches full scale; click them to clear that. The meter
shows output, not the selected clip, so soloing or muting a track is visible in
it. Stopping playback drops the bars to silence.

Zoom, pan, the overlays, and fullscreen are viewing positions, not edits: none
of them enters the undo history or changes what an export produces.

### Info

**Window > Info** or Shift+8 docks the Info panel. Over the Viewer it shows the
canvas pixel under the pointer and the colour of the frame on screen there: a
swatch, the hex value, the RGB channels, and alpha. The colour is read from the
picture as displayed, at the preview resolution, so a reduced resolution reads
the pixel that covers the canvas point. Below that it shows the selected clip's
name, In, Out, Duration, source, and Source In and Out; during a trim, the tool,
the clip, and the change; during a drag in the Viewer, the move, scale, crop,
mask, or tangent being dragged. The Errors section lists driver faults with
their clip and property, plugins that failed to mount, and plugin faults.

## Audio Track Mixer

Shift+6, or **Window > Audio Track Mixer**, shows the Audio Track Mixer in place
of the Viewer. It carries one strip per lane that has sound, plus one per submix
bus, drawn in the order the mixdown folds them: a bus sits after everything that
feeds it. The Master strip comes last and carries the master's **Inserts**.

Each strip carries:

- The lane's name. A bus is marked `· bus`.
- **Inserts**: the lane's audio effects, applied to its sum before the fader
  and the sends ([Audio effects](#audio-effects)).
- **Out**: one chip per destination. `Master` is the default; the others are the
  buses. Click a chip to route the lane there. A route that would loop back into
  the lane is refused and the routing is left as it was.
- **Sends**: one chip per bus. Click it once to send a copy of the lane to that
  bus after its fader, again to take the copy before the fader, and a third time
  to drop the send. A send reads its level beside the bus name.
- The fader, read in decibels below it. Drag it: up is louder, the travel runs
  from silence to +12 dB, and it snaps to 0 dB near unity. The strip follows the
  pointer and the project is written once on release, so riding a fader is one
  undo step.

**+ Submix bus** at the top right adds a bus, named `Bus 1`, `Bus 2`, and so on.
`Mixer: add a submix bus` in the palette does the same. Deleting a bus returns
everything that fed it to the master; undo restores both the bus and the routing.

A strip whose signal cannot arrive at the master — behind a muted bus, or with
its fader at silence — is bordered and reads `not in the mix`. Those lanes are
not decoded during export.

The assistant routes too: it adds a submix lane, moves a fader, points a lane's
output at a bus, and adds, changes or drops a send. `describe_project` carries
each lane's fader, the bus its output feeds, and the sends it holds in the order
`set_send` addresses them. A level outside silence to +12 dB, a destination that
is not a bus, a route that would loop, and a second send from one lane to one
bus are refused with what to send instead.

## Look and grade

The Look tab carries the filter catalog and, below it, the grade:

- The colour wheels: Lift, Gamma and Gain. Press anywhere on a wheel to put
  its puck under the pointer, then drag. The drag follows the pointer off the
  wheel and stops at the rim. A wheel moves the balance between its red, green
  and blue channels and leaves their level where the press found it; the
  **Master** value under the wheel sets the level and drags or types like any
  other value. The rim moves a channel a quarter for Lift and a half for Gamma
  and Gain. Releasing keeps the drag as one undo step, and Escape during the
  drag returns the wheel to where the press found it. The reset button beside
  the wheel's name returns its channels and master to neutral as one undo
  step.
- One row per grade property the catalog carries: the basic correction
  (temperature, tint, exposure, contrast, highlights, shadows, whites, blacks,
  brightness, saturation), the creative values (faded film, vibrance,
  sharpness, hue), the RGB curves, the hue curves, the twelve wheel values as
  numbers, the HSL secondary, and the vignette. Drag a row's value to change
  it, or click it to type one. **Reset grade** returns every adjustment to
  neutral.
- A blend mode for the layer. Normal composites source-over. In a matte the
  pair composites with the content clip's mode, and each side of a transition
  composites with its own clip's mode.
- Stabilize, offering the three smoothing levels and Off. The section is
  absent on a clip with no motion to smooth, such as a still.
- Denoise, which runs RNNoise over the clip's audio on export.

**Window > Color** opens the Color panel, which lays the same grade out in
sections: Basic Correction, Creative, RGB Curves, Hue Curves, Color Wheels, HSL
Secondary, and Vignette. Click a section's title to open or close it. Basic
Correction opens first and the rest start closed. **Reset grade** heads the
panel.

The Audio tab toggles pitch lock. Locked (the default), a retimed clip keeps its
original pitch: the sound is stretched in time and the pitch stays put. Unlocked,
pitch follows rate, so a fast clip sounds high and a slow one low. Both preview
and export mix the same way.

## Framing

The Transform tab carries the framing block below the transform rows:

- Crop insets for the left, top, right, and bottom edges. Each stops at the
  frame rather than refusing the edit, and the kept region never collapses.
- Flip H and Flip V mirror the content after the crop. **Reset framing**
  restores the whole frame and clears both mirrors.
- Motion blur, with a shutter angle in degrees and a sub-frame sample count.

The Viewer's control row carries three icon toggles, Transform, Crop, and Mask,
one per set of on-canvas handles; the lit one is the live set. Click one to make
it live, or choose one under **View > Canvas Handles**. Only one set is drawn at a time, because a
crop corner and a scale corner sit on the same pixel when the crop is full.

In Transform mode, dragging the picture moves it and dragging a corner scales it.
A corner scale is uniform: both axes keep their ratio. Use the Transform tab rows
for a per-axis scale. The scale stops at 0.02 and at 20. Releasing without moving
writes nothing.

In Crop mode, the preview draws crop handles on the selected clip: one dot per edge and one per
corner. Drag a dot to move that edge; a corner moves both of its edges. While you
drag, the preview shows the uncropped frame, so you can see what you are cutting
into. The window stops before it collapses instead of inverting. Escape abandons
the drag and writes nothing; releasing commits the whole drag as one edit, so one
undo restores the frame you started from.

A generated clip whose size follows the canvas gets no handles; crop it with the
inset rows.

### Align and distribute

**Window > Align** docks the Align panel. **Align to** selects what the selected
clips line up against: the box around the selection, the frame, or the first
selected clip. **Left**, **Centre**, and **Right** move each clip horizontally;
**Top**, **Middle**, and **Bottom** move it vertically. A single selected clip
aligns to the frame. **Across** and **Down** under **Distribute** keep the two
outermost of three or more clips in place and space the centres evenly between
them.

Bounds are what the compositor draws at the playhead: a rotated clip aligns by
the box around its turned corners, a title by the box its text renders into,
and a parented clip where its parents place it. A clip whose Position is
keyframed gets a keyframe at the playhead, keeping the curve and handles of a
keyframe already there; an unkeyed clip gets a new constant Position. Clips on locked lanes, clips with a
Position driver, and clips not drawn at the playhead stay where they are. One
press is one undo step. A press that would move nothing writes nothing and
reports why on the status line.

## Mask and chroma key

Below the grade, the Look tab shows the masks of every visual clip: footage,
stills, titles, shapes, stickers, animations, and adjustment layers. The masks
cut what the clip draws before its effects, LUT, and layer styles, so a shadow
or glow follows the masked edge. On an adjustment layer the masks limit the
layer's look to the masked area. The Look tab holds:

- A stack of up to 32 masks. The seven shape buttons under **Add a mask**
  (linear, mirror, circle, rectangle, heart, star, path) each add a mask at the
  bottom of the stack and make it the active mask. The palette's **Add <shape>
  mask** rows do the same.
- One header per mask: its number, its shape, and **(inverted)** when it keeps
  the outside. Click the header to make that mask the active one. The active
  mask is marked with a dot and shows its rows: feather, roundness on a
  rectangle, rotation, width and height (not on a path), the two offsets,
  opacity, and expansion. The Viewer handles, the Pen, and mask tracking act on
  the active mask.
- A mode per mask: None, Add, Subtract, Intersect, Lighten, Darken, or
  Difference. Masks apply top to bottom. The stack starts empty, or full when
  the first mask with a mode other than None subtracts, intersects, or darkens;
  each mask then replaces the coverage with its mode's result, mixed by its
  opacity. Expansion grows the mask outward by that many pixels, or shrinks it
  when negative, and scales with the clip.
- Invert, Move up, Move down, and Remove on each header. Each press is one undo
  step. The palette's **Invert mask** and **Remove mask** rows and the Effect
  menu's **Remove Mask** row act on the active mask; removing a mask makes the
  one above it active.
- Mask handles on the preview for the active mask: a dot at the centre moves the
  mask, one on its right edge resizes it, and one above it rotates it. The
  handles sit where the mask is at the playhead. The outline follows the
  pointer; the composite catches up on release, which commits centre, size, and
  rotation as one edit. A keyframed centre, size, or rotation that the drag
  changed gains a keyframe at the playhead and keeps its other keyframes; one
  the drag left alone keeps its curve. Escape abandons the drag.
- Path masks, drawn with the Pen tool (P) on the selected clip in the preview. A
  Pen press off the active path starts a new path mask; closing it adds the mask
  to the bottom of the stack. Each click adds a corner vertex, and a click-drag
  pulls a smooth one with mirrored tangents. Click the first vertex or press
  Enter to close the path; Escape drops it. A path needs three vertices and
  holds at most 1024. On an existing path the Pen inserts a vertex where it
  clicks a segment, Ctrl+click deletes a vertex, and Alt+click turns one between
  corner and smooth; an Alt+drag pulls new tangents. The Pen's Add Vertex,
  Delete Vertex, and Convert Vertex variants do the same without the modifiers.
  Any tool drags a vertex or a tangent handle. Each gesture is one undo step.
- Path keyframes. A path mask has a **Mask N Path** row at the head of its
  mask's timeline rows: a stopwatch, the vertex count at the playhead, and the
  keyframe navigator. A Path keyframe holds the whole path. The stopwatch keys
  the path at the playhead; pressed again it deletes every Path keyframe and
  keeps the path shown at the playhead. While the path is keyframed, a vertex
  or tangent drag in the Viewer keys the path at the playhead and leaves the
  other keyframes alone, and inserting or deleting a vertex changes every
  keyframe so the shape keeps moving between them. Between two keyframes with
  different vertex counts the earlier path holds. Drag a Path keyframe to move
  it in time; it snaps to the playhead and the path's other keyframes, and Alt
  moves it freely. Click one to bring the playhead to it. Right-click one for
  Linear, Ease In and Out, Ease In, Ease Out, Hold or Continuous, and Delete
  Keyframe. Each press is one undo step.
- Mask tracking from the command palette: **Track mask forward from the
  playhead**. It follows the picture under the active mask through the rest of
  the clip and keyframes that mask's centre for every frame it followed. The
  pass is a job, so the editor keeps drawing while it runs; the jobs board
  counts the frames it has read against the frames it will read, and Cancel
  stops it and keeps what it tracked. A pass that loses the feature stops there
  and names the frame. Undo removes the whole track in one step. Masks added,
  removed, or reordered while it runs do not move the keyframes off the tracked
  mask; if the tracked mask is removed, dragged, or reshaped meanwhile, nothing
  is written and the editor reports it.
- A chroma key. Pick Green, Blue, or Cyan to key on that colour at a
  starting tolerance, then adjust Strength and Shadow. Re-keying keeps the
  tolerance. Remove key clears it.
- Four layer styles — shadow, glow, outline, background. Click one to add it
  at its default, then adjust its knobs and pick a colour. Clear styles
  removes all four.
- A `.cube` LUT. **Load .cube…** picks a file; Intensity blends it over the
  ungraded image; Remove LUT clears it. Loading another LUT keeps the
  intensity you dialled in.
- In, Out, and Loop animations. Chips list the presets for that slot; the
  applied one is marked. Speed, Intensity, and Stagger appear only when the
  preset exposes them. A loop animation replaces the In and Out pair and vice
  versa. Text-only presets appear on text clips.

The Audio tab tags a clip as Music, Sound FX, Voiceover, or Extracted.
**Untag** clears it.

## Effects

The Effects tab of Effect Controls lists the effect chain in render order. Each effect
has a header: **fx**, the effect's name, ↑ and ↓ to move it one step (hidden
at the ends of the chain), a reset button, a menu button, and a bin to remove
it.

- **fx** switches the effect off and back on, one undo step per press. A
  switched-off effect keeps its parameters and its place in the chain, renders
  nothing, and draws its name dimmed.
- Drag a header onto another effect to move it above that effect, or onto the
  rest of the tab to move it to the end of the chain. The move is one undo
  step.
- The reset button puts every parameter back at its default, in one undo step.
  The effect keeps its name and its **fx** state.
- The menu button, or a right click on the header, opens the effect menu:
  - **Reset** does what the reset button does.
  - **Rename Effect** opens the name field on the effect's name. Enter keeps
    the name; an empty name puts the catalog name back.
  - **Duplicate** places a copy with every value and keyframe below the effect,
    named with the next free number.
  - **Copy** holds the effect. **Paste Below** places the copy below the effect
    the menu was opened on; Effect > **Paste Effect** places it at the end of
    the chain of every selected clip that draws a picture.
  - **Save as Preset…** opens the preset name field over that effect alone.
  - **Remove Effect** takes the effect out of the chain.

  Rows a plugin adds follow, below a rule.

Parameters of the applied effect appear under its header. Each row reads its
value at the playhead.

- A numeric parameter is a value: drag it left or right to scrub it, or click it
  to type one. A point parameter is two rows, x and y, and a 3D point is three.
- A colour parameter names its value beside the label, a preset's name or a
  hex such as `#ff0000`, with its swatches on the line under it. Click a
  swatch to set the colour.
- A parameter that can be animated has the stopwatch, the keyframe navigator,
  the reset button, and the time area described in
  [Property controls](#property-controls). Editing an animated parameter writes
  a keyframe at the playhead; editing a constant one changes the constant.

**Presets** holds the chains you saved. Click a preset to apply it to the
selected clip, replacing that clip's chain in one undo entry. **Delete** beside
a preset removes it.

**Save as preset** opens a name field over the current chain. Type a name and
press Enter, or click **Save**. A name that is empty or already taken is
refused; the field stays open with what you typed. Escape leaves without
saving. Presets are stored with your settings, so they are there on the next
launch.

**Add** lists every effect the compositor can render. Clicking one appends it
to the end of the chain.

The **Effect** menu lists every video effect and every audio effect, one
submenu per category: Blur & Sharpen, Color, Distort, Noise & Grain, and
Stylize for video; Amplitude & Compression, Channels, Delay & Echo, Filter & EQ,
Noise Reduction, Reverb, and Time & Pitch for audio. A plugin effect is listed
under the category its plugin names, or under Other or Other Audio. A video
effect row appends the effect to the selected clip's chain. An audio effect row
appends it to the chain of every selected clip with sound, as one undo step.

## Audio effects

An audio effect processes sound in a chain: on one clip, on a lane, or on the
master after every lane and bus is summed. Effects in a chain run top to
bottom. Aria ships Gain, Parametric EQ (eight bands with low and high cuts),
Compressor, Limiter, De-esser, Noise Reduction, Reverb, Delay, Pitch Shifter,
Swap Channels, Fill Left with Right, Fill Right with Left, Mono, and Loudness
Normalizer; a plugin adds more.

A clip's chain is the **Audio effects** section of the Audio tab, shown for a
clip with sound. A lane's chain and the master's are the **Inserts** row of each
mixer strip and of the Master strip. A lane and the master hold five effects
each; a clip holds any number.

- **+ Audio effect** opens the list of effects; clicking one appends it and
  closes the list.
- Each effect row carries its name, a chip that reads **On** or **Bypassed**
  (click it to switch), ↑ and ↓ to move it one step, hidden at the ends of the
  chain, and a bin to remove it.
- Parameters step within their range: left click raises a value, right click
  lowers it. In the Audio tab every effect shows its parameters; on a mixer
  strip, click an effect's name to show or hide them. A keyframed parameter
  reads `keyed`, and a step writes a keyframe at the playhead instead of
  replacing the curve.
- An effect that reads a key, such as the Compressor, shows **Key from** on a
  mixer strip: **No key** or one chip per lane that may key it.
- An effect whose plugin is not installed reads `(not installed)`. Its values
  stay in the project and the sound passes through it unchanged.

Every change is one undo step. Preview and export run the same chain; the
mixer delays every other path by the latency of the Limiter, Noise Reduction,
and Pitch Shifter so lanes stay in time.

## Transitions

A transition is a blend at the cut between two clips that touch. Select the
clip on the left of the cut; the Look tab, or the Audio tab on an audio lane,
then shows the Transition block. It is absent when the clip has no clip
abutting it on the right, and on lanes that cannot carry a transition.

- On a picture lane the catalog lists cross fade, dip to black, dip to white,
  the four wipes, and slide. On an audio lane it lists the audio crossfades.
  The applied blend is marked. Picking another replaces it.
- Duration sets the window length in ticks.
- **Center at Cut**, **Start at Cut**, and **End at Cut** position the window across the cut, after the cut, or before the cut.
- **Remove transition** clears it.

Ctrl+T puts the default picture transition, the first in the catalog, at the
selected edge or at the cut nearest the playhead on the targeted picture lanes.
**Sequence > Apply Default Transitions** runs the same command. Ctrl+Shift+T
puts Constant Power at the selected edge or at the cut nearest the playhead on
the targeted audio lanes. A selected edge on the other kind of lane is refused
with a status line naming the other chord. The Effects panel lists both sets,
under **Video Transitions** and **Audio Transitions**.

The timeline draws the window as a band at the cut according to its alignment.
Drag either edge of the band to set the length: a centered band moves both
edges symmetrically around the cut, and a start-aligned or end-aligned band
changes length 1:1 with the dragged edge. Right-click the band for **Remove
transition**, **Center at Cut**, **Start at Cut**, **End at Cut**, and the
blend catalog. All act on that cut whatever is selected.

Deleting or moving either clip so the two no longer touch drops the transition;
undo restores it.

### Audio crossfades

- **Constant Power** fades by `sin` and `cos`. The summed power stays constant,
  so two unrelated sounds keep their loudness through the cut. Ctrl+Shift+T
  applies it.
- **Constant Gain** fades linearly in amplitude. The two gains sum to one, so a
  sound continued across the cut keeps its level.
- **Exponential Fade** moves each level linearly in decibels between -60 dB
  and 0 dB.

The outgoing clip plays past the cut into its unused source media and fades
out across the window; the incoming clip starts early from its unused source
media and fades in. A clip with less unused media than half the window fades
over the part of the window it covers, reaching silence where its media ends. A
retimed clip fades inside its own extent. The crossfade replaces the clip's own
fade on that side, in playback and in export.

## Templates

The Info tab carries the template block.

- **Replace media** lists every pooled item of a compatible kind. Picking one
  repoints the clip at it and keeps the placement, transform, and effects. The
  source window keeps its length where the new media is long enough.
- **Mark as template slot** makes the clip fillable when the project is used as
  a template. Fill order steps, the media restriction is a chip row, and
  **Edit hint…** types the placeholder text. The timeline paints a slot badge on
  a marked clip.
- On a text clip, **Text editable in template** lets the text be retyped while
  the style and animation stay.

## Keyframes

The Transform tab carries a Keyframes block with one row per animatable
property: position, scale, rotation, opacity, and — on a media clip — volume and
pan. Each row shows how many keyframes the property holds.

- ◇ writes a keyframe at the playhead, pinning the value the property already
  has there. ◆ means one is already there; clicking it removes that keyframe.
- ‹ and › move the playhead to the previous and next keyframe.
- **Flatten** drops every keyframe and holds the value at the playhead.
- With a keyframe under the playhead, the easing chips shape the segment leaving
  it: Snappy, Overshoot, and Anticipate are single curves; Bounce out, Elastic
  out, and Back out expand the segment into several keyframes.

Once a property is animated, moving a Transform slider writes the keyframe under
the playhead instead of replacing the curve. The reset glyph at the foot of the
tab clears the animation.

An expanded clip draws its keyframe ticks as marks in the strip.

### Property controls

A property that can be animated has these controls, in Effect Controls and on a
timeline property row:

- The stopwatch, left of the name, turns animation on and off. A click on a
  constant property writes a keyframe at the playhead. A click on an animated
  property deletes every keyframe and keeps the value at the playhead, as one
  undo step. Alt+click adds a driver, or removes the one the property has; see
  [Drivers](plugins/drivers.md).
- Editing an animated value writes a keyframe at the playhead. With the
  playhead off the clip, the keyframe lands on the clip's nearest frame.
- ‹ and › move the playhead to the previous and next keyframe. ◆ marks a
  keyframe under the playhead, and a click deletes it. ◇ writes a keyframe
  there holding the value the property has.
- The reset button returns the property to its default. On an animated
  property it writes the default as a keyframe at the playhead and keeps the
  rest of the curve. It is dimmed when the property holds its default there.

In Effect Controls, a time area follows the reset button. It spans the selected
clip and draws the playhead and the property's keyframes. In a narrow Effect
Controls panel the time area narrows, and is left blank below 32 px, before the
property's name is shortened; hover a shortened name to read it in full.

- Click a keyframe to select it; Shift+click adds it to the selection. Drag to
  move every selected keyframe. The drag snaps to the playhead and to the
  clip's other keyframes; hold Alt to move freely. The move is one undo step,
  and the timeline's property row draws it while it is in flight.
- Press anywhere else to move the playhead to that frame, and drag to scrub.
- Escape drops a keyframe drag unwritten, and returns a scrubbed playhead to
  where the press found it.

### Several selected clips

With several clips selected, Effect Controls draws the rows of the first selected
clip. An edit there reaches every other selected clip that has the property, as
one undo step:

- A step, a scrub, or a typed value gives every clip the value the first clip
  lands on. On a point row it gives only that axis; each clip keeps its own
  value on the other axes.
- The reset button returns each clip to its own default.
- The stopwatch, the keyframe navigator's diamond, **Flatten**, and the easing
  chips act on every clip as the first clip's row drew them: a diamond drawn ◆
  deletes the keyframe at the playhead on every clip, and one drawn ◇ writes
  one on every clip. Alt+click on the stopwatch adds a driver to the clips that
  have none when the first clip has none, and removes it from the clips that
  have one when the first clip has one.
- A chip or a switch reaches the clips of the first clip's kind that read what
  the first clip reads there.
- The colour wheels' drag and reset reach every clip that grades.
- An effect added with **Add**, or dragged onto the Effects tab, lands on every
  clip on the same kind of lane; a shorter chain takes it at its end. A
  parameter edit, **fx**, a move, the reset button, and the effect menu's
  **Reset**, **Rename Effect**, **Duplicate**, **Paste Below**, and **Remove
  Effect** reach every clip holding the same effect at the same place in its
  chain. A menu opened on an effect of another clip acts on that clip alone.

A dash replaces a value the selected clips read differently at the playhead.
On a point row, a dash on one axis means the clips differ on that axis.

The time area, ‹, and › act on the first selected clip, and a timeline property
row acts on its own clip. While a title is being typed into in the Viewer, an
edit reaches that title alone.

### Property rows

The arrow at the left of a clip bar opens the clip into property rows under its
track, grouped under Transform, Effects, Masks, and Audio headers. A header
arrow opens or closes its group; a clip on an audio lane shows only Effects and
Audio. Each row shows the property's value at the playhead and the clip's
keyframes on that property.

- Drag a row's value left or right to scrub it. On an animated property the
  scrub writes the keyframe under the playhead.
- A keyframe's icon shows its interpolation: a diamond for linear, an hourglass
  for Easy Ease, a half-hourglass for an ease on one side, a square for Hold,
  and a circle for a roving keyframe.
- Click a keyframe to select it; Shift+click adds or removes it. Drag across an
  empty part of a row to select every keyframe inside the band.
- Drag a selected keyframe to move every selected keyframe in time. The drag
  snaps to the playhead and to the clip's other keyframes; hold Alt to move
  freely. The move is one undo step.
- Right-click a keyframe for Linear, Easy Ease, Easy Ease In, Easy Ease Out,
  Toggle Hold, Rove Across Time (on a keyframe between two others), Copy,
  Paste at Playhead, and Delete.

The reveal chords open rows without the arrow: Alt+P, Alt+S, Alt+R, Alt+T, and
Alt+A show only Position, Scale, Rotation, Opacity, or Anchor Point; Alt+E,
Alt+M, and Alt+L show the effect, mask, or audio rows; Alt+M includes each path
mask's Path row. Alt+U shows every animated or driven property and every
keyframed path, and a second Alt+U also shows every property whose value
differs from its default and every Path row. Adding Shift adds the property to
the rows already open instead of replacing them. Alt+` closes every row.

### Graph editor

Shift+F3 opens the graph editor under the timeline. It draws the curves of the
properties chosen by clicking a row's name (Shift+click adds a property).

- **Value** plots the property's value; **Speed** plots its rate of change.
- Each keyframe has incoming and outgoing influence handles labelled with their
  influence. Dragging a handle sets the ease speed and influence of that side.
- On Position, the Value graph also draws the spatial tangent handles that bend
  the motion path.
- **Easy Ease** and **Hold** apply to the selected keyframes, or to the keyframe
  under the playhead when none is selected.

### Animation menu

- **Add Keyframe** writes a keyframe at the playhead on every selected property
  row, holding the value the property has there.
- **Add Expression** adds a driver to the selected property row and opens it.
  A row that already has a driver opens that driver.
- **Keyframe Velocity…**, or a double-click on a keyframe, sets the incoming and
  outgoing speed (property units per second) and influence (per cent of the
  segment) of the selected keyframes, or of the keyframes under the playhead.
  **Continuous** keeps both sides at one speed. A Hold keyframe stays Hold.
- **Keyframe Assistant > Time-Reverse Keyframes** mirrors two or more selected
  keyframes of one property across their span, so the animation between the
  first and the last plays backwards.
- **Reveal Modified Properties** opens every property row of the selected clip
  whose value differs from its default, as the second Alt+U does.

Each edit is one undo step.

Edit > **Remove Attributes…** lists the attribute groups the selected clips
hold, each ticked: Motion, Opacity, Crop, Time Remapping, Volume, Pan, Effects,
Audio Effects, Grade, Mask, and Chroma Key. Remove resets the ticked groups to a
new clip's values as one undo step. Clearing Time Remapping re-derives the
clip's duration; clearing Volume keeps the fades.

## Speed

The Speed tab retimes a media clip. It is absent on generated clips, which have
nothing to retime.

- The preset row lists every built-in ramp. The active one is marked, matched by
  the ramp's shape, so a hand-edited ramp shows none as active.
- The graph draws the ramp. Time runs left to right across the clip; speed runs
  up, logarithmically, so 1x sits near the middle and the guide line marks it.
  Every point of the ramp is a dot.
- Drag a dot to move it. The first and last dot keep their position in time and
  change only their speed. The graph follows the pointer; the edit is written on
  release, and Esc abandons the drag.
- Click the graph away from a dot to add a point there. Right-click a dot to
  remove it. A ramp keeps its last two points; removing one of those is refused.
  Use **Clear ramp** to go back to flat speed.
- **Length** under the graph is the length the ramp implies, updated while you
  drag.
- Start, Middle, and End set the speed multiplier at those points of the clip. A
  flat clip gains a 1x ramp on the first edit, so only the point you touch moves.
- **Clear ramp** returns the clip to flat speed and its original length.

A ramp changes the clip's timeline length: the engine re-derives it from the
curve. A ramp that would push the clip into its neighbour is refused.

**Clip > Speed/Duration…** sets constant speed (`5%` to `10000%`), duration,
**Reverse Speed**, and **Maintain Audio Pitch** across the selected clips as one
undo step. Editing speed updates the resulting duration; editing duration
updates the speed. Values outside the supported range are rejected with the
reason shown in the dialog.

**Clip > Time Remap** toggles keyframed speed mapping (`Clip::speed_curve`) on
the selected media clip and opens the Speed tab. While Time Remap is active, the
curve maps sequence time to source time by integrating the keyframed speed
multipliers across the clip.

## Canvas

Settings carries a Canvas section while a project is open. Pick an aspect:
`auto` follows the largest video on the timeline; the rest are fixed shapes.
Preview and export both follow the choice, and it is undoable.

The section also shows the size the project renders at and where it came from,
`explicit` or `derived`. Pick **Derived** to hand the size back to the aspect
(1080 on the shorter edge) or, under `auto`, to the largest video on the
timeline. Pick a baseline row — 720p through 4320p — to set the size in pixels.
The baseline is the shorter edge, so 1080p is 1920×1080 in a wide project and
1080×1920 in a vertical one. A baseline whose long edge the encoder cannot take
is not offered.

Changing the aspect keeps the baseline and reshapes the frame. Changing it to
`auto` clears the explicit size. Both are one undo step.

The section then carries the sequence frame rate: the rate now, one row per
offered rate from 23.976 through 60, and a mismatch line when the largest video
in the pool runs at another rate. Picking a rate retimes every placement on the
timeline so each clip keeps the time it occupied, along with fades, transitions,
markers, and keyframes, so an animation runs over the same seconds it did
before. It is one undo step.

Then the audio layout: Mono, Stereo, or 5.1 Surround. The layout sets how many
channels the sequence mixes to and what export writes.

Last is the time display: **Timecode** (`00:00:12:04`), **Frames** (`292`), or
**Feet + Frames 35mm** (`0018+04`, sixteen frames to the foot). The timeline
ruler, the timecode over the track headers, the Viewer's playhead field, and the
trim readout all read in it. It is saved with the sequence and is one undo step.

## Provider keys

Settings carries a Provider keys section listing every BYOK provider Aria can use —
`pexels`, `pixabay`, `elevenlabs`, `fal` — with what each one powers and where its
key comes from. A provider with no key is listed as unset rather than omitted.

Set a key in `~/.aria/config.toml`:

```toml
[providers.pexels]
api_key_env = "PEXELS_API_KEY"

[providers.fal]
api_key = "..."
```

`api_key_env` names an environment variable and is preferred. A variable that is not
set counts as no key, and the section says so.

## Storage

Settings carries a Storage section. It shows the storage root, the download cache
size against its quota, and how many cached files are pinned because an open
project uses them.

- **−512 MiB** / **+512 MiB** change the quota. The new quota is written to
  `~/.aria/config.toml` and enforced at once, so lowering it evicts the
  least-recently-used files down to the new figure. A value outside the accepted
  range is refused and names the range.
- **Clear cache** deletes every unpinned cached file and reports how much it removed
  and how many files it kept.
- **Move storage…** picks a new root, moves the cache into it, and records the root
  for the next launch. The destination must not already exist. A failed move leaves
  everything where it was.

Cached downloads are re-fetchable, so clearing them costs bandwidth, not work.

The **Maintenance** row reads `held` while a cloud download is in flight. Clear and
Move are refused then: either one would delete or move the partial file the download
is still writing. It returns to `available` when the download ends, whether it
succeeded or failed.

## Caches

Settings carries a Caches section listing every cache the app keeps: preview frames,
library thumbnails, timeline filmstrips and waveforms, proxies, render files,
analysis state, AI model weights, downloads, catalog data, LUTs, Lottie assets,
and templates.

Each row names where the bytes live (Memory or Disk), what is lost by clearing it
(regenerated locally, downloaded again, or your own data), when it is evicted
relative to the others, and its resolved directory.

Library thumbnails outlive the session. The resting tile picture of each pool item
is written to disk once, keyed by the file's path, length and modification time, so
reopening a project draws its pool without decoding anything. A file whose size or
header does not match is refused and the frame is decoded again, and a scrubbed or
monitor-sized frame is never written: one picture per source per tile size.

- **Measure** walks every cache directory and fills in the sizes and file counts. It
  is a button rather than automatic because the walk is the slow part; sizes stay as
  measured until you press it again.
- **Copy paths** puts the storage root and every resolved cache path on the
  clipboard, for a bug report.
- **Clear** empties one cache's directory and reports what it removed. A memory cache
  has no Clear: there is nothing on disk to remove, and the row says so.
- **Move…** moves that cache's files to a directory you pick and then records the new
  path. The files are copied or renamed first; the path is written only after they
  arrive, so a failed move leaves the old location serving.
- **Redirect…** points the cache at a directory without moving what is already there.
- **Use default** appears on a redirected cache and returns it to the storage root.

Per-cache paths are stored under `[storage.paths]` in `config.toml`. A relative path
is refused, and so is one that would put two caches in the same directory: one
cache's Clear would delete the other's files.

A hand-edited value the editor cannot use keeps its default and is listed in the
Caches section as **Config refused**, naming the key, the value, and the reason.
**Copy paths** puts the same lines in the report. Four reasons are refused this
way: a value of the wrong kind, a name outside the set a key accepts, a relative
path, and a number outside a key's range. The editor still starts.

## Plugins

Every capability of the app is a plugin: storage, settings, decoding, encoding,
compositing, effects, transitions, generators, rendering, the engine, cloud,
analysis, jobs, transcription, and the assistant. Settings carries a Plugins
section listing all of them in mount order.

A row names the plugin, its version, and the services it registered. A plugin that
did not mount says why instead — a missing GPU, an unreadable config file, a
dependency that failed first. A failed plugin does not stop the app: the rest mount
and the capability is absent, not broken.

**Disable** drops a plugin's services from the registry, and the surfaces that need
them disappear. **Enable** mounts it again. Storage, settings, and the engine have
no toggle: the editor cannot run without them.

The first two rows of the section are the limits a WASM plugin runs under, each
with **Halve** and **Double** steppers: `WASM fuel per call` (500,000,000
instructions) and `WASM memory` (64 MiB). They persist to the `[plugin_limits]`
table in `config.toml` and are clamped to 1,000,000 to 100,000,000,000
instructions and 16 to 4,096 MiB. A change remounts every mounted WASM plugin
under the new limits; one that does not mount under them is listed as failed.

## AI

Settings carries an AI section.

- **Source** picks `local` (an OpenAI-compatible server such as Ollama or LM Studio),
  `openrouter`, or `custom` for any other endpoint.
- **AI base URL**, **AI model**, **AI key**, and **AI key variable** are typed
  fields. Click one to edit it, press Enter to save, and Escape to leave the saved
  value alone. A URL must start with `http://` or `https://`. A key variable names an
  environment variable holding the key, which is preferred over storing the key
  itself; a stored key is shown as its length, never its text.
- **Protocol** picks Chat Completions or the Responses API, and **Reasoning** whether
  a Responses model is asked for a reasoning summary. Local and OpenRouter always use
  Chat Completions.
- **Autonomy** is `ask` or `full`: whether destructive assistant tools need a
  confirmation.
- **Status** is the same check the assistant runs. It names what is missing — no base
  URL, an unsupported model, a key variable that is not set — rather than reporting a
  bare "not configured". No request is made; whether the endpoint answers is found out
  when a prompt runs.
- **Limits** shows the caps one prompt is held to: edit-tool calls, host-tool calls,
  provider turns, and images. They come from the AI plugin, so with that plugin
  disabled the row says so instead of naming a cap nothing enforces.
- **Can do** counts the tools a prompt is offered: the validated edit vocabulary,
  then the app tools, split into the ones that only look and the ones that ask
  first. The counts are taken from those tables, not written here, so they cannot
  claim a capability this build does not have.
- **Active** lists which parts of a turn are switched on right now: edits, senses,
  reasoning when a summary is asked for, and whether a destructive call asks.
- **Agent files** shows where skills, rules, and slash commands live, and **Reveal
  agent files** opens it.

## Workspaces

The editor area is a tree of splits and tabbed frames. Each frame has a 24 px
tab bar at the top. Click a tab to activate it, click `×` or middle-click to
close it, drag a tab within its bar to reorder it, drag a tab onto another
frame's centre or edge (`Left`, `Right`, `Top`, `Bottom`) to dock or split it,
or drag a tab outside the main window (or choose **Undock Panel** from the frame
menu `≡`) to float it in a separate window. A floating window's **Dock** puts
its panels back in their default frames; **Close Window**, or the window
manager's close control, closes them, and **Window** opens each one again.
Double-click a tab bar or press `` ` `` to maximize the focused frame, and
repeat to restore the split tree. **Add Panel** in the frame menu adds a panel
the frame does not hold as a new tab.

A workspace is a named arrangement of frames, tabs, and pane sizes. Five ship:

| Workspace | What it arranges |
| --- | --- |
| Editing | The default, six panels. Upper row: `Project \| Effect Controls` (320 px) on the left, `Viewer` in the centre, `Effects & Presets` (300 px) on the right. Lower row: `Timeline` and `Audio Meters` (64 px). Audio Track Mixer and Assistant open from **Window** beside the Viewer; Info and Character open beside Effects & Presets. |
| Animation | `Effect Controls \| Project` on the left, `Viewer \| Audio Track Mixer \| Assistant` in the centre, `Info \| Effects & Presets \| Align` on the right with Effects & Presets in front, and `Timeline` at 50% height with the graph editor open. |
| Color | `Viewer \| Audio Track Mixer \| Assistant` at 38% width, `Scopes \| Project` at 32%, `Color \| Effect Controls \| Effects & Presets` at 30%, and `Timeline` at 35% height below. |
| Audio | `Viewer \| Project` at 22% width, `Audio Track Mixer \| Assistant` at 50%, `Audio Meters \| Effects & Presets \| Effect Controls` at 28% with Effects & Presets filtered to Audio Effects, and `Timeline` at 35% height below. In a window of 1440 x 900 or larger, the mixer strips of a new project show without scrolling. |
| Effects | `Effects & Presets \| Project` at 32% width, `Viewer \| Audio Track Mixer \| Assistant` at 36%, `Effect Controls \| Info` at 32%, and `Timeline` at 40% height below. |

Pick one from **Window**, from Settings > Workspaces, by Shift+Alt+1 through
Shift+Alt+9, or from the command palette by name. Edits made to a workspace's
frames and splits are remembered for that workspace; **Window > Reset to Saved
Layout** restores the workspace's default arrangement. Double-clicking a splitter
restores that split's saved workspace ratio.

**Window > Save Workspace** saves the current arrangement to the **Custom** slot
and opens the inline **Save workspace as** prompt; committing a name writes
`<data dir>/workspaces/<name>.json`. The Custom row appears on the Window menu
once saved, and **Forget** in Settings > Workspaces removes it again.
Settings > Workspaces lists every workspace with its measurements, and marks the
workspace last chosen, as the tool bar's workspace switcher and the Window menu
do. A fresh session marks Editing. The workspace keeps the dividers you drag and
the panels you move, so it stays marked through them and through a click on
another panel.

## Finding a setting

The box at the top of Settings filters the sections by name. It narrows as you type,
Escape clears it, and it is never written to the config file.

In the Appearance section, **System**, **Light**, and **Dark** choose the
appearance, **Accent** chooses the family used for the accent, the playhead,
the selection, and the caret (Blue, Ember, Teal, or Graphite), and **Theme**
chooses a theme file from `~/.aria/themes`, a theme a
plugin installed, or the shipped palette (see [Theme files](theme-tokens.md#theme-files)). The accent choice
persists in `[appearance]`, the theme in `ui-settings.json`, and both apply
immediately.

The Cloud section carries the backend base URL. Left empty it uses the shipped
default.

## Checking for updates

**Help > Check for updates** asks the cloud endpoint for the newest published
version. The status line reports either that the installed version is current, or
which version is newer. A failed check names the failure and still reports the
installed version.

## Stock and sound effects

The Project panel carries a **Stock** tab and an **SFX** tab.

Stock searches video, photo, and audio libraries. Click the query box, type, and
press Enter or click **Search**. Escape leaves the box. The **Video**, **Photo**,
and **Audio** chips choose what to search for; switching chips clears the results,
which answered the previous kind.

The caption above the box names the route. With a Pexels or Pixabay key set in
Settings, the query goes straight to that provider and Aria's servers never see it.
With no key, the query goes through Aria's anonymous search endpoint.

SFX lists the sound-effect catalog. Click **Load catalog** to fetch it.

A failed feed shows the reason instead of an empty list. Clicking a result
downloads it into the download cache and pins it, so **Clear cache** cannot delete
a file the project references. What happens next depends on the catalog.

## Cloud LUTs, templates, and text presets

Three more catalogs load the same way, with **Load catalog**:

- **Look tab > Cloud LUTs** applies the picked `.cube` to the selected clip. With
  nothing selected the click is refused and says so.
- **Home > Samples > Cloud templates** downloads a bundle and then asks for one
  file per slot, exactly as **File > Use Template…** does.
- **Text tab > Cloud presets** downloads a *pack*. The pack's presets then list
  below it; clicking one writes its font, size, and fill onto the selected title
  and applies its animations. A preset the pack names but this build does not carry
  is skipped, not refused.

Stock results and SFX import into the media pool instead.

## Generate

The Project panel's **Generate** tab sends a prompt to fal.ai with your own key.

Click the prompt box, type what you want, and press Enter or click **Generate**.
The **Image**, **Video**, and **Voice** chips choose what to produce; switching
chips keeps the prompt. Escape leaves the box.

Each submit becomes a job. Its row appears under the prompt and on the Jobs
board with the queue state and the download progress, and Cancel stops it. The
result downloads into the download cache, is pinned, and imports into the media
pool.

Without a `fal` key the tab says which key to add and offers no button. See
[Provider keys](#provider-keys) for where the key goes.

## Captions

Transcription runs offline. Install a model first: **Settings > Transcription**
lists each Whisper model with its size and state, and **Install** downloads it
into `models/` under the data directory. The download is a job with byte
progress and can be cancelled. A file that fails size or checksum verification
reads as `Corrupt` with the reason, and the action becomes **Re-download**.

Three timeouts govern the download, each with **Halve** and **Double** steppers
in the same section: `Connect` (15 s), `Read` (30 s), and `Total` (1800 s). They
persist to the `[transcription]` table in `config.toml`. Values are clamped to
between 1 second and 24 hours, so a stepper cannot write a value the downloader
refuses. A slow link needs a larger `Read`; a large model on a slow link needs a
larger `Total`.

With a model installed, select a clip whose media has an audio track and open
the Project panel's **Captions** tab. Set a language code such as `en` in the box, or
leave it empty to detect one. The thread chips choose how many CPU threads the
decoder uses. Press Enter or click **Transcribe**.

The run is a job: it decodes the clip's source range as 16 kHz mono, transcribes
it, and lists the segments with their timecodes. Cancel stops it and leaves the
project untouched. A run refuses before it starts, by name, when nothing is
selected, the clip is generated, the media has no audio track, the media is
offline, no model is installed, or the language is one Whisper does not know.

**Text clips** and **Markers** choose what a commit writes. **One clip per word**
splits each segment by its word timings; a segment the model gave no word timings
for stays whole. **Commit** writes the units at their own times — caption clips
onto the text lane, markers onto the timeline. Each unit is its own undo entry.

Limits: audio longer than 30 minutes is refused, the window comes from the clip's
source range rather than its speed-warped output, and no model ships with the
app.

## Analysis

The Project panel's **Analysis** tab runs three detectors on the selected clip and keeps
their results in a moments index under `analysis/` in the data directory.

- **Shots** samples the video at 2 frames per second and finds the cuts.
- **Silence** finds the spans quieter than −50 dBFS.
- **Beats** finds the beat times in the audio.

Each detector has a **Detect** button and a `×` that clears its results for that
media file. A run is a job: it decodes and detects on a background thread, reports
progress, and can be cancelled. A run refuses before it starts, by name, when
nothing is selected, the clip is generated, the media is offline, or an audio
detector is asked to read media with no audio track. 10-bit video is refused for
shot detection rather than reported as having no cuts.

A beat run also writes its grid onto the clip it ran on, as one undo entry. The
grid is stored in source ticks, so it rides trims, splits, and speed changes: a
trimmed clip offers only the beats it kept, and the grid saves and reloads with
the project. Clearing the beats analysis clears that grid too. A run that finds
nothing stores nothing, which also clears a grid left by an older run.

Results are rows with a timecode and a confidence. The kind chips filter by
detector and the confidence chips set a floor, both without re-reading the index.
**Refresh** re-reads the index, which is how a run that finished while the tab
was showing another clip appears without re-detecting anything.
**Previous** and **Next** move the playhead between the visible rows and stop at
the ends. Stored beats also draw as hairlines on the ruler and act as snap
magnets; they are not clickable.

Two rows turn a detection into an edit:

- **Split at shots** cuts the selected clip at every shot boundary inside it.
  Boundaries outside the clip are skipped.
- **Ripple-delete silence** removes every silent span inside the selected clip and
  closes the gaps. Each span is its own undo entry.

**Measure loudness** measures the clip's loudness contour and draws it in the
Effect Controls panel's Audio tab, one column per window, with the peak level and
a clipping flag. The contour is measured on demand, not per frame.

Results are keyed on the media file, identified by its path, length, and
modification time rather than a digest of its bytes: hashing a multi-gigabyte file
would cost more than re-analyzing it. A byte-identical copy under a second path is
therefore analyzed twice. Each detector also stores its own identity and version.
When a later build raises a detector's version, its old rows are labelled
`stale (analyzer vN)` and offer a re-run; they are never silently trusted or
dropped. A transcription stores its segments in the same index, so a committed
transcript is queryable by kind and confidence alongside the detectors.

**Settings > Storage** shows the analysis cache size and its path, and **Clear
analysis** deletes the whole index. Per-media clears live on the Analysis tab.

### Ducking music under voice

A duck dips the music wherever someone is talking. It is measured, not authored:
the result is ordinary volume keyframes you can drag, retime, or undo.

1. Right-click the voice lane's header and pick **Duck Source**, or run `Toggle
   track as duck source` in the palette. Only an audio lane can be the source.
2. Select the music clip or clips.
3. Open the Effect Controls panel's **Audio** tab and use **Duck under voice**.

Three rows above the button shape the dip. **Dip to** is the level the music
holds while the voice plays, as a share of that clip's own volume, so ducking a
clip already at half volume never raises it. **Attack** is how long the dip takes
to arrive, ending where the voice starts; **Release** is how long it takes to let
go after the voice stops. Each row cycles through five choices.

The line above the button names the lane being listened to. When the run cannot
happen, the reason is written under the button rather than the button being dead:
nothing selected, no lane flagged, no audio on the flagged lane, or a selection
that is only the voice itself.

The measurement is a job: every audio clip on the voice lane is decoded, its
silence is detected, and the voice is what is left. Speech that is closer together
than attack plus release becomes one dip, because there is no room for the music
to come back up between two sentences. Voice that starts before a music clip does
leaves that clip open already ducked, and voice still running at its tail leaves
it ducked to the last frame.

The whole run is one undo entry however many clips and keyframes it wrote. Running
it again replaces the envelope rather than ducking the dip: the level the music
returns to is the top of whatever is on the clip now.

## Media pool

The Media tab has one row above the list and one below it. Above: the
**Search** box, **Audio only** (the icon beside it, which lists the pool's audio
sources), and **Import**. Below: how many of the pool the filter leaves, then the
views, the order, **New bin**, and **New multicam**. Hover an icon for its
name.

The search box matches an item's name, its path, its kind, its frame size, and
its label colour, so a search for `1920`, `wav`, or `teal` finds media whose name
includes none of them. The order button reads the current order; click it for
**Name**, **Duration**, **Resolution**, or **Date added**. Duration and
resolution sort largest first, and date added sorts newest first. An item from a
project saved before import times were recorded has none and sorts last. Neither
the filter nor the order is saved with the project.

The view buttons set the pool layout; in a narrow Project frame one button cycles the four. **Tiles** puts each
item on its own row with its picture above its facts. **List** drops the picture
and reads each item as one line, name on the left and facts on the right, which
is the view to use on a pool of hundreds: no picture means no decode. **Icons**
wraps the pictures into a 96px grid with the name underneath and no facts.
**Freeform** places the pictures where you drag them. The
view is saved with your settings, not with the project, so it is the same in the
next project you open.

Click an item to select it; double-click it to open it in a Source tab (see
[Previewing a source](#previewing-a-source)). Each row states the
frame size, the length, the video codec, and either the audio codec or that the
file is silent, so a silent video is visible before it is cut in. The block under
the list reports what the pool has for the selected item: container, frame
size, frame rate, duration, video codec, audio, when it was imported (UTC),
whether it is still linked, and its path.

Ctrl+click (Cmd+click on macOS) adds an item to the selection, or drops it if it
is already in. Shift+click picks every item drawn between the last one you
clicked outright and the one you shift-click, in the order the list is showing.
No click places a clip. Selected items are bordered, and the properties block
describes the last one clicked.

The order of the selection is the order you clicked, which is the angle order of
a multicam group built from it. Opening another folder drops the selection.

The container is the file extension. A codec name is what the platform's demuxer
reported at import: on Linux, GStreamer's description of the track's caps. Where
a backend reports no codec, the row reads `unknown` rather than guessing from the
extension, which would repeat the container. Stills have no codec track and
read `unknown`.

Drag an item onto the timeline to place it where you drop it:

- Dropped on a lane, it lands on that lane at the tick under the pointer. The
  magnet applies, so a drop near a cut, a marker, or the playhead takes that
  position. A lane that cannot hold the media (audio on a video lane) passes
  it to a lane of the right kind at the same tick.
- Dropped on the thin strip above the top lane or below the bottom one, it
  creates a lane of its own kind and lands there. This is the only drop that
  adds a lane.

With Program focused in the Viewer, `,` inserts the selected items at the
playhead, end to end in the order you clicked them, and `.` overwrites with them.
Both go through the patched lanes, and the placement is one undo step. An item
whose file is missing is skipped.

Right-click a pool item for **Remove from project** and for the label swatches.
An item is removable only
while no clip references it; a removal that a clip still holds is refused with
the reason. Removal is undoable, and the undo reattaches any clips removed
alongside it.

### Bins

The pool holds folders. The folders inside the open one are listed above its
items. While a folder is open, a breadcrumb above them shows the open path
(`Pool / Footage / Day 1`) with **Rename** and **Delete bin** at its right.

- **New bin**, in the row below the list, makes a folder inside the open one,
  opens it, and starts its name in a text field. Type the name and press Enter;
  Esc leaves the field without renaming. An empty name is refused.
- Click a folder to open it. Double-click it to rename it. Click a crumb to go
  back out; `Pool` is the root.
- **Rename** and **Delete bin** act on the open folder. Deleting one moves its
  media and its folders up to its parent, so nothing leaves the pool. Both are
  undoable, and undoing a deletion puts the folder back with what it held.
- Drag a pool item onto a folder row or a crumb to file it there. Drag a folder
  onto another folder or a crumb to move it. A folder cannot be dropped into
  itself or into one of its own folders.

The list shows what is directly in the open folder. Typing in the search box
searches the open folder and everything under it, so a file one level down is
still findable, and the count below the list describes that same scope. Bins are
saved with the project; which folder is open is not.

### Previewing a source

Each pool row holds a picture of its source. Move the pointer across that
picture to scrub the file: the left edge is its first frame and the right edge
its last. Nothing is placed and nothing is edited; a still shows its one frame.
A row shows a glyph until its first frame is decoded, and an audio-only file has
no picture.

Double-click a pool item, or press `Space` with the pool focused, to open it in a
Source tab of the Viewer. The tab row above the picture holds **Program** first
and one **Source:** tab per open item; click a tab to show it, and close a Source
tab with its close button, a middle click, or Ctrl+W with the Viewer focused.
Opening an item that already has a tab shows that tab. A tab keeps its frame and
its marks while it stays open. `Shift+2` with the Viewer focused switches between
Program and the Source tab shown last. `Space` with the pool focused shows Program
again when the item's tab is in front, **Sequence** in the corner shows Program,
and `Space` over any other panel, including Effects and the Project panel tabs that do
not list the pool, or over a pool with nothing selected, shows Program and plays
the sequence. Opening a source stops playback. Safe areas and the transform
handles belong to the sequence and are not drawn over a source. An audio-only
item has no picture and opens no tab. In a narrow Viewer a long name ends in an
ellipsis on its tab and on the name in the corner.

**Split Viewer**, under the wrench at the right of the tab row, shows the Source
tab shown last on the left and Program on the right. The keys reach whichever tab is focused.

Double-click a media clip on the timeline to open its take in a Source tab linked
to that clip. The tab opens at the frame the sequence shows under the playhead,
or at the clip's first frame when the playhead is elsewhere, and its In and Out
are the clip's. Marking In or Out in that tab trims the clip: a new In trims the
head, a new Out the tail, and every frame that stays keeps its place on the
timeline. Clearing a mark extends the clip to that end of the take. A reversed
clip trims the opposite end, a retimed clip trims at its speed, and its linked
sound trims with it. A trim that would overlap a neighbour, start before the
sequence, or put Out before In is refused and changes nothing. Clips with a speed
ramp and freeze frames trim on the timeline only. Undo puts the clip and the
tab's marks back together. Deleting the clip leaves a plain Source tab with its
last In and Out. The same take opened from the pool, or from another clip, opens
in its own tab. `F` (Match Frame) opens the pool's tab for the take under the
playhead instead, with the clip's In and Out as plain marks.

The mini ruler under the Viewer's control row spans the tab in front: the whole
sequence on Program, with its In and Out range and its markers, and the whole
take on a Source tab, with the tab's marks. Press it to put that tab on the frame
under the pointer, and drag to scrub; the drag continues when the pointer leaves
the ruler, and Esc puts the frame back. On a Source tab the pool row's picture
follows the ruler. `I` marks the source in point and
`O` the out point while a source is loaded, so those keys mark the take you are
looking at rather than the sequence; `Ctrl+Shift+I` and `Ctrl+Shift+O` clear them, and
**Clear** clears both. An out point is exclusive, so the frame you marked is
included. The ruler shades the marked range, and the line under the picture
counts the frames kept.

A focused Source tab has its own transport. With the Viewer focused, `Space`
plays or stops the take, `Shift+Space` plays its In to Out, `J` and `L` shuttle
it at 1x, 2x, 4x, and 8x, `K` stops it, and `K` held with `J` or `L` steps one
frame. `Left` and `Right` step a frame, `Shift+Left` and `Shift+Right` five,
`Home` and `End` go to either end of the take, and `Shift+I` and `Shift+O` go to
its In and Out. The play button under the Viewer plays the tab in front of it.
Source playback has no sound. Starting the sequence stops the source, starting
the source stops the sequence, and showing another tab stops the source. Over any
other panel the same keys move the sequence.

**Insert** (`,`) puts the marked range into the sequence at the playhead on the
patched lanes and pushes the rest of every sync-locked lane right. **Overwrite**
(`.`) puts it there without moving anything: material inside the span is
dropped, material reaching into the span is trimmed to the edge it crosses, and a
clip that spans the whole window survives on both sides of it. Mark the sequence
at both ends as well as the source and either button becomes a four-point edit:
the sequence's span sets the length and the take is retimed to fill it. Drag
the frame onto the timeline to drop the marked range where you let go, covering
what is there; hold Ctrl to insert instead. Each is one undo entry. The tab stays
open, so takes can be cut in one after another.

### Multicam

A multicam group is several cameras of the same take, cut between as one clip.

Select the camera files in the pool and press **New multicam**, beside **New
bin**. The group takes one angle per selected item, in the order you clicked
them, and appears as a row below the folder strip. With nothing selected the
button groups the whole pool, which is what an import of a card's worth of
angles wants. An empty pool reports that and builds nothing.

The row names the group, counts its angles, and holds one row per camera. The
lit chip is the camera live at the playhead, read from the group's cut list as
you scrub. Clicking a chip cuts to that camera at the playhead.

**Place** puts the group on a video lane at the playhead, following the same lane
target a media drop follows. Its length is the group's own: from the group's
first frame to the last frame any of its cameras can still answer for, so a
camera that runs longer than the first one does not get cut short.

What the frame shows is the camera the cut list makes live at that point,
decoded from that camera's own file and fitted to the frame at its own size, so
a group of mixed formats is not letterboxed to whichever camera came first.

Beside each chip is that camera's sound, drawn across the whole group's span
rather than across its own file. Two strips read one above the other are two
pictures of the same stretch of time: cameras that are in sync show the same
transients at the same place, and one that is out shows them displaced by
however far out it is.

The chevrons either side of the offset move a camera on the group's clock by one
frame, and the number between them is where that camera's first frame sits: `0f`
is the start of the group, `+24f` a camera that started a second late at 24 fps.
The offset is what the frame is read through, so moving it moves what the group
shows at every tick that camera is live.

The waveform button measures it instead. The peaks already decoded for the strips
are correlated against the first camera's, and every other camera is placed
where its sound matches. The first camera is the clock and does not
move, so syncing a group twice gives the same answer, and the whole group is one
undo entry however many cameras moved.

A camera it cannot place is left where it is and named: sound still being read,
a file with no sound or none that varies, sound that does not match the first
camera's well enough to be the same take, or sound that repeats. A repeating
sound matches at every repeat, so the best match is whichever repeat came first
rather than the right one. An offset applied from a guess would move a camera
that was already in sync, and nothing on screen would tell you it had.

## Markers

`M` adds a marker at the playhead: on the selected clip in clip-source time when
a selected clip spans the playhead, or on the sequence ruler otherwise. Pressing
`M` when a marker is already at the playhead, or choosing **Markers > Edit
Marker…**, opens the Edit Marker dialog to set its name, comment, colour
swatch, duration timecode, and kind (`Comment`, `Chapter`, or `Segmentation`) as
one undo step. Negative durations and durations that run past the sequence end
or the clip's source range are rejected with a reason. `Shift+M` removes the
marker at the playhead, and `Ctrl+Shift+M` / `Ctrl+Shift+Alt+M` step to the
previous marker or clear all sequence markers.

The **Markers** panel (Window > Markers) lists sequence and clip markers in time order with
colour filter chips (`All` plus the eight palette colours). Each row shows the
marker's colour, timecode, duration when positive, clip badge for clip markers,
kind badge when not `Comment`, name, and note. Clicking a row moves the playhead
to the marker. Clicking the colour swatch cycles the eight-colour palette,
**Edit…** opens the Edit Marker dialog, **Rename** edits the name inline,
**Note** edits the comment inline (an empty note clears the comment), and
**Remove** deletes the marker. Every edit is one undo step. A marker with a
positive duration draws as a range bar on the timeline ruler or on its clip
card, and snapping targets both ends of a range marker.

## Label colours

A label colour groups clips that belong together. The sixteen labels are
Violet, Iris, Caribbean, Lavender, Cerulean, Forest, Rose, Mango, Purple, Blue,
Teal, Magenta, Tan, Green, Brown, and Yellow. Right-click a clip: the menu
holds their swatches and an `×`. **Edit > Label** lists the same sixteen and
**None**. Clicking a swatch labels every selected clip, `×` clears the label,
and each is one undo entry per clip.

A clip bar is filled with its label colour. A clip with no label takes its
kind's label: Teal for video, Forest for audio, Mango for generators, titles
and shapes, Tan for nested sequences, and Yellow for clips on adjustment,
effect, and filter tracks. A selected clip is drawn in a lighter fill of its
label with a 1 px white border.

The **Select** chips under the swatches list every label in use with its clip
count; a clip with no label counts under its kind's label. Clicking a chip
selects every clip drawn in that colour, in timeline order, and the status line
reports how many it found.

Labels are saved by name. A project saved when labels used the eight marker
colours opens with `pink` as Magenta, `red` as Rose, and `orange` as Mango. A
project saved with no labels is byte-for-byte what an older version of the app
wrote.

A media-pool row takes a label the same way: right-click the row and pick a
swatch. The row draws a dot in its colour, and the pool filter matches the
colour name, so typing `teal` in the filter box lists the teal group.

## Template bundles

A template is a finished project whose replaceable clips are marked as slots.
See [Templates](#templates) for the marks.

- **File > Save as Template…** writes a `.ariat` bundle. The project itself is
  untouched: no rename, no dirty flag, no change to the project path. The status
  line reports how many slots the bundle carries.
- **File > Use Template…**, or the Use Template tile on Home, opens a bundle,
  asks for one file per slot in fill order — using the slot's hint as the prompt
  — and replaces the session with the filled result. Cancelling the picker stops
  asking and applies what was chosen, so a bundle can be opened unfilled with its
  own sample media in place.

## Export and save

- Ctrl+S saves to `~/.aria/untitled.aria` the first time, or to the last
  project path.
- Ctrl+Shift+S saves to a new path.
- Ctrl+O opens a project. Ctrl+Shift+W returns to Home.
- Ctrl+M opens Export. Choose a destination or accept the default `.mp4`.
- Export writes what this platform's encoder supports. The sheet's **Format**
  and **Encoder** rows name the container, the codec of the file about to be
  written, and the backend (GStreamer on Linux, VideoToolbox on macOS, Media
  Foundation on Windows). A build with no native encoder says so and refuses
  rather than starting a render it cannot finish.
- An export that would write no frames is refused before any file is created.
  A sequence with nothing on it asks for a clip; a marked range that covers no
  frame asks for the In and Out points to be moved apart. Neither writes a
  zero-frame file.
- Export runs in the background and opens the Jobs board. Editing and playback
  continue while it renders.

The Export sheet carries seven choices. Each row steps to its next value when
clicked, and none of them is an edit or is saved with the project:

- **Scale** renders at 100, 75, 50, or 25 per cent of the canvas. The row shows
  the resulting pixel size.
- **Frame rate** is the project's rate or a fixed 24, 25, 30, 50, or 60.
- **Codec** is H.264 or H.265. The row offers only the codecs this machine can
  write: on Linux that is probed from the installed GStreamer encoders, and on
  macOS, Windows, and Android it is H.264 alone. With one codec available the
  row refuses and names it.
- **Bitrate** is the encoder's own rate control, or a target of 4, 8, 16, or 40
  Mb/s. Only GStreamer takes a target; the other backends refuse the row and say
  so. A codec or bitrate the encoder cannot honour is refused by name before any
  file is created, never swapped for something else.
- **Keyframes** is the codec's own spacing or one every 15, 30, or 60 frames.
- **Audio** includes the mix or writes a video-only file.
- **Range** is the whole timeline or the marked range. Marking is required
  first: with no In and Out set, the row refuses and says so.

The **Destination** row shows the file the export writes. A path wider than the
sheet loses its start to an ellipsis, and the file name stays in view.

The status line names the size, whether audio is included, and the span when the
export starts.

The sheet governs every export, whether it runs as a job or blocks: File > Export
sends the same choices, and `aria export` on the command line takes them as
flags. One piece of code resolves a request against the project and the machine's
encoder, so the same choices write the same file from any of the three.

Settings live in `~/.aria/config.toml`. UI chrome widths persist in
`~/.aria/ui-settings.json`. Recents persist in `~/.aria/recents.json`.

Each table in `config.toml` carries a schema version, recorded in one table:

```toml
[schema]
ai = 2
appearance = 1
```

A missing key keeps its default, so a file an older build wrote still opens. A
table recording an older version is migrated to the current shape, and the
current version is recorded on the next save. A table recording a version this
build does not read is refused: the load fails naming the table and both
versions, the settings fall back to defaults, and a save leaves the file in
place instead of rewriting a meaning it cannot read.

Two tables have changed meaning so far. `[ai]` version 1 had no `source` key and
read a base URL with a model as the freeform endpoint; version 2 reads an absent
`source` as the local server. `[cloud]` version 1 kept the API host in
`[account].base_url`; version 2 keeps it in `[cloud].base_url`.

## Unsaved work

New, Open, Close, Home, and a project dropped on Home all replace what is
loaded. With unsaved edits in the project, each one asks first:

- **Save** writes the project, then does what you asked. A save that fails
  leaves the prompt up and the edits in place.
- **Discard** does what you asked and loses the edits.
- **Stay** cancels. Nothing is saved and nothing is replaced.

While a project has unsaved edits, a copy is written to `~/.aria/recovery.aria`
every twenty seconds. Saving, opening, or starting a project deletes it, so the
copy only ever exists when the file on disk is behind the session.

If the app stops without saving — a crash, a kill, a power cut — Home shows the
copy on the next launch:

- **Restore** loads it and aims Save back at the project it came from. The
  session reads unsaved, because the project file still holds the older content.
- **Discard** deletes the copy.

## Command palette

Ctrl+Shift+P opens the palette. Type to filter, Up and Down move the highlight,
Enter runs it, and Escape closes it. The highlight wraps at both ends and returns
to the first row whenever the query changes.

The palette offers every registered command and every row the menus would draw,
including rows contributed by a plugin. A row that came from a menu names that
menu on its right. A row a menu hides because it cannot run now is not offered by
name either.

Filtering ranks in four tiers: labels starting with what you typed, then labels
with a word starting with it, then labels containing it, then labels whose
letters appear in order, so `fs` finds **Fullscreen preview**.

## Control names

Resting the pointer on an icon-only control names it, with its shortcut when it
has one: the tools, the transport, the snapping magnet, the timecode field, the
preview zoom controls, **Add a track**, and the effect row's reorder and remove
buttons. The shortcut shown is read from the key bindings, so a re-chorded key
reads correctly rather than repeating a stale letter.

## History

**Window > History**, `History` in the command palette, or Ctrl+Shift+H brings
the History panel forward. It lists every step this session took, oldest first,
starting with **Project opened**.

Each row is one undo step, named after the command that made it: `Split clip`,
`Move clip`, `Add effect`. A gesture that ran several commands is one row named
after its first, with the count of the rest: `Move clip +2` undoes all three.

The row the project is at is marked **current**. Clicking any other row moves
the project to the state after that step, in either direction; it is undo and
redo without counting keystrokes. A step that has been undone stays in the list,
dimmed, because redo can still reach it. Making a new edit while dimmed rows are
present replaces them, exactly as Ctrl+Z then a new edit always has.

History is per session. Saving does not clear it; opening or loading a project
does.

## Jobs

**Window > Jobs** docks the Jobs panel, which lists background work: exports,
cloud downloads, transcription, and analysis. `Jobs` in the command palette
opens the same list as a dialog. While work runs, the centre of the status bar
shows the first running job, its progress, and how many others are running;
click it to bring the Jobs panel forward.

Each row shows the label, the state, a detail line such as `frame 240/1800`,
and the elapsed time. A running job that reports a fraction also shows the time
left at its rate so far: `Running · 12.4s · 38s left`. A job that reports a fraction draws a filled bar; one
that cannot measure its total draws a moving bar and reports what it has done
so far.

**Stop** appears while the job can still be stopped, on the board and on the
panel row that started the work, and beside a render in the export sheet. The
same job shows the same control everywhere. It disappears once the job has
reached its publication boundary, the point where it starts writing its result:
an export's finished file, a transcript, or a set of detected moments.
Everything before that point is discarded on stop; a stop that arrives after it
is refused, so a job never both writes its result and reads as cancelled.
Stopping an export discards the render; the destination file is not written.

A request that is not a job has no Stop and does not need one: the AI
connection probe and the model list are bounded at twenty seconds, so a host
that accepts the connection and then says nothing is reported as unreachable
rather than leaving Settings waiting.

A finished job keeps its row and its outputs. **Reveal** on an output path opens
the containing folder in the file manager.

## Assistant

Press A to open the assistant panel. Click the prompt box, type an instruction,
and press Enter or click **Send**. The label above the box names the provider and
model the prompt goes to.

A prompt is refused before it costs anything when the box is empty, when a turn
is still running, or when no AI provider is configured. Only the last of those
opens Settings, and it says what is missing.

While a turn runs it appears on the Jobs board and the panel's **Stop** ends it.
Stop is read between deltas, so it lands mid-answer rather than at the end of
one. A stopped turn is undone: the edits it had already applied are rolled back,
the project is left where the prompt found it, and no undo entry is added. A turn
that stops because the assistant hit its edit cap is different — those edits are
kept, and Ctrl+Z undoes them as one step.

The transcript shows the answer as it streams, in the pieces the provider sends,
each edit the assistant applied, and its reasoning summary when the provider
sends one. An edit lands in the project as it is made, so the timeline updates
during the answer rather than after it. The text already streamed stays in the
transcript when a turn is stopped; the edits do not.

Up and Down in the prompt box walk back through prompts already sent. **Clear**
forgets the conversation; the project is untouched.

A long conversation scrolls, and follows the answer while it streams. **Copy** in
the header copies the whole conversation with speaker names; the same button on a
row copies that row.

### Plan only

Turn on **Plan only** and the turn reports the edits it would make without
changing the project. The plan card lists each step in order.

Each step is validated against the plan so far, not against the project as it
stands, so a step that acts on what an earlier step made is planned rather than
refused. The turn applies its steps to work that out and takes them all back
before it returns; a render is validated and not started.

- **Apply** replays that plan as one undo entry. Each step is checked again
  against the project as it now stands; if one no longer fits, nothing is
  applied and the message names the step that failed.
- **Discard** drops the plan.

Clearing the conversation drops a pending plan with it.

### Skills

The panel lists the skills installed for the agent, and **Load catalog** below
the list fetches the cloud skill catalog. Clicking an entry downloads it and
installs it as `~/.aria/agent/skills/<id>/SKILL.md`, which is where the prompt
builder reads skills from. A download that is not a valid skill is refused and
installs nothing. Skills are read at send time, so a skill installed now is
available to the next prompt, and editing a rule file takes effect on the next
prompt without a restart.

The ✓ beside each skill switches it off for this conversation. It stays
installed; the next prompt does not carry it. Clearing the conversation
switches them all back on.

Rules are read from `~/.aria/agent/` at send time. The rule text is capped, and a
prompt that had to truncate it says so in the transcript instead of quietly
sending less than the files hold.

### What it can do besides edit

Timeline edits go through the validated vocabulary and land in the undo history.
Alongside them the assistant has app tools:

- **Look at a frame.** It renders the timeline at a time and looks at the picture,
  so it can check what a shot shows before changing it. The row in the
  transcript names the moment it looked at.
- **Read a media file.** Size, frame rate, length, and codecs come from the file
  itself, not from its name.
- **Move the playhead** and **save the project**.
- **Empty a cache.** Destructive, so it goes through **Autonomy**.

With **Autonomy** set to `ask`, a destructive call stops and the transcript shows
what it would do with **Allow** and **Deny**. The turn waits for the answer: it
continues on Allow, and on Deny the assistant is told it was refused and carries
on. **Stop** while a question is open denies it. With `full` there is no question.

### What it is told about the project

Every prompt carries a fresh description of the project: the tracks, the clips
with their ids and times, the media pool, the markers, and where the playhead is.
It is rebuilt for each turn, so the assistant reads the timeline as it stands
rather than as it was when the conversation started.

Up to 300 clips the description holds all of them. Past that it holds the 300
clips nearest the playhead and states its own bounds, how many clips it carries,
and how many clips and markers fall outside them. The assistant can ask for
another stretch of the timeline by name, so a note about a clip an hour in is
answered by reading that stretch rather than by guessing. Nothing is dropped in
silence: a windowed description says what it left out.

### What is remembered

Each project keeps its own conversation, stored beside the app's other session
data rather than in the project file. Reopen the project and the conversation is
back, so the next prompt continues from it. Opening a different project, or
starting a new one, starts a new conversation.

- **Clear** deletes the stored copy at once.
- A project that has never been saved keeps its conversation in memory only; the
  first save starts storing it.
- A stored conversation written by another version of Aria is ignored rather than
  half-read.
- Screenshots are not stored. The assistant already drops them from history
  before each request, so keeping them would store bytes no prompt can use.

See [keyboard-shortcuts.md](keyboard-shortcuts.md) for the full list.
