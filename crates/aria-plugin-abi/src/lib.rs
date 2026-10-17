//! Stable C ABI types and versioned function tables shared by the Aria host and native plugins.
//!
//! Host and guest communicate through two versioned function tables, [`AriaHostV1`]
//! and [`AriaPluginV1`], whose first fields are `struct_size` and `api_major`.
//! Strings and byte buffers returned across the boundary carry explicit free
//! callbacks on the table of the side that allocated them.

use std::ffi::c_void;
use std::mem::ManuallyDrop;

/// Current major version of the native plugin C ABI.
pub const ARIA_API_MAJOR: u32 = 1;

/// Symbol name a native plugin library exports as its entry point.
pub const ARIA_PLUGIN_ENTRY_SYMBOL: &[u8] = b"aria_plugin_entry";

/// Status code for a successful call.
pub const ARIA_STATUS_OK: u32 = 0;
/// Status code for a call that returned a normal error string.
pub const ARIA_STATUS_ERR: u32 = 1;
/// Status code for a call where the guest panicked and `catch_unwind` caught it.
pub const ARIA_STATUS_PANIC: u32 = 2;
/// Status code for a decoder call on a file the decoder does not read. The
/// host tries the next decoder without reporting the message.
pub const ARIA_STATUS_UNSUPPORTED: u32 = 3;

/// Status code for a successful `audio_create` call.
pub const ARIA_INSTANCE_OK: u32 = ARIA_STATUS_OK;
/// Status code for an `audio_create` call that failed with an error string.
pub const ARIA_INSTANCE_ERR: u32 = ARIA_STATUS_ERR;
/// Status code for an `audio_create` call where the guest panicked.
pub const ARIA_INSTANCE_PANIC: u32 = ARIA_STATUS_PANIC;
/// Status code for a decoder open on a file the decoder does not read.
pub const ARIA_INSTANCE_UNSUPPORTED: u32 = ARIA_STATUS_UNSUPPORTED;

/// Status code when `storage_get` found the requested key.
pub const ARIA_BYTES_FOUND: u32 = 0;
/// Status code when `storage_get` did not find the requested key.
pub const ARIA_BYTES_NOT_FOUND: u32 = 1;
/// Status code when `storage_get` failed with an error string.
pub const ARIA_BYTES_ERR: u32 = 2;
/// Status code when a bytes-returning callback (`panel_view`) panicked.
pub const ARIA_BYTES_PANIC: u32 = 3;

/// Trace log level.
pub const ARIA_LOG_TRACE: u32 = 0;
/// Debug log level.
pub const ARIA_LOG_DEBUG: u32 = 1;
/// Info log level.
pub const ARIA_LOG_INFO: u32 = 2;
/// Warn log level.
pub const ARIA_LOG_WARN: u32 = 3;
/// Error log level.
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

/// [`AriaParamValue::kind`]: scalar `f32` in `x`.
pub const ARIA_PARAM_SCALAR: u32 = 0;
/// [`AriaParamValue::kind`]: 2-d vector `[x, y]`.
pub const ARIA_PARAM_VEC2: u32 = 1;
/// [`AriaParamValue::kind`]: RGBA8 colour `[r, g, b, a]`.
pub const ARIA_PARAM_COLOR: u32 = 2;
/// [`AriaParamValue::kind`]: axis-aligned rect `[x, y, z, w]`.
pub const ARIA_PARAM_RECT: u32 = 3;
/// [`AriaParamValue::kind`]: 3-d vector `[x, y, z]`.
pub const ARIA_PARAM_VEC3: u32 = 4;

/// Borrowed UTF-8 string slice valid for the duration of an FFI call.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaStr {
    pub ptr: *const u8,
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

/// Borrowed byte slice valid for the duration of an FFI call.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaBytes {
    pub ptr: *const u8,
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

/// Owned UTF-8 string returned across the ABI boundary and freed by the
/// allocating side's `free_string` callback.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaOwnedStr {
    pub ptr: *mut u8,
    pub len: usize,
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

/// Owned byte buffer returned across the ABI boundary and freed by the
/// allocating side's `free_bytes` callback.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaOwnedBytes {
    pub ptr: *mut u8,
    pub len: usize,
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

/// Result of a fallible ABI call returning an optional or JSON string payload.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaResult {
    pub status: u32,
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

/// Result of `storage_get`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaBytesResult {
    pub status: u32,
    pub bytes: AriaOwnedBytes,
    pub error: AriaOwnedStr,
}

impl AriaBytesResult {
    /// Key was found with `bytes`.
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

    /// Storage lookup failed with `message`.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_BYTES_ERR,
            bytes: AriaOwnedBytes::empty(),
            error: AriaOwnedStr::from_string(message.into()),
        }
    }

    /// Callback panicked with `message`.
    pub fn panic(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_BYTES_PANIC,
            bytes: AriaOwnedBytes::empty(),
            error: AriaOwnedStr::from_string(message.into()),
        }
    }
}

/// Result of `audio_create`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaInstanceResult {
    pub status: u32,
    pub instance: u64,
    pub error: AriaOwnedStr,
}

impl AriaInstanceResult {
    /// Successfully created audio processor instance `instance`.
    pub const fn ok(instance: u64) -> Self {
        Self {
            status: ARIA_STATUS_OK,
            instance,
            error: AriaOwnedStr::empty(),
        }
    }

    /// Normal error creating an audio processor instance.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            status: ARIA_STATUS_ERR,
            instance: 0,
            error: AriaOwnedStr::from_string(message.into()),
        }
    }

    /// Caught panic while creating an audio processor instance.
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

/// One parameter signal across an audio block.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaAudioParam {
    pub constant: f32,
    pub per_frame: *const f32,
    pub per_frame_len: usize,
}

/// One stereo planar audio block passed to `audio_process`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaAudioBlock {
    pub left: *mut f32,
    pub right: *mut f32,
    pub frames: usize,
    pub position: i64,
    pub sample_rate: u32,
    pub has_sidechain: u32,
    pub sidechain_left: *const f32,
    pub sidechain_right: *const f32,
}

/// Configuration passed to `encoder_open`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaEncoderConfig {
    pub width: u32,
    pub height: u32,
    pub fps_num: i64,
    pub fps_den: i32,
    pub video_bitrate_kbps: u32,
    pub audio_bitrate_kbps: u32,
}

/// One RGBA video frame passed to `encoder_push_frame`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaEncoderFrame {
    pub rgba: *const u8,
    pub rgba_len: usize,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pts_value: i64,
    pub pts_rate: i32,
}

/// One interleaved audio block passed to `encoder_push_audio`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaEncoderAudio {
    pub samples: *const f32,
    pub samples_len: usize,
    pub sample_rate: u32,
    pub channels: u32,
    pub pts_value: i64,
    pub pts_rate: i32,
}

/// Progress a plugin reports from `job_step`.
///
/// `progress` below zero or NaN reports indeterminate progress; values above 1 clamp to 1.
/// `done` and `commit` are 0 or 1. The detail line is the `AriaResult` value.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaJobStep {
    pub progress: f32,
    pub done: u32,
    pub commit: u32,
}

/// What `decoder_open_video` reports about the stream it opened.
///
/// `width` and `height` are even. `format` is an `ARIA_FORMAT_*` value and
/// `color` an `ARIA_COLOR_*` value. Every pts the stream reports counts ticks
/// at a rate of `time_base_num / time_base_den` ticks per second. `duration`
/// is the length in those ticks, or negative when unknown.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AriaVideoInfo {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub color: u32,
    pub frame_rate_num: i32,
    pub frame_rate_den: i32,
    pub time_base_num: i32,
    pub time_base_den: i32,
    pub duration: i64,
}

/// One host-allocated plane `decoder_next_frame` fills: `rows` rows of
/// `stride` bytes in `len` bytes at `data`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaFramePlane {
    pub data: *mut u8,
    pub len: usize,
    pub stride: u32,
    pub rows: u32,
}

/// The frame `decoder_next_frame` writes: the planes the host allocated for
/// the stream's format, in plane order. The plugin fills them, sets `pts`, and
/// sets `produced` to 1, or leaves `produced` at 0 at end of stream.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaVideoFrameOut {
    pub planes: *mut AriaFramePlane,
    pub planes_len: usize,
    pub pts: i64,
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
    pub frame: *const u8,
    pub frame_len: usize,
    pub frame_width: u32,
    pub frame_height: u32,
    pub image: *mut u8,
    pub image_len: usize,
    pub image_width: u32,
    pub image_height: u32,
}

/// The frame and parameters `cpu_effect_process` reads and the output frame it writes.
///
/// `input` and `output` each hold `width * height` premultiplied RGBA16F pixels
/// in `pixels_len` IEEE-754 binary16 `u16` channel elements (`width * height * 4`),
/// top row first, with no row padding. `params` holds `params_len` flattened `f32`
/// values in catalog order (1 per scalar, 2 per vec2, 4 per colour in `0.0..=1.0`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaCpuEffectCall {
    pub input: *const u16,
    pub output: *mut u16,
    pub pixels_len: usize,
    pub width: u32,
    pub height: u32,
    pub params: *const f32,
    pub params_len: usize,
    pub params_json: AriaStr,
    pub time: f32,
}

/// One parameter value (`Scalar`, `Vec2`, `Color`, or `Rect`) passed to or returned
/// from `driver_evaluate`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AriaParamValue {
    pub kind: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Frame variables `driver_evaluate` reads and the output value it writes.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaDriverEvalCall {
    pub time: f64,
    pub clip_time: f64,
    pub frame_rate: f64,
    pub duration: f64,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub value: AriaParamValue,
    pub seed: u64,
    pub output: AriaParamValue,
}

