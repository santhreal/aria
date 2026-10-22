// Generated from crates/aria-plugin-abi/src/lib.rs by
// crates/aria-plugin-abi/tests/abi_reference/render.rs. Edit the doc comments there, then run
// `ARIA_BLESS_DOCS=1 cargo test -p aria-plugin-abi --test abi_reference`.
//
// Stable C ABI types and versioned function tables shared by the Aria host and native plugins.
//
// A native plugin is a shared library that exports `aria_plugin_entry` with the
// `AriaPluginEntryFn` signature. The host calls it once, when it loads the library,
// with its `AriaHostV1` table and `ARIA_API_MAJOR`. The plugin returns its
// `AriaPluginV1` table, or null to decline the load. Both tables stay valid while
// the library is loaded. `include/aria_plugin.h` declares the same items for C and C++.
//
// Both tables start with the fields of `AriaTableHeader`. `struct_size` is the byte
// size of the table its sender filled, and `api_major` is `ARIA_API_MAJOR`. A table
// with another major, or shorter than `ARIA_HOST_V1_MIN_SIZE` or
// `ARIA_PLUGIN_V1_MIN_SIZE`, fails to load. A longer table adds slots in the
// prefixes the `ARIA_*_V1_*_SIZE` constants mark, and each side reads only the slots
// the other side's `struct_size` covers.
//
// Every callback receives `host_ctx` first. It is valid until that callback returns.
// Pass it to the host calls made during the callback, and do not keep it.
//
// An `AriaStr` or `AriaBytes` argument borrows the caller's memory until the call
// returns. An `AriaStr` is UTF-8 and holds no terminating NUL. An `AriaOwnedStr` or
// `AriaOwnedBytes` in a returned value passes to the receiver, which releases it with
// the `free_string` or `free_bytes` slot of the sender's table: the host releases what
// a plugin returns with `AriaPluginV1.free_string` and `AriaPluginV1.free_bytes`,
// and a plugin releases what a host call returns with `AriaHostV1.free_string` and
// `AriaHostV1.free_bytes`.
//
// A callback that fails returns `ARIA_STATUS_ERR` with the message. A callback must
// not unwind into the host: it catches a panic or exception and returns
// `ARIA_STATUS_PANIC`, `ARIA_BYTES_PANIC` or `ARIA_INSTANCE_PANIC` with the
// message, and the host fails the plugin for the rest of the session. `driver_evaluate`
// is the one exception: a panic there fails only that evaluation.

#ifndef ARIA_PLUGIN_H
#define ARIA_PLUGIN_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Major version of this C ABI. The host passes it to `aria_plugin_entry`, and both
/// tables hold it in `api_major`.
#define ARIA_API_MAJOR 1u

/// Name of the function a native plugin library exports as its entry point, with the
/// `AriaPluginEntryFn` signature.
#define ARIA_PLUGIN_ENTRY_SYMBOL "aria_plugin_entry"

/// `AriaResult.status`: the call succeeded, and `value` holds its result or is empty.
#define ARIA_STATUS_OK 0u

/// `AriaResult.status`: the call failed, and `value` holds the error message.
#define ARIA_STATUS_ERR 1u

/// `AriaResult.status`: the plugin caught a panic or exception, and `value` holds
/// its message.
#define ARIA_STATUS_PANIC 2u

/// `AriaResult.status`: a decoder call on a file the decoder does not read. The
/// host tries the next decoder without reporting the message.
#define ARIA_STATUS_UNSUPPORTED 3u

/// `AriaInstanceResult.status`: the call created the instance in `instance`. Every
/// call that returns an `AriaInstanceResult` uses these codes: `audio_create`,
/// `encoder_open`, `decoder_open_video`, `decoder_open_audio`, `analyzer_open` and
/// `start_job`.
#define ARIA_INSTANCE_OK ARIA_STATUS_OK

/// `AriaInstanceResult.status`: the call failed, and `error` holds the message.
#define ARIA_INSTANCE_ERR ARIA_STATUS_ERR

/// `AriaInstanceResult.status`: the plugin caught a panic or exception, and `error`
/// holds its message.
#define ARIA_INSTANCE_PANIC ARIA_STATUS_PANIC

/// `AriaInstanceResult.status`: a decoder open on a file the decoder does not read.
/// The host tries the next decoder without reporting the message.
#define ARIA_INSTANCE_UNSUPPORTED ARIA_STATUS_UNSUPPORTED

/// `AriaBytesResult.status`: `bytes` holds the result: the value stored under a
/// `storage_get` key, the panel tree JSON of `panel_view`, or the scene JSON of
/// `overlay_scene`.
#define ARIA_BYTES_FOUND 0u

/// `AriaBytesResult.status`: `storage_get` found no value under the key. `bytes` and
/// `error` are empty.
#define ARIA_BYTES_NOT_FOUND 1u

/// `AriaBytesResult.status`: the call failed, and `error` holds the message.
#define ARIA_BYTES_ERR 2u

/// `AriaBytesResult.status`: the plugin caught a panic or exception in `panel_view`
/// or `overlay_scene`, and `error` holds its message.
#define ARIA_BYTES_PANIC 3u

/// `level` of `AriaHostV1.log`: trace.
#define ARIA_LOG_TRACE 0u

/// `level` of `AriaHostV1.log`: debug.
#define ARIA_LOG_DEBUG 1u

/// `level` of `AriaHostV1.log`: info.
#define ARIA_LOG_INFO 2u

/// `level` of `AriaHostV1.log`: warn.
#define ARIA_LOG_WARN 3u

/// `level` of `AriaHostV1.log`: error. The host logs any other level as an error too.
#define ARIA_LOG_ERROR 4u

/// `AriaVideoInfo.format`: a full-size Y plane, then interleaved U and V at
/// half width and half height.
#define ARIA_FORMAT_NV12 0u

