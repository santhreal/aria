#ifndef ARIA_PLUGIN_H
#define ARIA_PLUGIN_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define ARIA_API_MAJOR 1u

#define ARIA_STATUS_OK 0u
#define ARIA_STATUS_ERR 1u
#define ARIA_STATUS_PANIC 2u
#define ARIA_STATUS_UNSUPPORTED 3u

#define ARIA_BYTES_FOUND 0u
#define ARIA_BYTES_NOT_FOUND 1u
#define ARIA_BYTES_ERR 2u
#define ARIA_BYTES_PANIC 3u

#define ARIA_LOG_TRACE 0u
#define ARIA_LOG_DEBUG 1u
#define ARIA_LOG_INFO 2u
#define ARIA_LOG_WARN 3u
#define ARIA_LOG_ERROR 4u

#define ARIA_FORMAT_NV12 0u
#define ARIA_FORMAT_YUV420P 1u

#define ARIA_COLOR_BT709 0u
#define ARIA_COLOR_BT601 1u
#define ARIA_COLOR_BT2020_PQ 2u
#define ARIA_COLOR_SRGB 3u

#define ARIA_PARAM_SCALAR 0u
#define ARIA_PARAM_VEC2 1u
#define ARIA_PARAM_COLOR 2u
#define ARIA_PARAM_RECT 3u
#define ARIA_PARAM_VEC3 4u

typedef struct AriaStr {
    const uint8_t *ptr;
    size_t len;
} AriaStr;

typedef struct AriaBytes {
    const uint8_t *ptr;
    size_t len;
} AriaBytes;

typedef struct AriaOwnedStr {
    uint8_t *ptr;
    size_t len;
    size_t cap;
} AriaOwnedStr;

typedef struct AriaOwnedBytes {
    uint8_t *ptr;
    size_t len;
    size_t cap;
} AriaOwnedBytes;

typedef struct AriaResult {
    uint32_t status;
    AriaOwnedStr value;
} AriaResult;

typedef struct AriaBytesResult {
    uint32_t status;
    AriaOwnedBytes bytes;
    AriaOwnedStr error;
} AriaBytesResult;

typedef struct AriaInstanceResult {
    uint32_t status;
    uint64_t instance;
    AriaOwnedStr error;
} AriaInstanceResult;

typedef struct AriaAudioParam {
    float constant;
    const float *per_frame;
    size_t per_frame_len;
} AriaAudioParam;

typedef struct AriaAudioBlock {
    float *left;
    float *right;
    size_t frames;
    int64_t position;
    uint32_t sample_rate;
    uint32_t has_sidechain;
    const float *sidechain_left;
    const float *sidechain_right;
} AriaAudioBlock;

typedef struct AriaEncoderConfig {
    uint32_t width;
    uint32_t height;
    int64_t fps_num;
    int32_t fps_den;
    uint32_t video_bitrate_kbps;
    uint32_t audio_bitrate_kbps;
} AriaEncoderConfig;

typedef struct AriaEncoderFrame {
    const uint8_t *rgba;
    size_t rgba_len;
    uint32_t width;
    uint32_t height;
    uint32_t stride;
    int64_t pts_value;
    int32_t pts_rate;
} AriaEncoderFrame;

typedef struct AriaEncoderAudio {
    const float *samples;
    size_t samples_len;
    uint32_t sample_rate;
    uint32_t channels;
    int64_t pts_value;
    int32_t pts_rate;
} AriaEncoderAudio;

typedef struct AriaJobStep {
    float progress;
    uint32_t done;
    uint32_t commit;
} AriaJobStep;

typedef struct AriaVideoInfo {
    uint32_t width;
    uint32_t height;
    uint32_t format;
    uint32_t color;
    int32_t frame_rate_num;
    int32_t frame_rate_den;
    int32_t time_base_num;
    int32_t time_base_den;
    int64_t duration;
} AriaVideoInfo;

typedef struct AriaFramePlane {
    uint8_t *data;
    size_t len;
    uint32_t stride;
    uint32_t rows;
} AriaFramePlane;

typedef struct AriaVideoFrameOut {
    AriaFramePlane *planes;
    size_t planes_len;
    int64_t pts;
    uint32_t produced;
} AriaVideoFrameOut;

typedef struct AriaScopeCall {
    const uint8_t *frame;
    size_t frame_len;
    uint32_t frame_width;
    uint32_t frame_height;
    uint8_t *image;
    size_t image_len;
    uint32_t image_width;
    uint32_t image_height;
} AriaScopeCall;