/// Version 1 host import table handed to `aria_plugin_entry`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaHostV1 {
    pub struct_size: usize,
    pub api_major: u32,
    pub reserved: u32,
    pub free_string: unsafe extern "C" fn(str: AriaOwnedStr),
    pub free_bytes: unsafe extern "C" fn(bytes: AriaOwnedBytes),
    pub log: unsafe extern "C" fn(host_ctx: *mut c_void, level: u32, text: AriaStr),
    pub storage_get: unsafe extern "C" fn(host_ctx: *mut c_void, key: AriaStr) -> AriaBytesResult,
    pub storage_set:
        unsafe extern "C" fn(host_ctx: *mut c_void, key: AriaStr, value: AriaBytes) -> AriaResult,
    pub storage_delete: unsafe extern "C" fn(host_ctx: *mut c_void, key: AriaStr) -> AriaResult,
    pub storage_keys: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    pub setting_get: unsafe extern "C" fn(host_ctx: *mut c_void, key: AriaStr) -> AriaResult,
    pub register_command:
        unsafe extern "C" fn(host_ctx: *mut c_void, id: AriaStr, title: AriaStr) -> AriaResult,
    pub subscribe: unsafe extern "C" fn(host_ctx: *mut c_void, event_kind: AriaStr) -> AriaResult,
    pub register_importer:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub register_exporter:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub register_audio_effect:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub project_snapshot: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    pub editor_view: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    pub project_transact: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        label: AriaStr,
        commands_json: AriaStr,
    ) -> AriaResult,
    pub project_undo: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    pub register_panel:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub refresh_panel: unsafe extern "C" fn(host_ctx: *mut c_void, id: AriaStr) -> AriaResult,
    pub register_encoder:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub register_viewer_overlay:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub refresh_viewer_overlay:
        unsafe extern "C" fn(host_ctx: *mut c_void, id: AriaStr) -> AriaResult,
    pub start_job:
        unsafe extern "C" fn(host_ctx: *mut c_void, label: AriaStr) -> AriaInstanceResult,
    pub register_decoder:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub register_ai_tool:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub register_scope:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub register_effect: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        spec_json: AriaStr,
        wgsl: AriaStr,
    ) -> AriaResult,
    pub register_cpu_effect:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub register_transition: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        spec_json: AriaStr,
        wgsl: AriaStr,
    ) -> AriaResult,
    pub register_generator: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        spec_json: AriaStr,
        wgsl: AriaStr,
    ) -> AriaResult,
    pub register_clip_type:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub register_driver_language:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub register_preset:
        unsafe extern "C" fn(host_ctx: *mut c_void, preset_json: AriaStr) -> AriaResult,
    pub register_theme:
        unsafe extern "C" fn(host_ctx: *mut c_void, id: AriaStr, toml: AriaStr) -> AriaResult,
    pub register_workspace:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub register_analyzer:
        unsafe extern "C" fn(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult,
    pub clip: unsafe extern "C" fn(host_ctx: *mut c_void, id: u64) -> AriaResult,
    pub clips_in_range: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        sequence_id: u64,
        range_json: AriaStr,
    ) -> AriaResult,
    pub clips_at: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        sequence_id: u64,
        tick_json: AriaStr,
    ) -> AriaResult,
    pub selected_clips: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    pub markers_in_range: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        sequence_id: u64,
        range_json: AriaStr,
    ) -> AriaResult,
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

/// Version 1 plugin export table returned by `aria_plugin_entry`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaPluginV1 {
    pub struct_size: usize,
    pub api_major: u32,
    pub reserved: u32,
    pub free_string: unsafe extern "C" fn(str: AriaOwnedStr),
    pub free_bytes: unsafe extern "C" fn(bytes: AriaOwnedBytes),
    pub mount: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    pub unmount: unsafe extern "C" fn(host_ctx: *mut c_void) -> AriaResult,
    pub command_run:
        unsafe extern "C" fn(host_ctx: *mut c_void, id: AriaStr, args_json: AriaStr) -> AriaResult,
    pub event: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        kind: AriaStr,
        event_json: AriaStr,
    ) -> AriaResult,
    pub import: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        importer_id: AriaStr,
        path: AriaStr,
    ) -> AriaResult,
    pub export: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        exporter_id: AriaStr,
        project_json: AriaStr,
        path: AriaStr,
    ) -> AriaResult,
    pub audio_create: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        effect_id: AriaStr,
        sample_rate: u32,
    ) -> AriaInstanceResult,
    pub audio_process: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        instance: u64,
        params: *const AriaAudioParam,
        params_len: usize,
        block: *mut AriaAudioBlock,
    ) -> AriaResult,
    pub audio_reset: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
    pub audio_drop: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
    pub panel_view:
        unsafe extern "C" fn(host_ctx: *mut c_void, panel_id: AriaStr) -> AriaBytesResult,
    pub panel_event: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        panel_id: AriaStr,
        event_json: AriaStr,
    ) -> AriaResult,
    pub encoder_open: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        encoder_id: AriaStr,
        path: AriaStr,
        config: *const AriaEncoderConfig,
    ) -> AriaInstanceResult,
    pub encoder_push_frame: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        instance: u64,
        frame: *const AriaEncoderFrame,
    ) -> AriaResult,
    pub encoder_push_audio: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        instance: u64,
        audio: *const AriaEncoderAudio,
    ) -> AriaResult,
    pub encoder_finish: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
    pub encoder_drop: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
    pub overlay_scene:
        unsafe extern "C" fn(host_ctx: *mut c_void, overlay_id: AriaStr) -> AriaBytesResult,
    pub overlay_event: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        overlay_id: AriaStr,
        event_json: AriaStr,
    ) -> AriaResult,
    pub job_step:
        unsafe extern "C" fn(host_ctx: *mut c_void, job: u64, step: *mut AriaJobStep) -> AriaResult,
    pub job_end:
        unsafe extern "C" fn(host_ctx: *mut c_void, job: u64, cancelled: u32) -> AriaResult,
    pub decoder_probe: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        decoder_id: AriaStr,
        path: AriaStr,
    ) -> AriaResult,
    pub decoder_open_video: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        decoder_id: AriaStr,
        path: AriaStr,
        info: *mut AriaVideoInfo,
    ) -> AriaInstanceResult,
    pub decoder_seek:
        unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64, pts: i64) -> AriaResult,
    pub decoder_next_frame: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        instance: u64,
        frame: *mut AriaVideoFrameOut,
    ) -> AriaResult,
    pub decoder_open_audio: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        decoder_id: AriaStr,
        path: AriaStr,
        sample_rate: u32,
        channels: u32,
    ) -> AriaInstanceResult,
    pub decoder_read_audio: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        instance: u64,
        out: *mut f32,
        out_len: usize,
        written: *mut usize,
    ) -> AriaResult,
    pub decoder_seek_audio:
        unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64, frame: i64) -> AriaResult,
    pub decoder_close: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
    pub ai_tool_call: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        name: AriaStr,
        arguments_json: AriaStr,
    ) -> AriaResult,
    pub scope_measure: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        scope_id: AriaStr,
        call: *mut AriaScopeCall,
    ) -> AriaResult,
    pub cpu_effect_process: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        effect_id: AriaStr,
        call: *mut AriaCpuEffectCall,
    ) -> AriaResult,
    pub driver_evaluate: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        language_id: AriaStr,
        expr: AriaStr,
        call: *mut AriaDriverEvalCall,
    ) -> AriaResult,
    pub analyzer_open: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        analyzer_id: AriaStr,
        sample_rate: u32,
        channels: u32,
    ) -> AriaInstanceResult,
    pub analyzer_push: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        instance: u64,
        samples: *const f32,
        samples_len: usize,
    ) -> AriaResult,
    pub analyzer_finish: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
    pub analyzer_drop: unsafe extern "C" fn(host_ctx: *mut c_void, instance: u64) -> AriaResult,
}

/// Entry function signature exported by a native plugin as `aria_plugin_entry`.
pub type AriaPluginEntryFn =
    unsafe extern "C" fn(host: *const AriaHostV1, api_major: u32) -> *const AriaPluginV1;

/// Minimum prefix of a versioned table required to read `struct_size` and `api_major`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AriaTableHeader {
    pub struct_size: usize,
    pub api_major: u32,
    pub reserved: u32,
}

/// Minimum byte size of an [`AriaHostV1`] table (v1 prefix before panel callbacks).
pub const ARIA_HOST_V1_MIN_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_panel);

/// Minimum byte size of an [`AriaPluginV1`] table (v1 prefix before panel callbacks).
pub const ARIA_PLUGIN_V1_MIN_SIZE: usize = std::mem::offset_of!(AriaPluginV1, panel_view);

/// Byte size of an [`AriaHostV1`] table up to and including panel callbacks.
pub const ARIA_HOST_V1_PANELS_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_encoder);

/// Byte size of an [`AriaPluginV1`] table up to and including panel callbacks.
pub const ARIA_PLUGIN_V1_PANELS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, encoder_open);

/// Byte size of an [`AriaHostV1`] table up to and including encoder callbacks.
pub const ARIA_HOST_V1_ENCODERS_SIZE: usize =
    std::mem::offset_of!(AriaHostV1, register_viewer_overlay);

/// Byte size of an [`AriaPluginV1`] table up to and including encoder callbacks.
pub const ARIA_PLUGIN_V1_ENCODERS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, overlay_scene);

/// Byte size of an [`AriaHostV1`] table up to and including viewer overlay callbacks.
pub const ARIA_HOST_V1_OVERLAYS_SIZE: usize = std::mem::offset_of!(AriaHostV1, start_job);

