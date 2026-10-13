# Plugin development

Aria's capabilities are mounted as plugins on a [`PluginContext`]: a typed
service registry plus an event bus. This guide describes the current host; the
target host is specified in [spec/plugins.md](spec/plugins.md), and
[spec/status.md](spec/status.md#plugins-pluginsmd) lists the differences.

Plugins live in `crates/aria-plugin`. Built-in plugins that need GPU types
are mounted from `crates/aria-ui/src/plugins/` so engine crates stay
cycle-free.

## Plugin trait

```rust
pub struct PluginManifest {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub requires: &'static [&'static str],
    pub uses: &'static [&'static str],
}

pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;
    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError>;
    fn unmount(&self, _ctx: &PluginContext) {}
}
```

`ctx.mount(plugin)` refuses a plugin whose `requires` ids are not already
mounted. Services registered during `mount` stay alive until `unmount`.

A failed mount is recorded as well as returned. `ctx.failures()` lists every
plugin that did not mount and the reason it gave; a later successful mount of the
same id clears its entry. `ctx.mounted()` lists what did mount, each with the
service type names it registered. Settings shows both.

`ctx.unmount(id)` drops that plugin's registrations, so its services are gone from
the registry. A surface that needs one asks `try_get` and hides itself when the
answer is `None`.

## Load order

`aria_plugin::load_order(&manifests)` returns the order to mount in, derived from
`requires`. Plugins that do not depend on each other keep the order they were
listed in, because mount order is handler order. A cycle is refused and the error
names every plugin in it. A dependency outside the set is left to `mount`, which
names it.

## Capabilities

`uses` is the list of services a plugin reads, by short name:

```rust
uses: &["StorageService", "SettingsService"],
```

`mount` hands the plugin a context scoped to that list. `try_get` of anything else
returns `None` and `get` panics, whatever is in the registry — a plugin cannot
reach a capability it did not declare. The refusal is recorded, not logged:
`ctx.denials()` names the plugin and the service, and the Plugins list shows it as
`refused <Service>`. A clone of the scoped context keeps the scope, so a handler
registered during `mount` is bound by the same list. Registration is never scoped:
providing a capability is not consuming one.

A plugin that only registers services and handlers declares `uses: &[]`.

## Services

Register a value with `ctx.register(service)` or `ctx.register_arc(arc)`.
Look it up with `ctx.get::<T>()` or `ctx.try_get::<T>()`.

Service traits in `aria_plugin::service`:

| Trait | Role |
| --- | --- |
| `DecoderService` | Open a `VideoDecoder` for a path |
| `EncoderService` | Create a `VideoEncoder` from `EncoderConfig` |
| `EffectService` | Effect catalog. `render_effect` draws one catalog effect over an image on the GPU; see [Effects and transitions](#effects-and-transitions) |
| `TransitionService` | Transition catalog. `render_transition` blends two images of one size at a progress in 0..1 |
| `StorageService` | Cache and project directories |
| `SettingsService` | `~/.aria/config.toml` get / set / save |
| `UiPanelService` | Panel metadata; see [Panels](#panels) for the host that draws it |

A service holds one value per type. Anything several plugins must see in order is
an event instead.

## Effects and transitions

A plugin adds an effect or a transition by supplying a catalog spec and WGSL.
The compositor draws it like a shipped one: on clips, in the effect rack, in
the browser and the palette, in previews and in exports.

```rust
use aria_plugin::{EffectDefinition, EffectParamKind, EffectParamSpec, EffectSpec};

const TINT: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let px = textureSample(input_tex, aria_sampler, in.uv);
    let wave = 0.5 + 0.5 * sin(aria.time * param_speed());
    return vec4<f32>(mix(px.rgb, param_tint().rgb * px.a, wave), px.a);
}";

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_effect(EffectDefinition {
        spec: EffectSpec {
            id: "example.pulse_tint".into(),
            label: "Pulse Tint".into(),
            params: vec![
                EffectParamSpec {
                    name: "speed".into(),
                    label: "Speed".into(),
                    kind: EffectParamKind::Scalar,
                    default: 2.0,
                    min: 0.0,
                    max: 20.0,
                    default_color: [0; 4],
                    default_vec2: [0.0; 2],
                },
                EffectParamSpec {
                    name: "tint".into(),
                    label: "Tint".into(),
                    kind: EffectParamKind::Color,
                    default: 0.0,
                    min: 0.0,
                    max: 0.0,
                    default_color: [255, 120, 0, 255],
                    default_vec2: [0.0; 2],
                },
            ],
            generator: false,
        },
        passes: vec![TINT.into()],
    })?;
    Ok(())
}
```

`register_effect` and `register_transition` return a `Registration`. During
`mount` the context keeps it: the entry stays until the plugin unmounts, and a
failed mount removes it. Outside a mount the caller keeps it, and dropping it
removes the entry. Clips that use a removed effect draw their input unchanged;
a removed transition draws a crossfade.

### Generators

An effect whose spec sets `generator: true` draws a picture of its own. The
sidebar's Generators tab and the command palette list it. Picking it places a
clip at the playhead; dragging it from the sidebar places the clip on the lane
and at the time it lands on. The clip is a transparent solid the size of the
frame, named after the effect, with the effect first in its chain. Placing it
is one undo step.

The generator's `input_tex` holds the transparent solid, so `fs` draws from
`aria.size`, `aria.time`, `in.uv`, and its parameters. The clip's transform,
opacity, mask, blend mode, and the effects after the generator apply to the
generated picture as to any layer. The effect lists also offer a generator, and
on another clip it draws over that clip's picture.

A clip whose generator is no longer registered keeps the effect and its
parameters and draws nothing.

### Shader source

Each pass is the plugin's WGSL followed by a host prelude. The prelude declares
`struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }`
and the vertex stage `vs`. The plugin writes `@fragment fn fs` returning
`@location(0) vec4<f32>`, and declares no bindings of its own. Line numbers in
a refusal are lines of the plugin's source.

Textures hold premultiplied RGBA, and `fs` returns premultiplied RGBA.

Effect bindings, group 0:

| Binding | Name | Content |
| --- | --- | --- |
| 0 | `input_tex` | The previous pass's output, or the effect's input for pass 0 |
| 1 | `source_tex` | The effect's input |
| 2 | `aria_sampler` | Linear sampler, clamped to edge |
| 3 | `aria` | Uniform `AriaPass` |

`AriaPass` fields: `size` (output size in pixels), `texel` (`1 / size`), `time`
(seconds since the clip started), `pass_index`, `pass_count`, and `params`.

Each parameter gets an accessor `param_<name>()`: `f32` for a scalar,
`vec2<f32>` for a vec2, `vec4<f32>` RGBA in 0..1 for a colour. Parameters are
keyframeable like those of shipped effects.

Transition bindings, group 0: `outgoing_tex` (0), `incoming_tex` (1),
`aria_sampler` (2), and the uniform `aria: AriaTransition` (3), whose fields
are `size`, `texel`, and `progress` in 0..1. A transition takes no parameters.

### Limits and refusals

Registration parses and validates every pass with naga and returns
`PluginError::RegisterRefused(RegisterError)` when:

- the id has no `.` namespace, or is already in the catalog;
- the label is empty, a parameter name repeats or is not a WGSL identifier, or
  a range is inverted;
- an effect has no passes, or more than `MAX_EFFECT_PASSES` (8);
- the parameters flatten to more than `MAX_EFFECT_FLOATS` (256) floats: 1 per
  scalar, 2 per vec2, 4 per colour;
- the WGSL does not parse or validate, has no `fs` fragment entry point, or
  declares a binding.

`RegisterError` holds the `id`, the `pass` index, the `location` as
`(line, column)` in the pass source, and the `message`.

A device that rejects a validated pipeline logs the error once; the effect
then draws its input unchanged.

## Audio effects

A plugin adds an audio effect by supplying a catalog spec and a processor
factory. The mixer runs it like a shipped effect: in a clip's chain, a lane's,
or the master's, in playback, in export, and in `AudioHost` mixes.

```rust
use aria_plugin::{
    AudioBlock, AudioEffectSpec, AudioProcessor, EffectParamKind, EffectParamSpec,
};

struct Drive {
    memory: [f32; 2],
}

impl AudioProcessor for Drive {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let amount = block.param(0).expect("slot 0");
        let (left, right) = block.channels();
        for (frame, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let k = amount.at(frame);
            *l = (*l * k).tanh();
            *r = (*r * k).tanh();
        }
    }

    fn reset(&mut self) {
        self.memory = [0.0; 2];
    }
}

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_audio_effect(
        AudioEffectSpec {
            id: "example.drive".into(),
            label: "Drive".into(),
            params: vec![EffectParamSpec {
                name: "amount".into(),
                label: "Amount".into(),
                kind: EffectParamKind::Scalar,
                default: 1.0,
                min: 1.0,
                max: 20.0,
                default_color: [0; 4],
                default_vec2: [0.0; 2],
            }],
            latency: 0,
            sidechain: false,
        },
        |_rate: u32| Box::new(Drive { memory: [0.0; 2] }) as Box<dyn AudioProcessor>,
    )?;
    Ok(())
}
```

The returned `Registration` is held on the terms of `register_effect`.

The inspector's Audio block lists the effect under Audio effects for a clip
with sound, and each mixer strip and the master strip list it under Inserts. A
lane and the master hold at most `STRIP_INSERTS` (5) inserts; a clip has no
limit. An insert whose effect is not registered keeps its parameters, draws as
not installed, and passes its input through.

### Processing

The mixer calls the factory once per insert when it opens over a project, with
the sample rate (`EXPORT_AUDIO_RATE`, 48000). It then calls `process` once per
block, in signal order along the chain. The block length is the length the
mix was asked for and varies between calls. `process` runs on the audio thread
during playback: allocate in the factory, and do not lock, allocate, or
perform I/O in `process`.

| `AudioBlock` call | Result |
| --- | --- |
| `frames()` | Frames in the block |
| `channels()` | Left and right, planar, written in place |
| `position()` | The sequence position of the first frame, in frames at the sample rate |
| `sample_rate()` | Frames per second |
| `param(slot)` | The parameter in spec order: `ParamSignal::Constant(value)`, or `ParamSignal::PerFrame(values)` when it is keyframed and changes within the block. `at(frame)` reads either |
| `sidechain()` | The key signal, planar, when the spec sets `sidechain` and the insert has a key lane |

Consecutive blocks continue each other. `reset` is called before a block that
does not continue the previous one, such as after a seek, and clears every
value carried between blocks.

A parameter keyed on a clip insert is in ticks from the clip's start, like any
clip curve. A parameter keyed on a lane or master insert is in sequence ticks.

`latency` is the number of frames at 48 kHz by which the output trails the
input, at most `MAX_AUDIO_LATENCY` (48000). The mixer delays every parallel
path by the same amount, so lanes stay aligned.

An effect with `sidechain: true` shows a Key from row on a mixer strip. The
master accepts a key from any lane with sound. A lane accepts any lane with
sound except itself and the lanes its own signal reaches through outputs,
sends, and keys. A clip insert takes no key. A muted or silent key lane reads
as zeros.

### Faults

A panic in the factory or in `process` bypasses the insert for the rest of the
mix and fails the plugin for the session, which drops its registrations.
During playback, a block has the real time it covers as its deadline; three
misses in a row bypass the insert for the rest of the mix and record a
`PluginFault` in `ctx.faults()`. A bypassed insert keeps delaying its input by
its declared latency.

Registration returns `PluginError::AudioEffectRefused(message)` when:

- the id has no `.` namespace, is a shipped id, or is already registered;
- the label is empty, a parameter name repeats, or a range is inverted;
- a parameter is not `EffectParamKind::Scalar`, or its default lies outside
  its range;
- the latency exceeds `MAX_AUDIO_LATENCY`.

## Importers and exporters

A plugin adds a timeline interchange format with an importer, an exporter, or
both. An importer reads a file into the open project through a
[`Transaction`](#project); an exporter writes the open timeline.

```rust
use std::io::Write;
use std::path::Path;

use aria_models::TrackKind;
use aria_plugin::{
    Command, EditCommand, Exporter, ExporterSpec, Importer, ImporterSpec,
    InterchangeReport, Project, TransactError, Transaction,
};

struct LaneList;

impl Importer for LaneList {
    fn import(
        &self,
        file: &Path,
        tx: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError> {
        let text = std::fs::read_to_string(file)
            .map_err(|err| TransactError::Aborted(err.to_string()))?;
        for name in text.lines() {
            tx.apply(Command::Edit(EditCommand::AddTrack {
                kind: TrackKind::Video,
                name: name.to_owned(),
                index: None,
                pinned: true,
            }))?;
        }
        Ok(InterchangeReport::default())
    }
}

impl Exporter for LaneList {
    fn export(
        &self,
        project: &Project,
        out: &mut dyn Write,
    ) -> std::io::Result<InterchangeReport> {
        for track in project.timeline().tracks_ordered() {
            writeln!(out, "{}", track.name)?;
        }
        Ok(InterchangeReport::default())
    }
}

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_importer(
        ImporterSpec {
            id: "example.lanes".into(),
            name: "Lane list".into(),
            extensions: vec!["lanes".into()],
        },
        LaneList,
    )?;
    ctx.register_exporter(
        ExporterSpec {
            id: "example.lanes".into(),
            name: "Lane list".into(),
            extension: "lanes".into(),
        },
        LaneList,
    )?;
    Ok(())
}
```

Both returned `Registration`s are held on the terms of `register_effect`.

| Call | Result |
| --- | --- |
| `ctx.importers()`, `ctx.exporters()` | Every registered spec, in registration order |
| `ctx.importer_for(path)` | The importer for the extension of `path` |
| `ctx.import_file(&host, path)` | Runs that importer and returns its report and the transaction's `Receipt` |
| `ctx.export_file(id, &project, path)` | Runs the exporter registered as `id` and returns its report |

Extensions match without regard to case and are stored lowercase. When two
importers list one extension, the one registered first reads it. Built-in
plugins mount before external ones.

An import is one transaction named `Import <file name>`, or
`<plugin id>: Import <file name>` for a plugin's importer: one undo step. When
the importer returns an error or panics, nothing of the import remains and
`import_file` returns `InterchangeError::Import`.

An export writes to `.<file name>.partial` beside the destination, syncs it,
and renames it over the destination after the exporter returns. When the
exporter returns an error or panics, the destination is unchanged, the partial
file is removed, and `export_file` returns `InterchangeError::Export` or
`InterchangeError::ExportPanicked`. An exporter refuses a timeline its format
cannot hold with `std::io::Error::other(reason)`.

`InterchangeReport::skipped` lists, one line each, what the file held or the
project holds that the format could not carry.

A panic in `import` or `export` fails the plugin for the session.

Registration returns `PluginError::InterchangeRefused(message)` when:

- the id is empty, or another importer or exporter holds it;
- an importer lists no extension;
- an extension is empty or holds anything but ASCII letters and digits,
  such as a leading dot.

## Events

`ctx.events()` is an `EventBus`. It dispatches three kinds of event:

- Notifications (`Event`). `post` queues the event and returns. One delivery
  thread per bus runs the handlers in post order.
- Waterfalls (`WaterfallEvent`). `emit_waterfall` runs the handlers on the
  calling thread. A handler can rewrite the event or stop the chain.
- Serial events (`SerialEvent`). `serial` runs the handlers on the calling
  thread; the first non-`None` result wins.

Subscribe during `mount` with `ctx.on_event`, `ctx.on_event_main`,
`ctx.on_waterfall`, or `ctx.on_serial`. The context keeps the `Registration`
guard for the life of the plugin, the same as a service. The bus methods
(`subscribe`, `subscribe_main`, `waterfall`, `on_serial`) return the guard to
the caller, so a bare `ctx.events().subscribe(…)` in `mount` unsubscribes
immediately.

Handlers run in registration order, which is mount order.

### Delivery

- A notification reaches the handlers registered when it was posted. A handler
  whose registration drops before delivery is skipped.
- A handler that panics is logged, and delivery continues to the rest.
- A slow handler delays later notifications, never the thread that posted.
- `ctx.on_event_main` runs the handler on the editor's UI thread instead of the
  delivery thread, in the same order.
- `events.flush(within)` waits until every notification posted before the call
  has run. It returns `false` when `within` elapses first, and at once when
  called from a handler.

```rust
ctx.on_event::<MediaImported, _>(|imported| {
    tracing::info!(path = %imported.path.display(), "imported");
});
```

### Editor notifications

| Event | Posted when | Fields |
| --- | --- | --- |
| `CommandApplied` | A command can no longer be rolled back | `command`, `outcome`, `revision`, `origin` |
| `ProjectClosed` | A session is about to be replaced; followed by `ProjectOpened` | `path` |
| `ProjectOpened` | A project became the session: Open, Load, a template, or a new project | `path`, `project` |
| `ProjectSaved` | The project was written | `path` |
| `MediaImported` | A file was added to the pool | `media`, `path` |
| `MediaRelinked` | A pool item reads a new file | `media`, `path` |
| `MediaOffline` | The pool first shows a file missing | `media`, `path` |
| `SelectionChanged` | The clip selection changed | `clips` |
| `PlayheadMoved` | The playhead moved | `at` |
| `InOutChanged` | The sequence In or Out was set or cleared | `mark_in`, `mark_out` |
| `PlaybackStarted` | Playback started, or its speed changed while playing | `at`, `speed` |
| `PlaybackStopped` | Playback stopped | `at` |
| `ExportStarted` | An export started writing | `path` |
| `ExportFinished` | An export that started has ended | `path`, `result` |
| `RenderFinished` | A clip render or an image sequence ended | `kind`, `path`, `result` |
| `JobProgress` | A background job was queued, started, or reported progress | `job`, `label`, `progress`, `detail` |
| `JobFinished` | A background job ended | `job`, `label`, `outcome`, `detail` |
| `PluginActivated` | A plugin finished mounting | `id` |
| `PluginDeactivated` | A plugin was unmounted | `id` |

`CommandApplied` is posted when a lone command applies, or when the outermost
group that contains it commits. A group that rolls back posts nothing. The
session and media events follow the `CommandApplied` of the command that caused
them, and an `Export` command posts `ExportStarted` and `ExportFinished` before
its `CommandApplied`. The editor redraws on a `CommandApplied` whose origin is
not `Origin::Editor`.

The view events are posted at most once per drawn frame, one per part that
changed. `MediaOffline` is posted again for a file that returns and goes
missing a second time. A render queue entry cancelled before it starts posts no
export events. `result` is `RenderResult::Written`, `Failed(reason)`, or
`Cancelled`, and agrees with the job's final state on the Jobs board.

### Editor view

`EditorView` is a service holding the view as of the last drawn frame. The
engine plugin registers it; a plugin declares `uses: &["EditorView"]` and calls
`current()`, which returns a `ViewState` (`selection`, `playhead`, `mark_in`,
`mark_out`, `playing`, `speed`), or `None` before the first frame.

### Context menus

`ContextMenuRequested` is a waterfall run when a context menu opens. `target`
is a `MenuTarget`: `Clip`, `Track`, `Transition`, `Viewer`, `Media`, or
`Recent`. `project` is the project the menu was opened over.
`request.add(label, run)` adds a row. Plugin rows are drawn after the editor's
own, below a rule, in mount order.

Handlers run on the UI thread while the menu opens. A handler reads the request
and returns; the work belongs in `run`, which runs on a thread of its own when
the row is clicked. A handler that panics ends the round, and the rows added
before it are kept.

```rust
let host = ctx.get::<ProjectHost>();
ctx.on_waterfall::<ContextMenuRequested, _>(move |request| {
    if let MenuTarget::Clip { clip, .. } = request.target {
        let host = Arc::clone(&host);
        request.add("Remove clip", move || {
            let command = Command::Edit(EditCommand::RemoveClip { clip });
            let _ = host.apply(&Origin::plugin("example.tidy"), command);
        });
    }
});
```

### Hooks

`CommandProposed` is a waterfall run before every command reaches the engine:

| Field | Contents |
| --- | --- |
| `command` | The command; a handler can edit it in place |
| `project` | The project the command applies to, as an `Arc<Project>` |
| `origin` | `Origin::Editor`, `Origin::Assistant`, or `Origin::Plugin(id)` |

`proposed.refuse(reason)` stops the chain: the command is never applied, nothing
enters history, and the reason is shown as written. `EditCommand::targets()`
names the clips and tracks a command touches, so a policy handler does not match
on variants; `ignores_track_lock()` marks the commands a lock must not block. The
engine plugin uses both, with `proposed.project`, to refuse an edit on a locked
lane.

The engine is locked while the chain runs. A handler reads `proposed.project`,
or `ProjectHost::snapshot()`, which returns the same value there; a handler that
locks the engine deadlocks the edit. An edit cannot start inside a handler:
`transact` returns `TransactError::Reentrant`, and `apply`, `group`, `undo`,
`redo`, `history_jump`, and `replace_session` panic.

```rust
ctx.on_waterfall::<CommandProposed, _>(|proposed| {
    if let Command::Edit(edit) = &proposed.command {
        if edit.targets().tracks.contains(&guarded_track) {
            proposed.refuse("That lane is under review");
        }
    }
});
```

`CommandApplied` handlers run on the delivery thread with no lock held, so a
handler can edit through the host.

`ResolveDroppedPath` is a serial event asking which plugin owns a dropped file.
Return `Some(DropAction)` to claim it — `OpenProject`, `ImportMedia`, `ApplyLut`,
or `InstallSkill`. The decoder claims media extensions, the effects plugin claims
`.cube`, and the AI plugin claims `SKILL.md`. An unclaimed path is imported as
media. The project extension is not up for election: the host opens a `.aria` file
whatever is mounted, so a plugin that fails to load cannot turn a project into a
media import.

```rust
ctx.on_serial::<ResolveDroppedPath, DropAction, _>(|event| {
    event
        .path
        .extension()
        .is_some_and(|ext| ext == "srt")
        .then_some(DropAction::ImportMedia)
});
```

## Project

`ProjectHost` is the one path that reads and edits the project. The editor,
the assistant, and render jobs edit through it too. The engine plugin registers
it; a plugin declares `uses: &["ProjectHost"]` and takes it with
`ctx.try_get::<ProjectHost>()`.

| Call | Result |
| --- | --- |
| `snapshot()` | The project as an immutable `Arc<Project>`. A snapshot held across an edit costs one copy of the project |
| `revision()` | The engine revision, bumped by every change |
| `apply(&origin, command)` | Proposes and applies one command as its own undo step, or as part of this thread's open group |
| `transact(origin, label, commands)` | Applies the commands as one undo step named `label`, or none of them |
| `transact_with(origin, label, body)` | Runs `body` with a `Transaction`: `tx.apply(command)` and `tx.project()`. An `Err` or a panic from `body` reverts it |
| `group(&origin)` | Opens an `EditGroup`: every command this thread applies until `commit` is one undo step. Dropping it rolls it back |
| `undo()`, `redo()`, `history_jump(depth)` | Step history. `Ok(false)` while this thread has a group open |
| `replace_session(&origin, f)` | Runs `f` on the engine with no edit in flight, for a session replacement that is not a command, then posts `ProjectClosed` and `ProjectOpened` |
| `holder()` | The origin holding the project from another thread, if any |

```rust
// `get` panics unless the manifest lists "ProjectHost" in `uses`.
let host = ctx.get::<ProjectHost>();
let result: Result<Receipt, TransactError> = host.transact(
    Origin::plugin("example.review"),
    "Add review lane",
    vec![Command::Edit(EditCommand::AddTrack {
        kind: TrackKind::Video,
        name: "Review".into(),
        index: None,
        pinned: true,
    })],
);
```

A plugin's transaction is named `<plugin id>: <label>` in History. When a command
is refused or fails, or the body returns an error or panics, every command the
transaction applied is reverted, and `TransactError` states which: `Refused`,
`Failed`, `Aborted`, or `Panicked`. A group or transaction opened inside another
is a savepoint: rolling it back keeps the commands before it.

`Save`, `Open`, `Load`, `RelinkMedia`, `Export`, `SaveTemplate`, and
`ApplyTemplate` write or replace the session and cannot be undone. Inside a group
or transaction they fail with `TransactError::NotUndoable`.

One thread edits at a time. A thread holds the project from its first command
or group until that command lands or the outermost group closes, and its own
later edits join in. An edit from another thread waits up to
`ProjectHost::CONTENDED_WAIT` (100 ms) and then fails with
`TransactError::Busy`, or `EngineError::Refused` from `apply`, naming the holder:
"the assistant is editing the project; try again when it finishes". A background
job retries until it gets the project or is cancelled.

## Frames

`RenderHost` renders frames of a project. The render plugin registers it on the
process's GPU device; a plugin declares `uses: &["RenderHost"]`. The host renders
with a renderer of its own, built on the first request, so a plugin render does
not wait on the preview.

| Call | Result |
| --- | --- |
| `frame(&project, time, &request)` | The frame at `time` as a straight-alpha `RgbaImage` |
| `frames(&project, range, &request, each)` | One frame per tick of `range` at the range's rate, passed to `each` until it returns `ControlFlow::Break` |
| `frame_to_texture(&project, time, &request, &texture)` | The frame written premultiplied into an `Rgba8Unorm` texture with `COPY_DST` usage, with no copy to the CPU |
| `frame_size(&project, &request)` | The width and height of a frame of `request` |
| `gpu()` | The `GpuContext` the host renders on, for creating a target texture |

A `FrameRequest` sets the source, the size, and motion blur. The constructors
`FrameRequest::open()`, `sequence(id)`, and `clip(id)` take canvas size without
motion blur; `.size(size)` and `.motion_blur(on)` change one field.

| Field | Values |
| --- | --- |
| `source` | `FrameSource::Open`: the sequence open for editing. `FrameSource::Sequence(None)`: the root sequence; `Sequence(Some(id))`: a nested one, whichever is open. `FrameSource::Clip(id)`: one clip of any sequence as placed on its canvas, with its effects, over transparency |
| `size` | `FrameSize::Canvas`; `Fit { max_width, max_height }`, scaled down only; `Exact { width, height }`, scaled to fit and centred over the background |
| `motion_blur` | Supersample moving transforms as an export does |

```rust
let project = ctx.get::<ProjectHost>().snapshot();
let frames = ctx.get::<RenderHost>();
let request = FrameRequest::open().size(FrameSize::Fit {
    max_width: 320,
    max_height: 180,
});
let range = TimeRange::at_rate(0, 48, project.timeline().frame_rate);
frames.frames(&project, range, &request, |time, image| {
    thumbnails.push((time, image));
    ControlFlow::Continue(())
})?;
```

A texture target is created on `frames.gpu().device`, with the size
`frame_size` reports:

```rust
let (width, height) = frames.frame_size(&project, &request)?;
let texture = frames.gpu().device.create_texture(&wgpu::TextureDescriptor {
    label: Some("thumbnail"),
    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Rgba8Unorm,
    usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
});
frames.frame_to_texture(&project, time, &request, &texture)?;
```

`aria_plugin::wgpu` is the `wgpu` the device belongs to. Any other texture
fails with `CompositorError::FrameTarget`, which states the size, format, and
usage required, before anything is drawn.

A clip in a transition or a track matte at `time` draws without the other clip.
A clip that is not visible at `time` renders as a transparent frame. A clip id
that is not in the project fails with `ModelError::UnknownClip`, and a sequence
id with `ModelError::UnknownSequence`.

## Sound

`AudioHost` mixes the sound of a project the way an export does: clip, lane,
and master inserts, sends, buses, and faders, at `EXPORT_AUDIO_RATE` (48000)
in interleaved stereo. The editor registers it; a plugin declares
`uses: &["AudioHost"]`. Each call opens a mixer of its own, so calls on several
threads run side by side.

| Call | Result |
| --- | --- |
| `mix(&project, source, range, each)` | Blocks of `range` of `source`, in order, passed to `each` with the sample frame each starts at, until it returns `ControlFlow::Break` |

`source` is `AudioSource::Open`, `AudioSource::Sequence(None)` for the root
sequence or `Sequence(Some(id))` for a nested one, or `AudioSource::Clip(id)`:
one clip with every other clip of its sequence removed, through its own
inserts, its lane's inserts and fader, and the buses and master it reaches.
Blocks hold `AUDIO_HOST_BLOCK` (4096) frames except a shorter last one. A range
with nothing audible arrives as silence.

```rust
let project = ctx.get::<ProjectHost>().snapshot();
let sound = ctx.get::<AudioHost>();
let range = TimeRange::at_rate(0, 240, project.timeline().frame_rate);
let mut peak = 0.0f32;
sound.mix(&project, AudioSource::Open, range, |_, block| {
    peak = block.iter().fold(peak, |held, s| held.max(s.abs()));
    ControlFlow::Continue(())
})?;
```

An analyzer writes what it finds (markers, keyframes, clip metadata) through
`ProjectHost`.

## Panels

Every panel the editor draws is registered, including the shipped ones.
`aria.ui` mounts a `PanelHost` and adds the media pool, the timeline, and the
inspector to it; the shell then resolves each slot from that host. A plugin
contributes a panel to the same host:

```rust
use aria_ui::plugins::panel_host::{host_of, ALWAYS, PanelEntry};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(host) = host_of(ctx) {
        host.add(PanelEntry {
            id: "example.panel.notes",
            title: "Notes",
            location: PanelLocation::Inspector,
            render: draw_notes,
            visible: ALWAYS,
        });
    }
    Ok(())
}
```

`render` is an `aria-ui` function pointer, so `aria-plugin` stays free of GPUI.
Registering an id that already exists replaces that panel, which is how a plugin
overrides a built-in. Mount after `aria.ui` to reach the host.

A slot holds the panels in registration order, stacked. `Overlay` has no
built-in entry: an overlay contribution is drawn over the viewer.

### Body slots

`Viewer` and `Home` are slots whose entries are alternatives rather than a
stack. The shipped set puts the preview and the assistant in `Viewer` and the
launch screen in `Home`. Which one draws is decided by `visible`, a
`fn(&AppState) -> bool` the entry carries:

```rust
host.add(PanelEntry {
    id: "example.panel.scope",
    title: "Scope",
    location: PanelLocation::Viewer,
    render: draw_scope,
    visible: |state| state.active_panel == ActivePanel::Preview,
});
```

A docked pane uses `ALWAYS`. Two entries whose predicates hold at once both
draw, so a replacement body must either take over an existing id or narrow its
own predicate to a state no shipped entry claims.

## Dialogs and overlays

Dialogs are registered the same way panels are. `aria.ui` mounts an
`OverlayHost` holding Export, Settings, the palette, Jobs, the context menu, and
the rename bar; the shell draws whatever the host has for the current state and
names no dialog module of its own.

```rust
use aria_ui::app::DialogKind;
use aria_ui::plugins::overlay_host::{host_of, OverlayEntry, OverlayWhen};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(host) = host_of(ctx) {
        host.add(OverlayEntry {
            id: "example.dialog.notes",
            when: OverlayWhen::Dialog(DialogKind::Plugin("example.dialog.notes")),
            render: draw_notes_dialog,
        });
    }
    Ok(())
}
```

Open it with `state.open_dialog(DialogKind::Plugin("example.dialog.notes"))` —
from a command, a menu item, or a key binding. One dialog is on screen at a
time, so opening yours closes whatever was up, and Escape closes it.

`OverlayWhen::ContextMenu` and `OverlayWhen::Rename` stack: everything
registered for them draws while that condition holds. A dialog does not stack;
the first entry registered for a kind is the one drawn.

## Commands

The command palette draws a registry too. `aria.ui` mounts a `CommandHost` and
adds the shipped commands from `commands::BUILTIN_COMMANDS`; the
palette lists whatever the host holds, in registration order, and Enter runs the
first row the query matches. A plugin adds a command the same way:

```rust
use aria_ui::commands::host::{host_of, CommandEntry};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(host) = host_of(ctx) {
        host.add(CommandEntry {
            id: "example-notes-clear",
            label: "Clear notes",
            run: |state| state.open_dialog(DialogKind::Jobs),
        });
    }
    Ok(())
}
```

`run` takes `&mut AppState`, so a command does what a menu item does. An id that
already exists is replaced, which is how a plugin overrides a shipped command.
With no host mounted the palette offers nothing.

A command that starts work needs the entity context, which `AppState` does not
have. Register that kind with `add_windowed`:

```rust
use aria_ui::commands::host::{host_of, WindowedEntry};

host.add_windowed(WindowedEntry {
    id: "example-notes-index",
    label: "Index notes",
    run: |state, cx| state.check_for_updates(cx),
});
```

The palette hands a windowed row back to the shell to run, so it reaches
`cx.spawn` and the job list. The two kinds share one id space and one order: a
windowed registration replaces a plain row of the same id. The shipped detectors,
transcription, loudness, and the duck are registered this way, in
`BUILTIN_JOB_COMMANDS`.

## Menus

The app bar reads a registry as well. `aria.ui` mounts a `MenuHost` and adds the
shipped tables as its first contributor; the bar asks the host for one menu's
items each time it opens. A contributor is a function, because an entry that
cannot run right now is drawn from the state:

```rust
use aria_ui::chrome::menu_host::host_of;

fn notes_items(menu: AppMenu, _state: &AppState) -> Vec<MenuItem> {
    if menu != AppMenu::Window {
        return Vec::new();
    }
    vec![MenuItem {
        id: "example-menu-notes",
        label: "Notes",
        run: |_id, state, _| state.open_dialog(DialogKind::Jobs),
    }]
}
```

Items land after the ones already registered, so the shipped entries stay at the
top of each menu. An id that a contributor already used is dropped rather than
drawn twice: ids are element ids, and a plugin must not shadow a shipped entry.

A row is told its own id, so one contributor can generate a family of rows from a
table instead of writing a closure per row. The shipped workspaces are drawn that
way: the id the row carries is the workspace it applies. A row that does not need
it names the argument `_`.

## Keys

`aria.ui` mounts a `KeyHost` and adds the shipped bindings; the shell resolves
each keystroke through it and holds no chord of its own. A binding is data:

```rust
use aria_ui::keys::{host_of, After, Binding, Chord, KeyRoute};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(host) = host_of(ctx) {
        host.add(Binding {
            id: "example-notes-open",
            chord: Chord::ctrl_shift("n"),
            route: KeyRoute::Editor,
            run: |state| state.open_dialog(DialogKind::Jobs),
            after: After::Nothing,
        });
    }
    Ok(())
}
```

A chord is exact: Alt+I and I are different bindings, and `ctrl` means
control-or-command. `route` separates Home from the editor, which bind some of
the same chords to different actions. `after` is the side effect the action
cannot do itself, because it needs the window: `Play` starts the playback clock,
`Preview` asks for a fresh frame.

`add` returns `false` when the chord is already bound on that route, and records
the refusal in `conflicts()` — the first binding keeps the key, so a plugin
cannot take Ctrl+S away from saving. Text keys and Escape never reach the
registry: the shell answers those first.

## Settings

`SettingsService` reads and writes `~/.aria/config.toml`. A value nobody can see
is a value nobody sets, so a plugin also declares what its fields are and the
host draws them:

```rust
use aria_plugin::{settings_schema::schema_of, SettingSpec, SettingsSection};

const SETTINGS: &[SettingSpec] = &[SettingSpec {
    key: "notes.directory",
    label: "Notes directory",
    placeholder: "the project directory",
    secret: false,
}];

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(schema) = schema_of(ctx) {
        schema.declare(SettingsSection {
            plugin: "example.notes",
            title: "Notes",
            fields: SETTINGS,
        });
    }
    Ok(())
}
```

Reading the registry counts as a capability, so the manifest declares
`uses: &["SettingsSchema"]`. The section shows in **Settings** under its title,
one row per field, drawn and key-routed like a shipped field. A `secret` field
shows its length, never its text. Editing writes through `SettingsService` under
the declared key and saves; clearing a field stores nothing rather than an empty
string, so the plugin's own default applies again. Declaring twice replaces the
section, so a remount does not double the rows. `aria.settings` mounts the
registry, and it mounts before every plugin but `aria.storage`.

## Mount order

`aria_ui::plugins::builtin(data_dir)` is the built-in list, in mount order:

`aria.storage`, `aria.settings`, `aria.decoder`, `aria.encoder`,
`aria.compositor`, `aria.render`, `aria.effects`, `aria.transitions`,
`aria.engine`, `aria.cloud`, `aria.analysis`, `aria.jobs`,
`aria.transcription`, `aria.ai`, `aria.ui`.

`mount_all` mounts every one of them and returns the failures instead of stopping
at the first, so a host without a GPU still starts. `builtin_by_id` returns one
plugin for re-mounting after it was disabled.

`mount_all` takes the data directory to read external plugins and catalog
overrides from. Pass the same directory the session uses, so a process pointed at
another profile does not load code and catalogs from the default one.

That directory reaches exactly one plugin. `aria.storage` owns the root and
registers a `StorageLayout` and a `StorageService` for it; a plugin that needs a
directory asks that service instead of resolving one. `aria.transcription`
refuses to mount without it rather than downloading models into a directory the
session never reads.

## External plugins

A plugin does not have to be compiled into the app. `<data dir>/plugins` is
scanned at boot and every shared library in it is loaded and mounted after the
built-ins, in path order.

Build the plugin as a `cdylib` that depends on `aria-plugin` and export it:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
aria-plugin = { path = "…/crates/aria-plugin" }
```

```rust
aria_plugin::export_plugin!(NotesPlugin);
```

```text
cargo build -p aria-notes-plugin --release
cp target/release/libaria_notes_plugin.so ~/.aria/plugins/
```

`examples/notes-plugin` is a working one: it claims `.srt` and vetoes a command
that touches more than eight tracks. Check a library before shipping it with

```text
cargo run -p aria-plugin --example load_external -- ~/.aria/plugins
```

Rust has no stable ABI, so `export_plugin!` also exports the loader version and
the compiler that built the library. A plugin built by a different `rustc`, or
against a different `aria-plugin`, is refused before its code runs and appears in
the Plugins list with the reason. Rebuild it against the same toolchain.
The loader version is 6, so a library built against an `aria-plugin` from before
`RenderHost` replaced `RenderInterceptorService` is refused.

A loaded library stays mapped until the process exits, because a manifest borrows
its strings out of the library. Disabling an external plugin is therefore not
offered: its row has no toggle.

## Worked example

One plugin that does all three things a plugin can do: contribute a panel, sit in
front of every command, and claim a file type.

```rust
use aria_plugin::{
    CommandProposed, DropAction, PanelLocation, Plugin, PluginContext, PluginError,
    PluginManifest, ResolveDroppedPath,
};
use aria_ui::plugins::panel_host::{ALWAYS, host_of, PanelEntry};
use gpui::{AnyElement, Entity, IntoElement, ParentElement, Styled, div};

pub struct NotesPlugin;

impl Plugin for NotesPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "example.notes",
            name: "Notes",
            version: "0.1.0",
            // The panel host is mounted by `aria.ui`, so it has to come first.
            requires: &["aria.ui"],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        if let Some(host) = host_of(ctx) {
            host.add(PanelEntry {
                id: "example.panel.notes",
                title: "Notes",
                location: PanelLocation::Inspector,
                render: draw_notes,
                visible: ALWAYS,
            });
        }

        // Refuse a command instead of letting it reach the engine. The reason is
        // shown to the user verbatim, so write it as a sentence.
        ctx.on_waterfall::<CommandProposed, _>(|event| {
            if event.command.targets().clips.is_empty() {
                event.refuse("Notes needs a clip selected.");
            }
        });

        // Claim a file type. The first plugin to claim one wins, in mount order.
        ctx.on_serial::<ResolveDroppedPath, DropAction, _>(|event| {
            event
                .path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                .then_some(DropAction::InstallSkill)
        });

        Ok(())
    }
}

fn draw_notes(
    _state: &Entity<aria_ui::app::AppState>,
    _cx: &mut gpui::Context<aria_ui::shell::Shell>,
) -> AnyElement {
    div().p_2().child("Notes").into_any_element()
}
```

The GPU catalogs are not contributions: `EffectService`, `TransitionService`,
`DecoderService`, and `EncoderService` hold one value each, owned by the shipped
plugins. A plugin that registers one replaces the shipped catalog rather than
adding to it.

Shipping an effect therefore takes two pieces. The offerable set is
`aria_models::effect_catalog`, so the id and its parameters go in an `effects.toml`
override (see `docs/catalogs.md`); an id the model does not carry is refused by the
engine, so the browser never offers one. Dispatch comes from the mounted
`EffectService`, and the Add list marks an entry passthrough when that service does
not carry a descriptor for it — including when no GPU plugin mounted at all.

The headed app resolves its data directory once, calls
`plugins::mount_all(&ctx, &data_dir)`, then `aria_ui::run_app_with` with the same
directory. Do not remount inside `run_app`.