typedef struct AriaCpuEffectCall {
    const uint16_t *input;
    uint16_t *output;
    size_t pixels_len;
    uint32_t width;
    uint32_t height;
    const float *params;
    size_t params_len;
    AriaStr params_json;
    float time;
} AriaCpuEffectCall;

typedef struct AriaParamValue {
    uint32_t kind;
    float x;
    float y;
    float z;
    float w;
    uint8_t r;
    uint8_t g;
    uint8_t b;
    uint8_t a;
} AriaParamValue;

typedef struct AriaDriverEvalCall {
    double time;
    double clip_time;
    double frame_rate;
    double duration;
    uint32_t canvas_width;
    uint32_t canvas_height;
    AriaParamValue value;
    uint64_t seed;
    AriaParamValue output;
} AriaDriverEvalCall;

typedef struct AriaHostV1 {
    size_t struct_size;
    uint32_t api_major;
    uint32_t reserved;
    void (*free_string)(AriaOwnedStr str);
    void (*free_bytes)(AriaOwnedBytes bytes);
    void (*log)(void *host_ctx, uint32_t level, AriaStr text);
    AriaBytesResult (*storage_get)(void *host_ctx, AriaStr key);
    AriaResult (*storage_set)(void *host_ctx, AriaStr key, AriaBytes value);
    AriaResult (*storage_delete)(void *host_ctx, AriaStr key);
    AriaResult (*storage_keys)(void *host_ctx);
    AriaResult (*setting_get)(void *host_ctx, AriaStr key);
    AriaResult (*register_command)(void *host_ctx, AriaStr id, AriaStr title);
    AriaResult (*subscribe)(void *host_ctx, AriaStr event_kind);
    AriaResult (*register_importer)(void *host_ctx, AriaStr spec_json);
    AriaResult (*register_exporter)(void *host_ctx, AriaStr spec_json);
    AriaResult (*register_audio_effect)(void *host_ctx, AriaStr spec_json);
    AriaResult (*project_snapshot)(void *host_ctx);
    AriaResult (*editor_view)(void *host_ctx);
    AriaResult (*project_transact)(void *host_ctx, AriaStr label, AriaStr commands_json);
    AriaResult (*project_undo)(void *host_ctx);
    AriaResult (*register_panel)(void *host_ctx, AriaStr spec_json);
    AriaResult (*refresh_panel)(void *host_ctx, AriaStr id);
    AriaResult (*register_encoder)(void *host_ctx, AriaStr spec_json);
    AriaResult (*register_viewer_overlay)(void *host_ctx, AriaStr spec_json);
    AriaResult (*refresh_viewer_overlay)(void *host_ctx, AriaStr id);
    AriaInstanceResult (*start_job)(void *host_ctx, AriaStr label);
    AriaResult (*register_decoder)(void *host_ctx, AriaStr spec_json);
    AriaResult (*register_ai_tool)(void *host_ctx, AriaStr spec_json);
    AriaResult (*register_scope)(void *host_ctx, AriaStr spec_json);
    AriaResult (*register_effect)(void *host_ctx, AriaStr spec_json, AriaStr wgsl);
    AriaResult (*register_cpu_effect)(void *host_ctx, AriaStr spec_json);
    AriaResult (*register_transition)(void *host_ctx, AriaStr spec_json, AriaStr wgsl);
    AriaResult (*register_generator)(void *host_ctx, AriaStr spec_json, AriaStr wgsl);
    AriaResult (*register_clip_type)(void *host_ctx, AriaStr spec_json);
    AriaResult (*register_driver_language)(void *host_ctx, AriaStr spec_json);
    AriaResult (*register_preset)(void *host_ctx, AriaStr preset_json);
    AriaResult (*register_theme)(void *host_ctx, AriaStr id, AriaStr toml);
    AriaResult (*register_workspace)(void *host_ctx, AriaStr spec_json);
    AriaResult (*register_analyzer)(void *host_ctx, AriaStr spec_json);
    AriaResult (*clip)(void *host_ctx, uint64_t id);
    AriaResult (*clips_in_range)(void *host_ctx, uint64_t sequence_id, AriaStr range_json);
    AriaResult (*clips_at)(void *host_ctx, uint64_t sequence_id, AriaStr tick_json);
    AriaResult (*selected_clips)(void *host_ctx);
    AriaResult (*markers_in_range)(void *host_ctx, uint64_t sequence_id, AriaStr range_json);
    AriaResult (*in_out)(void *host_ctx);
    AriaResult (*register_texture)(void *host_ctx, AriaStr id, uint32_t width, uint32_t height, AriaBytes pixels);
} AriaHostV1;

