# Architecture

Aria is a Rust workspace. The engine is UI-agnostic. The headed editor is
GPUI.

## Crates

| Crate | Role |
| --- | --- |
| `aria-core` | Time, color, pixels, decode/encode contracts |
| `aria-models` | Project, timeline, media pool, `.aria` schema |
| `aria-commands` | Edit and project command vocabulary |
| `aria-decoder` / `aria-encoder` | Platform-native media |
| `aria-compositor` | wgpu passes (effects, transitions) |
| `aria-text` / `aria-shapes` | Generators |
| `aria-render` | Scene resolve, frames, audio mix, export |
| `aria-engine` | Session, apply, undo, preview frames |
| `aria-plugin` | `PluginContext`, services, event bus |
| `aria-ui` | GPUI shell, panels, theme, motion, built-in plugins |
| `aria-settings` | `~/.aria/config.toml` |
| `aria-dialog` | Native file and message dialogs, `async` only |
| `aria-storage` / `aria-jobs` / `aria-cloud` | Cache, jobs, catalog |
| `aria-ai` / `aria-analysis` / `aria-transcription` | Assistant and analysis |
| `aria-detect` | Decode-to-detector seam: analysis PCM, sampled frames |
| `aria-cli` / `aria-mobile` / `aria-py` | Headless and bindings |
| `apps/aria-desktop` | Headed binary: mount plugins, `run_app` |

## UI boundary

`apps/aria-desktop` starts the process. `aria-ui` owns the window.

```
data dir (ARIA_DATA_DIR, else the platform default)
    ├─ mount_all (17 built-in plugins, then <data dir>/plugins)
    └─ aria_ui::run_app_with
           └─ AppState (engine session + UiEditorStore)
                  └─ Shell (Home route, or titlebar + four panels)
```

The data directory is resolved once and passed to both: the mount reads catalog
overrides and external plugins from it, and the session writes settings, caches
and the recovery file into it. A start is measured against the boot cover's
ceiling by `cargo test -p aria-desktop first_interactive_frame`; the numbers are
in `docs/performance.md`.

`projection::project_to_ui` maps `aria_models::Project` onto `UiEditorStore`.
Panels never hold engine types. They call `AppState` methods, which apply
`Command::Edit` / `Command::Project` and re-project.

A frame takes no session lock. The engine sits behind one mutex and
`Engine::get_frame` holds it for a whole decode, so a value fetched from the
session while drawing is a frame that parks behind a seek. Everything a panel
reads is projected from the same engine read that builds the store: session
facts in `app::project_facts` (canvas, render size, pool items in use, undo
list, marker count), the selected clip's model and resolved quad in
`app::selected_detail`, the report's counts in `app::project_report`, and a
frame's stage timings recorded in `commit_preview` under the lock that produced
the frame.

A poll compares the inputs the projection reads against the key the current
store was built from (`ProjectionKey` in `app/selection.rs`). Nothing moved: the
poll returns. Only the playhead moved: the store keeps its projection, because
the projection carries the tick as a value and derives nothing from it, and the
poll re-samples what is read at the playhead instead. That is every frame of
playback and every frame of a scrub, so the walk of every track, clip and
keyframe plus the magnet index rebuild happen on edits, not on transport.
Anything else: a full re-projection. `app/scrub_stale_tests.rs` holds the two
halves of that claim, both without naming a field: a projection at two ticks may
differ in `playhead_ticks` and nowhere else, and what a scrub leaves must equal a
full re-projection at the same tick.

What may still take the session is declared, with a reason, in
`app/engine_lock_tests.rs`: a report the operator asked for, a job about to
start, a pointer hit test, a republish to the audio engine. `engine_lock_census`
parses every `&self` method in the crate, follows calls to a fixed point, and
fails on a lock without a row.

Preview: `Engine::get_frame` returns CPU `RgbaImage`. `frame::rgba_to_render`
swaps RGBA to BGRA `RenderImage` for GPUI `img`.

