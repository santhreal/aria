use std::ffi::c_void;

#[cfg(doc)]
use super::{
    ARIA_API_MAJOR, ARIA_BYTES_FOUND, ARIA_BYTES_NOT_FOUND, ARIA_INSTANCE_UNSUPPORTED,
    ARIA_PLUGIN_ENTRY_SYMBOL, ARIA_PLUGIN_V1_MIN_SIZE, ARIA_STATUS_ERR, ARIA_STATUS_OK,
    ARIA_STATUS_PANIC, ARIA_STATUS_UNSUPPORTED,
};
use super::{
    AriaAudioBlock, AriaAudioParam, AriaBytes, AriaBytesResult, AriaCpuEffectCall,
    AriaDriverEvalCall, AriaEncoderAudio, AriaEncoderConfig, AriaEncoderFrame, AriaInstanceResult,
    AriaJobStep, AriaOwnedBytes, AriaOwnedStr, AriaResult, AriaScopeCall, AriaStr,
    AriaVideoFrameOut, AriaVideoInfo,
};

/// The calls a host makes available to a plugin. The host passes it to
/// `aria_plugin_entry`, and it stays valid while the library is loaded.
///
/// A `register_*` call and `subscribe` succeed only while `mount` runs. Every other
/// call works in any callback. A call returns [`ARIA_STATUS_ERR`] with the reason when it
/// fails, including when `host_ctx` is null or an [`AriaStr`] argument is not UTF-8.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaHostV1 {
    /// Byte size of this table.
    pub struct_size: usize,
    /// [`ARIA_API_MAJOR`].
    pub api_major: u32,
    /// 0.
    pub reserved: u32,
    /// Release an [`AriaOwnedStr`] a host call returned.
    pub free_string: unsafe extern "C" fn(str: AriaOwnedStr),
    /// Release an [`AriaOwnedBytes`] a host call returned.
    pub free_bytes: unsafe extern "C" fn(bytes: AriaOwnedBytes),
    /// Write `text` to the editor log under the plugin's id. `level` is an
    /// `ARIA_LOG_*` value. A null `host_ctx` or text that is not UTF-8 writes nothing.
    pub log: unsafe extern "C" fn(host_ctx: *mut c_void, level: u32, text: AriaStr),
    /// Read the value stored under `key` in the plugin's storage directory,
    /// `<data dir>/plugin-data/<plugin id>`. Returns [`ARIA_BYTES_FOUND`] with the
    /// bytes, or [`ARIA_BYTES_NOT_FOUND`]. A key is one file name: not empty, not `.` or
    /// `..`, and without `/`, `\` or NUL.
    pub storage_get: unsafe extern "C" fn(host_ctx: *mut c_void, key: AriaStr) -> AriaBytesResult,
    /// Store `value` under `key`, replacing any earlier value. Keys follow the rules of
    /// `storage_get`.
    pub storage_set:
        unsafe extern "C" fn(host_ctx: *mut c_void, key: AriaStr, value: AriaBytes) -> AriaResult,
    /// Delete the value stored under `key`. Deleting a key with no value succeeds.
    pub storage_delete: unsafe extern "C" fn(host_ctx: *mut c_void, key: AriaStr) -> AriaResult,
    /// The stored keys as a sorted JSON array of strings.
    pub storage_keys: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    /// The current value of the setting `key` as JSON, or an empty value when the
    /// setting has none.
    pub setting_get: unsafe extern "C" fn(host_ctx: *mut c_void, key: AriaStr) -> AriaResult,
    /// Register the command `id` with the display name `title`. Running it calls
    /// [`AriaPluginV1::command_run`] with `id`.
    pub register_command:
        unsafe extern "C" fn(host_ctx: *mut c_void, id: AriaStr, title: AriaStr) -> AriaResult,
    /// Deliver events of `event_kind` to [`AriaPluginV1::event`]. `event_kind` is a
    /// kind from `docs/plugins/events.md`. `resolve-dropped:<n>` subscribes with the
    /// priority `n`, a 32-bit integer; a higher priority runs first, and the default
    /// is 0.
    pub subscribe: unsafe extern "C" fn(host_ctx: *mut c_void, event_kind: AriaStr) -> AriaResult,
    /// Register an importer. `spec_json` is an object with `id`, `name`, and
    /// `extensions`, an array of file extensions without the dot. Importing a file
    /// calls [`AriaPluginV1::import_file`].
    pub register_importer:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// Register an exporter. `spec_json` is an object with `id`, `name`, and
    /// `extension`, without the dot. Exporting calls [`AriaPluginV1::export_file`].
    pub register_exporter:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// Register an audio effect. `spec_json` is an object with `id`, `label`, and the
    /// optional `category`, `latency` in sample frames at 48 kHz, `sidechain`, and
    /// `params`: an array of objects with `name`, `label`, `default`, `min`, and `max`,
    /// in the order [`AriaPluginV1::audio_process`] receives them. Each use of the
    /// effect calls [`AriaPluginV1::audio_create`].
    pub register_audio_effect:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// The project as JSON. Requires `project = "read"` or `"read-write"` in the
    /// manifest's `[permissions]`.
    pub project_snapshot: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    /// The editor state as a JSON object with `selection` (clip ids), `playhead`,
    /// `mark_in`, `mark_out` (times or null), `playing`, and `speed`. Requires project
    /// read permission.
    pub editor_view: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    /// Apply `commands_json`, a JSON array of commands, as one undo step named
    /// `label`, or apply none of them. The value is an object with the new `revision`
    /// and the `outcomes` of the commands. Requires `project = "read-write"`.
    pub project_transact: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        label: AriaStr,
        commands_json: AriaStr,
    ) -> AriaResult,
    /// Undo the last undo step. The value is `true`, or `false` when there was nothing
    /// to undo. Requires `project = "read-write"`.
    pub project_undo: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    /// Register a panel. `spec_json` is an object with `id`, `title`, and `location`:
    /// `sidebar`, `inspector`, or `timeline`. Any other location is refused. Drawing it
    /// calls [`AriaPluginV1::panel_view`]. Fails unless the plugin table has the panel
    /// slots.
    pub register_panel:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// Redraw the panel `id`, which calls [`AriaPluginV1::panel_view`] again.
    pub refresh_panel: unsafe extern "C" fn(host_ctx: *mut c_void, id: AriaStr) -> AriaResult,
    /// Register an export encoder. `spec_json` is an object with `id`, `name`,
    /// `extension`, and the optional `audio`, true when the encoder takes audio.
    /// Exporting with it calls [`AriaPluginV1::encoder_open`]. Fails unless the plugin
    /// table has the encoder slots.
    pub register_encoder:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// Register a viewer overlay. `spec_json` is an object with `id` and `title`.
    /// Drawing it calls [`AriaPluginV1::overlay_scene`]. Fails unless the plugin table
    /// has the overlay slots.
    pub register_viewer_overlay:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// Redraw the viewer overlay `id`, which calls [`AriaPluginV1::overlay_scene`]
    /// again.
    pub refresh_viewer_overlay:
        unsafe extern "C" fn(host_ctx: *mut c_void, id: AriaStr) -> AriaResult,
    /// Start a background job named `label` on the Jobs board. The host calls
    /// [`AriaPluginV1::job_step`] on the job thread until a step reports `done`, a step
    /// fails, or Cancel is accepted, then [`AriaPluginV1::job_end`]. The instance is the
    /// job id both receive. The first step can run before `start_job` returns. Fails
    /// unless the plugin table has the job slots.
    pub start_job:
        unsafe extern "C" fn(host_ctx: *mut c_void, label: AriaStr) -> AriaInstanceResult,
    /// Register a media decoder. `spec_json` is an object with `id`, `name`,
    /// `extensions`, and the optional `priority`; a higher priority is tried first.
    /// Fails unless the plugin table has the decoder slots.
    pub register_decoder:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// Register an assistant tool. `spec_json` is an object with `name`,
    /// `description`, `parameters` (a JSON Schema), and `tier`: `read-only`,
    /// `workspace`, or `system`. A call to the tool calls [`AriaPluginV1::ai_tool_call`].
    /// Fails unless the plugin table has the AI tool slot.
    pub register_ai_tool:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// Register a scope. `spec_json` is an object with `id`, `name`, and the `width`
    /// and `height` of its image in pixels. Drawing it calls
    /// [`AriaPluginV1::scope_measure`]. Fails unless the plugin table has the scope slot.
    pub register_scope:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// Register a GPU effect. `spec_json` is an effect spec object, as in
    /// `docs/plugins/effects.md`. `wgsl` is WGSL source, a JSON array of WGSL strings
    /// for a chain of fragment passes, or a JSON array of graph pass objects.
    pub register_effect: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        spec_json: AriaStr,
        wgsl: AriaStr,
    ) -> AriaResult,
    /// Register an effect that [`AriaPluginV1::cpu_effect_process`] draws. `spec_json`
    /// is an effect spec object. Fails unless the plugin table has the contribution
    /// slots.
    pub register_cpu_effect:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// Register a transition. `spec_json` is a transition spec object and `wgsl` its
    /// WGSL source.
    pub register_transition: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        spec_json: AriaStr,
        wgsl: AriaStr,
    ) -> AriaResult,
    /// Register a generator. `spec_json` is an effect spec object, and `wgsl` takes the
    /// forms of `register_effect`. An empty `wgsl` registers a generator that
    /// [`AriaPluginV1::cpu_effect_process`] draws, which requires the contribution slots.
    pub register_generator: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        spec_json: AriaStr,
        wgsl: AriaStr,
    ) -> AriaResult,
    /// Register a clip type. `spec_json` is an object with `id`, `label`, `colour`,
    /// and the optional `generator`, `icon_svg`, `handles`, and `schema`, as in
    /// `docs/plugins/clip-types.md`.
    pub register_clip_type:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// Register a driver language, whose expressions [`AriaPluginV1::driver_evaluate`]
    /// evaluates. `spec_json` is a driver language spec object, as in
    /// `docs/plugins/drivers.md`. Fails unless the plugin table has the contribution
    /// slots.
    pub register_driver_language:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// Register a preset. `preset_json` is a preset object, the JSON form of an
    /// `.ariapreset` file.
    pub register_preset:
        unsafe extern "C" fn(host_ctx: *mut c_void, preset_json: AriaStr) -> AriaResult,
    /// Register the theme `id` from `toml`, the text of a theme file.
    pub register_theme:
        unsafe extern "C" fn(host_ctx: *mut c_void, id: AriaStr, toml: AriaStr) -> AriaResult,
    /// Register a workspace. `spec_json` is a workspace spec object, as in
    /// `docs/plugins/workspaces.md`.
    pub register_workspace:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// Register an audio analyzer, which runs through [`AriaPluginV1::analyzer_open`].
    /// `spec_json` is an analyzer spec object, as in `docs/plugins/analyzers.md`. Fails
    /// unless the plugin table has the contribution slots.
    pub register_analyzer:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    /// The clip `id` as a JSON object with `id`, `track`, `start`, `end`, `kind`, and
    /// `name`, from the open sequence, the root timeline, or a nested sequence. An
    /// empty value when no clip has the id. Requires project read permission.
    pub clip: unsafe extern "C" fn(host_ctx: *mut c_void, id: u64) -> AriaResult,
    /// The clips of `sequence_id` that overlap the half-open range `range_json`, a JSON
    /// object with `start` and `duration` times, as a JSON array in track order, then
    /// time order. A `sequence_id` of 0 reads the open sequence, and an unknown id
    /// returns an empty array. Requires project read permission.
    pub clips_in_range: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        sequence_id: u64,
        range_json: AriaStr,
    ) -> AriaResult,
    /// The clips of `sequence_id` with `start <= tick < end`, where `tick_json` is a
    /// time object with `value` and `rate`, as a JSON array in track order, then time
    /// order. `sequence_id` reads as in `clips_in_range`. Requires project read
    /// permission.
    pub clips_at: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        sequence_id: u64,
        tick_json: AriaStr,
    ) -> AriaResult,
    /// The selected clips as a JSON array, in selection order. Requires project read
    /// permission.
    pub selected_clips: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    /// The markers of `sequence_id` whose point or span overlaps the half-open range
    /// `range_json`, as a JSON array ordered by time. `sequence_id` and `range_json`
    /// read as in `clips_in_range`. Requires project read permission.
    pub markers_in_range: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        sequence_id: u64,
        range_json: AriaStr,
    ) -> AriaResult,
    /// The In and Out points as a JSON object with `mark_in` and `mark_out`, each a
    /// time or null. Requires project read permission.
    pub in_out: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    /// Register `width * height * 4` straight-alpha RGBA8 `pixels` as the
    /// plugin texture `id`, which an effect pass reads as an input.
    pub register_texture: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        id: AriaStr,
        width: u32,
        height: u32,
        pixels: AriaBytes,
    ) -> AriaResult,
}

