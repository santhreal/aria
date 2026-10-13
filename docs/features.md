# Features

What the editor does today. Each entry names the surface it is reached from.
Anything not listed here is not in the build.

## Project and media

- Create, open, save, and Save As a `.aria` project. Home lists recents with
  their last-modified time and drops entries whose file is gone.
- Import files from the File menu, the pool, or a drop onto the window. A
  multi-file drop lands the files end to end on the matching lane.
- Media pool with bins and folders, rename, remove, and relink for a moved
  source.
- Save the session as an `.ariat` template, and start a project from one by
  filling its replaceable slots.
- Autosave and crash recovery. A recovered session is offered on next start.

## Timeline editing

- Video, audio, and generator lanes. Add, rename, reorder, lock, mute, solo,
  collapse, and set a lane's height.
- Select by click, add with Shift, toggle with Ctrl, marquee with a drag on
  empty timeline. A selection follows clip links; Alt+click ignores them.
- Move, ripple move (Shift-drag), trim, split, ripple delete, duplicate,
  cut, copy, paste, paste insert, and paste attributes.
- Slip, slide, roll, rate, and ripple trim, each with its own tool.
- In and out points on the sequence and on a loaded source. Insert and
  overwrite from the source monitor, three-point and four-point, with the take
  retimed to fill a marked span.
- Markers with automatic names and colours, next and previous marker, rename,
  and delete at the playhead.
- Snapping to clip edges, markers, the playhead, and the in and out points,
  with a guide drawn at the edge that caught.
- Speed and speed-curve ramps, reverse, and freeze frame.
- Nested sequences. Nest a selection into a sequence of its own, open it, place
  another copy of it at the playhead, and leave it. The status strip names the
  sequence that is open, and clicking that name leaves it. An edit made while a
  nest is open lands in the nest.
- Multicam camera groups. Build one from the pool, cut between its cameras at
  the playhead, and place it on a lane; the frame decodes the camera live at
  that point, fitted at its own shape, and the placement is as long as the
  group's longest camera reaches. Each angle carries its own offset on the
  group's clock, drawn as a waveform strip, nudged by the frame, or measured
  from the cameras' own sound.
- Waveform overlays and filmstrip thumbnails per lane, decoded once per source
  and shared between clips cut from it.

## Preview

- Fit, 100%, and a zoom ladder with pan while zoomed.
- Title-safe, action-safe, and thirds overlays. Fullscreen preview.
- On-canvas handles for transform, crop, and mask, cycled from one control.
- Scopes: waveform monitor, vectorscope, and RGB histogram, measured from the
  frame on screen and remeasured with the playhead. Open from the preview
  toolbar, the View menu, or the palette.
- Peak meter with a clip indicator.
- Source monitor for a pool item, with its own scrub bar and marked range.

## Effects, transitions, and generators

- Twelve effects and eight transitions, resolved from data files rather than
  code. Every catalog id has a GPU pass; an unknown effect is skipped and an
  unknown transition cross-dissolves.
- Effect parameters per clip, including colour and vector slots, with
  keyframes and easing presets.
- An effect chain is saved as a named preset and applied to another clip in one
  undo entry. A saved preset is deleted from its own row. An empty or duplicate
  name is refused.
- Keyframes on every animatable clip property, not a fixed list: the strip reads
  `clip_param_catalog()` and offers what the selected clip can actually animate —
  transform, opacity, volume, pan, crop, colour grade, mask geometry, chroma
  tolerance, matte softness, layer shadow, glow, outline and background, text
  stroke, shadow and card, and shape fill, stroke and geometry. A property whose
  block is absent is not offered.
- Inspector numeric rows come from the same catalog as the keyframe strip. A
  block names its property group; the rows are whatever the selected clip owns in
  it, so the transform block carries the anchor point, the text block carries the
  twelve text properties, and a shape clip gets a Shape tab with its geometry and
  stroke. Each row takes a keyframe under the playhead when the property is
  animated and a constant when it is not, so a nudge moves a curve instead of
  replacing it.
- Every chip and button in the inspector is a registry row with one writer:
  filter, mask shape, key colour, matte source, layer style and its colour,
  blend mode, stabilization level, the flips, motion blur, noise reduction, the
  audio role and fades, reverse, pitch lock, the animation chips and their knobs,
  the speed ramps, a template slot's media, kind, order and hint, the text
  effects, presets and fonts, the transitions, the effects rack, and every button
  on a keyframe row. A pick made while nothing is selected does nothing and is
  not an undo entry.
- The key button on a keyframe row takes a keyframe under the playhead, or takes
  back the one already there. The two arrows beside it move the playhead to the
  next and previous keyframe on that property and change nothing.
- A mixer strip's routing is six gestures: point a lane at a bus, cycle a send
  off, post and pre, grab a fader, ride it, release it, and add a submix bus.
  Riding a fader writes one undo entry when you let go, and a fader pressed and
  released in place writes none.
- Transform, crop, and mask per clip. Chroma key. Colour filters with an
  intensity per clip.
- Mask tracking: "Track mask forward from the playhead" in the palette follows
  the picture under the mask across the rest of the clip and writes its centre
  keyframes. The pass runs as a job on a renderer of its own, so the window keeps
  drawing and stays scrubbable while it tracks, and the jobs board counts frames
  against the total. It stops when the feature is lost and reports the frame that
  lost it, and Cancel keeps the frames it already followed. What it wrote is one
  undo entry either way.
