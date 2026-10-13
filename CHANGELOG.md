# Changelog

Notes for the latest release. For previous releases, see the
[GitHub releases page](https://github.com/santhreal/aria/releases).


## Unreleased

### Added

- Plugins read and edit the project through `ProjectHost`: immutable snapshots, single commands, and named transactions that land as one undo step or not at all.
- Plugins receive `CommandApplied` after each command can no longer be rolled back, and the editor redraws when a plugin or the assistant edits.
- `CommandProposed` carries the project the command applies to and its origin, and external plugins must be rebuilt for loader version 2.
- An edit made during an assistant turn is refused with a message naming the assistant, instead of joining the turn's undo step.
- Plugin notifications are posted to a delivery thread in post order, so a slow handler no longer stalls the editor, and `on_event_main` runs a handler on the UI thread; external plugins must be rebuilt for loader version 3.
- Plugins receive project open, close, and save, media import, relink, and offline, selection, playhead, In/Out, playback, export, render, job, and plugin activation events.
- Plugins read the selection, playhead, In/Out, and transport through the `EditorView` service.
- Plugins add rows to every context menu through `ContextMenuRequested`; a clicked row runs off the UI thread.
- Plugins register effects and transitions written in WGSL, which render on clips, in previews, and in exports like shipped ones, and `render_effect` and `render_transition` now draw images; external plugins must be rebuilt for loader version 4.
- Plugins add generators, effects marked `generator: true` that the Generators tab and the palette place as clips in one undo step; external plugins must be rebuilt for loader version 5.
- Plugins render frames through `RenderHost`: the open sequence, any sequence, or one clip, at canvas, fitted, or exact size, as an image, per tick of a range, or into a GPU texture; external plugins must be rebuilt for loader version 6.
- Clips, lanes, and the master carry audio effect chains, edited in the inspector's Audio tab and the mixer's Inserts rows, with fourteen shipped effects, keyframed parameters, sidechain keys, and latency compensation in preview and export.
- Plugins add audio effects with `register_audio_effect` and mix project sound through `AudioHost`; a processor that panics or misses three playback deadlines is bypassed, and external plugins must be rebuilt for loader version 7.
- Plugins add timeline importers and exporters with `register_importer` and `register_exporter`; an import is one undo step, an export never leaves a partial destination, and external plugins must be rebuilt for loader version 8.
- The engine, project host, and jobs services registered by a session started without the engine plugin stay registered for the session instead of being dropped at once.
- Render and replace no longer hangs the editor when it swaps the rendered file in.
- Every button, icon, card, and menu row dims while pressed, so clicks read on every control.
- Shift-dragging a clip ripple-moves every sync-locked lane: the sound under the picture moves with it instead of staying behind.
- Opening a video file on a busy machine retries once before reporting a decode failure, so a slow preroll no longer reads as a broken file.

- Shift+Z fits the timeline to the selection, the same action as Shift+F.
- The timeline draws a sequence overview under the ruler; a click pans the view to that time.
- Shift-dragging a clip body ripple-moves it: the hole it left closes and the drop opens.
- Ripple-moving a clip now takes its linked companions with it, as one undo.
- Moving a clip now takes its linked companions with it, as one undo.
- With nothing selected, the inspector shows sequence rate, frame size, and duration, and tells you a gap click edits the sequence.
- The Edit menu names the pending Undo and Redo steps instead of a bare Undo/Redo.
- Assistant tools now derive their command variant, schema, decode, reverse name, and group from one registry row.
- BT.2020 exports now use BT.2020 conversion coefficients instead of BT.709 coefficients.
- Android and Windows decode now reject unknown pixel layouts instead of reading them as NV12.
- Every workspace target compiles on the declared Rust 1.97 minimum.
- Scope refreshes reuse their pixel canvases, playback recycles mixed audio blocks, and filmstrip edits invalidate only affected timeline spans.
- Built-in menus, panel-location resolution, inspector controls, clip timecodes, settings filtering, tooltips, shared control hover colors, the animated mark, sidebar catalogs, duck controls, toolbar clocks, status aging, bin navigation, selection fitting, and marquee gestures now avoid repeated repaint work.
- Clipboard attribute paste is one atomic undoable edit that preserves transform keyframes, and palette, keyframe, media-caption, ducking, and export-range paths avoid repeated hot-path allocation or scans.
- Renderer frame scratch is retained, effect passes are sampled once and borrowed from resolved scenes, and animated text moves glyph pixels while using numeric atlas identities.
- Media-pool missing status is cached and periodically revalidated instead of issuing a filesystem stat during every project projection.
- Automatic canvas sizing caches the largest used visual-media dimensions until a project mutation invalidates the result.
- Transform-gesture scene partitioning moves resolved layers into below, sprite, and above passes without copying layer slices.
- Track-matte folding walks the stack once and stores each recursive content-stencil pair in one allocation.
- Constant layer styles bypass curve evaluation, and whole-layer and per-character combo effects use one wrapped phase calculation.
- File-backed LUTs begin loading on background workers before scene-layer realization.
- Animated sticker frames use a cumulative delay index with logarithmic lookup.
- Retimed audio derives source bounds from monotonic endpoints and avoids zero-filling scratch that interpolation overwrites.
- Pool thumbnails render from direct single-source scenes that preserve session media and decoder-cache identity.
- Storage layout snapshots observe the current generation without joining the lease queue, so a held lease cannot deadlock a waiting writer.
- History inspection borrows step labels, grouped rollback publishes one revision, structural edits snapshot transitions only on touched tracks, and ducking work scales with detected voice spans.
- Compositor effect passes rewrite persistent uniform slots instead of allocating GPU buffers for animated constants.
- Base compositor layers retain bind groups and uniform storage across frames instead of rebuilding descriptors per draw.
- Decoded NV12 and I420 frames rewrite retained plane textures instead of allocating two or three GPU textures per frame.
- Static RGBA layers retain premultiplied GPU textures by authored-pixel digest and skip repeated upload and binding work.
- Linux zero-copy decode retains Vulkan DMA-BUF imports by underlying surface identity across decoder-pool reuse.
- Export pipelines retain two bounded GPU readback slots and submit the next frame before finishing and encoding the prior frame.
- LUT textures and subtitle glyph atlases evict one inactive least-recently-used entry at a time, and LUT uploads reuse compositor-owned CPU staging.
- Lottie playback retains rasterizer state and recycles evicted frame storage instead of allocating a frame-sized buffer for each rasterization.
- Home recent-project cards cache display text and share paths instead of rebuilding card projections during every draw.
- Timeline lane headers use semantic solo, target, and sync-lock icons instead of letter and symbol placeholders.
- Transition edits clamp to adjacent media bounds, structural undo restores transitions pruned by model edits, and explicit filesystem mutations invalidate media status immediately.
- Test media is stored under `tests/fixtures/`, and legacy assistant conversations with the `user` role load with stable transcript identities.
- Compositor inert-effect checks read a precomputed parameter-offset table instead of scanning descriptor slots per pass.
- Background jobs limit live worker threads to host parallelism by default, and rejected spawns report the limit instead of oversubscribing the machine.
- Transcription leases native decoder threads from one process-wide budget, so concurrent jobs cannot each claim full host parallelism.
- Whisper model downloads are built through the shared HTTP boundary with a registered caller identity.
- Multicam angle nudges, audio sync, and inspector resets draw named glyphs with hover names instead of text placeholders.
- Layer style blur covers 32 texels per render pass through bilinear midpoint sampling, so a maximum-radius shadow or glow costs four passes instead of six.
- The application bar shows the Aria mark, and preview zoom draws the magnifier glyphs instead of a plus and a minus character.

- **A long project no longer sends the assistant every clip on every turn.** The
  description pushed into each prompt was the whole timeline, unconditionally, so
  a two-thousand-clip project spent most of a context window restating clips the
  question was not about, once per turn, and a small local model had nothing left
  to answer with. Up to 300 clips nothing changes. Past that the description
  covers the 300 clips nearest the playhead and carries its own bounds, the
  number of clips it holds, and the number of clips and markers outside them, so
  the assistant can tell the difference between a clip that is not there and a
  clip it was not shown. `describe_project` takes a `from_seconds` and
  `to_seconds` stretch, which is how it reads the rest: a question about an hour
  in is answered by asking for that stretch rather than by guessing. The system
  prompt says all of this, so the behaviour is instructed rather than inferred.

- **A config value the editor cannot use now says so instead of vanishing.**
  Loading was tolerant and silent: `root = "cache"` under `[storage]` is a
  relative path, which resolves against wherever the process was started, so
  the loader dropped it and returned no root at all. The caches went on filling
  under the profile directory while the file said they had moved, and no
  surface anywhere could tell the difference. Every read of an owned key now
  records what it stepped over — the key, the value as the file wrote it, and
  which of four reasons it failed: the wrong kind of value, a name outside the
  set the key accepts, a relative path, or a number out of range. The Caches
  section of Settings lists them, **Copy paths** carries them into a support
  report, and the session still starts on the defaults, because a hand edit
  must not stop the editor opening. The rule is enforced rather than
  remembered: a test reads the loader's own source and fails if a key is read
  in a way that cannot report a refusal.

- **A multicam angle can be moved onto the group's clock, by hand or by its own
  sound.** The offset field was read everywhere it mattered, so a group whose
  cameras did not start together decoded the wrong instant on every angle but the
  first, and nothing in the editor could correct it. Each angle row in the pool
  sidebar now draws that camera's sound across the group's span, with per-frame
  nudge buttons and the offset itself between them, so two cameras out of sync
  look out of sync. **Sync Audio** correlates the peaks already decoded for those
  strips against the first camera's and places every other camera where its sound
  says it belongs, as one undo entry however many moved. The first camera is the
  clock, so syncing twice gives the same answer. A camera whose sound is still
  being read, has none, does not match the take, or repeats is left where it is
  and named: a repeating sound matches at every repeat, so the best match is
  whichever repeat the search reached first, and an offset from a guess moves a
  camera that was already right. The measurement reports how well anything else
  matched alongside how well the winner did, and it was that number which caught
  a test fixture built from a metronome tick: two alignment tests were passing on
  lags thousands of samples wrong.

- **The assistant can render a clip and put the file in its place.** Every other
  verb in the editor was reachable from a prompt except this one, so the one edit
  worth minutes of machine time was the one that had to be clicked. Two tool rows
  now cover the pair: one asks for a render of a clip, the other restores the
  clip a render replaced. A restore is an ordinary edit and lands in the turn's
  history entry like any other. A render is not: the file it swaps in does not
  exist yet and takes longer to write than a turn lasts, so the turn hands it to
  the shell, the Jobs board carries it, and the line the model reads back says
  the render started rather than that the clip was replaced. A render nothing
  would gain is refused before any of that, naming the same reason the menu row
  names: a clip with nothing applied to it would only be re-encoded, a lane bar
  has no picture of its own, and an already rendered clip has to be restored
  first. Which lane bakes to which medium is read where the renderer reads it, so
  the tool refuses exactly what the render would have refused, in the turn rather
  than minutes later. Inside a plan, a render starts once the plan's edits are in
  force: rendering a half-applied sequence would write the wrong picture.
- **A sound clip renders and replaces, like a picture clip.** Render and replace
  refused every clip on an audio lane by name: there was no sink for a file with
  no picture in it, and every platform encoder needs a frame size to open one at
  all. A sound lane now bakes to a WAV of 16-bit PCM, written in Rust, through
  the same mixer the export uses, the same transactional publish, the same
  digest-named home in `<project> Renders`, and the same one-entry history that
  imports the file and swaps it in. Restore unrendered maps back through it, so a
  clip trimmed, moved or split since the render still returns to what it was.
  What the file holds is the clip's own sound: its volume, pan, fades and
  denoise. The lane's fader, mute, solo, sends and routing stay out of it,
  because they are applied again when the file plays back, and a bake that folded
  them in would keep applying them after they were changed. Which lane kinds bake
  to which medium is read off the lane kind rows, so a lane kind that says it is
  neither picture nor sound is refused by name instead of rendered as the wrong
  thing. A sound bake claims no GPU and needs no platform encoder, so it works on
  a machine that cannot export video at all.
- **An effect chain can be saved as a preset, and a saved one can be deleted.**
  The Effects tab drew a row per saved preset and nothing ever saved one, so on a
  fresh install the block was empty and stayed empty however many chains were
  built. **Save as preset** opens a name field over the chain, Enter or **Save**
  writes it, and each preset row now carries a **Delete** next to it. A name that
  is empty or already taken is refused with the field still open and still
  holding what was typed. Both reach the preset file, so the next launch reads
  what the last one saved.
- **A multicam group reaches the frame, and the timeline.** Camera groups, named
  angles and angle cuts were built and switched from the pool sidebar, and the
  renderer answered nothing for a multicam clip: the whole feature ended at the
  model, and the placement command was declared unreachable because a row that
  placed one would have put a black clip on a lane. The resolver now decodes the
  camera the cut list makes live at that tick, read from that camera's own file
  at the group's tick less the camera's sync offset and clamped at its first
  frame, and fits it to the canvas at its own shape, so a group of mixed formats
  is not stretched to whichever camera came first. A **Place** button on each
  group row puts the group on a video lane at the playhead, as long as the last
  frame any of its cameras can still answer for, which the model derives from the
  angles rather than the panel guessing from the first one. Every angle in a
  group is still synced at zero: nothing in the build offsets one camera against
  another, and `docs/deferred.md` says so rather than implying the sync is there.
- **The pool selects several items.** It held one: a single id, written by every
  click. Every surface that acts on more than one had to pretend otherwise, and
  the multicam button was the one that shipped that way, building a group from
  whatever single item was selected or, with none selected, from the entire pool.
  Ctrl+click adds a card or drops it, Shift+click picks the run the list drew
  between the last card clicked outright and this one, and neither places a clip:
  a modified click changes the selection and nothing else. Selected cards are
  drawn selected, the properties block still describes the last one clicked, and
  the selection keeps click order, which is the angle order of a group built from
  it. Opening another folder drops it.
- **Paste To Fit.** The retimed paste the engine has always been able to apply
  had no control anywhere: three thin wrappers around it sat in the editor with
  no caller, so the only way to reach a paste that changes the copy's length was
  the assistant. The Edit menu has a Paste To Fit row that drops the copy into
  the marked range and retimes it to fill the range exactly, landing at the In
  point rather than at the playhead, because the range is the destination. With
  no range marked the row says so and changes nothing, rather than pasting at the
  playhead at some other length.
- **The mask tracker has a control, and it does not freeze the window.** The pass
  that follows a mask across frames shipped as a library call nothing reached: a
  whole-shot walk is a decode and a composite per frame, and running that on the
  thread that draws would have parked the editor for the length of the shot. The
  walk is now a function that reads pictures, asks whether it was cancelled and
  reports how far it got, so it runs on a job thread over a copy of the project,
  reading frames from a renderer of its own like every other background render.
  "Track mask forward from the playhead" in the palette follows the rest of the
  clip while the window keeps drawing and stays scrubbable, the jobs board counts
  frames against the total and offers Cancel, and the keyframes come back to the
  window as one undo entry. A track that is stopped keeps the frames it already
  followed, and a track that loses its feature says which frame lost it.
- **Every strip has a meter, and it is that lane's level.** The mixer drew a
  fader and nothing to mix against: one peak pair was published, for the
  programme output, so a strip had no level of its own and a bus had none at all
  even though the fold already held every lane's samples and threw them away.
  Each lane now reads its own peak, taken post-fader inside the fold, published
  once per mixed block into a table sized when playback starts, so metering
  costs no allocation per block and the device callback never touches it. The
  bars fall on the repaint clock like the programme meter, a lane the mix is not
  carrying reads silence rather than the master's level, and a lane that reaches
  full scale turns red.
- **The assistant can route the mix.** The mixer shipped as an operator surface
  with nothing behind it for a prompt: a lane's fader, its output and its sends
  were five commands the assistant had no tool for, so "send the dialogue to a
  reverb bus" had nothing to call. It now adds a submix lane (`submix` joined
  the lane kinds `add_track` takes), rides a lane's fader, points an output at a
  bus, and adds, changes or drops a send. `describe_project` carries each lane's
  fader, the bus it feeds, and its sends in the order `set_send` addresses them,
  so an index is something the model can read rather than guess. Every rule is
  the model's own — the level bound and the loop check are the same calls the
  setter makes — and each refusal names the way forward: which lanes have a
  fader, which buses exist, which send index the lane holds.
- **Every grade slider the picture has, the assistant can set.** The adjustment
  arguments were a hand-written list beside the model's own, and it had drifted:
  `set_clip_adjustments` took eleven sliders while a clip carries twenty-three,
  so the lift, gamma and gain wheels could be keyframed one axis at a time but
  never set in one call, and "warm the highlights" had nothing to reach for. The
  tool now takes one argument per slider, each validated against its own row's
  range, and an argument this build does not know is refused by name instead of
  being ignored — a misspelled slider used to lower to a call that changed
  nothing and read back as "adjustments unchanged".
- **One row declares an adjustment, everywhere.** A slider was six declarations
  — a struct field with two serde attributes, a `Default` line, a slider table
  row, a parameter-axis arm, a range group, a catalog row, plus the assistant's
  argument and its lowering — and nothing tied them together, which is how
  twelve sliders ended up with no tool argument and how the wheels ended up
  absent from a projection that claimed to mirror the grade field for field. The
  row list in `look/adjust_rows.rs` is now the only place a slider is written
  down: the struct, its neutral, its save format, its validated range, its
  parameter axis, its catalog row and the assistant's argument all expand from
  it. A row with no axis does not compile, a save carries only the sliders that
  moved, and a project written before a slider existed reads it at neutral.
- **A filter preset states any part of the grade, and a typo in one is
  reported.** A recipe in `filters.toml` was its own hand-written list of the
  same sliders, read with `#[serde(default)]`: a key the build did not know was
  skipped, so a misspelled offset shipped as a preset that quietly did nothing.
  The offsets now expand from the adjustment row list, so a slider added there
  is a catalog key by construction, and an unknown key refuses the file whole
  while naming every key a recipe may carry. The documented key table is pinned
  to the same rows, so it cannot describe yesterday's keys.
