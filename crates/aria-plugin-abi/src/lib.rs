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

use std::ffi::c_void;
use std::mem::ManuallyDrop;

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

/// Borrowed UTF-8 text, valid until the call it is passed to returns. It holds no
/// terminating NUL.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaStr {
    /// First byte of the text, or null when `len` is 0.
    pub ptr: *const u8,
    /// Length of the text in bytes.
    pub len: usize,
}

impl AriaStr {
    /// Borrow a Rust string slice as an [`AriaStr`].
    pub const fn from_str(text: &str) -> Self {
        Self {
            ptr: text.as_ptr(),
            len: text.len(),
        }
    }

    /// View the borrowed bytes as a UTF-8 string slice.
    ///
    /// # Safety
    ///
    /// `self.ptr` must point to `self.len` valid initialized bytes that remain
    /// alive for `'a` (or `self.len == 0`).
    #[allow(unsafe_code)]
    pub unsafe fn as_str<'a>(self) -> Result<&'a str, std::str::Utf8Error> {
        if self.len == 0 {
            return Ok("");
        }
        // SAFETY: The caller guarantees `ptr` is valid for `len` bytes for `'a`.
        let bytes = unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
        std::str::from_utf8(bytes)
    }
}

/// Borrowed bytes, valid until the call they are passed to returns.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaBytes {
    /// First byte, or null when `len` is 0.
    pub ptr: *const u8,
    /// Number of bytes.
    pub len: usize,
}

impl AriaBytes {
    /// Borrow a Rust byte slice as an [`AriaBytes`].
    pub const fn from_slice(bytes: &[u8]) -> Self {
        Self {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        }
    }

    /// View the borrowed bytes as a slice.
    ///
    /// # Safety
    ///
    /// `self.ptr` must point to `self.len` valid initialized bytes that remain
    /// alive for `'a` (or `self.len == 0`).
    #[allow(unsafe_code)]
    pub unsafe fn as_slice<'a>(self) -> &'a [u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: The caller guarantees `ptr` is valid for `len` bytes for `'a`.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

/// UTF-8 text returned across the boundary. The receiver releases it with the
/// `free_string` slot of the table of the side that allocated it. A `free_string`
/// accepts a null `ptr` and does nothing for it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaOwnedStr {
    /// First byte of the text, or null for an empty string that allocates nothing.
    pub ptr: *mut u8,
    /// Length of the text in bytes.
    pub len: usize,
    /// Allocation size in bytes, for the allocating side's `free_string`. The receiver
    /// does not read it.
    pub cap: usize,
}

impl AriaOwnedStr {
    /// An empty owned string that allocates nothing.
    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    /// Transfer ownership of a Rust [`String`] into an [`AriaOwnedStr`].
    pub fn from_string(text: String) -> Self {
        if text.capacity() == 0 {
            return Self::empty();
        }
        let mut bytes = ManuallyDrop::new(text.into_bytes());
        Self {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        }
    }

    /// Whether the string has no pointer (`None` in option positions).
    pub fn is_null(&self) -> bool {
        self.ptr.is_null() && self.len == 0
    }

    /// Copy the string into a Rust [`String`] and release the buffer with `free`.
    ///
    /// # Safety
    ///
    /// `self` must have been produced by the side whose `free` function is passed.
    #[allow(unsafe_code)]
    pub unsafe fn into_string_with(self, free: unsafe extern "C" fn(AriaOwnedStr)) -> String {
        let out = if self.len == 0 || self.ptr.is_null() {
            String::new()
        } else {
            // SAFETY: `self.ptr` points to `self.len` valid bytes until `free` runs.
            let slice = unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
            String::from_utf8_lossy(slice).into_owned()
        };
        // SAFETY: `self` was allocated by the side providing `free`.
        unsafe { free(self) };
        out
    }
}

/// Bytes returned across the boundary. The receiver releases them with the
/// `free_bytes` slot of the table of the side that allocated them. A `free_bytes`
/// accepts a null `ptr` and does nothing for it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaOwnedBytes {
    /// First byte, or null for an empty buffer that allocates nothing.
    pub ptr: *mut u8,
    /// Number of bytes.
    pub len: usize,
    /// Allocation size in bytes, for the allocating side's `free_bytes`. The receiver
    /// does not read it.
    pub cap: usize,
}

