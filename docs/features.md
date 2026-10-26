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
  empty timeline. A selection follows clip links and clip groups; Alt+click
  ignores them.
- Enable or disable clips from the Clip menu, the clip context menu, or the
  palette. A disabled clip stays on its lane at 40% opacity and is omitted from
  preview, export, and audio mixdown.
- Clip groups from the Clip menu, the clip context menu, or the palette.
  Selecting or moving one member selects or moves the group; trims stay per
  clip.
- Move, ripple move (Shift-drag), trim, split, ripple delete, duplicate,
  cut, copy, paste, paste insert, and paste attributes.
- Slip, slide, roll, rate, and ripple trim, each with its own tool.
- In and out points on the sequence and on each Source tab. Insert and
  overwrite from a Source tab, three-point and four-point, with the take
  retimed to fill a marked span. Source patching sends the picture and the
  sound to the lanes lit in the lane headers, as linked clips.
- Linked selection on or off: linked clips select, move, and trim together, or
  one at a time.
- Sequence and clip markers with automatic names, eight colors, duration ranges,
  Comment/Chapter/Segmentation kinds, the Edit Marker dialog, color filtering
  in the Markers panel, next and previous marker, and delete at the playhead.
- Snapping to clip edges, markers, the playhead, and the in and out points,
  with a guide drawn at the edge that caught.
- Speed/Duration dialog (from the Clip menu, the clip context menu, or the
  palette) for constant speed, duration, reverse, and pitch lock in one undo
  entry; Time Remap and speed-curve ramps; freeze frame.
- Nested sequences. Nest a selection into a sequence of its own, open it, place
  another copy of it at the playhead, and leave it. The status strip names the
  sequence that is open, and clicking that name leaves it. An edit made while a
  nest is open lands in the nest. A nest clip draws every lane of its sequence
  as one picture, which takes the clip's own transform, effects, mask, and look.
- Multicam camera groups. Build one from the pool, cut between its cameras at
  the playhead, and place it on a lane; the frame decodes the camera live at
  that point, fitted at its own shape, and the placement is as long as the
  group's longest camera reaches. Each angle has its own offset on the
  group's clock, drawn as a waveform strip, nudged by the frame, or measured
  from the cameras' own sound.
- Waveform overlays and filmstrip thumbnails per lane, decoded once per source
  and shared between clips cut from it.

## Preview

- A control row with the playhead field, the transport, and In and Out. The
  transport moves the tab in front of the Viewer: Program or a Source tab, and
  a held step button repeats once per frame. Drag the playhead field to scrub
  it, or click it to type a timecode.
- A mini ruler under the control row spanning the tab in front, with its In
  and Out range, the sequence markers, and a playhead to drag.
- Fit, 100%, and a zoom ladder with pan while zoomed.
- Title-safe, action-safe, and thirds overlays, under the display wrench in the
  Viewer's control row. Fullscreen preview.
- On-canvas handles for transform, crop, and mask, cycled from one control.
- Scopes: waveform monitor, vectorscope, and RGB histogram, measured from the
  frame on screen and remeasured with the playhead. Open from the Viewer's
  control row, the View menu, or the palette.
- The waveform shows luma, an RGB parade, or a YCbCr parade, chosen from the
  chips over it or View > Scope Waveform. The choice is saved with the UI
  settings.
- The vectorscope draws a skin tone line and a target box for each primary
  and secondary at 75 % and 100 %.
- Peak meter with a clip indicator.
- Program and Source tabs in one Viewer, each Source tab with its own scrub
  bar, marked range, and transport (play, J/K/L shuttle to 8x, step, and
  jumps to In, Out, and either end), and a split that shows a Source tab beside
  Program.
- Rulers in canvas pixels, guides pulled from them, and layer snapping to the
  guides.

## Effects, transitions, and generators

- Nineteen effects and eight transitions, resolved from data files rather than
  code. Every catalog id has a GPU pass; an unknown effect is skipped and an
  unknown transition cross-dissolves.
- Effect parameters per clip, including color and vector slots, with
  keyframes and easing presets.
- Animation presets (`.ariapreset`) capture a clip's visual effects, audio
  inserts, and keyframed properties, each mask's by its index in the stack,
  with per-part toggles when saving, and apply across every selected clip in one
  undo entry anchored at the playhead when inside the clip or at the clip start
  otherwise. User presets are stored in
  `<data dir>/presets/*.ariapreset` and imported by file drop or File > Import;
  plugins contribute read-only presets via `register_preset` or
  `presets/*.ariapreset`.
- Keyframes on every animatable clip property, not a fixed list: the strip reads
  `clip_params_for(clip)` and offers what the selected clip can animate:
  transform, opacity, volume, pan, crop, color grade, each mask's geometry,
  path, feather, opacity, and expansion, each effect's parameters, chroma tolerance,
  matte softness, layer shadow, glow, outline and background, text stroke,
  shadow and card, and shape fill, stroke and geometry. A property whose block
  is absent is not offered.
- Effect Controls numeric rows come from the same catalog as the keyframe strip.
  A block specifies its property group; the rows are the properties defined for the
  selected clip in that group, so the transform block includes the anchor point, the
  text block includes the eleven text properties, and a shape clip gets a Shape tab with its geometry and
  stroke. Each row takes a keyframe under the playhead when the property is
  animated and a constant when it is not, so a nudge moves a curve instead of
  replacing it.
