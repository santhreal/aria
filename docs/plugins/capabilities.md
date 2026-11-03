# Capabilities across surfaces

Every plugin capability runs through the same host services on `PluginContext`
that the editor UI, CLI, and assistant call. In-process Rust plugins call the
services directly, native C ABI plugins call slots on `AriaHostV1` (wrapped by
`aria_sdk::Host`), and WebAssembly components call imports of the `plugin` world
in `plugin.wit`.

## Host call matrix

The table below maps every `AriaHostV1` function slot and every `plugin.wit`
import to its in-process host service, `aria_sdk::Host` wrapper, and required
`[permissions]` entry in [`plugin.toml`](packages.md#permissions).

| Capability | In-process (`aria_plugin`) | Native C ABI (`AriaHostV1`) | `aria_sdk::Host` | WebAssembly WIT (`plugin.wit`) | Permission |
| --- | --- | --- | --- | --- | --- |
| Release C string | n/a | `free_string` | handled by `Host` | n/a | none |
| Release C byte buffer | n/a | `free_bytes` | handled by `Host` | n/a | none |
| Write plugin log line | `PluginContext::log` | `log` | `log` | `log` | none |
| Read package storage | `Package::data_dir` | `storage_get` | `storage_get` | `storage-get` | none |
| Write package storage | `Package::data_dir` | `storage_set` | `storage_set` | `storage-set` | none |
| Delete package storage | `Package::data_dir` | `storage_delete` | `storage_delete` | `storage-delete` | none |
| List package storage | `Package::data_dir` | `storage_keys` | `storage_keys` | `storage-keys` | none |
| Read setting value | `SettingsService::get` | `setting_get` | `setting_get`, `setting_get_json` | `setting-get` | none |
| Register command | `CommandRegistry::register` | `register_command` | `register_command` | `register-command` | mount only |
| Subscribe to event | `EventBus` | `subscribe` | `subscribe`, `subscribe_with_priority` | `subscribe` | mount only |
| Register importer | `PluginContext::register_importer` | `register_importer` | `register_importer` | `register-importer` | mount only |
| Register exporter | `PluginContext::register_exporter` | `register_exporter` | `register_exporter` | `register-exporter` | mount only |
| Register encoder | `PluginContext::register_encoder` | `register_encoder` | `register_encoder`, `register_encoder_spec` | `register-encoder` | mount only |
| Register decoder | `PluginContext::register_decoder` | `register_decoder` | `register_decoder` | `register-decoder` | mount only (`media = "read"` or `"read-write"`) |
| Register audio effect | `AudioEffectRegistry` | `register_audio_effect` | `register_audio_effect` | `register-audio-effect` | mount only |
| Register audio analyzer | `AnalyzerRegistry` | `register_analyzer` | `register_analyzer`, `register_analyzer_spec` | `register-analyzer` | mount only |
| Register panel | `PanelRegistry` | `register_panel` | `register_panel`, `register_panel_spec` | `register-panel` | mount only |
| Redraw panel | `PanelRegistry::refresh` | `refresh_panel` | `refresh_panel` | `refresh-panel` | panel owner |
| Register viewer overlay | `ViewerOverlayRegistry` | `register_viewer_overlay` | `register_viewer_overlay` | `register-viewer-overlay` | mount only |
| Redraw viewer overlay | `ViewerOverlayRegistry::refresh` | `refresh_viewer_overlay` | `refresh_viewer_overlay` | `refresh-viewer-overlay` | overlay owner |
| Start background job | `PluginContext::spawn_job` | `start_job` | `start_job` | `start-job` | none |
| Register AI tool | `PluginContext::register_ai_tool` | `register_ai_tool` | `register_ai_tool` | `register-ai-tool` | mount only |
| Register video scope | `PluginContext::register_scope` | `register_scope` | `register_scope` | `register-scope` | mount only |
| Register GPU effect | `EffectRegistry` | `register_effect` | `register_effect`, `register_effect_passes`, `register_effect_graph` | `register-effect`, `register-effect-graph`, `set-effect-category` | mount only |
| Register CPU effect | `EffectRegistry` | `register_cpu_effect` | `register_cpu_effect`, `register_cpu_effect_spec` | `register-cpu-effect` | mount only |
| Register transition | `EffectRegistry` | `register_transition` | `register_transition` | `register-transition` | mount only |
| Register generator | `EffectRegistry` | `register_generator` | `register_generator`, `register_generator_passes`, `register_generator_graph`, `register_cpu_generator`, `register_cpu_generator_spec` | `register-generator` | mount only |
| Register plugin texture | `PluginContext::register_texture` | `register_texture` | `register_texture` | `register-texture` | mount only |
| Register clip type | `ClipTypeRegistry` | `register_clip_type` | `register_clip_type`, `define_clip_type`, `define_cpu_clip_type`, `define_cpu_clip_type_spec` | `register-clip-type`, `define-clip-type` | mount only |
| Register driver language | `DriverRegistry` | `register_driver_language` | `register_driver_language`, `register_driver_language_spec` | `register-driver-language` | mount only |
| Register preset | `PresetRegistry` | `register_preset` | `register_preset`, `register_preset_json` | `register-preset` | mount only |
| Register theme | `PluginContext::register_theme` | `register_theme` | `register_theme` | `register-theme` | mount only |
| Register workspace | `WorkspaceRegistry` | `register_workspace` | `register_workspace` | `register-workspace` | mount only |
| Read project snapshot | `ProjectHost::snapshot` | `project_snapshot` | `project_snapshot`, `project_snapshot_json` | `project-snapshot` | `project = "read"` or `"read-write"` |
| Read editor view | `EditorView::current` | `editor_view` | `editor_view`, `editor_view_json` | `editor-view` | `project = "read"` or `"read-write"` |
| Look up clip by id | `ProjectHost::clip` | `clip` | `clip`, `clip_json` | `clip` | `project = "read"` or `"read-write"` |
| Query clips in range | `ProjectHost::clips_in_range` | `clips_in_range` | `clips_in_range`, `clips_in_range_json` | `clips-in-range` | `project = "read"` or `"read-write"` |
| Query clips at tick | `ProjectHost::clips_at` | `clips_at` | `clips_at`, `clips_at_json` | `clips-at` | `project = "read"` or `"read-write"` |
| Query selected clips | `ProjectHost::selected_clips` | `selected_clips` | `selected_clips`, `selected_clips_json` | `selected-clips` | `project = "read"` or `"read-write"` |
| Query markers in range | `ProjectHost::markers_in_range` | `markers_in_range` | `markers_in_range`, `markers_in_range_json` | `markers-in-range` | `project = "read"` or `"read-write"` |
| Query In and Out marks | `PluginContext::in_out` | `in_out` | `in_out`, `in_out_json` | `in-out` | `project = "read"` or `"read-write"` |
| Render project or clip frame | `RenderHost::frame` | `render_frame` | `render_frame` | `render-frame` | `project = "read"` or `"read-write"` |
| Apply edit transaction | `ProjectHost::transact` | `project_transact` | `project_transact`, `project_transact_json` | `project-transact` | `project = "read-write"` |
| Undo latest edit step | `ProjectHost::undo` | `project_undo` | `project_undo` | `project-undo` | `project = "read-write"` |
| Seek playhead | `EditorView::seek` | `editor_seek` | `editor_seek`, `editor_seek_json` | `editor-seek` | `project = "read-write"` |
| Start or shuttle playback | `EditorView::play` | `editor_play` | `editor_play` | `editor-play` | `project = "read-write"` |
| Stop playback | `EditorView::stop` | `editor_stop` | `editor_stop` | `editor-stop` | `project = "read-write"` |
| Open `.aria` project | `ProjectHost::open` | `project_open` | `project_open` | `project-open` | `project = "read-write"` and `fs` read |
| Save open `.aria` project | `ProjectHost::save` | `project_save` | `project_save` | `project-save` | `project = "read-write"` and `fs` write |
| Save `.aria` project to path | `ProjectHost::save_as` | `project_save_as` | `project_save_as` | `project-save-as` | `project = "read-write"` and `fs` write |
| Sandboxed file read | `std::fs` | OS file I/O | OS file I/O | `fs-read`, `fs-read-at`, `fs-len` | `fs` read scope or active import/decode path |
| Sandboxed file write | `std::fs` | OS file I/O | OS file I/O | `fs-write` | `fs` write scope or active export/encode path |

## Frame rendering

`RenderHost` composites a sequence or a single clip on the process's GPU device
and returns straight-alpha RGBA8 pixels (`width * height * 4` bytes, top row
first). External plugins call the same compositor path without requiring direct
`wgpu` access (`gpu = true` is not required when `project = "read"` or
`"read-write"` is granted).

| Parameter | In-process (`aria_plugin`) | `aria_sdk` | WebAssembly WIT (`plugin.wit`) | C ABI (`AriaFrameRequest`) |
| --- | --- | --- | --- | --- |
| Open sequence | `FrameSource::Open` | `FrameSource::Open` | `frame-source::open` | `source_kind = ARIA_FRAME_SOURCE_OPEN` (`0`), `source_id = 0` |
| Root sequence | `FrameSource::Sequence(None)` | `FrameSource::Sequence(None)` | `frame-source::sequence(none)` | `source_kind = ARIA_FRAME_SOURCE_SEQUENCE` (`1`), `source_id = 0` |
| Nested sequence | `FrameSource::Sequence(Some(id))` | `FrameSource::Sequence(Some(id))` | `frame-source::sequence(some(id))` | `source_kind = ARIA_FRAME_SOURCE_SEQUENCE` (`1`), `source_id = id` |
| Single clip | `FrameSource::Clip(id)` | `FrameSource::Clip(id)` | `frame-source::clip(id)` | `source_kind = ARIA_FRAME_SOURCE_CLIP` (`2`), `source_id = id` |
| Canvas size | `FrameSize::Canvas` | `FrameSize::Canvas` | `frame-size::canvas` | `size_kind = ARIA_FRAME_SIZE_CANVAS` (`0`) |
| Fit within box | `FrameSize::Fit { max_width, max_height }` | `FrameSize::Fit { max_width, max_height }` | `frame-size::fit((max_width, max_height))` | `size_kind = ARIA_FRAME_SIZE_FIT` (`1`), `width = max_width`, `height = max_height` |
| Exact size | `FrameSize::Exact { width, height }` | `FrameSize::Exact { width, height }` | `frame-size::exact((width, height))` | `size_kind = ARIA_FRAME_SIZE_EXACT` (`2`), `width`, `height` |

Validation and bounds:

- Requires `project = "read"` or `"read-write"` in `[permissions]`. When denied,
  the host records a `CapabilityDenial` on `PluginContext` and returns an error.
- `time` must have `rate.num > 0` and `rate.den > 0`.
- Each requested or resolved output dimension must lie in
  `1..=MAX_RENDER_FRAME_DIMENSION` (`ARIA_FRAME_MAX_SIDE = 4096`). Requests with
  a zero dimension or a side above `4096` fail before allocating a texture.
- Unknown sequence or clip IDs fail with the model lookup error.
- To display a rendered frame inside a declarative panel (`UiNode::Image`),
  encode the RGBA8 buffer as a PNG or JPEG data URL
  (`data:image/png;base64,...`) and pass it to `UiNode::Image { src, width, height, alt }`.

### In-process Rust

```rust
use aria_core::{RationalTime, RgbaImage};
use aria_plugin::{FrameRequest, FrameSize, PluginContext, ProjectHost, RenderHost};

pub fn render_playhead_thumbnail(ctx: &PluginContext) -> Result<RgbaImage, String> {
    let project = ctx.get::<ProjectHost>().snapshot();
    let time = ctx
        .editor_view()
        .map(|view| view.playhead)
        .unwrap_or_else(|| RationalTime::new(0, project.timeline().frame_rate));
    let request = FrameRequest::open().size(FrameSize::Fit {
        max_width: 320,
        max_height: 180,
    });
    ctx.get::<RenderHost>()
        .frame(&project, time, &request)
        .map_err(|err| err.to_string())
}
```

### Native (`aria-sdk`)

```rust
use aria_sdk::{FrameSize, FrameSource, Host, Rational, RationalTime, RenderedFrame};

pub fn grab_clip_poster(host: &Host, clip_id: u64) -> Result<RenderedFrame, String> {
    let clip = host
        .clip(clip_id)?
        .ok_or_else(|| format!("clip {clip_id} not found"))?;
    let midpoint = RationalTime::new(
        clip.start.value + (clip.end.value - clip.start.value) / 2,
        Rational::new(clip.start.rate.num, clip.start.rate.den),
    );
    host.render_frame(
        FrameSource::Clip(clip_id),
        midpoint,
        FrameSize::Fit {
            max_width: 240,
            max_height: 135,
        },
    )
}
```

### WebAssembly (`plugin.wit`)

```rust
let frame = render_frame(
    FrameSource::Open,
    RationalTime {
        value: 0,
        rate_num: 24,
        rate_den: 1,
    },
    FrameSize::Fit((160, 90)),
)?;
```

## Transport and playhead control

`EditorView` holds the published `ViewState` and the transport queue drained by
the window thread. Moving the playhead or starting and stopping playback from a
plugin runs through `EditorView::seek`, `EditorView::play`, and
`EditorView::stop`, which update `ViewState`, post `PlayheadMoved`,
`PlaybackStarted`, or `PlaybackStopped` on `EventBus`, and wake `AppState` to
run `set_playhead`, `set_shuttle_speed`, or `stop_transport`.

| Operation | In-process (`EditorView`) | `aria_sdk::Host` | WebAssembly WIT (`plugin.wit`) | Event posted |
| --- | --- | --- | --- | --- |
| Move playhead | `seek(at, bus)` | `editor_seek(time)`, `editor_seek_json(tick_json)` | `editor-seek(time)` | `PlayheadMoved { at }` |
| Start or shuttle playback | `play(speed, bus)` | `editor_play(speed)` | `editor-play(speed)` | `PlaybackStarted { at, speed }` |
| Stop playback | `stop(bus)` | `editor_stop()` | `editor-stop()` | `PlaybackStopped { at }` (when playing) |

Validation and bounds:

- Requires `project = "read-write"` in `[permissions]` for external plugins.
- `seek(at)` requires `at.value >= 0` and `at.rate.num > 0 && at.rate.den > 0`.
  When `ProjectHost` is registered, `seek` rescales `at` to the open sequence's
  `frame_rate` and clamps it to the sequence duration when the duration is
  positive, returning the resulting `RationalTime`.
- `play(speed)` requires a non-zero integer multiplier in
  `MIN_PLAYBACK_SPEED..=MAX_PLAYBACK_SPEED` (`-8..=8`). Zero or out-of-range
  speeds return an error without changing transport state.
- `stop()` sets `playing = false` and `speed = 1` and posts `PlaybackStopped`
  when `playing` was `true`; calling `stop()` while already stopped is a no-op
  that succeeds without posting a duplicate event.

### In-process Rust

```rust
use aria_core::RationalTime;
use aria_plugin::{EditorView, PluginContext, ProjectHost};

pub fn jump_to_start_and_play(ctx: &PluginContext) -> Result<(), String> {
    let rate = ctx.get::<ProjectHost>().snapshot().timeline().frame_rate;
    let view = ctx.get::<EditorView>();
    view.seek(RationalTime::new(0, rate), ctx.events())?;
    view.play(1, ctx.events())
}
```

### Native (`aria-sdk`)

```rust
use aria_sdk::{Host, Rational, RationalTime};

pub fn shuttle_forward(host: &Host) -> Result<RationalTime, String> {
    let landed = host.editor_seek(RationalTime::new(48, Rational::FPS_24))?;
    host.editor_play(2)?;
    Ok(landed)
}
```

### WebAssembly (`plugin.wit`)

```rust
let landed = editor_seek(RationalTime {
    value: 24,
    rate_num: 24,
    rate_den: 1,
})?;
editor_play(1)?;
editor_stop()?;
```

## Project session open and save

`ProjectHost::open`, `ProjectHost::save`, and `ProjectHost::save_as` execute
`Command::Project(ProjectCommand::{Open, Save})` outside undo history, matching
**File > Open Project**, **File > Save**, and **File > Save As**. Both the
editor window (`AppState::open_project`, `save_project`, `save_project_as`) and
plugins call these methods on `ProjectHost`.

| Operation | In-process (`ProjectHost`) | `aria_sdk::Host` | WebAssembly WIT (`plugin.wit`) | Required permissions | Events posted |
| --- | --- | --- | --- | --- | --- |
| Read current path | `project_path()` | via `project_snapshot` | via `project-snapshot` | `project = "read"` or `"read-write"` | none |
| Check unsaved edits | `is_dirty()` | via `CommandApplied` / `ProjectSaved` | via `command-applied` / `project-saved` | `project = "read"` or `"read-write"` | none |
| Open project file | `open(&origin, path)` | `project_open(path)` | `project-open(path)` | `project = "read-write"` and `fs` read for `path` | `ProjectClosed`, `ProjectOpened`, `CommandApplied` |
| Save to current path | `save(&origin)` | `project_save()` | `project-save()` | `project = "read-write"` and `fs` write for `project_path()` | `ProjectSaved`, `CommandApplied` |
| Save to new path | `save_as(&origin, path)` | `project_save_as(path)` | `project-save-as(path)` | `project = "read-write"` and `fs` write for `path` | `ProjectSaved`, `CommandApplied` |

Validation, permissions, and session synchronization:

- Cannot run inside an open `EditGroup` or `Transaction`: `ProjectCommand::Open`
  and `ProjectCommand::Save` are session commands and fail with
  `TransactError::NotUndoable` inside a group.
- `open` fails with `EngineError::Refused` when `is_dirty()` is `true` ("project
  has unsaved changes; save the project before opening another") so a plugin
  cannot discard unsaved edits without saving first.
- `save` fails with `EngineError::Refused` when `project_path()` is `None` (an
  unsaved project); use `save_as` to assign the initial `.aria` path.
- External plugins require `project = "read-write"` plus `permissions.fs`
  coverage for the target path (`host_wire::check_project_fs_path` with `"read"`
  for `project_open` and `"write"` for `project_save` and `project_save_as`).
  Denied calls record a `CapabilityDenial` (`Project` or `Fs` scope) on
  `PluginContext` and return an error.
- When a non-editor origin opens or saves a project through `ProjectHost`,
  `AppState::sync_from_plugin_host` updates the window's dirty indicator,
  recovery file, recent-projects list, and (on open) resets selection, In and
  Out marks, and the playhead.
