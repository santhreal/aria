//! JNI entry points for the catalogs document and the session handle:
//! lifecycle, wire commands, gesture intents, history, and preview frames.
//!
//! Declared from `android.rs` with `#[path]`; `use super::*` reaches that
//! module's imports and its Java marshalling helpers, so the move changed no
//! name resolution. `#[unsafe(no_mangle)]` exports ignore module privacy.
//! The module exports `#[unsafe(no_mangle)]` JNI entry points that forward raw handles to the C ABI and is `unsafe` throughout.

#![allow(unsafe_code)]

use super::*;

// --- Catalogs ---------------------------------------------------------------

/// `AriaNative.catalogs()`: every preset vocabulary as one JSON document.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_catalogs<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    crate::android_jni::init_logging();
    take_string(&env, aria_catalogs())
}

// --- Session lifecycle -------------------------------------------------------

/// `AriaNative.sessionNew(fpsNum, fpsDen)`: a fresh project session.
/// Returns the handle as a `jlong` (`0` on failure).
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionNew<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    fps_num: jint,
    fps_den: jint,
) -> jlong {
    crate::android_jni::init_logging();
    aria_session_new(fps_num, fps_den) as jlong
}

/// `AriaNative.sessionOpen(path)`: open a `.aria` project file.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionOpen<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
) -> jlong {
    crate::android_jni::init_logging();
    let path = string_arg(&mut env, &path);
    // SAFETY: the pointer/length pair borrows the live `path` String.
    unsafe { aria_session_open(path.as_ptr(), path.len()) as jlong }
}

/// `AriaNative.sessionClose(handle)`: release a session.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionClose<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    // SAFETY: `handle` came from `sessionNew`/`sessionOpen`, freed once.
    unsafe { aria_session_close(handle as *mut AriaSession) }
}

// --- Commands / intents / state ----------------------------------------------

/// `AriaNative.sessionApply(handle, json)`: one wire command; the JSON
/// response envelope comes back.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionApply<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    json: JString<'local>,
) -> jstring {
    let json = string_arg(&mut env, &json);
    // SAFETY: session handle per caller contract; bytes borrow `json`.
    let response =
        unsafe { aria_session_apply(handle as *mut AriaSession, json.as_ptr(), json.len()) };
    take_string(&env, response)
}

/// `AriaNative.sessionIntent(handle, json)`: one gesture intent.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionIntent<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    json: JString<'local>,
) -> jstring {
    let json = string_arg(&mut env, &json);
    // SAFETY: same contract as `sessionApply`.
    let response =
        unsafe { aria_session_intent(handle as *mut AriaSession, json.as_ptr(), json.len()) };
    take_string(&env, response)
}

/// `AriaNative.sessionUiState(handle)`: the lane-stack JSON (null for a
/// null handle).
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionUiState<
    'local,
>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jstring {
    // SAFETY: session handle per caller contract.
    take_string(&env, unsafe {
        aria_session_ui_state(handle as *mut AriaSession)
    })
}

/// `AriaNative.sessionUndo(handle)`.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionUndo<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jboolean {
    // SAFETY: session handle per caller contract.
    (unsafe { aria_session_undo(handle as *mut AriaSession) }) as jboolean
}

/// `AriaNative.sessionRedo(handle)`.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionRedo<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jboolean {
    // SAFETY: session handle per caller contract.
    (unsafe { aria_session_redo(handle as *mut AriaSession) }) as jboolean
}

/// `AriaNative.sessionCanUndo(handle)`.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionCanUndo<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jboolean {
    // SAFETY: session handle per caller contract.
    (unsafe { aria_session_can_undo(handle as *mut AriaSession) }) as jboolean
}

/// `AriaNative.sessionCanRedo(handle)`.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionCanRedo<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jboolean {
    // SAFETY: session handle per caller contract.
    (unsafe { aria_session_can_redo(handle as *mut AriaSession) }) as jboolean
}

/// `AriaNative.sessionRevision(handle)`.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionRevision<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jlong {
    // SAFETY: session handle per caller contract.
    (unsafe { aria_session_revision(handle as *mut AriaSession) }) as jlong
}

/// `AriaNative.sessionIsDirty(handle)`.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionIsDirty<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jboolean {
    // SAFETY: session handle per caller contract.
    (unsafe { aria_session_is_dirty(handle as *mut AriaSession) }) as jboolean
}

/// `AriaNative.sessionBeginGroup(handle)`.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionBeginGroup<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    // SAFETY: session handle per caller contract.
    unsafe { aria_session_begin_group(handle as *mut AriaSession) }
}

/// `AriaNative.sessionCommitGroup(handle)`.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionCommitGroup<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    // SAFETY: session handle per caller contract.
    unsafe { aria_session_commit_group(handle as *mut AriaSession) }
}

/// `AriaNative.sessionRollbackGroup(handle)`.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionRollbackGroup<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    // SAFETY: session handle per caller contract.
    unsafe { aria_session_rollback_group(handle as *mut AriaSession) }
}

/// `AriaNative.sessionDurationSeconds(handle)`.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionDurationSeconds<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jdouble {
    // SAFETY: session handle per caller contract.
    unsafe { aria_session_duration_seconds(handle as *mut AriaSession) }
}

/// `AriaNative.sessionRenderFit(handle, seconds, maxWidth, maxHeight)`:
/// the timeline frame nearest `seconds` as `[width, height, argb…]`. Empty
/// array on failure.
#[unsafe(no_mangle)]
pub(super) extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_sessionRenderFit<
    'local,
>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    seconds: jdouble,
    max_width: jint,
    max_height: jint,
) -> jintArray {
    // SAFETY: session handle per caller contract.
    let image = unsafe {
        aria_session_render_fit(
            handle as *mut AriaSession,
            seconds,
            max_width.max(0) as u32,
            max_height.max(0) as u32,
        )
    };
    take_image(&env, image)
}
