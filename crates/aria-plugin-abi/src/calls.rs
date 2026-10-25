use super::AriaStr;
#[cfg(doc)]
use super::{ARIA_FORMAT_NV12, ARIA_FORMAT_YUV420P, ARIA_STATUS_OK};

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
/// color with each channel from 0 to 1).
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
    /// Red channel of a color.
    pub r: u8,
    /// Green channel of a color.
    pub g: u8,
    /// Blue channel of a color.
    pub b: u8,
    /// Alpha channel of a color.
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