/// Byte size of an [`AriaPluginV1`] table up to and including viewer overlay callbacks.
pub const ARIA_PLUGIN_V1_OVERLAYS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, job_step);

/// Byte size of an [`AriaHostV1`] table up to and including the job callback.
pub const ARIA_HOST_V1_JOBS_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_decoder);

/// Byte size of an [`AriaPluginV1`] table up to and including job callbacks.
pub const ARIA_PLUGIN_V1_JOBS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, decoder_probe);

/// Byte size of an [`AriaHostV1`] table up to and including the decoder callback.
pub const ARIA_HOST_V1_DECODERS_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_ai_tool);

/// Byte size of an [`AriaPluginV1`] table up to and including decoder callbacks.
pub const ARIA_PLUGIN_V1_DECODERS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, ai_tool_call);

/// Byte size of an [`AriaHostV1`] table up to and including the AI tool callback.
pub const ARIA_HOST_V1_AI_TOOLS_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_scope);

/// Byte size of an [`AriaPluginV1`] table up to and including the AI tool callback.
pub const ARIA_PLUGIN_V1_AI_TOOLS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, scope_measure);

/// Byte size of an [`AriaHostV1`] table up to and including the scope callback.
pub const ARIA_HOST_V1_SCOPES_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_effect);

/// Byte size of an [`AriaPluginV1`] table up to and including the scope callback.
pub const ARIA_PLUGIN_V1_SCOPES_SIZE: usize =
    std::mem::offset_of!(AriaPluginV1, cpu_effect_process);

/// Byte size of an [`AriaHostV1`] table up to and including contribution callbacks
/// (`register_effect`..`register_analyzer`).
pub const ARIA_HOST_V1_CONTRIBUTIONS_SIZE: usize = std::mem::offset_of!(AriaHostV1, clip);

/// Byte size of an [`AriaHostV1`] table up to and including the project query
/// callbacks (`clip`..`in_out`).
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
    /// Whether this host table includes the `register_panel` and `refresh_panel` callbacks.
    pub const fn has_panels(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_PANELS_SIZE
    }

    /// Whether this host table includes the `register_encoder` callback.
    pub const fn has_encoders(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_ENCODERS_SIZE
    }

    /// Whether this host table includes the `register_viewer_overlay` and `refresh_viewer_overlay` callbacks.
    pub const fn has_overlays(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_OVERLAYS_SIZE
    }

    /// Whether this host table includes the `start_job` callback.
    pub const fn has_jobs(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_JOBS_SIZE
    }

    /// Whether this host table includes the `register_decoder` callback.
    pub const fn has_decoders(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_DECODERS_SIZE
    }

    /// Whether this host table includes the `register_ai_tool` callback.
    pub const fn has_ai_tools(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_AI_TOOLS_SIZE
    }

    /// Whether this host table includes the `register_scope` callback.
    pub const fn has_scopes(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_SCOPES_SIZE
    }

    /// Whether this host table includes the effect, transition, generator, clip type,
    /// driver language, preset, theme, and workspace registration callbacks.
    pub const fn has_contributions(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_CONTRIBUTIONS_SIZE
    }

    /// Whether this host table includes the project query callbacks
    /// (`clip`, `clips_in_range`, `clips_at`, `selected_clips`, `markers_in_range`, `in_out`).
    pub const fn has_project_queries(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_QUERIES_SIZE
    }

    /// Whether this host table includes `register_texture`.
    pub const fn has_textures(&self) -> bool {
        self.struct_size >= std::mem::size_of::<Self>()
    }
}

impl AriaPluginV1 {
    /// Whether this plugin table includes the `panel_view` and `panel_event` callbacks.
    pub const fn has_panels(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_PANELS_SIZE
    }

    /// Whether this plugin table includes the encoder callbacks (`encoder_open`..`encoder_drop`).
    pub const fn has_encoders(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_ENCODERS_SIZE
    }

    /// Whether this plugin table includes the `overlay_scene` and `overlay_event` callbacks.
    pub const fn has_overlays(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_OVERLAYS_SIZE
    }

    /// Whether this plugin table includes the `job_step` and `job_end` callbacks.
    pub const fn has_jobs(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_JOBS_SIZE
    }

    /// Whether this plugin table includes the decoder callbacks
    /// (`decoder_probe`..`decoder_close`).
    pub const fn has_decoders(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_DECODERS_SIZE
    }

    /// Whether this plugin table includes the `ai_tool_call` callback.
    pub const fn has_ai_tools(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_AI_TOOLS_SIZE
    }

    /// Whether this plugin table includes the `scope_measure` callback.
    pub const fn has_scopes(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_SCOPES_SIZE
    }

    /// Whether this plugin table includes the `cpu_effect_process` and
    /// `driver_evaluate` callbacks.
    pub const fn has_contributions(&self) -> bool {
        self.struct_size >= std::mem::size_of::<Self>()
    }

    /// Read a validated [`AriaPluginV1`] from `ptr`, filling missing trailing panel,
    /// encoder, overlay, job, decoder, AI tool, or scope fields with error-returning stubs when `struct_size < size_of::<AriaPluginV1>()`.
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

/// Render the canonical C header corresponding to the Rust ABI definitions.
pub fn render_c_header() -> String {
    format!(
        r#"#ifndef ARIA_PLUGIN_H
#define ARIA_PLUGIN_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {{
#endif

#define ARIA_API_MAJOR {ARIA_API_MAJOR}u

#define ARIA_STATUS_OK {ARIA_STATUS_OK}u
#define ARIA_STATUS_ERR {ARIA_STATUS_ERR}u
#define ARIA_STATUS_PANIC {ARIA_STATUS_PANIC}u
#define ARIA_STATUS_UNSUPPORTED {ARIA_STATUS_UNSUPPORTED}u

#define ARIA_BYTES_FOUND {ARIA_BYTES_FOUND}u
#define ARIA_BYTES_NOT_FOUND {ARIA_BYTES_NOT_FOUND}u
#define ARIA_BYTES_ERR {ARIA_BYTES_ERR}u
#define ARIA_BYTES_PANIC {ARIA_BYTES_PANIC}u

#define ARIA_LOG_TRACE {ARIA_LOG_TRACE}u
#define ARIA_LOG_DEBUG {ARIA_LOG_DEBUG}u
#define ARIA_LOG_INFO {ARIA_LOG_INFO}u
#define ARIA_LOG_WARN {ARIA_LOG_WARN}u
#define ARIA_LOG_ERROR {ARIA_LOG_ERROR}u

#define ARIA_FORMAT_NV12 {ARIA_FORMAT_NV12}u
#define ARIA_FORMAT_YUV420P {ARIA_FORMAT_YUV420P}u

#define ARIA_COLOR_BT709 {ARIA_COLOR_BT709}u
#define ARIA_COLOR_BT601 {ARIA_COLOR_BT601}u
#define ARIA_COLOR_BT2020_PQ {ARIA_COLOR_BT2020_PQ}u
#define ARIA_COLOR_SRGB {ARIA_COLOR_SRGB}u

#define ARIA_PARAM_SCALAR {ARIA_PARAM_SCALAR}u
#define ARIA_PARAM_VEC2 {ARIA_PARAM_VEC2}u
#define ARIA_PARAM_COLOR {ARIA_PARAM_COLOR}u
#define ARIA_PARAM_RECT {ARIA_PARAM_RECT}u
#define ARIA_PARAM_VEC3 {ARIA_PARAM_VEC3}u

typedef struct AriaStr {{
    const uint8_t *ptr;
    size_t len;
}} AriaStr;

typedef struct AriaBytes {{
    const uint8_t *ptr;
    size_t len;
}} AriaBytes;

typedef struct AriaOwnedStr {{
    uint8_t *ptr;
    size_t len;
    size_t cap;
}} AriaOwnedStr;

typedef struct AriaOwnedBytes {{
    uint8_t *ptr;
    size_t len;
    size_t cap;
}} AriaOwnedBytes;

typedef struct AriaResult {{
    uint32_t status;
    AriaOwnedStr value;
}} AriaResult;

typedef struct AriaBytesResult {{
    uint32_t status;
    AriaOwnedBytes bytes;
    AriaOwnedStr error;
}} AriaBytesResult;

typedef struct AriaInstanceResult {{
    uint32_t status;
    uint64_t instance;
    AriaOwnedStr error;
}} AriaInstanceResult;

typedef struct AriaAudioParam {{
    float constant;
    const float *per_frame;
    size_t per_frame_len;
}} AriaAudioParam;

typedef struct AriaAudioBlock {{
    float *left;
    float *right;
    size_t frames;
    int64_t position;
    uint32_t sample_rate;
    uint32_t has_sidechain;
    const float *sidechain_left;
    const float *sidechain_right;
}} AriaAudioBlock;

typedef struct AriaEncoderConfig {{
    uint32_t width;
    uint32_t height;
    int64_t fps_num;
    int32_t fps_den;
    uint32_t video_bitrate_kbps;
    uint32_t audio_bitrate_kbps;
}} AriaEncoderConfig;

typedef struct AriaEncoderFrame {{
    const uint8_t *rgba;
    size_t rgba_len;
    uint32_t width;
    uint32_t height;
    uint32_t stride;
    int64_t pts_value;
    int32_t pts_rate;
}} AriaEncoderFrame;

typedef struct AriaEncoderAudio {{
    const float *samples;
    size_t samples_len;
    uint32_t sample_rate;
    uint32_t channels;
    int64_t pts_value;
    int32_t pts_rate;
}} AriaEncoderAudio;

typedef struct AriaJobStep {{
    float progress;
    uint32_t done;
    uint32_t commit;
}} AriaJobStep;

typedef struct AriaVideoInfo {{
    uint32_t width;
    uint32_t height;
    uint32_t format;
    uint32_t color;
    int32_t frame_rate_num;
    int32_t frame_rate_den;
    int32_t time_base_num;
    int32_t time_base_den;
    int64_t duration;
}} AriaVideoInfo;

typedef struct AriaFramePlane {{
    uint8_t *data;
    size_t len;
    uint32_t stride;
    uint32_t rows;
}} AriaFramePlane;

typedef struct AriaVideoFrameOut {{
    AriaFramePlane *planes;
    size_t planes_len;
    int64_t pts;
    uint32_t produced;
}} AriaVideoFrameOut;

typedef struct AriaScopeCall {{
    const uint8_t *frame;
    size_t frame_len;
    uint32_t frame_width;
    uint32_t frame_height;
    uint8_t *image;
    size_t image_len;
    uint32_t image_width;
    uint32_t image_height;
}} AriaScopeCall;

typedef struct AriaCpuEffectCall {{
    const uint16_t *input;
    uint16_t *output;
    size_t pixels_len;
    uint32_t width;
    uint32_t height;
    const float *params;
    size_t params_len;
    AriaStr params_json;
    float time;
}} AriaCpuEffectCall;

typedef struct AriaParamValue {{
    uint32_t kind;
    float x;
    float y;
    float z;
    float w;
    uint8_t r;
    uint8_t g;
    uint8_t b;
    uint8_t a;
}} AriaParamValue;

typedef struct AriaDriverEvalCall {{
    double time;
    double clip_time;
    double frame_rate;
    double duration;
    uint32_t canvas_width;
    uint32_t canvas_height;
    AriaParamValue value;
    uint64_t seed;
    AriaParamValue output;
}} AriaDriverEvalCall;

typedef struct AriaHostV1 {{
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
}} AriaHostV1;

typedef struct AriaPluginV1 {{
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
}} AriaPluginV1;

typedef const AriaPluginV1 *(*AriaPluginEntryFn)(const AriaHostV1 *host, uint32_t api_major);

const AriaPluginV1 *aria_plugin_entry(const AriaHostV1 *host, uint32_t api_major);

#ifdef __cplusplus
}}
#endif

#endif /* ARIA_PLUGIN_H */
"#
    )
}

