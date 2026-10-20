//! Opening and configuring a Media Foundation source reader, and probing the
//! static source properties off its negotiated output type.
//!
//! Declared from `wmf.rs` with `#[path]`; `use super::*` reaches that module's
//! private items and its own imports, so the move changed no name resolution.

use super::*;

/// Start the Media Foundation platform once per process. The matching
/// `MFShutdown` is intentionally skipped: the platform lives for the process
/// (like a global logger), and decoders open/close freely against it.
pub(crate) fn ensure_mf_platform() {
    static MF_PLATFORM: Once = Once::new();
    MF_PLATFORM.call_once(|| unsafe {
        let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
    });
}

/// Duration of the first audio track of `path`, for the audio-only probe
/// fallback (music/voiceover files with no video stream). `None` when the
/// file has no audio track or reports no duration.
pub(crate) fn audio_track_duration(path: &Path) -> Option<RationalTime> {
    ensure_mf_platform();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let reader = open_source_reader(path, None).ok()?;
    unsafe {
        reader
            .GetNativeMediaType(MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32, 0)
            .ok()?;
    }
    probe_duration(&reader, Some(path))
}

/// Open a Media Foundation source reader for the local file at `path`,
/// optionally with reader-configuration `attributes`.
///
/// Prefers a **self-opened byte stream** over [`MFCreateSourceReaderFromURL`].
/// Once the process brings up a Direct3D device (wgpu, in the desktop app),
/// MF's URL *source resolver* starts failing local-file opens with
/// `ERROR_BAD_NETPATH` (`0x80070035`) — it misroutes a plain `C:\…` path as if
/// it were a UNC/network path. Opening the file ourselves with [`MFCreateFile`]
/// and resolving the source from that byte stream sidesteps the URL resolver
/// entirely (plain file I/O is unaffected by the D3D init). The URL path stays
/// as a fallback for the pre-D3D case and anything the byte-stream resolver
/// can't handle (e.g. non-`file:` URLs, should we ever pass one).
pub(crate) fn open_source_reader(
    path: &Path,
    attributes: Option<&IMFAttributes>,
) -> Result<IMFSourceReader, DecodeError> {
    let raw = path
        .to_str()
        .ok_or_else(|| DecodeError::Open("path is not valid UTF-8".into()))?;
    let url = HSTRING::from(mf_path(raw));
    match reader_from_byte_stream(&url, attributes) {
        Ok(reader) => Ok(reader),
        // `&HSTRING: Param<PCWSTR>`.
        Err(stream_err) => {
            unsafe { MFCreateSourceReaderFromURL(&url, attributes) }.map_err(|url_err| {
                // An unrecognized container is a capability gap, not an I/O
                // failure; the error states which one it is.
                if stream_err.code() == MF_E_UNSUPPORTED_BYTESTREAM_TYPE
                    || url_err.code() == MF_E_UNSUPPORTED_BYTESTREAM_TYPE
                {
                    return DecodeError::Unsupported(
                        "Media Foundation does not recognize this container format".into(),
                    );
                }
                DecodeError::Open(format!(
                    "byte-stream open failed ({stream_err}); URL open failed ({url_err})"
                ))
            })
        }
    }
}

/// The path Media Foundation can open: `canonicalize` returns verbatim
/// (`\\?\`-prefixed) paths on Windows, but MF resolves mapped drives through
/// the DOS device namespace, which the verbatim prefix bypasses — the same
/// file then fails the byte-stream open as not-found and the URL fallback as
/// `ERROR_BAD_NETPATH`. Strip the prefix (and fold the `\\?\UNC\` server form
/// back to `\\`) so both opens see an ordinary absolute path.
pub(super) fn mf_path(raw: &str) -> String {
    const UNC: &str = r"\\?\UNC\";
    const VERBATIM: &str = r"\\?\";
    if let Some(rest) = raw.strip_prefix(UNC) {
        format!(r"\\{rest}")
    } else if let Some(rest) = raw.strip_prefix(VERBATIM) {
        rest.to_owned()
    } else {
        raw.to_owned()
    }
}