- **A command says which control an operator reaches it through.** Reach was a
  yes or no, and only the "no" was checked: a palette row could not send a
  command declared unreachable, but a command declared reachable with no control
  anywhere passed every test. Three shipped that way, and one still has no
  control at all. A row now names its surface, so a command cannot claim to be
  reachable without someone writing down where the control is — which is the
  moment its absence is noticed — and a command with no control yet says so and
  names the work that blocks it instead of pretending. The editor measures the
  claim where it can: every palette row, every menu row and every plain chord is
  driven, what each dispatches is observed at the command boundary rather than
  read out of the history list (so saving and exporting count, though they write
  no history), a command something sends may not be declared unreachable, and a
  row that names the palette or a chord has to be sent by one.
- **A mixer, and submix buses to mix into.** Every audio lane had a per-clip
  volume and nothing else: no lane fader, no way to hold a group of lanes under
  one hand, and no send. A lane now names its output — the master, or a submix
  bus — and may copy itself into any bus at its own level, pre-fader or post.
  `Submix` is a lane kind with its own zone below the audio floor, so a bus
  cannot hold media and cannot be moved out of the mixer; membership of the
  mixer is the lane catalog's own `is_bus`/`carries_audio` answer rather than a
  list. The model owns the graph and hands the mixer a feed order, so the
  mixdown folds forward with no lookahead and a lane whose signal cannot arrive
  at the master is never decoded at all. Refused rather than guessed: a level
  outside 0 to +12 dB, a route that would loop (checked at the setter and again
  when a hand-edited project is read, which is why opening the mix now reports
  the lane a cycle runs through instead of picking a fold order), an output that
  is not a bus, and a second send to a bus a lane already sends to. Deleting a
  bus returns its feeders to the master and one undo restores the lane and the
  routes that pointed at it. Shift+6 or the View menu opens the mixer: one strip
  per lane in fold order, a fader read in decibels with a detent at unity, a
  chip per output, and a chip per bus that cycles a send off → post → pre.
  Riding a fader is one undo entry, not one per pixel.
- **Render and replace, and the way back.** A clip with a deep effect stack was
  re-rendered on every frame of every playback with no way to spend that cost
  once. Rendering one now encodes it to a file that stands in for it, kept in
  `<project> Renders` beside the project and named after a digest of what it
  shows, so an unchanged clip adopts the file that is already there and a
  changed one renders a new file rather than overwriting the one the undo stack
  points at. The intermediate is the clip's timeline span rendered one-to-one at
  the sequence rate, which is what makes the return exact: Restore unrendered
  maps a head trim, a move or a split made since through the original's own
  retiming. Refused rather than approximated: a lane bar (it has no picture of
  its own), an audio clip, a plain media clip with nothing applied to it (the
  render would be a re-encode), and restoring a clip that was retimed after it
  was rendered.
- **A lane bar's look is animated, in sequence time.** An adjustment, filter or
  effect bar carries an effect stack, a grade and a LUT, and its parameters now
  take keyframes on the same inspector routes a clip's do. A bar has no content
  of its own — it reprocesses whatever is composited beneath it — so its
  keyframes are addressed in sequence ticks rather than clip-local ones: the
  generator catalog declares the convention per row (`ParamTime`), one clip
  method converts in both directions, and every read and write goes through it.
  Dragging or splitting a bar moves the bar, not the moment its look happens,
  while a clip that paints its own pixels still carries its animation with it.
- **Nested sequences are reachable from the editor.** `CreateNestedSequence`,
  `OpenSequence` and `PlaceNestedSequence` had shipped as operator commands
  since sequences existed, and no palette row, menu row or panel sent any of
  them: a nest could be built by a script and never opened. The Sequence menu
  and the palette now nest the selection, open the selected nest, leave it, and
  place another copy of it at the playhead, and the status strip carries a chip
  naming the sequence that is open — click it to leave. An edit inside a nest
  lands in the nest, which is what the projection and the breadcrumb now both
  read.
- **The colour wheels are reachable by name.** Lift, gamma and gain shipped in
  the model and on the wheel widget, but the twelve of them had no wire member,
  so an assistant could set exposure or saturation and not touch a wheel, and a
  described clip never mentioned one. `adjust_lift_r` through `adjust_gain_master`
  are tool parameters now (tool schema 45), and the grade description reads each
  slider against its own neutral, so gamma and gain at 1.0 read as untouched
  instead of as set.
- **One table describes every grade slider.** Which sliders a colour grade has,
  what each one's neutral is and what range it accepts were three hand-written
  sweeps that had already drifted: `ColorAdjustments::SLIDERS` carries name,
  neutral, range and accessors, and neutrality checking, validation, the
  assistant's description and the parity test all read it. A field added to the
  grade without a row does not compile.
- **A script reaches the five capabilities only the editor could reach.** The
  Python binding could build a timeline but not annotate it, ramp it, repoint it,
  reuse it, or keep two clips together: markers, speed curves, relink, templates
  and clip links were editor-only, and the API reference listed all five as not
  shipped. Scripts now write `p.add_marker(t, name, color=…)` and read
  `p.markers`; `clip.set_speed_curve([(0.0, 1.0), (0.5, 2.0)])` or a preset id,
  with `clip.speed_curve` and `clip.speed_preset` reading back;
  `clip.set_media(other, start=…)` to relink; `clip.replaceable = aria.Slot(…)`
  plus `p.save_template(path, name, bundle=…)`, `aria.Template.load/install`,
  `t.slots`, `t.texts` and `t.fill(picks)`; and `clip.link_with([other])`,
  `clip.unlink()`, `clip.linked`. `aria.speed_presets()` and
  `aria.marker_colors()` list what can be named.

- **Clip linkage has one owner.** Which clips form a group, and which groups a
  dissolve touches, lived only inside the engine's undoable action, so nothing
  outside the editor could link two clips without reimplementing the rules.
  `Project::link_plan`, `unlink_plan`, `set_clip_links`, `link_clips`,
  `unlink_clips` and `linked_clips` now own them in the model, and the engine
  action keeps what is actually its own: capturing the inverse for undo.

- **An effect chain costs one bind group per distinct pass, not one per clip.**
  Every effect pass of every frame built a uniform buffer and a bind group and
  dropped both at submit — twelve layers with two effects each meant 24 of each,
  per frame, for descriptors identical to the frame before. A pass is now keyed
  by what defines it (pipeline, input texture, constants), so layers sharing an
  effect share one binding while each still draws its own pixels. On a 12-layer
  scene at 512×288, 24 passes a frame, a frame's median falls from 0.78 ms to
  0.57 ms and a repeated frame allocates nothing; the counts are the contract and
  the clock is reported, measured by alternating the two modes one frame at a
  time so a busy machine cannot flatter either. The Render cost panel and the
  status line report the passes a frame drew and how many of them had to be
  built, so a chain that made the preview dear says so.

- **One edit's cost is measured, and it is the edit's, not the project's.**
  `cargo bench -p aria-engine --bench command_apply` applies commands to 8 tracks
  of 480 clips: a transform takes 216 ns there and 183 ns on a project of one
  clip, and the 33 ns between them is the lookup of one clip among 480. Undo and
  redo of that transform cost 493 ns together, a split with its undo 686 ns, and
  a move to another track and back 764 ns. A project is copied per export and per
  assistant turn, where a snapshot is the point; no command copies one.

- **A stopped assistant turn says what it undid.** The loop rolls a cancelled
  turn back — an interrupted plan half-applied is worse than none — but the panel
  kept the action rows it had printed and said only "stopped", leaving an
  operator to discover by looking that the split they had just watched was gone.
  A stopped turn that had applied edits now prints "N edit(s) undone" in the
  transcript and the status line, and a turn that edited nothing still says only
  that it stopped. Stop itself is read between deltas, so it lands mid-answer.
  The manual said the opposite of the code and now matches it.

- **Startup is measured, and one third as expensive.** The editor brought up
  three GPU devices before its window opened — the compositor plugin's, the
  render plugin's and the engine's — each with its own driver instance, adapter
  and full pipeline set. Exactly one was ever rendered with; the other two were
  registered and no caller read them. One device is created now and shared, so a
  cold start fell from 583 ms to 222 ms and the process holds a third of the
  video memory it used to. `cargo test -p aria-desktop first_interactive_frame`
  measures a cold and a warm start against the boot cover's ceiling and prints
  the dearest mounts; Settings shows what each plugin cost to mount, next to the
  plugin.

- **The profile directory is a parameter.** `ARIA_DATA_DIR` moved the session's
  settings, caches and recovery file, and nothing else: the plugin mount resolved
  the default directory itself, so external plugins, catalog overrides, speech
  models and the cloud cache all came from the standard profile no matter what
  the process was pointed at. The directory is resolved once and passed in, and
  reaches exactly one plugin — storage owns the root and everything that needs a
  directory asks it.

- **The scopes are compared against committed pictures, and a failure says what
  moved.** Nothing in the workspace compared a rendered surface to a reference,
  so a graticule off by one row or a trace drawn without its gamma curve was a
  regression no gate could see. The waveform, vectorscope and histogram are
  painted on the CPU from a fixed synthetic frame and checked pixel for pixel
  against `crates/aria-ui/references/*.png`. A difference reports the bounding
  box of everything that changed plus a coarse occupancy map of it, because "the
  picture changed" is what the reader already knew: a shifted graticule reads as
  a full-width row, a moved trace as a blob, a repainted canvas as solid, and
  all three can be the same pixel count. No file is written when a check fails.
  A reference is created or replaced only with `ARIA_BLESS_SNAPSHOTS=1`, and the
  set on disk has to match the set the tests claim, so blessing a picture nobody
  compares fails the gate.

- **Playback is measured against the rate it was asked for, and says so on
  screen.** Frame cost was visible one frame at a time and against a fixed
  150 ms slow-frame threshold, so a change that doubled per-frame cost stayed
  invisible as long as it stayed under it. `aria_render::PlaybackMeter` counts a
  frame as late when it exceeds the sequence's own frame budget and reports the
  achieved rate, the late share, and the worst frame; the preview panel that
  breaks a frame into decode, raster and composite now carries that line under
  the split, green while the run holds its rate and amber once it does not. The
  target follows the shuttle, so 4x is measured against four frames per frame
  interval rather than one, reverse is measured against the same rate as
  forward, and a new run never inherits the previous one's drops. A measured run
  over a known sequence gates the preview path — relaxed, not skipped, on a host
  that rasterizes on the CPU — and holds 5517 of 24 fps with no late frame on a
  Ryzen 9 9950X with an RTX 5090. Scrub seek is measured too: 151 ms to open a
  file and land mid-clip, 0.84 ms per seek through a decoder already open.
- **An export writes what was asked for, from anywhere.** The export sheet's
  choices reached only the background job: File > Export while a render was
  blocking wrote the project's own canvas, rate and codec whatever the sheet
  said, and the command line kept its own copy of the encoder's refusals.
  `ProjectCommand::Export` now carries an `ExportFormat` — every field optional,
  so an older `{"path": …}` payload still means the project's own output — and
  one resolver turns that request into what will be written, refusing a codec,
  bitrate, size or rate this machine will not honour by name before a file
  exists. The app, the command line and the mobile FFI all go through it.
- **The Linux build installs like an application.** The tarball shipped a bare
  binary: no launcher, no icon, and no file type, so Aria appeared in no menu
  and a project could not be opened from a file manager. It now carries a
  desktop entry, the app icon in the hicolor theme, the
  `application/x-aria-project` type — matched by the `*.aria` glob and by the
  schema string inside the file, so an extensionless copy still resolves — and
  an `install.sh` that copies them into `~/.local` or any `--prefix`, rewrites
  the launcher to the absolute installed path, refreshes the desktop, MIME and
  icon caches, and reverses itself with `--uninstall`.
- **An export that would write nothing is refused, once, in the render path.**
  Exporting an empty sequence produced a file: zero frames, a valid header, and
  no way to tell it apart from a successful export until it was opened. The
  frame span is now checked before a staging directory is created or an encoder
  is opened, and the refusal carries a typed reason — the sequence has no
  content, or the marked range covers no frame — so each one names the control
  that fixes it. The two shells had each grown their own copy of the rule; both
  now read it from `aria_render::export_frame_count`, which is also what an
  export sheet uses as its pre-flight.
- **A pool tile is decoded once, ever.** The library's thumbnails lived in
  memory, so every launch re-decoded a frame of every file in the project and a
  large pool spent its first seconds drawing grey. The resting tile picture now
  lands on disk under the library-thumbnails cache — one versioned file per
  source per tile size, keyed by the file's path, length and modification time,
  written through a temporary sibling so no reader sees a partial picture, and
  refused on a size or header mismatch rather than drawn. The cache has a
  `[storage.paths]` key of its own, so it can be moved, redirected and cleared
  like every other disk cache.
- **An effect's parameters have one owner.** The compositor kept its own list of
  every effect's parameter names, order and widths beside the catalog the
  inspector browses and the engine validates against, and a cross-crate test in
  `aria-engine` existed to notice when the two drifted. The render slots are now
  derived from `aria_models::effect_catalog` — scalar one float, vec2 two, colour
  four, in catalog order — so there is nothing left to drift, and the registry row
  carries only what the GPU owns: which shaders run and which parameter makes the
  pass inert. Because the catalog is a file a user can replace, each effect is
  checked against its own WGSL before it becomes renderable: an entry whose
  parameters the shader does not read, or that overflows the shader's uniform, is
  refused with a reason and shows as passthrough rather than being fed slots the
  shader disagrees about.
- **A chord can start work.** A key binding was one signature — state in,
  nothing out — so no keystroke could queue a job: ducking, the detectors,
  transcription and loudness were reachable from their own panel and from the
  palette and from nowhere on the keyboard. A binding now says whether it needs
  the window, the shell dispatches both kinds, and Shift+Alt+D ducks the
  selection at the amount Alt+D cycles.
- **Scrubbing stops the frame it has already left.** A drag across the timeline
  asked for a picture at every playhead it passed, and each decode ran to
  completion even after the operator had scrubbed past it, so the preview
  arrived a frame or two behind the hand and the last frame of a long drag
  queued behind every frame before it. A newer request now cancels the decode in
  flight: the renderer checks between layers and once more before it submits,
  which are the two places it can stop without abandoning work already on the
  GPU. A cancelled frame is not a failure and never reaches the status strip,
  the engine is usable immediately afterwards, and the next frame renders the
  same pixels it always did.
- **A frame that changed nothing costs nothing.** Every poll rebuilt the whole
  timeline projection — tracks, clips, media, markers — even when the project,
  the playhead and the selection were all where the last frame left them, so an
  open menu or a hovering pointer paid for a full rebuild sixty times a second.
  The projection is now memoised on exactly the values it reads, including the
  proxy table and the three history flags, so an idle frame reuses the store it
  drew last time and a real change still reprojects on the frame it happens.
- **An animation costs the view it runs in.** The motion clock notified every
  view in the window on every tick, so one fading badge repainted the timeline
  under it. A tick now notifies only the views holding a lease on the clock, and
  the clock parks itself when the last lease expires instead of ticking over an
  idle window.
- **Relink All checks the file before it binds it.** It matched on file name
  alone, so a folder holding two `C0001.MP4` — two cameras, or an export beside
  its proxy — got the first one, reported "Relinked", and left the wrong footage
  in the timeline under a row that had stopped asking to be repaired. It now
  searches the chosen folder and four levels below it, and checks every
  same-named file against what the project recorded for that row — frame size,
  duration, audio presence — using the same probe that adopts the file, so the
  match and the bind cannot disagree. A row is repaired only when exactly one
  file agrees. Every row that stays missing says why on itself: not in that
  folder, several files match, the file there is a different source and which
  fact says so, or it would not open.
- **The shuttle rate is on screen, and the sound tells the truth.** J and L moved
  the picture off 1x while nothing said so and the mixer went on playing forward
  at 1x underneath it — and because a running audio clock is what drives the
  playhead, reverse did not move backwards at all. The rate now appears under the
  preview with a muted speaker beside it whenever the mixer is not following the
  transport, and clicking it returns to 1x with sound. Above 1x and in reverse the
  audio path goes quiet: the mixer plays a sequence forward at its own rate, so
  there is no sound at those speeds that matches the picture, and mixing the wrong
  one is worse than mixing none. Whether the mixer can follow is one predicate the
  whole transport reads, so a command applied mid-playback cannot restart audio at
  a speed the operator is not watching. A stop — K, space, or running out of
  sequence — leaves the transport at 1x.
- **Stop, wherever the work is.** A running job could only be stopped from the
  Jobs board, so the operator who started an analysis from the panel that shows
  its progress had to go and find it somewhere else. Every surface that shows
  work in flight now offers the same control in the same place: the panel row,
  the board, and the export sheet beside the render it started. Whether a job can
  be stopped is the job's own rule rather than each surface's, so a control is
  never offered for a request the registry would refuse, and never withheld from
  one it would accept. The control's identity carries its surface, because the
  board opens over the panel underneath it and one shared element id is a pointer
  lighting the wrong row.
- **A liveness probe that ends.** The AI providers bounded the connect and
  nothing else, so an endpoint that accepted the socket and then said nothing —
  a misconfigured gateway, a tunnel with no listener behind it — left Settings
  spinning until the operating system gave up on the connection. Both transports
  now bound the probe itself at twenty seconds and report the host as
  unreachable. A chat request is deliberately not bounded this way: a long answer
  is a long answer.
- **A duration scale, and no way around it.** Every animation used to carry its
  own number: sixteen specs spanning eleven unrelated durations, plus raw
  millisecond values written into panels — a caret at 1060ms, a list stagger at
  18ms, a playhead borrowing another spec's length. There is now one scale of
  ten named steps, from `FRAME` (16 ms) to `BREATH` (2400 ms), and a spec is one
  step plus one named curve. Neither can be written as a number: the scale's
  constructor and the curve's constructor are both private, so a duration
  nobody chose does not compile. A stagger is `Beat::times(n)`, which keeps a
  list on the same grid. Adding a length is one `pub const` and one row in
  `SCALE`; the tests fail if the scale is out of order, if a step is unlisted,
  or if the documentation does not give a step its length.
