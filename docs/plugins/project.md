# Project, frames, and sound

## Project

`ProjectHost` is the one path that reads and edits the project. The editor,
the assistant, and render jobs edit through it too. The engine plugin registers
it; a plugin declares `uses: &["ProjectHost"]` and takes it with
`ctx.try_get::<ProjectHost>()`.

| Call | Result |
| --- | --- |
| `new(engine, ctx)` | Wraps `Arc<Mutex<Engine>>` in a `ProjectHost` using `ctx`'s event bus |
| `engine()` | The underlying `&Arc<Mutex<Engine>>` |
| `snapshot()` | The project as an immutable `Arc<Project>`. A snapshot held across an edit costs one copy of the project |
| `revision()` | The engine revision, bumped by every change |
| `clip(id)` | `Option<ClipRef>` (`id`, `track`, `start`, `end`, `kind`, `name`) across the root timeline and nested sequences |
| `clips_in_range(sequence, range)` | `Vec<ClipRef>` overlapping the half-open `range` (`None` for the open sequence, `Some(id)` for a nested one), in track order then time order |
| `clips_at(sequence, tick)` | `Vec<ClipRef>` covering `tick` (`start <= tick < end`), in track order then time order |
| `selected_clips(selection)` | `Vec<ClipRef>` for `selection` in selection order, omitting ids no longer in the project (`ctx.selected_clips()` reads `EditorView` directly) |
| `markers_in_range(sequence, range)` | `Vec<Marker>` in the sequence whose point or span overlaps the half-open `range`, ordered by tick then id |
| `holder()` | The origin holding the project from another thread, if any |
| `apply(&origin, command)` | Proposes and applies one command as its own undo step, or as part of this thread's open group |
| `apply_observed(&origin, command, before_event)` | Proposes and applies one command, running `before_event` under the engine lock before announcing |
| `group(&origin)` | Opens an `EditGroup`: every command this thread applies until `commit` is one undo step. Dropping it rolls it back |
| `transact_with(origin, label, body)` | Runs `body` with a `Transaction`: `tx.apply(command)` and `tx.project()`. An `Err` or a panic from `body` reverts it |
| `transact(origin, label, commands)` | Applies the commands as one undo step named `label`, or none of them |
| `undo()` | Undoes the latest step (`Ok(false)` while this thread has a group open) |
| `redo()` | Redoes the latest undone step (`Ok(false)` while this thread has a group open) |
| `history_jump(depth)` | Jumps undo history to `depth` (`Ok(false)` while this thread has a group open) |
| `replace_session(&origin, f)` | Runs `f` on the engine with no edit in flight, for a session replacement that is not a command, then posts `ProjectClosed` and `ProjectOpened` |

`PluginContext` also exposes convenience query methods (`ctx.clip(id)`,
`ctx.clips_in_range(sequence, range)`, `ctx.clips_at(sequence, tick)`,
`ctx.selected_clips()`, `ctx.in_out()`, `ctx.markers_in_range(sequence, range)`)
that read `ProjectHost` and `EditorView`.

| Type | Fields / Variants / Methods |
| --- | --- |
| `Origin` | `Editor`, `Assistant`, `Plugin` |
| `CommandApplied` | `command`, `outcome`, `revision`, `origin` |
| `Receipt` | `revision`, `outcomes` |
| `EditGroup` | `commit`, `commit_as`, `rollback` |
| `Transaction` | `project`, `origin`, `apply` |
| `TransactError` | `Busy`, `NotUndoable`, `Refused`, `Failed`, `Aborted`, `Panicked`, `Reentrant` |
| `ClipRef` | `id`, `track`, `start`, `end`, `kind`, `name` |
| `InOut` | `mark_in`, `mark_out` |

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
| `new(gpu)` | Creates a `RenderHost` on `GpuContext` |
| `gpu()` | The `GpuContext` the host renders on, for creating a target texture |
| `frame_size(&project, &request)` | The width and height of a frame of `request` |
| `frame(&project, time, &request)` | The frame at `time` as a straight-alpha `RgbaImage` |
| `frames(&project, range, &request, each)` | One frame per tick of `range` at the range's rate, passed to `each` until it returns `ControlFlow::Break` |
| `frame_to_texture(&project, time, &request, &texture)` | The frame written premultiplied into an `Rgba8Unorm` texture with `COPY_DST` usage, with no copy to the CPU |

A `FrameRequest` sets the source, the size, and motion blur. The constructors
`FrameRequest::open()`, `sequence(id)`, and `clip(id)` take canvas size without
motion blur; `.size(size)` and `.motion_blur(on)` change one field.

| Field | Values |
| --- | --- |
| `source` | `FrameSource::Open`: the sequence open for editing. `FrameSource::Sequence(None)`: the root sequence; `Sequence(Some(id))`: a nested one, whichever is open. `FrameSource::Clip(id)`: one clip of any sequence as placed on its canvas, with its effects, over transparency |
| `size` | `FrameSize::Canvas`; `Fit { max_width, max_height }`, scaled down only; `Exact { width, height }`, scaled to fit and centered over the background |
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
| `new()` | Creates an `AudioHost` |
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
