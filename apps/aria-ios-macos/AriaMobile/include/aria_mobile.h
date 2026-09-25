#ifndef ARIA_MOBILE_H
#define ARIA_MOBILE_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

/*
 * C ABI for the `aria-mobile` Rust library.
 *
 * Two data shapes cross this boundary:
 *  - pixels: `AriaImage` RGBA8 buffers, released with `aria_image_free`;
 *  - everything else: UTF-8 JSON strings (commands, intents, UI state,
 *    verdicts), released with `aria_string_free`.
 *
 * String-returning calls answer with a response envelope:
 *   {"ok": <payload>, "revision": <n>}   on success
 *   {"err": {"kind": "...", "message": "..."}} on failure
 * `kind` is stable ("model" | "time" | "render" | "decode" | "io" |
 * "import" | "export" | "missing_media" | "unsupported" | "protocol" |
 * "cancelled"); `message` is human-readable.
 */

typedef struct AriaImage {
    /* RGBA8 pixels (`len` bytes), or NULL if rendering failed. */
    uint8_t *data;
    /* Length of `data` in bytes (== width * height * 4). */
    size_t len;
    uint32_t width;
    uint32_t height;
} AriaImage;

/* Render the built-in demo scene at `width` x `height`. `data` is NULL on failure. */
AriaImage aria_render_demo(uint32_t width, uint32_t height);

/*
 * Decode + composite the first frame of the video at `path_utf8` (a UTF-8 path
 * of `path_len` bytes, no NUL terminator required), scaled to fit
 * `max_width` x `max_height`. `data` is NULL on failure.
 */
AriaImage aria_render_file_frame(const uint8_t *path_utf8, size_t path_len,
                                       uint32_t max_width, uint32_t max_height);

/* Release a buffer returned by `aria_render_demo`. NULL/empty is a no-op. */
void aria_image_free(AriaImage img);

/*
 * Interactive preview.
 *
 * A `AriaPreview` holds a persistent GPU device + decoder cache bound to a
 * project, so scrubbing only pays for the frame at a given time. Open a session,
 * call `aria_preview_render` per slider tick, and free it with
 * `aria_preview_close`. Not thread-safe: serialize calls on one handle.
 */
typedef struct AriaPreview AriaPreview;

/* Open the synthetic scrub demo (no assets). NULL on failure. */
AriaPreview *aria_preview_open_demo(void);

/*
 * Open a preview that scrubs the video at `path_utf8` (`path_len` UTF-8 bytes).
 * NULL if the file can't be probed or the GPU is unavailable.
 */
AriaPreview *aria_preview_open_video(const uint8_t *path_utf8, size_t path_len);

/* Total scrub length in seconds. 0.0 for a NULL handle. */
double aria_preview_duration_seconds(const AriaPreview *handle);

/*
 * Render the preview frame at `seconds` (clamped to range). `data` is NULL on
 * failure; release every non-null result once with `aria_image_free`.
 */
AriaImage aria_preview_render(AriaPreview *handle, double seconds);

/* Release a handle from `aria_preview_open_*`. NULL is a no-op. */
void aria_preview_close(AriaPreview *handle);

/*
 * Editing session.
 *
 * A `AriaSession` owns the full editing engine for one project: state,
 * command dispatch, undo/redo, dirty tracking, save/load, and a persistent
 * renderer. Commands/intents go in as JSON, UI state comes out as JSON,
 * preview frames come out as `AriaImage`.
 *
 * Not thread-safe: serialize all calls on one handle (wrap it in an actor).
 * Different handles are independent.
 */
typedef struct AriaSession AriaSession;

/* Free a string returned by any `aria_session_*`/`aria_export_*` call.
 * NULL is a no-op. */
void aria_string_free(char *ptr);