#[cfg(test)]
mod tests {
    use std::mem::{offset_of, size_of};

    use super::*;

    const HEADER: &str = include_str!("../include/aria_plugin.h");

    /// Extract the ordered field names of every `typedef struct <Name> { ... } <Name>;`
    /// block in a C header.
    fn parse_header_structs(header: &str) -> Vec<(String, Vec<String>)> {
        let mut structs = Vec::new();
        let mut current: Option<(String, Vec<String>)> = None;
        for raw_line in header.lines() {
            let line = raw_line.trim();
            if let Some(rest) = line.strip_prefix("typedef struct ") {
                if let Some(name) = rest.strip_suffix('{') {
                    current = Some((name.trim().to_owned(), Vec::new()));
                }
                continue;
            }
            if let Some((name, fields)) = &mut current {
                if let Some(tail) = line.strip_prefix('}') {
                    let closing = tail.trim().trim_end_matches(';').trim();
                    assert_eq!(closing, name, "mismatched closing typedef for {name}");
                    structs.push(current.take().expect("open struct"));
                    continue;
                }
                let decl = line.trim_end_matches(';').trim();
                if decl.is_empty() {
                    continue;
                }
                let field_name = if let Some(start) = decl.find("(*") {
                    let after = &decl[start + 2..];
                    let end = after.find(')').expect("closing paren on fn pointer");
                    after[..end].trim().to_owned()
                } else {
                    let token = decl
                        .split_whitespace()
                        .last()
                        .expect("field declaration token");
                    token.trim_start_matches('*').to_owned()
                };
                fields.push(field_name);
            }
        }
        structs
    }