typedef struct AriaPluginV1 {
    size_t struct_size;
    uint32_t api_major;
    uint32_t reserved;
    void (*free_string)(AriaOwnedStr str);
    void (*free_bytes)(AriaOwnedBytes bytes);
    AriaResult (*mount)(void *host_ctx);
    AriaResult (*unmount)(void *host_ctx);
    AriaResult (*command_run)(void *host_ctx, AriaStr id, AriaStr args_json);
    AriaResult (*event)(void *host_ctx, AriaStr kind, AriaStr event_json);
    AriaResult (*import)(void *host_ctx, AriaStr importer_id, AriaStr path);
    AriaResult (*export)(void *host_ctx, AriaStr exporter_id, AriaStr project_json, AriaStr path);
    AriaInstanceResult (*audio_create)(void *host_ctx, AriaStr effect_id, uint32_t sample_rate);
    AriaResult (*audio_process)(void *host_ctx, uint64_t instance, const AriaAudioParam *params, size_t params_len, AriaAudioBlock *block);
    AriaResult (*audio_reset)(void *host_ctx, uint64_t instance);
    AriaResult (*audio_drop)(void *host_ctx, uint64_t instance);
    AriaBytesResult (*panel_view)(void *host_ctx, AriaStr panel_id);
    AriaResult (*panel_event)(void *host_ctx, AriaStr panel_id, AriaStr event_json);
    AriaInstanceResult (*encoder_open)(void *host_ctx, AriaStr encoder_id, AriaStr path, const AriaEncoderConfig *config);
    AriaResult (*encoder_push_frame)(void *host_ctx, uint64_t instance, const AriaEncoderFrame *frame);
    AriaResult (*encoder_push_audio)(void *host_ctx, uint64_t instance, const AriaEncoderAudio *audio);
    AriaResult (*encoder_finish)(void *host_ctx, uint64_t instance);
    AriaResult (*encoder_drop)(void *host_ctx, uint64_t instance);
    AriaBytesResult (*overlay_scene)(void *host_ctx, AriaStr overlay_id);
    AriaResult (*overlay_event)(void *host_ctx, AriaStr overlay_id, AriaStr event_json);
    AriaResult (*job_step)(void *host_ctx, uint64_t job, AriaJobStep *step);
    AriaResult (*job_end)(void *host_ctx, uint64_t job, uint32_t cancelled);
    AriaResult (*decoder_probe)(void *host_ctx, AriaStr decoder_id, AriaStr path);
    AriaInstanceResult (*decoder_open_video)(void *host_ctx, AriaStr decoder_id, AriaStr path, AriaVideoInfo *info);
    AriaResult (*decoder_seek)(void *host_ctx, uint64_t instance, int64_t pts);
    AriaResult (*decoder_next_frame)(void *host_ctx, uint64_t instance, AriaVideoFrameOut *frame);
    AriaInstanceResult (*decoder_open_audio)(void *host_ctx, AriaStr decoder_id, AriaStr path, uint32_t sample_rate, uint32_t channels);
    AriaResult (*decoder_read_audio)(void *host_ctx, uint64_t instance, float *out, size_t out_len, size_t *written);
    AriaResult (*decoder_seek_audio)(void *host_ctx, uint64_t instance, int64_t frame);
    AriaResult (*decoder_close)(void *host_ctx, uint64_t instance);
    AriaResult (*ai_tool_call)(void *host_ctx, AriaStr name, AriaStr arguments_json);
    AriaResult (*scope_measure)(void *host_ctx, AriaStr scope_id, AriaScopeCall *call);
    AriaResult (*cpu_effect_process)(void *host_ctx, AriaStr effect_id, AriaCpuEffectCall *call);
    AriaResult (*driver_evaluate)(void *host_ctx, AriaStr language_id, AriaStr expr, AriaDriverEvalCall *call);
    AriaInstanceResult (*analyzer_open)(void *host_ctx, AriaStr analyzer_id, uint32_t sample_rate, uint32_t channels);
    AriaResult (*analyzer_push)(void *host_ctx, uint64_t instance, const float *samples, size_t samples_len);
    AriaResult (*analyzer_finish)(void *host_ctx, uint64_t instance);
    AriaResult (*analyzer_drop)(void *host_ctx, uint64_t instance);
} AriaPluginV1;

typedef const AriaPluginV1 *(*AriaPluginEntryFn)(const AriaHostV1 *host, uint32_t api_major);

const AriaPluginV1 *aria_plugin_entry(const AriaHostV1 *host, uint32_t api_major);

#ifdef __cplusplus
}
#endif

#endif /* ARIA_PLUGIN_H */