/*
 * Every preset vocabulary the panels render (effects, transitions, masks,
 * filters, animations, text effects, speed presets, stabilize levels, audio
 * roles) as one JSON document of `{id, label, ...}` lists — the same catalogs
 * the engine validates against. Static data: fetch once per process, no
 * session needed. Free with `aria_string_free`.
 */
char *aria_catalogs(void);

/*
 * Open a fresh session: an empty project at `fps_num/fps_den` (falls back to
 * 30 fps when non-positive) with a main video track. NULL when the GPU
 * renderer can't be brought up. Free with `aria_session_close`.
 */
AriaSession *aria_session_new(int32_t fps_num, int32_t fps_den);

/*
 * Open a session from a `.aria` project file (`path_len` UTF-8 bytes).
 * Missing media paths are tolerated (clips relink later). NULL on failure.
 */
AriaSession *aria_session_open(const uint8_t *path_utf8, size_t path_len);

/* Release a session handle. NULL is a no-op. */
void aria_session_close(AriaSession *handle);

/*
 * Retrieve the last error message recorded by a session open or new failure.
 * NULL if no error was recorded. Free with `aria_string_free`.
 */
char *aria_last_error(void);

/*
 * Apply one wire command (`json_len` UTF-8 bytes of the flat {"type": ...}
 * command object; Save/Load/Import are commands too). Returns a response
 * envelope; free it with `aria_string_free`.
 */
char *aria_session_apply(AriaSession *handle, const uint8_t *json_utf8, size_t json_len);

/*
 * Run one gesture-level intent ({"intent": ...} JSON). Multi-command intents
 * are grouped into a single undo step and roll back atomically on failure.
 * Returns a response envelope; free with `aria_string_free`.
 */
char *aria_session_intent(AriaSession *handle, const uint8_t *json_utf8, size_t json_len);

/*
 * The full UI presentation state (ordered lanes, clips, canvas, durations,
 * undo/redo/dirty flags) as JSON. Free with `aria_string_free`.
 */
char *aria_session_ui_state(AriaSession *handle);

/* Undo/redo the most recent edit (or group). Returns whether anything changed. */
bool aria_session_undo(AriaSession *handle);
bool aria_session_redo(AriaSession *handle);
bool aria_session_can_undo(const AriaSession *handle);
bool aria_session_can_redo(const AriaSession *handle);

/* Monotonic revision; bumps on every successful mutation (cache keys). */
uint64_t aria_session_revision(const AriaSession *handle);

/* Whether the session has edits not yet saved to its project file. */
bool aria_session_is_dirty(const AriaSession *handle);

/*
 * History grouping: every command between begin and commit folds into one
 * undo step (property-panel sessions). Rollback aborts the open group,
 * reverting its commands.
 */
void aria_session_begin_group(AriaSession *handle);
void aria_session_commit_group(AriaSession *handle);
void aria_session_rollback_group(AriaSession *handle);

/* End of the timeline in seconds (0 for an empty project). */
double aria_session_duration_seconds(const AriaSession *handle);

/*
 * Render the timeline frame nearest `seconds` (snapped to the project frame
 * grid, clamped to the timeline), scaled to fit `max_width` x `max_height`
 * (aspect preserved, never upscaled). `data` is NULL on failure; release
 * every non-null result once with `aria_image_free`.
 */
AriaImage aria_session_render_fit(AriaSession *handle, double seconds,
                                        uint32_t max_width, uint32_t max_height);

/*
 * Background export job.
 *
 * `aria_export_start` snapshots the session's project and encodes it to
 * `path` on a private thread with its own renderer, so the session stays
 * fully interactive. Poll `progress`/`finished` from anywhere; `cancel` stops
 * after the frame in flight (the partial file is deleted); `join` blocks and
 * returns the JSON verdict {"ok":{"frames":n}} or
 * {"err":{"kind":"cancelled"|"render","message":...}} (idempotent). Always
 * `join` then `free`. Freeing a running job joins it first — cancel for a
 * fast teardown.
 */
typedef struct AriaExportJob AriaExportJob;