- **Every curve is a named token.** `motion::CURVES` is the list, and the
  gallery draws a specimen of each row without being told about it, so a curve
  added to the catalog gets a plot and coverage for free. Each catalog row names
  the curve its spec rides, and a test holds that the name and the spec agree —
  a curve swapped under a spec is a different animation at the same length,
  which no duration test can see.
- **The product's own mark, in the window and on the icon.** Home's rail and the
  boot splash drew a placeholder built from thirty-four pulsing cells because the
  shipped art had never been given an asset source. Both draw
  `icons::brand_mark` now — the clapperboard mark — served through the same
  registry as every glyph, and `scripts/package-macos.sh` and the Xcode asset
  catalog build the app icon from that same file, so the mark in the window and
  the icon in the dock cannot drift apart. The macOS packager was pointing at the
  upstream project's logo.
- **A loader's key decides its phase.** Four loaders took an id and ignored it,
  so two panels waiting at once pulsed in lockstep and read as one animation
  drawn twice. Each offsets its phase by a hash of the key it already carries,
  which is stable frame to frame, so a row keeps its phase and two rows keep
  apart.
- **The media pool draws three ways.** Chips beside the sort order switch the
  pool between Tiles (picture over facts, one item per row), List (one line per
  item, no picture, so a pool of hundreds asks the preview store for nothing) and
  Icons (a wrapped 96px grid of pictures under their names). The choice is saved
  in `ui-settings.json`, so it is a habit rather than something to set again in
  the next project. One tile builder reads the view's own answers about what it
  draws, so clicking, dragging, scrubbing, the context menu and the label
  swatches exist once for all three views, and a fourth view is a variant plus
  its answers.
- **Every chip in the app is the same control.** The media tabs, the stock kinds,
  the analysis kind and confidence filters, the caption thread and commit rows,
  the generator kinds and the pool's own sort and view rows were six copies of
  one fifty-line chip, three of which swapped colour on hover instead of easing
  into it. They are all drawn by `components::chip` now, in one of two sizes: a
  panel's primary filter row, or a secondary row beside a title or a count. A
  caller keeps only its own click.
- **The editor arranges itself.** Four shipped workspaces (Editing, Colour,
  Audio, Effects) sit on the Window menu, in the palette, and in Settings >
  Workspaces, and each one restores the rail and inspector widths, whether either
  is collapsed, the timeline height, the scope strip, the pane the keyboard is
  aimed at, and the route. Save Workspace keeps the arrangement on screen as a
  Custom row that survives a relaunch, and Forget takes it off the menu. Which
  workspace is in force is measured rather than stored, so dragging a divider
  simply leaves none in force instead of leaving a stale name behind, and a saved
  arrangement from a build with different pane limits is pulled inside them
  instead of being refused. The scope strip is opened through the same toggle the
  View menu uses, so switching to Colour measures the frame on screen rather than
  showing an empty scope. The menu row family is generated from the workspace
  table: a fifth arrangement is one row of data and no new code.
- **Every key can be rebound.** Settings > Keys lists every binding the registry
  holds — the shipped chords and a plugin's — with the chord it answers to now.
  Clicking a row arms it and the next keystroke is its chord, so a chord is never
  typed as text and never spelled wrong; Esc cancels, Clear leaves a binding with
  no key, Revert restores one row and Reset all restores every row. A chord
  another binding on the same surface already answers is refused with the holder
  named at the moment the key is pressed, rather than shadowing a binding that
  still works. Rebindings live in `ui-settings.json` under `keys`, keyed by
  binding id and spelled the way the list spells them, so a saved key this build
  no longer has is dropped on load instead of being kept as a chord that does
  nothing. The registry resolves through the same table it draws from, so a
  rebound key dispatches immediately and its control's tooltip names the new
  chord; the refusals the registry has always recorded for a plugin whose chord
  was taken are now listed with them.
- **Every agent tool is a registry row with a test.** The model-visible tool
  surface is split into eleven domain groups (tracks, clips, timeline, effects,
  transitions, animation, retime, audio, look, markers, project), and the
  registry is the only enumeration of it: the specs offered to a provider, the
  tool-call dispatch, and the tests all read the same rows, so the two
  hand-maintained counts that used to lock the surface are gone. Each group
  drives its rows through validation and id remapping with valid and invalid
  arguments, asserting the engine command a call lowers to and the refusal an
  invalid one earns. A row with no case fails its group's test and a
  `WireCommand` variant with no row fails the registry census, both enumerated
  from source at run time, so a tool cannot ship without a test. The tool array
  is now ordered by group; names, descriptions, and argument schemas are
  unchanged.
- **Every layer topology has a pixel proof.** Each of the eleven `LayerSource`
  topologies the renderer ships (media, still, sticker, lottie, text, solid,
  canvas pass, shape, path shape, transition, matte) is rendered alone onto an
  empty canvas and read back, so an arm that realizes a layer which draws
  nothing fails instead of compiling. The three the sub-scene dispatch refuses
  are checked for a refusal that names the topology. The variant list comes from
  the enum declaration at run time, so a twelfth topology fails the sweep until
  it has a sample, a pixel expectation, and a recorded decision about nesting.
- **Every persisted setting carries a version.** Each table in
  `~/.aria/config.toml` is a shape with a schema version recorded in a
  `[schema]` table, so a stale file is migrated to the current shape or refused
  rather than served as if it were current. A refused version fails the load
  naming the table and both versions, and a save leaves such a file in place
  instead of rewriting a meaning it cannot read. Absence stays tolerant: a
  missing key keeps its default, so a file an older build wrote still opens.
  Closing the gap fixed a round trip that was already wrong: a local AI endpoint
  saved with a base URL and a model came back as the freeform endpoint, because
  version 1 inferred `source` from those two keys and version 2 records it.
- **Every command states how a person reaches it.** `EditCommand::reach()` and
  `ProjectCommand::reach()` are exhaustive, so a new command cannot compile
  until someone declares whether an operator can ask for it, and a command with
  no operator surface carries the reason in place instead of being found later
  by diffing identifiers across crates. One command is declared internal:
  `AddClip`, because a drop resolves through `OverwriteClip` or `RippleInsert`,
  which also decide what happens to what is already there. The command census
  that makes this checkable — one constructed sample of every variant — moved
  from a test fixture into the library, so the editor and the wire lock read the
  same list, and the editor now measures which commands its palette actually
  sends rather than asserting it by name.
- **The assistant reaches every property the model declares.** A test walks
  `LookParam::ALL`, `StyleParam::ALL`, `TextParam::ALL`, and `ShapeParam::ALL`
  and requires each variant to have a wire member under the same serde name that
  lowers back to exactly that variant, so a property added to the model turns
  the suite red until the tool schema can address it.
- **Every animatable property is animatable.** The keyframe strip offered six
  properties — position, scale, rotation, opacity, volume, pan — while the model
  declared fifty-eight, because the editor carried its own table and three
  fallback arms that silently swallowed the rest. Properties now come from one
  registry, `aria_models::clip_param_catalog()`, which carries each property's
  id, label, value kind, scope, range, and the route to the value it reads. The
  route is also the availability rule, so a property whose block is missing is
  neither offered nor readable, and the strip lists exactly what the selected
  clip can animate: colour grade, mask geometry, chroma tolerance, matte
  softness, layer shadow, glow, outline and background, text stroke, shadow and
  card, and shape fill, stroke and geometry, alongside the six it had.
  `param_enum!` declares each property enum together with its census, so a new
  property appears in the conformance suite by construction rather than by
  someone remembering to add it.
- **A clip property declares whether it is animatable at the enum.**
  `ClipParam::class()` returns, for each variant, the catalog id it addresses,
  the property axis it delegates to, or the reason the registry holds no row
  for it. Two variants carry a reason: a speed ramp is keyframed on its own
  normalized tick scale, and an effect slot is declared per effect id. That
  rule used to be a match inside a test, beside a hand-written list of nine
  variants that a tenth would not have joined. A new variant now fails to
  compile until it is classified, and `ClipParam::CENSUS` is emitted from the
  same variant list as the enum, so the conformance sweep covers every variant.
- **The timeline strip marks every keyframe, not six properties' worth.** The
  clip projection unioned keyframe ticks from position, scale, rotation,
  opacity, volume and pan by hand, so a keyframe on a mask feather, a grade
  wheel, a text style or a shape corner left no mark on the clip and looked
  unset. The union is derived from the property registry, which already knows
  which properties the clip can reach.
- **A palette row that sends nothing fails the suite.** Every row in the command
  palette is declared as editing, not editing, or opening a file dialog, and the
  declaration is checked against the live table in both directions and against
  what each row actually writes to history. A row added without a decision turns
  the suite red instead of shipping as a dead control. The check also found that
  three rows call the blocking file-dialog API on the drawing thread, which
  stops the window redrawing until a person answers it.
- **Effect parameters are keyframable from the strip.** An effect's parameters
  were animatable in the model and invisible to every surface that enumerates
  properties, because the registry described only the properties every clip
  declares. `clip_params_for(clip)` now returns those rows plus one per effect
  parameter, in effect order, so two instances of one effect are two addressable
  rows and a parameter nobody has written yet reads the value the renderer is
  using rather than zero. This is also how a plugin ships an animatable
  property: it contributes an effect, and its parameters reach the keyframe
  strip by the same route as a shipped effect's.
- **The menu bar is tested.** Seventy-nine menu rows had no coverage at all,
  because each takes a `gpui::Context` that only a running window could build.
  `gpui`'s `test-support` feature is a dev-dependency now, so a headless app
  drives every row: each is declared as editing, not editing, or opening a
  blocking dialog, the declaration is checked against the live menus in both
  directions, and the history each row writes is compared with what it claims. A
  row added without a decision turns the suite red. Windowed palette rows are
  driven the same way and must queue a job or refuse in words; doing neither is
  a dead control.
- **Track mattes.** A clip can be cut by the layer directly above it, reading
  that layer's alpha or its luma, with an invert toggle and a softness that
  blurs the matte edge instead of lowering its contrast. Resolve folds the
  stencil layer and the content layer into one canvas-sized pass and the
  compositor renders both offscreen before cutting, so a matte composes like a
  transition rather than like a per-clip mask. Softness is animatable through
  `LookParam::MatteSoftness`. A matted clip with nothing above it draws
  unmatted, a stencil layer's own matte request is dropped rather than chained,
  and the inspector reports which of the two the operator just got. Set one from
  the inspector, **Sequence → Track Matte from Luma Above**, or the palette.
- **Adjustment layers.** A lane bar that grades and filters everything below it:
  its effect stack, its colour grade, and its LUT all resolve as one canvas-wide
  pass, so one bar carries a look across every clip under it instead of the same
  settings being copied clip by clip. Add one from **Sequence → Add Adjustment
  Layer** or the palette; it covers the marked range, else the selection, else
  the whole sequence, and a layer over a range already taken opens a lane of its
  own rather than failing on the overlap. The lane kind and the generator have
  been in the project format and the resolver since the look work, with nothing
  able to create one; effect and filter lane bars also dropped half of what they
  carried — an effect lane ignored its grade, an adjustment or filter lane
  ignored its effect stack.
- **Scopes in the preview.** A waveform monitor, a vectorscope, and an RGB
  histogram, measured from the frame on screen and remeasured every time the
  playhead moves, so exposure and saturation are numbers instead of an
  impression. Open the strip from the preview toolbar, the View menu, or the
  palette. Measurement is bounded: a frame over the sample budget is read at a
  fixed stride rather than in full, so a 4K sequence scrubs at the same cost as
  a 1080p one, and a closed strip measures nothing at all. Transparent pixels
  are left out, so padding around the picture cannot report an exposure the
  picture does not have.
- **A features chapter, and an index for the documents.** `docs/features.md`
  states what the editor does and what it does not, surface by surface, and
  `docs/README.md` says which document owns which claim and which ones describe
  intent rather than shipped behaviour.
- **The assistant sends prompts.** Type in the panel's prompt box and press Enter
  or **Send**: the turn runs as a job, its answer streams into the transcript as
  it arrives, and each edit lands in the project as the model makes it — visible
  on the timeline during the answer and undoable with Ctrl+Z, not applied to a
  copy. **Stop** ends a turn and keeps the edits already made. Up and Down walk
  back through prompts already sent, **Clear** forgets the conversation, and the
  next prompt remembers the last one exactly as the model saw it. Every reason a
  prompt cannot be sent — an empty box, a turn already running, no provider — is
  answered before the request costs anything, and only the last of them opens
  Settings. The panel previously had a Send button that sent a canned sentence
  and opened Settings whatever was typed.
- **Conversations survive closing the app.** Each project keeps its own
  conversation — the transcript you can read and the history the model was
  given — written beside the app's other session data rather than into the
  project file, which is a document you send to somebody. Reopening a project
  brings its conversation back so the next prompt continues where the last one
  stopped; opening a different project, or starting a new one, starts a new
  conversation instead of carrying stale rows into it. **Clear** deletes the
  saved copy at once, an unsaved project keeps its conversation in memory until
  the first save, and a file written by another version is ignored rather than
  half-read and fed to a model as fact. Screenshots are not kept, because the
  assistant already drops them from history before every request.
- **Skills can be held back for one conversation.** The assistant panel lists the
  skills a prompt would carry and each one can be switched off without
  uninstalling it; the list is read when you press Send, so editing a rule or a
  skill on disk takes effect on the next prompt with no restart.
- **Plan first, then apply.** With **Plan only** on, a turn reports the edits it
  would make and changes nothing. **Apply** replays exactly that plan as one undo
  entry — each command re-checked against the project as it now stands — and if
  any step no longer fits, the whole plan is rolled back and the failing step is
  named. **Discard** drops it.
- **The conversation can be copied.** Copy one bubble, or the whole conversation
  with speakers, out of the assistant panel. Long conversations scroll and follow
  the answer as it streams.
- **The assistant can look, and can act outside the timeline.** Besides the
  validated edits it always had, a prompt is now offered app tools: render the
  timeline at a moment and *look* at the frame before changing it, read a media
  file's real frame rate, length, and codecs from the file, move the playhead,
  save the project, and empty a cache. Each call appears in the transcript as it
  happens. **Autonomy** finally decides something: with `ask`, a destructive call
  stops and the transcript offers **Allow** and **Deny** while the turn waits —
  Deny is reported to the model, which carries on — and **Stop** denies it. With
  `full` it does not ask. Settings' AI section counts both vocabularies from the
  tables themselves, so it cannot advertise a capability the build does not have.
- **Pitch lock does something.** A retimed clip now holds its original pitch:
  the sound is stretched in time (WSOLA overlap-add, aligned so a held note does
  not warble) and the pitch stays where it was, in preview and in export.
  Clearing the lock mixes varispeed, where pitch follows rate. The toggle was
  stored, exported, and on by default while the mixer was varispeed only, so it
  said the opposite of what the file did. An untimed clip is untouched, and a
  stretched export is identical whatever block size it was rendered in.
- **Every catalog effect and transition renders.** Sharpen, Glitch, Chromatic
  Aberration, Film Grain, Glow, Zoom Blur and Mirror each got their own pass —
  they were listed, tunable, saved and exported while doing nothing. Dip to
  Black, Dip to White, the three remaining wipes and Slide likewise stopped
  borrowing the crossfade: a dip passes through one opaque frame, a wipe has a
  hard edge from its own side, and a slide moves both frames. Glitch and grain
  are seeded, so a scrub back and forth renders the same frame.
- **The decoder says what it can do before you open a file.**
  `decoder_capability()` reports the backend and whether it hands back GPU
  surfaces or CPU planes, so a caller picks a mode instead of opening a file and
  reading a refusal. Android reports CPU planes only and refuses a GPU open up
  front rather than failing part way through a decode. On Linux, a DMA-BUF whose
  modifier cannot be imported now names that modifier and its vendor once per
  session, so a machine decoding on the CPU says why.