impl AriaOwnedBytes {
    /// An empty byte buffer that allocates nothing.
    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    /// Transfer ownership of a Rust [`Vec<u8>`] into an [`AriaOwnedBytes`].
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        if bytes.capacity() == 0 {
            return Self::empty();
        }
        let mut bytes = ManuallyDrop::new(bytes);
        Self {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        }
    }

    /// Copy the buffer into a [`Vec<u8>`] and release it with `free`.
    ///
    /// # Safety
    ///
    /// `self` must have been produced by the side whose `free` function is passed.
    #[allow(unsafe_code)]
    pub unsafe fn into_vec_with(self, free: unsafe extern "C" fn(AriaOwnedBytes)) -> Vec<u8> {
        let out = if self.len == 0 || self.ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: `self.ptr` points to `self.len` valid bytes until `free` runs.
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }.to_vec()
        };
        // SAFETY: `self` was allocated by the side providing `free`.
        unsafe { free(self) };
        out
    }
}

/// Free an [`AriaOwnedStr`] allocated by Rust's [`AriaOwnedStr::from_string`].
///
/// # Safety
///
/// `text` must have been created by [`AriaOwnedStr::from_string`] in the same
/// dynamic object, or be [`AriaOwnedStr::empty`].
#[allow(unsafe_code)]
pub unsafe extern "C" fn free_rust_string(text: AriaOwnedStr) {
    if !text.ptr.is_null() && text.cap > 0 {
        // SAFETY: `ptr`, `len`, and `cap` came from `String::into_bytes` in this module.
        drop(unsafe { Vec::from_raw_parts(text.ptr, text.len, text.cap) });
    }
}

/// Free an [`AriaOwnedBytes`] allocated by Rust's [`AriaOwnedBytes::from_vec`].
///
/// # Safety
///
/// `bytes` must have been created by [`AriaOwnedBytes::from_vec`] in the same
/// dynamic object, or be [`AriaOwnedBytes::empty`].
#[allow(unsafe_code)]
pub unsafe extern "C" fn free_rust_bytes(bytes: AriaOwnedBytes) {
    if !bytes.ptr.is_null() && bytes.cap > 0 {
        // SAFETY: `ptr`, `len`, and `cap` came from `Vec<u8>` in this module.
        drop(unsafe { Vec::from_raw_parts(bytes.ptr, bytes.len, bytes.cap) });
    }
}

/// Result of a call that returns text: a JSON document, a message, or nothing.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaResult {
    /// An `ARIA_STATUS_*` value.
    pub status: u32,
    /// The result on [`ARIA_STATUS_OK`], empty when the call returns nothing, and the
    /// message on any other status.
    pub value: AriaOwnedStr,
}

impl AriaResult {
    /// Successful unit result with no allocated payload.
    pub const fn ok_empty() -> Self {
        Self {
            status: ARIA_STATUS_OK,
            value: AriaOwnedStr::empty(),
        }
    }

    /// Successful result carrying `value`.
    pub fn ok(value: String) -> Self {
        Self {
            status: ARIA_STATUS_OK,
            value: AriaOwnedStr::from_string(value),
        }
    }

    /// Normal error result carrying `message`.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_STATUS_ERR,
            value: AriaOwnedStr::from_string(message.into()),
        }
    }

    /// Caught panic result carrying `message`.
    pub fn panic(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_STATUS_PANIC,
            value: AriaOwnedStr::from_string(message.into()),
        }
    }

    /// A decoder call on a file the decoder does not read.
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_STATUS_UNSUPPORTED,
            value: AriaOwnedStr::from_string(message.into()),
        }
    }
}

/// Result of a call that returns bytes: `storage_get`, `panel_view` and
/// `overlay_scene`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaBytesResult {
    /// An `ARIA_BYTES_*` value.
    pub status: u32,
    /// The result on [`ARIA_BYTES_FOUND`], otherwise empty.
    pub bytes: AriaOwnedBytes,
    /// The message on [`ARIA_BYTES_ERR`] and [`ARIA_BYTES_PANIC`], otherwise empty.
    pub error: AriaOwnedStr,
}

impl AriaBytesResult {
    /// Success with `bytes`.
    pub fn found(bytes: Vec<u8>) -> Self {
        Self {
            status: ARIA_BYTES_FOUND,
            bytes: AriaOwnedBytes::from_vec(bytes),
            error: AriaOwnedStr::empty(),
        }
    }

    /// Key was not found (`Ok(None)`).
    pub const fn not_found() -> Self {
        Self {
            status: ARIA_BYTES_NOT_FOUND,
            bytes: AriaOwnedBytes::empty(),
            error: AriaOwnedStr::empty(),
        }
    }

    /// Failure with `message`.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_BYTES_ERR,
            bytes: AriaOwnedBytes::empty(),
            error: AriaOwnedStr::from_string(message.into()),
        }
    }

    /// Caught panic with `message`.
    pub fn panic(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_BYTES_PANIC,
            bytes: AriaOwnedBytes::empty(),
            error: AriaOwnedStr::from_string(message.into()),
        }
    }
}

