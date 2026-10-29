//! Static source probing and pixel-format mapping for the AVFoundation
//! backend: track lookup, colorimetry off the `CMFormatDescription`, frame
//! rate, rotation, FourCC to [`PixelFormat`], and the CPU plane copy.
//!
//! Declared from `apple.rs` with `#[path]`; `use super::*` reaches that module's
//! private items and its own imports, so the move changed no name resolution.
//! The module calls AVFoundation, CoreMedia, and CoreVideo FFI and is `unsafe`
//! throughout.

#![allow(unsafe_code)]

use super::*;

/// Find the first video track of an asset (synchronous probe).
pub(super) fn first_video_track(asset: &AVURLAsset) -> Option<Retained<AVAssetTrack>> {
    let media_video = unsafe { AVMediaTypeVideo }?;
    #[allow(deprecated)]
    let tracks = unsafe { asset.tracksWithMediaType(media_video) };
    tracks.firstObject()
}

/// Duration of the first audio track at `path`, for probing sources with no
/// video stream (music/voiceover files). `None` when there is no audio track
/// or the container reports no duration.
pub(crate) fn audio_track_duration(path: &Path) -> Option<RationalTime> {
    let track = first_audio_track(path)?;
    cmtime_to_rational(unsafe { track.timeRange() }.duration)
}

fn first_audio_track(path: &Path) -> Option<Retained<AVAssetTrack>> {
    let path_str = path.to_str()?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path_str));
    let asset = unsafe { AVURLAsset::initWithURL_options(AVURLAsset::alloc(), &url, None) };
    let media_audio = unsafe { AVMediaTypeAudio }?;
    #[allow(deprecated)]
    let tracks = unsafe { asset.tracksWithMediaType(media_audio) };
    tracks.firstObject()
}

/// Probe static source properties from the track.
pub(super) fn probe(track: &AVAssetTrack) -> SourceInfo {
    let size: CGSize = unsafe { track.naturalSize() };
    let display_w = size.width.round().max(0.0) as u32;
    let display_h = size.height.round().max(0.0) as u32;

    let transform: CGAffineTransform = unsafe { track.preferredTransform() };
    let rotation = rotation_from_transform(transform);

    let frame_rate = frame_rate_of(track);
    let time_base = Rational::new(unsafe { track.naturalTimeScale() }.max(1), 1);
    // Track-level duration (CMTime) at the source's own timescale; consumers
    // resample to whatever rate they need (e.g. the media pool's frame count).
    let duration = cmtime_to_rational(unsafe { track.timeRange() }.duration);

    SourceInfo {
        coded_size: (display_w, display_h),
        display_size: (display_w, display_h),
        rotation,
        pixel_format: PixelFormat::Nv12,
        // Primaries/transfer/matrix come from the track's format description;
        // `range` is refined per frame from the actual pixel format.
        color: color_from_track(track),
        frame_rate,
        time_base,
        duration,
    }
}

/// Read colorimetry (primaries / transfer / matrix) from the track's first
/// `CMFormatDescription`. Missing tags keep the BT.709 SDR default; `range` is
/// left at the default and refined per frame from the pixel format.
fn color_from_track(track: &AVAssetTrack) -> ColorSpace {
    let mut color = ColorSpace::BT709;

    let descs = unsafe { track.formatDescriptions() };
    let Some(obj) = descs.firstObject() else {
        return color;
    };
    // The array holds `CMFormatDescription`s (a CoreMedia CFType) bridged in as
    // objects; reinterpret the element to reach the extension accessors.
    let desc: &CMFormatDescription =
        unsafe { &*(Retained::as_ptr(&obj).cast::<CMFormatDescription>()) };

    if let Some(p) = read_tag(
        desc,
        unsafe { kCMFormatDescriptionExtension_ColorPrimaries },
        map_primaries,
    ) {
        color.primaries = p;
    }
    if let Some(t) = read_tag(
        desc,
        unsafe { kCMFormatDescriptionExtension_TransferFunction },
        map_transfer,
    ) {
        color.transfer = t;
    }
    if let Some(m) = read_tag(
        desc,
        unsafe { kCMFormatDescriptionExtension_YCbCrMatrix },
        map_matrix,
    ) {
        color.matrix = m;
    }
    color
}

/// Look up a format-description extension expected to be a `CFString` tag and
/// run it through `map`. Returns `None` when the tag is absent or not a string.
fn read_tag<T>(
    desc: &CMFormatDescription,
    key: &CFString,
    map: impl Fn(&CFString) -> T,
) -> Option<T> {
    let value = unsafe { desc.extension(key) }?;
    let tag = value.downcast_ref::<CFString>()?;
    Some(map(tag))
}

/// CFString content equality (`CFEqual`) against a known CoreMedia constant.
fn cf_tag_eq(value: &CFString, constant: &CFString) -> bool {
    let value: &CFType = value;
    let constant: &CFType = constant;
    value == constant
}

pub(super) fn map_primaries(value: &CFString) -> ColorPrimaries {
    if cf_tag_eq(value, unsafe {
        kCMFormatDescriptionColorPrimaries_ITU_R_709_2
    }) {
        ColorPrimaries::Bt709
    } else if cf_tag_eq(value, unsafe {
        kCMFormatDescriptionColorPrimaries_ITU_R_2020
    }) {
        ColorPrimaries::Bt2020
    } else if cf_tag_eq(value, unsafe { kCMFormatDescriptionColorPrimaries_P3_D65 })
        || cf_tag_eq(value, unsafe { kCMFormatDescriptionColorPrimaries_DCI_P3 })
    {
        ColorPrimaries::DisplayP3
    } else if cf_tag_eq(value, unsafe { kCMFormatDescriptionColorPrimaries_SMPTE_C }) {
        ColorPrimaries::Bt601_525
    } else if cf_tag_eq(value, unsafe {
        kCMFormatDescriptionColorPrimaries_EBU_3213
    }) {
        ColorPrimaries::Bt601_625
    } else {
        ColorPrimaries::Unspecified
    }
}