Playback is wall-clock when preview audio is silent:
`transport::playback_tick` computes the playhead from elapsed time so a
slow preview frame drops instead of stretching time. When the timeline
has audible clips, `AudioSystem` (cpal, UI thread) drives the playhead
from mixed PCM and `advance_playhead` prefers that clock.
File drops use `os_drop::plan_sequential_starts` so multiple files land
end-to-end on the matching lane.

Linux decode and encode wrap host GStreamer plugins behind the same
`VideoDecoder` / `AudioReader` / `VideoEncoder` traits as AVFoundation
and Media Foundation. Aria never ships a codec.

### Waiting for work off the window's thread

`handoff` owns every wait. `handoff::on_a_thread(label, work)` runs `work` on a
named thread and returns a future; `handoff::wakes()` gives a writer that never
blocks and a reader a task awaits. A panic in `work` closes the handoff, so the
caller reads `None` and the window keeps drawing.

Nothing in `aria-ui` calls a blocking `recv()`, `recv_timeout()`, or `join()`
inside a task. A task runs on a scheduler thread, and under the test scheduler
that thread is the one ticking the run loop: a blocking wait there stops the
application instead of costing a frame. Two rules in `test_support` are derived
from the sources — one refuses a blocking call inside a `background_executor()`
expression anywhere in the workspace, the other refuses a thread started outside
the modules that declare why they need one.

### Asking for a path

`aria-dialog` is the only crate that depends on `rfd`, and it exposes `async fn`
only. No editor module can open a blocking dialog, because there is no blocking
dialog in the dependency graph of `aria-ui`.

A caller asks through `AppState::request_pick(ask, then)`. `Ask` names the
dialog and `then` says what to do with the `Answer`. The request goes to
`watch_picks`, which runs the dialog on the background executor, applies `then`
to the session when the answer arrives, and refreshes the preview. Cancelling is
an answer: `then` runs with no paths, so the cancel path is code that ran rather
than code that was skipped.

Tests answer in advance. `AppState::supply_pick(answer)` queues one answer, and
a request that finds a queued answer applies it in place without reaching the
executor. `picks_asked()` counts the requests a row made and `picks_unused()`
counts the answers nobody took.

### Decoder output modes

`aria_decoder::decoder_capability()` answers what the platform can deliver
before a file is opened, so a caller picks a mode instead of reading a refusal.

| Platform | Backend | GPU surfaces | CPU planes |
| --- | --- | --- | --- |
| macOS / iOS | AVFoundation | IOSurface `CVPixelBuffer` | yes |
| Windows | Media Foundation | shared D3D11 NV12 | yes |
| Linux | GStreamer | LINEAR NV12 DMA-BUF | yes |
| Android | MediaCodec | no | yes |

Preview asks for `OutputMode::Gpu`. On Linux a LINEAR NV12 DMABuf imports into
Vulkan; a tiled modifier cannot be imported by wgpu-hal, so decode falls back to
CPU NV12 and the modifier is named once per session in the log.

Android is a CPU-planes platform: MediaCodec runs in ByteBuffer mode, there is no
`AImageReader` surface, and so no `AHardwareBuffer` for the compositor to import.
Asking for `OutputMode::Gpu` there is refused at open rather than part way
through a decode. That is the shipped path, not a temporary state of the build.

MediaCodec and Media Foundation accept only pixel layouts with declared plane
mappings. An unknown output format stops decode before its bytes reach the
compositor.

## Command reach

`aria_commands::every_edit_command()` and `every_project_command()` are the
command census: one constructed sample per variant, library code rather than a
test fixture because completeness is the contract. The wire-format lock pins
their length and its exhaustive variant-name matches refuse to compile when a
variant is added, so a command cannot exist without appearing in the census.

`EditCommand::reach()` and `ProjectCommand::reach()` are exhaustive and return
one of three declarations. `Reach::Operator(surface)` names the control an
operator uses: a palette row, a menu item, a chord, an inspector block, a
context-menu item, a timeline gesture. `Reach::Planned(missing)` says nothing
sends it yet and names the work that blocks it, plus the ledger row that owns
that work. `Reach::Internal(reason)` says it never gets a control, because
another command covers the intent with more cases handled or because the caller
is the assistant, a plugin, or the CLI.