/// Result of a call that creates an instance the host addresses in later calls.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaInstanceResult {
    /// An `ARIA_INSTANCE_*` value.
    pub status: u32,
    /// The new instance's id on [`ARIA_INSTANCE_OK`], otherwise 0. The side that creates
    /// the instance picks the id, and the other side passes it back unchanged.
    pub instance: u64,
    /// The message on any status but [`ARIA_INSTANCE_OK`], otherwise empty.
    pub error: AriaOwnedStr,
}

impl AriaInstanceResult {
    /// Success with the new instance id `instance`.
    pub const fn ok(instance: u64) -> Self {
        Self {
            status: ARIA_STATUS_OK,
            instance,
            error: AriaOwnedStr::empty(),
        }
    }

    /// Failure with `message`.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_STATUS_ERR,
            instance: 0,
            error: AriaOwnedStr::from_string(message.into()),
        }
    }

    /// Caught panic with `message`.
    pub fn panic(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_STATUS_PANIC,
            instance: 0,
            error: AriaOwnedStr::from_string(message.into()),
        }
    }

    /// A decoder open on a file the decoder does not read.
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_INSTANCE_UNSUPPORTED,
            instance: 0,
            error: AriaOwnedStr::from_string(message.into()),
        }
    }
}

/// One parameter of an audio effect across one [`AriaAudioBlock`]: a constant, or one
/// value per frame when the parameter is automated.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaAudioParam {
    /// The value for every frame of the block when `per_frame` is null. 0 when the
    /// parameter has no value.
    pub constant: f32,
    /// One value per frame, or null when the parameter is constant.
    pub per_frame: *const f32,
    /// Number of values at `per_frame`: the block's `frames`, or 0.
    pub per_frame_len: usize,
}

/// One stereo block `audio_process` processes in place.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaAudioBlock {
    /// `frames` samples of the left channel, read and overwritten in place.
    pub left: *mut f32,
    /// `frames` samples of the right channel, read and overwritten in place.
    pub right: *mut f32,
    /// Frames in the block.
    pub frames: usize,
    /// Timeline position of the block's first frame, in frames at `sample_rate`.
    /// Consecutive blocks continue each other until the next `audio_reset`.
    pub position: i64,
    /// Frames per second.
    pub sample_rate: u32,
    /// 1 when `sidechain_left` and `sidechain_right` hold the key signal, otherwise 0.
    pub has_sidechain: u32,
    /// `frames` samples of the key signal's left channel, or null. A silent or muted
    /// key lane reads as zeros.
    pub sidechain_left: *const f32,
    /// `frames` samples of the key signal's right channel, or null.
    pub sidechain_right: *const f32,
}

/// The export settings `encoder_open` receives.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaEncoderConfig {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Frame rate numerator: `fps_num / fps_den` frames per second.
    pub fps_num: i64,
    /// Frame rate denominator.
    pub fps_den: i32,
    /// Target video bitrate in kbit/s, or 0 when the export sets none.
    pub video_bitrate_kbps: u32,
    /// Target audio bitrate in kbit/s. The host sends 0, for no target.
    pub audio_bitrate_kbps: u32,
}

/// One export frame `encoder_push_frame` receives.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaEncoderFrame {
    /// Straight-alpha RGBA8 pixels, `height` rows of `stride` bytes, top row first.
    pub rgba: *const u8,
    /// Number of bytes at `rgba`.
    pub rgba_len: usize,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Bytes per row, at least `width * 4`.
    pub stride: u32,
    /// Presentation time in ticks of `pts_rate` per second.
    pub pts_value: i64,
    /// Ticks per second of `pts_value`.
    pub pts_rate: i32,
}

/// One block of export audio `encoder_push_audio` receives.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaEncoderAudio {
    /// Interleaved samples, `channels` per frame.
    pub samples: *const f32,
    /// Number of samples at `samples`, a multiple of `channels`.
    pub samples_len: usize,
    /// Frames per second.
    pub sample_rate: u32,
    /// Channels per frame.
    pub channels: u32,
    /// Presentation time of the first frame, in ticks of `pts_rate` per second.
    pub pts_value: i64,
    /// Ticks per second of `pts_value`.
    pub pts_rate: i32,
}