/// `AriaVideoInfo.format`: full-size Y, then U, then V at half width and
/// half height.
#define ARIA_FORMAT_YUV420P 1u

/// `AriaVideoInfo.color`: BT.709 limited range.
#define ARIA_COLOR_BT709 0u

/// `AriaVideoInfo.color`: BT.601 limited range.
#define ARIA_COLOR_BT601 1u

/// `AriaVideoInfo.color`: BT.2020 primaries with the PQ transfer.
#define ARIA_COLOR_BT2020_PQ 2u

/// `AriaVideoInfo.color`: sRGB, full range.
#define ARIA_COLOR_SRGB 3u

/// `AriaParamValue.kind`: a scalar in `x`.
#define ARIA_PARAM_SCALAR 0u

/// `AriaParamValue.kind`: 2-d vector in `x` and `y`.
#define ARIA_PARAM_VEC2 1u

/// `AriaParamValue.kind`: RGBA8 colour in `r`, `g`, `b` and `a`.
#define ARIA_PARAM_COLOR 2u

/// `AriaParamValue.kind`: axis-aligned rect, with its left edge in `x`, its top
/// edge in `y`, its width in `z` and its height in `w`.
#define ARIA_PARAM_RECT 3u

/// `AriaParamValue.kind`: 3-d vector in `x`, `y` and `z`.
#define ARIA_PARAM_VEC3 4u

/// Borrowed UTF-8 text, valid until the call it is passed to returns. It holds no
/// terminating NUL.
typedef struct AriaStr {
    /// First byte of the text, or null when `len` is 0.
    const uint8_t *ptr;

    /// Length of the text in bytes.
    size_t len;
} AriaStr;

/// Borrowed bytes, valid until the call they are passed to returns.
typedef struct AriaBytes {
    /// First byte, or null when `len` is 0.
    const uint8_t *ptr;

    /// Number of bytes.
    size_t len;
} AriaBytes;

/// UTF-8 text returned across the boundary. The receiver releases it with the
/// `free_string` slot of the table of the side that allocated it. A `free_string`
/// accepts a null `ptr` and does nothing for it.
typedef struct AriaOwnedStr {
    /// First byte of the text, or null for an empty string that allocates nothing.
    uint8_t *ptr;

    /// Length of the text in bytes.
    size_t len;

    /// Allocation size in bytes, for the allocating side's `free_string`. The receiver
    /// does not read it.
    size_t cap;
} AriaOwnedStr;

/// Bytes returned across the boundary. The receiver releases them with the
/// `free_bytes` slot of the table of the side that allocated them. A `free_bytes`
/// accepts a null `ptr` and does nothing for it.
typedef struct AriaOwnedBytes {
    /// First byte, or null for an empty buffer that allocates nothing.
    uint8_t *ptr;

    /// Number of bytes.
    size_t len;

    /// Allocation size in bytes, for the allocating side's `free_bytes`. The receiver
    /// does not read it.
    size_t cap;
} AriaOwnedBytes;

/// Result of a call that returns text: a JSON document, a message, or nothing.
typedef struct AriaResult {
    /// An `ARIA_STATUS_*` value.
    uint32_t status;

    /// The result on `ARIA_STATUS_OK`, empty when the call returns nothing, and the
    /// message on any other status.
    AriaOwnedStr value;
} AriaResult;

/// Result of a call that returns bytes: `storage_get`, `panel_view` and
/// `overlay_scene`.
typedef struct AriaBytesResult {
    /// An `ARIA_BYTES_*` value.
    uint32_t status;

    /// The result on `ARIA_BYTES_FOUND`, otherwise empty.
    AriaOwnedBytes bytes;

    /// The message on `ARIA_BYTES_ERR` and `ARIA_BYTES_PANIC`, otherwise empty.
    AriaOwnedStr error;
} AriaBytesResult;

/// Result of a call that creates an instance the host addresses in later calls.
typedef struct AriaInstanceResult {
    /// An `ARIA_INSTANCE_*` value.
    uint32_t status;

    /// The new instance's id on `ARIA_INSTANCE_OK`, otherwise 0. The side that creates
    /// the instance picks the id, and the other side passes it back unchanged.
    uint64_t instance;

    /// The message on any status but `ARIA_INSTANCE_OK`, otherwise empty.
    AriaOwnedStr error;
} AriaInstanceResult;

/// One parameter of an audio effect across one `AriaAudioBlock`: a constant, or one
/// value per frame when the parameter is automated.
typedef struct AriaAudioParam {
    /// The value for every frame of the block when `per_frame` is null. 0 when the
    /// parameter has no value.
    float constant;

    /// One value per frame, or null when the parameter is constant.
    const float *per_frame;

    /// Number of values at `per_frame`: the block's `frames`, or 0.
    size_t per_frame_len;
} AriaAudioParam;

/// One stereo block `audio_process` processes in place.
typedef struct AriaAudioBlock {
    /// `frames` samples of the left channel, read and overwritten in place.
    float *left;

    /// `frames` samples of the right channel, read and overwritten in place.
    float *right;

    /// Frames in the block.
    size_t frames;

    /// Timeline position of the block's first frame, in frames at `sample_rate`.
    /// Consecutive blocks continue each other until the next `audio_reset`.
    int64_t position;

    /// Frames per second.
    uint32_t sample_rate;

    /// 1 when `sidechain_left` and `sidechain_right` hold the key signal, otherwise 0.
    uint32_t has_sidechain;

    /// `frames` samples of the key signal's left channel, or null. A silent or muted
    /// key lane reads as zeros.
    const float *sidechain_left;

    /// `frames` samples of the key signal's right channel, or null.
    const float *sidechain_right;
} AriaAudioBlock;