Naming the surface is the point of the payload: a command cannot be declared
reachable without someone writing down which control sends it, which is the
moment a missing control is noticed. `AddClip` is `Internal`, because a drop
resolves through `OverwriteClip` or `RippleInsert`, which also decide what
happens to what is already there.

The editor measures the other direction. Every operator surface that can be
driven headlessly is driven, and what it produces is recorded:

| Surface | Harness | What is measured |
| --- | --- | --- |
| Plain palette rows | `app/reach_tests.rs` | the history labels each row writes, and every command it dispatches |
| Menu rows, all six menus | `app/menu_reach_tests.rs` | the same, through a headless `TestAppContext` |
| Windowed palette rows | `app/windowed_reach_tests.rs` | a queued job, or a refusal in words |
| Shipped chords | `app/operator_surface_tests.rs` | every command each plain chord dispatches |

Each harness declares every row as editing or not editing, and checks the
declaration against the live table in both directions: a row added without a
decision fails the suite, and an exemption that names a row which no longer
exists fails too. A row that neither edits nor is declared inert is a dead
control, which is the class these close.

`app/operator_surface_tests.rs` closes the forward direction. Dispatch is
observed at `apply_command` through a `CommandProposed` subscription rather than
read out of the history list, because saving, opening and exporting ask for a
command and write no history entry. A command a driven table sends may not be
declared `Planned` or `Internal`, and a row whose surface text names the palette
or a chord has to be sent by one. A row that names a drawn control — an
inspector block, a context-menu item, an on-canvas handle — is a statement a
reader checks: gpui keeps a drawn element's action in a closure, so nothing can
enumerate those surfaces. Ledger row 140 is the registry that would.

A row that asks for a path is driven like any other. The pick is supplied to the
state before the row runs, so the row applies it and the test asserts the model
change. The harnesses ask the state how many picks a row requested, so a new row
that opens a dialog and is not driven with an answer fails the suite.

`gpui`'s `test-support` feature is a dev-dependency of `aria-ui` for this. A
menu row takes a `&mut gpui::Context<AppState>`, so before it existed the bar
was the largest surface in the product with no coverage at all.

## Plugin system

See [plugin-development.md](plugin-development.md). `aria-plugin` already
depends on compositor and render, so plugin impls that touch GPU live in
`aria-ui/src/plugins/` rather than in those crates.

Every edit goes through `aria_plugin::ProjectHost`: the editor's gestures, the
assistant's turns, render-and-replace jobs, and plugins. The host proposes each
command to `CommandProposed` handlers, applies it, and announces it as
`CommandApplied` once no open group can roll it back. Groups and transactions
make one undo step; one thread holds the project at a time, and an edit from
another thread is refused after `ProjectHost::CONTENDED_WAIT` instead of
blocking the window. The project is an `Arc<Project>` replaced copy-on-write, so
a snapshot is a reference count.

Everything the shell draws comes from a registry the shipped code registers into
like any plugin: panels (`PanelHost`), dialogs and overlays (`OverlayHost`),
palette commands (`CommandHost`), app-bar menu items (`MenuHost`), key bindings
(`KeyHost`), plugin settings (`SettingsSchema`), and the effect/transition
catalogs. The shell holds no panel or dialog module name.

Keyboard reach is part of that. A dialog's primary action is a row in
`app/dialog_primary.rs`, so Enter commits the sheet the same way the button does
and a dialog that has nothing to commit says why. The palette's offer is derived
from the command registry, the menu bar and the effect catalog, so every effect
is searchable by name without a compiled row: the catalog is data read at run
time, which is why it cannot be one.

## Property registry

An animatable clip property is one row in `clip_param_catalog()`: a stable id, a
label, a value kind, a scope, a range, and a route to the `Param` it reads.
`ClipParam` names the property, the catalog describes it.

The route is also the availability rule. A row whose owning block is missing
resolves to `ParamRoute::Absent`, so `ParamSpec::available` needs no second
condition to keep in step, and `sample`, `keyframes`, and `easing_at` are all
gated on the same answer. A row that reports itself unavailable cannot return a
value.