/*
 * `width`/`height` override the output size when both are non-zero (aspect
 * preserved, letterboxed on mismatch; odd sizes rounded down to even for
 * H.264). `fps_num`/`fps_den` override the frame rate when both are positive.
 * Zero keeps the project's native value. NULL when the session/path is
 * invalid.
 */
AriaExportJob *aria_export_start(AriaSession *session,
                                       const uint8_t *path_utf8, size_t path_len,
                                       uint32_t width, uint32_t height,
                                       int32_t fps_num, int32_t fps_den);

/* Fraction of frames written so far, 0.0..1.0. */
double aria_export_progress(const AriaExportJob *job);

/* Whether the job's thread has finished (successfully or not). */
bool aria_export_finished(const AriaExportJob *job);

/* Ask the job to stop after the frame in flight. Idempotent. */
void aria_export_cancel(const AriaExportJob *job);

/* Block until finished; the JSON verdict. Free with `aria_string_free`. */
char *aria_export_join(const AriaExportJob *job);

/* Release the job handle (joins first if nobody has). NULL is a no-op. */
void aria_export_free(AriaExportJob *job);

/*
 * Filmstrip thumbnails.
 *
 * A `AriaThumbnailer` owns a private renderer + decoder cache for one
 * media file, so filmstrips can render off the session thread. Serialize
 * calls per handle; different handles are independent.
 */
typedef struct AriaThumbnailer AriaThumbnailer;

/* Open a thumbnailer for the media file at `path` (`path_len` UTF-8 bytes).
 * NULL if the file can't be probed or the GPU is unavailable. */
AriaThumbnailer *aria_thumbnailer_open(const uint8_t *path_utf8, size_t path_len);

/* Media length in seconds (0 for a NULL handle). */
double aria_thumbnailer_duration_seconds(const AriaThumbnailer *handle);

/*
 * The frame nearest `seconds`, scaled to fit `max_width` x `max_height`
 * (never upscaled). `data` is NULL on failure; free non-null results with
 * `aria_image_free`.
 */
AriaImage aria_thumbnailer_thumb(AriaThumbnailer *handle, double seconds,
                                       uint32_t max_width, uint32_t max_height);

/* Release a thumbnailer handle. NULL is a no-op. */
void aria_thumbnailer_close(AriaThumbnailer *handle);

/*
 * Realtime audio playback.
 *
 * `aria_session_audio_open` snapshots the session's project and returns a
 * pull reader of the timeline's mixed audio (every audible clip summed, with
 * volume/fades applied) as interleaved stereo f32 at 48 kHz. Later edits
 * never affect an open reader — watch the session revision and reopen at the
 * playhead on change, same as seeking.
 *
 * Reads block while sources decode: pull from a feeder thread into a ring
 * buffer and let the audio render callback only copy. Serialize calls per
 * handle; readers are independent of each other and of the session.
 */
typedef struct AriaAudioReader AriaAudioReader;

/* Sample rate (Hz) and interleaved channel count of every read block. */
uint32_t aria_audio_rate(void);
uint32_t aria_audio_channels(void);

/*
 * Open a mixed-audio reader at `start_seconds` (clamped into the timeline).
 * NULL when the session is NULL or the timeline has no audible clips. Free
 * with `aria_audio_close`.
 */
AriaAudioReader *aria_session_audio_open(AriaSession *session, double start_seconds);

/*
 * Pull up to `max_frames` interleaved stereo sample frames into `out`
 * (`max_frames * 2` floats), advancing the reader. Returns the number of
 * frames written: 0 at the end of the timeline, -1 after a decode failure
 * (sticky).
 */
intptr_t aria_audio_read(AriaAudioReader *handle, float *out, size_t max_frames);

/* Release a reader handle. NULL is a no-op. */
void aria_audio_close(AriaAudioReader *handle);

#endif /* ARIA_MOBILE_H */
