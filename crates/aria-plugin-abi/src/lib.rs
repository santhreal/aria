//! Stable C ABI types and versioned function tables shared by the Aria host and native plugins.
//!
//! A native plugin is a shared library that exports `aria_plugin_entry` with the
//! [`AriaPluginEntryFn`] signature. The host calls it once, when it loads the library,
//! with its [`AriaHostV1`] table and [`ARIA_API_MAJOR`]. The plugin returns its
//! [`AriaPluginV1`] table, or null to decline the load. Both tables stay valid while
//! the library is loaded. `include/aria_plugin.h` declares the same items for C and C++.
//!
//! Both tables start with the fields of [`AriaTableHeader`]. `struct_size` is the byte
//! size of the table its sender filled, and `api_major` is [`ARIA_API_MAJOR`]. A table
//! with another major, or shorter than [`ARIA_HOST_V1_MIN_SIZE`] or
//! [`ARIA_PLUGIN_V1_MIN_SIZE`], fails to load. A longer table adds slots in the
//! prefixes the `ARIA_*_V1_*_SIZE` constants mark, and each side reads only the slots
//! the other side's `struct_size` covers.
//!
//! Every callback receives `host_ctx` first. It is valid until that callback returns.
//! Pass it to the host calls made during the callback, and do not keep it.
//!
//! An [`AriaStr`] or [`AriaBytes`] argument borrows the caller's memory until the call
//! returns. An [`AriaStr`] is UTF-8 and holds no terminating NUL. An [`AriaOwnedStr`] or
//! [`AriaOwnedBytes`] in a returned value passes to the receiver, which releases it with
//! the `free_string` or `free_bytes` slot of the sender's table: the host releases what
//! a plugin returns with [`AriaPluginV1::free_string`] and [`AriaPluginV1::free_bytes`],
//! and a plugin releases what a host call returns with [`AriaHostV1::free_string`] and
//! [`AriaHostV1::free_bytes`].
//!
//! A callback that fails returns [`ARIA_STATUS_ERR`] with the message. A callback must
//! not unwind into the host: it catches a panic or exception and returns
//! [`ARIA_STATUS_PANIC`], [`ARIA_BYTES_PANIC`] or [`ARIA_INSTANCE_PANIC`] with the
//! message, and the host fails the plugin for the rest of the session. `driver_evaluate`
//! is the one exception: a panic there fails only that evaluation.

#![deny(missing_docs)]

/// Major version of this C ABI. The host passes it to `aria_plugin_entry`, and both
/// tables hold it in `api_major`.
pub const ARIA_API_MAJOR: u32 = 1;

/// Name of the function a native plugin library exports as its entry point, with the
/// [`AriaPluginEntryFn`] signature.
pub const ARIA_PLUGIN_ENTRY_SYMBOL: &[u8] = b"aria_plugin_entry";

/// [`AriaResult::status`]: the call succeeded, and `value` holds its result or is empty.
pub const ARIA_STATUS_OK: u32 = 0;
/// [`AriaResult::status`]: the call failed, and `value` holds the error message.
pub const ARIA_STATUS_ERR: u32 = 1;
/// [`AriaResult::status`]: the plugin caught a panic or exception, and `value` holds
/// its message.
pub const ARIA_STATUS_PANIC: u32 = 2;
/// [`AriaResult::status`]: a decoder call on a file the decoder does not read. The
/// host tries the next decoder without reporting the message.
pub const ARIA_STATUS_UNSUPPORTED: u32 = 3;

/// [`AriaInstanceResult::status`]: the call created the instance in `instance`. Every
/// call that returns an [`AriaInstanceResult`] uses these codes: `audio_create`,
/// `encoder_open`, `decoder_open_video`, `decoder_open_audio`, `analyzer_open` and
/// `start_job`.
pub const ARIA_INSTANCE_OK: u32 = ARIA_STATUS_OK;
/// [`AriaInstanceResult::status`]: the call failed, and `error` holds the message.
pub const ARIA_INSTANCE_ERR: u32 = ARIA_STATUS_ERR;
/// [`AriaInstanceResult::status`]: the plugin caught a panic or exception, and `error`
/// holds its message.
pub const ARIA_INSTANCE_PANIC: u32 = ARIA_STATUS_PANIC;
/// [`AriaInstanceResult::status`]: a decoder open on a file the decoder does not read.
/// The host tries the next decoder without reporting the message.
pub const ARIA_INSTANCE_UNSUPPORTED: u32 = ARIA_STATUS_UNSUPPORTED;