/// Progress a plugin reports from `job_step`. The host sets `progress` to -1 and
/// `done` and `commit` to 0 before each call. The `AriaResult` value of `job_step` is
/// the detail line.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaJobStep {
    /// Fraction done, from 0 to 1. A value below 0, or NaN, reports progress as
    /// unknown, and a value above 1 reads as 1.
    pub progress: f32,
    /// 1 when the job has finished, otherwise 0.
    pub done: u32,
    /// 1 when the step starts the job's commit, after which Cancel no longer stops it.
    pub commit: u32,
}

/// What `decoder_open_video` reports about the stream it opened. The host fills the
/// struct with zeros before the call and refuses a stream whose values break the
/// rules below.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AriaVideoInfo {
    /// Frame width in pixels: even, from 2 to 16384.
    pub width: u32,
    /// Frame height in pixels: even, from 2 to 16384.
    pub height: u32,
    /// An `ARIA_FORMAT_*` value.
    pub format: u32,
    /// An `ARIA_COLOR_*` value.
    pub color: u32,
    /// Frame rate numerator: `frame_rate_num / frame_rate_den` frames per second, positive.
    pub frame_rate_num: i32,
    /// Frame rate denominator, positive.
    pub frame_rate_den: i32,
    /// Time base numerator: every pts the stream reports counts ticks at
    /// `time_base_num / time_base_den` ticks per second, positive.
    pub time_base_num: i32,
    /// Time base denominator, positive.
    pub time_base_den: i32,
    /// Length of the stream in time base ticks, or negative when unknown.
    pub duration: i64,
}

/// One plane the host allocates for `decoder_next_frame` to fill.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaFramePlane {
    /// `len` writable bytes, zeroed.
    pub data: *mut u8,
    /// Number of bytes at `data`: `stride * rows`.
    pub len: usize,
    /// Bytes per row: the frame width, or half of it for the U and V planes of
    /// [`ARIA_FORMAT_YUV420P`].
    pub stride: u32,
    /// Rows: the frame height for the Y plane, half of it for the others.
    pub rows: u32,
}

/// The frame `decoder_next_frame` writes. The plugin fills the planes, sets `pts`, and
/// sets `produced` to 1, or leaves `produced` at 0 at end of stream.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaVideoFrameOut {
    /// The planes of the stream's format, in plane order: Y then UV for
    /// [`ARIA_FORMAT_NV12`], Y then U then V for [`ARIA_FORMAT_YUV420P`].
    pub planes: *mut AriaFramePlane,
    /// Number of planes at `planes`.
    pub planes_len: usize,
    /// Presentation time of the frame in time base ticks. The host sets 0.
    pub pts: i64,
    /// 1 when the plugin wrote a frame, 0 at end of stream. The host sets 0.
    pub produced: u32,
}

/// The frame `scope_measure` reads and the image it draws.
///
/// `frame` holds `frame_width * frame_height` RGBA8 pixels in `frame_len`
/// bytes: sRGB, straight alpha, top row first, no row padding. `image` holds
/// `image_width * image_height` pixels of the same layout in `image_len` bytes,
/// transparent black on entry, at the size the scope registered.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaScopeCall {
    /// Pixels of the measured frame.
    pub frame: *const u8,
    /// Number of bytes at `frame`.
    pub frame_len: usize,
    /// Width of the measured frame in pixels.
    pub frame_width: u32,
    /// Height of the measured frame in pixels.
    pub frame_height: u32,
    /// Pixels of the scope image, which the plugin overwrites.
    pub image: *mut u8,
    /// Number of bytes at `image`. The plugin cannot resize the image.
    pub image_len: usize,
    /// Width of the scope image in pixels, as registered.
    pub image_width: u32,
    /// Height of the scope image in pixels, as registered.
    pub image_height: u32,
}

/// The frame and parameters `cpu_effect_process` reads and the output frame it writes.
///
/// `input` and `output` each hold `width * height` premultiplied RGBA16F pixels
/// in `pixels_len` 16-bit channel elements (`width * height * 4`), each an IEEE-754
/// binary16 value, top row first, with no row padding. `params` holds `params_len`
/// flattened 32-bit float values in catalog order (1 per scalar, 2 per vec2, 4 per
/// colour with each channel from 0 to 1).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaCpuEffectCall {
    /// The source frame, or a transparent frame for a generator.
    pub input: *const u16,
    /// The frame the plugin writes, zeroed on entry. The host reads it only when the
    /// call returns [`ARIA_STATUS_OK`].
    pub output: *mut u16,
    /// Number of 16-bit elements at `input` and at `output`.
    pub pixels_len: usize,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Flattened parameter values.
    pub params: *const f32,
    /// Number of values at `params`.
    pub params_len: usize,
    /// The effect instance being rendered, serialized as JSON, or empty when there is
    /// none.
    pub params_json: AriaStr,
    /// Seconds since the clip's first frame.
    pub time: f32,
}