    #[test]
    fn header_matches_rendered_rust_definition_and_field_offsets() {
        assert_eq!(HEADER, render_c_header());

        let parsed = parse_header_structs(HEADER);
        let expected: Vec<(&str, &[&str])> = vec![
            ("AriaStr", &["ptr", "len"]),
            ("AriaBytes", &["ptr", "len"]),
            ("AriaOwnedStr", &["ptr", "len", "cap"]),
            ("AriaOwnedBytes", &["ptr", "len", "cap"]),
            ("AriaResult", &["status", "value"]),
            ("AriaBytesResult", &["status", "bytes", "error"]),
            ("AriaInstanceResult", &["status", "instance", "error"]),
            (
                "AriaAudioParam",
                &["constant", "per_frame", "per_frame_len"],
            ),
            (
                "AriaAudioBlock",
                &[
                    "left",
                    "right",
                    "frames",
                    "position",
                    "sample_rate",
                    "has_sidechain",
                    "sidechain_left",
                    "sidechain_right",
                ],
            ),
            (
                "AriaEncoderConfig",
                &[
                    "width",
                    "height",
                    "fps_num",
                    "fps_den",
                    "video_bitrate_kbps",
                    "audio_bitrate_kbps",
                ],
            ),
            (
                "AriaEncoderFrame",
                &[
                    "rgba",
                    "rgba_len",
                    "width",
                    "height",
                    "stride",
                    "pts_value",
                    "pts_rate",
                ],
            ),
            (
                "AriaEncoderAudio",
                &[
                    "samples",
                    "samples_len",
                    "sample_rate",
                    "channels",
                    "pts_value",
                    "pts_rate",
                ],
            ),
            ("AriaJobStep", &["progress", "done", "commit"]),
            (
                "AriaVideoInfo",
                &[
                    "width",
                    "height",
                    "format",
                    "color",
                    "frame_rate_num",
                    "frame_rate_den",
                    "time_base_num",
                    "time_base_den",
                    "duration",
                ],
            ),
            ("AriaFramePlane", &["data", "len", "stride", "rows"]),
            (
                "AriaVideoFrameOut",
                &["planes", "planes_len", "pts", "produced"],
            ),
            (
                "AriaScopeCall",
                &[
                    "frame",
                    "frame_len",
                    "frame_width",
                    "frame_height",
                    "image",
                    "image_len",
                    "image_width",
                    "image_height",
                ],
            ),
            (
                "AriaCpuEffectCall",
                &[
                    "input",
                    "output",
                    "pixels_len",
                    "width",
                    "height",
                    "params",
                    "params_len",
                    "params_json",
                    "time",
                ],
            ),
            (
                "AriaParamValue",
                &["kind", "x", "y", "z", "w", "r", "g", "b", "a"],
            ),
            (
                "AriaDriverEvalCall",
                &[
                    "time",
                    "clip_time",
                    "frame_rate",
                    "duration",
                    "canvas_width",
                    "canvas_height",
                    "value",
                    "seed",
                    "output",
                ],
            ),
            (
                "AriaHostV1",
                &[
                    "struct_size",
                    "api_major",
                    "reserved",
                    "free_string",
                    "free_bytes",
                    "log",
                    "storage_get",
                    "storage_set",
                    "storage_delete",
                    "storage_keys",
                    "setting_get",
                    "register_command",
                    "subscribe",
                    "register_importer",
                    "register_exporter",
                    "register_audio_effect",
                    "project_snapshot",
                    "editor_view",
                    "project_transact",
                    "project_undo",
                    "register_panel",
                    "refresh_panel",
                    "register_encoder",
                    "register_viewer_overlay",
                    "refresh_viewer_overlay",
                    "start_job",
                    "register_decoder",
                    "register_ai_tool",
                    "register_scope",
                    "register_effect",
                    "register_cpu_effect",
                    "register_transition",
                    "register_generator",
                    "register_clip_type",
                    "register_driver_language",
                    "register_preset",
                    "register_theme",
                    "register_workspace",
                    "register_analyzer",
                    "clip",
                    "clips_in_range",
                    "clips_at",
                    "selected_clips",
                    "markers_in_range",
                    "in_out",
                    "register_texture",
                ],
            ),
            (
                "AriaPluginV1",
                &[
                    "struct_size",
                    "api_major",
                    "reserved",
                    "free_string",
                    "free_bytes",
                    "mount",
                    "unmount",
                    "command_run",
                    "event",
                    "import",
                    "export",
                    "audio_create",
                    "audio_process",
                    "audio_reset",
                    "audio_drop",
                    "panel_view",
                    "panel_event",
                    "encoder_open",
                    "encoder_push_frame",
                    "encoder_push_audio",
                    "encoder_finish",
                    "encoder_drop",
                    "overlay_scene",
                    "overlay_event",
                    "job_step",
                    "job_end",
                    "decoder_probe",
                    "decoder_open_video",
                    "decoder_seek",
                    "decoder_next_frame",
                    "decoder_open_audio",
                    "decoder_read_audio",
                    "decoder_seek_audio",
                    "decoder_close",
                    "ai_tool_call",
                    "scope_measure",
                    "cpu_effect_process",
                    "driver_evaluate",
                    "analyzer_open",
                    "analyzer_push",
                    "analyzer_finish",
                    "analyzer_drop",
                ],
            ),
        ];

        assert_eq!(parsed.len(), expected.len());
        for ((got_name, got_fields), (want_name, want_fields)) in parsed.iter().zip(expected.iter())
        {
            assert_eq!(got_name, want_name);
            let want_vec: Vec<String> = want_fields.iter().map(|s| (*s).to_owned()).collect();
            assert_eq!(got_fields, &want_vec, "field drift in {got_name}");
        }

        // Verify exact C layout monotonicity and first-field `struct_size` invariant.
        assert_eq!(offset_of!(AriaHostV1, struct_size), 0);
        assert_eq!(offset_of!(AriaHostV1, api_major), size_of::<usize>());
        assert!(offset_of!(AriaHostV1, free_string) < offset_of!(AriaHostV1, free_bytes));
        assert!(offset_of!(AriaHostV1, free_bytes) < offset_of!(AriaHostV1, log));
        assert!(offset_of!(AriaHostV1, log) < offset_of!(AriaHostV1, storage_get));
        assert!(offset_of!(AriaHostV1, storage_get) < offset_of!(AriaHostV1, storage_set));
        assert!(offset_of!(AriaHostV1, storage_set) < offset_of!(AriaHostV1, storage_delete));
        assert!(offset_of!(AriaHostV1, storage_delete) < offset_of!(AriaHostV1, storage_keys));
        assert!(offset_of!(AriaHostV1, storage_keys) < offset_of!(AriaHostV1, setting_get));
        assert!(offset_of!(AriaHostV1, setting_get) < offset_of!(AriaHostV1, register_command));
        assert!(offset_of!(AriaHostV1, register_command) < offset_of!(AriaHostV1, subscribe));
        assert!(offset_of!(AriaHostV1, subscribe) < offset_of!(AriaHostV1, register_importer));
        assert!(
            offset_of!(AriaHostV1, register_importer) < offset_of!(AriaHostV1, register_exporter)
        );
        assert!(
            offset_of!(AriaHostV1, register_exporter)
                < offset_of!(AriaHostV1, register_audio_effect)
        );
        assert!(
            offset_of!(AriaHostV1, register_audio_effect)
                < offset_of!(AriaHostV1, project_snapshot)
        );
        assert!(offset_of!(AriaHostV1, project_snapshot) < offset_of!(AriaHostV1, editor_view));
        assert!(offset_of!(AriaHostV1, editor_view) < offset_of!(AriaHostV1, project_transact));
        assert!(offset_of!(AriaHostV1, project_transact) < offset_of!(AriaHostV1, project_undo));
        assert!(offset_of!(AriaHostV1, project_undo) < offset_of!(AriaHostV1, register_panel));
        assert!(offset_of!(AriaHostV1, register_panel) < offset_of!(AriaHostV1, refresh_panel));
        assert!(offset_of!(AriaHostV1, refresh_panel) < offset_of!(AriaHostV1, register_encoder));
        assert!(
            offset_of!(AriaHostV1, register_encoder)
                < offset_of!(AriaHostV1, register_viewer_overlay)
        );
        assert!(
            offset_of!(AriaHostV1, register_viewer_overlay)
                < offset_of!(AriaHostV1, refresh_viewer_overlay)
        );
        assert!(offset_of!(AriaHostV1, refresh_viewer_overlay) < offset_of!(AriaHostV1, start_job));
        assert!(offset_of!(AriaHostV1, start_job) < offset_of!(AriaHostV1, register_decoder));
        assert!(
            offset_of!(AriaHostV1, register_decoder) < offset_of!(AriaHostV1, register_ai_tool)
        );
        assert!(offset_of!(AriaHostV1, register_ai_tool) < offset_of!(AriaHostV1, register_scope));
        assert!(offset_of!(AriaHostV1, register_scope) < offset_of!(AriaHostV1, register_effect));
        assert!(
            offset_of!(AriaHostV1, register_effect) < offset_of!(AriaHostV1, register_cpu_effect)
        );
        assert!(
            offset_of!(AriaHostV1, register_cpu_effect)
                < offset_of!(AriaHostV1, register_transition)
        );
        assert!(
            offset_of!(AriaHostV1, register_transition)
                < offset_of!(AriaHostV1, register_generator)
        );
        assert!(
            offset_of!(AriaHostV1, register_generator) < offset_of!(AriaHostV1, register_clip_type)
        );
        assert!(
            offset_of!(AriaHostV1, register_clip_type)
                < offset_of!(AriaHostV1, register_driver_language)
        );
        assert!(
            offset_of!(AriaHostV1, register_driver_language)
                < offset_of!(AriaHostV1, register_preset)
        );
        assert!(offset_of!(AriaHostV1, register_preset) < offset_of!(AriaHostV1, register_theme));
        assert!(
            offset_of!(AriaHostV1, register_theme) < offset_of!(AriaHostV1, register_workspace)
        );
        assert!(
            offset_of!(AriaHostV1, register_workspace) < offset_of!(AriaHostV1, register_analyzer)
        );
        assert!(offset_of!(AriaHostV1, register_analyzer) < offset_of!(AriaHostV1, clip));
        assert!(offset_of!(AriaHostV1, clip) < offset_of!(AriaHostV1, clips_in_range));
        assert!(offset_of!(AriaHostV1, clips_in_range) < offset_of!(AriaHostV1, clips_at));
        assert!(offset_of!(AriaHostV1, clips_at) < offset_of!(AriaHostV1, selected_clips));
        assert!(offset_of!(AriaHostV1, selected_clips) < offset_of!(AriaHostV1, markers_in_range));
        assert!(offset_of!(AriaHostV1, markers_in_range) < offset_of!(AriaHostV1, in_out));
        assert!(offset_of!(AriaHostV1, in_out) < offset_of!(AriaHostV1, register_texture));

        assert_eq!(offset_of!(AriaPluginV1, struct_size), 0);
        assert_eq!(offset_of!(AriaPluginV1, api_major), size_of::<usize>());
        assert!(offset_of!(AriaPluginV1, free_string) < offset_of!(AriaPluginV1, free_bytes));
        assert!(offset_of!(AriaPluginV1, free_bytes) < offset_of!(AriaPluginV1, mount));
        assert!(offset_of!(AriaPluginV1, mount) < offset_of!(AriaPluginV1, unmount));
        assert!(offset_of!(AriaPluginV1, unmount) < offset_of!(AriaPluginV1, command_run));
        assert!(offset_of!(AriaPluginV1, command_run) < offset_of!(AriaPluginV1, event));
        assert!(offset_of!(AriaPluginV1, event) < offset_of!(AriaPluginV1, import));
        assert!(offset_of!(AriaPluginV1, import) < offset_of!(AriaPluginV1, export));
        assert!(offset_of!(AriaPluginV1, export) < offset_of!(AriaPluginV1, audio_create));
        assert!(offset_of!(AriaPluginV1, audio_create) < offset_of!(AriaPluginV1, audio_process));
        assert!(offset_of!(AriaPluginV1, audio_process) < offset_of!(AriaPluginV1, audio_reset));
        assert!(offset_of!(AriaPluginV1, audio_reset) < offset_of!(AriaPluginV1, audio_drop));
        assert!(offset_of!(AriaPluginV1, audio_drop) < offset_of!(AriaPluginV1, panel_view));
        assert!(offset_of!(AriaPluginV1, panel_view) < offset_of!(AriaPluginV1, panel_event));
        assert!(offset_of!(AriaPluginV1, panel_event) < offset_of!(AriaPluginV1, encoder_open));
        assert!(
            offset_of!(AriaPluginV1, encoder_open) < offset_of!(AriaPluginV1, encoder_push_frame)
        );
        assert!(
            offset_of!(AriaPluginV1, encoder_push_frame)
                < offset_of!(AriaPluginV1, encoder_push_audio)
        );
        assert!(
            offset_of!(AriaPluginV1, encoder_push_audio) < offset_of!(AriaPluginV1, encoder_finish)
        );
        assert!(offset_of!(AriaPluginV1, encoder_finish) < offset_of!(AriaPluginV1, encoder_drop));
        assert!(offset_of!(AriaPluginV1, encoder_drop) < offset_of!(AriaPluginV1, overlay_scene));
        assert!(offset_of!(AriaPluginV1, overlay_scene) < offset_of!(AriaPluginV1, overlay_event));
        assert!(offset_of!(AriaPluginV1, overlay_event) < offset_of!(AriaPluginV1, job_step));
        assert!(offset_of!(AriaPluginV1, job_step) < offset_of!(AriaPluginV1, job_end));
        let decoder_fields = [
            offset_of!(AriaPluginV1, job_end),
            offset_of!(AriaPluginV1, decoder_probe),
            offset_of!(AriaPluginV1, decoder_open_video),
            offset_of!(AriaPluginV1, decoder_seek),
            offset_of!(AriaPluginV1, decoder_next_frame),
            offset_of!(AriaPluginV1, decoder_open_audio),
            offset_of!(AriaPluginV1, decoder_read_audio),
            offset_of!(AriaPluginV1, decoder_seek_audio),
            offset_of!(AriaPluginV1, decoder_close),
            offset_of!(AriaPluginV1, ai_tool_call),
            offset_of!(AriaPluginV1, scope_measure),
            offset_of!(AriaPluginV1, cpu_effect_process),
            offset_of!(AriaPluginV1, driver_evaluate),
            offset_of!(AriaPluginV1, analyzer_open),
            offset_of!(AriaPluginV1, analyzer_push),
            offset_of!(AriaPluginV1, analyzer_finish),
            offset_of!(AriaPluginV1, analyzer_drop),
        ];
        assert!(
            decoder_fields.is_sorted_by(|a, b| a < b),
            "{decoder_fields:?}"
        );
    }