/// The export settings `encoder_open` receives.
typedef struct AriaEncoderConfig {
    /// Frame width in pixels.
    uint32_t width;

    /// Frame height in pixels.
    uint32_t height;

    /// Frame rate numerator: `fps_num / fps_den` frames per second.
    int64_t fps_num;

    /// Frame rate denominator.
    int32_t fps_den;

    /// Target video bitrate in kbit/s, or 0 when the export sets none.
    uint32_t video_bitrate_kbps;

    /// Target audio bitrate in kbit/s. The host sends 0, for no target.
    uint32_t audio_bitrate_kbps;
} AriaEncoderConfig;

/// One export frame `encoder_push_frame` receives.
typedef struct AriaEncoderFrame {
    /// Straight-alpha RGBA8 pixels, `height` rows of `stride` bytes, top row first.
    const uint8_t *rgba;

    /// Number of bytes at `rgba`.
    size_t rgba_len;

    /// Frame width in pixels.
    uint32_t width;

    /// Frame height in pixels.
    uint32_t height;

    /// Bytes per row, at least `width * 4`.
    uint32_t stride;

    /// Presentation time in ticks of `pts_rate` per second.
    int64_t pts_value;

    /// Ticks per second of `pts_value`.
    int32_t pts_rate;
} AriaEncoderFrame;

/// One block of export audio `encoder_push_audio` receives.
typedef struct AriaEncoderAudio {
    /// Interleaved samples, `channels` per frame.
    const float *samples;

    /// Number of samples at `samples`, a multiple of `channels`.
    size_t samples_len;

    /// Frames per second.
    uint32_t sample_rate;

    /// Channels per frame.
    uint32_t channels;

    /// Presentation time of the first frame, in ticks of `pts_rate` per second.
    int64_t pts_value;

    /// Ticks per second of `pts_value`.
    int32_t pts_rate;
} AriaEncoderAudio;

/// Progress a plugin reports from `job_step`. The host sets `progress` to -1 and
/// `done` and `commit` to 0 before each call. The `AriaResult` value of `job_step` is
/// the detail line.
typedef struct AriaJobStep {
    /// Fraction done, from 0 to 1. A value below 0, or NaN, reports progress as
    /// unknown, and a value above 1 reads as 1.
    float progress;

    /// 1 when the job has finished, otherwise 0.
    uint32_t done;

    /// 1 when the step starts the job's commit, after which Cancel no longer stops it.
    uint32_t commit;
} AriaJobStep;

/// What `decoder_open_video` reports about the stream it opened. The host fills the
/// struct with zeros before the call and refuses a stream whose values break the
/// rules below.
typedef struct AriaVideoInfo {
    /// Frame width in pixels: even, from 2 to 16384.
    uint32_t width;

    /// Frame height in pixels: even, from 2 to 16384.
    uint32_t height;

    /// An `ARIA_FORMAT_*` value.
    uint32_t format;

    /// An `ARIA_COLOR_*` value.
    uint32_t color;

    /// Frame rate numerator: `frame_rate_num / frame_rate_den` frames per second, positive.
    int32_t frame_rate_num;

    /// Frame rate denominator, positive.
    int32_t frame_rate_den;

    /// Time base numerator: every pts the stream reports counts ticks at
    /// `time_base_num / time_base_den` ticks per second, positive.
    int32_t time_base_num;

    /// Time base denominator, positive.
    int32_t time_base_den;

    /// Length of the stream in time base ticks, or negative when unknown.
    int64_t duration;
} AriaVideoInfo;

/// One plane the host allocates for `decoder_next_frame` to fill.
typedef struct AriaFramePlane {
    /// `len` writable bytes, zeroed.
    uint8_t *data;

    /// Number of bytes at `data`: `stride * rows`.
    size_t len;

    /// Bytes per row: the frame width, or half of it for the U and V planes of
    /// `ARIA_FORMAT_YUV420P`.
    uint32_t stride;

    /// Rows: the frame height for the Y plane, half of it for the others.
    uint32_t rows;
} AriaFramePlane;

/// The frame `decoder_next_frame` writes. The plugin fills the planes, sets `pts`, and
/// sets `produced` to 1, or leaves `produced` at 0 at end of stream.
typedef struct AriaVideoFrameOut {
    /// The planes of the stream's format, in plane order: Y then UV for
    /// `ARIA_FORMAT_NV12`, Y then U then V for `ARIA_FORMAT_YUV420P`.
    AriaFramePlane *planes;

    /// Number of planes at `planes`.
    size_t planes_len;

    /// Presentation time of the frame in time base ticks. The host sets 0.
    int64_t pts;

    /// 1 when the plugin wrote a frame, 0 at end of stream. The host sets 0.
    uint32_t produced;
} AriaVideoFrameOut;

/// The frame `scope_measure` reads and the image it draws.
///
/// `frame` holds `frame_width * frame_height` RGBA8 pixels in `frame_len`
/// bytes: sRGB, straight alpha, top row first, no row padding. `image` holds
/// `image_width * image_height` pixels of the same layout in `image_len` bytes,
/// transparent black on entry, at the size the scope registered.
typedef struct AriaScopeCall {
    /// Pixels of the measured frame.
    const uint8_t *frame;

    /// Number of bytes at `frame`.
    size_t frame_len;

    /// Width of the measured frame in pixels.
    uint32_t frame_width;

    /// Height of the measured frame in pixels.
    uint32_t frame_height;

    /// Pixels of the scope image, which the plugin overwrites.
    uint8_t *image;

    /// Number of bytes at `image`. The plugin cannot resize the image.
    size_t image_len;

    /// Width of the scope image in pixels, as registered.
    uint32_t image_width;

    /// Height of the scope image in pixels, as registered.
    uint32_t image_height;
} AriaScopeCall;