/// One parameter value passed to or returned from `driver_evaluate`. `kind` selects
/// the fields that hold it; the others are 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AriaParamValue {
    /// An `ARIA_PARAM_*` value.
    pub kind: u32,
    /// First component of a scalar, vector or rect.
    pub x: f32,
    /// Second component of a vector or rect.
    pub y: f32,
    /// Third component of a 3-d vector, or the width of a rect.
    pub z: f32,
    /// Height of a rect.
    pub w: f32,
    /// Red channel of a colour.
    pub r: u8,
    /// Green channel of a colour.
    pub g: u8,
    /// Blue channel of a colour.
    pub b: u8,
    /// Alpha channel of a colour.
    pub a: u8,
}

/// What `driver_evaluate` reads and the value it writes. Times are sequence seconds.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaDriverEvalCall {
    /// The time being evaluated.
    pub time: f64,
    /// Seconds since the first frame of the driver's clip.
    pub clip_time: f64,
    /// Frames per second of the sequence.
    pub frame_rate: f64,
    /// Length of the sequence in seconds.
    pub duration: f64,
    /// Canvas width in pixels.
    pub canvas_width: u32,
    /// Canvas height in pixels.
    pub canvas_height: u32,
    /// The property's own keyframed value at `time`.
    pub value: AriaParamValue,
    /// A number fixed for this clip and property, for seeded noise.
    pub seed: u64,
    /// The value the plugin writes. The host sets it to `value` before the call, and
    /// its `kind` must stay the same.
    pub output: AriaParamValue,
}

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
    /// result refuses the scene with the error.
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

/// Smallest valid [`AriaHostV1`]: the byte size of every slot through `project_undo`.
pub const ARIA_HOST_V1_MIN_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_panel);

/// Smallest valid [`AriaPluginV1`]: the byte size of every slot through `audio_drop`.
pub const ARIA_PLUGIN_V1_MIN_SIZE: usize = std::mem::offset_of!(AriaPluginV1, panel_view);

/// Byte size of an [`AriaHostV1`] through `refresh_panel`.
pub const ARIA_HOST_V1_PANELS_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_encoder);

/// Byte size of an [`AriaPluginV1`] through `panel_event`.
pub const ARIA_PLUGIN_V1_PANELS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, encoder_open);

/// Byte size of an [`AriaHostV1`] through `register_encoder`.
pub const ARIA_HOST_V1_ENCODERS_SIZE: usize =
    std::mem::offset_of!(AriaHostV1, register_viewer_overlay);

/// Byte size of an [`AriaPluginV1`] through `encoder_drop`.
pub const ARIA_PLUGIN_V1_ENCODERS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, overlay_scene);

/// Byte size of an [`AriaHostV1`] through `refresh_viewer_overlay`.
pub const ARIA_HOST_V1_OVERLAYS_SIZE: usize = std::mem::offset_of!(AriaHostV1, start_job);

/// Byte size of an [`AriaPluginV1`] through `overlay_event`.
pub const ARIA_PLUGIN_V1_OVERLAYS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, job_step);

/// Byte size of an [`AriaHostV1`] through `start_job`.
pub const ARIA_HOST_V1_JOBS_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_decoder);

/// Byte size of an [`AriaPluginV1`] through `job_end`.
pub const ARIA_PLUGIN_V1_JOBS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, decoder_probe);

/// Byte size of an [`AriaHostV1`] through `register_decoder`.
pub const ARIA_HOST_V1_DECODERS_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_ai_tool);

/// Byte size of an [`AriaPluginV1`] through `decoder_close`.
pub const ARIA_PLUGIN_V1_DECODERS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, ai_tool_call);

/// Byte size of an [`AriaHostV1`] through `register_ai_tool`.
pub const ARIA_HOST_V1_AI_TOOLS_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_scope);

/// Byte size of an [`AriaPluginV1`] through `ai_tool_call`.
pub const ARIA_PLUGIN_V1_AI_TOOLS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, scope_measure);

/// Byte size of an [`AriaHostV1`] through `register_scope`.
pub const ARIA_HOST_V1_SCOPES_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_effect);

/// Byte size of an [`AriaPluginV1`] through `scope_measure`.
pub const ARIA_PLUGIN_V1_SCOPES_SIZE: usize =
    std::mem::offset_of!(AriaPluginV1, cpu_effect_process);

/// Byte size of an [`AriaHostV1`] through `register_analyzer`.
pub const ARIA_HOST_V1_CONTRIBUTIONS_SIZE: usize = std::mem::offset_of!(AriaHostV1, clip);