    #[test]
    fn owned_string_and_bytes_round_trip_and_free() {
        let s = AriaOwnedStr::from_string("hello native abi".to_owned());
        assert!(!s.is_null());
        #[allow(unsafe_code)]
        // SAFETY: `s` was created by `AriaOwnedStr::from_string` in this crate.
        let recovered = unsafe { s.into_string_with(free_rust_string) };
        assert_eq!(recovered, "hello native abi");

        let empty = AriaOwnedStr::empty();
        assert!(empty.is_null());
        #[allow(unsafe_code)]
        // SAFETY: `empty` is `AriaOwnedStr::empty()`.
        let empty_str = unsafe { empty.into_string_with(free_rust_string) };
        assert_eq!(empty_str, "");

        let b = AriaOwnedBytes::from_vec(vec![1, 2, 3, 4]);
        #[allow(unsafe_code)]
        // SAFETY: `b` was created by `AriaOwnedBytes::from_vec` in this crate.
        let recovered_bytes = unsafe { b.into_vec_with(free_rust_bytes) };
        assert_eq!(recovered_bytes, vec![1, 2, 3, 4]);
    }

    #[test]
    fn table_validation_refuses_wrong_api_major_and_short_tables() {
        assert_eq!(
            validate_plugin_table(ARIA_API_MAJOR, size_of::<AriaPluginV1>()),
            Ok(())
        );
        // A v1 table without the trailing panel callbacks is still accepted.
        assert_eq!(
            validate_plugin_table(ARIA_API_MAJOR, ARIA_PLUGIN_V1_MIN_SIZE),
            Ok(())
        );
        for wrong in [0, 2, 99] {
            let err = validate_plugin_table(wrong, size_of::<AriaPluginV1>())
                .expect_err("wrong API major must be refused");
            assert!(err.contains(&wrong.to_string()), "{err}");
            assert!(err.contains(&ARIA_API_MAJOR.to_string()), "{err}");
        }
        let short = ARIA_PLUGIN_V1_MIN_SIZE - 8;
        let err = validate_plugin_table(ARIA_API_MAJOR, short)
            .expect_err("short plugin table must be refused");
        assert!(err.contains(&short.to_string()), "{err}");
        assert!(err.contains(&ARIA_PLUGIN_V1_MIN_SIZE.to_string()), "{err}");
    }