- Text generator with font, size, weight, slant, tracking, wrapping, padding,
  alignment, colour, and stroke.
- Shape generator: rounded rectangle, ellipse, polygon, star, line, arrow, and
  heart, each with fill and stroke.
- Adjustment layers: a lane bar that applies its own effect stack, grade, and
  LUT to everything below it. Add one from the Sequence menu or the palette; it
  covers the marked range, else the selection, else the sequence, and stacks
  onto a second lane when the range is taken. Its effect parameters and its
  grade take keyframes on the same routes a clip's do, keyed in sequence time:
  the look stays over the programme it was set on when the bar is dragged or
  split.
- Track mattes: a clip is cut by the layer directly above it, reading either that
  layer's alpha or its luma, with an invert toggle and a softness that blurs the
  matte edge. Set one from the inspector, the Sequence menu, or the palette. A
  clip with nothing above it draws unmatted, and the inspector says so.

## Audio

- Per-clip volume, pan, and fades, mixed the same way for preview and export.
- Pitch-preserving retime by default, with a per-clip lock that can be cleared
  to mix varispeed instead.
- Noise reduction per clip.
- Loudness and true-peak measurement, and beat detection, run as jobs.
- Duck measurement writes editable volume keyframes rather than a hidden
  automation pass.
- Extract audio from a clip onto its own lane.
- Audio effect chains on clips, lanes, and the master: Gain, an eight-band
  Parametric EQ, Compressor with a sidechain key, Limiter, De-esser, Noise
  Reduction, Reverb, Delay, Pitch Shifter, four channel tools, and a Loudness
  Normalizer. Parameters take keyframes, the mixer compensates each effect's
  latency, and a plugin adds effects of its own.
- A mixer, one strip per lane and per submix bus, on Shift+6. Each strip carries
  the lane's fader in decibels, the bus or master it outputs to, and a chip per
  bus that cycles the send off, post-fader, and pre-fader. Strips are drawn in
  the order the mixdown folds them, so a bus sits after everything feeding it,
  and a lane whose signal never arrives at the master is marked.
- A peak meter beside every fader, in the mixer and on the transport row. A
  strip's bars read that lane's own level after its own fader, so a bus reads
  what everything routed into it sums to. A lane the mix is not carrying reads
  silence. Full scale is the top of the fader's travel, and a bar that reaches it
  turns red.
- Submix buses: add one from the mixer or the palette, route lanes and other
  buses into it, and mix it with one fader. A level outside 0 to +12 dB, a route
  that would loop, and a second send to the same bus are refused by name rather
  than clamped or silently merged. Deleting a bus returns its feeders to the
  master, and one undo restores both the lane and the routing that pointed at it.

## Assistant

- Prompts run as jobs against a configured provider. Edits land in the live
  session as the model makes them and undo like any other edit.
- Plan-only mode reports the edits a turn would make; Apply replays exactly
  that plan as one undo entry.
- Per-project conversation history, kept beside the app's session data rather
  than in the project file.
- Skills can be switched off for one conversation. Host tools are gated by an
  autonomy setting, and a confirmation is a card in the transcript.

## Jobs, analysis, and cloud

- A jobs board with progress and cancellation for every long operation.
- Shot detection, transcription, and object detection over decoded media.
- Cloud catalogs with a quota and an evicting cache.

## Export

- Export a sequence to a file with progress. Encoders are the platform's own:
  AVFoundation, Media Foundation, or GStreamer.
- Render and replace one clip: what the renderer folds into a picture clip is
  encoded to a file that stands in for it, kept in `<project> Renders` beside
  the project and named after what it shows. Restore unrendered puts the clip
  back, mapping trims, moves and splits made since through the render.
- A clip on a sound lane renders the same way, to a WAV of the mix its own
  volume, pan, fades and denoise produce. The lane's fader, mute, solo and
  routing stay out of the file, because they are applied again when it plays
  back.
- The assistant renders and restores the same way, by clip. A render it asks for
  runs as a job, and it reports that the render started, not that the clip was
  replaced.

## Chrome and plugins

- Home route and editor route. App bar with File, Edit, Sequence, View,
  Window, and Help menus, client-side on Linux.
- Command palette over every registered command and menu row.
- Settings for appearance, keys, storage, providers, and each plugin's own
  schema.
- Four shipped workspaces (Editing, Colour, Audio, Effects) plus one slot for
  your own, on the Window menu and in Settings. A workspace restores the rail and
  inspector widths, whether either is collapsed, the timeline height, the scope
  strip, the focused pane, and the route.
- Every panel, dialog, overlay, palette command, menu row, key binding,
  setting, and catalog entry comes from a registry. The shell names no panel
  module. See [plugin-development.md](plugin-development.md).

## Not in the build

Named because their absence is a question a Premiere or Resolve user will ask.

| Absent | Nearest shipped surface |
| --- | --- |
| Per-channel curves | lift, gamma, and gain wheels, and a draggable curve per animated parameter |

## Related documents

- [getting-started.md](getting-started.md) walks the editor surface by surface.
- [keyboard-shortcuts.md](keyboard-shortcuts.md) lists every binding.
- [command-line.md](command-line.md) covers the headless CLI.
- [architecture.md](architecture.md) covers the crates and the UI boundary.