/// The frame and parameters `cpu_effect_process` reads and the output frame it writes.
///
/// `input` and `output` each hold `width * height` premultiplied RGBA16F pixels
/// in `pixels_len` 16-bit channel elements (`width * height * 4`), each an IEEE-754
/// binary16 value, top row first, with no row padding. `params` holds `params_len`
/// flattened 32-bit float values in catalog order (1 per scalar, 2 per vec2, 4 per
/// colour with each channel from 0 to 1).
typedef struct AriaCpuEffectCall {
    /// The source frame, or a transparent frame for a generator.
    const uint16_t *input;

    /// The frame the plugin writes, zeroed on entry. The host reads it only when the
    /// call returns `ARIA_STATUS_OK`.
    uint16_t *output;

    /// Number of 16-bit elements at `input` and at `output`.
    size_t pixels_len;

    /// Frame width in pixels.
    uint32_t width;

    /// Frame height in pixels.
    uint32_t height;

    /// Flattened parameter values.
    const float *params;

    /// Number of values at `params`.
    size_t params_len;

    /// The effect instance being rendered, serialized as JSON, or empty when there is
    /// none.
    AriaStr params_json;

    /// Seconds since the clip's first frame.
    float time;
} AriaCpuEffectCall;

/// One parameter value passed to or returned from `driver_evaluate`. `kind` selects
/// the fields that hold it; the others are 0.
typedef struct AriaParamValue {
    /// An `ARIA_PARAM_*` value.
    uint32_t kind;

    /// First component of a scalar, vector or rect.
    float x;

    /// Second component of a vector or rect.
    float y;

    /// Third component of a 3-d vector, or the width of a rect.
    float z;

    /// Height of a rect.
    float w;

    /// Red channel of a colour.
    uint8_t r;

    /// Green channel of a colour.
    uint8_t g;

    /// Blue channel of a colour.
    uint8_t b;

    /// Alpha channel of a colour.
    uint8_t a;
} AriaParamValue;

/// What `driver_evaluate` reads and the value it writes. Times are sequence seconds.
typedef struct AriaDriverEvalCall {
    /// The time being evaluated.
    double time;

    /// Seconds since the first frame of the driver's clip.
    double clip_time;

    /// Frames per second of the sequence.
    double frame_rate;

    /// Length of the sequence in seconds.
    double duration;

    /// Canvas width in pixels.
    uint32_t canvas_width;

    /// Canvas height in pixels.
    uint32_t canvas_height;

    /// The property's own keyframed value at `time`.
    AriaParamValue value;

    /// A number fixed for this clip and property, for seeded noise.
    uint64_t seed;

    /// The value the plugin writes. The host sets it to `value` before the call, and
    /// its `kind` must stay the same.
    AriaParamValue output;
} AriaDriverEvalCall;