/// Open `url` as a file byte stream and build a source reader from it, so the
/// source is resolved via `CreateObjectFromByteStream` rather than the URL
/// resolver. See [`open_source_reader`] for why this is preferred.
fn reader_from_byte_stream(
    url: &HSTRING,
    attributes: Option<&IMFAttributes>,
) -> Result<IMFSourceReader, windows::core::Error> {
    let stream: IMFByteStream = unsafe {
        MFCreateFile(
            MF_ACCESSMODE_READ,
            MF_OPENMODE_FAIL_IF_NOT_EXIST,
            MF_FILEFLAGS_NONE,
            url,
        )
    }?;
    unsafe { MFCreateSourceReaderFromByteStream(&stream, attributes) }
}

/// Reader-creation attributes for the hardware decode path: the shared D3D11
/// device (so decoder MFTs output DXGI textures) and permission to load
/// hardware transforms at all (off by default in the source reader).
///
/// `MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING` is deliberately *not*
/// set: it would splice in a video processor that auto-rotates and converts,
/// while rotation/colorimetry are already reported to (and applied by) the
/// renderer — the processor would double-apply them.
fn video_reader_attributes(d3d: &D3dShared) -> Result<IMFAttributes, windows::core::Error> {
    let mut attributes: Option<IMFAttributes> = None;
    unsafe { MFCreateAttributes(&mut attributes, 2) }?;
    let attributes = attributes.expect("MFCreateAttributes succeeded");
    unsafe {
        attributes.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, &d3d.manager)?;
        attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
    }
    Ok(attributes)
}

/// Open a source reader for `path` and negotiate NV12 on the video stream —
/// in hardware when `d3d` is given, in software otherwise.
pub(super) fn configure_reader(
    path: &Path,
    stream: u32,
    d3d: Option<&D3dShared>,
) -> Result<IMFSourceReader, DecodeError> {
    let attributes = match d3d {
        Some(d3d) => Some(video_reader_attributes(d3d).map_err(open_err)?),
        None => None,
    };
    let reader = open_source_reader(path, attributes.as_ref())?;

    // Decode only the video track (leave audio deselected so MF never spins
    // up an audio decoder for it).
    unsafe {
        reader
            .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
            .map_err(open_err)?;
        reader.SetStreamSelection(stream, true).map_err(open_err)?;
    }

    // Request uncompressed NV12. Setting an uncompressed subtype is what
    // makes the reader insert the decoder (and a converter for 10-bit
    // sources); without it the reader would hand back compressed samples.
    let out_type = unsafe { MFCreateMediaType() }.map_err(open_err)?;
    unsafe {
        out_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(open_err)?;
        out_type
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
            .map_err(open_err)?;
        reader
            .SetCurrentMediaType(stream, None, &out_type)
            .map_err(negotiate_err)?;
    }
    Ok(reader)
}

/// Map an NV12-negotiation failure to a [`DecodeError::Unsupported`], turning
/// the common "this machine can't decode this file" HRESULTs into
/// plain-language messages instead of raw COM errors.
fn negotiate_err(error: windows::core::Error) -> DecodeError {
    let reason = match error.code() {
        MF_E_TOPO_CODEC_NOT_FOUND => "no decoder is installed for this codec".into(),
        MF_E_INVALIDMEDIATYPE => "the decoder cannot produce NV12 for this stream".into(),
        MF_E_UNSUPPORTED_D3D_TYPE => "the hardware decoder rejected this stream's format".into(),
        _ => format!("could not negotiate NV12 output: {error}"),
    };
    DecodeError::Unsupported(reason)
}