Each property axis enum is declared by the `param_enum!` macro, which emits the
variant list twice: once as the enum, once as `ALL`. A new variant therefore
appears in the census by construction, and the conformance test that walks
`LookParam::ALL`, `StyleParam::ALL`, `TextParam::ALL`, and `ShapeParam::ALL`
fails until the variant has a catalog row. Direct `ClipParam` variants cannot be
enumerated that way — they carry data and `variant_count` is unstable — so an
exhaustive `direct_param_id` match plus a `DIRECT` list stands in, which still
refuses to compile when a variant is added.

No surface keeps its own property list. `clip_params_for(clip)` returns the rows
one clip can animate; `AppState::key_params` narrows that by lane, and
`keyframes_of`, `value_at_playhead`, `keyframe_easing`, and the timeline strip's
keyframe band all delegate to the row. This is not a style preference: the
six-row table those methods used to carry, with `_ =>` fallbacks under it,
offered 6 of the model's 58 animatable properties and the suite stayed green, and
the strip's own hand-written union of six properties drew no mark for the other
fifty-two.

An effect's parameters are rows too, built per clip rather than declared
statically: they exist once per effect instance, addressed by
`ClipParam::Effect { effect, param }` and resolved through `effect_spec`. A
parameter nobody has written yet routes to its catalog default and carries no
keyframes, so a first keyframe pins what the renderer is already using. This is
also how a plugin ships an animatable property: it contributes an effect through
`EffectService`, and the effect's parameters reach the strip by the same route as
a shipped effect's. There is no separate plugin-property mechanism to keep in
step.

A row also states the block it belongs to, in `ParamSpec.group`. The panel that
draws a block asks for its group and gets rows; it keeps no list of which
properties it owns, and an id prefix is never parsed to guess one. The shipped
groups are `transform`, `framing`, `audio`, `filter`, `lut`, `grade`, `mask`,
`chroma`, `matte`, `shadow`, `glow`, `outline`, `background`, `text`, and
`shape`. Every instance of an effect is its own group (`effect-0`, `effect-1`),
so two copies of the same effect on one clip are two blocks rather than one
block with doubled rows.

One write path serves every row. `AppState::set_param_scalar` clamps to the
range the row declares, then writes a keyframe under the playhead when the
property is animated and the playhead is inside the clip, and a constant
otherwise. The rule matters because the block commands (`SetClipMask`,
`SetClipChroma`, `SetClipLayerStyles`, `SetClipAdjustments`) each carry a whole
block: a panel that rebuilds the block from its projection in order to change
one knob deletes every keyframe in it, which is what the per-block knob tables
did — `MASK_KNOBS`, `CHROMA_KNOBS`, `STYLE_KNOBS`, and `ADJUSTMENTS`, all now
deleted. Addressing the single property instead makes that class
unrepresentable.

`ParamRange` carries the bounds one side at a time, because the model's bounds
are one-sided: a blur radius is non-negative with no ceiling, an opacity is
`[0, 1]`, a mask rotation is `±3600`. A side left undeclared is a claim that the
model accepts anything there, and the conformance sweep proves it by writing a
far value on that side against three dressed fixtures. That test found sixteen
rows whose real bounds lived only in the inspector's own knob table, where a
stepper clamped to numbers the model had never agreed to.

A colour adjustment is a row in `aria_models::adjust_slider_rows!`, and that row
reaches the GPU. The model's struct and save format, the parameter axis and its
catalog row, the assistant's arguments, the filter recipe's keys, the
compositor's `ColorGrade` with its neutral and its identity test, the packed
uniform lanes, the filter fold and its clamps, and one WGSL accessor per slider
all expand from it. The shader is handed a generated preamble that states the
lane count and names each slider, so the uniform grows on both sides of the
boundary in one build and no file counts vec4 components. What a row cannot
carry is its own maths: a new slider needs its formula written in `grade.wgsl`
and in `ColorGrade::apply`, and until it is there the slider composites as
neutral. That is what the render census refuses, by pressing every row through a
real frame on the layer path and the canvas pass and requiring the pixels to
move.

### Extension cost

