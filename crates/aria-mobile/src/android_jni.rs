//! The Android JNI entry points for the demo and file-frame renderers.
//!
//! Declared from `lib.rs` with `#[path]` behind the same `target_os` gate, so
//! the `super::` imports below still name the crate root.

use std::path::Path;

use aria_compositor::RgbaImage;
use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jdouble, jint, jintArray, jlong};

use super::{AriaPreview, render_demo_rgba, render_file_frame_rgba};

/// Route `log::*` to logcat (tag `aria`) once, so native failures are
/// visible via `adb logcat -s aria`. Idempotent.
pub(crate) fn init_logging() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_tag("aria")
                .with_max_level(log::LevelFilter::Debug),
        );
    });
}

fn empty_array(env: &JNIEnv) -> jintArray {
    env.new_int_array(0)
        .map(|a| a.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Marshal an [`RgbaImage`] into a Java `int[]` laid out as
/// `[width, height, argb_0, argb_1, …]` — the leading dimensions let the
/// caller build the `Bitmap` without knowing the (fitted) output size up
/// front. Pixels are ARGB_8888 (what `Bitmap.createBitmap` expects).
fn rgba_to_argb_array(env: &JNIEnv, image: &RgbaImage) -> jintArray {
    let count = (image.width as usize).saturating_mul(image.height as usize);
    let mut argb = vec![0i32; count + 2];
    argb[0] = image.width as i32;
    argb[1] = image.height as i32;
    for (slot, px) in argb[2..].iter_mut().zip(image.pixels.chunks_exact(4)) {
        let r = i32::from(px[0]);
        let g = i32::from(px[1]);
        let b = i32::from(px[2]);
        let a = i32::from(px[3]);
        *slot = (a << 24) | (r << 16) | (g << 8) | b;
    }
    match env.new_int_array(argb.len() as jint) {
        Ok(array) => {
            if env.set_int_array_region(&array, 0, &argb).is_err() {
                return empty_array(env);
            }
            array.into_raw()
        }
        Err(_) => empty_array(env),
    }
}

/// `com.scytheralpha.aria_android.AriaNative.renderDemo(width, height)`.
/// Returns `width * height` ARGB_8888 pixels, or an empty array on failure.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_renderDemo<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    width: jint,
    height: jint,
) -> jintArray {
    init_logging();
    match render_demo_rgba(width.max(0) as u32, height.max(0) as u32) {
        Some(image) => rgba_to_argb_array(&env, &image),
        None => empty_array(&env),
    }
}

/// `AriaNative.renderFileFrame(path, maxWidth, maxHeight)`: decode +
/// composite the first frame of the video at `path`, scaled to fit, as
/// ARGB_8888 pixels. Empty array on failure.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_renderFileFrame<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
    max_width: jint,
    max_height: jint,
) -> jintArray {
    let Ok(path) = env.get_string(&path) else {
        return empty_array(&env);
    };
    let path: String = path.into();
    match render_file_frame_rgba(
        Path::new(&path),
        max_width.max(0) as u32,
        max_height.max(0) as u32,
    ) {
        Some(image) => rgba_to_argb_array(&env, &image),
        None => empty_array(&env),
    }
}

/// `AriaNative.previewOpenDemo()`: open the synthetic scrub demo. Returns
/// a handle as a `jlong` (`0` on failure) — pass it to the other
/// `preview*` calls and free it with `previewClose`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_previewOpenDemo<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jlong {
    init_logging();
    match AriaPreview::demo() {
        Some(preview) => Box::into_raw(Box::new(preview)) as jlong,
        None => 0,
    }
}

/// `AriaNative.previewOpenVideo(path)`: open a video scrub session.
/// Returns a handle as a `jlong` (`0` on failure).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_previewOpenVideo<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
) -> jlong {
    init_logging();
    let Ok(path) = env.get_string(&path) else {
        return 0;
    };
    let path: String = path.into();
    match AriaPreview::video(Path::new(&path)) {
        Some(preview) => Box::into_raw(Box::new(preview)) as jlong,
        None => 0,
    }
}

/// `AriaNative.previewDurationSeconds(handle)`: total scrub length.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_previewDurationSeconds<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jdouble {
    if handle == 0 {
        return 0.0;
    }
    // SAFETY: `handle` is a live pointer from `previewOpen*`, read-only here.
    let preview = unsafe { &*(handle as *const AriaPreview) };
    preview.duration_seconds()
}

/// `AriaNative.previewRender(handle, seconds)`: the frame at `seconds` as
/// `[width, height, argb…]`. Empty array on failure.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_previewRender<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    seconds: jdouble,
) -> jintArray {
    if handle == 0 {
        return empty_array(&env);
    }
    // SAFETY: `handle` is a live pointer used non-concurrently (UI serializes
    // scrub renders on one handle).
    let preview = unsafe { &mut *(handle as *mut AriaPreview) };
    match preview.render_seconds(seconds) {
        Some(image) => rgba_to_argb_array(&env, &image),
        None => empty_array(&env),
    }
}

/// `AriaNative.previewClose(handle)`: release a preview handle.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_scytheralpha_aria_1android_AriaNative_previewClose<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    if handle != 0 {
        // SAFETY: `handle` came from `Box::into_raw` in `previewOpen*` and is
        // freed exactly once.
        drop(unsafe { Box::from_raw(handle as *mut AriaPreview) });
    }
}