/// The callbacks a plugin exports. `aria_plugin_entry` returns it, and it stays valid
/// while the library is loaded.
///
/// The host calls `mount` once, before any other callback. A callback that has no
/// result returns [`ARIA_STATUS_OK`] with an empty `value`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaPluginV1 {
    /// Byte size of this table, at least [`ARIA_PLUGIN_V1_MIN_SIZE`].
    pub struct_size: usize,
    /// [`ARIA_API_MAJOR`].
    pub api_major: u32,
    /// 0.
    pub reserved: u32,
    /// Release an [`AriaOwnedStr`] a callback returned.
    pub free_string: unsafe extern "C" fn(str: AriaOwnedStr),
    /// Release an [`AriaOwnedBytes`] a callback returned.
    pub free_bytes: unsafe extern "C" fn(bytes: AriaOwnedBytes),
    /// Register the plugin's contributions with the `register_*` host calls and
    /// `subscribe`. [`ARIA_STATUS_ERR`] fails the load with the message.
    pub mount: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    /// Release the plugin's state before the library unloads. Called once, after a
    /// `mount` that succeeded, unless the plugin has failed.
    pub unmount: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    /// Run the command `id`, which the plugin registered with
    /// [`AriaHostV1::register_command`] or declared in `plugin.toml`. `args_json` is a
    /// JSON object, `{}` for a command run from the editor. [`ARIA_STATUS_ERR`] reports
    /// the message as the command's failure.
    pub command_run:
        unsafe extern "C" fn(host_ctx: *mut c_void, id: AriaStr, args_json: AriaStr) -> AriaResult,
    /// Handle an event of the subscribed `kind`. `event_json` is its payload and the
    /// value is the reply, or empty for none, as `docs/plugins/events.md` lists them.
    /// [`ARIA_STATUS_ERR`] is logged and counts as no reply.
    pub event: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        kind: AriaStr,
        event_json: AriaStr,
    ) -> AriaResult,
    /// Import the file at `path` with the importer `importer_id`. The value is a JSON
    /// object with the optional `commands_json`, a string holding a JSON array of
    /// commands the host applies as one undo step, and `skipped`, an array of strings
    /// naming what the import left out. [`ARIA_STATUS_ERR`] cancels the import.
    pub import_file: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        importer_id: AriaStr,
        path: AriaStr,
    ) -> AriaResult,
    /// Write `project_json`, the project as JSON, with the exporter `exporter_id` to
    /// the file `path`, a staging file in the plugin's storage directory that the host
    /// copies to the destination. The value is empty or a JSON object with `skipped`,
    /// an array of strings naming what the file leaves out.
    pub export_file: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        exporter_id: AriaStr,
        project_json: AriaStr,
        path: AriaStr,
    ) -> AriaResult,
    /// Create an instance of the audio effect `effect_id` at `sample_rate` frames per
    /// second, with fresh state. The host creates one per insert of the effect.
    pub audio_create: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        effect_id: AriaStr,
        sample_rate: u32,
    ) -> AriaInstanceResult,
    /// Process `block` in place. `params` holds `params_len` values, one per parameter
    /// of the effect's spec, in spec order.
    pub audio_process: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        instance: u64,
        params: *const AriaAudioParam,
        params_len: usize,
        block: *mut AriaAudioBlock,
    ) -> AriaResult,
    /// Clear every value carried over from earlier blocks, such as delay lines and
    /// envelopes. Called before a block that does not continue the last one, such as
    /// after a seek.
    pub audio_reset: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
    /// Release an instance `audio_create` returned.
    pub audio_drop: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
    /// The contents of the panel `panel_id`: [`ARIA_BYTES_FOUND`] with the UI tree as
    /// JSON, as in `docs/plugins/panels.md`, at most 257 MiB. Any other result shows the
    /// error in the panel and records a fault.
    pub panel_view:
        unsafe extern "C" fn(host_ctx: *mut c_void, panel_id: AriaStr) -> AriaBytesResult,
    /// Handle `event_json`, a control event in the panel `panel_id`.
    pub panel_event: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        panel_id: AriaStr,
        event_json: AriaStr,
    ) -> AriaResult,
    /// Start writing the file `path` with the encoder `encoder_id` and the settings
    /// `config`. The instance receives the export's frames and audio.
    pub encoder_open: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        encoder_id: AriaStr,
        path: AriaStr,
        config: *const AriaEncoderConfig,
    ) -> AriaInstanceResult,
    /// Encode the next frame of the export.
    pub encoder_push_frame: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        instance: u64,
        frame: *const AriaEncoderFrame,
    ) -> AriaResult,
    /// Encode the next block of the export's audio.
    pub encoder_push_audio: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        instance: u64,
        audio: *const AriaEncoderAudio,
    ) -> AriaResult,
    /// Complete the file after the last frame. The host makes no further call on the
    /// instance.
    pub encoder_finish: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
    /// Abandon an export that stopped before `encoder_finish`, and release the instance.
    pub encoder_drop: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
    /// The scene of the viewer overlay `overlay_id`: [`ARIA_BYTES_FOUND`] with the
    /// scene as JSON, as in `docs/plugins/viewer-overlays.md`, at most 4 MiB. Any other
    /// result rejects the scene with the error.
    pub overlay_scene:
        unsafe extern "C" fn(host_ctx: *mut c_void, overlay_id: AriaStr) -> AriaBytesResult,
    /// Handle `event_json`, a pointer event on a handle of the overlay `overlay_id`.
    pub overlay_event: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        overlay_id: AriaStr,
        event_json: AriaStr,
    ) -> AriaResult,
    /// Run one bounded step of the job `job` and write its progress to `step`. The
    /// value is the detail line. [`ARIA_STATUS_ERR`] ends the job as failed.
    pub job_step:
        unsafe extern "C" fn(host_ctx: *mut c_void, job: u64, step: *mut AriaJobStep) -> AriaResult,
    /// The job `job` has ended. `cancelled` is 1 when Cancel ended it, otherwise 0.
    pub job_end:
        unsafe extern "C" fn(host_ctx: *mut c_void, job: u64, cancelled: u32) -> AriaResult,
    /// Describe the file at `path` for the decoder `decoder_id`. The value is a JSON
    /// object with `frame_rate_num` and `frame_rate_den`, and the optional `width`,
    /// `height`, `frame_count`, `has_audio`, `is_image`, `video_codec`, and
    /// `audio_codec`. [`ARIA_STATUS_UNSUPPORTED`] passes the file to the next decoder.
    pub decoder_probe: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        decoder_id: AriaStr,
        path: AriaStr,
    ) -> AriaResult,
    /// Open the video stream of `path` and write its description to `info`.
    /// [`ARIA_INSTANCE_UNSUPPORTED`] passes the file to the next decoder.
    pub decoder_open_video: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        decoder_id: AriaStr,
        path: AriaStr,
        info: *mut AriaVideoInfo,
    ) -> AriaInstanceResult,
    /// Position the video instance at the sync frame at or before `pts`, in time base
    /// ticks.
    pub decoder_seek:
        unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64, pts: i64) -> AriaResult,
    /// Decode the next frame of the video instance into `frame`.
    pub decoder_next_frame: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        instance: u64,
        frame: *mut AriaVideoFrameOut,
    ) -> AriaResult,
    /// Open the audio of `path`, resampled to `sample_rate` frames per second and
    /// `channels` channels. [`ARIA_INSTANCE_UNSUPPORTED`] passes the file to the next
    /// decoder.
    pub decoder_open_audio: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        decoder_id: AriaStr,
        path: AriaStr,
        sample_rate: u32,
        channels: u32,
    ) -> AriaInstanceResult,
    /// Write up to `out_len` interleaved samples to `out`, and their count to
    /// `*written`: whole frames, and fewer than `out_len` only at end of stream.
    pub decoder_read_audio: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        instance: u64,
        out: *mut f32,
        out_len: usize,
        written: *mut usize,
    ) -> AriaResult,
    /// Position the next read of the audio instance at output frame `frame`.
    pub decoder_seek_audio:
        unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64, frame: i64) -> AriaResult,
    /// Release an instance `decoder_open_video` or `decoder_open_audio` returned.
    pub decoder_close: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
    /// Run the assistant tool `name`, as its spec registered it, with `arguments_json`,
    /// an object that matches its `parameters` schema. The value is a JSON object with
    /// the optional `text` and `images`, an array of objects with `label` and `png`,
    /// the PNG file as an array of bytes.
    pub ai_tool_call: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        name: AriaStr,
        arguments_json: AriaStr,
    ) -> AriaResult,
    /// Measure the frame in `call` for the scope `scope_id` and draw the scope image.
    /// The value is the readout text shown with the scope.
    pub scope_measure: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        scope_id: AriaStr,
        call: *mut AriaScopeCall,
    ) -> AriaResult,
    /// Draw one frame of the CPU effect, generator, or clip type `effect_id` into
    /// `call.output`.
    pub cpu_effect_process: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        effect_id: AriaStr,
        call: *mut AriaCpuEffectCall,
    ) -> AriaResult,
    /// Evaluate `expr`, a non-empty expression in the driver language `language_id`,
    /// and write the result to `call.output`. [`ARIA_STATUS_ERR`] or
    /// [`ARIA_STATUS_PANIC`] fails this evaluation with the message.
    pub driver_evaluate: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        language_id: AriaStr,
        expr: AriaStr,
        call: *mut AriaDriverEvalCall,
    ) -> AriaResult,
    /// Start a run of the analyzer `analyzer_id` over audio at `sample_rate` frames per
    /// second with `channels` channels.
    pub analyzer_open: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        analyzer_id: AriaStr,
        sample_rate: u32,
        channels: u32,
    ) -> AriaInstanceResult,
    /// Read the next `samples_len` interleaved samples of the run.
    /// [`ARIA_STATUS_ERR`] stops the run with the message.
    pub analyzer_push: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        instance: u64,
        samples: *const f32,
        samples_len: usize,
    ) -> AriaResult,
    /// End the run after the last block. The value is the analyzer result as JSON, as
    /// in `docs/plugins/analyzers.md`. The host makes no further call on the instance.
    pub analyzer_finish: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
    /// Release a run that ended before `analyzer_finish`.
    pub analyzer_drop: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
}

/// The entry point a native plugin exports under [`ARIA_PLUGIN_ENTRY_SYMBOL`]. `host`
/// is the host's table and `api_major` is [`ARIA_API_MAJOR`]. Returns the plugin's
/// table, or null to decline the load.
pub type AriaPluginEntryFn =
    unsafe extern "C" fn(host: *const AriaHostV1, api_major: u32) -> *const AriaPluginV1;

/// The fields both tables start with, which a receiver reads before any slot.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaTableHeader {
    /// Byte size of the whole table.
    pub struct_size: usize,
    /// [`ARIA_API_MAJOR`] of the table's sender.
    pub api_major: u32,
    /// 0.
    pub reserved: u32,
}