/// Byte size of an [`AriaHostV1`] through `in_out`.
pub const ARIA_HOST_V1_QUERIES_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_texture);

#[allow(unsafe_code)]
unsafe extern "C" fn stub_panel_view(
    _host_ctx: *mut c_void,
    _panel_id: AriaStr,
) -> AriaBytesResult {
    AriaBytesResult::err("plugin table does not export panel_view")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_panel_event(
    _host_ctx: *mut c_void,
    _panel_id: AriaStr,
    _event_json: AriaStr,
) -> AriaResult {
    AriaResult::err("plugin table does not export panel_event")
}
#[allow(unsafe_code)]
unsafe extern "C" fn stub_encoder_open(
    _host_ctx: *mut c_void,
    _encoder_id: AriaStr,
    _path: AriaStr,
    _config: *const AriaEncoderConfig,
) -> AriaInstanceResult {
    AriaInstanceResult::err("plugin table does not export encoder_open")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_encoder_push_frame(
    _host_ctx: *mut c_void,
    _instance: u64,
    _frame: *const AriaEncoderFrame,
) -> AriaResult {
    AriaResult::err("plugin table does not export encoder_push_frame")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_encoder_push_audio(
    _host_ctx: *mut c_void,
    _instance: u64,
    _audio: *const AriaEncoderAudio,
) -> AriaResult {
    AriaResult::err("plugin table does not export encoder_push_audio")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_encoder_finish(_host_ctx: *mut c_void, _instance: u64) -> AriaResult {
    AriaResult::err("plugin table does not export encoder_finish")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_encoder_drop(_host_ctx: *mut c_void, _instance: u64) -> AriaResult {
    AriaResult::err("plugin table does not export encoder_drop")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_overlay_scene(
    _host_ctx: *mut c_void,
    _overlay_id: AriaStr,
) -> AriaBytesResult {
    AriaBytesResult::err("plugin table does not export overlay_scene")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_overlay_event(
    _host_ctx: *mut c_void,
    _overlay_id: AriaStr,
    _event_json: AriaStr,
) -> AriaResult {
    AriaResult::err("plugin table does not export overlay_event")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_job_step(
    _host_ctx: *mut c_void,
    _job: u64,
    _step: *mut AriaJobStep,
) -> AriaResult {
    AriaResult::err("plugin table does not export job_step")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_job_end(
    _host_ctx: *mut c_void,
    _job: u64,
    _cancelled: u32,
) -> AriaResult {
    AriaResult::err("plugin table does not export job_end")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_probe(
    _host_ctx: *mut c_void,
    _decoder_id: AriaStr,
    _path: AriaStr,
) -> AriaResult {
    AriaResult::err("plugin table does not export decoder_probe")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_open_video(
    _host_ctx: *mut c_void,
    _decoder_id: AriaStr,
    _path: AriaStr,
    _info: *mut AriaVideoInfo,
) -> AriaInstanceResult {
    AriaInstanceResult::err("plugin table does not export decoder_open_video")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_seek(
    _host_ctx: *mut c_void,
    _instance: u64,
    _pts: i64,
) -> AriaResult {
    AriaResult::err("plugin table does not export decoder_seek")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_next_frame(
    _host_ctx: *mut c_void,
    _instance: u64,
    _frame: *mut AriaVideoFrameOut,
) -> AriaResult {
    AriaResult::err("plugin table does not export decoder_next_frame")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_open_audio(
    _host_ctx: *mut c_void,
    _decoder_id: AriaStr,
    _path: AriaStr,
    _sample_rate: u32,
    _channels: u32,
) -> AriaInstanceResult {
    AriaInstanceResult::err("plugin table does not export decoder_open_audio")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_read_audio(
    _host_ctx: *mut c_void,
    _instance: u64,
    _out: *mut f32,
    _out_len: usize,
    _written: *mut usize,
) -> AriaResult {
    AriaResult::err("plugin table does not export decoder_read_audio")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_seek_audio(
    _host_ctx: *mut c_void,
    _instance: u64,
    _frame: i64,
) -> AriaResult {
    AriaResult::err("plugin table does not export decoder_seek_audio")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_close(_host_ctx: *mut c_void, _instance: u64) -> AriaResult {
    AriaResult::err("plugin table does not export decoder_close")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_ai_tool_call(
    _host_ctx: *mut c_void,
    _name: AriaStr,
    _arguments_json: AriaStr,
) -> AriaResult {
    AriaResult::err("plugin table does not export ai_tool_call")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_scope_measure(
    _host_ctx: *mut c_void,
    _scope_id: AriaStr,
    _call: *mut AriaScopeCall,
) -> AriaResult {
    AriaResult::err("plugin table does not export scope_measure")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_cpu_effect_process(
    _host_ctx: *mut c_void,
    _effect_id: AriaStr,
    _call: *mut AriaCpuEffectCall,
) -> AriaResult {
    AriaResult::err("plugin table does not export cpu_effect_process")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_driver_evaluate(
    _host_ctx: *mut c_void,
    _language_id: AriaStr,
    _expr: AriaStr,
    _call: *mut AriaDriverEvalCall,
) -> AriaResult {
    AriaResult::err("plugin table does not export driver_evaluate")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_analyzer_open(
    _host_ctx: *mut c_void,
    _analyzer_id: AriaStr,
    _sample_rate: u32,
    _channels: u32,
) -> AriaInstanceResult {
    AriaInstanceResult::err("plugin table does not export analyzer_open")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_analyzer_push(
    _host_ctx: *mut c_void,
    _instance: u64,
    _samples: *const f32,
    _samples_len: usize,
) -> AriaResult {
    AriaResult::err("plugin table does not export analyzer_push")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_analyzer_finish(_host_ctx: *mut c_void, _instance: u64) -> AriaResult {
    AriaResult::err("plugin table does not export analyzer_finish")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_analyzer_drop(_host_ctx: *mut c_void, _instance: u64) -> AriaResult {
    AriaResult::err("plugin table does not export analyzer_drop")
}

impl AriaHostV1 {
    /// Whether the table holds `register_panel` and `refresh_panel`.
    pub const fn has_panels(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_PANELS_SIZE
    }

    /// Whether the table holds `register_encoder`.
    pub const fn has_encoders(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_ENCODERS_SIZE
    }

    /// Whether the table holds `register_viewer_overlay` and `refresh_viewer_overlay`.
    pub const fn has_overlays(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_OVERLAYS_SIZE
    }

    /// Whether the table holds `start_job`.
    pub const fn has_jobs(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_JOBS_SIZE
    }

    /// Whether the table holds `register_decoder`.
    pub const fn has_decoders(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_DECODERS_SIZE
    }

    /// Whether the table holds `register_ai_tool`.
    pub const fn has_ai_tools(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_AI_TOOLS_SIZE
    }

    /// Whether the table holds `register_scope`.
    pub const fn has_scopes(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_SCOPES_SIZE
    }

    /// Whether the table holds the contribution calls, `register_effect` through
    /// `register_analyzer`.
    pub const fn has_contributions(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_CONTRIBUTIONS_SIZE
    }

    /// Whether the table holds the project queries, `clip` through `in_out`.
    pub const fn has_project_queries(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_QUERIES_SIZE
    }

    /// Whether the table holds `register_texture`.
    pub const fn has_textures(&self) -> bool {
        self.struct_size >= std::mem::size_of::<Self>()
    }
}

impl AriaPluginV1 {
    /// Whether the table holds `panel_view` and `panel_event`.
    pub const fn has_panels(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_PANELS_SIZE
    }

    /// Whether the table holds the encoder callbacks, `encoder_open` through
    /// `encoder_drop`.
    pub const fn has_encoders(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_ENCODERS_SIZE
    }

    /// Whether the table holds `overlay_scene` and `overlay_event`.
    pub const fn has_overlays(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_OVERLAYS_SIZE
    }

    /// Whether the table holds `job_step` and `job_end`.
    pub const fn has_jobs(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_JOBS_SIZE
    }

    /// Whether the table holds the decoder callbacks, `decoder_probe` through
    /// `decoder_close`.
    pub const fn has_decoders(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_DECODERS_SIZE
    }

    /// Whether the table holds `ai_tool_call`.
    pub const fn has_ai_tools(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_AI_TOOLS_SIZE
    }

    /// Whether the table holds `scope_measure`.
    pub const fn has_scopes(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_SCOPES_SIZE
    }

    /// Whether the table holds the contribution callbacks, `cpu_effect_process`,
    /// `driver_evaluate`, and the analyzer callbacks.
    pub const fn has_contributions(&self) -> bool {
        self.struct_size >= std::mem::size_of::<Self>()
    }

    /// Read a validated [`AriaPluginV1`] from `ptr`. When `struct_size` is smaller than
    /// [`AriaPluginV1`], each slot group the prefix does not cover holds stubs that
    /// return [`ARIA_STATUS_ERR`], [`ARIA_BYTES_ERR`] or [`ARIA_INSTANCE_ERR`]: the
    /// panel, encoder, overlay, job, decoder, AI tool, scope, and contribution groups.
    ///
    /// # Safety
    ///
    /// `ptr` must point to at least `struct_size` valid bytes of an `AriaPluginV1` prefix,
    /// where `struct_size >= ARIA_PLUGIN_V1_MIN_SIZE`.
    #[allow(unsafe_code)]
    pub unsafe fn copy_from_prefix(ptr: *const Self, struct_size: usize) -> Self {
        if struct_size >= std::mem::size_of::<Self>() {
            // SAFETY: The caller guarantees `ptr` holds at least `size_of::<Self>()` bytes.
            return unsafe { *ptr };
        }
        let mut fallback = std::mem::MaybeUninit::<Self>::zeroed();
        let copy_len = if struct_size >= ARIA_PLUGIN_V1_SCOPES_SIZE {
            ARIA_PLUGIN_V1_SCOPES_SIZE
        } else if struct_size >= ARIA_PLUGIN_V1_AI_TOOLS_SIZE {
            ARIA_PLUGIN_V1_AI_TOOLS_SIZE
        } else if struct_size >= ARIA_PLUGIN_V1_DECODERS_SIZE {
            ARIA_PLUGIN_V1_DECODERS_SIZE
        } else if struct_size >= ARIA_PLUGIN_V1_JOBS_SIZE {
            ARIA_PLUGIN_V1_JOBS_SIZE
        } else if struct_size >= ARIA_PLUGIN_V1_OVERLAYS_SIZE {
            ARIA_PLUGIN_V1_OVERLAYS_SIZE
        } else if struct_size >= ARIA_PLUGIN_V1_ENCODERS_SIZE {
            ARIA_PLUGIN_V1_ENCODERS_SIZE
        } else if struct_size >= ARIA_PLUGIN_V1_PANELS_SIZE {
            ARIA_PLUGIN_V1_PANELS_SIZE
        } else {
            ARIA_PLUGIN_V1_MIN_SIZE
        };
        // SAFETY: Copying the validated prefix into `fallback`, then initializing missing trailing fn pointers.
        unsafe {
            std::ptr::copy_nonoverlapping(
                ptr.cast::<u8>(),
                fallback.as_mut_ptr().cast::<u8>(),
                copy_len,
            );
            let dst = fallback.as_mut_ptr();
            if struct_size < ARIA_PLUGIN_V1_PANELS_SIZE {
                (*dst).panel_view = stub_panel_view;
                (*dst).panel_event = stub_panel_event;
            }
            if struct_size < ARIA_PLUGIN_V1_ENCODERS_SIZE {
                (*dst).encoder_open = stub_encoder_open;
                (*dst).encoder_push_frame = stub_encoder_push_frame;
                (*dst).encoder_push_audio = stub_encoder_push_audio;
                (*dst).encoder_finish = stub_encoder_finish;
                (*dst).encoder_drop = stub_encoder_drop;
            }
            if struct_size < ARIA_PLUGIN_V1_OVERLAYS_SIZE {
                (*dst).overlay_scene = stub_overlay_scene;
                (*dst).overlay_event = stub_overlay_event;
            }
            if struct_size < ARIA_PLUGIN_V1_JOBS_SIZE {
                (*dst).job_step = stub_job_step;
                (*dst).job_end = stub_job_end;
            }
            if struct_size < ARIA_PLUGIN_V1_DECODERS_SIZE {
                (*dst).decoder_probe = stub_decoder_probe;
                (*dst).decoder_open_video = stub_decoder_open_video;
                (*dst).decoder_seek = stub_decoder_seek;
                (*dst).decoder_next_frame = stub_decoder_next_frame;
                (*dst).decoder_open_audio = stub_decoder_open_audio;
                (*dst).decoder_read_audio = stub_decoder_read_audio;
                (*dst).decoder_seek_audio = stub_decoder_seek_audio;
                (*dst).decoder_close = stub_decoder_close;
            }
            if struct_size < ARIA_PLUGIN_V1_AI_TOOLS_SIZE {
                (*dst).ai_tool_call = stub_ai_tool_call;
            }
            if struct_size < ARIA_PLUGIN_V1_SCOPES_SIZE {
                (*dst).scope_measure = stub_scope_measure;
            }
            (*dst).cpu_effect_process = stub_cpu_effect_process;
            (*dst).driver_evaluate = stub_driver_evaluate;
            (*dst).analyzer_open = stub_analyzer_open;
            (*dst).analyzer_push = stub_analyzer_push;
            (*dst).analyzer_finish = stub_analyzer_finish;
            (*dst).analyzer_drop = stub_analyzer_drop;
            fallback.assume_init()
        }
    }
}

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
mod tests;
