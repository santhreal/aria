# WIT reference

<!-- Generated from crates/aria-plugin-wasm/wit/plugin.wit by crates/aria-plugin-wasm/tests/wit_reference.rs. Edit the WIT comments, then run `ARIA_BLESS_DOCS=1 cargo test -p aria-plugin-wasm --test wit_reference`. -->

Package `aria:plugin@1.0.0`, world `plugin`. WebAssembly component world for Aria plugins (API major 1).

A component calls the host imports, implements the guest exports, and passes the types below between them. [WebAssembly plugins](wasm.md) lists the permission each import requires and the chapter that covers each function.

## Host imports

### import `log`

```wit
import log: func(level: log-level, text: string);
```

Emit a log line at `level`. The line is kept in the plugin's log in Settings > Plugins.

### import `storage-get`

```wit
import storage-get: func(key: string) -> result<option<list<u8>>, string>;
```

Read a key from the package's storage directory, `<data_root>/plugin-data/<id>`.

### import `storage-set`

```wit
import storage-set: func(key: string, value: list<u8>) -> result<_, string>;
```

Write `value` under `key` in the package's storage directory.

### import `storage-delete`

```wit
import storage-delete: func(key: string) -> result<_, string>;
```

Delete `key` from the package's storage directory.

### import `storage-keys`

```wit
import storage-keys: func() -> result<list<string>, string>;
```

List every key in the package's storage directory in sorted order.

### import `setting-get`

```wit
import setting-get: func(key: string) -> result<option<string>, string>;
```

Read a setting value as JSON text, or `none` if unset.

### import `register-command`

```wit
import register-command: func(id: string, title: string) -> result<_, string>;
```

Register a palette command during `mount`.

### import `subscribe`

```wit
import subscribe: func(event-kind: string) -> result<_, string>;
```