- **A real command line.** `aria` assembles a project from a folder of media
  (`aria new rough.aria takes/*.mp4 --fps 23.976`), renders one instant to a PNG
  (`aria frame`), encodes a span (`aria export --at 0 --to 30 --codec h265`),
  prints a project's canvas, pool, offline media, and lanes (`aria inspect
  --clips`), and runs the beat, silence, loudness, and shot detectors over a
  file (`aria analyze`). Every verb drives the crates the editor drives, so a
  frame, a file, or a detection matches what the editor produces. A wrong
  command line exits 2 and prints the usage block; a failed run exits 1 and
  prints the reason. A codec this machine cannot write, or a bitrate its encoder
  ignores, is refused by name instead of silently swapped.
- **`aria-detect`.** The decode-to-detector seam — analysis-rate mono PCM and
  sampled RGBA frames — is its own crate with one owner, so the editor, the
  command line, and a plugin all detect the same way on the same input. It used
  to live inside the editor, which is why nothing but the editor could detect.
- **Ducking music under voice.** Flag a lane as the duck source, select the
  music, and **Duck under voice** in the Inspector's Audio tab measures the voice
  and writes the dip as ordinary volume keyframes — draggable, retimeable, and
  one undo entry for the whole run however many clips it touched. **Dip to**,
  **Attack**, and **Release** shape it; the dip is a share of each clip's own
  level, so ducking a quiet clip never raises it. Sentences closer together than
  attack plus release become one dip instead of letting the music rise between
  them, and voice overlapping a clip's edge leaves that edge already ducked. The
  refusal is written under the button, not hidden in a dead control.
- **Pool previews and a source monitor.** Every pool row carries a picture of
  its source and scrubs under the pointer, left edge to right edge, without
  placing anything. With the pool focused, `Space` loads the selected item into
  the preview pane as a source, named with the frame it is showing, and scrubbing
  its row moves it; `Space` again or **Sequence** returns the pane to the
  timeline. Loading a source stops playback, and the safe areas and transform
  handles are not drawn over it. A source frame is rendered by the same path
  that renders the timeline, so nothing here decodes a second way.
- **Source in and out points, Insert and Overwrite.** A loaded source has its own
  scrub bar, `I` and `O` mark its in and out points while it is loaded (the marks
  are exclusive at the out end, so the frame you marked is kept), and **Insert**
  (`,`) or **Overwrite** (`.`) cuts the marked range into the sequence at the
  playhead. Overwrite is a new `OverwriteClip` edit command: it covers the span it
  lands on without moving anything downstream, dropping material wholly inside the
  span, trimming material that reaches into it, and leaving a straddled clip alive
  on both sides — one undo entry, rolled back whole if any step is refused.
  Dropping pool media onto the timeline in Overwrite mode uses it too, where it
  previously failed on any overlap.
- **Three-point, four-point, and Match Frame.** A cut-in lands on the sequence's
  In point rather than always the playhead, and a sequence marked at both ends
  makes the edit four-point: the new `FitToFill` edit command opens the marked
  span (by rippling or by covering), places the take, and retimes it to fill the
  span exactly, as one undo entry. Alt+F is Match Frame: it opens the source of
  the clip under the playhead in the monitor, on the same frame, with that clip's
  take marked, so the way back from a timeline frame to its footage is one key.
  **Match Frame**, **Insert Source**, and **Overwrite Source** join the Sequence
  menu, and `fit_to_fill` joins the assistant's tools (schema version 42).
- **Lane targeting and sync lock.** Every track header gains two toggles. Target
  (`T`) picks which lane an unaimed edit lands on — an Insert, Overwrite or Fit
  to Fill from the source monitor, or a media drop with no lane under the
  pointer — with the topmost targeted lane of the media's kind taking it and an
  aimed lane still winning. Sync lock (`⇄`) picks which other lanes ride along
  with an insert: a locked lane shifts by the same span so picture and sound stay
  in step, and clearing it pins that lane instead. The lane being inserted on
  always opens, a cover still moves nothing, and the whole ride-along is one undo
  entry. Both flags are set on every lane, are saved with the sequence only when
  cleared (so existing projects round-trip byte for byte), and are reachable from
  the assistant as `set_track_targeted` and `set_track_sync_lock` (schema
  version 43).
- **Export codec and bitrate.** The Export sheet gains a **Codec** row (H.264 or
  H.265) and a **Bitrate** row (the encoder's own rate control, or 4, 8, 16, or
  40 Mb/s). Each row offers only what the machine can honour: Linux codecs are
  probed from the installed GStreamer encoders at run time, and the other
  backends write H.264 at their own rate and refuse both rows by name. A codec a
  backend cannot write, or a target bitrate it would ignore, is refused before
  the output file is created rather than quietly swapped, and the Format row now
  names the codec of the file about to be written instead of the backend's
  default.
- **Beats are stored on the clip.** A beat run now writes its grid onto the clip
  it ran on, in source ticks, as one undo entry: the ruler hairlines and the snap
  magnet read the project rather than the analysis cache, so a reopened project
  still snaps to its beats and a trimmed clip offers only the beats it kept.
  Clearing the beats analysis clears the grid, and a run that finds nothing
  stores nothing. `DetectBeats` and `DuckLanes` are gone from the command set —
  both returned `Unsupported`. Detection is a job whose result arrives as the new
  `SetClipBeats` command, and a duck is volume keyframes (see above); no engine
  arm returns `Unsupported` any more.
- **Bins in the media pool.** The pool holds folders: a breadcrumb strip shows
  where you are, **New bin** makes one inside the open folder and starts its name
  in a field, and a pool item or a folder can be dragged onto a folder row or a
  crumb to file it there. Deleting a folder moves its media and its folders up to
  its parent, so nothing leaves the pool, and undoing the deletion puts the
  folder back with what it held. A folder cannot be moved into its own subtree.
  The list shows the open folder; the filter box searches it and everything under
  it, and the count beside the box describes that scope. Bins are saved with the
  project, and a project with no bins writes none.
- **Slip, slide, rolling edit, rate stretch, and ripple trim.** Five trim tools
  join the tool strip, on Y, U, N, R, and P. Slip moves the take inside a clip;
  slide moves a clip and gives the difference to its neighbours; a rolling edit
  moves the cut between two abutting clips; rate stretch changes how long a clip
  plays without dropping any source, composing with a speed the clip already has;
  a ripple trim moves the rest of the lane by what the edge gave up, so a trim
  leaves no gap. The gesture is chosen by tool rather than by a modifier, so it
  does not collide with add-to-selection or magnet suspension. Slide, a rolling
  edit, and a ripple trim each issue two edits and cost one undo, and a refusal
  rolls the pair back instead of leaving half a gesture on the timeline. Slip is a
  new `SlipClip` edit command, because a trim derives its source window from the
  timeline and so cannot express one.
- **Codecs are named in the pool.** A pool row and the properties block report the
  video and audio codec, read from the demuxer at import rather than guessed from
  the file extension; a backend that cannot name one reads `unknown`. On Linux the
  probe now answers the audio track and both codec names in a single GStreamer
  discovery pass instead of one pass per question.
- **An output peak meter.** The preview transport now shows the peak level of
  what is playing, per channel, measured on the block handed to the audio device.
  A sample at full scale latches a clip indication until it is clicked, so a
  clipping mix is visible while editing rather than at export.
- **Move and scale on the canvas.** The preview now offers transform handles: drag
  the picture to move it, drag a corner to scale it uniformly. A **Handles**
  control in the transport, **View > Cycle Canvas Handles**, and the palette step
  between the Transform, Crop, and Mask handle sets, so the three sets no longer
  compete for the same corners. One drag is one undo entry, and a release without
  movement writes nothing.
- **Pool media is dropped where you aim it.** Dragging a pool item onto a lane
  places it on that lane at the tick under the pointer, through the same magnet a
  clip drag uses. The strips above and below the lane stack are the only drop
  that adds a lane, so a mis-aimed release cannot grow the sequence, and media a
  lane cannot hold falls back to a lane of its own kind at the same tick.
- **Unsaved work is asked about, not thrown away.** New, Open, Close, Home, and a
  project dropped on Home now ask before replacing a project with unsaved edits.
  Save writes the project first, Discard goes ahead, Stay cancels, and a failed
  save keeps both the prompt and the edits.
- **Autosave and crash recovery.** A dirty project is copied to
  `~/.aria/recovery.aria` every twenty seconds and the copy is deleted whenever
  the project file catches up. After a crash, Home offers the copy back: Restore
  loads it, aims Save at the original file, and leaves the session reading
  unsaved so the recovered work cannot be quit on by accident.
- **Export names its own encoder.** The Format and Encoder rows come from
  `aria-encoder` instead of a compiled-in label, so they name the real
  container, codecs, and backend, and a build with no native encoder refuses the
  export with that reason instead of failing mid-render.
- **The export sheet asks what to render.** Scale, frame rate, keyframe spacing,
  whether to include audio, and whether to render only the marked range. A
  ranged export writes a file that starts at its own first frame, and the status
  line names the size, the audio, and the span.
- **Icon-only controls name themselves.** Hovering a tool, a transport button,
  the magnet, the timecode field, a preview zoom control, **Add a track**, or an
  effect row's buttons shows what it is and the key that does the same thing,
  read from the key bindings rather than typed in twice.
- **The palette offers the whole menu bar.** Every row the menus draw, including
  a plugin's, is now reachable by name, and each such row names its menu. Up and
  Down move a highlight that Enter runs, and filtering ranks prefix, word start,
  substring, then letters in order.
- **Zoom, pan, and fullscreen in the preview.** The transport row gained a zoom
  ladder from 25 to 400 per cent with a readout that returns to fit when
  clicked, and a frame drawn above 100 per cent can be dragged. **View** adds
  title safe, action safe, and a thirds grid as three independent overlays, and
  **Fullscreen Preview** fills the window with the picture until Escape. None of
  it is an edit.
- **Rubber-band selection.** Drag on empty timeline space to select every
  unlocked clip the band touches, across the lanes between its corners. A press
  with no drag clears the selection and Esc abandons the band.
- **Pool rows say whether a file has sound.** A row now reads frame size, length,
  and `audio` or `silent`, and the properties block adds the import date.
- **Pool order by date added.** An import records when it happened, so the bin
  can list newest first. An entry from a project saved before import times were
  recorded keeps no time and sorts last rather than being given a fake one.
- **The tool-strip timecode is typable.** Click the clock and type a position:
  frames, or one to four colon-separated fields filling from the right. Enter
  seeks, Esc leaves it alone, and a value that is not a timecode is refused by
  name with the playhead left where it was.
- **The magnet says what it caught.** The snap guide is drawn with the name of
  what fired it: playhead, marker, beat, clip start or end, In or Out, sequence
  start or end. It is drawn while the drag is still in flight rather than after
  it commits. Holding Alt suspends the magnet for one drag without touching the
  toolbar toggle.
- **Draggable fades.** A square in each top corner of a clip card sits where the
  fade ends; drag it to set the fade in or out without going through the
  inspector. The drag clamps to the clip's length instead of being refused, and
  it works on the clip under the pointer rather than the selected one.
- **Clip names.** Double-click a clip's name on the timeline, or use **Rename
  Clip** in its context menu, to name it yourself. The name is stored on the clip
  and outranks the one its source provides, so it survives a relink; an empty
  name gives the source's name back.
- **The status line clears itself.** A status is a receipt and goes after four
  seconds; an error stays up until an edit, an undo, or a redo succeeds, which is
  the only thing that makes a failure stale.
- **Lane collapse.** The chevron in a track header folds the lane to a 20-pixel
  strip of flat coloured bars, so the cut pattern stays readable while the lane
  takes almost no room; the height is kept and expanding restores it. Collapse is
  stored with the sequence.
- **Per-lane display options.** Drag a lane's lower border to resize it (28–200
  pixels, one edit per drag), or take a height preset and turn waveforms or
  filmstrips off for that lane from the track-header menu. All three are stored
  with the sequence, so reopening a project gives back the timeline layout;
  projects written before this load with the shipped lane. A display change is
  allowed on a locked lane, because how a lane is drawn is not an edit to its
  clips.
- **Waveforms on audio clips.** Peaks are decoded once per source file off the
  interactive thread and every clip cut from that file slices its own source
  window out of them, so trimming and splitting cost no decode. The wave is
  drawn at the level the mix will play: volume scales it and fades ramp it.
- **Filmstrips on media clips.** Each cell is a rendered frame of the sequence,
  so the strip shows the picture with transform, grade, and effects applied.
  Cells sit on one grid shared by every clip, render one at a time in the
  background, and invalidate only the ticks whose picture changed.
- **Label colours** on clips and media-pool rows. Right-click either for a row of
  the eight marker colours plus a clear; a labelled clip is drawn in its label
  colour rather than its kind colour, a labelled pool row carries a dot in it,
  **Select same label** reaches the whole clip group again in timeline order, and
  the pool filter matches a colour name.
- A **History** panel (**Window > History**, Ctrl+Shift+H): every step of the
  session, named after the command that made it, with the current point marked.
  **Go** on a row moves the project to the state after that step in either
  direction, so undo is no longer a keystroke into the dark. Undone steps stay
  listed until a new edit replaces them.
- **Paste Insert** (Ctrl+Shift+V) and **Paste Attributes** (Ctrl+Alt+V). Insert
  ripples the target lane right by the copy's length first, so a paste at the
  playhead never overwrites; Attributes writes the copied clip's transform,
  filter, and whole effect chain, with every parameter, onto the selection and
  leaves its timing alone. Both are also in the Edit menu and the palette.
- **Fit Selection** (Shift+F): zoom the timeline to the selected clips instead of
  the whole sequence. Also in the Sequence menu and the palette.
- A plugin can ship as a shared library. Everything in `<data dir>/plugins` is
  loaded and mounted after the built-ins, with the same services, events, and
  panels a built-in has. `examples/notes-plugin` is a working one, and
  `cargo run -p aria-plugin --example load_external -- <dir>` reports what a
  directory would mount.
- A plugin library built by a different compiler is refused before its code runs
  and appears in the Plugins list with the reason, instead of crashing the app on
  a vtable that no longer matches.
- Transitions, effects, filters, animations, text effects, easings, and speed presets
  are data files. A copy under `<data dir>/catalogs` replaces the shipped set, so a
  filter with a different grade or an extra effect parameter needs no rebuild. An
  override that will not parse or will not validate is refused whole and the shipped
  set stays in effect. **Settings > Catalogs** names every catalog, its entry count,
  whether your file or the shipped copy is in effect, and why a file was refused.
  See `docs/catalogs.md`.
- A plugin declares the services it reads (`uses`) and gets a context scoped to
  that list: asking for anything else returns nothing, whatever is registered.
  A refusal is recorded against the plugin and shown in **Settings > Plugins** as
  `refused <Service>`, so a missing declaration is visible instead of a capability
  that quietly does nothing.
- Mount order is derived from each plugin's declared dependencies, so a plugin
  listed before the one it requires no longer fails with a message that reads like
  a missing plugin. A dependency cycle is refused and names every plugin in it.
- A plugin can add a command to the palette. The shipped commands register through
  the same call, so the palette lists whatever is registered and a plugin can
  replace a shipped command by reusing its id. See `docs/plugin-development.md`.
- A plugin can add an item to a menu in the app bar. The shipped menus are the
  first contributor to the same registry, so a contributed item is drawn by the
  code that draws File > New, and it cannot take a shipped entry's id.
- A plugin can bind a key. Every shipped shortcut is a registered binding, the
  shell holds no chord of its own, and a chord that is already taken is refused
  and recorded instead of overriding the binding that holds it.
- A plugin can declare its own settings. Each declared field shows in
  **Settings** under the plugin's own section, is edited like a shipped field, and
  persists to `config.toml` under the key the plugin named. A secret field shows
  its length, never its text.
- Stickers are a catalog. `stickers.toml` names each sticker's art file, art is
  read from `<data dir>/catalogs/stickers/` with the shipped pack as the
  fallback, and a sticker lands without a rebuild. An entry whose art is missing
  is dropped rather than offered as a tile that cannot draw, and the reason shows
  in **Settings > Catalogs**.
- A Markers tab. Every ruler marker is a row with its time, label, colour, and
  note: clicking one seeks to it, the swatch cycles the palette, and Note edits
  a comment the marker keeps. A marker's colour now shows on the ruler flag
  instead of every flag being drawn in the accent colour.
- A marker carries a note. It is written and cleared from the marker list, kept
  by every other marker edit, restored by undo, and absent from a save while
  empty so an older project loads unchanged. The assistant can set it too.
- The bin filters and sorts. The filter matches a pool item's name, path, kind,
  and frame size, the header says how many of the pool it leaves, and the order
  is name, duration, or resolution. Selecting an item shows what the pool knows
  about it: container, frame size, frame rate, duration, audio, link status, and
  path.
- **Every text box takes a paste and an IME commit.** Typing was routed one
  character at a time, so a pasted path, a pasted prompt, and every non-Latin
  input were unreachable in all nine boxes. A run of text now goes through each
  box's own key row, which means it keeps that box's rules: the timecode box
  still takes digits and separators only, a name still stops at its cap, and a
  control character is dropped, so a path pasted with its trailing newline
  arrives as a path. The app registers a platform input handler on the window,
  so a preedit run is marked and replaced by the finished text rather than
  appended to it.
- **The inspector's tabs are a registry.** The tab list was a fixed array read
  by three matches, so a tab could not be contributed and each one carried its
  visibility rule in a second place. A tab is now a row with an id, a label, the
  clip kinds it applies to, and the body it draws; the selected tab is that id.
  A test walks every clip kind and fails when one has no tab to draw or when a
  selected tab resolves to one that does not apply.
- **The inspector's sliders are the property catalog.** Every numeric row in the
  Mask, Chroma Key, Colour, LUT, and layer-style blocks is generated from
  `clip_param_catalog()` by asking for a group, so a property the model declares
  arrives with a control, a label, a step, and a range without an inspector edit.
  Each row also derives its step and its number formatting from the declared
  range rather than from a hand-kept table, and a property with no control —
  a colour or a crop rectangle, which the swatches and the canvas handles own —
  is declared exempt instead of silently unreachable.
- Sixteen properties now declare the bounds they always had. Mask rotation,
  layer blur, glow and outline width, background padding and radius, text size,
  line spacing, letter spacing, stroke width, shadow blur and distance, and
  shape width, height, corner radius and stroke width carried their limits only
  in the inspector's stepper, so the assistant and the wire could write values
  the model then refused. A conformance sweep writes a far value on every side a
  row leaves undeclared, which turns an under-declared range into a failing test
  instead of an error toast.
- **The shell is drawn in tests.** Every other test in the editor crate drove the
  state and never asked the tree for a frame, so a panel that panicked on an
  empty project, a missing frame, or a route it had never been drawn in shipped
  green. A headless window now puts the real shell in front of Home, a project
  with a clip on the timeline, that edit undone, the unsaved-changes prompt, and
  every dialog kind in turn — the plugin dialog included, opened with an id no
  plugin registered. Naming the kinds is an exhaustive match, so a new dialog
  cannot ship without being drawn at least once.
- **Every colour the theme declares is painted by a control.** Twelve tokens were
  declared and unused, which is a design decision nobody can see: the two hover
  tiers are now named rather than computed at the call site, buttons have a
  pressed plate, a queued job's row carries its outcome, the status strip turns
  and grows a live dot while work is running, a missing recent project rests on
  the warning wash, and the marquee, the drop line, and the marker bands use the
  tokens meant for them. A test reads the token list out of the theme
  declaration at run time and fails on a token nothing paints.
- **A clip lands instead of appearing.** Whatever created it — a drop, a paste, a
  razor split, an insert at the playhead — the new card fades and settles the
  short distance into its lane, because the edit that created it records the fact
  once and the draw asks for it once per frame rather than per card.
- **The editor opens in three stages.** Chrome, then the rails and the picture,
  then the timeline, 80 ms apart, inside the splash's own exit. Reduced motion
  means no open rather than a faster one.
- **Disclosure triangles turn.** The lane and bin arrows rotate a quarter turn
  instead of swapping one glyph for another, which at 12 px read as a flicker.
- **The assistant transcript follows its newest line.** A reply that is still
  arriving eases the view to the bottom over one glide and carries a caret that
  breathes at the end of the growing text, and the follow is bounded twice so a
  transcript being read is never dragged back down.
- **Every chord the app dispatches is in the shortcut reference.** Six shipping
  chords were documented nowhere: ripple insert, snap a marker, shift a lane one
  second either way, cycle the duck amount, and reset a clip's effects. The
  reference and the dispatcher are now checked against each other in both
  directions at run time, so an undocumented chord and a documented chord that
  dispatches nothing both fail.
- **The component gallery covers every primitive and every motion spec.** The
  disclosure triangle, the band fills, and the staged app-open had no specimen.
  Coverage is read out of the component and motion sources at run time, so a new
  primitive or a new spec is red until it is on screen, and the gallery takes the
  window context so reduced motion is shown as a state rather than skipped.
- **Every gesture the mobile shells send has a test.** Eleven of the twenty-two
  intents had no case anywhere: a lane trim, a main-track reorder, a sticker or
  effect drop, a retime, a transform, the keyframe diamond, and a transition
  could land on the wrong lane or write the wrong ticks without failing a test.
  Catalog ids come from the catalogs rather than the cases, and the variant list
  is parsed at run time, so a twenty-third gesture is red until it is covered.
- **A browsable catalog says what the clip already carries.** The transitions,
  masks, and animations shelves listed every entry identically, so the only way
  to learn which blend or shape was applied was to apply one. The applied
  entries now carry the same `•` prefix and standing wash the inspector's
  pickers use, and the blend catalog on a wedge's own menu marks its junction's
  blend too. Animations mark all three slots at once, because a clip can hold an
  entrance, an exit, and a combo.
- **The unsaved prompt says where it is going.** "Save" and "Discard" both leave
  the project, and the operator asked for one specific elsewhere: the prompt now
  names it — Home, a new project, another project, the sample, or what was
  dropped — from the parked intent rather than a sentence per surface.
- **A pool item the timeline is using says so, and reaches the clips.** Removing
  one was offered, refused by the model, and reported as an error after the
  click. The row now states the item is in use and selects the clips holding it.
- **Alt-dragging a bin item onto a clip replaces its media.** Repointing a clip
  at other pooled media was reachable only from a list in the inspector. The
  drop now does it too: the clip keeps its placement, transform, effects, and
  keyframes, the lane says a replace is coming while Alt is held, and Alt over
  empty lane still places the media rather than doing nothing.
- **A pool item can be given a proxy, and says when it has one.** The renderer
  could always decode through a proxy and Settings could always measure, clear,
  move, or redirect the `proxies` cache; nothing ever wrote one, so the whole
  path had no first step. A bin item now offers "Decode through a proxy" — which
  adopts the file already on disk, or renders one as a cancellable job at 960
  pixels on the long edge — and "Proxy every pool item" does the pool in one
  sweep. The proxy is named by the same content digest analysis batches use, so
  a re-encoded source cannot inherit the proxy of the frames it replaced, and a
  proxied row says `proxy` in its caption. Export is unaffected: it renders the
  originals, as it already did.

### Changed

- The `GeneratorService` trait and the `aria.text` and `aria.shapes` plugins that registered it are deleted; text and shape clips render as before, and startup no longer builds a text renderer that nothing read.
- The `RenderInterceptorService` trait, which nothing called, is deleted.

- **GPUI builds from santh-gpui, the framework shared with iris and Veyyon.**
  The vendored copy of the `wingleeio/zed` fork is deleted, and building from
  source requires Rust 1.97 or newer.

- **Linux: the GPU device and a new window's surface are set up off the UI
  thread.** On Wayland the device is created on a thread while the display
  connection starts, and on X11 a new window configures its surface on a thread.

- **An image uploads to the GPU as it is first drawn.** The GPU renderer writes
  a new image tile through the queue when the tile is allocated, with no copy of
  the pixels held until the next frame.

- **Interface motion uses the `gpui::motion` presets of santh-gpui.** Menus open
  in 140 ms and close in 100 ms, dialogs arrive in 180 ms, hover fades, quick
  fades and the inspector tab indicator take 150 ms, lane heights settle in
  180 ms, and the splash holds 150 ms before it fades.

- **Headless renderers share one process GPU device.** Engine construction no
  longer opens another adapter and logical device for every background render or
  test. The GL fallback keeps its thread-bound context separate.

- **Text renderers parse the installed font catalog once.** Each renderer keeps
  isolated shaping, raster, and custom-font state over the cached host catalog,
  and the font picker reads the same catalog without another directory scan.

- **An animation is keyed by an identity rather than by text as well.** GPUI keys
  animation state by element id exactly as it keys interaction state, so the six
  entry and exit helpers, the row stagger, the settled row and the window's
  opening stages all accepted a formatted id. A drag rebuilt the snap guide's id
  on every mouse move, the preview rebuilt the frame fade's id on every seek, and
  a list rebuilt one per row. All of them take a `ControlId`, the snap guide and
  the frame fade return a key instead of a string, and the census that bans a
  formatted identity now counts an animation as a place one lands.

- **A control's identity is no longer a string built every frame.** A shared
  button, chip, loader row or progress bar took anything that converts to a GPUI
  element id, and `String` converts, so fourteen controls named themselves with
  `format!`: one per multicam angle, per mixer strip and meter and fader, per
  live job's status line and bar, per curve parameter, per easing preset. Each
  allocated on every frame the control drew, and the loaders allocated a second
  time to hash the same text back into a motion key. Those controls now take a
  `ControlId`, which is either a name fixed in the source or a hashed key, and
  has no conversion from `String` at all, so a formatted identity does not
  compile.

- **A panel's own controls stopped naming themselves with text too.** Six helpers
  that panels use to draw a chip, a menu row, a lane toggle, a text field, an
  overlay grip and a settings row took their identity as a `String` or a
  `SharedString` and assigned it to an element id themselves, so the ban above
  stopped at the shared components: the timeline built four formatted ids per
  clip per frame for its trim and fade grips, and the mixer built one per output
  chip and one per send chip per strip. All six take a `ControlId`, so a caller
  cannot hand one text.

- **The pool draws its folders instead of walking them.** The breadcrumb and the
  folder rows were each a walk of the whole folder tree, run on every frame the
  pool was on screen and copying every folder they kept, and the strip's own
  count was a third walk that could disagree with the rows it described. Both
  are one projection now, replaced when the folder tree or the open folder
  changes, and a test reads the crate's own sources to keep a draw out of the
  tree.

- **A stepper row carries shared text.** A row's id and label were owned strings
  built per row per frame, and a vector property formatted two more of each for
  its `X` and `Y` rows. The component text is built where the property list is
  projected now, and the row holds shared text the draw refcounts.

- **Three dialogs stop copying their whole list to draw it.** The history dialog
  copied every step of the undo stack, the project report copied its project
  facts and its clip facts, and the pool sidebar's camera strip copied every
  multicam group and every angle in it, once per frame each was open, because a
  panel cannot hold a borrow of the state across the frame. Each list is a handle
  now and the accessor hands one back, so a frame refcounts, and the report's
  measured values are shared text rather than owned strings the draw copied
  again.

- **The inspector reads one property list instead of rebuilding it thousands of
  times a frame.** Enumerating an animatable property cloned every catalog row
  that applied to the clip, and the inspector did it once for the row list, again
  for every row to read that row's keys, again for every stepper group, and again
  for every sample of a curve — so a hundred-point graph beside a sixty-property
  clip cloned the same rows thousands of times per frame, and the cost grew with
  the square of the property count. The rows are projected once per change now,
  beside the clip they belong to: each carries its shared id and label, its
  catalog row, and its keys in timeline ticks, so a frame clones a handle and a
  panel that wants one property asks for that row. A scrub keeps the list, since
  no instant moves a property's keys. One module enumerates the catalog, and a
  test reads the crate's own sources to keep it that way.

- **A panel no longer copies its text to draw it.** Every string the store held
  for the interface was an owned `String`, and `String::clone` allocates every
  time, so each frame turned the visible rows back into fresh heap copies: a clip
  name, a job label, a media report line, a model row and a caption were copied
  once per row per frame, and a media path or a report value is far too long to
  live anywhere but the heap. A catalog chip went further and rebuilt the whole
  filter, transition and text-effect vocabulary out of the catalogs on every
  pass. The store now holds shared text, so a draw is a 24-byte copy for short
  text and a reference count for the rest, the projection pays once when the
  value changes, and the three catalogs are shared once per process behind their
  accessors. A streamed assistant answer grows one shared string per fragment
  instead of rebuilding the visible transcript. Two rules read out of the sources
  keep it that way: no mirror type may hold an owned string, and nothing in the
  draw layer may copy a string to hand it to a draw.

- **A render clones a handle, not a list.** A panel cannot hold a read borrow of
  the store while it builds its click handlers, so four panels copied a whole
  list out of the store instead: the assistant conversation, the marker list, the
  set of clips with their keyframe lane open, and the loaded text-preset pack,
  each copied once per frame. The conversation was the worst of them, because it
  is the one list that grows while a model answers, so a long answer was copied
  again for every token that arrived. Those lists now live behind a shared
  handle: a frame clones a reference count, an edit takes the list back with
  copy-on-write, and a preset is copied only when its row is clicked. A frame
  already on screen keeps the rows it was drawn with. A rule reads the store's
  own field types and fails on a render that copies a list, so a new list cannot
  arrive with a per-frame copy attached.

- **A list one level down is behind a handle too.** The rule above read only the
  store's own field types, so a list inside a form the store holds was invisible
  to it, and three renders still copied one: the Captions panel cloned its whole
  form to read a language and two switches off it, and every transcript segment
  in that form carries its own word list; the Audio tab cloned the measured
  loudness contour, one sample per analysis window over the whole clip, to draw
  forty-eight columns; the assistant panel cloned the pending plan every frame
  until it was applied or dropped. The contour and the plan are now shared
  handles, the Captions panel reads the four values it draws and counts the
  units a commit would write instead of building them, and the rule follows a
  store field into the form it names, at any depth, so a list that arrives one
  level down is red on the first frame that copies it.

- **The jobs board is projected when a job reports, not when a frame draws.**
  Every surface that shows pending work built the whole list itself: the pending
  strip under a panel, the jobs board, the export sheet and the status line each
  asked the job registry for a snapshot, which takes its lock and copies every
  job's label, detail and outputs, and then rebuilt a row out of each one. A
  frame with one panel open did that four times, while the job threads writing
  progress into that registry waited on the same lock — the editor got busier
  exactly when a render was running. The board is a projection now, like the
  sequence: it is rebuilt when the registry publishes a transition and when an
  export is queued or cancelled, and every surface reads it from a shared
  handle. Starting work puts its row up in the same update that started it,
  rather than a scheduler hop later. Two rules hold it: every producer goes
  through the one call that projects the board, and a draw that reaches for the
  registry is refused.

- **A colour adjustment is one row, all the way to the shader.** The renderer's
  end of the grade was a second hand-list of the same 23 sliders: the struct,
  its neutral, its identity test, the six uniform lanes, the filter fold and its
  clamps each named every field, and the WGSL read the lanes by position, so a
  slider that reached the model but not a lane composited as neutral with
  nothing red. All of it now expands from the adjustment row list, including the
  lane count and one WGSL accessor per slider, generated ahead of the shader
  that reads them. A slider whose lane no shader reads is refused by a census
  that presses every row through a real render, on the layer path and on the
  canvas pass, and asks the frame to move.
- **Every compositor pass reuses its bind group, not just the effect passes.**
  A graded frame with a LUT built a descriptor set and a uniform buffer per
  layer per frame for the grade, the LUT, the composite blit, the transition,
  the matte, the blend and each of the four style passes, and dropped them all
  at submit; only the effect chain reused anything. All of them now take both
  from one table keyed on the layout and the resources bound, so a twelve-lane
  frame builds what a two-lane frame builds and a repeated frame builds
  nothing. A table entry holds the textures it binds, so it cannot name a
  freed view, and it retires after eight undrawn frames, so a scene that
  shrinks gives its memory back.
- **An export is a value before it is a job.** Pressing Export built the whole
  request inside the click that queued it, so the one command whose surface is a
  shipped dialog had nothing a test could read: asserting the button meant
  starting a render, which spends the machine the suite runs on and needs an
  accelerator to pass. The sheet now assembles an `ExportRequest`, its
  destination, format, evened frame size and range, with no window, no session
  lock and no encoder probe, and queueing takes that value. The printed
  destination and the written path have one owner, so the label and the file
  cannot disagree, and adding an export scale, rate, GOP length, bitrate or
  codec stops the request tests compiling until the new member is stated.
- **The pool list is a view of the projection, not a copy of it.** Drawing the
  media sidebar cost two pool-sized clone passes per frame: the rows were a
  `Vec` of full entries, name, path and both codec strings per item, and the "3
  of 40" line called the same filter again for its numbers. A click that ran a
  span along the drawn order asked for a third. The rows are now one walk that
  produces indices into the projection the panel already holds, so the filter,
  the sort, the count and the span cost one refcount bump between them, the
  count can no longer disagree with the rows it describes, and a tile copies
  only the label a drag carries.
- **The settings dialog's project rows are a declared surface.** The canvas
  aspect, the delivery size, the sequence rate and the channel layout each wrote
  the project from inside a click closure, so three commands claimed a dialog no
  test could press and a row wired to the wrong setter turned nothing red. The
  rows are a registry the dialog is built from, the frame rates it offers live
  with the rows rather than in the panel, and a census presses every aspect,
  size, rate and layout it draws. Picking what the project already holds still
  changes nothing, and that is now asserted rather than assumed: a click on the
  current value writes no undo entry.
- **The source monitor's bar, the markers panel and the pool sidebar's buttons are
  declared surfaces.** Eight commands claimed controls no test could press, and one
  of them claimed a control that was never drawn: there is no fit-to-fill button.
  A fit is what Insert and Overwrite do when the sequence carries a marked range
  of its own, so the bar's rows are now driven with and without that range and the
  command each sends is asserted. The multicam button used to discard its result,
  so a group the model refused looked exactly like one it built; it reports the
  refusal, says when the pool is empty rather than building nothing quietly, and
  the rule for which pool items become angles is part of the row instead of the
  panel. The markers panel's add, colour and delete controls are pressed against a
  marker the panel would have drawn, and a press on a marker that is already gone
  is asserted to reach nothing.
- **The analysis panel's controls are a declared surface, and a clear reports what
  it could not do.** Two commands claimed the panel and nothing could press them,
  so a clear wired to the neighbouring detector would have deleted the wrong
  results with nothing red. Each clear is now pressed against a project that
  carries results from every detector and is asserted to drop its own and keep the
  rest, clearing the beats is asserted to take the snap grid with it, and both
  filters and both seeks are asserted to reach the engine not at all. A refused
  clear or refused edit used to vanish into a discarded result; it reaches the
  error line. The one command in the pair that no button sends is the beat run's
  own write, so the run's adoption path is driven directly rather than given a
  button it does not have.
- **Every drag is a declared surface: the timeline's, the preview's, and the Home
  screen's tiles.** A dozen commands were reachable only by dragging or from one
  tile, and each one's only mention was a match arm inside a mouse-up listener,
  so a tool wired to the wrong verb read exactly like a tool wired to the right
  one. `Slip clip`, `Set transition length` and the recovery banner's
  `Load project` had no measurable surface at all, which put a button that throws
  away a crashed session an inch from the only button that brings it back, with
  nothing red between them. The three sets are registries now: the timeline's
  body, edge and transition drags, the canvas overlay's move, scale, crop and
  mask handles, and the Home route's tiles, recent-card actions and recovery
  offer. The panels convert pixels to ticks and hand the gesture over, so a drag
  is asserted independently of the zoom it was performed at, and a census drags
  every handle each set draws. A press that never moved stays a click rather than
  an undo entry, a roll with no neighbour on the far side of the edge edits
  nothing instead of quietly trimming one side, and the magnet's decision lives
  with the gestures rather than in whichever listener called it. Reveal is the
  one row a test does not press, declared at the row with its reason, because it
  opens a file manager. The reach table also claimed a cloud-panel action that
  opens projects; the cloud panel loads catalogs, so the row now names the
  surface that exists.
- **An inspector block draws every property the model gave it, and a nudge no
  longer flattens a curve.** Each block carried its own list of the properties it
  drew, so a property the model gained was a property the panel silently lacked:
  the transform's anchor point, all twelve text properties and all seven shape
  properties had no surface at all, and a shape clip had no tab. A hand-listed row
  also wrote a block-shaped command, which carries the whole block, and the panel
  had only constants to put in it, so dragging the transform's scale on a clip
  with a keyed position replaced the position curve with a number. A block now
  names its property group and nothing else, and the rows come from the property
  catalog: the transform block gains its anchor rows, the text block gains a Type
  section with the model's twelve properties, a shape clip gains a Shape tab, and
  every one of those rows takes a keyframe under the playhead when the property is
  animated and a constant when it is not. Volume, pan and matte softness write
  through the same path, so a fade survives a volume nudge and a mask curve
  survives a softness nudge. Which outline a shape draws is still set when the
  shape is placed; the tab says so rather than offering a picker with nothing
  behind it.
- **Every control in the mixer and the inspector is a registry row, so none of
  them can be a button wired to the wrong thing.** A strip's output chip, its
  send chip and its fader were four gpui listeners, and the inspector held
  another seventy: the filter chips, mask shapes, key swatches, matte sources,
  layer-style toggles and their swatches, blend modes, stabilization levels,
  flips, motion blur, noise reduction, the audio role and fades, reverse and
  pitch lock, the animation chips and knobs, the speed ramps, the template slot's
  media, kind, order and hint, the text effects, presets and fonts, the
  transitions, the effects rack's add, remove, reorder, preset, colour and
  parameter fields, and every button on a keyframe row. Each sealed its call
  inside a closure, so about thirty of the model's commands were reachable only
  that way and nothing could enumerate them. There are now six registries in
  `surfaces`, each with one function that applies a named payload, and the panels
  only build payloads. Four consequences an operator sees: riding a fader writes
  one history entry on release rather than one per pointer move, a fader pressed
  and released without moving writes nothing at all, the key button on a
  keyframe row takes a keyframe or takes back the one under the playhead as one
  toggle, and a click made while the selection is being replaced does nothing
  instead of half of something. Every member of every catalog is pressed by a
  test that reads the registry's own arms out of its source, so a control added
  to the model has no dressed state and no green suite until someone writes one.
- **Every menu row is driven against a state that can offer it.** The bar's
  coverage ran all ninety-odd rows against one fixture: one still clip on one
  lane, nothing on the clipboard, no marker, no source loaded, no nest, and no
  render on disk. Fifteen rows were recorded as writing no history for a reason
  about the fixture rather than about the row, and that is the same word the
  table uses for a row that moves the view, so a row that had been broken read
  exactly like a row that had nothing to act on. A row now declares the state it
  is driven against, and eighteen of them are driven against one they can use:
  the three paste rows against a filled clipboard, insert and overwrite against a
  marked source range, the marker deletion against a marker, link against two
  selected clips, unlink against a linked pair, extract audio and freeze frame
  against media with sound, the two transition rows against a cut and against a
  blend, the three nest rows against a nest, and render-and-replace and restore
  against a project whose render is already on disk. One consequence for anyone
  reading a failure: a row that sends nothing is now a defect rather than a
  footnote.
  Six more are driven with the file dialog answered instead of opened: open,
  save-as, save template, use template, import and relink-all now dispatch under
  the census, so a command reached only through a file picker is measured rather
  than assumed. Every row the bar draws is therefore claimed by a driven table,
  and the two tests that check the animation knobs and the animation slots press
  the preset that actually carries the knob and clear a slot that actually holds
  an animation, instead of pressing a row the panel would not have drawn.
- **Scrubbing and playback no longer re-project the timeline.** Moving the
  playhead invalidated the whole projection, so every frame of a scrub and every
  frame of playback walked every track, clip and keyframe and rebuilt the magnet
  index to produce a store that differed in one number. The projection carries
  the tick as a value and derives nothing from it, so a poll where only the
  playhead moved now keeps the store and re-samples the one thing that is read at
  the playhead: the selected clip's resolved quad. Two guards hold that without
  naming a field — a projection at two ticks may differ in `playhead_ticks` and
  nowhere else, and what a scrub leaves must equal a full re-projection at the
  same tick — so a value that starts depending on the playhead has to be derived
  on read or re-sampled by the poll instead of quietly reading one scrub behind.
- **The mask shapes are a catalog file, and a catalog in code is now a declared
  exception.** Eight catalogs read from `data/*.toml`; the mask browser's six
  shapes were still a Rust `const`, so relabelling or reordering them meant
  rebuilding the largest crate in the workspace. They ship as
  `data/masks.toml` and take an override from `<data dir>/catalogs/masks.toml`
  like the rest — a file may relabel and reorder the shapes, and one that names
  a shape the renderer cannot draw, or leaves one out, is refused whole rather
  than leaving a button that draws nothing. The rule is derived from the tree in
  both directions: every shipped data file must be read at startup, so a file
  added without a loader is red rather than dead, and every spec table left in
  code must say what a file cannot carry for it — a fact-extraction function, a
  route into the model, a census constructor, waypoint geometry, or a variant of
  a closed enum the compositor dispatches on. Each of those six declarations is
  counted, so one that becomes data-driven turns the suite red until its row
  goes.
- **A panel is a leaf.** The editor's shared vocabulary lived inside the panels
  that happened to show it first: a loader read the eased-value store out of
  `panels::timeline`, the keymap and the menu bar reached the command table
  through `panels::palette`, the recent-projects list belonged to Home, the shell
  named nine dialog modules by hand, and the import dialog's format list sat in
  the sidebar. So a panel could not be removed without breaking a loader, and a
  plugin's dialog had nowhere to appear. Each of those now has an owner that is
  not a panel — `motion::glide` for eased geometry, `commands` for the registry
  and its dispatch, `pending` for the wait vocabulary, `drag_hint` for the drop
  target, `app::recents` for the list, `app::media::import` for the formats,
  `chrome::dialog` for the card a dialog is drawn on — and the panel and overlay
  registries moved to `plugins`, so the shell asks by location and by condition
  and a plugin registers through the same call the shipped ones use. The
  direction is derived from the sources and fails by default: a file outside
  `panels/` that names a panel module turns the suite red on the run that adds
  it, and the one file allowed to name them — the mount that registers them — is
  counted, so an allowlist entry cannot rot into an exemption for a file that no
  longer registers anything.
- **Every animation is sampled, and proven to finish.** The catalog held
  durations and call sites; nothing held behaviour. Each of the specs is now
  sampled at both boundaries, mid-flight, and across sixty-four points: it starts
  at rest, moves once, stays inside the bound its own curve earns, never goes
  backwards unless that curve is the one that overshoots, holds through its own
  delay, and reaches its end by the end of its own span. The class this closes is
  a transition that never finishes, which leaves whatever it animates part-way
  forever and looks like a layout bug.
- **A released pan carries its momentum.** A pan used to stop dead on the frame
  the pointer left, which reads as the view being dropped rather than let go.
  The rate the gesture was moving at is now sampled while it moves — weighted so
  one jittery frame cannot decide it, capped so a pointer that jumps cannot throw
  the view across the sequence — and the release spends it as a distance. The
  travel to that distance is a new `FLING` spec on `SPRING`, the one curve in the
  app that passes its target and comes back: a flick overshoots by under a tenth
  of the throw and settles. A pan that was placed rather than thrown, one that
  paused before it was released, and a session with motion turned off all stay
  exactly where the hand left them, and any other scroll — a wheel notch, a fit,
  playhead follow — ends the settle rather than inheriting it.
- **The catalog's timings moved onto the scale.** Nine specs were retimed to the
  nearest step: menus open in 160 ms rather than 140, overlays leave in 120
  rather than 100, dialogs and lane collapses take 200, and the boot mark's
  delay is 160. The caret and the Home card grid, which had invented their own
  numbers, now derive theirs from the scale — the grid's fractions are steps
  divided by the animation they are fractions of.
- **Motion off is decided once, not at every call site.** The eased-geometry
  store took reduced motion as an argument, so respecting the setting was
  fourteen call sites each remembering to pass it, and the fifteenth would have
  left one pane sliding for an operator who had turned motion off. The store
  reads the setting itself now, and the caller's flag means only what it should:
  this value is under the hand and must not lag it. One-shot animations were
  already covered, in `gpui`'s own animation element, which draws the end state
  and schedules no frames when the setting is on
  (`crates/gpui/src/elements/animation.rs`). The store gained its first tests:
  a first sighting does not travel, a changed target travels and lands, a value
  only moves toward its target, nothing draws a dropped key, and motion off wins
  over a caller that asked to animate.
- **The timeline's scale travels when the editor moves it.** A fit, a keyed
  step, and a menu row used to jump the horizontal scale, which loses the eye's
  place in a long sequence because every clip changes width and position on one
  frame. Those three now ease over 200 ms, on the same scale the panes and the
  scroll offset use. A wheel notch still lands instantly, and has to: the notch
  is anchored to the pointer, so the frame it lands on must be drawn at the
  scale the anchoring was computed for, or the clip under the cursor slides out
  from under it. `set_zoom` carries which of the two it was, the same way
  `scroll_timeline_to` already did, so the decision is made by the writer that
  knows rather than guessed at by the panel. 200 ms rather than the 500 ms the
  scroll offset takes, because while the glide runs the drawn scale is not the
  committed one, and a click in that window maps to the tick on screen instead
  of the tick the model would compute.
- **A clip card's states fade both ways, and a locked clip says it is locked.**
  Selection played a one-shot, which can only arrive: `gpui` drops the element
  on the frame the flag flips, so the ring appeared over a beat and vanished in
  one, and a click that moved the selection between two cards showed one card
  fading in while the other cut out. Selection and lock are chased values now,
  at the same beat as the hover fill under them, so a reversal continues from
  wherever the fade had got to instead of restarting. Lock also had no picture
  at all: it changed the cursor and removed the trim handles, both of which the
  operator discovers by trying to drag. It carries a badge in the corner those
  handles occupy, which is free exactly when the badge is drawn.
- **Interruption is a property of the two stores, not of each animation.** Both
  the hover store and the eased-geometry store take a changed target as a new
  leg from the current drawn value, so nothing queues and nothing snaps to an
  end it never reached. That is written down now, with the rule it implies: a
  state that can be left again belongs in a store, and a one-shot is for an
  entrance or an exit that happens once per appearance.
- **A route change is one transition, at one rate, and it can be interrupted.**
  Home and the editor each had their own entrance: Home a 160 ms one-shot fade,
  the editor its 320 ms staged assembly. Leaving therefore felt quicker than
  arriving, and because both were one-shots, opening a project and pressing
  Escape before it had finished replayed a full fade from nothing — the screen
  went back to the shell's bare background on the way. The shell now owns the
  transition: one chased value per end, both advanced every frame so neither is
  evicted while it is off screen, and the body being drawn appears at the value
  it has reached. A route change during a route change continues from what is on
  the screen. What it is not is a cross-fade: the outgoing body is unmounted on
  the frame the route changes, because keeping a second editor's worth of live
  listeners alive to fade one out costs more than the transition is worth.
- **A running job has a bar, and the bar is the same bar everywhere.** A render
  reports a fraction every few hundred frames, which the Jobs board drew as a
  fixed 220px track that jumped between reports and the export sheet drew as a
  percentage in a label. Neither said anything while a job reported nothing. One
  bar now serves both, keyed by the job rather than by the surface, so the board
  and a panel's own row draw one width for one job: a reported fraction fills it
  and travels to each new report, and a job that cannot report one shuttles a
  segment at the loader clock's rate. It is built only for a live job, so it
  stops when the job does, and the fraction is clamped where it is drawn — a job
  that over-counted its frames fills the bar instead of drawing past it.
- **A wait shorter than a settle shows nothing at all.** Every loader appeared on
  the first frame of its wait, so a cached feed or a fast probe flashed a spinner
  for two frames: the operator saw something start and stop with no time to read
  it, which reads as a glitch rather than as work. A wait is now aged, and the
  spinner and the bar's fill both wait 320 ms for it. The row's geometry does not
  change when they arrive — the spinner's width is held from the first frame —
  and a wait that ends inside the span leaves no trace, not even an entry to
  clean up.
- **A panel-level wait is the same row as every other wait.** Three shapes said
  "busy": the pulsing block for a panel, the large gradient ring for the export
  sheet, and the small ring for a row. They ran at different rates and meant the
  same thing, so a fetch that stalled in one place looked unlike a fetch that
  stalled in another. Every inline wait now draws through one row.
- The boot cover ends when the editor is ready, not when a clock runs out. It
  held for a fixed 1100ms whether the editor was ready in forty milliseconds or
  nine hundred, so a fast start was made to wait and a slow one was uncovered
  while it was still empty. Home is ready on its first frame — the session is
  built before the window opens — and a project opened from the command line is
  ready when its first picture has been decoded, with a two-second ceiling so a
  decode that never lands cannot hold the window. Reduced motion removes the
  cover instead of fading it.
- Home is called Home in the source. The route had been a dialog once, and the
  three modules, the motion namespace and the icon folder all still said
  `launch`, so the first surface of the app was the one place where what you see
  and what the code calls it disagreed. `panels/home.rs`, `home_cards.rs` and
  `home_recents.rs` now, and no path in the crate says `launch`.
- A drag no longer rebuilds every magnet in the sequence per mouse move. Snapping
  walked every clip, every beat on every clip, and every marker to build one list
  and then scanned it, once per mouse move of every drag and trim. The magnets are
  built once per edit into an index sorted by tick, and a drag binary-searches it
  and walks out until it passes the threshold, so the cost is the magnets near the
  pointer rather than the whole sequence. Which magnet wins a tie used to be
  whichever the builder pushed first; it is a rank on the kind now, in that same
  order, so the playhead still beats a clip edge and a marker still beats a beat.
- A frame reads five numbers instead of copying the settings. Drawing the window
  copied the whole `UiSettings` — including the key map, one entry per binding —
  to read the rail widths and the timeline height, and a splitter did it again on
  mouse-down. Both take the pane geometry now.
- The filmstrip and the audio waveform also stopped waiting on the foreground
  executor. A lane of filmstrip cells and a waveform decode each blocked the
  window the same way the preview did, and a waveform for a long take is seconds
  rather than milliseconds.
- An edit no longer throws away every picture in the pool. Pool thumbnails and
  the source monitor's frame were held against the session revision, so moving a
  clip — which changes nothing about any source — cleared up to 256 rendered
  pictures and the pool re-decoded them one at a time through the engine the
  preview needs. They are now held against what the picture is of: the source's
  path and whether the renderer substitutes a proxy for it. A relink or a new
  proxy drops that entry's pictures and only that entry's; a timeline edit drops
  nothing. Asking for a thumbnail also stopped taking the engine lock, which on
  the render path meant waiting for whatever decode was holding it.
- Scrubbing decodes two frames, not forty. Every request for a preview frame
  started its own decode, and a decode holds the engine for its whole length, so
  dragging the playhead queued one behind another and the picture under the
  pointer arrived after every frame the pointer had already left. One decode now
  runs at a time and a request that arrives during one is coalesced to the
  newest playhead, so a drag costs the frame being decoded plus the frame the
  operator stopped on. The wait for the decode also moved off the foreground
  executor: the window used to hold still — animations, splash, and all — for as
  long as a frame took to composite.
- Home holds its recents list instead of reading it per frame. Drawing the route
  read `recents.json` and stat-ed every path in it to decide which projects are
  still there, once per frame, on the one route that is animated. The list is
  now state the editor holds: it is loaded at startup, refreshed on arrival at
  Home, and refreshed by the four verbs that change it — remembering a saved or
  opened project, forgetting one, and deleting one — so a frame reads nothing
  from disk and the list can never disagree with the file.
- The timeline draws to the width it has. Culling and the two edge fades read a
  fixed 2000px guess at the viewport, so a narrow window built ticks, markers,
  beats, and clip bodies past its own right edge, and a window wider than the
  guess faded an edge that had sequence beyond it. All of them now read the
  measured body width the fit and the playhead follow already used, which is one
  measurement for the panel and the state instead of two that disagree.
- The panels share the projection instead of copying it. The timeline, the media
  rail, and the assistant each deep-copied the projected store — every track,
  every clip, every keyframe, and the whole pool — once per render, because a
  panel cannot hold a borrow of the state across the listeners it attaches. They
  now take a handle to it, and a write takes the store back, so a frame that is
  still drawing keeps what it measured.
- Playback publishes a shared project. Starting playback copied the whole
  project, the audio system copied it again whenever it rebuilt the stream, and
  every command applied while playing repeated both; the copy is now made once
  and shared. The mixer thread is handed the sample it starts at rather than the
  snapshot, so a playing project is no longer held alive by the mixer for the
  length of the playback.
- The timeline zoom has one writer. Four call sites — a key, a menu row, the
  wheel, and the zoom tool — each wrote the state's zoom and the projection's
  copy by hand and spelled the 0.1 to 16 range out again; they call one method
  that clamps and writes both, and the range, the key notch, and the wheel notch
  are named constants beside the fit that reads them.
- Clip volume reads and steps in decibels. The inspector showed a raw
  percentage and nudged it by a flat 0.05 of the multiplier, which is inaudible
  at the top of the range and a cliff at the bottom; the row now reads
  `-6.0 dB · 50%` and steps by 1 dB, sharing the conversion with normalisation
  and export headroom rather than spelling it out again. The bottom of the
  travel is silence: a step under −60 dB mutes, and a step up from silence lands
  on −60 dB instead of seventy steps under unity.
- A finished job re-reads the engine once. The job subscriber woke a repaint per
  transition and never re-projected, so work that changed what the engine holds
  — a written proxy installs itself as a substitution — was invisible until the
  next edit. Only the terminal transition re-reads: rebuilding the projection on
  every progress tick would cost one per frame of a two-minute export.
- One desktop binary. The editor crate carried a second entry point of its own
  that named a retired frontend and set up no tracing; `aria-desktop` is the only
  binary now.
- `aria-desktop` opens the paths it is given. `aria-desktop path/to/video.mp4`
  was documented and read by nothing; a project, media, a LUT, or a skill file
  named on the command line now goes through the same owner that handles a drop
  on the Home screen, so an argument and a drop cannot diverge. `ARIA_DATA_DIR`
  chooses the profile directory for a scratch run.
- The Python API design document separates what ships from what does not. It
  described a retired v1 plan, an open-questions list, and a text renderer that
  has rendered every field it doubted for some time; the planned surface is now
  one table of five members with the command each would need.
- Element ids and hover keys are hashed rather than formatted, so drawing a
  frame no longer allocates a string per row, tile, tab, keyframe, and marker on
  screen. Identity is unchanged: the same row keeps the same id across frames.
- The animation browser says when a preset has no renderer. Whether an id
  animates anything is answered by the sampler that would run it, so a catalog
  file that adds a preset nothing implements is labelled `(no renderer)` instead
  of looking like a working entry that quietly does nothing.
- Opening a project whose media cannot be resolved runs the same session open as
  any other, rather than a second copy of it that had drifted: one owner adopts a
  project as the open one, and the offline case only adds the refusal message.
- A Detect button says whether that detector has already run: the primary button
  when nothing is stored, a ghost with a clear beside it once there is, and no
  clear when there is nothing to clear. Which detectors hold results is recorded
  by the refresh that already has the index open, so a panel drawing three
  buttons no longer opens the database three times per frame.
- The Project Report is offered once there is something to report on, instead of
  opening a column of zeroes on an empty project.
- The Analysis panel reads the scalars it draws instead of cloning every moment
  row and the loudness contour once per frame.
- Dialogs and overlays are registered, the shipped ones included. Export,
  Settings, the palette, Jobs, the context menu, and the rename bar are entries
  in the same host a plugin adds to, and a plugin's own dialog opens through the
  same call, so the shell names no dialog module.
- The preview, the assistant, and the Home screen are registered panels too. Each
  carries the state predicate that decides whether it draws, so the viewer and
  Home are slots a plugin can fill, and the shell holds no panel module name at
  all.
- Every text box in the app is one row in a table rather than its own block in
  the shell, so precedence between a rename and a search box is a position in a
  list and a new box costs no key handling. The sidebar tab strip is generated
  from the tab list for the same reason.
- Every panel the editor draws is registered, the shipped ones included. The media
  pool, the timeline, and the inspector are entries in the same host a plugin adds
  to, so a contributed panel is drawn by the code that draws a built-in and can
  replace one by registering its id.
- The effect Add list marks an entry passthrough according to the mounted effect
  service rather than the compositor the app was built with, so a session with no
  GPU plugin says so on every entry instead of promising passes nothing can
  dispatch, and a plugin that replaces the GPU catalog changes what is marked.
- Startup resolves the session before it opens a window. A failure names the cause
  and the data directory and exits, instead of starting on a blank session that
  discards the plugins that were mounted.
- A locked track refuses every edit that touches it, not just the two it happened
  to check before. Unlocking it is the one command that still goes through, and a
  refused edit names the lane and leaves the project and the history untouched.
- Dropping a file uses its type. A `.aria` file opens as a project wherever it is
  dropped, a `.cube` file becomes the selected clip's LUT, a `SKILL.md` file
  installs as an assistant skill, and anything else is imported as media.
- Plugins can refuse or rewrite a command before it is applied (`CommandProposed`)
  and claim a dropped file type (`ResolveDroppedPath`). Both replace the old
  single-slot `CommandInterceptorService`, which only one plugin could hold.
- **Settings > AI** shows the caps one assistant prompt is held to, read from the AI
  plugin rather than restated next to the display.
- Detector tuning belongs to the analysis plugin. A run reads it once when it starts,
  so a shot, silence, beat, or loudness pass cannot pick up different settings than
  the ones the plugin provides.
- Built-in plugins are grouped by what they provide — host, media, GPU, engine, and
  services — instead of one module. No behaviour change.
- Settings carries a **Plugins** section: every built-in plugin in mount order, its
  version, the services it registered, and — for one that did not mount — the reason.
  Anything but storage, settings, and the engine can be disabled and re-enabled.
- A plugin that fails to mount no longer stops the others. The app starts with the
  capabilities that did mount and names the ones that did not.
- The Speed tab draws the ramp as a curve. Dots are the ramp's own points: drag one
  to move it, click the graph to add one, right-click one to remove it. The value
  axis is logarithmic so 1x reads near the middle, and a **Length** line shows the
  clip length the ramp implies, updated while the pointer is down.
- **Shift selection…** takes a signed frame count and moves the selection on every
  lane it spans, from the earliest selected clip on each. An offset that would move
  a clip before the start of the sequence is refused whole.
- A track header can be dragged onto another header to move that lane to the row
  the pointer is released over. The target row is marked while the drag is live,
  Escape abandons it, and a drop is one edit.
- Colour and point effect parameters are editable. A colour parameter shows a
  palette with the current value named; a point parameter shows x and y steppers.
  Neither has a plain value in the project format, so a click writes a keyframe at
  the playhead, or at the clip's first frame when the playhead is elsewhere.
- The transition band on the timeline is interactive. Drag either edge to set the
  window length, and right-click it for Remove plus the blend catalog. Both act on
  that cut whatever is selected, and a drag is one edit.
- The preview draws crop and mask handles on the selected clip. Eight crop dots
  move an edge or a corner, and the preview shows the uncropped frame while the
  drag is live. Three mask dots move, resize, and rotate an applied mask. A drag is
  one edit whatever the pointer did, Escape abandons it, and a release that moved
  nothing writes nothing.
- The project canvas carries an explicit output size. Settings Canvas shows the
  size the project renders at and whether it is explicit or derived, and offers
  Derived plus one delivery baseline per row (720p through 4320p, the baseline
  being the shorter edge). Preview and export both use the explicit size over the
  aspect and the footage. Changing the aspect reshapes the size and keeps the
  baseline; `auto` clears it. A size the encoder cannot take is refused with the
  bounds named, and the agent's `set_canvas` gained the same field
  (`[0, 0]` derives the size again).
- Settings Caches lists every registered cache with its kind, tier, eviction order,
  and resolved path. Measure fills in sizes on demand, Clear empties one cache,
  Move carries its files to a new directory and records the path only once they
  arrive, Redirect points it elsewhere without moving, and Use default returns it
  to the storage root. Per-cache paths persist under `[storage.paths]`; a relative
  path or one shared with another cache is refused. Copy paths puts the whole
  resolved layout on the clipboard.
- Settings AI is editable in the app: source, base URL, model, key, key variable,
  protocol, reasoning summary, and autonomy. The status line is the assistant's own
  validation and names what is missing. A stored key is shown as its length. The
  agent directory is shown and can be revealed.
- Settings gained sections and a filter box, and the Appearance section gained an
  accent choice that retints the accent, playhead, and caret and persists in
  `[appearance]`.
- Every job that writes its result takes an ordered publication boundary first:
  an export's atomic publish, a stored transcript, and a stored moments batch.
  A cancel accepted before the boundary discards the work and names what was not
  written; one that arrives after it is refused, so a job never both publishes
  and reads as cancelled. A second publication from the same job is refused.
- Analysis: the sidebar's Analysis tab detects shots, silence, and beats on the
  selected clip as a cancellable job, and keeps the results in a moments index
  keyed on the media file. Rows carry a timecode and a confidence and filter by
  detector and by a confidence floor; Previous and Next walk them. Split at
  shots and Ripple-delete silence turn a detection into an edit, both undoable.
  Measure loudness draws the clip's contour with its peak and a clipping flag in
  the Audio tab, and detected beats draw on the ruler and act as snap magnets.
  Results from an older analyzer version are labelled stale and offer a re-run.
  A committed transcription writes into the same index. Settings > Storage
  reports the analysis cache and clears it.
- Captions: the sidebar's Captions tab transcribes the selected clip's audio
  offline and commits the transcript as text clips or markers, per segment or
  per word. Settings > Transcription installs a Whisper model as a job with
  byte progress; a model that fails verification reads as corrupt with the
  reason and offers a re-download. A run refuses by name before it starts when
  there is no selection, no audio track, no model, or an unknown language.
  The connect, read, and total download timeouts are configurable in the same
  section and persist to the `[transcription]` table in `config.toml`, clamped
  to a range the downloader accepts.
- Hovering an animation catalog entry previews it on the selected clip. The
  preview is a render override, not an edit: it creates no undo step, and
  leaving the row restores the clip.
- Jobs board (Window > Jobs) lists every background job with its progress,
  detail line, elapsed time, and outputs. A running job that still accepts
  cancellation offers Cancel; past the commit boundary the button is gone,
  and asking anyway reports that it is too late. Rows update by
  subscription, so an idle app does no work.
- Export runs as a job. The dialog no longer blocks on the render: it opens
  the board and reports `frame N/M`, and Cancel leaves the destination file
  untouched. The pass renders on its own headless renderer, so the preview
  stays live for the whole export.
- Cloud downloads report determinate progress against the file size and can
  be cancelled. A server that sends no `Content-Length` reports bytes
  transferred instead of a made-up percentage.
- A finished job's output path can be revealed: Reveal opens the containing
  folder in the platform file manager.
- Sidebar Generate tab sends a prompt to fal.ai and imports the result as
  media. Image, video, and voice are selectable; progress and cancellation
  are on the job row like any other background work. With no `fal` key the
  tab says which key to add instead of offering a button that fails.
- Assistant panel lists installed agent skills and browses the cloud skill
  catalog. Installing one writes `agent/skills/<id>/SKILL.md`, where the
  prompt builder already reads skills from. A download that does not parse
  as a skill is refused and installs nothing.

- Sidebar Stock tab searches stock video, photo, and audio. A configured
  Pexels or Pixabay key routes the query at the provider; without one it
  goes through the anonymous endpoint. The SFX tab lists the sound-effect
  catalog. A result downloads into the cache, is pinned against Clear
  cache, and imports into the pool. A failed feed reports the reason
  instead of showing an empty list.
- Cloud catalogs reach their surfaces: Look tab applies a cloud LUT to the
  selection, Home Samples downloads a cloud template and fills its slots,
  and Text tab loads a preset pack and applies a preset's font, size, fill,
  and animations to a title.
- Settings lists every BYOK provider with what it powers and where its key
  comes from, including the ones with no key. A key naming an unset
  environment variable counts as no key.
- Cache maintenance is held while a cloud download is in flight, so Clear
  and Move cannot delete the partial file mid-download.
- Help > Check for updates reports the published version against the
  installed one, and names the failure if the check cannot run.
- Sidebar browses the bundled catalogs: Stickers places a sticker, and
  Animations, Masks, and Transitions apply an entry to the selected clip.
- Catalog entries drag out of the sidebar: a sticker onto a lane at that
  time, an animation, mask, or transition onto a clip.
- Effects Add list is the model catalog. An effect with no GPU pass is
  offered and marked passthrough instead of being hidden.
- Settings Storage shows the download cache against its quota, edits the
  quota in 512 MiB steps, clears the cache naming what it removed and what
  was kept in use, and moves the storage root, carrying the cache with it.
- Text tab edits a title's content in place and carries the text effect
  presets. Retyping a title no longer discards its font, size, colour,
  alignment, stroke, shadow, background, or effect preset.
- Desktop launch crate is process bring-up only. Playback clock, file-drop
  placement, and the GPUI shell live in `aria-ui`.
- Aria opens on a full-window Home route. Ctrl+W returns there. Ctrl+O
  opens a project. Recents persist mtime, mark missing paths, and offer
  Relink.
- Editor chrome is a File/Edit/Sequence/View/Window/Help bar plus a tool
  strip. Home / Project breadcrumb, magnet snap, and Ripple/Overwrite
  live there. A/B/H/Z select tools. Esc returns to the pointer.
- J/K/L shuttle reverse, stop, and forward. Double-click a splitter
  resets that pane. Esc cancels an in-progress splitter drag.
- File Relink All and Sequence Add Marker (M). Palette type-to-filter.
  Context menus open at the click. Locked clips refuse move and trim.
  Export shows the canvas pixel size.
- In and Out points: I and O set them at the playhead, Shift+I and Shift+O
  seek to them, Alt+I and Alt+O clear them. They cannot be left inverted.
- Q and W trim the head and the tail of the selected clip to the playhead,
  and neither can collapse a clip to nothing.
- Delete follows the edit mode. Ripple closes the gap, Overwrite leaves it.
  Cut, the palette, and the clip context menu all take the same path.
- Clicking a clip selects everything linked to it. Alt+click selects the
  clicked clip alone.
- Markers, In, and Out are magnets. Drag and trim snap to them, and the
  ruler draws a marker flag and the In and Out ticks, each seekable.
- Inspector Look tab browses the filter catalog, sets filter amount, and
  clears the filter.
- Home opens on a Start section: New Project, Open, and Stills sequence.
- Markers carry a name and a colour, are deletable at the playhead (Alt+M),
  navigable (Shift+M, Shift+Alt+M), renameable, movable, and recolourable.
- Extract Audio, Freeze Frame, Link Clips, and Unlink Clips reach the clip
  context menu, the Sequence menu, and the palette.
- Clip menus are gated on engine-computed capabilities, so a surface never
  offers an action the command layer would refuse.
- Inline rename for markers, tracks, and the project. Typing is key-routed
  like the palette, Enter applies, Esc cancels, and an empty or unchanged
  draft makes no undo entry.
- Inspector Look tab carries the grade: eleven colour adjustments with
  reset, every blend mode, three stabilization levels plus Off, and a
  denoise toggle. The Audio tab toggles pitch lock.
- Transform tab carries framing: crop insets on all four edges that clamp at
  the frame, Flip H and Flip V, Reset framing, and motion blur with shutter
  angle and sample count.
- Look tab carries masks and the chroma key: the six mask shapes with
  feather, roundness, rotation, size, offset, and invert, plus a green, blue,
  or cyan key with strength and shadow. Editing one knob keeps the rest.
- The palette and the Sequence menu reach the new look, framing, mask, and
  chroma commands.
- Look tab carries layer styles and LUTs: shadow, glow, outline, and
  background with their knobs and colours, plus a `.cube` LUT with an
  intensity blend. Editing one style block keeps the other three.
- Look tab carries clip animations: In, Out, and Loop slots with the catalog
  presets for each, and the Speed, Intensity, and Stagger knobs the applied
  preset exposes. Text-only presets appear on text clips only.
- Audio tab tags a clip Music, Sound FX, Voiceover, or Extracted.
- Effects tab reorders the chain with ↑ and ↓ and steps each effect's numeric
  parameters within the range the effect declares.
- Look tab carries transitions at a cut: the blend catalog, the window length
  in ticks, and Remove. The block appears only on a clip that abuts another,
  and the timeline draws the window across the cut.
- Info tab replaces a clip's media from the pool, keeping placement, transform,
  and effects, and carries the template marks: a replaceable slot with fill
  order, media restriction, and placeholder hint, plus text editability on a
  text clip. The timeline paints a badge on a marked slot.
- Transform tab writes, removes, walks, and flattens keyframes on position,
  scale, rotation, opacity, volume, and pan, and applies the named bezier and
  the bounce/elastic/back easing presets to the keyframe under the playhead. An
  expanded clip marks its keyframe ticks.
- New Speed tab on media clips: the built-in ramp presets, the ramp's start,
  middle, and end multipliers, and Clear ramp. The active preset is matched by
  the ramp's shape.
- Tracks can be soloed. A solo silences the other audio tracks, or drops the
  other video tracks out of the composite, in preview and export alike; it never
  crosses between audio and video, and never overrides a track's own mute.
- Track context menu moves a track up or down the stack, marks an audio track as
  the auto-duck source, and opens or closes a one-second gap at the playhead.
- A drop onto the timeline in Ripple mode inserts and pushes the clips after it
  right, instead of being refused for overlapping them.
- Settings exposes the project canvas aspect while a project is open. Preview and
  export follow it, and the change is undoable.
- Pool items can be removed from the project. A removal is refused with a reason
  while a clip still references the item.
- File menu saves the current project as a `.ariat` template bundle and applies
  one, asking for media per slot in fill order using each slot's hint as the
  prompt. Home carries a Use Template tile.
- Hand tool pans the timeline. Zoom tool click zooms around the pointer.
  Open menus and dialogs use frost.
- Recents fade at the scroll edge. Sidebar offers Relink All when any
  media is missing. Context menus stay inside the window.
- Space plays from elapsed wall time. Slow preview frames drop instead of
  stretching the playhead.
- Dragging several files onto the window places them end-to-end.
- Missing media opens with Relink instead of failing closed.
- Titles and Shapes tabs place a generator at the playhead.
- Linux decode, probe, and H.264/AAC encode use host GStreamer plugins.
- Linux preview prefers LINEAR NV12 DMABuf import into Vulkan. Tiled
  modifiers fall back to CPU NV12.
- Preview audio plays through cpal when the timeline has audible clips.
- Reversed clips mix backward in preview and export.
- Inspector Audio tab edits volume, pan, fades, and reverse.
- F, the command palette, and the preview context menu fit the timeline
  to the sequence. Titlebar, transport, and track headers use the shipped
  Lucide icons instead of letter placeholders.

### Fixed

- A pool row and the properties block state a source's length in the sequence timecode, so a 2 s WAV reads 00:00:02:00 instead of 00:01:23:08, and the Duration sort orders by time across frame rates.
- The properties block no longer states a frame rate for an audio-only source.
- A parameter row in a mixer strip moves its value under the label when both do not fit, instead of pushing the value past the strip edge.
- Shift+6 opens the mixer and Shift+, ripple-inserts on Linux, where the keystroke arrives as `^` or `<`, and a Shift chord on a digit or punctuation key can be captured in Settings.
- A menu drop draws over the tool strip and the panels, and a press outside the menu bar closes it.
- A text box types each character once, and the command palette types the character a key produces, so Shift+F types `F`.
- Ripple delete now closes the deleted span on every sync-locked lane instead of failing, so sound under the removed shot goes with it and the rest of the lane moves up.
- Undoing a combo animation now restores the entrance and exit animations it replaced instead of discarding them.
- Changing a sequence frame rate now resamples clip keyframe ticks, so an animation keeps its wall-clock timing instead of sliding by the ratio of the old and new rates.
- An effect instance with the wrong number of parameters now reports the count its catalog row declares rather than the size of the GPU uniform block.
- A download whose stream stalls mid-body now fails on the read timeout in a bounded time instead of leaving a test thread waiting on a connection that was refused before it was made.

- **Splitting a title no longer slides its background card.** Every clip-relative
  curve travels with the clip through one walk, except that generated content had
  its own second list: the text rebase named the size, the fill, the spacing, the
  outline and the shadow, and not the card. A title whose card colour or corner
  radius animated kept that animation where the split found it, so the card ran
  against the words it sits behind, and a rate change had a third list again.
  Generated content is now walked with everything else, which is the mechanism
  that made the equivalent defect in the grade, the mask and the LUT impossible:
  one list per carrier, visited by both the rebase and the resample. The rebase
  is atomic across generated content too, so a title that cannot move that far
  is left exactly as it was instead of half rebased. A generator that paints
  something new does not compile until it says which of its parameters travel
  with the clip, and the walk census now animates a title and a star from the
  source's own field list.

- **The test suite stopped hanging.** `cargo test -p aria-ui --lib` froze part
  way through and had to be killed by a timeout, and on the runs it did finish
  four scratch-directory tests failed for no reason a reader could see. Both
  were one design defect: the five shared scratch slots were a set of statics, so
  the tests that prove what happens to a *full* pool had to fill the pool the
  other 1400 tests were queueing on. Every one of them then waited, several hit
  the deadlock bound and failed, and when one of the filling test's own threads
  lost the race for a slot and panicked at that bound, the untimed barrier it
  never reached held the whole run until the runner killed it. A pool is a value
  now: a test about a full pool builds its own, with its own root and its own
  bound, and the shared pool is one instance of the same type. The rules that
  needed a barrier are stated as a receive that must time out and a receive that
  must not, so nothing in them can hang, and a rule read out of the sources
  counts the one test allowed to claim a shared slot from a thread it spawned.

- **A busy test suite is no longer mistaken for a stuck one.** The five shared
  scratch slots bounded the wait for one by the waiting thread's own clock, so a
  test that queued for two minutes behind holders that were each baking a few
  seconds of video was declared a deadlock and failed. A run of 1500 tests
  through five slots queues by design: on the run that exposed this, 1497 tests
  passed and 5 failed at the bound with nothing stuck anywhere. What separates a
  queue from a deadlock is whether slots are coming back at all, so the bound is
  on that now, and a real deadlock still fails by name inside it rather than
  hanging the run.

- **The projection's scaling test measures the algorithm, not the machine.** It
  compared a sequence against one twice the size and allowed the larger to cost
  up to three times as much per clip, which failed once in a full run and could
  not see a quadratic pass even when one was inserted on purpose: eight times
  the data means eight times the working set, and that alone moved the number as
  much as a real regression would. It now compares one sequence of 3840 clips
  against eight separate sequences of 480, which is the same number of clip
  projections either way, so the cache, the allocator and whatever else is
  running on the machine cancel out of the ratio. A projection whose cost
  follows the clip count reads 1.02x to 1.13x; a pass that walks the sequence
  once per clip reads 2.04x and fails at the 1.4x bound.

- **Every message that carries an image is pruned by the image budget.** Three
  readers and the history collector each matched the two message kinds that
  carry attachments and ended in a catch-all, so a fourth kind that carried one
  would have been counted nowhere, pruned nowhere and stripped nowhere: a vision
  turn would have gone over its budget with no placeholder saying what was
  dropped, and the session file would have kept raw frames it is supposed to hold
  as labels. The message type answers which of its kinds carry attachments, once
  and exhaustively, so a new kind does not compile until it says.

- **A plan is checked against the plan, not against the project.** A plan-only
  turn validated every step against the project as it stood, so a step that
  depended on an earlier step of the same plan was refused for a reason applying
  the plan would never hit: adding an effect and then setting one of its
  parameters reported that the clip has no effect chain. A dry run now applies
  each step into the turn's own history group and takes that group back before it
  returns, so every step is checked against the plan so far and nothing is left
  behind. A step that names a clip, a track or a marker an earlier step created
  is pointed at the id the apply produced. A render inside a plan is checked and
  not started, because the file it would write is not what a later step reads. A
  turn that takes its group back no longer leaves a saved project marked unsaved,
  which also covers an agent turn that stops halfway.

- **Every effect is in the palette.** An effect could only be added by clicking:
  the rack's Add row and the sidebar catalog are pointer surfaces, and the
  palette built its offer from two compiled registries while the effect catalog
  is data read at run time, so nothing on the keyboard reached it. The palette
  now offers every effect in the catalog by its own name, derived from the
  catalog on each read, so an effect added to the data file is searchable the
  moment it loads and none of them is written down twice. Running the row adds
  the effect to the selection; running it with nothing selected says so.

- **A command that finds nothing to act on says so.** Adding a cross fade right
  after a split applied nothing and reported nothing: a split leaves the right
  half of the cut selected, that half's own junction abuts nothing, and the
  command returned quietly. From the operator's side the editor had ignored a
  key. Every refusal in the transition family now names itself in the status
  line: nothing selected, no clip on the other side of that cut, no such
  transition, no transition at that cut. The command still succeeds, because
  declining is not a failure, and a drag that ends where it started still says
  nothing, because it found what it was looking for. The selection every such
  command acts on is read in one place, so the next one cannot be silent by
  accident.

- **Enter answers a dialog.** Ctrl+E opened the export sheet and the keyboard
  could do nothing with it: Enter was consumed only by a focused text box and by
  the palette, so the sheet sat there until a pointer found the Export button,
  and the unsaved-changes prompt standing between an hour of edits and a new
  project needed one too. Each dialog now declares what Enter does. Enter on the
  export sheet queues the export the sheet describes, at the destination it
  prints, and Enter on the unsaved prompt saves and continues. A dialog that has
  no single action to commit, the settings sheet, the jobs board, the history
  list and the project report, says so and swallows the key rather than letting
  it reach the timeline behind it. The Export button and Enter now press the same
  call, so they cannot queue two different exports.

- **A relative profile directory starts the editor instead of killing it.**
  `ARIA_DATA_DIR=scratch/profile` mounted every plugin, opened the GPU device,
  created the window, and then panicked on the first storage layout, because the
  layout refuses a relative root and the reader asserted the directory was
  absolute rather than resolving it. The profile root is now resolved once where
  the session is built, against the working directory, so the desktop app, the
  library entry point and the tests all get the same directory, and the settings
  file the first frame reads is the one under it.

- **A rendered clip's swap goes past the plugin boundary like every other edit.**
  Render And Replace imported the rendered file and swapped it in by applying
  both commands straight to the engine, because the swap is the render job's last
  act and there is no editor state to hold at that point. A plugin that rewrites
  or refuses an edit therefore never saw either one, and neither did anything
  else watching what the editor asks for. Both commands are now proposed on the
  plugin bus first, on the job thread and on the already-rendered path alike, and
  a refusal rolls the group back instead of half-applying it.

- **The curve editor draws the curve.** The graph painted one flat bar per
  sample pair, two pixels tall, parked at the pair's mean value, so a linear
  ramp read as a staircase and a steep ease read as a single step — on the one
  surface that exists to show an operator what an ease does. A segment is drawn
  as pixel-wide columns spanning the value the curve crosses in that column, so
  a slope is a line at any angle, a curve that runs outside the graph is a line
  along the edge it left by, and the drawn columns are asserted against the
  curve's own samples at the same x.

- **The multicam panel follows the playhead.** The live angle was resolved when
  the projection was built, which happens on an edit, so scrubbing past a cut
  left the sidebar highlighting the camera that was live when the cut was made.
  The cut list is projected instead and the angle is derived per read, from the
  one function the model resolves it with.

- **Editing inside a nested sequence lands in that sequence.** Placing a clip
  addressed the timeline the operator had open, while adding a lane, a marker,
  a look, a transition, a canvas or a sequence rate addressed the project's root
  timeline. So a lane created inside an open sequence appeared nowhere, and the
  clip dropped on it was refused as an unknown track: half of nested-sequence
  editing was unreachable. `Project::timeline` now means the timeline being
  edited, `Project::root_timeline` names the root for the few readers that mean
  it, and every editing surface — the projection the window draws, the preview
  the renderer resolves, the assistant's descriptions — follows what is open.
  `no_edit_touches_the_root_timeline_while_a_sequence_is_open` walks the command
  census and holds the root byte-identical, so a command added later is covered
  by existing rather than by being remembered.
- **A drop that lands inside a clip inserts.** A ripple insert shifts clips by
  start tick, so an insert landing halfway through a shot moved nothing and was
  then refused for overlapping the clip it landed in — an operator dragging a
  pool item into the middle of a clip got an error. The lane is cut at the
  insertion tick first, on the target lane and on every sync-locked lane that
  rides with it, and the cut is rejoined by the same single undo that removes
  the clip and closes the hole.
- **A still is not 1000 fps footage.** A still image is stored on a
  millisecond tick base so its duration math stays exact. The pool read that
  base as a shot cadence, so a project holding one PNG was told its footage ran
  at 1000 fps and the sequence settings showed a permanent mismatch warning.
  Only video sources report a cadence now, and a large still no longer outvotes
  the footage beside it.
- **A clip with no grade comes back byte for byte.** The CPU grade path is
  algebraically identity at neutral but not numerically: the luminance split
  alone rewrote `0.123` as `0.122999996`, so an ungraded pixel differed from a
  never-graded one. A neutral grade now returns its input untouched, which is
  also the cheapest path through the chain — the whole thing is skipped.
- **A frame no longer waits for a decode.** The engine sits behind one mutex and
  a preview decode holds it for the whole decode, so every value a panel read by
  locking the session was a frame that could park behind a seek. The editor read
  about fifteen of them per repaint: the canvas size three times in the settings
  dialog, the undo list twice in the history panel, the export sheet's output
  size twice through a full export resolve, the proxy flag once per pool row, and
  every inspector parameter once per widget — plus a raw `engine().lock()` inside
  the export sheet's drawing code. All of it is projected now, built from the
  same engine read that builds the store and keyed by the same projection key, so
  a drawn value cannot disagree with the store beside it: session facts in
  `app::project_facts`, the selected clip's model and its resolved quad in
  `app::selected_detail`, and the report's counts in `app::project_report`. A
  frame's stage timings are recorded where the frame lands, under the lock that
  produced it, so the render-cost panel and the report's stage table follow a
  scrub without asking. The report's counts are walks of the whole project, so
  they are taken when the report is opened and again on any edit that lands while
  it stays open; an operator with it closed pays nothing. What may still take the
  session is declared with a reason and fenced by `engine_lock_census`, which
  parses every `&self` method in the crate, follows calls to a fixed point, and
  fails on a lock without a row — the export sheet's size row reached the session
  through two clean-looking calls, and a scan that only looked for the literal
  lock would have called it innocent.
- **A click on the ruler lands where the pointer is.** Every pointer-to-tick
  conversion in the timeline subtracted the track-header width and nothing else,
  which assumes the panel starts at the window's left edge. It never does: the
  media rail and its splitter sit to its left. So a ruler click, a lane marquee
  and a wheel zoom all landed the rail's whole width further into the sequence —
  about three seconds at the default zoom on a 256 px rail. Where the timeline
  was drawn is now measured (`AppState::timeline_frame`, recorded by the panel
  itself) and one function, `panels::timeline::pointer_ticks`, converts against
  it. A panel that has not been painted yet converts nothing rather than
  converting against a guessed origin.
- **The editor draws.** Seven render paths fetched the app state by reading the
  shell entity back off the context — `cx.entity().read(cx)` — while gpui had
  that same entity leased for the render in progress, which is an immediate
  panic. The timeline ruler's marker row and the inspector's animation,
  keyframe, speed, template, info and parameter blocks all did it, so the editor
  route died on the first frame that drew any of them. Each one takes the state
  as a parameter now, the way the inspector's tab bodies already did. The shell
  tests missed it because they never mounted the UI plugin: with no panels
  registered they were asserting that an empty frame does not panic. They mount
  it, they notify after an edit, and they now drive the pointer and the keyboard
  — a ruler click, a splitter drag with its Escape, and the selection, step,
  marker, mark, trim, delete and undo chords.
- **The Python bindings compile, and a script's own path is tested.** `aria-py`
  had rotted out of the tree: it was covered only by a pytest suite that needs a
  maturin wheel, so nothing noticed when the catalogs started handing out owned
  ids or when every text metric became keyframable, and the crate no longer
  built at all. It builds again — `size`, `letter_spacing`, `line_spacing`, the
  fill and the three text decorations go in as constant animatable values, and
  `repr(Text(…))` says `size=animated` rather than printing one keyframe's value
  as if it were the clip's — and it is its own workspace root, which is what
  stopped `cargo test` from running there at all. Twenty Rust tests now drive
  the real module the way `import aria` does: a descriptor for every shape the
  engine knows, a placed clip, a split whose halves meet, a handle to a removed
  track that reports instead of panicking, an unknown sticker that lists what
  could have been asked for, an unknown canvas refused before a project exists,
  and a shape on a picture lane that names the lane it was put on. A shape the
  engine gains with no descriptor behind it stops the crate compiling.
- **A wait never holds a thread the window needs.** Twelve places spawned a
  thread or subscribed to a channel and then waited for it on the background
  executor — the preview decode, the filmstrip, the pool thumbnail, the
  waveform, the job board, the picker, the assistant's beats, and every
  analysis job — so a scheduler thread sat blocked for the length of a decode
  or, for a wait that never arrives, for good. The shell's own integration
  suite could not run at all: building a window parked every thread and spent
  no CPU, because the job watcher was blocking on a job that never started.
  Waiting now belongs to `aria_ui::handoff`: work that blocks runs on a named
  thread of its own and reports back as a future, a wake from another thread
  arrives as a message a task awaits, and a panic in that work costs the frame
  rather than the window. Two rules derived from the sources refuse a blocking
  call inside a task and an undeclared thread.
- **The test suite cannot open a window on your desktop.** Two modules
  spawned a file manager of their own — the jobs dialog's "Reveal" and Home's
  recents — so a run of the menu tests, which exercises every menu row,
  executed "Open data dir" and opened a folder on whatever machine was running
  the suite. Starting another application is now one module's job
  (`aria_ui::app::launch`), off until the headed editor switches it on at
  startup: anything else records the folder it would have shown, so a reveal
  can be asserted without a window appearing. A rule derived from the sources
  fails the suite if a second spawn site is ever added.
- **The design gallery stopped hiding components from its own census.** The
  check that every `components::` entry has a specimen read a declaration up to
  its first bracket, so a generic component was recorded under a name carrying
  its type parameters and could never be matched — `dialog_chrome` counted as
  covered by a needle that matched nothing, and `chip` had no specimen at all in
  either size. The parser now cuts generics with the arguments, the gallery draws
  a chip row per `ChipSize`, and the motion section plots the motion catalog
  itself instead of a hand-kept list that had already fallen a spec behind.
- **L twice is 4x.** The shuttle ladder doubled only when the pressed rate
  matched the rate already showing, so the second L fell back to 1x: the
  keyboard could reach 2x and nothing beyond it, in either direction. A press
  now doubles whatever is playing in the direction pressed, and the opposite key
  still turns the transport around at 1x.
- **Two renders at once no longer take the process down.** Building a headless
  GPU context loads the platform's driver stack, and two of them starting at the
  same moment raced inside the loader: on Linux that is a crash in
  `vkCreateInstance` with no error to catch, or an EGL `BadAccess` from the
  fallback backend. Contexts are now built one at a time.
- A test-only class of false failures in the model layer: a "missing" id written
  as a small literal (`TrackId::from_raw(77)`) named a real object once a test
  binary had allocated that many ids, so a rejection test asserted against a live
  track. Ids for absent objects now come from the allocator itself.

- Moving a Transform slider on an animated clip wrote the property as a
  constant, deleting every keyframe on it. It now writes the keyframe under the
  playhead; Reset is the one path that clears the animation.
- The same defect in every block: a mask feather, a chroma tolerance, a layer
  shadow offset, or a colour adjustment was written by rebuilding the whole
  block — `SetClipMask`, `SetClipChroma`, `SetClipLayerStyles`,
  `SetClipAdjustments` — from what the panel could see, which is one constant
  per property, so every other animated property in that block lost its curve on
  each nudge. Rows now address the single property through one write path.
- Opening a file dialog froze the window. Every pick ran the dialog on the draw
  thread, so the editor stopped redrawing until the operator answered, and a
  cancelled dialog could leave the window unpainted. This covered open, save as,
  import, relink, LUT, template, export destination, cache and storage folders,
  and the confirmation before deleting a recent project.
  Picks are now asked on the background executor and applied to the session when
  the answer arrives, and the preview refreshes on the picks that change what is
  under the playhead.
  Blocking dialog code is gone from the editor: `rfd` lives behind the
  `aria-dialog` crate, which exposes only `async fn`, so no editor module can
  reach a blocking dialog. Every command and menu row that asks for a path is
  now driven in tests with a supplied answer, including the nine that were
  excluded from the reach census for opening a dialog.

## [alpha-0.7.0] — 2026-07-22

### Added

- **Blend modes.** Every visual clip has a blend mode dropdown (multiply,
  screen, overlay, darken, lighten, and friends) composited on the GPU.

- **Layer styles.** Drop shadow, glow, outline, and background plate on any
  visual clip — video, text, shapes, stickers. Every style property is
  animatable, with live preview while dragging sliders.

- **Keyframe graph editor.** A drawer under the timeline plots parameter
  curves; drag keyframes, insert and delete them, and shape segment easing
  with bezier handles.

- **Motion paths.** Position keyframes take spatial bezier tangents, so
  clips travel curves instead of straight lines. The path draws on the
  preview canvas with draggable points and tangent handles.

- **Motion blur.** Per-clip motion blur supersamples animated transforms
  in the compositor.

- **Per-character text animation.** Character-cascade presets rendered
  through a new GPU glyph atlas with an instanced text pipeline, plus
  speed / intensity / stagger knobs on all animation presets.

- **Mask and chroma key inspectors.** Clip masks gained animatable geometry
  (position, size, rotation, roundness); mask and chroma sections are now
  editable in the clip inspector.

- **Six new color adjust sliders.** Tint, hue, highlights, shadows,
  sharpness, and vignette, alongside the existing grade controls.

- **Animatable crop.** Crop rectangles interpolate between keyframes.

- **Per-axis scale.** Scale X and Y independently from the transform
  inspector (old uniform-scale projects load unchanged).

- **Audio pan.** Animatable constant-power pan in the shared preview /
  export mixer.

- **More easing.** Hold (step) easing, plus named presets: snappy,
  overshoot, anticipate, bounce, elastic, and back.

- **Typed effect parameters.** Effects can take color and vec2 parameters
  (not just scalars), with new duotone and color-overlay effect passes.

### Changed

- **Almost everything is keyframable.** Text style metrics, filter / LUT /
  adjust intensities, mask and chroma settings, and layer styles all route
  through the same keyframe system, with keyframe controls in every
  inspector row and full coverage on the AI wire.

- **AI setup is provider-based.** Configure any OpenAI-compatible provider
  (key, base URL, model) in settings; the old cloud account flow is gone.

- **Text backgrounds animate.** The text background card (color, padding,
  radius, opacity) is animatable and rendered in preview and export.

[alpha-0.7.0]: https://github.com/santhreal/aria/releases/tag/alpha-0.7.0
