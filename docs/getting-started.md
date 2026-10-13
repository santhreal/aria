# Getting started

Aria opens on Home. Start a new project, open a `.aria` file, or drop
media onto the window.

## New project

1. Click **New Project**, or press Ctrl+N (Cmd+N on macOS).
2. The editor opens with an empty timeline.
3. Click **Import** in the Media sidebar, or drag files onto the window.
4. Click a media tile to place it on the timeline at the playhead.
5. Titles, Shapes, and Stickers tabs place a generator at the playhead.

Supported import types: `mp4`, `mov`, `mkv`, `webm`, `wav`, `mp3`, `png`,
`jpg`, `jpeg`, `webp`. Linux video and audio decode through the host
GStreamer plugins. Stills work without them.

## Dropped files

What a dropped file becomes follows its type, on Home and in the editor alike:

| File | Result |
| --- | --- |
| `.aria` | Opens as a project, replacing the session |
| Media (see the list above) | Imported into the media pool |
| `.cube` | Loaded as the selected clip's LUT |
| `SKILL.md` | Installed as an assistant skill |
| Anything else | Imported as media, which reports why it cannot be |

A project file dropped alongside other files wins: opening one replaces the
session, so the rest would be discarded.

## Missing media

If a media file moves after save, its pool row is marked missing. Click
**Relink** on the row to point it at the file, or **Relink All** to pick one
folder and repair every missing row from it.

Relink All searches the folder and up to four levels below it, matching by file
name, then checks each same-named file against what the project recorded for
that row: frame size, duration, and whether it has audio. A row is repaired
only when exactly one file matches. Anything else leaves the row missing and
states why on it:

| Row says | Meaning |
| --- | --- |
| not in that folder | No file there carries that name |
| N files there match | Several files match everything recorded; relink that row by hand |
| the file there has a different frame size / duration / audio | The name matched, the file is another source |
| could not read the file there | The file exists and did not open |

## Sidebar tabs

The sidebar filters what you browse:

- **Media** — every imported file. **Audio** — the audio files only.
- **Titles**, **Shapes**, **Stickers** — click to place a generated clip at the
  playhead.
- **Animations**, **Masks**, **Transitions** — click to apply the entry to the
  selected clip. A transition lands at the selected clip's right cut. With no
  clip selected, or on a clip that refuses the entry, the click reports why.