/// The calls a host makes available to a plugin. The host passes it to
/// `aria_plugin_entry`, and it stays valid while the library is loaded.
///
/// A `register_*` call and `subscribe` succeed only while `mount` runs. Every other
/// call works in any callback. A call returns `ARIA_STATUS_ERR` with the reason when it
/// fails, including when `host_ctx` is null or an `AriaStr` argument is not UTF-8.
typedef struct AriaHostV1 {
    /// Byte size of this table.
    size_t struct_size;

    /// `ARIA_API_MAJOR`.
    uint32_t api_major;

    /// 0.
    uint32_t reserved;

    /// Release an `AriaOwnedStr` a host call returned.
    void (*free_string)(AriaOwnedStr str);

    /// Release an `AriaOwnedBytes` a host call returned.
    void (*free_bytes)(AriaOwnedBytes bytes);

    /// Write `text` to the editor log under the plugin's id. `level` is an
    /// `ARIA_LOG_*` value. A null `host_ctx` or text that is not UTF-8 writes nothing.
    void (*log)(void *host_ctx, uint32_t level, AriaStr text);

    /// Read the value stored under `key` in the plugin's storage directory,
    /// `<data dir>/plugin-data/<plugin id>`. Returns `ARIA_BYTES_FOUND` with the
    /// bytes, or `ARIA_BYTES_NOT_FOUND`. A key is one file name: not empty, not `.` or
    /// `..`, and without `/`, `\` or NUL.
    AriaBytesResult (*storage_get)(void *host_ctx, AriaStr key);

    /// Store `value` under `key`, replacing any earlier value. Keys follow the rules of
    /// `storage_get`.
    AriaResult (*storage_set)(void *host_ctx, AriaStr key, AriaBytes value);

    /// Delete the value stored under `key`. Deleting a key with no value succeeds.
    AriaResult (*storage_delete)(void *host_ctx, AriaStr key);

    /// The stored keys as a sorted JSON array of strings.
    AriaResult (*storage_keys)(void *host_ctx);

    /// The current value of the setting `key` as JSON, or an empty value when the
    /// setting has none.
    AriaResult (*setting_get)(void *host_ctx, AriaStr key);

    /// Register the command `id` with the display name `title`. Running it calls
    /// `AriaPluginV1.command_run` with `id`.
    AriaResult (*register_command)(void *host_ctx, AriaStr id, AriaStr title);

    /// Deliver events of `event_kind` to `AriaPluginV1.event`. `event_kind` is a
    /// kind from `docs/plugins/events.md`. `resolve-dropped:<n>` subscribes with the
    /// priority `n`, a 32-bit integer; a higher priority runs first, and the default
    /// is 0.
    AriaResult (*subscribe)(void *host_ctx, AriaStr event_kind);

    /// Register an importer. `spec_json` is an object with `id`, `name`, and
    /// `extensions`, an array of file extensions without the dot. Importing a file
    /// calls `AriaPluginV1.import_file`.
    AriaResult (*register_importer)(void *host_ctx, AriaStr spec_json);

    /// Register an exporter. `spec_json` is an object with `id`, `name`, and
    /// `extension`, without the dot. Exporting calls `AriaPluginV1.export_file`.
    AriaResult (*register_exporter)(void *host_ctx, AriaStr spec_json);

    /// Register an audio effect. `spec_json` is an object with `id`, `label`, and the
    /// optional `category`, `latency` in sample frames at 48 kHz, `sidechain`, and
    /// `params`: an array of objects with `name`, `label`, `default`, `min`, and `max`,
    /// in the order `AriaPluginV1.audio_process` receives them. Each use of the
    /// effect calls `AriaPluginV1.audio_create`.
    AriaResult (*register_audio_effect)(void *host_ctx, AriaStr spec_json);

    /// The project as JSON. Requires `project = "read"` or `"read-write"` in the
    /// manifest's `[permissions]`.
    AriaResult (*project_snapshot)(void *host_ctx);

    /// The editor state as a JSON object with `selection` (clip ids), `playhead`,
    /// `mark_in`, `mark_out` (times or null), `playing`, and `speed`. Requires project
    /// read permission.
    AriaResult (*editor_view)(void *host_ctx);

    /// Apply `commands_json`, a JSON array of commands, as one undo step named
    /// `label`, or apply none of them. The value is an object with the new `revision`
    /// and the `outcomes` of the commands. Requires `project = "read-write"`.
    AriaResult (*project_transact)(void *host_ctx, AriaStr label, AriaStr commands_json);

    /// Undo the last undo step. The value is `true`, or `false` when there was nothing
    /// to undo. Requires `project = "read-write"`.
    AriaResult (*project_undo)(void *host_ctx);

    /// Register a panel. `spec_json` is an object with `id`, `title`, and `location`:
    /// `sidebar`, `inspector`, or `timeline`. Any other location is refused. Drawing it
    /// calls `AriaPluginV1.panel_view`. Fails unless the plugin table has the panel
    /// slots.
    AriaResult (*register_panel)(void *host_ctx, AriaStr spec_json);

    /// Redraw the panel `id`, which calls `AriaPluginV1.panel_view` again.
    AriaResult (*refresh_panel)(void *host_ctx, AriaStr id);

    /// Register an export encoder. `spec_json` is an object with `id`, `name`,
    /// `extension`, and the optional `audio`, true when the encoder takes audio.
    /// Exporting with it calls `AriaPluginV1.encoder_open`. Fails unless the plugin
    /// table has the encoder slots.
    AriaResult (*register_encoder)(void *host_ctx, AriaStr spec_json);

    /// Register a viewer overlay. `spec_json` is an object with `id` and `title`.
    /// Drawing it calls `AriaPluginV1.overlay_scene`. Fails unless the plugin table
    /// has the overlay slots.
    AriaResult (*register_viewer_overlay)(void *host_ctx, AriaStr spec_json);

    /// Redraw the viewer overlay `id`, which calls `AriaPluginV1.overlay_scene`
    /// again.
    AriaResult (*refresh_viewer_overlay)(void *host_ctx, AriaStr id);

    /// Start a background job named `label` on the Jobs board. The host calls
    /// `AriaPluginV1.job_step` on the job thread until a step reports `done`, a step
    /// fails, or Cancel is accepted, then `AriaPluginV1.job_end`. The instance is the
    /// job id both receive. The first step can run before `start_job` returns. Fails
    /// unless the plugin table has the job slots.
    AriaInstanceResult (*start_job)(void *host_ctx, AriaStr label);

    /// Register a media decoder. `spec_json` is an object with `id`, `name`,
    /// `extensions`, and the optional `priority`; a higher priority is tried first.
    /// Fails unless the plugin table has the decoder slots.
    AriaResult (*register_decoder)(void *host_ctx, AriaStr spec_json);

    /// Register an assistant tool. `spec_json` is an object with `name`,
    /// `description`, `parameters` (a JSON Schema), and `tier`: `read-only`,
    /// `workspace`, or `system`. A call to the tool calls `AriaPluginV1.ai_tool_call`.
    /// Fails unless the plugin table has the AI tool slot.
    AriaResult (*register_ai_tool)(void *host_ctx, AriaStr spec_json);

    /// Register a scope. `spec_json` is an object with `id`, `name`, and the `width`
    /// and `height` of its image in pixels. Drawing it calls
    /// `AriaPluginV1.scope_measure`. Fails unless the plugin table has the scope slot.
    AriaResult (*register_scope)(void *host_ctx, AriaStr spec_json);

    /// Register a GPU effect. `spec_json` is an effect spec object, as in
    /// `docs/plugins/effects.md`. `wgsl` is WGSL source, a JSON array of WGSL strings
    /// for a chain of fragment passes, or a JSON array of graph pass objects.
    AriaResult (*register_effect)(void *host_ctx, AriaStr spec_json, AriaStr wgsl);

    /// Register an effect that `AriaPluginV1.cpu_effect_process` draws. `spec_json`
    /// is an effect spec object. Fails unless the plugin table has the contribution
    /// slots.
    AriaResult (*register_cpu_effect)(void *host_ctx, AriaStr spec_json);

    /// Register a transition. `spec_json` is a transition spec object and `wgsl` its
    /// WGSL source.
    AriaResult (*register_transition)(void *host_ctx, AriaStr spec_json, AriaStr wgsl);

    /// Register a generator. `spec_json` is an effect spec object, and `wgsl` takes the
    /// forms of `register_effect`. An empty `wgsl` registers a generator that
    /// `AriaPluginV1.cpu_effect_process` draws, which requires the contribution slots.
    AriaResult (*register_generator)(void *host_ctx, AriaStr spec_json, AriaStr wgsl);

    /// Register a clip type. `spec_json` is an object with `id`, `label`, `colour`,
    /// and the optional `generator`, `icon_svg`, `handles`, and `schema`, as in
    /// `docs/plugins/clip-types.md`.
    AriaResult (*register_clip_type)(void *host_ctx, AriaStr spec_json);

    /// Register a driver language, whose expressions `AriaPluginV1.driver_evaluate`
    /// evaluates. `spec_json` is a driver language spec object, as in
    /// `docs/plugins/drivers.md`. Fails unless the plugin table has the contribution
    /// slots.
    AriaResult (*register_driver_language)(void *host_ctx, AriaStr spec_json);

    /// Register a preset. `preset_json` is a preset object, the JSON form of an
    /// `.ariapreset` file.
    AriaResult (*register_preset)(void *host_ctx, AriaStr preset_json);

    /// Register the theme `id` from `toml`, the text of a theme file.
    AriaResult (*register_theme)(void *host_ctx, AriaStr id, AriaStr toml);

    /// Register a workspace. `spec_json` is a workspace spec object, as in
    /// `docs/plugins/workspaces.md`.
    AriaResult (*register_workspace)(void *host_ctx, AriaStr spec_json);

    /// Register an audio analyzer, which runs through `AriaPluginV1.analyzer_open`.
    /// `spec_json` is an analyzer spec object, as in `docs/plugins/analyzers.md`. Fails
    /// unless the plugin table has the contribution slots.
    AriaResult (*register_analyzer)(void *host_ctx, AriaStr spec_json);

    /// The clip `id` as a JSON object with `id`, `track`, `start`, `end`, `kind`, and
    /// `name`, from the open sequence, the root timeline, or a nested sequence. An
    /// empty value when no clip has the id. Requires project read permission.
    AriaResult (*clip)(void *host_ctx, uint64_t id);

    /// The clips of `sequence_id` that overlap the half-open range `range_json`, a JSON
    /// object with `start` and `duration` times, as a JSON array in track order, then
    /// time order. A `sequence_id` of 0 reads the open sequence, and an unknown id
    /// returns an empty array. Requires project read permission.
    AriaResult (*clips_in_range)(void *host_ctx, uint64_t sequence_id, AriaStr range_json);

    /// The clips of `sequence_id` with `start <= tick < end`, where `tick_json` is a
    /// time object with `value` and `rate`, as a JSON array in track order, then time
    /// order. `sequence_id` reads as in `clips_in_range`. Requires project read
    /// permission.
    AriaResult (*clips_at)(void *host_ctx, uint64_t sequence_id, AriaStr tick_json);

    /// The selected clips as a JSON array, in selection order. Requires project read
    /// permission.
    AriaResult (*selected_clips)(void *host_ctx);

    /// The markers of `sequence_id` whose point or span overlaps the half-open range
    /// `range_json`, as a JSON array ordered by time. `sequence_id` and `range_json`
    /// read as in `clips_in_range`. Requires project read permission.
    AriaResult (*markers_in_range)(void *host_ctx, uint64_t sequence_id, AriaStr range_json);

    /// The In and Out points as a JSON object with `mark_in` and `mark_out`, each a
    /// time or null. Requires project read permission.
    AriaResult (*in_out)(void *host_ctx);

    /// Register `width * height * 4` straight-alpha RGBA8 `pixels` as the
    /// plugin texture `id`, which an effect pass reads as an input.
    AriaResult (*register_texture)(void *host_ctx, AriaStr id, uint32_t width, uint32_t height, AriaBytes pixels);
} AriaHostV1;

