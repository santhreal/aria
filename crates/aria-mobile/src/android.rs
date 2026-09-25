//! JNI mirrors of the editing-session C ABI (Android front door).
//!
//! Every function marshals Java types to the exact same Rust entry points the
//! iOS shell calls — strings in/out for JSON, `[width, height, argb…]` int
//! arrays for pixels, `jlong` for opaque handles — so Kotlin and Swift ride
//! one engine surface. Threading contracts match the C header: serialize all
//! calls on one session handle; export jobs/thumbnailers/audio readers are
//! independent.
//!
//! Class: `com.scytheralpha.aria_android.AriaNative`.

use jni::JNIEnv;
use jni::objects::{JClass, JFloatArray, JString};
use jni::sys::{jboolean, jdouble, jint, jintArray, jlong, jstring};

use crate::audio::{
    AriaAudioReader, aria_audio_channels, aria_audio_close, aria_audio_rate, aria_audio_read,
    aria_session_audio_open,
};
use crate::catalogs::aria_catalogs;
use crate::export_job::{
    AriaExportJob, aria_export_cancel, aria_export_finished, aria_export_free, aria_export_join,
    aria_export_progress, aria_export_start,
};
use crate::session::{
    AriaSession, aria_session_apply, aria_session_begin_group, aria_session_can_redo,
    aria_session_can_undo, aria_session_close, aria_session_commit_group,
    aria_session_duration_seconds, aria_session_intent, aria_session_is_dirty, aria_session_new,
    aria_session_open, aria_session_redo, aria_session_render_fit, aria_session_revision,
    aria_session_rollback_group, aria_session_ui_state, aria_session_undo,
};
use crate::thumbs::{
    AriaThumbnailer, aria_thumbnailer_close, aria_thumbnailer_duration_seconds,
    aria_thumbnailer_open, aria_thumbnailer_thumb,
};
use crate::wire::aria_string_free;
use crate::{AriaImage, aria_image_free};

// The entry points themselves, grouped by the surface they front. Both children
// open with `use super::*` for the marshalling helpers below.
#[path = "android_media.rs"]
mod android_media;
#[path = "android_session.rs"]
mod android_session;

/// Take ownership of a C string from the FFI, hand it to Java, and free it.
/// Null becomes a null `jstring` (calls that can fail return nullable JSON).
fn take_string(env: &JNIEnv, ptr: *mut std::ffi::c_char) -> jstring {
    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: `ptr` came from a `aria_*` string API this crate allocated.
    let text = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    unsafe { aria_string_free(ptr) };
    match env.new_string(text) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Read a Java string argument (empty on conversion failure — the callee then
/// answers with its usual invalid-arg response).
fn string_arg(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(String::from).unwrap_or_default()
}

fn empty_int_array(env: &JNIEnv) -> jintArray {
    env.new_int_array(0)
        .map(|a| a.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Consume a [`AriaImage`] into `[width, height, argb…]` (ARGB_8888, the
/// `Bitmap.createBitmap` layout) and free the Rust buffer without intermediate allocations.
fn take_image(env: &JNIEnv, image: AriaImage) -> jintArray {
    if image.data.is_null() || image.len == 0 {
        return empty_int_array(env);
    }
    let count = (image.width as usize).saturating_mul(image.height as usize);
    let total_len = count + 2;
    let array = match env.new_int_array(total_len as jint) {
        Ok(a) => a,
        Err(_) => {
            unsafe { aria_image_free(image) };
            return empty_int_array(env);
        }
    };
    let raw_env = env.get_raw();
    let array_raw = array.as_raw();
    let ptr = unsafe {
        let get_fn = (**raw_env)
            .GetPrimitiveArrayCritical
            .expect("GetPrimitiveArrayCritical");
        get_fn(raw_env, array_raw, std::ptr::null_mut()) as *mut i32
    };
    if ptr.is_null() {
        unsafe { aria_image_free(image) };
        return empty_int_array(env);
    }
    // SAFETY: non-null `data`/`len` came from `AriaImage::from_rgba`.
    let pixels = unsafe { std::slice::from_raw_parts(image.data, image.len) };
    unsafe {
        *ptr = image.width as i32;
        *ptr.add(1) = image.height as i32;
        let mut out_ptr = ptr.add(2);
        for px in pixels.chunks_exact(4) {
            let r = px[0] as i32;
            let g = px[1] as i32;
            let b = px[2] as i32;
            let a = px[3] as i32;
            *out_ptr = (a << 24) | (r << 16) | (g << 8) | b;
            out_ptr = out_ptr.add(1);
        }
        let rel_fn = (**raw_env)
            .ReleasePrimitiveArrayCritical
            .expect("ReleasePrimitiveArrayCritical");
        rel_fn(raw_env, array_raw, ptr as *mut std::ffi::c_void, 0);
        aria_image_free(image);
    }
    array.into_raw()
}
