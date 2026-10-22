# Native ABI reference

<!-- Generated from crates/aria-plugin-abi/src/ by crates/aria-plugin-abi/tests/abi_reference/render.rs. Edit the doc comments in src/, then run `ARIA_BLESS_DOCS=1 cargo test -p aria-plugin-abi --test abi_reference`. -->

Stable C ABI types and versioned function tables shared by the Aria host and native plugins.

A native plugin is a shared library that exports `aria_plugin_entry` with the [`AriaPluginEntryFn`](#ariapluginentryfn) signature. The host calls it once, when it loads the library, with its [`AriaHostV1`](#ariahostv1) table and [`ARIA_API_MAJOR`](#version-and-entry). The plugin returns its [`AriaPluginV1`](#ariapluginv1) table, or null to decline the load. Both tables stay valid while the library is loaded. `include/aria_plugin.h` declares the same items for C and C++.

Both tables start with the fields of [`AriaTableHeader`](#ariatableheader). `struct_size` is the byte size of the table its sender filled, and `api_major` is [`ARIA_API_MAJOR`](#version-and-entry). A table with another major, or shorter than [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes) or [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes), fails to load. A longer table adds slots in the prefixes the `ARIA_*_V1_*_SIZE` constants mark, and each side reads only the slots the other side's `struct_size` covers.

Every callback receives `host_ctx` first. It is valid until that callback returns. Pass it to the host calls made during the callback, and do not keep it.

An [`AriaStr`](#ariastr) or [`AriaBytes`](#ariabytes) argument borrows the caller's memory until the call returns. An [`AriaStr`](#ariastr) is UTF-8 and holds no terminating NUL. An [`AriaOwnedStr`](#ariaownedstr) or [`AriaOwnedBytes`](#ariaownedbytes) in a returned value passes to the receiver, which releases it with the `free_string` or `free_bytes` slot of the sender's table: the host releases what a plugin returns with [`AriaPluginV1.free_string`](#ariapluginv1free_string) and [`AriaPluginV1.free_bytes`](#ariapluginv1free_bytes), and a plugin releases what a host call returns with [`AriaHostV1.free_string`](#ariahostv1free_string) and [`AriaHostV1.free_bytes`](#ariahostv1free_bytes).

A callback that fails returns [`ARIA_STATUS_ERR`](#status-codes) with the message. A callback must not unwind into the host: it catches a panic or exception and returns [`ARIA_STATUS_PANIC`](#status-codes), [`ARIA_BYTES_PANIC`](#bytes-status-codes) or [`ARIA_INSTANCE_PANIC`](#instance-status-codes) with the message, and the host fails the plugin for the rest of the session. `driver_evaluate` is the one exception: a panic there fails only that evaluation.

[Native plugins](native.md) shows how to build, package and load a plugin. `crates/aria-plugin-abi/include/aria_plugin.h` is generated from the same source as this page.

## Defines

### Version and entry

| Define | Value | Description |
| --- | --- | --- |
| `ARIA_API_MAJOR` | `1u` | Major version of this C ABI. The host passes it to `aria_plugin_entry`, and both tables hold it in `api_major`. |
| `ARIA_PLUGIN_ENTRY_SYMBOL` | `"aria_plugin_entry"` | Name of the function a native plugin library exports as its entry point, with the [`AriaPluginEntryFn`](#ariapluginentryfn) signature. |

### Status codes

| Define | Value | Description |
| --- | --- | --- |
| `ARIA_STATUS_OK` | `0u` | [`AriaResult.status`](#ariaresult): the call succeeded, and `value` holds its result or is empty. |
| `ARIA_STATUS_ERR` | `1u` | [`AriaResult.status`](#ariaresult): the call failed, and `value` holds the error message. |
| `ARIA_STATUS_PANIC` | `2u` | [`AriaResult.status`](#ariaresult): the plugin caught a panic or exception, and `value` holds its message. |
| `ARIA_STATUS_UNSUPPORTED` | `3u` | [`AriaResult.status`](#ariaresult): a decoder call on a file the decoder does not read. The host tries the next decoder without reporting the message. |

### Instance status codes

| Define | Value | Description |
| --- | --- | --- |
| `ARIA_INSTANCE_OK` | `ARIA_STATUS_OK` | [`AriaInstanceResult.status`](#ariainstanceresult): the call created the instance in `instance`. Every call that returns an [`AriaInstanceResult`](#ariainstanceresult) uses these codes: `audio_create`, `encoder_open`, `decoder_open_video`, `decoder_open_audio`, `analyzer_open` and `start_job`. |
| `ARIA_INSTANCE_ERR` | `ARIA_STATUS_ERR` | [`AriaInstanceResult.status`](#ariainstanceresult): the call failed, and `error` holds the message. |
| `ARIA_INSTANCE_PANIC` | `ARIA_STATUS_PANIC` | [`AriaInstanceResult.status`](#ariainstanceresult): the plugin caught a panic or exception, and `error` holds its message. |
| `ARIA_INSTANCE_UNSUPPORTED` | `ARIA_STATUS_UNSUPPORTED` | [`AriaInstanceResult.status`](#ariainstanceresult): a decoder open on a file the decoder does not read. The host tries the next decoder without reporting the message. |

### Bytes status codes

| Define | Value | Description |
| --- | --- | --- |
| `ARIA_BYTES_FOUND` | `0u` | [`AriaBytesResult.status`](#ariabytesresult): `bytes` holds the result: the value stored under a `storage_get` key, the panel tree JSON of `panel_view`, or the scene JSON of `overlay_scene`. |
| `ARIA_BYTES_NOT_FOUND` | `1u` | [`AriaBytesResult.status`](#ariabytesresult): `storage_get` found no value under the key. `bytes` and `error` are empty. |
| `ARIA_BYTES_ERR` | `2u` | [`AriaBytesResult.status`](#ariabytesresult): the call failed, and `error` holds the message. |
| `ARIA_BYTES_PANIC` | `3u` | [`AriaBytesResult.status`](#ariabytesresult): the plugin caught a panic or exception in `panel_view` or `overlay_scene`, and `error` holds its message. |

### Log levels

| Define | Value | Description |
| --- | --- | --- |
| `ARIA_LOG_TRACE` | `0u` | `level` of [`AriaHostV1.log`](#ariahostv1log): trace. |
| `ARIA_LOG_DEBUG` | `1u` | `level` of [`AriaHostV1.log`](#ariahostv1log): debug. |
| `ARIA_LOG_INFO` | `2u` | `level` of [`AriaHostV1.log`](#ariahostv1log): info. |
| `ARIA_LOG_WARN` | `3u` | `level` of [`AriaHostV1.log`](#ariahostv1log): warn. |
| `ARIA_LOG_ERROR` | `4u` | `level` of [`AriaHostV1.log`](#ariahostv1log): error. The host logs any other level as an error too. |

### Pixel formats

| Define | Value | Description |
| --- | --- | --- |
| `ARIA_FORMAT_NV12` | `0u` | [`AriaVideoInfo.format`](#ariavideoinfo): a full-size Y plane, then interleaved U and V at half width and half height. |
| `ARIA_FORMAT_YUV420P` | `1u` | [`AriaVideoInfo.format`](#ariavideoinfo): full-size Y, then U, then V at half width and half height. |

### Color encodings

| Define | Value | Description |
| --- | --- | --- |
| `ARIA_COLOR_BT709` | `0u` | [`AriaVideoInfo.color`](#ariavideoinfo): BT.709 limited range. |
| `ARIA_COLOR_BT601` | `1u` | [`AriaVideoInfo.color`](#ariavideoinfo): BT.601 limited range. |
| `ARIA_COLOR_BT2020_PQ` | `2u` | [`AriaVideoInfo.color`](#ariavideoinfo): BT.2020 primaries with the PQ transfer. |
| `ARIA_COLOR_SRGB` | `3u` | [`AriaVideoInfo.color`](#ariavideoinfo): sRGB, full range. |

### Parameter kinds

| Define | Value | Description |
| --- | --- | --- |
| `ARIA_PARAM_SCALAR` | `0u` | [`AriaParamValue.kind`](#ariaparamvalue): a scalar in `x`. |
| `ARIA_PARAM_VEC2` | `1u` | [`AriaParamValue.kind`](#ariaparamvalue): 2-d vector in `x` and `y`. |
| `ARIA_PARAM_COLOR` | `2u` | [`AriaParamValue.kind`](#ariaparamvalue): RGBA8 colour in `r`, `g`, `b` and `a`. |
| `ARIA_PARAM_RECT` | `3u` | [`AriaParamValue.kind`](#ariaparamvalue): axis-aligned rect, with its left edge in `x`, its top edge in `y`, its width in `z` and its height in `w`. |
| `ARIA_PARAM_VEC3` | `4u` | [`AriaParamValue.kind`](#ariaparamvalue): 3-d vector in `x`, `y` and `z`. |

### Table sizes

| Define | Value | Description |
| --- | --- | --- |
| `ARIA_HOST_V1_MIN_SIZE` | `offsetof(AriaHostV1, register_panel)` | Smallest valid [`AriaHostV1`](#ariahostv1): the byte size of every slot through `project_undo`. |
| `ARIA_PLUGIN_V1_MIN_SIZE` | `offsetof(AriaPluginV1, panel_view)` | Smallest valid [`AriaPluginV1`](#ariapluginv1): the byte size of every slot through `audio_drop`. |
| `ARIA_HOST_V1_PANELS_SIZE` | `offsetof(AriaHostV1, register_encoder)` | Byte size of an [`AriaHostV1`](#ariahostv1) through `refresh_panel`. |
| `ARIA_PLUGIN_V1_PANELS_SIZE` | `offsetof(AriaPluginV1, encoder_open)` | Byte size of an [`AriaPluginV1`](#ariapluginv1) through `panel_event`. |
| `ARIA_HOST_V1_ENCODERS_SIZE` | `offsetof(AriaHostV1, register_viewer_overlay)` | Byte size of an [`AriaHostV1`](#ariahostv1) through `register_encoder`. |
| `ARIA_PLUGIN_V1_ENCODERS_SIZE` | `offsetof(AriaPluginV1, overlay_scene)` | Byte size of an [`AriaPluginV1`](#ariapluginv1) through `encoder_drop`. |
| `ARIA_HOST_V1_OVERLAYS_SIZE` | `offsetof(AriaHostV1, start_job)` | Byte size of an [`AriaHostV1`](#ariahostv1) through `refresh_viewer_overlay`. |
| `ARIA_PLUGIN_V1_OVERLAYS_SIZE` | `offsetof(AriaPluginV1, job_step)` | Byte size of an [`AriaPluginV1`](#ariapluginv1) through `overlay_event`. |
| `ARIA_HOST_V1_JOBS_SIZE` | `offsetof(AriaHostV1, register_decoder)` | Byte size of an [`AriaHostV1`](#ariahostv1) through `start_job`. |
| `ARIA_PLUGIN_V1_JOBS_SIZE` | `offsetof(AriaPluginV1, decoder_probe)` | Byte size of an [`AriaPluginV1`](#ariapluginv1) through `job_end`. |
| `ARIA_HOST_V1_DECODERS_SIZE` | `offsetof(AriaHostV1, register_ai_tool)` | Byte size of an [`AriaHostV1`](#ariahostv1) through `register_decoder`. |
| `ARIA_PLUGIN_V1_DECODERS_SIZE` | `offsetof(AriaPluginV1, ai_tool_call)` | Byte size of an [`AriaPluginV1`](#ariapluginv1) through `decoder_close`. |
| `ARIA_HOST_V1_AI_TOOLS_SIZE` | `offsetof(AriaHostV1, register_scope)` | Byte size of an [`AriaHostV1`](#ariahostv1) through `register_ai_tool`. |
| `ARIA_PLUGIN_V1_AI_TOOLS_SIZE` | `offsetof(AriaPluginV1, scope_measure)` | Byte size of an [`AriaPluginV1`](#ariapluginv1) through `ai_tool_call`. |
| `ARIA_HOST_V1_SCOPES_SIZE` | `offsetof(AriaHostV1, register_effect)` | Byte size of an [`AriaHostV1`](#ariahostv1) through `register_scope`. |
| `ARIA_PLUGIN_V1_SCOPES_SIZE` | `offsetof(AriaPluginV1, cpu_effect_process)` | Byte size of an [`AriaPluginV1`](#ariapluginv1) through `scope_measure`. |
| `ARIA_HOST_V1_CONTRIBUTIONS_SIZE` | `offsetof(AriaHostV1, clip)` | Byte size of an [`AriaHostV1`](#ariahostv1) through `register_analyzer`. |
| `ARIA_HOST_V1_QUERIES_SIZE` | `offsetof(AriaHostV1, register_texture)` | Byte size of an [`AriaHostV1`](#ariahostv1) through `in_out`. |

## Types

### `AriaStr`

Borrowed UTF-8 text, valid until the call it is passed to returns. It holds no terminating NUL.

| Field | Type | Description |
| --- | --- | --- |
| `ptr` | `const uint8_t *` | First byte of the text, or null when `len` is 0. |
| `len` | `size_t` | Length of the text in bytes. |

### `AriaBytes`

Borrowed bytes, valid until the call they are passed to returns.

| Field | Type | Description |
| --- | --- | --- |
| `ptr` | `const uint8_t *` | First byte, or null when `len` is 0. |
| `len` | `size_t` | Number of bytes. |

### `AriaOwnedStr`

UTF-8 text returned across the boundary. The receiver releases it with the `free_string` slot of the table of the side that allocated it. A `free_string` accepts a null `ptr` and does nothing for it.

| Field | Type | Description |
| --- | --- | --- |
| `ptr` | `uint8_t *` | First byte of the text, or null for an empty string that allocates nothing. |
| `len` | `size_t` | Length of the text in bytes. |
| `cap` | `size_t` | Allocation size in bytes, for the allocating side's `free_string`. The receiver does not read it. |

### `AriaOwnedBytes`

Bytes returned across the boundary. The receiver releases them with the `free_bytes` slot of the table of the side that allocated them. A `free_bytes` accepts a null `ptr` and does nothing for it.

| Field | Type | Description |
| --- | --- | --- |
| `ptr` | `uint8_t *` | First byte, or null for an empty buffer that allocates nothing. |
| `len` | `size_t` | Number of bytes. |
| `cap` | `size_t` | Allocation size in bytes, for the allocating side's `free_bytes`. The receiver does not read it. |

### `AriaResult`

Result of a call that returns text: a JSON document, a message, or nothing.

| Field | Type | Description |
| --- | --- | --- |
| `status` | `uint32_t` | An `ARIA_STATUS_*` value. |
| `value` | `AriaOwnedStr` | The result on [`ARIA_STATUS_OK`](#status-codes), empty when the call returns nothing, and the message on any other status. |

### `AriaBytesResult`

Result of a call that returns bytes: `storage_get`, `panel_view` and `overlay_scene`.

| Field | Type | Description |
| --- | --- | --- |
| `status` | `uint32_t` | An `ARIA_BYTES_*` value. |
| `bytes` | `AriaOwnedBytes` | The result on [`ARIA_BYTES_FOUND`](#bytes-status-codes), otherwise empty. |
| `error` | `AriaOwnedStr` | The message on [`ARIA_BYTES_ERR`](#bytes-status-codes) and [`ARIA_BYTES_PANIC`](#bytes-status-codes), otherwise empty. |

### `AriaInstanceResult`

Result of a call that creates an instance the host addresses in later calls.

| Field | Type | Description |
| --- | --- | --- |
| `status` | `uint32_t` | An `ARIA_INSTANCE_*` value. |
| `instance` | `uint64_t` | The new instance's id on [`ARIA_INSTANCE_OK`](#instance-status-codes), otherwise 0. The side that creates the instance picks the id, and the other side passes it back unchanged. |
| `error` | `AriaOwnedStr` | The message on any status but [`ARIA_INSTANCE_OK`](#instance-status-codes), otherwise empty. |

### `AriaAudioParam`

One parameter of an audio effect across one [`AriaAudioBlock`](#ariaaudioblock): a constant, or one value per frame when the parameter is automated.

| Field | Type | Description |
| --- | --- | --- |
| `constant` | `float` | The value for every frame of the block when `per_frame` is null. 0 when the parameter has no value. |
| `per_frame` | `const float *` | One value per frame, or null when the parameter is constant. |
| `per_frame_len` | `size_t` | Number of values at `per_frame`: the block's `frames`, or 0. |

### `AriaAudioBlock`

One stereo block `audio_process` processes in place.

| Field | Type | Description |
| --- | --- | --- |
| `left` | `float *` | `frames` samples of the left channel, read and overwritten in place. |
| `right` | `float *` | `frames` samples of the right channel, read and overwritten in place. |
| `frames` | `size_t` | Frames in the block. |
| `position` | `int64_t` | Timeline position of the block's first frame, in frames at `sample_rate`. Consecutive blocks continue each other until the next `audio_reset`. |
| `sample_rate` | `uint32_t` | Frames per second. |
| `has_sidechain` | `uint32_t` | 1 when `sidechain_left` and `sidechain_right` hold the key signal, otherwise 0. |
| `sidechain_left` | `const float *` | `frames` samples of the key signal's left channel, or null. A silent or muted key lane reads as zeros. |
| `sidechain_right` | `const float *` | `frames` samples of the key signal's right channel, or null. |

### `AriaEncoderConfig`

The export settings `encoder_open` receives.

| Field | Type | Description |
| --- | --- | --- |
| `width` | `uint32_t` | Frame width in pixels. |
| `height` | `uint32_t` | Frame height in pixels. |
| `fps_num` | `int64_t` | Frame rate numerator: `fps_num / fps_den` frames per second. |
| `fps_den` | `int32_t` | Frame rate denominator. |
| `video_bitrate_kbps` | `uint32_t` | Target video bitrate in kbit/s, or 0 when the export sets none. |
| `audio_bitrate_kbps` | `uint32_t` | Target audio bitrate in kbit/s. The host sends 0, for no target. |

### `AriaEncoderFrame`

One export frame `encoder_push_frame` receives.

| Field | Type | Description |
| --- | --- | --- |
| `rgba` | `const uint8_t *` | Straight-alpha RGBA8 pixels, `height` rows of `stride` bytes, top row first. |
| `rgba_len` | `size_t` | Number of bytes at `rgba`. |
| `width` | `uint32_t` | Frame width in pixels. |
| `height` | `uint32_t` | Frame height in pixels. |
| `stride` | `uint32_t` | Bytes per row, at least `width * 4`. |
| `pts_value` | `int64_t` | Presentation time in ticks of `pts_rate` per second. |
| `pts_rate` | `int32_t` | Ticks per second of `pts_value`. |

### `AriaEncoderAudio`

One block of export audio `encoder_push_audio` receives.

| Field | Type | Description |
| --- | --- | --- |
| `samples` | `const float *` | Interleaved samples, `channels` per frame. |
| `samples_len` | `size_t` | Number of samples at `samples`, a multiple of `channels`. |
| `sample_rate` | `uint32_t` | Frames per second. |
| `channels` | `uint32_t` | Channels per frame. |
| `pts_value` | `int64_t` | Presentation time of the first frame, in ticks of `pts_rate` per second. |
| `pts_rate` | `int32_t` | Ticks per second of `pts_value`. |

### `AriaJobStep`

Progress a plugin reports from `job_step`. The host sets `progress` to -1 and `done` and `commit` to 0 before each call. The `AriaResult` value of `job_step` is the detail line.

| Field | Type | Description |
| --- | --- | --- |
| `progress` | `float` | Fraction done, from 0 to 1. A value below 0, or NaN, reports progress as unknown, and a value above 1 reads as 1. |
| `done` | `uint32_t` | 1 when the job has finished, otherwise 0. |
| `commit` | `uint32_t` | 1 when the step starts the job's commit, after which Cancel no longer stops it. |

### `AriaVideoInfo`

What `decoder_open_video` reports about the stream it opened. The host fills the struct with zeros before the call and refuses a stream whose values break the rules below.

| Field | Type | Description |
| --- | --- | --- |
| `width` | `uint32_t` | Frame width in pixels: even, from 2 to 16384. |
| `height` | `uint32_t` | Frame height in pixels: even, from 2 to 16384. |
| `format` | `uint32_t` | An `ARIA_FORMAT_*` value. |
| `color` | `uint32_t` | An `ARIA_COLOR_*` value. |
| `frame_rate_num` | `int32_t` | Frame rate numerator: `frame_rate_num / frame_rate_den` frames per second, positive. |
| `frame_rate_den` | `int32_t` | Frame rate denominator, positive. |
| `time_base_num` | `int32_t` | Time base numerator: every pts the stream reports counts ticks at `time_base_num / time_base_den` ticks per second, positive. |
| `time_base_den` | `int32_t` | Time base denominator, positive. |
| `duration` | `int64_t` | Length of the stream in time base ticks, or negative when unknown. |

### `AriaFramePlane`

One plane the host allocates for `decoder_next_frame` to fill.

| Field | Type | Description |
| --- | --- | --- |
| `data` | `uint8_t *` | `len` writable bytes, zeroed. |
| `len` | `size_t` | Number of bytes at `data`: `stride * rows`. |
| `stride` | `uint32_t` | Bytes per row: the frame width, or half of it for the U and V planes of [`ARIA_FORMAT_YUV420P`](#pixel-formats). |
| `rows` | `uint32_t` | Rows: the frame height for the Y plane, half of it for the others. |

### `AriaVideoFrameOut`

The frame `decoder_next_frame` writes. The plugin fills the planes, sets `pts`, and sets `produced` to 1, or leaves `produced` at 0 at end of stream.

| Field | Type | Description |
| --- | --- | --- |
| `planes` | `AriaFramePlane *` | The planes of the stream's format, in plane order: Y then UV for [`ARIA_FORMAT_NV12`](#pixel-formats), Y then U then V for [`ARIA_FORMAT_YUV420P`](#pixel-formats). |
| `planes_len` | `size_t` | Number of planes at `planes`. |
| `pts` | `int64_t` | Presentation time of the frame in time base ticks. The host sets 0. |
| `produced` | `uint32_t` | 1 when the plugin wrote a frame, 0 at end of stream. The host sets 0. |

### `AriaScopeCall`

The frame `scope_measure` reads and the image it draws.

`frame` holds `frame_width * frame_height` RGBA8 pixels in `frame_len` bytes: sRGB, straight alpha, top row first, no row padding. `image` holds `image_width * image_height` pixels of the same layout in `image_len` bytes, transparent black on entry, at the size the scope registered.

| Field | Type | Description |
| --- | --- | --- |
| `frame` | `const uint8_t *` | Pixels of the measured frame. |
| `frame_len` | `size_t` | Number of bytes at `frame`. |
| `frame_width` | `uint32_t` | Width of the measured frame in pixels. |
| `frame_height` | `uint32_t` | Height of the measured frame in pixels. |
| `image` | `uint8_t *` | Pixels of the scope image, which the plugin overwrites. |
| `image_len` | `size_t` | Number of bytes at `image`. The plugin cannot resize the image. |
| `image_width` | `uint32_t` | Width of the scope image in pixels, as registered. |
| `image_height` | `uint32_t` | Height of the scope image in pixels, as registered. |

### `AriaCpuEffectCall`

The frame and parameters `cpu_effect_process` reads and the output frame it writes.

`input` and `output` each hold `width * height` premultiplied RGBA16F pixels in `pixels_len` 16-bit channel elements (`width * height * 4`), each an IEEE-754 binary16 value, top row first, with no row padding. `params` holds `params_len` flattened 32-bit float values in catalog order (1 per scalar, 2 per vec2, 4 per colour with each channel from 0 to 1).

| Field | Type | Description |
| --- | --- | --- |
| `input` | `const uint16_t *` | The source frame, or a transparent frame for a generator. |
| `output` | `uint16_t *` | The frame the plugin writes, zeroed on entry. The host reads it only when the call returns [`ARIA_STATUS_OK`](#status-codes). |
| `pixels_len` | `size_t` | Number of 16-bit elements at `input` and at `output`. |
| `width` | `uint32_t` | Frame width in pixels. |
| `height` | `uint32_t` | Frame height in pixels. |
| `params` | `const float *` | Flattened parameter values. |
| `params_len` | `size_t` | Number of values at `params`. |
| `params_json` | `AriaStr` | The effect instance being rendered, serialized as JSON, or empty when there is none. |
| `time` | `float` | Seconds since the clip's first frame. |

### `AriaParamValue`

One parameter value passed to or returned from `driver_evaluate`. `kind` selects the fields that hold it; the others are 0.

| Field | Type | Description |
| --- | --- | --- |
| `kind` | `uint32_t` | An `ARIA_PARAM_*` value. |
| `x` | `float` | First component of a scalar, vector or rect. |
| `y` | `float` | Second component of a vector or rect. |
| `z` | `float` | Third component of a 3-d vector, or the width of a rect. |
| `w` | `float` | Height of a rect. |
| `r` | `uint8_t` | Red channel of a colour. |
| `g` | `uint8_t` | Green channel of a colour. |
| `b` | `uint8_t` | Blue channel of a colour. |
| `a` | `uint8_t` | Alpha channel of a colour. |

### `AriaDriverEvalCall`

What `driver_evaluate` reads and the value it writes. Times are sequence seconds.

| Field | Type | Description |
| --- | --- | --- |
| `time` | `double` | The time being evaluated. |
| `clip_time` | `double` | Seconds since the first frame of the driver's clip. |
| `frame_rate` | `double` | Frames per second of the sequence. |
| `duration` | `double` | Length of the sequence in seconds. |
| `canvas_width` | `uint32_t` | Canvas width in pixels. |
| `canvas_height` | `uint32_t` | Canvas height in pixels. |
| `value` | `AriaParamValue` | The property's own keyframed value at `time`. |
| `seed` | `uint64_t` | A number fixed for this clip and property, for seeded noise. |
| `output` | `AriaParamValue` | The value the plugin writes. The host sets it to `value` before the call, and its `kind` must stay the same. |

### `AriaPluginEntryFn`

```c
typedef const AriaPluginV1 *(*AriaPluginEntryFn)(const AriaHostV1 *host, uint32_t api_major);
const AriaPluginV1 *aria_plugin_entry(const AriaHostV1 *host, uint32_t api_major);
```

The entry point a native plugin exports under [`ARIA_PLUGIN_ENTRY_SYMBOL`](#version-and-entry). `host` is the host's table and `api_major` is [`ARIA_API_MAJOR`](#version-and-entry). Returns the plugin's table, or null to decline the load.

### `AriaTableHeader`

The fields both tables start with, which a receiver reads before any slot.

| Field | Type | Description |
| --- | --- | --- |
| `struct_size` | `size_t` | Byte size of the whole table. |
| `api_major` | `uint32_t` | [`ARIA_API_MAJOR`](#version-and-entry) of the table's sender. |
| `reserved` | `uint32_t` | 0. |

## `AriaHostV1`

The calls a host makes available to a plugin. The host passes it to `aria_plugin_entry`, and it stays valid while the library is loaded.

A `register_*` call and `subscribe` succeed only while `mount` runs. Every other call works in any callback. A call returns [`ARIA_STATUS_ERR`](#status-codes) with the reason when it fails, including when `host_ctx` is null or an [`AriaStr`](#ariastr) argument is not UTF-8.

| Field | Type | Description |
| --- | --- | --- |
| `struct_size` | `size_t` | Byte size of this table. |
| `api_major` | `uint32_t` | [`ARIA_API_MAJOR`](#version-and-entry). |
| `reserved` | `uint32_t` | 0. |

### `AriaHostV1` prefixes

A table holds the slots of a prefix when its `struct_size` is at least the prefix's size.

| Prefix | Slots |
| --- | --- |
| `ARIA_HOST_V1_MIN_SIZE` | [`free_string`](#ariahostv1free_string), [`free_bytes`](#ariahostv1free_bytes), [`log`](#ariahostv1log), [`storage_get`](#ariahostv1storage_get), [`storage_set`](#ariahostv1storage_set), [`storage_delete`](#ariahostv1storage_delete), [`storage_keys`](#ariahostv1storage_keys), [`setting_get`](#ariahostv1setting_get), [`register_command`](#ariahostv1register_command), [`subscribe`](#ariahostv1subscribe), [`register_importer`](#ariahostv1register_importer), [`register_exporter`](#ariahostv1register_exporter), [`register_audio_effect`](#ariahostv1register_audio_effect), [`project_snapshot`](#ariahostv1project_snapshot), [`editor_view`](#ariahostv1editor_view), [`project_transact`](#ariahostv1project_transact), [`project_undo`](#ariahostv1project_undo) |
| `ARIA_HOST_V1_PANELS_SIZE` | [`register_panel`](#ariahostv1register_panel), [`refresh_panel`](#ariahostv1refresh_panel) |
| `ARIA_HOST_V1_ENCODERS_SIZE` | [`register_encoder`](#ariahostv1register_encoder) |
| `ARIA_HOST_V1_OVERLAYS_SIZE` | [`register_viewer_overlay`](#ariahostv1register_viewer_overlay), [`refresh_viewer_overlay`](#ariahostv1refresh_viewer_overlay) |
| `ARIA_HOST_V1_JOBS_SIZE` | [`start_job`](#ariahostv1start_job) |
| `ARIA_HOST_V1_DECODERS_SIZE` | [`register_decoder`](#ariahostv1register_decoder) |
| `ARIA_HOST_V1_AI_TOOLS_SIZE` | [`register_ai_tool`](#ariahostv1register_ai_tool) |
| `ARIA_HOST_V1_SCOPES_SIZE` | [`register_scope`](#ariahostv1register_scope) |
| `ARIA_HOST_V1_CONTRIBUTIONS_SIZE` | [`register_effect`](#ariahostv1register_effect), [`register_cpu_effect`](#ariahostv1register_cpu_effect), [`register_transition`](#ariahostv1register_transition), [`register_generator`](#ariahostv1register_generator), [`register_clip_type`](#ariahostv1register_clip_type), [`register_driver_language`](#ariahostv1register_driver_language), [`register_preset`](#ariahostv1register_preset), [`register_theme`](#ariahostv1register_theme), [`register_workspace`](#ariahostv1register_workspace), [`register_analyzer`](#ariahostv1register_analyzer) |
| `ARIA_HOST_V1_QUERIES_SIZE` | [`clip`](#ariahostv1clip), [`clips_in_range`](#ariahostv1clips_in_range), [`clips_at`](#ariahostv1clips_at), [`selected_clips`](#ariahostv1selected_clips), [`markers_in_range`](#ariahostv1markers_in_range), [`in_out`](#ariahostv1in_out) |
| `sizeof(AriaHostV1)` | [`register_texture`](#ariahostv1register_texture) |

### `AriaHostV1.free_string`

```c
void (*free_string)(AriaOwnedStr str);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Release an [`AriaOwnedStr`](#ariaownedstr) a host call returned.

### `AriaHostV1.free_bytes`

```c
void (*free_bytes)(AriaOwnedBytes bytes);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Release an [`AriaOwnedBytes`](#ariaownedbytes) a host call returned.

### `AriaHostV1.log`

```c
void (*log)(void *host_ctx, uint32_t level, AriaStr text);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Write `text` to the editor log under the plugin's id. `level` is an `ARIA_LOG_*` value. A null `host_ctx` or text that is not UTF-8 writes nothing.

### `AriaHostV1.storage_get`

```c
AriaBytesResult (*storage_get)(void *host_ctx, AriaStr key);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Read the value stored under `key` in the plugin's storage directory, `<data dir>/plugin-data/<plugin id>`. Returns [`ARIA_BYTES_FOUND`](#bytes-status-codes) with the bytes, or [`ARIA_BYTES_NOT_FOUND`](#bytes-status-codes). A key is one file name: not empty, not `.` or `..`, and without `/`, `\` or NUL.

### `AriaHostV1.storage_set`

```c
AriaResult (*storage_set)(void *host_ctx, AriaStr key, AriaBytes value);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Store `value` under `key`, replacing any earlier value. Keys follow the rules of `storage_get`.

### `AriaHostV1.storage_delete`

```c
AriaResult (*storage_delete)(void *host_ctx, AriaStr key);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Delete the value stored under `key`. Deleting a key with no value succeeds.

### `AriaHostV1.storage_keys`

```c
AriaResult (*storage_keys)(void *host_ctx);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

The stored keys as a sorted JSON array of strings.

### `AriaHostV1.setting_get`

```c
AriaResult (*setting_get)(void *host_ctx, AriaStr key);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

The current value of the setting `key` as JSON, or an empty value when the setting has none.

### `AriaHostV1.register_command`

```c
AriaResult (*register_command)(void *host_ctx, AriaStr id, AriaStr title);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Register the command `id` with the display name `title`. Running it calls [`AriaPluginV1.command_run`](#ariapluginv1command_run) with `id`.

### `AriaHostV1.subscribe`

```c
AriaResult (*subscribe)(void *host_ctx, AriaStr event_kind);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Deliver events of `event_kind` to [`AriaPluginV1.event`](#ariapluginv1event). `event_kind` is a kind from `docs/plugins/events.md`. `resolve-dropped:<n>` subscribes with the priority `n`, a 32-bit integer; a higher priority runs first, and the default is 0.

### `AriaHostV1.register_importer`

```c
AriaResult (*register_importer)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Register an importer. `spec_json` is an object with `id`, `name`, and `extensions`, an array of file extensions without the dot. Importing a file calls [`AriaPluginV1.import_file`](#ariapluginv1import_file).

### `AriaHostV1.register_exporter`

```c
AriaResult (*register_exporter)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Register an exporter. `spec_json` is an object with `id`, `name`, and `extension`, without the dot. Exporting calls [`AriaPluginV1.export_file`](#ariapluginv1export_file).

### `AriaHostV1.register_audio_effect`

```c
AriaResult (*register_audio_effect)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Register an audio effect. `spec_json` is an object with `id`, `label`, and the optional `category`, `latency` in sample frames at 48 kHz, `sidechain`, and `params`: an array of objects with `name`, `label`, `default`, `min`, and `max`, in the order [`AriaPluginV1.audio_process`](#ariapluginv1audio_process) receives them. Each use of the effect calls [`AriaPluginV1.audio_create`](#ariapluginv1audio_create).

### `AriaHostV1.project_snapshot`

```c
AriaResult (*project_snapshot)(void *host_ctx);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

The project as JSON. Requires `project = "read"` or `"read-write"` in the manifest's `[permissions]`.

### `AriaHostV1.editor_view`

```c
AriaResult (*editor_view)(void *host_ctx);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

The editor state as a JSON object with `selection` (clip ids), `playhead`, `mark_in`, `mark_out` (times or null), `playing`, and `speed`. Requires project read permission.

### `AriaHostV1.project_transact`

```c
AriaResult (*project_transact)(void *host_ctx, AriaStr label, AriaStr commands_json);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Apply `commands_json`, a JSON array of commands, as one undo step named `label`, or apply none of them. The value is an object with the new `revision` and the `outcomes` of the commands. Requires `project = "read-write"`.

### `AriaHostV1.project_undo`

```c
AriaResult (*project_undo)(void *host_ctx);
```

Prefix: [`ARIA_HOST_V1_MIN_SIZE`](#table-sizes).

Undo the last undo step. The value is `true`, or `false` when there was nothing to undo. Requires `project = "read-write"`.

### `AriaHostV1.register_panel`

```c
AriaResult (*register_panel)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_PANELS_SIZE`](#table-sizes).

Register a panel. `spec_json` is an object with `id`, `title`, and `location`: `sidebar`, `inspector`, or `timeline`. Any other location is refused. Drawing it calls [`AriaPluginV1.panel_view`](#ariapluginv1panel_view). Fails unless the plugin table has the panel slots.

### `AriaHostV1.refresh_panel`

```c
AriaResult (*refresh_panel)(void *host_ctx, AriaStr id);
```

Prefix: [`ARIA_HOST_V1_PANELS_SIZE`](#table-sizes).

Redraw the panel `id`, which calls [`AriaPluginV1.panel_view`](#ariapluginv1panel_view) again.

### `AriaHostV1.register_encoder`

```c
AriaResult (*register_encoder)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_ENCODERS_SIZE`](#table-sizes).

Register an export encoder. `spec_json` is an object with `id`, `name`, `extension`, and the optional `audio`, true when the encoder takes audio. Exporting with it calls [`AriaPluginV1.encoder_open`](#ariapluginv1encoder_open). Fails unless the plugin table has the encoder slots.

### `AriaHostV1.register_viewer_overlay`

```c
AriaResult (*register_viewer_overlay)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_OVERLAYS_SIZE`](#table-sizes).

Register a viewer overlay. `spec_json` is an object with `id` and `title`. Drawing it calls [`AriaPluginV1.overlay_scene`](#ariapluginv1overlay_scene). Fails unless the plugin table has the overlay slots.

### `AriaHostV1.refresh_viewer_overlay`

```c
AriaResult (*refresh_viewer_overlay)(void *host_ctx, AriaStr id);
```

Prefix: [`ARIA_HOST_V1_OVERLAYS_SIZE`](#table-sizes).

Redraw the viewer overlay `id`, which calls [`AriaPluginV1.overlay_scene`](#ariapluginv1overlay_scene) again.

### `AriaHostV1.start_job`

```c
AriaInstanceResult (*start_job)(void *host_ctx, AriaStr label);
```

Prefix: [`ARIA_HOST_V1_JOBS_SIZE`](#table-sizes).

Start a background job named `label` on the Jobs board. The host calls [`AriaPluginV1.job_step`](#ariapluginv1job_step) on the job thread until a step reports `done`, a step fails, or Cancel is accepted, then [`AriaPluginV1.job_end`](#ariapluginv1job_end). The instance is the job id both receive. The first step can run before `start_job` returns. Fails unless the plugin table has the job slots.

### `AriaHostV1.register_decoder`

```c
AriaResult (*register_decoder)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_DECODERS_SIZE`](#table-sizes).

Register a media decoder. `spec_json` is an object with `id`, `name`, `extensions`, and the optional `priority`; a higher priority is tried first. Fails unless the plugin table has the decoder slots.

### `AriaHostV1.register_ai_tool`

```c
AriaResult (*register_ai_tool)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_AI_TOOLS_SIZE`](#table-sizes).

Register an assistant tool. `spec_json` is an object with `name`, `description`, `parameters` (a JSON Schema), and `tier`: `read-only`, `workspace`, or `system`. A call to the tool calls [`AriaPluginV1.ai_tool_call`](#ariapluginv1ai_tool_call). Fails unless the plugin table has the AI tool slot.

### `AriaHostV1.register_scope`

```c
AriaResult (*register_scope)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_SCOPES_SIZE`](#table-sizes).

Register a scope. `spec_json` is an object with `id`, `name`, and the `width` and `height` of its image in pixels. Drawing it calls [`AriaPluginV1.scope_measure`](#ariapluginv1scope_measure). Fails unless the plugin table has the scope slot.

### `AriaHostV1.register_effect`

```c
AriaResult (*register_effect)(void *host_ctx, AriaStr spec_json, AriaStr wgsl);
```

Prefix: [`ARIA_HOST_V1_CONTRIBUTIONS_SIZE`](#table-sizes).

Register a GPU effect. `spec_json` is an effect spec object, as in `docs/plugins/effects.md`. `wgsl` is WGSL source, a JSON array of WGSL strings for a chain of fragment passes, or a JSON array of graph pass objects.

### `AriaHostV1.register_cpu_effect`

```c
AriaResult (*register_cpu_effect)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_CONTRIBUTIONS_SIZE`](#table-sizes).

Register an effect that [`AriaPluginV1.cpu_effect_process`](#ariapluginv1cpu_effect_process) draws. `spec_json` is an effect spec object. Fails unless the plugin table has the contribution slots.

### `AriaHostV1.register_transition`

```c
AriaResult (*register_transition)(void *host_ctx, AriaStr spec_json, AriaStr wgsl);
```

Prefix: [`ARIA_HOST_V1_CONTRIBUTIONS_SIZE`](#table-sizes).

Register a transition. `spec_json` is a transition spec object and `wgsl` its WGSL source.

### `AriaHostV1.register_generator`

```c
AriaResult (*register_generator)(void *host_ctx, AriaStr spec_json, AriaStr wgsl);
```

Prefix: [`ARIA_HOST_V1_CONTRIBUTIONS_SIZE`](#table-sizes).

Register a generator. `spec_json` is an effect spec object, and `wgsl` takes the forms of `register_effect`. An empty `wgsl` registers a generator that [`AriaPluginV1.cpu_effect_process`](#ariapluginv1cpu_effect_process) draws, which requires the contribution slots.

### `AriaHostV1.register_clip_type`

```c
AriaResult (*register_clip_type)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_CONTRIBUTIONS_SIZE`](#table-sizes).

Register a clip type. `spec_json` is an object with `id`, `label`, `colour`, and the optional `generator`, `icon_svg`, `handles`, and `schema`, as in `docs/plugins/clip-types.md`.

### `AriaHostV1.register_driver_language`

```c
AriaResult (*register_driver_language)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_CONTRIBUTIONS_SIZE`](#table-sizes).

Register a driver language, whose expressions [`AriaPluginV1.driver_evaluate`](#ariapluginv1driver_evaluate) evaluates. `spec_json` is a driver language spec object, as in `docs/plugins/drivers.md`. Fails unless the plugin table has the contribution slots.

### `AriaHostV1.register_preset`

```c
AriaResult (*register_preset)(void *host_ctx, AriaStr preset_json);
```

Prefix: [`ARIA_HOST_V1_CONTRIBUTIONS_SIZE`](#table-sizes).

Register a preset. `preset_json` is a preset object, the JSON form of an `.ariapreset` file.

### `AriaHostV1.register_theme`

```c
AriaResult (*register_theme)(void *host_ctx, AriaStr id, AriaStr toml);
```

Prefix: [`ARIA_HOST_V1_CONTRIBUTIONS_SIZE`](#table-sizes).

Register the theme `id` from `toml`, the text of a theme file.

### `AriaHostV1.register_workspace`

```c
AriaResult (*register_workspace)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_CONTRIBUTIONS_SIZE`](#table-sizes).

Register a workspace. `spec_json` is a workspace spec object, as in `docs/plugins/workspaces.md`.

### `AriaHostV1.register_analyzer`

```c
AriaResult (*register_analyzer)(void *host_ctx, AriaStr spec_json);
```

Prefix: [`ARIA_HOST_V1_CONTRIBUTIONS_SIZE`](#table-sizes).

Register an audio analyzer, which runs through [`AriaPluginV1.analyzer_open`](#ariapluginv1analyzer_open). `spec_json` is an analyzer spec object, as in `docs/plugins/analyzers.md`. Fails unless the plugin table has the contribution slots.

### `AriaHostV1.clip`

```c
AriaResult (*clip)(void *host_ctx, uint64_t id);
```

Prefix: [`ARIA_HOST_V1_QUERIES_SIZE`](#table-sizes).

The clip `id` as a JSON object with `id`, `track`, `start`, `end`, `kind`, and `name`, from the open sequence, the root timeline, or a nested sequence. An empty value when no clip has the id. Requires project read permission.

### `AriaHostV1.clips_in_range`

```c
AriaResult (*clips_in_range)(void *host_ctx, uint64_t sequence_id, AriaStr range_json);
```

Prefix: [`ARIA_HOST_V1_QUERIES_SIZE`](#table-sizes).

The clips of `sequence_id` that overlap the half-open range `range_json`, a JSON object with `start` and `duration` times, as a JSON array in track order, then time order. A `sequence_id` of 0 reads the open sequence, and an unknown id returns an empty array. Requires project read permission.

### `AriaHostV1.clips_at`

```c
AriaResult (*clips_at)(void *host_ctx, uint64_t sequence_id, AriaStr tick_json);
```

Prefix: [`ARIA_HOST_V1_QUERIES_SIZE`](#table-sizes).

The clips of `sequence_id` with `start <= tick < end`, where `tick_json` is a time object with `value` and `rate`, as a JSON array in track order, then time order. `sequence_id` reads as in `clips_in_range`. Requires project read permission.

### `AriaHostV1.selected_clips`

```c
AriaResult (*selected_clips)(void *host_ctx);
```

Prefix: [`ARIA_HOST_V1_QUERIES_SIZE`](#table-sizes).

The selected clips as a JSON array, in selection order. Requires project read permission.

### `AriaHostV1.markers_in_range`

```c
AriaResult (*markers_in_range)(void *host_ctx, uint64_t sequence_id, AriaStr range_json);
```

Prefix: [`ARIA_HOST_V1_QUERIES_SIZE`](#table-sizes).

The markers of `sequence_id` whose point or span overlaps the half-open range `range_json`, as a JSON array ordered by time. `sequence_id` and `range_json` read as in `clips_in_range`. Requires project read permission.

### `AriaHostV1.in_out`

```c
AriaResult (*in_out)(void *host_ctx);
```

Prefix: [`ARIA_HOST_V1_QUERIES_SIZE`](#table-sizes).

The In and Out points as a JSON object with `mark_in` and `mark_out`, each a time or null. Requires project read permission.

### `AriaHostV1.register_texture`

```c
AriaResult (*register_texture)(void *host_ctx, AriaStr id, uint32_t width, uint32_t height, AriaBytes pixels);
```

Prefix: `sizeof(AriaHostV1)`.

Register `width * height * 4` straight-alpha RGBA8 `pixels` as the plugin texture `id`, which an effect pass reads as an input.

## `AriaPluginV1`

The callbacks a plugin exports. `aria_plugin_entry` returns it, and it stays valid while the library is loaded.

The host calls `mount` once, before any other callback. A callback that has no result returns [`ARIA_STATUS_OK`](#status-codes) with an empty `value`.

| Field | Type | Description |
| --- | --- | --- |
| `struct_size` | `size_t` | Byte size of this table, at least [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes). |
| `api_major` | `uint32_t` | [`ARIA_API_MAJOR`](#version-and-entry). |
| `reserved` | `uint32_t` | 0. |

### `AriaPluginV1` prefixes

A table holds the slots of a prefix when its `struct_size` is at least the prefix's size.

| Prefix | Slots |
| --- | --- |
| `ARIA_PLUGIN_V1_MIN_SIZE` | [`free_string`](#ariapluginv1free_string), [`free_bytes`](#ariapluginv1free_bytes), [`mount`](#ariapluginv1mount), [`unmount`](#ariapluginv1unmount), [`command_run`](#ariapluginv1command_run), [`event`](#ariapluginv1event), [`import_file`](#ariapluginv1import_file), [`export_file`](#ariapluginv1export_file), [`audio_create`](#ariapluginv1audio_create), [`audio_process`](#ariapluginv1audio_process), [`audio_reset`](#ariapluginv1audio_reset), [`audio_drop`](#ariapluginv1audio_drop) |
| `ARIA_PLUGIN_V1_PANELS_SIZE` | [`panel_view`](#ariapluginv1panel_view), [`panel_event`](#ariapluginv1panel_event) |
| `ARIA_PLUGIN_V1_ENCODERS_SIZE` | [`encoder_open`](#ariapluginv1encoder_open), [`encoder_push_frame`](#ariapluginv1encoder_push_frame), [`encoder_push_audio`](#ariapluginv1encoder_push_audio), [`encoder_finish`](#ariapluginv1encoder_finish), [`encoder_drop`](#ariapluginv1encoder_drop) |
| `ARIA_PLUGIN_V1_OVERLAYS_SIZE` | [`overlay_scene`](#ariapluginv1overlay_scene), [`overlay_event`](#ariapluginv1overlay_event) |
| `ARIA_PLUGIN_V1_JOBS_SIZE` | [`job_step`](#ariapluginv1job_step), [`job_end`](#ariapluginv1job_end) |
| `ARIA_PLUGIN_V1_DECODERS_SIZE` | [`decoder_probe`](#ariapluginv1decoder_probe), [`decoder_open_video`](#ariapluginv1decoder_open_video), [`decoder_seek`](#ariapluginv1decoder_seek), [`decoder_next_frame`](#ariapluginv1decoder_next_frame), [`decoder_open_audio`](#ariapluginv1decoder_open_audio), [`decoder_read_audio`](#ariapluginv1decoder_read_audio), [`decoder_seek_audio`](#ariapluginv1decoder_seek_audio), [`decoder_close`](#ariapluginv1decoder_close) |
| `ARIA_PLUGIN_V1_AI_TOOLS_SIZE` | [`ai_tool_call`](#ariapluginv1ai_tool_call) |
| `ARIA_PLUGIN_V1_SCOPES_SIZE` | [`scope_measure`](#ariapluginv1scope_measure) |
| `sizeof(AriaPluginV1)` | [`cpu_effect_process`](#ariapluginv1cpu_effect_process), [`driver_evaluate`](#ariapluginv1driver_evaluate), [`analyzer_open`](#ariapluginv1analyzer_open), [`analyzer_push`](#ariapluginv1analyzer_push), [`analyzer_finish`](#ariapluginv1analyzer_finish), [`analyzer_drop`](#ariapluginv1analyzer_drop) |

### `AriaPluginV1.free_string`

```c
void (*free_string)(AriaOwnedStr str);
```

Prefix: [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes).

Release an [`AriaOwnedStr`](#ariaownedstr) a callback returned.

### `AriaPluginV1.free_bytes`

```c
void (*free_bytes)(AriaOwnedBytes bytes);
```

Prefix: [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes).

Release an [`AriaOwnedBytes`](#ariaownedbytes) a callback returned.

### `AriaPluginV1.mount`

```c
AriaResult (*mount)(void *host_ctx);
```

Prefix: [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes).

Register the plugin's contributions with the `register_*` host calls and `subscribe`. [`ARIA_STATUS_ERR`](#status-codes) fails the load with the message.

### `AriaPluginV1.unmount`

```c
AriaResult (*unmount)(void *host_ctx);
```

Prefix: [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes).

Release the plugin's state before the library unloads. Called once, after a `mount` that succeeded, unless the plugin has failed.

### `AriaPluginV1.command_run`

```c
AriaResult (*command_run)(void *host_ctx, AriaStr id, AriaStr args_json);
```

Prefix: [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes).

Run the command `id`, which the plugin registered with [`AriaHostV1.register_command`](#ariahostv1register_command) or declared in `plugin.toml`. `args_json` is a JSON object, `{}` for a command run from the editor. [`ARIA_STATUS_ERR`](#status-codes) reports the message as the command's failure.

### `AriaPluginV1.event`

```c
AriaResult (*event)(void *host_ctx, AriaStr kind, AriaStr event_json);
```

Prefix: [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes).

Handle an event of the subscribed `kind`. `event_json` is its payload and the value is the reply, or empty for none, as `docs/plugins/events.md` lists them. [`ARIA_STATUS_ERR`](#status-codes) is logged and counts as no reply.

### `AriaPluginV1.import_file`

```c
AriaResult (*import_file)(void *host_ctx, AriaStr importer_id, AriaStr path);
```

Prefix: [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes).

Import the file at `path` with the importer `importer_id`. The value is a JSON object with the optional `commands_json`, a string holding a JSON array of commands the host applies as one undo step, and `skipped`, an array of strings naming what the import left out. [`ARIA_STATUS_ERR`](#status-codes) cancels the import.

### `AriaPluginV1.export_file`

```c
AriaResult (*export_file)(void *host_ctx, AriaStr exporter_id, AriaStr project_json, AriaStr path);
```

Prefix: [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes).

Write `project_json`, the project as JSON, with the exporter `exporter_id` to the file `path`, a staging file in the plugin's storage directory that the host copies to the destination. The value is empty or a JSON object with `skipped`, an array of strings naming what the file leaves out.

### `AriaPluginV1.audio_create`

```c
AriaInstanceResult (*audio_create)(void *host_ctx, AriaStr effect_id, uint32_t sample_rate);
```

Prefix: [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes).

Create an instance of the audio effect `effect_id` at `sample_rate` frames per second, with fresh state. The host creates one per insert of the effect.

### `AriaPluginV1.audio_process`

```c
AriaResult (*audio_process)(void *host_ctx, uint64_t instance, const AriaAudioParam *params, size_t params_len, AriaAudioBlock *block);
```

Prefix: [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes).

Process `block` in place. `params` holds `params_len` values, one per parameter of the effect's spec, in spec order.

### `AriaPluginV1.audio_reset`

```c
AriaResult (*audio_reset)(void *host_ctx, uint64_t instance);
```

Prefix: [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes).

Clear every value carried over from earlier blocks, such as delay lines and envelopes. Called before a block that does not continue the last one, such as after a seek.

### `AriaPluginV1.audio_drop`

```c
AriaResult (*audio_drop)(void *host_ctx, uint64_t instance);
```

Prefix: [`ARIA_PLUGIN_V1_MIN_SIZE`](#table-sizes).

Release an instance `audio_create` returned.

### `AriaPluginV1.panel_view`

```c
AriaBytesResult (*panel_view)(void *host_ctx, AriaStr panel_id);
```

Prefix: [`ARIA_PLUGIN_V1_PANELS_SIZE`](#table-sizes).

The contents of the panel `panel_id`: [`ARIA_BYTES_FOUND`](#bytes-status-codes) with the UI tree as JSON, as in `docs/plugins/panels.md`, at most 257 MiB. Any other result shows the error in the panel and records a fault.

### `AriaPluginV1.panel_event`

```c
AriaResult (*panel_event)(void *host_ctx, AriaStr panel_id, AriaStr event_json);
```

Prefix: [`ARIA_PLUGIN_V1_PANELS_SIZE`](#table-sizes).

Handle `event_json`, a control event in the panel `panel_id`.

### `AriaPluginV1.encoder_open`

```c
AriaInstanceResult (*encoder_open)(void *host_ctx, AriaStr encoder_id, AriaStr path, const AriaEncoderConfig *config);
```

Prefix: [`ARIA_PLUGIN_V1_ENCODERS_SIZE`](#table-sizes).

Start writing the file `path` with the encoder `encoder_id` and the settings `config`. The instance receives the export's frames and audio.

### `AriaPluginV1.encoder_push_frame`

```c
AriaResult (*encoder_push_frame)(void *host_ctx, uint64_t instance, const AriaEncoderFrame *frame);
```

Prefix: [`ARIA_PLUGIN_V1_ENCODERS_SIZE`](#table-sizes).

Encode the next frame of the export.

### `AriaPluginV1.encoder_push_audio`

```c
AriaResult (*encoder_push_audio)(void *host_ctx, uint64_t instance, const AriaEncoderAudio *audio);
```

Prefix: [`ARIA_PLUGIN_V1_ENCODERS_SIZE`](#table-sizes).

Encode the next block of the export's audio.

### `AriaPluginV1.encoder_finish`

```c
AriaResult (*encoder_finish)(void *host_ctx, uint64_t instance);
```

Prefix: [`ARIA_PLUGIN_V1_ENCODERS_SIZE`](#table-sizes).

Complete the file after the last frame. The host makes no further call on the instance.

### `AriaPluginV1.encoder_drop`

```c
AriaResult (*encoder_drop)(void *host_ctx, uint64_t instance);
```

Prefix: [`ARIA_PLUGIN_V1_ENCODERS_SIZE`](#table-sizes).

Abandon an export that stopped before `encoder_finish`, and release the instance.

### `AriaPluginV1.overlay_scene`

```c
AriaBytesResult (*overlay_scene)(void *host_ctx, AriaStr overlay_id);
```

Prefix: [`ARIA_PLUGIN_V1_OVERLAYS_SIZE`](#table-sizes).

The scene of the viewer overlay `overlay_id`: [`ARIA_BYTES_FOUND`](#bytes-status-codes) with the scene as JSON, as in `docs/plugins/viewer-overlays.md`, at most 4 MiB. Any other result refuses the scene with the error.

### `AriaPluginV1.overlay_event`

```c
AriaResult (*overlay_event)(void *host_ctx, AriaStr overlay_id, AriaStr event_json);
```

Prefix: [`ARIA_PLUGIN_V1_OVERLAYS_SIZE`](#table-sizes).

Handle `event_json`, a pointer event on a handle of the overlay `overlay_id`.

### `AriaPluginV1.job_step`

```c
AriaResult (*job_step)(void *host_ctx, uint64_t job, AriaJobStep *step);
```

Prefix: [`ARIA_PLUGIN_V1_JOBS_SIZE`](#table-sizes).

Run one bounded step of the job `job` and write its progress to `step`. The value is the detail line. [`ARIA_STATUS_ERR`](#status-codes) ends the job as failed.

### `AriaPluginV1.job_end`

```c
AriaResult (*job_end)(void *host_ctx, uint64_t job, uint32_t cancelled);
```

Prefix: [`ARIA_PLUGIN_V1_JOBS_SIZE`](#table-sizes).

The job `job` has ended. `cancelled` is 1 when Cancel ended it, otherwise 0.

### `AriaPluginV1.decoder_probe`

```c
AriaResult (*decoder_probe)(void *host_ctx, AriaStr decoder_id, AriaStr path);
```

Prefix: [`ARIA_PLUGIN_V1_DECODERS_SIZE`](#table-sizes).

Describe the file at `path` for the decoder `decoder_id`. The value is a JSON object with `frame_rate_num` and `frame_rate_den`, and the optional `width`, `height`, `frame_count`, `has_audio`, `is_image`, `video_codec`, and `audio_codec`. [`ARIA_STATUS_UNSUPPORTED`](#status-codes) passes the file to the next decoder.

### `AriaPluginV1.decoder_open_video`

```c
AriaInstanceResult (*decoder_open_video)(void *host_ctx, AriaStr decoder_id, AriaStr path, AriaVideoInfo *info);
```

Prefix: [`ARIA_PLUGIN_V1_DECODERS_SIZE`](#table-sizes).

Open the video stream of `path` and write its description to `info`. [`ARIA_INSTANCE_UNSUPPORTED`](#instance-status-codes) passes the file to the next decoder.

### `AriaPluginV1.decoder_seek`

```c
AriaResult (*decoder_seek)(void *host_ctx, uint64_t instance, int64_t pts);
```

Prefix: [`ARIA_PLUGIN_V1_DECODERS_SIZE`](#table-sizes).

Position the video instance at the sync frame at or before `pts`, in time base ticks.

### `AriaPluginV1.decoder_next_frame`

```c
AriaResult (*decoder_next_frame)(void *host_ctx, uint64_t instance, AriaVideoFrameOut *frame);
```

Prefix: [`ARIA_PLUGIN_V1_DECODERS_SIZE`](#table-sizes).

Decode the next frame of the video instance into `frame`.

### `AriaPluginV1.decoder_open_audio`

```c
AriaInstanceResult (*decoder_open_audio)(void *host_ctx, AriaStr decoder_id, AriaStr path, uint32_t sample_rate, uint32_t channels);
```

Prefix: [`ARIA_PLUGIN_V1_DECODERS_SIZE`](#table-sizes).

Open the audio of `path`, resampled to `sample_rate` frames per second and `channels` channels. [`ARIA_INSTANCE_UNSUPPORTED`](#instance-status-codes) passes the file to the next decoder.

### `AriaPluginV1.decoder_read_audio`

```c
AriaResult (*decoder_read_audio)(void *host_ctx, uint64_t instance, float *out, size_t out_len, size_t *written);
```

Prefix: [`ARIA_PLUGIN_V1_DECODERS_SIZE`](#table-sizes).

Write up to `out_len` interleaved samples to `out`, and their count to `*written`: whole frames, and fewer than `out_len` only at end of stream.

### `AriaPluginV1.decoder_seek_audio`

```c
AriaResult (*decoder_seek_audio)(void *host_ctx, uint64_t instance, int64_t frame);
```

Prefix: [`ARIA_PLUGIN_V1_DECODERS_SIZE`](#table-sizes).

Position the next read of the audio instance at output frame `frame`.

### `AriaPluginV1.decoder_close`

```c
AriaResult (*decoder_close)(void *host_ctx, uint64_t instance);
```

Prefix: [`ARIA_PLUGIN_V1_DECODERS_SIZE`](#table-sizes).

Release an instance `decoder_open_video` or `decoder_open_audio` returned.

### `AriaPluginV1.ai_tool_call`

```c
AriaResult (*ai_tool_call)(void *host_ctx, AriaStr name, AriaStr arguments_json);
```

Prefix: [`ARIA_PLUGIN_V1_AI_TOOLS_SIZE`](#table-sizes).

Run the assistant tool `name`, as its spec registered it, with `arguments_json`, an object that matches its `parameters` schema. The value is a JSON object with the optional `text` and `images`, an array of objects with `label` and `png`, the PNG file as an array of bytes.

### `AriaPluginV1.scope_measure`

```c
AriaResult (*scope_measure)(void *host_ctx, AriaStr scope_id, AriaScopeCall *call);
```

Prefix: [`ARIA_PLUGIN_V1_SCOPES_SIZE`](#table-sizes).

Measure the frame in `call` for the scope `scope_id` and draw the scope image. The value is the readout text shown with the scope.

### `AriaPluginV1.cpu_effect_process`

```c
AriaResult (*cpu_effect_process)(void *host_ctx, AriaStr effect_id, AriaCpuEffectCall *call);
```

Prefix: `sizeof(AriaPluginV1)`.

Draw one frame of the CPU effect, generator, or clip type `effect_id` into `call.output`.

### `AriaPluginV1.driver_evaluate`

```c
AriaResult (*driver_evaluate)(void *host_ctx, AriaStr language_id, AriaStr expr, AriaDriverEvalCall *call);
```

Prefix: `sizeof(AriaPluginV1)`.

Evaluate `expr`, a non-empty expression in the driver language `language_id`, and write the result to `call.output`. [`ARIA_STATUS_ERR`](#status-codes) or [`ARIA_STATUS_PANIC`](#status-codes) fails this evaluation with the message.

### `AriaPluginV1.analyzer_open`

```c
AriaInstanceResult (*analyzer_open)(void *host_ctx, AriaStr analyzer_id, uint32_t sample_rate, uint32_t channels);
```

Prefix: `sizeof(AriaPluginV1)`.

Start a run of the analyzer `analyzer_id` over audio at `sample_rate` frames per second with `channels` channels.

### `AriaPluginV1.analyzer_push`

```c
AriaResult (*analyzer_push)(void *host_ctx, uint64_t instance, const float *samples, size_t samples_len);
```

Prefix: `sizeof(AriaPluginV1)`.

Read the next `samples_len` interleaved samples of the run. [`ARIA_STATUS_ERR`](#status-codes) stops the run with the message.

### `AriaPluginV1.analyzer_finish`

```c
AriaResult (*analyzer_finish)(void *host_ctx, uint64_t instance);
```

Prefix: `sizeof(AriaPluginV1)`.

End the run after the last block. The value is the analyzer result as JSON, as in `docs/plugins/analyzers.md`. The host makes no further call on the instance.

### `AriaPluginV1.analyzer_drop`

```c
AriaResult (*analyzer_drop)(void *host_ctx, uint64_t instance);
```

Prefix: `sizeof(AriaPluginV1)`.

Release a run that ended before `analyzer_finish`.