/// The callbacks a plugin exports. `aria_plugin_entry` returns it, and it stays valid
/// while the library is loaded.
///
/// The host calls `mount` once, before any other callback. A callback that has no
/// result returns `ARIA_STATUS_OK` with an empty `value`.
typedef struct AriaPluginV1 {
    /// Byte size of this table, at least `ARIA_PLUGIN_V1_MIN_SIZE`.
    size_t struct_size;

    /// `ARIA_API_MAJOR`.
    uint32_t api_major;

    /// 0.
    uint32_t reserved;

    /// Release an `AriaOwnedStr` a callback returned.
    void (*free_string)(AriaOwnedStr str);

    /// Release an `AriaOwnedBytes` a callback returned.
    void (*free_bytes)(AriaOwnedBytes bytes);

    /// Register the plugin's contributions with the `register_*` host calls and
    /// `subscribe`. `ARIA_STATUS_ERR` fails the load with the message.
    AriaResult (*mount)(void *host_ctx);

    /// Release the plugin's state before the library unloads. Called once, after a
    /// `mount` that succeeded, unless the plugin has failed.
    AriaResult (*unmount)(void *host_ctx);

    /// Run the command `id`, which the plugin registered with
    /// `AriaHostV1.register_command` or declared in `plugin.toml`. `args_json` is a
    /// JSON object, `{}` for a command run from the editor. `ARIA_STATUS_ERR` reports
    /// the message as the command's failure.
    AriaResult (*command_run)(void *host_ctx, AriaStr id, AriaStr args_json);

    /// Handle an event of the subscribed `kind`. `event_json` is its payload and the
    /// value is the reply, or empty for none, as `docs/plugins/events.md` lists them.
    /// `ARIA_STATUS_ERR` is logged and counts as no reply.
    AriaResult (*event)(void *host_ctx, AriaStr kind, AriaStr event_json);

    /// Import the file at `path` with the importer `importer_id`. The value is a JSON
    /// object with the optional `commands_json`, a string holding a JSON array of
    /// commands the host applies as one undo step, and `skipped`, an array of strings
    /// naming what the import left out. `ARIA_STATUS_ERR` cancels the import.
    AriaResult (*import_file)(void *host_ctx, AriaStr importer_id, AriaStr path);

    /// Write `project_json`, the project as JSON, with the exporter `exporter_id` to
    /// the file `path`, a staging file in the plugin's storage directory that the host
    /// copies to the destination. The value is empty or a JSON object with `skipped`,
    /// an array of strings naming what the file leaves out.
    AriaResult (*export_file)(void *host_ctx, AriaStr exporter_id, AriaStr project_json, AriaStr path);

    /// Create an instance of the audio effect `effect_id` at `sample_rate` frames per
    /// second, with fresh state. The host creates one per insert of the effect.
    AriaInstanceResult (*audio_create)(void *host_ctx, AriaStr effect_id, uint32_t sample_rate);

    /// Process `block` in place. `params` holds `params_len` values, one per parameter
    /// of the effect's spec, in spec order.
    AriaResult (*audio_process)(void *host_ctx, uint64_t instance, const AriaAudioParam *params, size_t params_len, AriaAudioBlock *block);

    /// Clear every value carried over from earlier blocks, such as delay lines and
    /// envelopes. Called before a block that does not continue the last one, such as
    /// after a seek.
    AriaResult (*audio_reset)(void *host_ctx, uint64_t instance);

    /// Release an instance `audio_create` returned.
    AriaResult (*audio_drop)(void *host_ctx, uint64_t instance);

    /// The contents of the panel `panel_id`: `ARIA_BYTES_FOUND` with the UI tree as
    /// JSON, as in `docs/plugins/panels.md`, at most 257 MiB. Any other result shows the
    /// error in the panel and records a fault.
    AriaBytesResult (*panel_view)(void *host_ctx, AriaStr panel_id);

    /// Handle `event_json`, a control event in the panel `panel_id`.
    AriaResult (*panel_event)(void *host_ctx, AriaStr panel_id, AriaStr event_json);

    /// Start writing the file `path` with the encoder `encoder_id` and the settings
    /// `config`. The instance receives the export's frames and audio.
    AriaInstanceResult (*encoder_open)(void *host_ctx, AriaStr encoder_id, AriaStr path, const AriaEncoderConfig *config);

    /// Encode the next frame of the export.
    AriaResult (*encoder_push_frame)(void *host_ctx, uint64_t instance, const AriaEncoderFrame *frame);

    /// Encode the next block of the export's audio.
    AriaResult (*encoder_push_audio)(void *host_ctx, uint64_t instance, const AriaEncoderAudio *audio);

    /// Complete the file after the last frame. The host makes no further call on the
    /// instance.
    AriaResult (*encoder_finish)(void *host_ctx, uint64_t instance);

    /// Abandon an export that stopped before `encoder_finish`, and release the instance.
    AriaResult (*encoder_drop)(void *host_ctx, uint64_t instance);

    /// The scene of the viewer overlay `overlay_id`: `ARIA_BYTES_FOUND` with the
    /// scene as JSON, as in `docs/plugins/viewer-overlays.md`, at most 4 MiB. Any other
    /// result refuses the scene with the error.
    AriaBytesResult (*overlay_scene)(void *host_ctx, AriaStr overlay_id);

    /// Handle `event_json`, a pointer event on a handle of the overlay `overlay_id`.
    AriaResult (*overlay_event)(void *host_ctx, AriaStr overlay_id, AriaStr event_json);

    /// Run one bounded step of the job `job` and write its progress to `step`. The
    /// value is the detail line. `ARIA_STATUS_ERR` ends the job as failed.
    AriaResult (*job_step)(void *host_ctx, uint64_t job, AriaJobStep *step);

    /// The job `job` has ended. `cancelled` is 1 when Cancel ended it, otherwise 0.
    AriaResult (*job_end)(void *host_ctx, uint64_t job, uint32_t cancelled);

    /// Describe the file at `path` for the decoder `decoder_id`. The value is a JSON
    /// object with `frame_rate_num` and `frame_rate_den`, and the optional `width`,
    /// `height`, `frame_count`, `has_audio`, `is_image`, `video_codec`, and
    /// `audio_codec`. `ARIA_STATUS_UNSUPPORTED` passes the file to the next decoder.
    AriaResult (*decoder_probe)(void *host_ctx, AriaStr decoder_id, AriaStr path);

    /// Open the video stream of `path` and write its description to `info`.
    /// `ARIA_INSTANCE_UNSUPPORTED` passes the file to the next decoder.
    AriaInstanceResult (*decoder_open_video)(void *host_ctx, AriaStr decoder_id, AriaStr path, AriaVideoInfo *info);

    /// Position the video instance at the sync frame at or before `pts`, in time base
    /// ticks.
    AriaResult (*decoder_seek)(void *host_ctx, uint64_t instance, int64_t pts);

    /// Decode the next frame of the video instance into `frame`.
    AriaResult (*decoder_next_frame)(void *host_ctx, uint64_t instance, AriaVideoFrameOut *frame);

    /// Open the audio of `path`, resampled to `sample_rate` frames per second and
    /// `channels` channels. `ARIA_INSTANCE_UNSUPPORTED` passes the file to the next
    /// decoder.
    AriaInstanceResult (*decoder_open_audio)(void *host_ctx, AriaStr decoder_id, AriaStr path, uint32_t sample_rate, uint32_t channels);

    /// Write up to `out_len` interleaved samples to `out`, and their count to
    /// `*written`: whole frames, and fewer than `out_len` only at end of stream.
    AriaResult (*decoder_read_audio)(void *host_ctx, uint64_t instance, float *out, size_t out_len, size_t *written);

    /// Position the next read of the audio instance at output frame `frame`.
    AriaResult (*decoder_seek_audio)(void *host_ctx, uint64_t instance, int64_t frame);

    /// Release an instance `decoder_open_video` or `decoder_open_audio` returned.
    AriaResult (*decoder_close)(void *host_ctx, uint64_t instance);

    /// Run the assistant tool `name`, as its spec registered it, with `arguments_json`,
    /// an object that matches its `parameters` schema. The value is a JSON object with
    /// the optional `text` and `images`, an array of objects with `label` and `png`,
    /// the PNG file as an array of bytes.
    AriaResult (*ai_tool_call)(void *host_ctx, AriaStr name, AriaStr arguments_json);

    /// Measure the frame in `call` for the scope `scope_id` and draw the scope image.
    /// The value is the readout text shown with the scope.
    AriaResult (*scope_measure)(void *host_ctx, AriaStr scope_id, AriaScopeCall *call);

    /// Draw one frame of the CPU effect, generator, or clip type `effect_id` into
    /// `call.output`.
    AriaResult (*cpu_effect_process)(void *host_ctx, AriaStr effect_id, AriaCpuEffectCall *call);

    /// Evaluate `expr`, a non-empty expression in the driver language `language_id`,
    /// and write the result to `call.output`. `ARIA_STATUS_ERR` or
    /// `ARIA_STATUS_PANIC` fails this evaluation with the message.
    AriaResult (*driver_evaluate)(void *host_ctx, AriaStr language_id, AriaStr expr, AriaDriverEvalCall *call);

    /// Start a run of the analyzer `analyzer_id` over audio at `sample_rate` frames per
    /// second with `channels` channels.
    AriaInstanceResult (*analyzer_open)(void *host_ctx, AriaStr analyzer_id, uint32_t sample_rate, uint32_t channels);

    /// Read the next `samples_len` interleaved samples of the run.
    /// `ARIA_STATUS_ERR` stops the run with the message.
    AriaResult (*analyzer_push)(void *host_ctx, uint64_t instance, const float *samples, size_t samples_len);

    /// End the run after the last block. The value is the analyzer result as JSON, as
    /// in `docs/plugins/analyzers.md`. The host makes no further call on the instance.
    AriaResult (*analyzer_finish)(void *host_ctx, uint64_t instance);

    /// Release a run that ended before `analyzer_finish`.
    AriaResult (*analyzer_drop)(void *host_ctx, uint64_t instance);
} AriaPluginV1;