Subscribe to an event kind during `mount`. The kinds, payloads, and replies are listed in [Events](events.md#external-plugins); the event type name (`CommandApplied`) is accepted for its kind (`command-applied`). `resolve-dropped:<n>` sets the handler priority to the `s32` `n`; the default is 0, and no other kind takes a priority.

### import `register-importer`

```wit
import register-importer: func(spec: importer-spec) -> result<_, string>;
```

Register a timeline importer during `mount`.

### import `register-exporter`

```wit
import register-exporter: func(spec: exporter-spec) -> result<_, string>;
```

Register a timeline exporter during `mount`.

### import `register-encoder`

```wit
import register-encoder: func(spec: encoder-spec) -> result<_, string>;
```

Register a media format encoder during `mount`.

### import `register-audio-effect`

```wit
import register-audio-effect: func(spec: audio-effect-spec) -> result<_, string>;
```

Register an audio effect during `mount`.

### import `register-panel`

```wit
import register-panel: func(spec: panel-spec) -> result<_, string>;
```

Register a docked declarative UI panel during `mount`.

### import `refresh-panel`

```wit
import refresh-panel: func(id: string) -> result<_, string>;
```

Queue a rebuild of a panel owned by this plugin.

### import `register-viewer-overlay`

```wit
import register-viewer-overlay: func(spec: overlay-spec) -> result<_, string>;
```

Register a viewer overlay during `mount`.

### import `refresh-viewer-overlay`

```wit
import refresh-viewer-overlay: func(id: string) -> result<_, string>;
```

Queue a scene rebuild of a viewer overlay owned by this plugin.

### import `project-snapshot`

```wit
import project-snapshot: func() -> result<string, string>;
```

Serialize the open project as serde JSON (`aria_models::Project`). Requires permission `project = "read"` or `"read-write"`.

### import `editor-view`

```wit
import editor-view: func() -> result<string, string>;
```

Serialize the editor view (selection, playhead, in/out) as JSON. Requires permission `project = "read"` or `"read-write"`.

### import `clip`

```wit
import clip: func(id: u64) -> result<option<clip-ref>, string>;
```

Look up clip `id` across the project's sequences. Requires permission `project = "read"` or `"read-write"`.

### import `clips-in-range`

```wit
import clips-in-range: func(sequence: option<u64>, range: time-range) -> result<list<clip-ref>, string>;
```

Clips on `sequence` (`none` for the open sequence) overlapping the half-open `range`, in track order then timeline start order. Requires permission `project = "read"` or `"read-write"`.

### import `clips-at`

```wit
import clips-at: func(sequence: option<u64>, tick: rational-time) -> result<list<clip-ref>, string>;
```

Clips on `sequence` (`none` for the open sequence) covering `tick`, in track order then timeline start order. Requires permission `project = "read"` or `"read-write"`.

### import `selected-clips`

```wit
import selected-clips: func() -> result<list<clip-ref>, string>;
```

Selected clips from `EditorView` resolved to `clip-ref`, dropping deleted ids. Requires permission `project = "read"` or `"read-write"`.

### import `markers-in-range`

```wit
import markers-in-range: func(sequence: option<u64>, range: time-range) -> result<list<marker>, string>;
```

Ruler markers on `sequence` (`none` for the open sequence) overlapping the half-open `range`, in timeline order. Requires permission `project = "read"` or `"read-write"`.

### import `in-out`

```wit
import in-out: func() -> result<in-out-marks, string>;
```

Sequence In and Out points from `EditorView`. Requires permission `project = "read"` or `"read-write"`.

### import `project-transact`

```wit
import project-transact: func(label: string, commands-json: string) -> result<string, string>;
```

Apply a command list (`Vec<aria_commands::Command>` JSON) as one undo step. Requires permission `project = "read-write"`.

### import `project-undo`

```wit
import project-undo: func() -> result<bool, string>;
```

Undo the latest step on the project host. Requires permission `project = "read-write"`.

### import `fs-read`

```wit
import fs-read: func(path: string) -> result<list<u8>, string>;
```

Read bytes from a permitted filesystem path or active import target.

### import `fs-write`

```wit
import fs-write: func(path: string, bytes: list<u8>) -> result<_, string>;
```

Write bytes to a permitted filesystem path or active export target.

### import `start-job`

```wit
import start-job: func(label: string) -> result<u64, string>;
```

Start a background job on the jobs board, labeled with the plugin's name and `label`. The host calls `job-step` with the returned id until a step is done, Cancel is pressed, or a step fails, then `job-end`.

### import `register-decoder`

```wit
import register-decoder: func(spec: decoder-spec) -> result<_, string>;
```

Register a media decoder during `mount`.

### import `fs-read-at`

```wit
import fs-read-at: func(path: string, offset: u64, len: u32) -> result<list<u8>, string>;
```

Read up to `len` bytes from byte `offset` of a permitted path or the file being decoded: fewer only at end of file. At most 64 MiB per call.

### import `fs-len`

```wit
import fs-len: func(path: string) -> result<u64, string>;
```

Length in bytes of a permitted path or the file being decoded.

### import `register-ai-tool`

```wit
import register-ai-tool: func(spec: ai-tool-spec) -> result<_, string>;
```

Offer the assistant an AI tool during `mount`. `ai-tool-call` runs it.

### import `register-scope`

```wit
import register-scope: func(spec: scope-spec) -> result<_, string>;
```

Add a scope to the scope strip during `mount`. `scope-measure` draws it.

### import `set-effect-category`

```wit
import set-effect-category: func(id: string, category: string) -> result<_, string>;
```

Set the category of an audio or video effect registered by this plugin.

### import `register-effect`

```wit
import register-effect: func(spec: effect-spec, passes: list<string>) -> result<_, string>;
```

Register a WGSL visual effect during `mount`.

### import `register-cpu-effect`

```wit
import register-cpu-effect: func(spec: effect-spec) -> result<_, string>;
```

Register a CPU visual effect or generator during `mount`. `cpu-effect-process` renders it.

### import `register-transition`

```wit
import register-transition: func(spec: transition-spec, wgsl: string) -> result<_, string>;
```

Register a WGSL transition during `mount`.

### import `register-generator`

```wit
import register-generator: func(spec: effect-spec, passes: list<string>) -> result<_, string>;
```

Register a WGSL generator during `mount`.

### import `register-effect-graph`

```wit
import register-effect-graph: func(spec: effect-spec, traits: effect-traits, passes: list<graph-pass>) -> result<_, string>;
```

Register a WGSL effect graph during `mount`: an effect, or a generator when `spec.generator` is set.

### import `register-texture`

```wit
import register-texture: func(id: string, width: u32, height: u32, pixels: list<u8>) -> result<_, string>;
```

Register straight-alpha RGBA8 `pixels`, `width * height * 4` bytes with the top row first, as the plugin texture `id` during `mount`.

### import `register-clip-type`

```wit
import register-clip-type: func(spec: clip-type-spec) -> result<_, string>;
```

Register a generator-backed clip type during `mount`.

### import `define-clip-type`

```wit
import define-clip-type: func(spec: clip-type-spec, schema: clip-type-schema, render: clip-type-render) -> result<_, string>;
```

Register during `mount` a clip type whose clips store `schema.fields` and that `render` draws. The host registers a generator under the type's id; `spec.generator` is empty or that id.

### import `register-driver-language`

```wit
import register-driver-language: func(spec: driver-language-spec) -> result<_, string>;
```

Register a property driver language during `mount`. `driver-evaluate` evaluates it.

### import `register-preset`

```wit
import register-preset: func(preset-json: string) -> result<_, string>;
```

Register an animation and effect preset JSON during `mount`.

### import `register-theme`

```wit
import register-theme: func(id: string, toml: string) -> result<_, string>;
```

Register a theme TOML under `id` during `mount`.

### import `register-workspace`

```wit
import register-workspace: func(spec: workspace-spec) -> result<_, string>;
```

Register a workspace arrangement during `mount`.

### import `register-analyzer`

```wit
import register-analyzer: func(spec: analyzer-spec) -> result<_, string>;
```

Register an audio analyzer during `mount`.

## Guest exports

### export `mount`

```wit
export mount: func() -> result<_, string>;
```

Mount the plugin. Registration calls are permitted only during `mount`.

### export `unmount`

```wit
export unmount: func() -> result<_, string>;
```

Unmount the plugin before unload or reload.

### export `command-run`

```wit
export command-run: func(id: string, args-json: string) -> result<_, string>;
```

Run a command previously registered by this plugin.

### export `event`

```wit
export event: func(kind: string, event-json: string) -> result<option<string>, string>;
```

Handle an event of a subscribed kind. `kind` is the kebab-case kind, whichever spelling subscribed; `event-json` is the event as a JSON object. The reply refuses or rewrites a `command-proposed` command and claims a `resolve-dropped` item; every other kind ignores it.

### export `%import`

```wit
export %import: func(importer-id: string, path: string) -> result<import-result, string>;
```

Import a timeline file at `path` using `importer-id`.

### export `%export`

```wit
export %export: func(exporter-id: string, project-json: string, path: string) -> result<report, string>;
```

Export `project-json` to `path` using `exporter-id`.

### export `audio-create`

```wit
export audio-create: func(effect-id: string, sample-rate: u32) -> result<instance, string>;
```

Create an audio processor instance for `effect-id` at `sample-rate`.

### export `audio-process`

```wit
export audio-process: func(instance: instance, params: list<audio-param>, block: audio-block) -> result<stereo-buffers, string>;
```

Process one stereo planar audio block on `instance`.

### export `audio-reset`

```wit
export audio-reset: func(instance: instance) -> result<_, string>;
```

Reset internal DSP state on `instance` after a transport seek.

### export `audio-drop`

```wit
export audio-drop: func(instance: instance) -> result<_, string>;
```

Drop an audio processor instance created by `audio-create`.

### export `panel-view`

```wit
export panel-view: func(id: string) -> list<ui-node>;
```

Build the flattened `ui-node` tree (`root = index 0`) for panel `id`.

### export `panel-event`

```wit
export panel-event: func(id: string, event: panel-event) -> result<_, string>;
```

Handle a user control interaction on panel `id`.

### export `overlay-scene`

```wit
export overlay-scene: func(id: string) -> list<overlay-shape>;
```

Build the shape list for viewer overlay `id`.

### export `overlay-event`

```wit
export overlay-event: func(id: string, event: overlay-event) -> result<_, string>;
```

Handle a user handle drag interaction on viewer overlay `id`.

### export `encoder-open`

```wit
export encoder-open: func(encoder-id: string, path: string, config: encoder-config) -> result<instance, string>;
```

Open an encoder session for `encoder-id` writing `config` to `path`.

### export `encoder-push-frame`

```wit
export encoder-push-frame: func(instance: instance, frame: encoder-frame) -> result<_, string>;
```

Push one straight-alpha RGBA8 video frame to an open encoder session.

### export `encoder-push-audio`

```wit
export encoder-push-audio: func(instance: instance, audio: encoder-audio) -> result<_, string>;
```

Push one interleaved `f32` audio block to an open encoder session.

### export `encoder-finish`

```wit
export encoder-finish: func(instance: instance) -> result<_, string>;
```

Finalize an open encoder session and flush its output via `fs-write`.

### export `encoder-drop`

```wit
export encoder-drop: func(instance: instance) -> result<_, string>;
```

Drop an open encoder session without finalizing it.

### export `job-step`

```wit
export job-step: func(job: u64) -> result<job-step, string>;
```

Run one step of job `job`, started by `start-job`.

### export `job-end`

```wit
export job-end: func(job: u64, cancelled: bool);
```

Job `job` ended: done, failed, or cancelled when `cancelled` is true.

### export `decoder-probe`

```wit
export decoder-probe: func(decoder-id: string, path: string) -> result<media-probe, decode-error>;
```

Describe `path` with decoder `decoder-id` without decoding it.

### export `decoder-open-video`

```wit
export decoder-open-video: func(decoder-id: string, path: string) -> result<opened-video, decode-error>;
```

Open the video stream of `path` with decoder `decoder-id`.

### export `decoder-seek`

```wit
export decoder-seek: func(instance: instance, pts: s64) -> result<_, string>;
```

Seek video stream `instance` to the sync frame at or before `pts`, in time-base ticks.

### export `decoder-next-frame`

```wit
export decoder-next-frame: func(instance: instance) -> result<option<decoded-frame>, string>;
```

The next frame of video stream `instance`, or none at end of stream.

### export `decoder-open-audio`

```wit
export decoder-open-audio: func(decoder-id: string, path: string, sample-rate: u32, channels: u16) -> result<instance, decode-error>;
```

Open the audio of `path` with decoder `decoder-id`, resampled to `sample-rate` and `channels`.

### export `decoder-read-audio`

```wit
export decoder-read-audio: func(instance: instance, max-samples: u32) -> result<list<f32>, string>;
```

Up to `max-samples` interleaved samples of audio stream `instance`: whole frames, fewer only at end of stream.

### export `decoder-seek-audio`

```wit
export decoder-seek-audio: func(instance: instance, frame: s64) -> result<_, string>;
```

Position the next read of audio stream `instance` at sample frame `frame`.

### export `decoder-close`

```wit
export decoder-close: func(instance: instance);
```

Drop the video or audio stream `instance`.

### export `ai-tool-call`

```wit
export ai-tool-call: func(name: string, arguments-json: string) -> result<ai-tool-output, string>;
```

Run the AI tool registered as `name` with `arguments-json`, an object matching its schema, on the assistant's turn thread.

### export `scope-measure`

```wit
export scope-measure: func(id: string, frame: scope-frame) -> result<scope-drawing, string>;
```

Measure `frame` with the scope registered as `id`, on the preview worker's thread while the scope strip is open.

### export `cpu-effect-process`

```wit
export cpu-effect-process: func(id: string, params: list<effect-param>, frame: video-frame) -> result<video-frame, string>;
```

Render one premultiplied RGBA16F video frame for a CPU effect or generator registered as `id`.

### export `driver-evaluate`

```wit
export driver-evaluate: func(id: string, expr: string, vars: driver-vars) -> result<param-value, string>;
```

Evaluate driver expression `expr` for the driver language registered as `id`.

### export `analyzer-open`

```wit
export analyzer-open: func(id: string, sample-rate: u32, channels: u16) -> result<instance, string>;
```

Open an audio analyzer session for `id` at `sample-rate` Hz and `channels` channels.

### export `analyzer-push`

```wit
export analyzer-push: func(instance: instance, samples: list<f32>) -> result<_, string>;
```

Feed interleaved `f32` audio samples (`frames * channels`) into analyzer `instance`.

### export `analyzer-finish`

```wit
export analyzer-finish: func(instance: instance) -> result<string, string>;
```

Finish analyzer `instance` and return its `AnalyzerResult` JSON.

### export `analyzer-drop`

```wit
export analyzer-drop: func(instance: instance);
```

Drop analyzer `instance` without finishing.

## Types

### enum `log-level`

Severity level for plugin log messages.

| Case | Description |
| --- | --- |
| `trace` | Step-by-step detail. |
| `debug` | Diagnostic detail. |
| `info` | Normal operation. |
| `warn` | A problem the plugin recovered from. |
| `error` | A failure. |

### record `importer-spec`

Timeline importer registration metadata.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Not empty, and unique among importers and exporters. |
| `name` | `string` | The format as a menu lists it, such as `CMX 3600 EDL`. |
| `extensions` | `list<string>` | The file extensions it reads, without the dot: at least one, each ASCII letters and digits. Matched without regard to case. |

### record `exporter-spec`

Timeline exporter registration metadata.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Not empty, and unique among importers and exporters. |
| `name` | `string` | The format as a menu lists it. |
| `extension` | `string` | The extension of the file it writes, without the dot: ASCII letters and digits, stored lowercase. |

### record `encoder-spec`

Media encoder registration metadata.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Not empty, and unique among encoders. |
| `name` | `string` | The format as the Export dialog lists it, such as `Apple ProRes 422`. |
| `extension` | `string` | The extension of the file it writes, without the dot: ASCII letters and digits, stored lowercase. Selecting the encoder sets the destination's extension. |
| `audio` | `bool` | The encoder takes audio through `encoder-push-audio`. When false, the Export dialog hides the Audio row and the export is video only. |

### record `encoder-config`

Configuration passed to `encoder-open`.

| Field | Type | Description |
| --- | --- | --- |
| `width` | `u32` | Frame width in pixels. |
| `height` | `u32` | Frame height in pixels. |
| `fps-num` | `s64` | Frame rate numerator: `fps-num / fps-den` frames per second. |
| `fps-den` | `s32` | Frame rate denominator. |
| `video-bitrate-kbps` | `u32` | Requested video bitrate in kilobits per second, or 0 when the export requests none. |
| `audio-bitrate-kbps` | `u32` | Requested audio bitrate in kilobits per second. The host passes 0. |

### record `encoder-frame`

One RGBA video frame passed to `encoder-push-frame`.

| Field | Type | Description |
| --- | --- | --- |
| `rgba` | `list<u8>` | Straight-alpha RGBA8 pixels: `height` rows of `stride` bytes, top row first. |
| `width` | `u32` | Frame width in pixels. |
| `height` | `u32` | Frame height in pixels. |
| `stride` | `u32` | Bytes per row, at least `width * 4`. |
| `pts-value` | `s64` | Presentation time in ticks of `pts-rate`: the frame starts `pts-value / pts-rate` seconds into the file. |
| `pts-rate` | `s32` | Ticks per second of `pts-value`. |

### record `encoder-audio`

One interleaved `f32` audio block passed to `encoder-push-audio`.

| Field | Type | Description |
| --- | --- | --- |
| `samples` | `list<f32>` | Interleaved samples, `channels` per sample frame. |
| `sample-rate` | `u32` | Sample frames per second. |
| `channels` | `u32` | Channels per sample frame. |
| `pts-value` | `s64` | Presentation time of the first sample frame in ticks of `pts-rate`. |
| `pts-rate` | `s32` | Ticks per second of `pts-value`. |

### record `import-result`

Result returned from a timeline import operation.

| Field | Type | Description |
| --- | --- | --- |
| `commands-json` | `string` | Serde JSON of `Vec<aria_commands::Command>`, applied as one transaction. |
| `skipped` | `list<string>` | Source items skipped by the importer. |

### record `report`

Report returned from a timeline export operation.

| Field | Type | Description |
| --- | --- | --- |
| `skipped` | `list<string>` | Project items that the target format could not represent. |

### record `audio-param-spec`

Scalar parameter definition for an audio effect.

| Field | Type | Description |
| --- | --- | --- |
| `name` | `string` | Parameter key, unique within the effect. |
| `label` | `string` | The parameter's row label. |
| `default-value` | `f32` | Value of a new insert, inside `min..=max`. |
| `min` | `f32` | Lowest value. |
| `max` | `f32` | Highest value, not below `min`. |

### record `audio-effect-spec`

Audio effect registration metadata.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Namespaced id with at least one `.`, such as `com.example.drive`. A shipped id or an id already registered is refused. |
| `label` | `string` | The name the Effect menu and the mixer inserts list. Not empty. |
| `params` | `list<audio-param-spec>` | Scalar parameters in slot order. `audio-process` receives their signals in this order. |
| `latency` | `u32` | Frames at 48 kHz by which the output trails the input, at most 48000. The mixer delays every parallel path by the same amount. |
| `sidechain` | `bool` | The effect reads a key signal: a mixer strip shows a Key from row, and `audio-block` carries the key lane. |

### type `instance`

Handle to a session the plugin holds: an audio processor, an encoder, a decoder stream, or an analyzer.

Alias of `u64`.

### record `audio-param`

Parameter signal for one parameter across an audio block.

| Field | Type | Description |
| --- | --- | --- |
| `constant` | `f32` | The value for the whole block, or the first frame's value when `per-frame` is set. |
| `per-frame` | `list<f32>` | One value per frame when the parameter is keyframed and changes within the block; empty otherwise. |

### record `audio-block`

Planar stereo audio block passed to `audio-process`.

| Field | Type | Description |
| --- | --- | --- |
| `left` | `list<f32>` | Left channel samples. The block holds as many frames as this list. |
| `right` | `list<f32>` | Right channel samples, as many as `left`. |
| `position` | `s64` | Sequence position of the first frame, in frames at `sample-rate`. |
| `sample-rate` | `u32` | Frames per second. |
| `sidechain-left` | `list<f32>` | Left channel of the key signal, or empty when the effect takes no key or the insert has no key lane. |
| `sidechain-right` | `list<f32>` | Right channel of the key signal, or empty as `sidechain-left` is. |

### record `stereo-buffers`

Processed planar stereo output buffers returned from `audio-process`.

| Field | Type | Description |
| --- | --- | --- |
| `left` | `list<f32>` | Processed left channel. The host copies at most the block's frame count; frames past a shorter list keep their input. |
| `right` | `list<f32>` | Processed right channel, copied as `left` is. |

### enum `panel-location`

Docked location for a declared plugin panel.

| Case | Description |
| --- | --- |
| `sidebar` | The left frame. |
| `inspector` | The right frame. |
| `timeline` | The bottom frame. |

### record `panel-spec`

Registration metadata for a docked plugin panel.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Not empty, and unique among panels. |
| `title` | `string` | The dock tab title and the Window menu row. Not empty. |
| `location` | [`panel-location`](#enum-panel-location) | The frame the panel opens in. |

### enum `tone`

Text color tone for `ui-node::text`.

| Case | Description |
| --- | --- |
| `normal` | The body text color. |
| `muted` | The secondary text color. |
| `success` | The theme's success color. |
| `warning` | The theme's warning color. |
| `danger` | The theme's danger color. |

### record `ui-text`

Paragraph text node with tone.

| Field | Type | Description |
| --- | --- | --- |
| `text` | `string` | The paragraph. |
| `tone` | [`tone`](#enum-tone) | Its color. |

### record `ui-field`

Key-value field row.

| Field | Type | Description |
| --- | --- | --- |
| `label` | `string` | The label on the left. |
| `value` | `string` | The value on the right. |

### record `ui-button`

Push button control.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Control id: unique in the tree, 1 to 128 bytes, no control characters. |
| `label` | `string` | The button text. |
| `disabled` | `bool` | Drawn disabled. A press sends no event. |

### record `ui-toggle`

Boolean toggle control.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Control id: unique in the tree, 1 to 128 bytes, no control characters. |
| `label` | `string` | The switch label. |
| `on` | `bool` | The current state. A click sends `panel-event::toggled` with the opposite. |

### record `ui-slider`

Numeric slider control.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Control id: unique in the tree, 1 to 128 bytes, no control characters. |
| `label` | `string` | The slider label. |
| `value` | `f64` | The current value, inside `min..=max`. |
| `min` | `f64` | The lowest value: finite and below `max`. |
| `max` | `f64` | The highest value: finite. |
| `step` | `f64` | The snap step, in `0..=max - min`. 0 clamps without snapping. |

### record `ui-input`

Single-line text input control.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Control id: unique in the tree, 1 to 128 bytes, no control characters. |
| `value` | `string` | The current text. |
| `placeholder` | `string` | The text shown while the box is empty. |

### record `ui-choice`

Multi-option chip selector control.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Control id: unique in the tree, 1 to 128 bytes, no control characters. |
| `label` | `string` | The selector label. |
| `options` | `list<string>` | The chip labels: 1 to 64. |
| `selected` | `u32` | Index of the selected option, below the option count. |

### record `ui-progress`

Progress bar indicator.

| Field | Type | Description |
| --- | --- | --- |
| `label` | `string` | The bar label. |
| `fraction` | `f32` | The filled fraction, 0.0 to 1.0. |

### record `ui-image`

Straight-alpha RGBA8 image node (`width * height * 4` bytes).

| Field | Type | Description |
| --- | --- | --- |
| `width` | `u32` | Width in pixels, 1 to 2048. |
| `height` | `u32` | Height in pixels, 1 to 2048. |
| `rgba` | `list<u8>` | `width * height * 4` bytes of straight-alpha RGBA8. |

### variant `ui-node`

One node in a flattened panel tree (`root = index 0`). Container nodes (`column`, `row`) hold indices into the flat `list<ui-node>`.

| Case | Payload | Description |
| --- | --- | --- |
| `column` | `list<u32>` | Children drawn top to bottom, as indices into the flat list. |
| `row` | `list<u32>` | Children drawn left to right, wrapping, as indices into the flat list. |
| `heading` | `string` | A title line. |
| `text` | [`ui-text`](#record-ui-text) | A paragraph in a tone. |
| `field` | [`ui-field`](#record-ui-field) | A label and value row. |
| `separator` |  | A rule between rows. |
| `button` | [`ui-button`](#record-ui-button) | A push button. |
| `toggle` | [`ui-toggle`](#record-ui-toggle) | A switch. |
| `slider` | [`ui-slider`](#record-ui-slider) | A track and value. |
| `input` | [`ui-input`](#record-ui-input) | A one-line text box. |
| `choice` | [`ui-choice`](#record-ui-choice) | A row of chips. |
| `progress` | [`ui-progress`](#record-ui-progress) | A bar filled to its fraction. |
| `image` | [`ui-image`](#record-ui-image) | An image, narrowed to fit the panel. |

### record `panel-toggled`

Payload for `panel-event::toggled`.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | The toggle's id. |
| `on` | `bool` | The new state. |

### record `panel-slid`

Payload for `panel-event::slid`.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | The slider's id. |
| `value` | `f64` | The value on release, clamped to the range and snapped to the step. |

### record `panel-submitted`

Payload for `panel-event::submitted`.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | The input's id. |
| `text` | `string` | The text when Enter was pressed, at most 16 KiB. |

### record `panel-chose`

Payload for `panel-event::chose`.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | The selector's id. |
| `index` | `u32` | Index of the chosen option. |

### variant `panel-event`

Interaction event delivered from a panel control to the plugin.

| Case | Payload | Description |
| --- | --- | --- |
| `pressed` | `string` | A button was pressed. The payload is its id. |
| `toggled` | [`panel-toggled`](#record-panel-toggled) | A toggle was clicked. |
| `slid` | [`panel-slid`](#record-panel-slid) | A slider was released. |
| `submitted` | [`panel-submitted`](#record-panel-submitted) | Enter was pressed in an input. Escape sends nothing. |
| `chose` | [`panel-chose`](#record-panel-chose) | A chip was clicked. |

### record `point`

2D coordinate or size in sequence frame pixels.

| Field | Type | Description |
| --- | --- | --- |
| `x` | `f32` | Horizontal coordinate or width, rightward. |
| `y` | `f32` | Vertical coordinate or height, downward. |

### variant `overlay-color`

Stroke or fill color for a viewer overlay shape.

| Case | Payload | Description |
| --- | --- | --- |
| `accent` |  | The theme accent color. |
| `text` |  | The theme text color. |
| `muted` |  | The theme muted color. |
| `success` |  | The theme success color. |
| `warning` |  | The theme warning color. |
| `danger` |  | The theme danger color. |
| `rgba` | `tuple<u8, u8, u8, u8>` | Straight-alpha sRGB bytes `(r, g, b, a)`. |

### enum `handle-look`

Visual glyph drawn for a draggable viewer overlay handle.

| Case | Description |
| --- | --- |
| `dot` | A dot. |
| `square` | A square. |
| `cross` | A cross. |

### record `overlay-line`

Straight line segment in a viewer overlay scene.

| Field | Type | Description |
| --- | --- | --- |
| `from` | [`point`](#record-point) | Start point in canvas pixels. |
| `to` | [`point`](#record-point) | End point in canvas pixels. |
| `color` | [`overlay-color`](#variant-overlay-color) | Stroke color. |
| `width` | `f32` | Stroke width in window pixels, above 0 and at most 32. |

### record `overlay-rect`

Axis-aligned rectangle in a viewer overlay scene.

| Field | Type | Description |
| --- | --- | --- |
| `origin` | [`point`](#record-point) | Top-left corner in canvas pixels. |
| `size` | [`point`](#record-point) | Width and height in canvas pixels, not negative. |
| `color` | [`overlay-color`](#variant-overlay-color) | Outline color. |
| `width` | `f32` | Stroke width in window pixels, above 0 and at most 32. |
| `fill` | `option<overlay-color>` | Fill color, or none for an outline only. |

### record `overlay-ellipse`

Axis-aligned ellipse in a viewer overlay scene.

| Field | Type | Description |
| --- | --- | --- |
| `center` | [`point`](#record-point) | Center in canvas pixels. |
| `radii` | [`point`](#record-point) | Horizontal and vertical radius in canvas pixels, not negative. |
| `color` | [`overlay-color`](#variant-overlay-color) | Outline color. |
| `width` | `f32` | Stroke width in window pixels, above 0 and at most 32. |
| `fill` | `option<overlay-color>` | Fill color, or none for an outline only. |

### record `overlay-path`

Polyline or closed polygon in a viewer overlay scene.

| Field | Type | Description |
| --- | --- | --- |
| `points` | `list<point>` | At least two points in canvas pixels. |
| `closed` | `bool` | Joins the last point back to the first. |
| `color` | [`overlay-color`](#variant-overlay-color) | Stroke color. |
| `width` | `f32` | Stroke width in window pixels, above 0 and at most 32. |

### record `overlay-text`

Text label in a viewer overlay scene.

| Field | Type | Description |
| --- | --- | --- |
| `at` | [`point`](#record-point) | Top-left corner of the line in canvas pixels. |
| `text` | `string` | One line of text, with no line break. |
| `color` | [`overlay-color`](#variant-overlay-color) | Text color. |
| `size` | `f32` | Text size in window pixels, 6 to 96. |

### record `overlay-handle`

Draggable handle in a viewer overlay scene.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Handle id: unique among the scene's handles, 1 to 128 bytes, no control characters. |
| `at` | [`point`](#record-point) | Center in canvas pixels. |
| `color` | [`overlay-color`](#variant-overlay-color) | Handle color. |
| `look` | [`handle-look`](#enum-handle-look) | The glyph, 12 pixels across. |

### variant `overlay-shape`

One vector primitive or handle in a viewer overlay scene.

| Case | Payload | Description |
| --- | --- | --- |
| `line` | [`overlay-line`](#record-overlay-line) | A straight stroke. |
| `rect` | [`overlay-rect`](#record-overlay-rect) | An axis-aligned rectangle. |
| `ellipse` | [`overlay-ellipse`](#record-overlay-ellipse) | An axis-aligned ellipse. |
| `path` | [`overlay-path`](#record-overlay-path) | Straight segments through its points. |
| `text` | [`overlay-text`](#record-overlay-text) | One line of text, drawn above every stroke. |
| `handle` | [`overlay-handle`](#record-overlay-handle) | A draggable handle, drawn above every stroke. |

### enum `drag-phase`

Phase of a pointer drag on an `overlay-handle`.

| Case | Description |
| --- | --- |
| `start` | The press on the handle. |
| `move` | A pointer step with the button held. |
| `end` | The release, wherever the pointer is. |

### record `overlay-drag`

Payload for `overlay-event::drag`.

| Field | Type | Description |
| --- | --- | --- |
| `handle` | `string` | Id of the pressed handle. |
| `phase` | [`drag-phase`](#enum-drag-phase) | The step of the drag. |
| `at` | [`point`](#record-point) | The pointer in canvas pixels. It may lie off the frame. |

### variant `overlay-event`

Interaction event delivered from a viewer overlay handle to the plugin.

| Case | Payload | Description |
| --- | --- | --- |
| `drag` | [`overlay-drag`](#record-overlay-drag) | A pointer drag on a handle. |

### record `overlay-spec`

Viewer overlay registration metadata.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Not empty, and unique among viewer overlays. |
| `title` | `string` | The overlay's name, shown before an error its handler returns. Not empty. |

### record `job-step`

One step of a background job, returned by `job-step`.

| Field | Type | Description |
| --- | --- | --- |
| `progress` | `option<f32>` | Fraction done, 0.0 to 1.0, or none while it is unknown. |
| `detail` | `string` | The line the jobs board shows. |
| `done` | `bool` | The job is finished, and `detail` is its last line. |
| `commit` | `bool` | The next step cannot be undone: Cancel is refused from here on. |

### enum `ai-tool-tier`

The trust an AI tool needs before the assistant runs it.

| Case | Description |
| --- | --- |
| `read-only` | Reads state and changes nothing. Runs without asking. |
| `workspace` | Changes the project or the editor in a way an undo reverses. |
| `system` | Deletes data, writes outside the project, or reaches another system. Runs once the call is allowed, unless Settings > AI > Autonomy is Full. |

### record `ai-tool-spec`

An AI tool the assistant lists.

| Field | Type | Description |
| --- | --- | --- |
| `name` | `string` | 1 to 56 bytes of lowercase ASCII letters, digits, and `_`. The assistant calls the tool `plugin_<name>`. |
| `description` | `string` | What the tool does, for the model. |
| `parameters-json` | `string` | A JSON Schema object describing the arguments, as JSON text. |
| `tier` | [`ai-tool-tier`](#enum-ai-tool-tier) | The trust the tool needs. |

### record `ai-tool-image`

A PNG image an AI tool returns, at most 2048 pixels on a side.

| Field | Type | Description |
| --- | --- | --- |
| `label` | `string` | A caption of a few words. |
| `png` | `list<u8>` | The PNG file bytes. |

### record `ai-tool-output`

What an AI tool returns to the model.

| Field | Type | Description |
| --- | --- | --- |
| `text` | `string` | The text the model reads. |
| `images` | `list<ai-tool-image>` | The images the model receives. |

### record `scope-spec`

A scope the scope strip shows.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | ASCII letters, digits, `.`, `-` and `_`, unique among every plugin's scopes. |
| `name` | `string` | The caption above the image. |
| `width` | `u32` | Width of the image the scope draws, 1 to 512. |
| `height` | `u32` | Height of the image the scope draws, 1 to 512. |

### record `scope-frame`

The frame a scope reads: `width * height` RGBA8 pixels, sRGB, straight alpha, top row first.

| Field | Type | Description |
| --- | --- | --- |
| `width` | `u32` | Frame width in pixels. |
| `height` | `u32` | Frame height in pixels. |
| `pixels` | `list<u8>` | `width * height * 4` bytes. |

### record `scope-drawing`

What a scope draws: RGBA8 pixels at the size its spec declares, or an empty list for a transparent image, and one readout line of at most 160 bytes.

| Field | Type | Description |
| --- | --- | --- |
| `pixels` | `list<u8>` | RGBA8 pixels at the size the spec declares, or empty for a transparent image. |
| `readout` | `string` | The readout line, at most 160 bytes. |

### record `decoder-spec`

Media decoder registration metadata.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Not empty, and unique among the plugin's decoders. |
| `name` | `string` | The format name, such as `YUV4MPEG2`. |
| `extensions` | `list<string>` | File extensions without the dot: ASCII letters and digits. |
| `priority` | `s32` | Among decoders claiming one extension, a higher priority is tried first. |

### record `media-probe`

What `decoder-probe` reports about a file.

| Field | Type | Description |
| --- | --- | --- |
| `width` | `u32` | Frame width in pixels. |
| `height` | `u32` | Frame height in pixels. |
| `frame-rate-num` | `s32` | Frame rate numerator: `frame-rate-num / frame-rate-den` frames per second. |
| `frame-rate-den` | `s32` | Frame rate denominator. |
| `frame-count` | `s64` | Length in whole frames at the frame rate, 0 when unknown. |
| `has-audio` | `bool` | The file has a sound track. |
| `is-image` | `bool` | The file is a still image: one frame, no time base. |
| `video-codec` | `option<string>` | The video codec name, or none when unknown. |
| `audio-codec` | `option<string>` | The audio codec name, or none when unknown. |

### enum `frame-format`

Pixel layout of decoded frames.

| Case | Description |
| --- | --- |
| `nv12` | A full-size Y plane, then interleaved U and V at half width and half height. |
| `yuv420p` | Y, U, and V planes; U and V at half width and half height. |

### enum `frame-color`

Color space of decoded frames.

| Case | Description |
| --- | --- |
| `bt709` | BT.709, limited range. |
| `bt601` | BT.601, limited range. |
| `bt2020-pq` | BT.2020 primaries, PQ transfer, limited range. |
| `srgb` | sRGB, full range. |

### record `video-info`

What `decoder-open-video` reports about the stream it opened.

| Field | Type | Description |
| --- | --- | --- |
| `width` | `u32` | Even, at most 16384. |
| `height` | `u32` | Even, at most 16384. |
| `format` | [`frame-format`](#enum-frame-format) | Pixel layout of every frame. |
| `color` | [`frame-color`](#enum-frame-color) | Color space of every frame. |
| `frame-rate-num` | `s32` | Frame rate numerator: `frame-rate-num / frame-rate-den` frames per second. |
| `frame-rate-den` | `s32` | Frame rate denominator. |
| `time-base-num` | `s32` | Every pts counts ticks at `time-base-num / time-base-den` ticks per second. |
| `time-base-den` | `s32` | Time base denominator. |
| `duration` | `option<s64>` | Length in time-base ticks, when known. |

### record `opened-video`

A video stream `decoder-open-video` opened.

| Field | Type | Description |
| --- | --- | --- |
| `instance` | [`instance`](#type-instance) | The stream handle `decoder-seek`, `decoder-next-frame`, and `decoder-close` receive. |
| `info` | [`video-info`](#record-video-info) | What the stream holds. |

### record `decoded-frame`

One decoded frame: its pts and its planes in plane order, each row packed at the plane's row width with no padding.

| Field | Type | Description |
| --- | --- | --- |
| `pts` | `s64` | Presentation time in time-base ticks. |
| `planes` | `list<list<u8>>` | The planes, rows packed with no padding: Y is `width` bytes by `height` rows, the NV12 UV plane is `width` bytes by `height / 2` rows, and each YUV 4:2:0 chroma plane is `width / 2` bytes by `height / 2` rows. |

### variant `decode-error`

Why a decoder open or probe failed.

| Case | Payload | Description |
| --- | --- | --- |
| `unsupported` | `string` | The decoder does not read this file; the host tries the next decoder. |
| `failed` | `string` | The decoder reads this file and failed. |

### enum `effect-param-kind`

Value kind of a visual effect or transition parameter.

| Case | Description |
| --- | --- |
| `scalar` | An `f32` in `min..=max`. |
| `int` | An integer in `min..=max`, each bound rounded. |
| `boolean` | A switch. |
| `choice` | One option of `choices`. |
| `color` | An RGBA color; the shader reads linear RGBA in `0..1`. |
| `vec2` | Two components in `min..=max`. A clip type's `handles` draws it on the Viewer. |
| `point3` | Three components in `min..=max`. |
| `size` | Width and height in `min..=max`. |
| `angle` | Degrees in `min..=max`. |
| `curve` | A response curve over `0..1`, read by a shader as 64 samples. |
| `gradient` | A color gradient over `0..1`, read by a shader as 64 linear RGBA samples. |
| `text` | UTF-8 text. Shaders do not read it. |
| `font` | A font family name. Shaders do not read it. |
| `file` | A file path. Shaders do not read it. |
| `layer` | A clip on the timeline. A pass that lists it as an input reads that clip's frame. |

### enum `effect-param-display`

Inspector control presentation for a scalar parameter.

| Case | Description |
| --- | --- |
| `slider` | A horizontal scrub slider. |
| `angle-dial` | A rotary dial in degrees. |
| `percentage` | The value formatted as a percentage. |

### record `effect-choice-option`

One option of an `effect-param-kind::choice` parameter.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Option id, stored on the effect instance. |
| `label` | `string` | The text the dropdown lists. |

### record `effect-curve-point`

One control point on an `effect-param-kind::curve` parameter (`x, y` in `0.0..=1.0`).

| Field | Type | Description |
| --- | --- | --- |
| `x` | `f32` | Input, in `0.0..=1.0`. |
| `y` | `f32` | Output at `x`. |

### record `effect-gradient-stop`

One color stop on an `effect-param-kind::gradient` parameter.

| Field | Type | Description |
| --- | --- | --- |
| `position` | `f32` | Position along the gradient, in `0.0..=1.0`. |
| `color` | `tuple<f32, f32, f32, f32>` | Linear RGBA `(r, g, b, a)`. |

### record `effect-visibility-condition`

Condition under which a parameter row is shown in the inspector.

| Field | Type | Description |
| --- | --- | --- |
| `param` | `string` | Name of the controlling parameter of the same effect or transition. |
| `equals-bool` | `option<bool>` | Shows the row while the controlling switch equals this. |
| `equals-int` | `option<s32>` | Shows the row while the controlling integer equals this. |
| `equals-choice` | `option<string>` | Shows the row while the controlling choice selects this option id. The host reads `equals-choice`, then `equals-bool`, then `equals-int`, and uses the first that is set. |

### record `effect-group-spec`

Named collapsible parameter group in an effect or transition inspector.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Group id that parameters name in `group`. |
| `label` | `string` | The section heading in Effect Controls. |
| `collapsed` | `bool` | The section starts collapsed. |

### record `effect-param-spec`

Parameter definition for a visual effect, generator, or transition.

| Field | Type | Description |
| --- | --- | --- |
| `name` | `string` | Parameter key: unique within the effect and a WGSL identifier. A shader reads it through `param_<name>()`. |
| `label` | `string` | The row label. |
| `kind` | [`effect-param-kind`](#enum-effect-param-kind) | The value kind. |
| `default-value` | `f32` | Default for `scalar` and `angle`. |
| `min` | `f32` | Lowest value of `scalar`, `vec2`, `point3`, `size`, and `angle`. Rounded, the lowest `int`. |
| `max` | `f32` | Highest value, as `min` is the lowest. Not below `min`. |
| `default-color` | `tuple<u8, u8, u8, u8>` | Default for `color`: RGBA `(r, g, b, a)` in `0..=255`. |
| `default-vec2` | `tuple<f32, f32>` | Default for `vec2` and `size`. |
| `display` | [`effect-param-display`](#enum-effect-param-display) | The control a `scalar` row draws. |
| `soft-min` | `option<f32>` | Lower end of the scrub range before values clamp to `min`, or none. |
| `soft-max` | `option<f32>` | Upper end of the scrub range before values clamp to `max`, or none. |
| `unit` | `option<string>` | Unit suffix, such as `px`, `deg`, `%`, or `s`, or none. |
| `tooltip` | `option<string>` | Tooltip text for the row, or none. |
| `group` | `option<string>` | Id of the `effect-group-spec` the row sits in, or none. |
| `visible-when` | `option<effect-visibility-condition>` | Shows the row only while the condition holds, or always when none. |
| `choices` | `list<effect-choice-option>` | The options of a `choice`. |
| `default-int` | `s32` | Default for `int`. |
| `default-bool` | `bool` | Default for `boolean`. |
| `default-choice` | `string` | Default option id for `choice`. Empty selects the first option. |
| `default-vec3` | `tuple<f32, f32, f32>` | Default for `point3`. |
| `default-curve` | `list<effect-curve-point>` | Default control points for `curve`. Empty is the identity `(0, 0)` to `(1, 1)`. |
| `default-gradient` | `list<effect-gradient-stop>` | Default stops for `gradient`. Empty is black to white. |
| `default-text` | `string` | Default for `text`, `font`, and `file`. |

### record `effect-spec`

Visual effect or generator registration metadata.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Namespaced id with at least one `.`, not already in the catalog. |
| `label` | `string` | The name the Effect menu and Effect Controls show. Not empty. |
| `category` | `string` | The Effect menu submenu. Empty lists the effect under Other. Generators are not listed in the Effect menu. |
| `params` | `list<effect-param-spec>` | Parameters in row order. Flattened values follow this order. |
| `groups` | `list<effect-group-spec>` | Collapsible parameter sections, in display order. |
| `generator` | `bool` | The entry draws a frame without reading an input. `register-effect` sets it false and `register-generator` true; `register-cpu-effect` and `register-effect-graph` read it. |
| `stateful` | `bool` | The output at a time depends on frames before it, as feedback and trails do. |

### record `transition-spec`

Transition registration metadata.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Namespaced id with at least one `.`, not already in the catalog. |
| `label` | `string` | The name the transition list shows. Not empty. |
| `params` | `list<effect-param-spec>` | Parameters in row order. A shader reads each through `param_<name>()`. |

### variant `pass-stage`

How an effect graph pass runs.

| Case | Payload | Description |
| --- | --- | --- |
| `fragment` |  | `@fragment fn fs`, drawn over the whole output. |
| `compute` | `tuple<u32, u32>` | `@compute @workgroup_size(x, y, 1) fn cs` dispatched over the output in tiles of this width and height, writing `output_tex`. |

### variant `pass-input`

One texture an effect graph pass reads.

| Case | Payload | Description |
| --- | --- | --- |
| `source` |  | The effect's input frame, bound as `source_tex`. |
| `pass` | `string` | The output of the named pass, bound as `pass_<name>`. |
| `layer` | `string` | The frame of the clip the named layer parameter references, bound as `layer_<param>`. |
| `texture` | `string` | The plugin texture registered under this id, bound as `texture_<id>` with every character other than an ASCII letter, digit, or `_` replaced by `_`. |

### variant `output-size`

The size an effect graph pass writes.

| Case | Payload | Description |
| --- | --- | --- |
| `same` |  | The effect's input size. |
| `scale` | `tuple<f32, f32>` | The input size times this width and height factor, rounded, at least one pixel. |
| `fixed` | `tuple<u32, u32>` | This width and height in pixels. |

### record `graph-pass`

One pass of an effect graph.

| Field | Type | Description |
| --- | --- | --- |
| `name` | `string` | The name other passes read this one by. Empty names it `pass<index>`. |
| `stage` | [`pass-stage`](#variant-pass-stage) | How the pass runs. |
| `inputs` | `list<pass-input>` | What the pass reads, in binding order; the first is also bound as `input_tex`. Empty reads the pass before it, or the source frame for the first pass. |
| `output` | [`output-size`](#variant-output-size) | The last pass writes the effect's frame and uses `same`. |
| `wgsl` | `string` | The pass's WGSL source. |

### record `effect-traits`

What an effect graph states about its frame beyond its spec.

| Field | Type | Description |
| --- | --- | --- |
| `bounds` | `tuple<f32, f32, f32, f32>` | Pixels drawn beyond the layer's left, top, right, and bottom edges. |
| `reads-outside` | `bool` | The effect samples its source outside `0.0..=1.0`. |
| `time-dependent` | `bool` | The frame changes with time when the parameters do not. A pass of an effect that is not reads `aria.time` as zero. |

### type `effect-param`

Flattened `f32` parameter value passed to `cpu-effect-process`.

Alias of `f32`.

### record `video-frame`

One premultiplied RGBA16F video frame passed to and returned from `cpu-effect-process`: `width * height * 4` IEEE-754 binary16 bit patterns (`[R, G, B, A]` per pixel), top row first, with no row padding.

| Field | Type | Description |
| --- | --- | --- |
| `width` | `u32` | Width in pixels. |
| `height` | `u32` | Height in pixels. |
| `time` | `f32` | Seconds since the clip's first frame, or 0 when the effect or clip type is not time dependent. An effect from `register-cpu-effect` is not. |
| `pixels` | `list<u16>` | `width * height * 4` binary16 bit patterns. |

### enum `marker-color`

A label color: a clip type's timeline tint and a marker's color.

| Case | Description |
| --- | --- |
| `teal` | The Teal label. |
| `blue` | The Blue label. |
| `purple` | The Purple label. |
| `pink` | The Magenta label. |
| `red` | The Rose label. |
| `orange` | The Mango label. |
| `yellow` | The Yellow label. |
| `green` | The Green label. |

### record `clip-type-spec`

Generator-backed clip type registration metadata.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Namespaced id with at least one `.`: at most 128 bytes, no space or control character, not already in the catalog. |
| `label` | `string` | The name in the Generators tab, the palette, Effect Controls, and on the clip: 1 to 64 bytes. |
| `generator` | `string` | Id of the generator that draws the type. Not empty. |
| `colour` | [`marker-color`](#enum-marker-color) | The timeline label color. |
| `icon-svg` | `option<string>` | SVG source of the icon drawn before the clip's name, or none. |
| `handles` | `list<string>` | Names of the generator's `vec2` parameters drawn as Viewer handles: at most 8, each once. |

### record `clip-type-schema`

The fields each clip of a type that draws itself stores: effect parameters of any kind, saved with the clip, keyframed where the kind is, and drawn as the clip's rows in the inspector and on the timeline.

| Field | Type | Description |
| --- | --- | --- |
| `fields` | `list<effect-param-spec>` | The fields, in row order. A handle names a `vec2` field. |
| `groups` | `list<effect-group-spec>` | Collapsible groups that fields name in `group`. |
| `time-dependent` | `bool` | The picture changes with time when the fields do not. A type that does not is drawn at time 0. |

### variant `clip-type-render`

What draws a clip type that declares its fields.

| Case | Payload | Description |
| --- | --- | --- |
| `cpu` |  | `cpu-effect-process`, called with the type's id on a transparent frame of the clip's size and the field values. |
| `passes` | `list<graph-pass>` | WGSL passes reading the fields as `aria.params`. |

### record `driver-language-spec`

Property driver language registration metadata.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | ASCII letters, digits, `.`, `-` and `_`. A second registration in one host is refused. |
| `name` | `string` | The language's display name. |

### variant `param-value`

Property value passed to and returned from `driver-evaluate`.

| Case | Payload | Description |
| --- | --- | --- |
| `scalar` | `f32` | One value. |
| `vec2` | `tuple<f32, f32>` | `(x, y)`. |
| `vec3` | `tuple<f32, f32, f32>` | `(x, y, z)`. |
| `color` | `tuple<u8, u8, u8, u8>` | RGBA8 `(r, g, b, a)`. |
| `rect` | `tuple<f32, f32, f32, f32>` | `(x, y, width, height)`. |

### record `driver-vars`

Scope variables passed to `driver-evaluate`.

| Field | Type | Description |
| --- | --- | --- |
| `time` | `f64` | Sequence seconds being evaluated. |
| `clip-time` | `f64` | Seconds since the clip's first frame. |
| `frame-rate` | `f64` | Sequence frames per second. |
| `duration` | `f64` | Sequence length in seconds. |
| `canvas-width` | `u32` | Canvas width in pixels. |
| `canvas-height` | `u32` | Canvas height in pixels. |
| `value` | [`param-value`](#variant-param-value) | The property's keyframed value at `time`. |
| `seed` | `u64` | A number fixed per clip and property. |

### enum `workspace-focus`

Keyboard focus target for a registered workspace.

| Case | Description |
| --- | --- |
| `media` | The Project panel. |
| `preview` | The Viewer. |
| `timeline` | The Timeline. |
| `inspector` | Effect Controls. |
| `assistant` | The assistant. |
| `mixer` | The mixer. |

### record `workspace-panes`

Pane arrangement restored when a workspace is applied.

| Field | Type | Description |
| --- | --- | --- |
| `sidebar-width` | `option<f32>` | Sidebar width in pixels, above 0 and at most 16384. None keeps the Editing width. |
| `sidebar-collapsed` | `option<bool>` | Collapses the sidebar. None shows it. |
| `inspector-width` | `option<f32>` | Inspector width in pixels, above 0 and at most 16384. None keeps the Editing width. |
| `inspector-collapsed` | `option<bool>` | Collapses the inspector. None shows it. |
| `timeline-height` | `option<f32>` | Timeline height in pixels, above 0 and at most 16384. None keeps the Editing height. |
| `scopes` | `option<bool>` | Opens the scope strip. None leaves it closed. |
| `focus` | `option<workspace-focus>` | The pane that takes the keyboard. None selects the Timeline. |

### record `workspace-spec`

Workspace registration metadata.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Unique within the plugin: 1 to 64 ASCII letters, digits, `-`, `_` and `.`, not starting with `.`. |
| `name` | `string` | The Window menu row and the Settings row: one line of at most 64 bytes. |
| `panes` | [`workspace-panes`](#record-workspace-panes) | The arrangement applied. |

### enum `analyzer-scope`

What target an audio analyzer operates on.

| Case | Description |
| --- | --- |
| `clip` | The selected clip's sound, offered in Clip > Analyze. |
| `sequence` | The whole sequence mix, offered in Sequence > Analyze. |
| `in-out` | The sequence mix from In to Out, or the whole sequence when no range is marked, offered in Sequence > Analyze. |

### record `analyzer-spec`

Audio analyzer registration metadata.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Namespaced id with at least one `.`: ASCII letters, digits, `.`, `_` and `-`. |
| `label` | `string` | The menu row, palette row, job title, and undo step name. Not blank. |
| `category` | `option<string>` | An optional grouping label. |
| `scope` | [`analyzer-scope`](#enum-analyzer-scope) | What the analyzer reads and which menu lists it. |

### record `rational-time`

Exact timestamp `value / (rate-num / rate-den)` seconds.

| Field | Type | Description |
| --- | --- | --- |
| `value` | `s64` | Tick count. |
| `rate-num` | `s32` | Tick rate numerator. |
| `rate-den` | `s32` | Tick rate denominator. |

### record `time-range`

Half-open time interval `[start, start + duration)`.

| Field | Type | Description |
| --- | --- | --- |
| `start` | [`rational-time`](#record-rational-time) | The first tick. |
| `duration` | [`rational-time`](#record-rational-time) | The length, at the rate of `start`. |

### record `clip-ref`

Summary of a timeline clip returned by project queries.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `u64` | The clip id. |
| `track` | `u64` | Id of the track holding the clip. |
| `start` | [`rational-time`](#record-rational-time) | The clip's first tick on the timeline. |
| `end` | [`rational-time`](#record-rational-time) | The tick after the clip's last frame. |
| `kind` | `string` | `media`, `sequence`, `multicam`, the generator id of a generated clip, or the clip type id of a typed clip. |
| `name` | `string` | The clip's title. A typed clip with no name of its own reads the type's name. |

### record `in-out-marks`

Sequence In and Out points from the editor view.

| Field | Type | Description |
| --- | --- | --- |
| `mark-in` | `option<rational-time>` | The In point, or none when unset. |
| `mark-out` | `option<rational-time>` | The Out point, or none when unset. |

### enum `marker-kind`

Category of a ruler marker.

| Case | Description |
| --- | --- |
| `comment` | A note. |
| `chapter` | A chapter point. |
| `segmentation` | A segment, such as a range an analyzer writes. |

### record `marker`

Ruler marker on a sequence timeline.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `u64` | The marker id. |
| `tick` | [`rational-time`](#record-rational-time) | Position on the sequence timeline. |
| `duration` | [`rational-time`](#record-rational-time) | Span of a range marker, or 0 ticks for a point marker. |
| `name` | `string` | The marker name. |
| `color` | [`marker-color`](#enum-marker-color) | The marker color. |
| `comment` | `string` | Comment text, empty when none. |
| `kind` | [`marker-kind`](#enum-marker-kind) | The marker category. |