- **Stock**, **SFX** — the cloud libraries. See
  [Stock and sound effects](#stock-and-sound-effects).
- **Generate** — a prompt to fal.ai. See [Generate](#generate).
- **Captions** — offline transcription of the selected clip. See
  [Captions](#captions).
- **Analysis** — shot, silence, and beat detection on the selected clip. See
  [Analysis](#analysis).
- **Markers** — every marker on the ruler. See [Markers](#markers).

Drag instead of clicking to choose the target. Drag a sticker onto a lane to place
it at that lane and time. Drag an animation, mask, or transition onto a clip to
apply it there; the clip becomes selected, so the inspector shows what landed. A
sticker dropped on a lane that cannot hold it keeps the time and moves to the
sticker lane.

Hover an **Animations** entry to see it on the selected clip before you choose it.
The preview is not an edit: it makes no undo step, and moving the pointer off the
row restores the clip. With no clip selected there is nothing to preview.

## Text

Select a title. The Text tab shows its content and **Edit text…**, which renames
the text in place. Below it, the Effect chips are the text presets: neon, shadow,
outline, glow, retro, and chrome. Click a chip to apply it, click the active chip
to clear it. A preset writes stroke, shadow, and background onto the style, so
clearing the chip stops marking the preset but leaves what it wrote; edit those
rows to change them.

Retyping a title keeps its font, size, colour, alignment, and effect.

## Edit

- Space plays and pauses.
- S splits the selected clip at the playhead.
- Drag a clip to move it. Drag a clip edge to trim. Nearby clips magnet
  at 8 px. The magnet also takes the playhead, the sequence start and end, the
  In and Out points, markers, and detected beats. While a drag is in flight the
  guide line names what it locked onto, so a snap onto the wrong edge is
  visible. Hold Alt to suspend the magnet for that one drag; the toolbar magnet
  stays on.
- Drag on empty timeline space to rubber-band: every unlocked clip the band
  touches is selected, in either direction, across every lane between the two
  corners. A press with no drag clears the selection. Esc abandons the band and
  leaves the selection as it was.
- Arrow keys nudge one frame. Shift+Arrow nudges ten.
- Delete or Backspace ripple-deletes the selection.
- Ctrl+Z / Ctrl+Y undo and redo.
- Double-click a clip to expand its keyframe strip.
- Double-click a clip's name to rename it. The name is stored on the clip, so it
  survives a relink or a media swap; committing an empty name gives the card the
  source's own name back.
- F fits the timeline to the sequence, Shift+F to the selection. + / - zoom.
- Right-click a clip, track, or the preview for a context menu.
- Alt+F is Match Frame: it opens the source of the clip under the playhead in the
  monitor, on the same frame, with that clip's take marked. The selection wins
  when it is under the playhead; otherwise the topmost clip does.
- With a source loaded in the monitor, `,` inserts its marked range and `.`
  overwrites with it. The sequence's own In point is where the clip lands, and
  the playhead stands in when there is none. Mark the sequence at both ends and
  the edit becomes four-point: the marked span decides the length and the take is
  retimed to fill it, rather than trimmed to fit.

### Trim tools

The tool strip picks what a drag means. Five tools trim without moving a clip's
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
- **Ripple trim (P)** trims an edge and moves the rest of the lane by the same
  amount, so the trim leaves no gap. Drag either edge: every clip later on that
  lane follows. Trimming the head slides the clip back to where it started, so
  the lane keeps its own start.

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
  retimes it to fill the range exactly. The copy lands at the In point, not at
  the playhead. Mark In and Out first: with no range marked the row says so and
  changes nothing.

The clipboard holds a reference, not a copy: deleting the copied clip makes the
pastes do nothing rather than paste a stale version of it.

The inspector on the right shows transform, look, effects, audio, text, and
info for the selected clip. Tabs hide when they do not apply.

The strip along the bottom shows the timecode, the frame rate, the track count,
and the last message. A status is a receipt for what just happened and clears
after four seconds. An error stays until an action succeeds: an edit, an undo, or
a redo replaces it.

The timecode in the tool strip is a field. Click it to type a position: a bare
number is a frame count, and one to four colon-separated fields fill from the
right, so `12` is twelve frames, `5:12` is five seconds and twelve frames, and
`01:00:05:12` is the full clock. `;` works where `:` does. Enter seeks. Anything
that is not a timecode is refused by name and the playhead stays where it was.
Esc leaves the field without seeking. A position past the end of the sequence
lands on the end.

## Tracks

Each track header carries a collapse chevron and six toggles: mute, solo (`S`),
lock, visibility, target (`T`), and sync lock (`⇄`).

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
- **Lane Height: Small**, **Medium**, and **Large** set the lane to 34, 48, and
  96 pixels.

Drag a track header onto another header to move that lane to the row you release
over. The target row is marked while the pointer is over it, Escape abandons the
drag, and a drop is one edit. The main track stays at the bottom of the stack.

Drag a lane's lower border to resize it. Heights below 28 or above 200 pixels are
clamped. The drag is one edit, so one Undo restores the previous height, and
Escape abandons it. Lane height, the collapse state, and the two strip options
are stored with the sequence: reopening the project gives back the layout you
left.

The chevron collapses a lane to a 20-pixel strip: each clip is drawn as a flat
bar in its own colour, so the cut pattern stays readable while the lane takes
almost no room. The height is kept, so expanding restores the lane you sized.

**Shift selection…**, in a clip's context menu or the command palette, asks for a
signed frame count. The offset moves the selection on every lane it spans, from
the earliest selected clip on each, carrying everything later on those lanes with
it. A leading `+` is accepted. An offset that would move a clip before the start
of the sequence is refused whole, and anything that is not a whole number of
frames is reported rather than guessed at.

Dropping media on the timeline follows the edit mode. In Ripple mode the drop
opens a hole first and pushes the clips at or after that point right, so nothing
is covered. Otherwise the clip lands where it was dropped.

### Routing

Target picks which lane an edit lands on when you did not aim at one: Insert,
Overwrite, and Fit to Fill from the source monitor, and a media drop with no
lane under the pointer. The topmost targeted lane of the media's kind takes it.
Aiming at a lane still wins over the flag. With no lane of that kind targeted,
the kind's first lane takes it, and one is created if the sequence has none.

Sync lock decides which other lanes follow a ripple edit. A lane with sync lock
set moves by the same amount as the lane being edited, so picture and sound stay
in step. Clearing it pins that lane: a ripple edit elsewhere then slides past it.
The lane being edited always moves, whatever its own sync lock says, and the
whole ride-along is one Undo.

An insert opens a hole and keeps every clip: a sync-locked clip lying across the
insert point is cut there and both halves survive. A ripple delete removes a
stretch of timeline, so a sync-locked lane loses that stretch as well — a clip of
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
status strip carries the nest's name while it is open; clicking that chip
leaves it, as does **Sequence > Leave Nested Sequence**.

**Sequence > Place Nest At Playhead** puts a second clip playing the same nest
at the playhead, on the lane a media drop would land on. Editing inside the
nest changes both clips, because both play the one sequence.

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

Each clip card carries a small square in its top left and top right corner. The
square sits where the fade currently ends, so its position is the fade length.
Drag it along the card to set the fade in or the fade out; the inspector shows
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

## Viewing the preview

The preview pane fits the frame to the pane by default. The transport row
carries `−`, the zoom readout, and `+`. The readout shows `Fit` or a whole
percentage; clicking it returns to fit. The buttons walk a fixed ladder: 25, 50,
75, 100, 150, 200, 400 per cent, and stop at either end.

Above 100 per cent the frame can be panned: the cursor becomes an open hand and
dragging moves the picture. Returning to fit, or zooming back to 100 per cent or
below, re-centres it.

**View** carries the overlays, each independent of the others:

- **Title safe** insets 10 per cent on every side.
- **Action safe** insets 5 per cent.
- **Grid** draws thirds.

**View > Fullscreen Preview** fills the window with the viewer, hiding the
sidebar, inspector, and timeline. Escape gives them back. A dialog opened by a
shortcut still appears over the picture, and the status line stays visible so a
refusal remains readable.

### Output meter

Two bars at the right of the transport row show the peak level of what the
device is playing, left above right. A full bar is 0 dBFS. The bars turn red and
stay red once a sample reaches full scale; click them to clear that. The meter
shows output, not the selected clip, so soloing or muting a track is visible in
it. Stopping playback drops the bars to silence.

Zoom, pan, the overlays, and fullscreen are viewing positions, not edits: none
of them enters the undo history or changes what an export produces.

## Mixer

Shift+6, or **View > Mixer**, replaces the preview with the mixer. It carries one
strip per lane that has sound, plus one per submix bus, drawn in the order the
mixdown folds them: a bus sits after everything that feeds it. The Master strip
comes last and carries the master's **Inserts**.

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

- The colour wheels: lift, gamma and gain, each with a red, green, blue and
  master control.
- One row per grade slider the catalog carries — brightness, contrast,
  saturation, exposure, temperature, tint, hue, highlights, shadows, sharpness
  and vignette, then the twelve wheel values as numbers. Left click a row
  raises the value, right click lowers it. **Reset grade** returns every
  adjustment to neutral.
- A blend mode for the layer. Normal composites source-over.
- Stabilize, offering the three smoothing levels and Off. The section is
  absent on a clip with no motion to smooth, such as a still.
- Denoise, which runs RNNoise over the clip's audio on export.

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

The preview transport carries a **Handles** control naming which set of on-canvas
handles is live: Transform, Crop, or Mask. Click it to step to the next set, or
use **View > Cycle Canvas Handles**. Only one set is drawn at a time, because a
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

## Mask and chroma key

Below the grade, the Look tab carries:

- The six mask shapes — linear, mirror, circle, rectangle, heart, star. The
  applied shape is marked. Feather, roundness, rotation, width, height, and
  the two offsets appear once a shape is applied, along with Invert and
  Remove mask. Changing the shape keeps the shaping.
- Mask handles on the preview, once a shape is applied: a dot at the centre moves
  the mask, one on its right edge resizes it, and one above it rotates it. The
  outline follows the pointer; the composite catches up on release, which commits
  centre, size, and rotation as one edit. Escape abandons the drag.
- Mask tracking from the command palette: **Track mask forward from the
  playhead**. It follows the picture under the mask through the rest of the clip
  and keyframes the mask centre for every frame it followed. The pass is a job,
  so the editor keeps drawing while it runs; the jobs board counts the frames it
  has read against the frames it will read, and Cancel stops it and keeps what it
  tracked. A pass that loses the feature stops there and names the frame. Undo
  removes the whole track in one step.
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

The Effects tab lists the effect chain in render order. Each row carries ↑ and
↓ to move that effect one step, hidden at the ends of the chain, and a bin to
remove it. Parameters of the applied effect appear under its row:

- A numeric parameter steps within the range the effect declares. Left click
  raises it, right click lowers it.
- A point parameter is two rows, x and y, stepping the same way.
- A colour parameter is a row of swatches with the current value named beside
  the label. A colour has no plain value in the project format, so a click
  writes a keyframe: at the playhead when the playhead is over the clip, and at
  the clip's first frame otherwise. Setting a colour at two playhead positions
  animates it.

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
clip on the left of the cut; the Look tab then shows the Transition block. It
is absent when the clip has no clip abutting it on the right, and on lanes that
cannot carry a transition.

- The catalog lists cross fade, dip to black, dip to white, the four wipes, and
  slide. The applied blend is marked. Picking another replaces it.
- Duration sets the window length in ticks, centered on the cut.
- **Remove transition** clears it.

The timeline draws the window as a band across the cut. Drag either edge of the
band to set the length; both edges move the same amount, since the window is
centered on the cut. Right-click the band for **Remove transition** and the
blend catalog. Both act on that cut whatever is selected.

Deleting or moving either clip so the two no longer touch drops the transition;
undo restores it.

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

Last is the audio layout: Mono, Stereo, or 5.1 Surround. The layout sets how many
channels the sequence mixes to and what export writes.

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
library thumbnails, timeline filmstrips and waveforms, proxies, analysis state, AI
model weights, downloads, catalog data, LUTs, Lottie assets, and templates.

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

A workspace is a named arrangement of the editor. Four ship:

| Workspace | What it arranges |
| --- | --- |
| Editing | The default. Rail and inspector at their normal widths, timeline 280 px. |
| Colour | Rail hidden, wide inspector, scope strip open, preview focused. |
| Audio | Tall timeline, narrower inspector, timeline focused. |
| Effects | Rail hidden, widest inspector, inspector focused. |

Pick one from **Window**, from Settings > Workspaces, or from the command palette
by name. Applying a workspace sets the sidebar and inspector widths, whether
either is collapsed, the timeline height, the scope strip, the focused pane, and
the route.

**Window > Save Workspace** keeps the arrangement on screen as **Custom**. There is
one such slot: saving again replaces it. The Custom row appears on the Window menu
only once something is saved in it, and **Forget** in Settings > Workspaces removes
it again. The slot is written to `ui-settings.json`, so it survives a restart.

Settings > Workspaces lists every workspace with its measurements, and marks the
one the panes are arranged like now. Drag a divider and no workspace is marked:
the mark is measured, not remembered.

## Finding a setting

The box at the top of Settings filters the sections by name. It narrows as you type,
Escape clears it, and it is never written to the config file.

The Appearance section carries two rows: **System**, **Light**, and **Dark** choose
the appearance, and **Accent** chooses the hue used for the accent, the playhead, and
the caret. The accent choice persists in `[appearance]` and applies immediately.

The Cloud section carries the backend base URL. Left empty it uses the shipped
default.

## Checking for updates

**Help > Check for updates** asks the cloud endpoint for the newest published
version. The status line reports either that the installed version is current, or
which version is newer. A failed check names the failure and still reports the
installed version.

## Stock and sound effects

The media sidebar carries a **Stock** tab and an **SFX** tab.

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

The sidebar's **Generate** tab sends a prompt to fal.ai with your own key.

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
the sidebar's **Captions** tab. Set a language code such as `en` in the box, or
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

The sidebar's **Analysis** tab runs three detectors on the selected clip and keeps
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
Inspector's Audio tab, one column per window, with the peak level and a clipping
flag. The contour is measured on demand, not per frame.

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
3. Open the Inspector's **Audio** tab and use **Duck under voice**.

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

The filter box above the list matches an item's name, its path, its kind, its
frame size, and its label colour, so a search for `1920`, `wav`, or `teal` finds
media whose name says none of them. The line under it reports how many of the
pool the filter leaves. The
order is **Name**, **Duration**, **Resolution**, or **Date added**; duration and
resolution sort largest first, and date added sorts newest first. An item from a
project saved before import times were recorded has none and sorts last. Neither
the filter nor the order is saved with the project.

The chips beside the order choose how the pool draws itself. **Tiles** puts each
item on its own row with its picture above its facts. **List** drops the picture
and reads each item as one line, name on the left and facts on the right, which
is the view to use on a pool of hundreds: no picture means no decode. **Icons**
wraps the pictures into a 96px grid with the name underneath and no facts. The
view is saved with your settings, not with the project, so it is the same in the
next project you open.

Click an item to place it at the playhead and to select it. Each row states the
frame size, the length, the video codec, and either the audio codec or that the
file is silent, so a silent video is visible before it is cut in. The block under
the list reports what the pool knows about the selected item: container, frame
size, frame rate, duration, video codec, audio, when it was imported (UTC),
whether it is still linked, and its path.

Ctrl+click (Cmd+click on macOS) adds an item to the selection, or drops it if it
is already in. Shift+click picks every item drawn between the last one you
clicked outright and the one you shift-click, in the order the list is showing.
Neither places a clip: a modified click only changes the selection. Selected
items are bordered, and the properties block describes the last one clicked.

The order of the selection is the order you clicked, which is the angle order of
a multicam group built from it. Opening another folder drops the selection.

The container is the file extension. A codec name is what the platform's demuxer
reported at import: on Linux, GStreamer's description of the track's caps. Where
a backend names no codec, the row reads `unknown` rather than guessing from the
extension, which would name the container again. Stills carry no codec track and
read `unknown`.

Drag an item onto the timeline to place it where you drop it:

- Dropped on a lane, it lands on that lane at the tick under the pointer. The
  magnet applies, so a drop near a cut, a marker, or the playhead takes that
  position. A lane that cannot hold the media — audio on a video lane — passes
  it to a lane of the right kind at the same tick.
- Dropped on the thin strip above the top lane or below the bottom one, it
  creates a lane of its own kind and lands there. This is the only drop that
  adds a lane.

Clicking still places at the playhead.

Right-click a pool item for **Remove from project** and for the label swatches.
An item is removable only
while no clip references it; a removal that a clip still holds is refused with
the reason. Removal is undoable, and the undo reattaches any clips removed
alongside it.

### Bins

The pool holds folders. The strip under the filter box shows where you are —
`Pool / Footage / Day 1` — and lists the folders inside the one that is open.

- **New bin** makes a folder inside the open one, opens it, and starts its name
  in a text field. Type the name and press Enter; Esc leaves the field without
  renaming. An empty name is refused.
- Click a folder to open it. Double-click it to rename it. Click a crumb to go
  back out; `Pool` is the root.
- **Rename** and **Delete bin** act on the open folder. Deleting one moves its
  media and its folders up to its parent, so nothing leaves the pool. Both are
  undoable, and undoing a deletion puts the folder back with what it held.
- Drag a pool item onto a folder row or a crumb to file it there. Drag a folder
  onto another folder or a crumb to move it. A folder cannot be dropped into
  itself or into one of its own folders.

The list shows what is directly in the open folder. Typing in the filter box
searches the open folder and everything under it, so a file one level down is
still findable, and the count beside the box describes that same scope. Bins are
saved with the project; which folder is open is not.

### Previewing a source

Each pool row carries a picture of its source. Move the pointer across that
picture to scrub the file: the left edge is its first frame and the right edge
its last. Nothing is placed and nothing is edited; a still shows its one frame.
A row shows a glyph until its first frame is decoded, and an audio-only file has
no picture.

With the pool focused, `Space` loads the selected item into the preview pane as a
source. The pane names the file and the frame it is showing, and scrubbing that
item's row moves it. `Space` again, **Sequence** in the corner, or `Space` over
any other panel returns the pane to the sequence. Loading a source stops
playback: the pane shows one thing at a time. Safe areas and the transform
handles belong to the sequence and are not drawn over a source.

The bar under a loaded source is its own scrub bar: drag it to move the frame on
screen, which moves the row's picture with it. `I` marks the source in point and
`O` the out point while a source is loaded, so those keys mark the take you are
looking at rather than the sequence; `Alt+I` and `Alt+O` clear them, and
**Clear** clears both. An out point is exclusive, so the frame you marked is
included. The bar shades the marked range and the line beside it counts the
frames kept.

**Insert** (`,`) puts the marked range into the sequence at the playhead and
pushes the rest of the lane right. **Overwrite** (`.`) puts it there without
moving anything: material inside the span is dropped, material reaching into the
span is trimmed to the edge it crosses, and a clip that spans the whole window
survives on both sides of it. Mark the sequence at both ends first and either
button becomes a four-point edit: the sequence's span decides the length and the
take is retimed to fill it. Each is one undo entry. The monitor stays open, so
takes can be cut in one after another.

### Multicam

A multicam group is several cameras of the same take, cut between as one clip.

Select the camera files in the pool and press **+ Multicam Group**. The group
takes one angle per selected item, in the order you clicked them, and appears as
a row below the button. With nothing selected the button groups the whole pool,
which is what an import of a card's worth of angles wants. An empty pool says so
and builds nothing.

The row names the group, counts its angles, and carries one row per camera. The
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
where its sound says it belongs. The first camera is the clock and does not
move, so syncing a group twice gives the same answer, and the whole group is one
undo entry however many cameras moved.

A camera it cannot place is left where it is and named: sound still being read,
a file with no sound or none that varies, sound that does not match the first
camera's well enough to be the same take, or sound that repeats. A repeating
sound matches at every repeat, so the best match is whichever repeat came first
rather than the right one. An offset applied from a guess would move a camera
that was already in sync, and nothing on screen would tell you it had.

## Markers

`M` drops a marker at the playhead, `Shift+M` removes the one under it, and the
sidebar's **Markers** tab lists them all in time order.

Each row shows the marker's colour, its timecode, its label, and its note.
Clicking the row seeks to it. The colour chip advances the eight-colour palette.
**Rename** edits the label, **Note** edits the comment, and **Remove** deletes
the marker. Every one of those is a single undo entry.

A note is free text kept with the marker. An empty note draft clears the note,
where an empty name draft cancels instead: a marker is allowed to have no note
and is not allowed to have no name. A project saved with no notes is byte-for-byte
what an older version of the app wrote, so a note costs nothing until you write
one.

## Label colours

A label colour groups clips that belong together. Right-click a clip: the menu
carries a row of eight swatches and an `×`. Clicking a swatch labels every
selected clip, `×` clears the label, and each is one undo entry per clip.

A labelled clip is drawn in its label colour instead of its kind colour, so the
grouping is what you see on the timeline. **Select same label** selects every
clip carrying the label of the clip you clicked, in timeline order, and the
status line reports how many it found.

The eight colours are the marker palette, so the teal group and a teal marker
are the same hue. A project saved with no labels is byte-for-byte what an older
version of the app wrote.

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
- Ctrl+O opens a project. Ctrl+W returns to Home.
- Ctrl+E opens Export. Choose a destination or accept the default `.mp4`.
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

**Window > History**, `History` in the command palette, or Ctrl+Shift+H lists
every step this session took, oldest first, starting with **Project opened**.

Each row is one undo step, named after the command that made it: `Split clip`,
`Move clip`, `Add effect`. A gesture that ran several commands is one row named
after its first, with the count of the rest: `Move clip +2` undoes all three.

The row the project is at is marked **current**. **Go** on any other row moves
the project to the state after that step, in either direction; it is undo and
redo without counting keystrokes. A step that has been undone stays in the list,
dimmed, because redo can still reach it. Making a new edit while dimmed rows are
present replaces them, exactly as Ctrl+Z then a new edit always has.

History is per session. Saving does not clear it; opening or loading a project
does.

## Jobs

**Window > Jobs**, or `Jobs` in the command palette, lists background work:
exports, cloud downloads, transcription, and analysis.

Each row shows the label, the state, a detail line such as `frame 240/1800`,
and the elapsed time. A job that reports a fraction draws a filled bar; one
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
installed; the next prompt simply does not carry it. Clearing the conversation
switches them all back on.

Rules are read from `~/.aria/agent/` at send time. The rule text is capped, and a
prompt that had to truncate it says so in the transcript instead of quietly
sending less than the files hold.

### What it can do besides edit

Timeline edits go through the validated vocabulary and land in the undo history.
Alongside them the assistant has app tools:

- **Look at a frame.** It renders the timeline at a time and looks at the picture,
  so it can check what a shot actually shows before changing it. The row in the
  transcript names the moment it looked at.
- **Read a media file.** Size, frame rate, length, and codecs come from the file
  itself, not from its name.
- **Move the playhead** and **save the project**.
- **Empty a cache.** Destructive, so it goes through **Autonomy**.

With **Autonomy** set to `ask`, a destructive call stops and the transcript shows
what it would do with **Allow** and **Deny**. The turn is genuinely waiting: it
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