/// The entry point a native plugin exports under `ARIA_PLUGIN_ENTRY_SYMBOL`. `host`
/// is the host's table and `api_major` is `ARIA_API_MAJOR`. Returns the plugin's
/// table, or null to decline the load.
typedef const AriaPluginV1 *(*AriaPluginEntryFn)(const AriaHostV1 *host, uint32_t api_major);

const AriaPluginV1 *aria_plugin_entry(const AriaHostV1 *host, uint32_t api_major);

/// The fields both tables start with, which a receiver reads before any slot.
typedef struct AriaTableHeader {
    /// Byte size of the whole table.
    size_t struct_size;

    /// `ARIA_API_MAJOR` of the table's sender.
    uint32_t api_major;

    /// 0.
    uint32_t reserved;
} AriaTableHeader;

/// Smallest valid `AriaHostV1`: the byte size of every slot through `project_undo`.
#define ARIA_HOST_V1_MIN_SIZE offsetof(AriaHostV1, register_panel)

/// Smallest valid `AriaPluginV1`: the byte size of every slot through `audio_drop`.
#define ARIA_PLUGIN_V1_MIN_SIZE offsetof(AriaPluginV1, panel_view)

/// Byte size of an `AriaHostV1` through `refresh_panel`.
#define ARIA_HOST_V1_PANELS_SIZE offsetof(AriaHostV1, register_encoder)