/// [`AriaBytesResult::status`]: `bytes` holds the result: the value stored under a
/// `storage_get` key, the panel tree JSON of `panel_view`, or the scene JSON of
/// `overlay_scene`.
pub const ARIA_BYTES_FOUND: u32 = 0;
/// [`AriaBytesResult::status`]: `storage_get` found no value under the key. `bytes` and
/// `error` are empty.
pub const ARIA_BYTES_NOT_FOUND: u32 = 1;
/// [`AriaBytesResult::status`]: the call failed, and `error` holds the message.
pub const ARIA_BYTES_ERR: u32 = 2;
/// [`AriaBytesResult::status`]: the plugin caught a panic or exception in `panel_view`
/// or `overlay_scene`, and `error` holds its message.
pub const ARIA_BYTES_PANIC: u32 = 3;

/// `level` of [`AriaHostV1::log`]: trace.
pub const ARIA_LOG_TRACE: u32 = 0;
/// `level` of [`AriaHostV1::log`]: debug.
pub const ARIA_LOG_DEBUG: u32 = 1;
/// `level` of [`AriaHostV1::log`]: info.
pub const ARIA_LOG_INFO: u32 = 2;
/// `level` of [`AriaHostV1::log`]: warn.
pub const ARIA_LOG_WARN: u32 = 3;
/// `level` of [`AriaHostV1::log`]: error. The host logs any other level as an error too.
pub const ARIA_LOG_ERROR: u32 = 4;

/// [`AriaVideoInfo::format`]: a full-size Y plane, then interleaved U and V at
/// half width and half height.
pub const ARIA_FORMAT_NV12: u32 = 0;
/// [`AriaVideoInfo::format`]: full-size Y, then U, then V at half width and
/// half height.
pub const ARIA_FORMAT_YUV420P: u32 = 1;

/// [`AriaVideoInfo::color`]: BT.709 limited range.
pub const ARIA_COLOR_BT709: u32 = 0;
/// [`AriaVideoInfo::color`]: BT.601 limited range.
pub const ARIA_COLOR_BT601: u32 = 1;
/// [`AriaVideoInfo::color`]: BT.2020 primaries with the PQ transfer.
pub const ARIA_COLOR_BT2020_PQ: u32 = 2;
/// [`AriaVideoInfo::color`]: sRGB, full range.
pub const ARIA_COLOR_SRGB: u32 = 3;

/// [`AriaParamValue::kind`]: a scalar in `x`.
pub const ARIA_PARAM_SCALAR: u32 = 0;
/// [`AriaParamValue::kind`]: 2-d vector in `x` and `y`.
pub const ARIA_PARAM_VEC2: u32 = 1;
/// [`AriaParamValue::kind`]: RGBA8 colour in `r`, `g`, `b` and `a`.
pub const ARIA_PARAM_COLOR: u32 = 2;
/// [`AriaParamValue::kind`]: axis-aligned rect, with its left edge in `x`, its top
/// edge in `y`, its width in `z` and its height in `w`.
pub const ARIA_PARAM_RECT: u32 = 3;
/// [`AriaParamValue::kind`]: 3-d vector in `x`, `y` and `z`.
pub const ARIA_PARAM_VEC3: u32 = 4;

mod values;
pub use values::*;

mod calls;
pub use calls::*;

mod tables;
pub use tables::*;

mod sizes;
pub use sizes::*;

/// Validate a plugin table's API major version and struct size.
///
/// Both refusals state the plugin's value and the host's expected value.
pub fn validate_plugin_table(api_major: u32, struct_size: usize) -> Result<(), String> {
    if api_major != ARIA_API_MAJOR {
        return Err(format!(
            "plugin targets API major {api_major}, host supports API major {ARIA_API_MAJOR}"
        ));
    }
    let expected = ARIA_PLUGIN_V1_MIN_SIZE;
    if struct_size < expected {
        return Err(format!(
            "plugin table is {struct_size} bytes, host requires at least {expected} bytes"
        ));
    }
    Ok(())
}

/// Validate a host table's API major version and struct size.
pub fn validate_host_table(api_major: u32, struct_size: usize) -> Result<(), String> {
    if api_major != ARIA_API_MAJOR {
        return Err(format!(
            "host provided API major {api_major}, plugin expects API major {ARIA_API_MAJOR}"
        ));
    }
    let expected = ARIA_HOST_V1_MIN_SIZE;
    if struct_size < expected {
        return Err(format!(
            "host table is {struct_size} bytes, plugin requires at least {expected} bytes"
        ));
    }
    Ok(())
}

#[cfg(test)]
use std::ffi::c_void;

#[cfg(test)]
use sizes::{stub_analyzer_drop, stub_analyzer_finish, stub_analyzer_open, stub_analyzer_push};
#[cfg(test)]
mod tests;