/// Source duration from the presentation descriptor (`MF_PD_DURATION`, a
/// `VT_UI8` count of 100-ns units), or `None` when the container doesn't
/// report one.
///
/// **Fragmented MP4s report no duration here.** MF's MPEG-4 media source only
/// surfaces `MF_PD_DURATION` from the `moov/mvhd` movie header; fragmented
/// files (common for downloaded/streamed footage) declare `0` there and carry
/// real timing in per-fragment `moof` boxes, so the attribute is simply
/// absent. For those, fall back to scanning the container's metadata boxes
/// ourselves ([`crate::mp4_duration`]) — without a duration the editor would
/// treat the source as zero-length and reject every clip placement.
pub(crate) fn probe_duration(
    reader: &IMFSourceReader,
    path: Option<&Path>,
) -> Option<RationalTime> {
    let from_source = unsafe {
        reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
    }
    .ok()
    .and_then(|value| u64::try_from(&value).ok())
    .filter(|hns| *hns > 0)
    .map(|hns| RationalTime::new(hns as i64, Rational::new(HNS_PER_SEC as i32, 1)));
    from_source.or_else(|| crate::mp4_duration::duration_from_file(path?))
}

/// Read static source properties from the reader's negotiated output type.
/// Returns the info plus the visible rect within the coded frame.
///
/// `path` (when available) feeds the fragmented-MP4 duration fallback; pass
/// `None` for mid-stream re-probes where the duration is already known.
pub(super) fn probe(
    reader: &IMFSourceReader,
    stream: u32,
    path: Option<&Path>,
) -> Result<(SourceInfo, Rect), DecodeError> {
    let media_type = unsafe { reader.GetCurrentMediaType(stream) }.map_err(open_err)?;

    // FRAME_SIZE packs width in the high 32 bits, height in the low 32.
    let frame_size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }.map_err(open_err)?;
    let width = (frame_size >> 32) as u32;
    let height = (frame_size & 0xffff_ffff) as u32;

    // The clean-aperture crop. Software decoders usually pre-crop FRAME_SIZE,
    // but hardware (DXVA) decoders report the macroblock-padded coded size
    // (e.g. 1920x1088) and describe the visible 1920x1080 here — without the
    // crop those padding rows would show as green/garbage edges.
    let visible = display_aperture(&media_type, width, height)
        .unwrap_or_else(|| Rect::from_size(width, height));

    // FRAME_RATE packs numerator in the high 32 bits, denominator in the low 32.
    let frame_rate = match unsafe { media_type.GetUINT64(&MF_MT_FRAME_RATE) } {
        Ok(packed) => {
            let num = (packed >> 32) as u32;
            let den = (packed & 0xffff_ffff) as u32;
            Rational::new(num.max(1) as i32, den.max(1) as i32)
        }
        Err(_) => Rational::FPS_30,
    };

    // VIDEO_ROTATION is stored as degrees clockwise (0/90/180/270).
    let rotation = unsafe { media_type.GetUINT32(&MF_MT_VIDEO_ROTATION) }
        .ok()
        .and_then(|deg| Rotation::from_degrees(deg as i32))
        .unwrap_or(Rotation::None);

    let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }.ok();
    let pixel_format = subtype_to_format(subtype).ok_or_else(|| {
        DecodeError::unsupported(format!("Media Foundation pixel subtype {subtype:?}"))
    })?;
    let color = color_from_type(&media_type);

    let info = SourceInfo {
        coded_size: (width, height),
        display_size: (visible.width, visible.height),
        rotation,
        pixel_format,
        color,
        frame_rate,
        time_base: Rational::new(HNS_PER_SEC as i32, 1),
        duration: probe_duration(reader, path),
    };
    Ok((info, visible))
}

/// Parse `MF_MT_MINIMUM_DISPLAY_APERTURE` into a [`Rect`], clamped to the
/// coded size. `None` when the attribute is absent, malformed, or degenerate
/// (callers then treat the full coded frame as visible).
fn display_aperture(media_type: &IMFMediaType, coded_w: u32, coded_h: u32) -> Option<Rect> {
    let mut area = MFVideoArea::default();
    unsafe {
        media_type.GetBlob(
            &MF_MT_MINIMUM_DISPLAY_APERTURE,
            core::slice::from_raw_parts_mut(
                (&raw mut area).cast::<u8>(),
                core::mem::size_of::<MFVideoArea>(),
            ),
            None,
        )
    }
    .ok()?;
    // MFOffset is a fixed-point value; only whole-pixel offsets make sense
    // for a crop, so the fractional part is ignored.
    let x = u32::try_from(area.OffsetX.value).ok()?.min(coded_w);
    let y = u32::try_from(area.OffsetY.value).ok()?.min(coded_h);
    let w = u32::try_from(area.Area.cx).ok()?.min(coded_w - x);
    let h = u32::try_from(area.Area.cy).ok()?.min(coded_h - y);
    let rect = Rect::new(x, y, w, h);
    (!rect.is_empty()).then_some(rect)
}