/// Byte size of an `AriaPluginV1` through `panel_event`.
#define ARIA_PLUGIN_V1_PANELS_SIZE offsetof(AriaPluginV1, encoder_open)

/// Byte size of an `AriaHostV1` through `register_encoder`.
#define ARIA_HOST_V1_ENCODERS_SIZE offsetof(AriaHostV1, register_viewer_overlay)

/// Byte size of an `AriaPluginV1` through `encoder_drop`.
#define ARIA_PLUGIN_V1_ENCODERS_SIZE offsetof(AriaPluginV1, overlay_scene)

/// Byte size of an `AriaHostV1` through `refresh_viewer_overlay`.
#define ARIA_HOST_V1_OVERLAYS_SIZE offsetof(AriaHostV1, start_job)

/// Byte size of an `AriaPluginV1` through `overlay_event`.
#define ARIA_PLUGIN_V1_OVERLAYS_SIZE offsetof(AriaPluginV1, job_step)

/// Byte size of an `AriaHostV1` through `start_job`.
#define ARIA_HOST_V1_JOBS_SIZE offsetof(AriaHostV1, register_decoder)

/// Byte size of an `AriaPluginV1` through `job_end`.
#define ARIA_PLUGIN_V1_JOBS_SIZE offsetof(AriaPluginV1, decoder_probe)

/// Byte size of an `AriaHostV1` through `register_decoder`.
#define ARIA_HOST_V1_DECODERS_SIZE offsetof(AriaHostV1, register_ai_tool)

/// Byte size of an `AriaPluginV1` through `decoder_close`.
#define ARIA_PLUGIN_V1_DECODERS_SIZE offsetof(AriaPluginV1, ai_tool_call)

/// Byte size of an `AriaHostV1` through `register_ai_tool`.
#define ARIA_HOST_V1_AI_TOOLS_SIZE offsetof(AriaHostV1, register_scope)

/// Byte size of an `AriaPluginV1` through `ai_tool_call`.
#define ARIA_PLUGIN_V1_AI_TOOLS_SIZE offsetof(AriaPluginV1, scope_measure)

/// Byte size of an `AriaHostV1` through `register_scope`.
#define ARIA_HOST_V1_SCOPES_SIZE offsetof(AriaHostV1, register_effect)

/// Byte size of an `AriaPluginV1` through `scope_measure`.
#define ARIA_PLUGIN_V1_SCOPES_SIZE offsetof(AriaPluginV1, cpu_effect_process)

/// Byte size of an `AriaHostV1` through `register_analyzer`.
#define ARIA_HOST_V1_CONTRIBUTIONS_SIZE offsetof(AriaHostV1, clip)

/// Byte size of an `AriaHostV1` through `in_out`.
#define ARIA_HOST_V1_QUERIES_SIZE offsetof(AriaHostV1, register_texture)

#ifdef __cplusplus
}
#endif

#endif /* ARIA_PLUGIN_H */
