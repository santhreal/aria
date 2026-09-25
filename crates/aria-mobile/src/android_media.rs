//! JNI entry points for the media services hanging off a session: export
//! jobs, the filmstrip thumbnailer, and the realtime audio pull reader.
//!
//! Declared from `android.rs` with `#[path]`; `use super::*` reaches that
//! module's imports and its Java marshalling helpers, so the move changed no
//! name resolution. `#[unsafe(no_mangle)]` exports ignore module privacy.

use super::*;

// --- Export jobs ---------------------------------------------------------------

/// `AriaNative.exportStart(session, path, width, height, fpsNum, fpsDen)`:
/// start a background export. Zero size/fps keeps the project's native value.
/// Returns the job handle (`0` on failure).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_exportStart<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session: jlong,
    path: JString<'local>,
    width: jint,
    height: jint,
    fps_num: jint,
    fps_den: jint,
) -> jlong {
    let path = string_arg(&mut env, &path);
    // SAFETY: session handle per caller contract; bytes borrow `path`.
    unsafe {
        aria_export_start(
            session as *mut AriaSession,
            path.as_ptr(),
            path.len(),
            width.max(0) as u32,
            height.max(0) as u32,
            fps_num,
            fps_den,
        ) as jlong
    }
}

/// `AriaNative.exportProgress(job)`: fraction written, 0.0–1.0.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_exportProgress<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    job: jlong,
) -> jdouble {
    // SAFETY: job handle per caller contract (progress is poll-safe).
    unsafe { aria_export_progress(job as *const AriaExportJob) }
}

/// `AriaNative.exportFinished(job)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_exportFinished<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    job: jlong,
) -> jboolean {
    // SAFETY: job handle per caller contract.
    (unsafe { aria_export_finished(job as *const AriaExportJob) }) as jboolean
}

/// `AriaNative.exportCancel(job)`: stop after the frame in flight.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_exportCancel<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    job: jlong,
) {
    // SAFETY: job handle per caller contract.
    unsafe { aria_export_cancel(job as *const AriaExportJob) }
}

/// `AriaNative.exportJoin(job)`: block until finished; the JSON verdict.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_exportJoin<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    job: jlong,
) -> jstring {
    // SAFETY: job handle per caller contract.
    take_string(&env, unsafe {
        aria_export_join(job as *const AriaExportJob)
    })
}

/// `AriaNative.exportFree(job)`: release the job (joins if needed).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_exportFree<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    job: jlong,
) {
    // SAFETY: job handle per caller contract, freed once.
    unsafe { aria_export_free(job as *mut AriaExportJob) }
}

// --- Thumbnails ------------------------------------------------------------------

/// `AriaNative.thumbnailerOpen(path)`: a filmstrip thumbnailer for one
/// media file. Returns the handle (`0` on failure).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_thumbnailerOpen<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
) -> jlong {
    crate::android_jni::init_logging();
    let path = string_arg(&mut env, &path);
    // SAFETY: bytes borrow the live `path` String.
    unsafe { aria_thumbnailer_open(path.as_ptr(), path.len()) as jlong }
}

/// `AriaNative.thumbnailerDurationSeconds(handle)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_thumbnailerDurationSeconds<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jdouble {
    // SAFETY: thumbnailer handle per caller contract.
    unsafe { aria_thumbnailer_duration_seconds(handle as *const AriaThumbnailer) }
}

/// `AriaNative.thumbnailerThumb(handle, seconds, maxWidth, maxHeight)`:
/// the frame nearest `seconds` as `[width, height, argb…]`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_thumbnailerThumb<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    seconds: jdouble,
    max_width: jint,
    max_height: jint,
) -> jintArray {
    // SAFETY: thumbnailer handle per caller contract.
    let image = unsafe {
        aria_thumbnailer_thumb(
            handle as *mut AriaThumbnailer,
            seconds,
            max_width.max(0) as u32,
            max_height.max(0) as u32,
        )
    };
    take_image(&env, image)
}

/// `AriaNative.thumbnailerClose(handle)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_thumbnailerClose<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    // SAFETY: thumbnailer handle per caller contract, freed once.
    unsafe { aria_thumbnailer_close(handle as *mut AriaThumbnailer) }
}

// --- Realtime audio -----------------------------------------------------------

/// `AriaNative.audioRate()`: sample rate of every read block (Hz).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_audioRate<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jint {
    aria_audio_rate() as jint
}

/// `AriaNative.audioChannels()`: interleaved channel count.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_audioChannels<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jint {
    aria_audio_channels() as jint
}

/// `AriaNative.sessionAudioOpen(session, startSeconds)`: a mixed-audio
/// pull reader over the current timeline. Returns the handle (`0` when the
/// timeline has no audible clips).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionAudioOpen<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session: jlong,
    start_seconds: jdouble,
) -> jlong {
    // SAFETY: session handle per caller contract.
    unsafe { aria_session_audio_open(session as *mut AriaSession, start_seconds) as jlong }
}

/// `AriaNative.audioRead(handle, out, maxFrames)`: pull up to `maxFrames`
/// interleaved stereo frames into `out` (`maxFrames * 2` floats). Returns
/// frames written; `0` at the end of the timeline, `-1` after a decode
/// failure (sticky).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_audioRead<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    out: JFloatArray<'local>,
    max_frames: jint,
) -> jint {
    if handle == 0 || max_frames <= 0 {
        return 0;
    }
    let capacity = env.get_array_length(&out).unwrap_or(0).max(0) as usize;
    let frames = (max_frames as usize).min(capacity / 2);
    if frames == 0 {
        return 0;
    }
    let raw_env = env.get_raw();
    let array_raw = out.as_raw();
    // SAFETY: GetPrimitiveArrayCritical gives direct pinned memory access to Java array elements
    // without heap allocation on the audio thread.
    let ptr = unsafe {
        let get_fn = (**raw_env)
            .GetPrimitiveArrayCritical
            .expect("GetPrimitiveArrayCritical");
        get_fn(raw_env, array_raw, std::ptr::null_mut()) as *mut f32
    };
    if ptr.is_null() {
        return -1;
    }
    // SAFETY: reader handle per caller contract; `ptr` points to at least `frames * 2` floats.
    let written = unsafe { aria_audio_read(handle as *mut AriaAudioReader, ptr, frames) };
    unsafe {
        let rel_fn = (**raw_env)
            .ReleasePrimitiveArrayCritical
            .expect("ReleasePrimitiveArrayCritical");
        rel_fn(raw_env, array_raw, ptr as *mut std::ffi::c_void, 0);
    }
}

/// `AriaNative.audioClose(handle)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_audioClose<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    // SAFETY: reader handle per caller contract, freed once.
    unsafe { aria_audio_close(handle as *mut AriaAudioReader) }
}