/// Pull colorimetry from the output type's color attributes, falling back to the
/// BT.709 SDR default for any attribute the source under-signals.
fn color_from_type(media_type: &IMFMediaType) -> ColorSpace {
    let mut color = ColorSpace::BT709;
    if let Ok(v) = unsafe { media_type.GetUINT32(&MF_MT_VIDEO_PRIMARIES) } {
        color.primaries = map_primaries(v);
    }
    if let Ok(v) = unsafe { media_type.GetUINT32(&MF_MT_TRANSFER_FUNCTION) } {
        color.transfer = map_transfer(v);
    }
    if let Ok(v) = unsafe { media_type.GetUINT32(&MF_MT_YUV_MATRIX) } {
        color.matrix = map_matrix(v);
    }
    if let Ok(v) = unsafe { media_type.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE) } {
        color.range = map_range(v);
    }
    color
}

/// Map an `MF_MT_SUBTYPE` GUID to a declared pixel format.
pub(super) fn subtype_to_format(subtype: Option<GUID>) -> Option<PixelFormat> {
    match subtype {
        Some(guid) => crate::pixel_map::wmf_biplanar_layout(
            guid == MFVideoFormat_NV12,
            guid == MFVideoFormat_P010,
        ),
        None => None,
    }
}

/// Map an `MFVideoPrimaries` code to [`ColorPrimaries`].
pub(super) fn map_primaries(value: u32) -> ColorPrimaries {
    if value == MFVideoPrimaries_BT709.0 as u32 {
        ColorPrimaries::Bt709
    } else if value == MFVideoPrimaries_SMPTE170M.0 as u32 {
        ColorPrimaries::Bt601_525
    } else if value == MFVideoPrimaries_BT470_2_SysBG.0 as u32 {
        ColorPrimaries::Bt601_625
    } else if value == MFVideoPrimaries_BT2020.0 as u32 {
        ColorPrimaries::Bt2020
    } else if value == MFVideoPrimaries_DCI_P3.0 as u32 {
        ColorPrimaries::DisplayP3
    } else {
        ColorPrimaries::Unspecified
    }
}

/// Map an `MFVideoTransferFunction` code to [`TransferFunction`].
pub(super) fn map_transfer(value: u32) -> TransferFunction {
    if value == MFVideoTransFunc_709.0 as u32 {
        TransferFunction::Bt709
    } else if value == MFVideoTransFunc_sRGB.0 as u32 {
        TransferFunction::Srgb
    } else if value == MFVideoTransFunc_2084.0 as u32 {
        TransferFunction::Pq
    } else if value == MFVideoTransFunc_HLG.0 as u32 {
        TransferFunction::Hlg
    } else {
        TransferFunction::Unspecified
    }
}

/// Map an `MFVideoTransferMatrix` code to [`MatrixCoefficients`].
pub(super) fn map_matrix(value: u32) -> MatrixCoefficients {
    if value == MFVideoTransferMatrix_BT709.0 as u32 {
        MatrixCoefficients::Bt709
    } else if value == MFVideoTransferMatrix_BT601.0 as u32 {
        MatrixCoefficients::Bt601
    } else if value == MFVideoTransferMatrix_BT2020_10.0 as u32
        || value == MFVideoTransferMatrix_BT2020_12.0 as u32
    {
        MatrixCoefficients::Bt2020Ncl
    } else {
        MatrixCoefficients::Unspecified
    }
}

/// Map an `MFNominalRange` code to [`ColorRange`]. Only the explicit full-range
/// code (`0_255`) is full; limited and unknown both map to limited (the video
/// norm).
pub(super) fn map_range(value: u32) -> ColorRange {
    if value == MFNominalRange_0_255.0 as u32 {
        ColorRange::Full
    } else {
        ColorRange::Limited
    }
}
