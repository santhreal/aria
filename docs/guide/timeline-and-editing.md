# The timeline and editing

## Edit

- Space plays and pauses.
- Ctrl+K splits the selected clip at the playhead; Ctrl+Shift+K splits every
  track.
- Drag a clip to move it. Drag a clip edge to trim. A move or trim follows the
  pointer when it leaves the lanes and lands where the button comes up. Nearby
  clips magnet at 8 px. The magnet also takes the playhead, the sequence start
  and end, the In and Out points, markers, and detected beats. While a drag is
  in flight the guide line states what it locked onto. Hold Alt to suspend the
  magnet for that one drag; the Timeline header's magnet stays on.
- Drag on empty timeline space to rubber-band: every unlocked clip the band
  touches is selected, in either direction, across every lane between the two
  corners. A press with no drag clears the selection. Esc abandons the band and
  leaves the selection as it was.
- Arrow keys nudge one frame. Shift+Arrow nudges ten.
- Delete or Backspace ripple-deletes the selection.
- Ctrl+Z / Ctrl+Y undo and redo.
- Double-click a media clip to open its take in a Source tab linked to the
  clip (see [Previewing a source](media-pool-and-bins.md#previewing-a-source)). Double-click a nested
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
- `\` fits the timeline to the sequence, Shift+F to the selection. + / - zoom
  around the playhead, and bring a playhead that is off screen to the middle of
  the view. Zooming out stops at the whole sequence.
- Right-click a clip, track, or the preview for a context menu. A row that
  switches a state draws a check mark in its lead column while the state
  holds, as a menu bar row does.
- **Clip > Enable** enables or disables the selected clips, and draws a check
  mark while every selected clip is enabled; the clip context menu's
  **Enable** reads the same. A disabled clip draws at 40 per cent opacity on
  its lane and produces no picture or sound in preview or export.
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


## Copy, paste, and timecode entry

Ctrl+C copies the selected clip, and there are four pastes:

- **Ctrl+V** drops the copy at the playhead, on the copied clip's own lane. What
  is already there is not moved, so a paste over a clip is refused.
- **Ctrl+Shift+V** (Paste Insert) first slides that lane right by the copy's
  length from the playhead on, then fills the hole. Nothing is overwritten.
- **Ctrl+Alt+V** (Paste Attributes) writes the copied clip's transform, filter,
  and whole effect chain onto the selected clip. Its position, length, media, and
  audio levels are unchanged. The target's existing effects are replaced, not
  added to, so pasting from a clip with no effects clears them.
- **Paste to Fit**, on the Edit menu, drops the copy into the marked range and
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
time display (see [Canvas](settings-and-storage.md#canvas)): timecode, a frame count, or feet and
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
  clip to the properties whose labels contain the text, ignoring case, and
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
to move the playhead; it stays under the pointer, follows the pointer when it
leaves the ruler, and stops at the end of the sequence when the pointer passes
it. A drag in the ruler leaves the view where it is.

Any other move of the playhead, from a key, a Viewer button, a marker, a typed
timecode, or a scrub strip, draws the line at the new position at once, and the
Viewer replaces its frame with the new one, with no fade, when the new one is
decoded.
A playhead that lands outside the view moves the view a page to show it, with
the playhead near the left edge. A playhead inside the view leaves the view
where it is.

Under the ticks, the work area bar spans `In` to `Out`. It draws in the accent
color when either point is set and faint when the work area is the whole
sequence. Drag the bar to move both points, drag either end to move that point,
and double-click the bar to set the work area to the whole sequence. An end
stops one frame short of the other, and the bar stops at the sequence ends. Esc
during the drag puts both points back. The render bar runs along the bottom of
the ruler (see [Render files](viewer.md#render-files)).

The zoom bar along the bottom of the timeline maps the whole sequence, and past
its end the view when the view reaches further. Drag the window to scroll; the
point you pressed stays under the pointer. Drag either handle to zoom with the
other end held in place; drag the handles to the bar's two ends to show the
whole sequence. Click outside the window to center the view there and keep
dragging from there. A drag continues when the pointer leaves the bar, and Esc
during the drag puts the zoom and the scroll back. At its left, **+** adds a
video track and the zoom readout fits the sequence on a click.


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
other picture-neutral edits retain them. Each move of a value scrub, a clip
line drag, or a color wheel drag replaces the edit of the move before it, and
invalidates only the cells inside the spans of both edits; Escape invalidates
only the cells inside the spans the drag changed. Undo, redo, and edits from
the assistant or a plugin invalidate every cell. Work started before an edit
is rejected when it finishes. A cell that is not ready leaves its space empty
rather than borrowing a neighboring frame.

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
- **Smooth Scroll** scrolls the view with the playhead held at its center.

A scroll by hand during playback stops the follow until the playhead leaves the
view.


## Label colors

A label color groups clips that belong together. The sixteen labels are
Violet, Iris, Caribbean, Lavender, Cerulean, Forest, Rose, Mango, Purple, Blue,
Teal, Magenta, Tan, Green, Brown, and Yellow. Right-click a clip: the menu
holds their swatches and an `×`. **Edit > Label** lists the same sixteen and
**None**. Clicking a swatch labels every selected clip, `×` clears the label,
and each is one undo entry per clip.

A clip bar is filled with its label color. A clip with no label takes its
kind's label: Teal for video, Forest for audio, Mango for generators, titles
and shapes, Tan for nested sequences, and Yellow for clips on adjustment,
effect, and filter tracks. A selected clip is drawn in a lighter fill of its
label with a 1 px white border.

The **Select** chips under the swatches list every label in use with its clip
count; a clip with no label counts under its kind's label. Clicking a chip
selects every clip drawn in that color, in timeline order, and the status line
reports how many it found.

Labels are saved by name. A project saved when labels used the eight marker
colors opens with `pink` as Magenta, `red` as Rose, and `orange` as Mango. A
project saved with no labels is byte-for-byte what an older version of the app
wrote.

A media-pool row takes a label the same way: right-click the row and pick a
swatch. The row draws a dot in its color, and the pool filter matches the
color name, so typing `teal` in the filter box lists the teal group.


## Command palette

Ctrl+Shift+P opens the palette. Type to filter, Up and Down move the highlight,
Enter runs it, and Escape closes it. The highlight wraps at both ends and returns
to the first row whenever the query changes.

The palette offers every registered command and every row the menus would draw,
including rows contributed by a plugin. A row that came from a menu states that
menu on its right. A row a menu hides because it cannot run now is not offered by
name either.

Filtering ranks in four tiers: labels starting with what you typed, then labels
with a word starting with it, then labels containing it, then labels whose
letters appear in order, so `fs` finds **Fullscreen Preview** (`pal-preview-fullscreen`).

## Control labels

Resting the pointer on an icon-only control shows its label, with its shortcut when it
has one: the tools, the transport, the snapping magnet, the timecode field, the
preview zoom controls, **Add a track**, and the effect row's reorder and remove
buttons. The shortcut shown is read from the key bindings, so a re-chorded key
reads correctly rather than repeating a stale letter.


## Example

Cut a short clip from a longer recording with In/Out and ripple delete:

1. Select the recording on `V1`, move the playhead to the frame where the unwanted section starts, and press <kbd>Ctrl+K</kbd> (`key-split`) or choose **Sequence > Add Edit** (or run **Split at Playhead** (`pal-split`) in the Command Palette). The recording splits into two adjacent clips at the playhead.
2. Move the playhead to the start of the keep range and press <kbd>I</kbd> (`key-mark-in`) or click [`program-mark-in`](../reference/controls.md#viewer--source-monitor) (or choose **Markers > Mark In**). The timeline ruler work area bar anchors its In point at the playhead.
3. Move the playhead to the end of the keep range and press <kbd>O</kbd> (`key-mark-out`) or click [`program-mark-out`](../reference/controls.md#viewer--source-monitor) (or choose **Markers > Mark Out**). The work area bar highlights the span between In and Out on the timeline ruler.
4. Press <kbd>Ctrl+K</kbd> (`key-split`) at the Out point to cut the tail off the keep clip, then select the unwanted tail clip on `V1`. The tail clip highlights with a white selection border.
5. Press <kbd>Shift+Backspace</kbd> (`key-ripple-delete`) or choose **Edit > Ripple Delete** (or run **Ripple Delete Selected** (`pal-ripple-delete`)). Aria removes the unwanted clip and shifts all later clips on sync-locked lanes left to close the gap, leaving the short cut clip on `V1`.
6. Press <kbd>\</kbd> (`key-fit`) or click [`fit-timeline`](../reference/controls.md#timeline) (or choose **View > Fit**) to fit the edited sequence across the timeline viewport. The timeline zooms to show the shortened sequence end to end.

## Reference

See [Controls](../reference/controls.md#timeline), [Menus](../reference/menus.md#edit-menu), [Commands](../reference/commands.md), and [Keyboard shortcuts](../keyboard-shortcuts.md) for all editing actions.