| Change | Cost |
| --- | --- |
| Effect, transition, filter, animation, sticker, text effect | one catalog row, one shader; an effect row is also offered in the palette by name, because the offer is derived from the catalog |
| Panel, palette command, menu item, key binding, setting, inspector tab | one registry call |
| Dialog | one registry call, plus one row in `app/dialog_primary.rs` saying what Enter does or why nothing does; the match is exhaustive, so a dialog with no row does not compile |
| Animatable clip property | one `clip_param_catalog()` row, which is also its inspector row |
| Colour adjustment slider | one `adjust_slider_rows!` row, plus its shader math |
| Assistant tool | one `wire_tool_registry!` row, plus its group description, lowering arm and behavioral case; the row generates `WireCommand`, schema, decode, reverse name and group membership |
| Lane kind | one `track_kind_specs()` row, which also decides what a bake of its clips renders to |
| Plugin-owned animatable property | one effect parameter in the plugin's own descriptor |
| Layer topology: matte, transition, canvas pass | `LayerSource`, `Realized`, `LayerJob`, `CompositorLayer`, one shader |

Only the last row touches five crates. Topologies are the primitive set the
other rows are expressed in, so adding one is a core change by construction.
Everything else is a row.

### Adding a layer topology

A topology is a new shape of layer tree, not a new leaf. Work the list in order;
each step compiles on its own.

1. `aria-models`: add the `LayerSource` variant and the state it reads. An
   optional block on `Clip` keeps existing projects loadable.
2. `aria-models`: route any animatable field through `clip_param_catalog()` and
   `look_param_mut`, so keyframes and validation come for free.
3. `aria-render` resolve: fold the source into a `Realized` layer. State what
   happens when the input is missing and when the topology nests inside itself;
   refuse rather than recurse.
4. `aria-render` realize: map model types to compositor types at the boundary.
   `aria-compositor` does not depend on `aria-models`.
5. `aria-compositor`: add the `CompositorLayer` variant, the render arm, the
   pipeline, and the shader. Two-texture passes reuse `transition_layout`.
6. `aria-commands`: add the `EditCommand` variant and a sample in
   `edit_samples()`; the wire-format test is fail-by-default.
7. `aria-ai`: DTO, `WireCommand` variant, `remap` arm, validate arm, a registry
   row in the domain group under `wire/tools/` that owns the tool, a case in
   that group's table, agent summary, `TOOL_SCHEMA_VERSION` bump, then
   `BLESS_TOOL_SCHEMA=1`.
8. `aria-ui`: projection field, `AppState` setter, inspector block, menu row,
   palette row.
9. Tests: a resolve test per fold decision, a GPU readback test per visual
   claim, an app test per setter.
10. Docs: `docs/features.md`, `CHANGELOG.md`, and the table above.

Both realize dispatches match `LayerSource` without a wildcard arm, so step 4 is
not optional: a new variant fails to compile until realize either draws it or
refuses it in words. `LayerSource::kind()` names the eleven topologies that ship
and is what refusal messages quote.

## GPU bridge

- Compositor owns wgpu. Preview currently readbacks into a CPU frame.
- Effect and transition catalogs come from `aria_compositor::effect_descriptors`.
- Missing GPU is not fatal: compositor/effect/transition plugins log and
  continue.
- An effect pass is (pipeline, input view, constants). The offscreen pool holds
  the bind group and uniform buffer for each distinct one, so layers sharing an
  effect share one binding and a repeated frame allocates nothing. The table is
  keyed on that pool's views and lives with it, and a fourth binding on the
  effect layout fails the build until the key covers it.

## Persistence

- Projects: `.aria`
- User config: `~/.aria/config.toml`
- UI chrome: `~/.aria/ui-settings.json`
- Recents: `~/.aria/recents.json`

## Motion and theme

The theme is adapted from [Comet](https://github.com/zeronsh/comet); see
[theme-tokens.md](theme-tokens.md) and
[animation-catalog.md](animation-catalog.md). GPUI comes from
[santh-gpui](https://github.com/santhreal/santh-gpui), pinned by git revision
in the workspace `Cargo.toml`. Curves, specs, presets, loader waveforms, and
release velocity are `gpui::motion`; `aria_ui::motion` holds the editor's
hover and glide stores, the entrances, the pulse clock, and the motion choice
in Settings.