pub(super) fn map_transfer(value: &CFString) -> TransferFunction {
    // Rec.2020's SDR transfer is, for our purposes, the Rec.709 curve.
    if cf_tag_eq(value, unsafe {
        kCMFormatDescriptionTransferFunction_ITU_R_709_2
    }) || cf_tag_eq(value, unsafe {
        kCMFormatDescriptionTransferFunction_ITU_R_2020
    }) {
        TransferFunction::Bt709
    } else if cf_tag_eq(value, unsafe {
        kCMFormatDescriptionTransferFunction_SMPTE_ST_2084_PQ
    }) {
        TransferFunction::Pq
    } else if cf_tag_eq(value, unsafe {
        kCMFormatDescriptionTransferFunction_ITU_R_2100_HLG
    }) {
        TransferFunction::Hlg
    } else if cf_tag_eq(value, unsafe { kCMFormatDescriptionTransferFunction_sRGB }) {
        TransferFunction::Srgb
    } else {
        TransferFunction::Unspecified
    }
}

pub(super) fn map_matrix(value: &CFString) -> MatrixCoefficients {
    if cf_tag_eq(value, unsafe {
        kCMFormatDescriptionYCbCrMatrix_ITU_R_709_2
    }) {
        MatrixCoefficients::Bt709
    } else if cf_tag_eq(value, unsafe {
        kCMFormatDescriptionYCbCrMatrix_ITU_R_601_4
    }) {
        MatrixCoefficients::Bt601
    } else if cf_tag_eq(value, unsafe { kCMFormatDescriptionYCbCrMatrix_ITU_R_2020 }) {
        MatrixCoefficients::Bt2020Ncl
    } else {
        MatrixCoefficients::Unspecified
    }
}

/// Frame rate from the track's minimum frame duration (exact for constant
/// frame rates), guarded against VFR by the nominal average rate.
fn frame_rate_of(track: &AVAssetTrack) -> Rational {
    let min_dur = unsafe { track.minFrameDuration() };
    // fps = timescale / value (frames per second).
    let exact = (min_dur.value > 0 && min_dur.timescale > 0)
        .then(|| Rational::new(min_dur.timescale, min_dur.value as i32));
    resolve_frame_rate(exact, unsafe { track.nominalFrameRate() })
}

/// Pick the reported frame rate: the exact minimum-frame-duration rate when
/// it agrees with the nominal average, the average otherwise. On VFR sources
/// the smallest inter-frame gap is an artifact, not the cadence — reporting
/// it inflates every duration-derived frame count, and the pool then asks
/// for frames past EOF.
pub(super) fn resolve_frame_rate(
    min_duration_rate: Option<Rational>,
    nominal_fps: f32,
) -> Rational {
    if let Some(exact) = min_duration_rate {
        // 5%: comfortably above timescale rounding (23.976 vs 23.98), well
        // below any real VFR spread.
        if nominal_fps <= 0.0 || exact.as_f64() <= f64::from(nominal_fps) * 1.05 {
            return exact;
        }
    }
    if nominal_fps > 0.0 {
        Rational::new((nominal_fps * 1000.0).round() as i32, 1000)
    } else {
        Rational::FPS_30
    }
}

/// Derive a 0/90/180/270 rotation from a track's preferred transform.
pub(super) fn rotation_from_transform(t: CGAffineTransform) -> Rotation {
    let degrees = t.b.atan2(t.a).to_degrees().round() as i32;
    Rotation::from_degrees(degrees).unwrap_or(Rotation::None)
}

/// Map a CoreVideo FourCC pixel format to ours plus its sample range.
pub(super) fn detect_format(fourcc: u32) -> Option<(PixelFormat, ColorRange)> {
    match fourcc {
        FOURCC_420V => Some((PixelFormat::Nv12, ColorRange::Limited)),
        FOURCC_420F => Some((PixelFormat::Nv12, ColorRange::Full)),
        FOURCC_X420 => Some((PixelFormat::P010, ColorRange::Limited)),
        FOURCC_BGRA => Some((PixelFormat::Bgra8, ColorRange::Full)),
        _ => None,
    }
}

/// Copy every plane of a (locked) pixel buffer into owned [`Plane`]s.
pub(super) fn read_cpu_planes(pb: &CVPixelBuffer) -> Result<Vec<Plane>, DecodeError> {
    unsafe {
        if CVPixelBufferLockBaseAddress(pb, CVPixelBufferLockFlags::ReadOnly) != 0 {
            return Err(DecodeError::Decode(
                "CVPixelBufferLockBaseAddress failed".into(),
            ));
        }
        let plane_count = CVPixelBufferGetPlaneCount(pb);
        let mut planes = Vec::with_capacity(plane_count.max(1));
        for i in 0..plane_count {
            let base = CVPixelBufferGetBaseAddressOfPlane(pb, i);
            let stride = CVPixelBufferGetBytesPerRowOfPlane(pb, i);
            let rows = CVPixelBufferGetHeightOfPlane(pb, i);
            let Some(base) = NonNull::new(base) else {
                CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags::ReadOnly);
                return Err(DecodeError::Decode(format!(
                    "plane {i} base address is null"
                )));
            };
            let bytes = slice::from_raw_parts(base.as_ptr() as *const u8, stride * rows).to_vec();
            planes.push(Plane::new(bytes, stride, rows));
        }
        CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags::ReadOnly);
        Ok(planes)
    }
}