- Every chip and button in the inspector is a registry row with one writer:
  filter, mask shape, key color, matte source, layer style and its color,
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
- Transform and crop per clip, and a stack of up to 32 masks. Each mask has a
  mode (None, Add, Subtract, Intersect, Lighten, Darken, Difference), an
  opacity, an expansion, and an invert toggle, and the stack reorders. Masks
  apply to footage, stills, titles, shapes, stickers, animations, and
  adjustment layers, and cut what the clip draws before its effects, LUT, and
  layer styles. Chroma key. Color filters with an intensity per clip.
- Mask tracking: "Track Mask Forward from the Playhead" in the palette follows
  the picture under the active mask across the rest of the clip and writes its
  center keyframes. The pass runs as a job on a renderer of its own, so the window keeps
  drawing and stays scrubbable while it tracks, and the jobs board counts frames
  against the total. It stops when the feature is lost and reports the frame that
  lost it, and Cancel keeps the frames it already followed. What it wrote is one
  undo entry either way, on the tracked mask wherever the stack has moved it; a
  tracked mask removed or reshaped while the pass runs gets nothing, and the
  editor reports it.
- Text generator with font, size, weight, slant, tracking, wrapping, padding,
  alignment, color, and stroke.
- Shape generator: rounded rectangle, ellipse, polygon, star, line, arrow, and
  heart, each with fill and stroke.
- Adjustment layers: a lane bar that applies its own effect stack, grade, and
  LUT to everything below it. Add one from the Sequence menu or the palette; it
  covers the marked range, else the selection, else the sequence, and stacks
  onto a second lane when the range is taken. Its effect parameters and its
  grade take keyframes on the same routes a clip's do, keyed in sequence time:
  the look stays over the program it was set on when the bar is dragged or
  split. A mask on the bar limits its look to the masked area.
- Track mattes: a clip is cut by the layer directly above it, reading either that
  layer's alpha or its luma, with an invert toggle and a softness that blurs the
  matte edge. Set one from the inspector, the Sequence menu, or the palette. A
  clip with nothing above it draws unmatted, and the inspector displays a notice. The
  clips in a matte, and each side of a transition, draw their masks, effects,
  LUTs, motion blur, and blend modes as they do alone.

## Audio

- Per-clip volume, pan, and fades, mixed the same way for preview and export.
  Audio Gain (from the Clip menu, the clip context menu, or the palette) sets
  gain, adjusts gain by dB, normalizes the selection's max peak to a dBFS
  target, or normalizes each selected clip's peak to a dBFS target in one undo
  entry.
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
- A mixer, one strip per lane and per submix bus, on Shift+6. Each strip has
  the lane's fader in decibels, the bus or master it outputs to, and a chip per
  bus that cycles the send off, post-fader, and pre-fader. Strips are drawn in
  the order the mixdown folds them, so a bus sits after everything feeding it,
  and a lane whose signal never arrives at the master is marked.
- A peak meter beside every fader, in the mixer and on the Viewer's control
  row. A strip's bars read that lane's own level after its own fader, so a bus
  reads what everything routed into it sums to. A lane the mix is not carrying reads
  silence. Full scale is the top of the fader's travel, and a bar that reaches it
  turns red.
- Submix buses: add one from the mixer or the palette, route lanes and other
  buses into it, and mix it with one fader. A level outside 0 to +12 dB, a route
  that would loop, and a second send to the same bus are rejected by name rather
  than clamped or silently merged. Deleting a bus returns its feeders to the
  master, and one undo restores both the lane and the routing that pointed at it.

## Assistant

- Prompts run as jobs against a configured provider. Edits land in the live
  session as the model makes them and undo like any other edit.
- Plan-only mode reports the edits a turn would make; Apply replays
  that plan, unchanged, as one undo entry.
- Per-project conversation history, kept beside the app's session data rather
  than in the project file.
- Skills can be switched off for one conversation. Host tools are controlled by an
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
- The assistant renders and restores the same way, by clip. A render it requests
  runs as a job, and it reports that the render started, not that the clip was
  replaced.

## Chrome and plugins

- Home route and editor route. App bar with File, Edit, Clip, Sequence,
  Markers, Effect, Animation, View, Window, and Help menus. Each row shows its
  current chord; a row that cannot run is dimmed and its tooltip states why.
- On Linux without server-side decorations the app bar is the title bar: its
  empty stretches move the window, a double click maximizes it, a right click
  opens the window menu, and the window's edges and corners resize it. The
  window title is the project name with a `*` while it has unsaved changes.
- Command palette over every registered command and menu row.
- Settings for appearance, keys, storage, providers, and each plugin's own
  schema.
- The Editing, Animation, Color, Audio, and Effects workspaces, plus named
  workspaces of your own, on the Window menu and in Settings. A workspace
  restores its dock arrangement of panels, splits, tabs, and floating windows,
  the timeline height, the scope strip, the graph editor, the focused panel,
  and the route.
- Every panel, dialog, overlay, palette command, menu row, key binding,
  setting, and catalog entry comes from a registry. The shell names no panel
  module. See [plugins/README.md](plugins/README.md).

## Not in the build

Listed because Premiere and Resolve have them.

| Absent | Nearest shipped surface |
| --- | --- |
| Per-channel curves | lift, gamma, and gain wheels, and a draggable curve per animated parameter |

## Related documents

- [getting-started.md](getting-started.md) covers first steps and [guide/README.md](guide/README.md) walks Aria surface by surface.
- [reference/README.md](reference/README.md) documents every catalog entry, tool, workspace, panel, menu, command, and setting.
- [keyboard-shortcuts.md](keyboard-shortcuts.md) lists every binding.
- [command-line.md](command-line.md) covers the headless CLI.
- [architecture.md](architecture.md) covers the crates and the UI boundary.