    #[test]
    fn prefix_size_predicates_and_copy_from_prefix_fill_trailing_stubs() {
        assert!(ARIA_HOST_V1_MIN_SIZE < ARIA_HOST_V1_PANELS_SIZE);
        assert!(ARIA_HOST_V1_PANELS_SIZE < ARIA_HOST_V1_ENCODERS_SIZE);
        assert!(ARIA_HOST_V1_ENCODERS_SIZE < ARIA_HOST_V1_OVERLAYS_SIZE);
        assert!(ARIA_HOST_V1_OVERLAYS_SIZE < ARIA_HOST_V1_JOBS_SIZE);
        assert!(ARIA_HOST_V1_JOBS_SIZE < ARIA_HOST_V1_DECODERS_SIZE);
        assert!(ARIA_HOST_V1_DECODERS_SIZE < ARIA_HOST_V1_AI_TOOLS_SIZE);
        assert!(ARIA_HOST_V1_AI_TOOLS_SIZE < ARIA_HOST_V1_SCOPES_SIZE);
        assert!(ARIA_HOST_V1_SCOPES_SIZE < ARIA_HOST_V1_CONTRIBUTIONS_SIZE);
        assert!(ARIA_HOST_V1_CONTRIBUTIONS_SIZE < ARIA_HOST_V1_QUERIES_SIZE);
        assert!(ARIA_HOST_V1_QUERIES_SIZE < size_of::<AriaHostV1>());

        assert!(ARIA_PLUGIN_V1_MIN_SIZE < ARIA_PLUGIN_V1_PANELS_SIZE);
        assert!(ARIA_PLUGIN_V1_PANELS_SIZE < ARIA_PLUGIN_V1_ENCODERS_SIZE);
        assert!(ARIA_PLUGIN_V1_ENCODERS_SIZE < ARIA_PLUGIN_V1_OVERLAYS_SIZE);
        assert!(ARIA_PLUGIN_V1_OVERLAYS_SIZE < ARIA_PLUGIN_V1_JOBS_SIZE);
        assert!(ARIA_PLUGIN_V1_JOBS_SIZE < ARIA_PLUGIN_V1_DECODERS_SIZE);
        assert!(ARIA_PLUGIN_V1_DECODERS_SIZE < ARIA_PLUGIN_V1_AI_TOOLS_SIZE);
        assert!(ARIA_PLUGIN_V1_AI_TOOLS_SIZE < ARIA_PLUGIN_V1_SCOPES_SIZE);
        assert!(ARIA_PLUGIN_V1_SCOPES_SIZE < size_of::<AriaPluginV1>());

        #[allow(unsafe_code)]
        unsafe extern "C" fn noop_ok(_ctx: *mut c_void) -> AriaResult {
            AriaResult::ok_empty()
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn noop_cmd(
            _ctx: *mut c_void,
            _id: AriaStr,
            _args: AriaStr,
        ) -> AriaResult {
            AriaResult::ok_empty()
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn noop_export(
            _ctx: *mut c_void,
            _id: AriaStr,
            _proj: AriaStr,
            _path: AriaStr,
        ) -> AriaResult {
            AriaResult::ok_empty()
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn noop_audio_create(
            _ctx: *mut c_void,
            _id: AriaStr,
            _sr: u32,
        ) -> AriaInstanceResult {
            AriaInstanceResult::ok(1)
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn noop_audio_proc(
            _ctx: *mut c_void,
            _inst: u64,
            _params: *const AriaAudioParam,
            _len: usize,
            _block: *mut AriaAudioBlock,
        ) -> AriaResult {
            AriaResult::ok_empty()
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn noop_u64(_ctx: *mut c_void, _inst: u64) -> AriaResult {
            AriaResult::ok_empty()
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn custom_panel_view(_ctx: *mut c_void, _id: AriaStr) -> AriaBytesResult {
            AriaBytesResult::found(b"panel-ok".to_vec())
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn custom_encoder_open(
            _ctx: *mut c_void,
            _id: AriaStr,
            _path: AriaStr,
            _cfg: *const AriaEncoderConfig,
        ) -> AriaInstanceResult {
            AriaInstanceResult::ok(77)
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn custom_encoder_frame(
            _ctx: *mut c_void,
            _inst: u64,
            _frame: *const AriaEncoderFrame,
        ) -> AriaResult {
            AriaResult::ok_empty()
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn custom_encoder_audio(
            _ctx: *mut c_void,
            _inst: u64,
            _audio: *const AriaEncoderAudio,
        ) -> AriaResult {
            AriaResult::ok_empty()
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn custom_overlay_scene(
            _ctx: *mut c_void,
            _id: AriaStr,
        ) -> AriaBytesResult {
            AriaBytesResult::found(b"overlay-ok".to_vec())
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn custom_job_step(
            _ctx: *mut c_void,
            job: u64,
            step: *mut AriaJobStep,
        ) -> AriaResult {
            // SAFETY: the caller passes a valid, writable `AriaJobStep`.
            unsafe {
                (*step).progress = 0.5;
                (*step).done = 1;
            }
            AriaResult::ok(format!("job {job}"))
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn noop_job_end(
            _ctx: *mut c_void,
            _job: u64,
            _cancelled: u32,
        ) -> AriaResult {
            AriaResult::ok_empty()
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn custom_decoder_probe(
            _ctx: *mut c_void,
            id: AriaStr,
            _path: AriaStr,
        ) -> AriaResult {
            // SAFETY: the caller passes a valid UTF-8 `AriaStr`.
            AriaResult::unsupported(format!("not {}", unsafe { id.as_str() }.unwrap_or("?")))
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn noop_decoder_open_video(
            _ctx: *mut c_void,
            _id: AriaStr,
            _path: AriaStr,
            _info: *mut AriaVideoInfo,
        ) -> AriaInstanceResult {
            AriaInstanceResult::ok(5)
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn noop_decoder_seek(
            _ctx: *mut c_void,
            _inst: u64,
            _at: i64,
        ) -> AriaResult {
            AriaResult::ok_empty()
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn noop_decoder_next_frame(
            _ctx: *mut c_void,
            _inst: u64,
            _frame: *mut AriaVideoFrameOut,
        ) -> AriaResult {
            AriaResult::ok_empty()
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn noop_decoder_open_audio(
            _ctx: *mut c_void,
            _id: AriaStr,
            _path: AriaStr,
            _rate: u32,
            _channels: u32,
        ) -> AriaInstanceResult {
            AriaInstanceResult::ok(6)
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn noop_decoder_read_audio(
            _ctx: *mut c_void,
            _inst: u64,
            _out: *mut f32,
            _len: usize,
            _written: *mut usize,
        ) -> AriaResult {
            AriaResult::ok_empty()
        }

        #[allow(unsafe_code)]
        unsafe extern "C" fn custom_ai_tool_call(
            _ctx: *mut c_void,
            _name: AriaStr,
            _arguments: AriaStr,
        ) -> AriaResult {
            AriaResult::ok("tool-ok".to_owned())
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn custom_scope_measure(
            _ctx: *mut c_void,
            _scope_id: AriaStr,
            call: *mut AriaScopeCall,
        ) -> AriaResult {
            // SAFETY: the test passes a call whose buffers hold at least one pixel each.
            unsafe {
                let call = &*call;
                std::ptr::copy_nonoverlapping(call.frame, call.image, 4);
            }
            AriaResult::ok("scope-ok".to_owned())
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn custom_cpu_effect_process(
            _ctx: *mut c_void,
            _effect_id: AriaStr,
            call: *mut AriaCpuEffectCall,
        ) -> AriaResult {
            // SAFETY: the test passes a call whose buffers hold at least one pixel each.
            unsafe {
                let call = &*call;
                std::ptr::copy_nonoverlapping(call.input, call.output, 4);
            }
            AriaResult::ok_empty()
        }
        #[allow(unsafe_code)]
        unsafe extern "C" fn custom_driver_evaluate(
            _ctx: *mut c_void,
            _language_id: AriaStr,
            _expr: AriaStr,
            call: *mut AriaDriverEvalCall,
        ) -> AriaResult {
            // SAFETY: the test passes a valid writable `AriaDriverEvalCall`.
            unsafe {
                let call = &mut *call;
                call.output = call.value;
                call.output.x *= 2.0;
            }
            AriaResult::ok_empty()
        }
        let scope_measure = |table: &AriaPluginV1| {
            let frame = [10_u8, 20, 30, 255];
            let mut image = [0_u8; 4];
            let mut call = AriaScopeCall {
                frame: frame.as_ptr(),
                frame_len: frame.len(),
                frame_width: 1,
                frame_height: 1,
                image: image.as_mut_ptr(),
                image_len: image.len(),
                image_width: 1,
                image_height: 1,
            };
            #[allow(unsafe_code)]
            // SAFETY: `scope_measure` is the table's own or a stub; `call` outlives it.
            let result = unsafe {
                (table.scope_measure)(
                    std::ptr::null_mut(),
                    AriaStr::from_str("meter"),
                    &raw mut call,
                )
            };
            #[allow(unsafe_code)]
            // SAFETY: the value was allocated by this crate's `AriaOwnedStr::from_string`.
            let text = unsafe { result.value.into_string_with(free_rust_string) };
            (result.status, text, image)
        };
        let tool_call = |table: &AriaPluginV1| {
            #[allow(unsafe_code)]
            // SAFETY: `ai_tool_call` is the table's own or a stub; both take any strings.
            let result = unsafe {
                (table.ai_tool_call)(
                    std::ptr::null_mut(),
                    AriaStr::from_str("echo"),
                    AriaStr::from_str("{}"),
                )
            };
            #[allow(unsafe_code)]
            // SAFETY: the value was allocated by this crate's `AriaOwnedStr::from_string`.
            let text = unsafe { result.value.into_string_with(free_rust_string) };
            (result.status, text)
        };

        let full = AriaPluginV1 {
            struct_size: size_of::<AriaPluginV1>(),
            api_major: ARIA_API_MAJOR,
            reserved: 0,
            free_string: free_rust_string,
            free_bytes: free_rust_bytes,
            mount: noop_ok,
            unmount: noop_ok,
            command_run: noop_cmd,
            event: noop_cmd,
            import: noop_cmd,
            export: noop_export,
            audio_create: noop_audio_create,
            audio_process: noop_audio_proc,
            audio_reset: noop_u64,
            audio_drop: noop_u64,
            panel_view: custom_panel_view,
            panel_event: noop_cmd,
            encoder_open: custom_encoder_open,
            encoder_push_frame: custom_encoder_frame,
            encoder_push_audio: custom_encoder_audio,
            encoder_finish: noop_u64,
            encoder_drop: noop_u64,
            overlay_scene: custom_overlay_scene,
            overlay_event: noop_cmd,
            job_step: custom_job_step,
            job_end: noop_job_end,
            decoder_probe: custom_decoder_probe,
            decoder_open_video: noop_decoder_open_video,
            decoder_seek: noop_decoder_seek,
            decoder_next_frame: noop_decoder_next_frame,
            decoder_open_audio: noop_decoder_open_audio,
            decoder_read_audio: noop_decoder_read_audio,
            decoder_seek_audio: noop_decoder_seek,
            decoder_close: noop_u64,
            ai_tool_call: custom_ai_tool_call,
            scope_measure: custom_scope_measure,
            cpu_effect_process: custom_cpu_effect_process,
            driver_evaluate: custom_driver_evaluate,
            analyzer_open: stub_analyzer_open,
            analyzer_push: stub_analyzer_push,
            analyzer_finish: stub_analyzer_finish,
            analyzer_drop: stub_analyzer_drop,
        };
        let probe = |table: &AriaPluginV1| {
            #[allow(unsafe_code)]
            // SAFETY: `decoder_probe` is the table's own or a stub; both take any strings.
            let result = unsafe {
                (table.decoder_probe)(
                    std::ptr::null_mut(),
                    AriaStr::from_str("tiff"),
                    AriaStr::from_str("/a.tif"),
                )
            };
            #[allow(unsafe_code)]
            // SAFETY: the value was allocated by this crate's `AriaOwnedStr::from_string`.
            let text = unsafe { result.value.into_string_with(free_rust_string) };
            (result.status, text)
        };

        // 1. MIN_SIZE prefix: no panels, no encoders, no overlays; all three stubbed.
        let mut min_table = full;
        min_table.struct_size = ARIA_PLUGIN_V1_MIN_SIZE;
        assert!(!min_table.has_panels());
        assert!(!min_table.has_encoders());
        assert!(!min_table.has_overlays());
        #[allow(unsafe_code)]
        let copied_min =
            unsafe { AriaPluginV1::copy_from_prefix(&raw const min_table, min_table.struct_size) };
        #[allow(unsafe_code)]
        let min_panel =
            unsafe { (copied_min.panel_view)(std::ptr::null_mut(), AriaStr::from_str("p")) };
        assert_eq!(min_panel.status, ARIA_BYTES_ERR);
        #[allow(unsafe_code)]
        let min_panel_err = unsafe { min_panel.error.into_string_with(free_rust_string) };
        assert!(min_panel_err.contains("panel_view"), "{min_panel_err}");

        // 2. PANELS_SIZE prefix: panels preserved, encoders and overlays stubbed.
        let mut panels_table = full;
        panels_table.struct_size = ARIA_PLUGIN_V1_PANELS_SIZE;
        assert!(panels_table.has_panels());
        assert!(!panels_table.has_encoders());
        assert!(!panels_table.has_overlays());
        #[allow(unsafe_code)]
        let copied_panels = unsafe {
            AriaPluginV1::copy_from_prefix(&raw const panels_table, panels_table.struct_size)
        };
        #[allow(unsafe_code)]
        let ok_panel =
            unsafe { (copied_panels.panel_view)(std::ptr::null_mut(), AriaStr::from_str("p")) };
        assert_eq!(ok_panel.status, ARIA_BYTES_FOUND);
        #[allow(unsafe_code)]
        let _ = unsafe { ok_panel.bytes.into_vec_with(free_rust_bytes) };
        #[allow(unsafe_code)]
        let stub_enc = unsafe {
            (copied_panels.encoder_open)(
                std::ptr::null_mut(),
                AriaStr::from_str("e"),
                AriaStr::from_str("p"),
                std::ptr::null(),
            )
        };
        assert_eq!(stub_enc.status, ARIA_STATUS_ERR);
        #[allow(unsafe_code)]
        let stub_enc_err = unsafe { stub_enc.error.into_string_with(free_rust_string) };
        assert!(stub_enc_err.contains("encoder_open"), "{stub_enc_err}");

        // 3. ENCODERS_SIZE prefix: panels and encoders preserved, overlays stubbed.
        let mut encoders_table = full;
        encoders_table.struct_size = ARIA_PLUGIN_V1_ENCODERS_SIZE;
        assert!(encoders_table.has_panels());
        assert!(encoders_table.has_encoders());
        assert!(!encoders_table.has_overlays());
        #[allow(unsafe_code)]
        let copied_encoders = unsafe {
            AriaPluginV1::copy_from_prefix(&raw const encoders_table, encoders_table.struct_size)
        };
        #[allow(unsafe_code)]
        let ok_enc = unsafe {
            (copied_encoders.encoder_open)(
                std::ptr::null_mut(),
                AriaStr::from_str("e"),
                AriaStr::from_str("p"),
                std::ptr::null(),
            )
        };
        assert_eq!(ok_enc.status, ARIA_STATUS_OK);
        assert_eq!(ok_enc.instance, 77);
        #[allow(unsafe_code)]
        let stub_ovl_scene = unsafe {
            (copied_encoders.overlay_scene)(std::ptr::null_mut(), AriaStr::from_str("ovl"))
        };
        assert_eq!(stub_ovl_scene.status, ARIA_BYTES_ERR);
        #[allow(unsafe_code)]
        let stub_ovl_scene_err = unsafe { stub_ovl_scene.error.into_string_with(free_rust_string) };
        assert!(
            stub_ovl_scene_err.contains("overlay_scene"),
            "{stub_ovl_scene_err}"
        );
        #[allow(unsafe_code)]
        let stub_ovl_ev = unsafe {
            (copied_encoders.overlay_event)(
                std::ptr::null_mut(),
                AriaStr::from_str("ovl"),
                AriaStr::from_str("{}"),
            )
        };
        assert_eq!(stub_ovl_ev.status, ARIA_STATUS_ERR);
        #[allow(unsafe_code)]
        let stub_ovl_ev_err = unsafe { stub_ovl_ev.value.into_string_with(free_rust_string) };
        assert!(
            stub_ovl_ev_err.contains("overlay_event"),
            "{stub_ovl_ev_err}"
        );

        // 4. OVERLAYS_SIZE prefix: overlays preserved, jobs stubbed.
        let mut overlays_table = full;
        overlays_table.struct_size = ARIA_PLUGIN_V1_OVERLAYS_SIZE;
        assert!(overlays_table.has_overlays());
        assert!(!overlays_table.has_jobs());
        #[allow(unsafe_code)]
        let copied_overlays = unsafe {
            AriaPluginV1::copy_from_prefix(&raw const overlays_table, overlays_table.struct_size)
        };
        #[allow(unsafe_code)]
        let kept_ovl = unsafe {
            (copied_overlays.overlay_scene)(std::ptr::null_mut(), AriaStr::from_str("ovl"))
        };
        assert_eq!(kept_ovl.status, ARIA_BYTES_FOUND);
        #[allow(unsafe_code)]
        let kept_bytes = unsafe { kept_ovl.bytes.into_vec_with(free_rust_bytes) };
        assert_eq!(kept_bytes, b"overlay-ok");
        let mut step = AriaJobStep {
            progress: -1.0,
            done: 0,
            commit: 0,
        };
        #[allow(unsafe_code)]
        let stub_step =
            unsafe { (copied_overlays.job_step)(std::ptr::null_mut(), 3, &raw mut step) };
        assert_eq!(stub_step.status, ARIA_STATUS_ERR);
        assert_eq!(step.done, 0, "a stubbed job_step leaves the step untouched");
        #[allow(unsafe_code)]
        let stub_step_err = unsafe { stub_step.value.into_string_with(free_rust_string) };
        assert!(stub_step_err.contains("job_step"), "{stub_step_err}");
        #[allow(unsafe_code)]
        let stub_end = unsafe { (copied_overlays.job_end)(std::ptr::null_mut(), 3, 1) };
        assert_eq!(stub_end.status, ARIA_STATUS_ERR);
        #[allow(unsafe_code)]
        let stub_end_err = unsafe { stub_end.value.into_string_with(free_rust_string) };
        assert!(stub_end_err.contains("job_end"), "{stub_end_err}");
        let (status, text) = probe(&copied_overlays);
        assert_eq!(status, ARIA_STATUS_ERR);
        assert!(text.contains("decoder_probe"), "{text}");

        // 5. JOBS_SIZE prefix: jobs preserved, decoders stubbed.
        let mut jobs_table = full;
        jobs_table.struct_size = ARIA_PLUGIN_V1_JOBS_SIZE;
        assert!(jobs_table.has_jobs());
        assert!(!jobs_table.has_decoders());
        #[allow(unsafe_code)]
        let copied_jobs = unsafe {
            AriaPluginV1::copy_from_prefix(&raw const jobs_table, jobs_table.struct_size)
        };
        let mut step = AriaJobStep {
            progress: -1.0,
            done: 0,
            commit: 0,
        };
        #[allow(unsafe_code)]
        let kept_step = unsafe { (copied_jobs.job_step)(std::ptr::null_mut(), 4, &raw mut step) };
        assert_eq!(kept_step.status, ARIA_STATUS_OK);
        #[allow(unsafe_code)]
        let kept_detail = unsafe { kept_step.value.into_string_with(free_rust_string) };
        assert_eq!(kept_detail, "job 4");
        let (status, text) = probe(&copied_jobs);
        assert_eq!(status, ARIA_STATUS_ERR);
        assert!(text.contains("decoder_probe"), "{text}");
        #[allow(unsafe_code)]
        let stub_close = unsafe { (copied_jobs.decoder_close)(std::ptr::null_mut(), 5) };
        assert_eq!(stub_close.status, ARIA_STATUS_ERR);
        #[allow(unsafe_code)]
        let stub_close_err = unsafe { stub_close.value.into_string_with(free_rust_string) };
        assert!(stub_close_err.contains("decoder_close"), "{stub_close_err}");
        let (status, text) = tool_call(&copied_jobs);
        assert_eq!(status, ARIA_STATUS_ERR);
        assert!(text.contains("ai_tool_call"), "{text}");

        // 6. DECODERS_SIZE prefix: decoders preserved, the AI tool and scope
        // calls stubbed.
        let mut decoders_table = full;
        decoders_table.struct_size = ARIA_PLUGIN_V1_DECODERS_SIZE;
        assert!(decoders_table.has_decoders());
        assert!(!decoders_table.has_ai_tools());
        #[allow(unsafe_code)]
        let copied_decoders = unsafe {
            AriaPluginV1::copy_from_prefix(&raw const decoders_table, decoders_table.struct_size)
        };
        assert_eq!(
            probe(&copied_decoders),
            (ARIA_STATUS_UNSUPPORTED, "not tiff".to_owned())
        );
        let (status, text) = tool_call(&copied_decoders);
        assert_eq!(status, ARIA_STATUS_ERR);
        assert!(text.contains("ai_tool_call"), "{text}");
        let (status, text, image) = scope_measure(&copied_decoders);
        assert_eq!(status, ARIA_STATUS_ERR);
        assert!(text.contains("scope_measure"), "{text}");
        assert_eq!(image, [0; 4], "a stub draws nothing");

        // 7. AI_TOOLS_SIZE prefix: the AI tool call preserved, the scope call
        // stubbed.
        let mut tools_table = full;
        tools_table.struct_size = ARIA_PLUGIN_V1_AI_TOOLS_SIZE;
        assert!(tools_table.has_ai_tools());
        assert!(!tools_table.has_scopes());
        #[allow(unsafe_code)]
        let copied_tools = unsafe {
            AriaPluginV1::copy_from_prefix(&raw const tools_table, tools_table.struct_size)
        };
        assert_eq!(
            tool_call(&copied_tools),
            (ARIA_STATUS_OK, "tool-ok".to_owned())
        );
        let (status, text, _) = scope_measure(&copied_tools);
        assert_eq!(status, ARIA_STATUS_ERR);
        assert!(text.contains("scope_measure"), "{text}");

        // 7b. SCOPES_SIZE prefix: scope_measure preserved, cpu_effect_process and driver_evaluate stubbed.
        let mut scopes_table = full;
        scopes_table.struct_size = ARIA_PLUGIN_V1_SCOPES_SIZE;
        assert!(scopes_table.has_scopes());
        assert!(!scopes_table.has_contributions());
        #[allow(unsafe_code)]
        let copied_scopes = unsafe {
            AriaPluginV1::copy_from_prefix(&raw const scopes_table, scopes_table.struct_size)
        };
        assert_eq!(
            scope_measure(&copied_scopes),
            (ARIA_STATUS_OK, "scope-ok".to_owned(), [10, 20, 30, 255])
        );
        #[allow(unsafe_code)]
        let stub_cpu = unsafe {
            (copied_scopes.cpu_effect_process)(
                std::ptr::null_mut(),
                AriaStr::from_str("fx"),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(stub_cpu.status, ARIA_STATUS_ERR);
        #[allow(unsafe_code)]
        let stub_cpu_err = unsafe { stub_cpu.value.into_string_with(free_rust_string) };
        assert!(
            stub_cpu_err.contains("cpu_effect_process"),
            "{stub_cpu_err}"
        );
        #[allow(unsafe_code)]
        let stub_drv = unsafe {
            (copied_scopes.driver_evaluate)(
                std::ptr::null_mut(),
                AriaStr::from_str("lang"),
                AriaStr::from_str("x"),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(stub_drv.status, ARIA_STATUS_ERR);
        #[allow(unsafe_code)]
        let stub_drv_err = unsafe { stub_drv.value.into_string_with(free_rust_string) };
        assert!(stub_drv_err.contains("driver_evaluate"), "{stub_drv_err}");
        // 8. Full table: every tier present and every callback is the plugin's own.
        assert!(full.has_panels());
        assert!(full.has_encoders());
        assert!(full.has_overlays());
        assert!(full.has_jobs());
        assert!(full.has_decoders());
        assert!(full.has_ai_tools());
        assert!(full.has_scopes());
        assert!(full.has_contributions());
        #[allow(unsafe_code)]
        let copied_full =
            unsafe { AriaPluginV1::copy_from_prefix(&raw const full, full.struct_size) };
        #[allow(unsafe_code)]
        let ok_ovl =
            unsafe { (copied_full.overlay_scene)(std::ptr::null_mut(), AriaStr::from_str("ovl")) };
        assert_eq!(ok_ovl.status, ARIA_BYTES_FOUND);
        #[allow(unsafe_code)]
        let ovl_bytes = unsafe { ok_ovl.bytes.into_vec_with(free_rust_bytes) };
        assert_eq!(ovl_bytes, b"overlay-ok");
        let mut step = AriaJobStep {
            progress: -1.0,
            done: 0,
            commit: 0,
        };
        #[allow(unsafe_code)]
        let ok_step = unsafe { (copied_full.job_step)(std::ptr::null_mut(), 9, &raw mut step) };
        assert_eq!(ok_step.status, ARIA_STATUS_OK);
        assert_eq!((step.progress, step.done, step.commit), (0.5, 1, 0));
        #[allow(unsafe_code)]
        let detail = unsafe { ok_step.value.into_string_with(free_rust_string) };
        assert_eq!(detail, "job 9");
        assert_eq!(
            probe(&copied_full),
            (ARIA_STATUS_UNSUPPORTED, "not tiff".to_owned())
        );
        assert_eq!(
            tool_call(&copied_full),
            (ARIA_STATUS_OK, "tool-ok".to_owned())
        );
        assert_eq!(
            scope_measure(&copied_full),
            (ARIA_STATUS_OK, "scope-ok".to_owned(), [10, 20, 30, 255])
        );
        let cpu_in = [0x4400u16, 0xc000, 0x3800, 0x3c00];
        let mut cpu_out = [0u16; 4];
        let mut cpu_call = AriaCpuEffectCall {
            input: cpu_in.as_ptr(),
            output: cpu_out.as_mut_ptr(),
            pixels_len: 4,
            width: 1,
            height: 1,
            params: std::ptr::null(),
            params_len: 0,
            params_json: AriaStr::from_str(""),
            time: 0.0,
        };
        #[allow(unsafe_code)]
        let ok_cpu = unsafe {
            (copied_full.cpu_effect_process)(
                std::ptr::null_mut(),
                AriaStr::from_str("fx"),
                &raw mut cpu_call,
            )
        };
        assert_eq!(ok_cpu.status, ARIA_STATUS_OK);
        assert_eq!(cpu_out, cpu_in);
    }
}
