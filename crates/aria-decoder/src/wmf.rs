//! Windows video decoding via Media Foundation's **Source Reader**.
//!
//! [`IMFSourceReader`] is the Windows analogue of Apple's `AVAssetReader`: one
//! object that demuxes the container *and* drives the hardware (or software)
//! decoder, so we feed it a URL and pull decoded samples. Control stays in Rust
//! behind [`aria_core::VideoDecoder`]; only the codec call is native.
//!
//! ## What this backend does today
//!
//! - Attaches the process's **D3D11 device manager** so the reader loads the
//!   GPU vendor's hardware decoder MFT (DXVA); machines where that fails
//!   (VMs, RDP, driver trouble) fall back to Microsoft's software decoders
//!   transparently ([`crate::wmf_d3d`]).
//! - Forces an **uncompressed NV12** output type, which makes the reader insert
//!   a decoder (+ a converter when the codec is 10-bit).
//! - **CPU output**: reads each sample's [`IMFMediaBuffer`] stride-aware (via
//!   [`IMF2DBuffer`] when available, falling back to a flat `Lock`) into
//!   [`FrameData::Cpu`] — for DXGI-backed samples the lock performs the
//!   staging readback internally.
//! - **GPU output** ([`crate::OutputMode::Gpu`], best effort): exports the
//!   sample's decoder-owned texture slice through a shared NV12 texture pool +
//!   shared fence ([`crate::wmf_gpu`]) as [`FrameData::Gpu`] — no CPU pixel
//!   copy. Sources/machines that can't (software path, pre-fence OS) degrade
//!   to CPU planes rather than failing.
//! - Probes size / rotation / frame-rate / colorimetry and the clean-aperture
//!   crop (`MF_MT_MINIMUM_DISPLAY_APERTURE` — hardware decoders report
//!   macroblock-padded coded sizes) from the negotiated output
//!   [`IMFMediaType`], duration from the presentation descriptor
//!   (`MF_PD_DURATION`) with a fragmented-MP4 fallback
//!   ([`crate::mp4_duration`]), and seeks with `SetCurrentPosition` (100-ns
//!   units).
//! - Answers [`VideoDecoder::frame_at`] with the shared roll-forward policy
//!   ([`crate::seek`]) so sequential playback / forward scrubbing never pays a
//!   seek-plus-GOP-re-decode per frame.
//!
//! ## 10-bit / HDR (P010)
//!
//! P010 passthrough is **documented-but-off**: the compositor has no 16-bit
//! YUV sampling path yet, so the reader keeps forcing NV12 and 10-bit sources
//! decode through MF's converter (a quality, not correctness, limitation).
//! When the compositor grows P010 planes this backend only needs the forced
//! subtype relaxed.
//!
//! ## Measured (4K H.264 3840×2160@29.97, Intel UHD 630, Win11)
//!
//! `decode_bench` / `composite_bench` means over 150 frames:
//!
//! | path | per frame | eff fps |
//! |---|---|---|
//! | software decode → CPU planes | 12.2 ms | 71 |
//! | hardware decode → CPU readback | 19.4 ms | 49 |
//! | hardware decode → GPU surface (zero-copy) | 5.0 ms | 199 |
//! | decode + composite + readback, CPU path | 41.1 ms | 23 |
//! | decode + composite + readback, zero-copy | 12.9 ms | 74 |
//!
//! Hardware decode with CPU *readback* loses to software on mean latency
//! (the 12 MB/frame NV12 copy over PCIe dominates) but wins on p95 and CPU
//! load; the zero-copy path removes the readback and the re-upload, which is
//! where the 3×+ end-to-end win comes from. Sequential `frame_at` overhead on
//! top of raw decode is ~0.9×–1.1×; seek-per-frame is ~80× slower than the
//! roll-forward path it replaced (see [`crate::seek`]).
//!
//! **`MF_LOW_LATENCY` was measured and rejected.** Setting it on the reader
//! (same clip/machine, `decode_bench` over 150 frames) improved only the
//! first frame after a cold open (CPU 109→84 ms, GPU 38→32 ms) while
//! regressing everything sustained: sequential decode 20.4→24.3 ms mean CPU
//! (−19%) and 5.75→6.03 ms GPU, seek-per-frame 1 545→1 638 ms, random scrub
//! 2 363→2 539 ms. The flag caps the decoder MFT's internal pipelining, and
//! seek cost here *is* prefix-decode throughput (long GOPs), so shrinking
//! the queue makes seeks slower, not faster.
//!
//! ## Time base
//!
//! Media Foundation timestamps are in **100-nanosecond units** (`hns`), so every
//! frame's PTS rate — and [`SourceInfo::time_base`] — is [`HNS_PER_SEC`].

use core::slice;
use std::path::Path;
use std::sync::{Arc, Once};

use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFAttributes, IMFByteStream, IMFDXGIBuffer, IMFMediaBuffer, IMFMediaType,
    IMFSample, IMFSourceReader, MF_ACCESSMODE_READ, MF_E_INVALIDMEDIATYPE,
    MF_E_TOPO_CODEC_NOT_FOUND, MF_E_UNSUPPORTED_BYTESTREAM_TYPE, MF_E_UNSUPPORTED_D3D_TYPE,
    MF_FILEFLAGS_NONE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
    MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION,
    MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_VIDEO_ROTATION, MF_MT_YUV_MATRIX,
    MF_OPENMODE_FAIL_IF_NOT_EXIST, MF_PD_DURATION, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
    MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_D3D_MANAGER,
    MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_SOURCE_READER_MEDIASOURCE, MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR, MF_VERSION, MFCreateAttributes,
    MFCreateFile, MFCreateMediaType, MFCreateSourceReaderFromByteStream,
    MFCreateSourceReaderFromURL, MFMediaType_Video, MFNominalRange_0_255, MFSTARTUP_FULL,
    MFStartup, MFVideoArea, MFVideoFormat_NV12, MFVideoFormat_P010, MFVideoPrimaries_BT470_2_SysBG,
    MFVideoPrimaries_BT709, MFVideoPrimaries_BT2020, MFVideoPrimaries_DCI_P3,
    MFVideoPrimaries_SMPTE170M, MFVideoTransFunc_709, MFVideoTransFunc_2084, MFVideoTransFunc_HLG,
    MFVideoTransFunc_sRGB, MFVideoTransferMatrix_BT601, MFVideoTransferMatrix_BT709,
    MFVideoTransferMatrix_BT2020_10, MFVideoTransferMatrix_BT2020_12,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::{GUID, HSTRING, Interface};

use aria_core::{
    ColorPrimaries, ColorRange, ColorSpace, CpuImage, DecodeError, FrameData, GpuSurface,
    GpuSurfaceKind, MatrixCoefficients, PixelFormat, Plane, Rational, RationalTime, Rect, Rotation,
    SourceInfo, TransferFunction, VideoDecoder, VideoFrame,
};

use crate::OutputMode;
use crate::wmf_d3d::D3dShared;
use crate::wmf_gpu::SharedNv12Pool;

#[path = "wmf_planes.rs"]
mod wmf_planes;
#[path = "wmf_source.rs"]
mod wmf_source;

/// Reader plumbing `crate::wmf_audio` and `crate::probe` reach for by these
/// paths; the reader they open is the same one the video path takes.
pub(crate) use wmf_source::{
    audio_track_duration, ensure_mf_platform, open_source_reader, probe_duration,
};
// Glob so this module and `wmf_tests` reach the split-out helpers by the same
// bare names they used before the move.
use wmf_planes::*;
use wmf_source::*;

/// Media Foundation's timestamp tick rate: 100-nanosecond units per second.
pub(crate) const HNS_PER_SEC: i64 = 10_000_000;

/// Upper bound on consecutive empty `ReadSample` returns (stream ticks / gaps)
/// before we give up, so a pathological source can't hang the decode worker.
pub(crate) const MAX_EMPTY_READS: usize = 1024;

/// A pull-based decoder over one video file, backed by Media Foundation.
pub struct WmfDecoder {
    reader: IMFSourceReader,
    /// The magic "first video stream" selector, reused for every reader call.
    stream: u32,
    info: SourceInfo,
    /// Visible region within the coded frame (the clean aperture); the
    /// decoder pads coded sizes to macroblock multiples (e.g. 1080 → 1088).
    visible: Rect,
    mode: OutputMode,
    /// The shared decode device when this reader runs the hardware (DXVA)
    /// path; `None` means software MFTs deliver system-memory samples.
    d3d: Option<&'static D3dShared>,
    /// Shared-texture pool for `Gpu` mode, built on first GPU frame (and
    /// rebuilt if the coded size renegotiates mid-stream).
    pool: Option<Arc<SharedNv12Pool>>,
    /// Set after the first failed zero-copy export: this source delivers CPU
    /// planes from then on instead of re-attempting per frame.
    gpu_unavailable: bool,
    eos: bool,
    /// PTS of the last emitted frame — the [`crate::seek`] roll-forward anchor.
    /// Cleared on [`VideoDecoder::seek`] (decode position moved).
    last_pts: Option<RationalTime>,
    /// Observed seek/decode costs driving the adaptive roll window.
    seek_stats: crate::seek_cost::SeekStats,
}

// SAFETY: the Source Reader and its COM children are owned and touched by a
// single thread at a time — the decode-ahead worker the engine parks this
// decoder on. We open in the multithreaded apartment (see `open`) so the COM
// objects may legally travel between MTA worker threads, and we never share
// `&self` across threads, which satisfies `VideoDecoder: Send` without `Sync`.
unsafe impl Send for WmfDecoder {}

impl WmfDecoder {
    /// Open `path` and probe its first video track.
    ///
    /// Tries the **hardware decode** path first: the reader gets the shared
    /// D3D11 device manager plus `MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS`,
    /// so it loads the GPU vendor's decoder MFT (DXVA) instead of Microsoft's
    /// software one. Any failure along that path — no usable GPU, a codec the
    /// hardware can't handle, a negotiation quirk — falls back to the plain
    /// software reader, which is the previous behavior verbatim.
    pub fn open(path: &Path, mode: OutputMode) -> Result<Self, DecodeError> {
        ensure_mf_platform();
        // The source resolver instantiates COM objects on the calling thread, so
        // it must have COM initialized. Join the MTA so the decoder can live on
        // any worker thread; best-effort — if the host already put this thread in
        // an STA, MF still works and `CoInitializeEx` just reports the conflict.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }

        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

        let d3d = crate::wmf_d3d::shared();
        let hardware = d3d.and_then(|d3d| match configure_reader(path, stream, Some(d3d)) {
            Ok(reader) => Some(reader),
            Err(error) => {
                tracing::debug!(%error, "hardware decode path failed; retrying in software");
                None
            }
        });
        let (reader, d3d) = match hardware {
            Some(reader) => (reader, d3d),
            None => (configure_reader(path, stream, None)?, None),
        };

        let (info, visible) = probe(&reader, stream, Some(path))?;

        // One line per opened source: which decode path this file actually
        // got (DXVA hardware MFT vs Microsoft's software one) and the output
        // mode it will try first — the first thing to check when preview
        // renders run slow.
        tracing::info!(
            path = %path.display(),
            hardware = d3d.is_some(),
            mode = ?mode,
            width = info.coded_size.0,
            height = info.coded_size.1,
            "opened video decoder"
        );

        Ok(Self {
            reader,
            stream,
            info,
            visible,
            mode,
            d3d,
            pool: None,
            gpu_unavailable: false,
            eos: false,
            last_pts: None,
            seek_stats: Default::default(),
        })
    }

    /// Whether this decoder runs on the hardware (DXVA) path. Diagnostic —
    /// the output contract is identical either way.
    pub fn is_hardware_accelerated(&self) -> bool {
        self.d3d.is_some()
    }

    /// Whether the source also carries an audio track, queried from this
    /// decoder's own reader — so the probe doesn't open the file a second
    /// time just to answer it. Stream selection doesn't matter here:
    /// `GetNativeMediaType` reads the presentation descriptor.
    pub(crate) fn has_audio_track(&self) -> bool {
        unsafe {
            self.reader
                .GetNativeMediaType(MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32, 0)
                .is_ok()
        }
    }

    /// Turn one decoded [`IMFSample`] into a [`VideoFrame`].
    ///
    /// `Gpu` mode is **best effort**: when the hardware path is live, the
    /// sample's D3D11 texture slice is exported zero-copy (see
    /// [`crate::wmf_gpu`]); when it isn't — software decode fallback, a
    /// non-DXGI buffer, or a machine without shared-fence support — frames
    /// degrade to CPU planes, which every consumer of [`VideoFrame`] already
    /// handles. Renderer-side import failures still trigger its own
    /// permanent CPU fallback.
    fn sample_to_frame(
        &mut self,
        sample: &IMFSample,
        pts: RationalTime,
    ) -> Result<VideoFrame, DecodeError> {
        if self.mode == OutputMode::Gpu && !self.gpu_unavailable {
            match self.gpu_frame(sample, pts) {
                Ok(frame) => return Ok(frame),
                Err(error) => {
                    // Structural, not per-frame: stop retrying for this source.
                    self.gpu_unavailable = true;
                    tracing::info!(
                        %error,
                        "zero-copy GPU output unavailable; delivering CPU planes"
                    );
                }
            }
        }

        // Flatten any multi-buffer sample into one contiguous buffer.
        let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(decode_err)?;
        let (width, height) = self.info.coded_size;
        let planes = read_planes(&buffer, self.info.pixel_format, width, height)?;
        Ok(VideoFrame::new(
            pts,
            self.info.pixel_format,
            self.info.color,
            (width, height),
            self.visible,
            self.info.rotation,
            FrameData::Cpu(CpuImage::new(planes)),
        ))
    }

    /// The zero-copy arm: copy the sample's decoder-owned texture slice into
    /// a shared pool texture and wrap the NT handles as a [`GpuSurface`].
    fn gpu_frame(
        &mut self,
        sample: &IMFSample,
        pts: RationalTime,
    ) -> Result<VideoFrame, DecodeError> {
        let d3d = self
            .d3d
            .ok_or_else(|| DecodeError::unsupported("decoder is on the software path"))?;

        // Hardware decoders put the frame in an `IMFDXGIBuffer` wrapping one
        // slice of their internal texture array.
        let buffer = unsafe { sample.GetBufferByIndex(0) }.map_err(decode_err)?;
        let dxgi: IMFDXGIBuffer = buffer
            .cast()
            .map_err(|_| DecodeError::unsupported("sample buffer is not DXGI-backed"))?;
        let mut resource: *mut core::ffi::c_void = core::ptr::null_mut();
        // SAFETY: valid DXGI buffer; asks for the ID3D11Texture2D interface.
        unsafe { dxgi.GetResource(&ID3D11Texture2D::IID, &mut resource) }.map_err(decode_err)?;
        // SAFETY: on success `resource` is a +1 ID3D11Texture2D pointer.
        let texture = unsafe { ID3D11Texture2D::from_raw(resource) };
        let subresource = unsafe { dxgi.GetSubresourceIndex() }.map_err(decode_err)?;

        // (Re)build the pool on first use and when the coded size changes
        // mid-stream; in-flight frames keep the old pool alive via their Arcs.
        let coded = self.info.coded_size;
        if self.pool.as_ref().is_none_or(|pool| pool.size() != coded) {
            self.pool = Some(Arc::new(SharedNv12Pool::new(d3d, coded)?));
        }
        let pool = self.pool.as_ref().expect("pool ensured above");

        // Box first, then point: the payload's address must be its final one
        // before it goes out via `handle`.
        let exported = Box::new(pool.export_frame(&texture, subresource)?);
        let handle = &exported.payload as *const aria_core::D3d11SharedSurface as u64;
        let surface = GpuSurface::new(GpuSurfaceKind::D3D11Texture2D, handle, Some(exported));

        Ok(VideoFrame::new(
            pts,
            self.info.pixel_format,
            self.info.color,
            coded,
            self.visible,
            self.info.rotation,
            FrameData::Gpu(surface),
        ))
    }
}

impl VideoDecoder for WmfDecoder {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        // `GUID_NULL` selects the 100-ns time format; the position rides in a
        // `VT_I8` PROPVARIANT. The reader lands at the keyframe at/before the
        // target and flushes, matching the trait contract.
        let position = PROPVARIANT::from(target.in_units(HNS_PER_SEC));
        let time_format = GUID::from_u128(0);
        unsafe { self.reader.SetCurrentPosition(&time_format, &position) }.map_err(seek_err)?;
        self.eos = false;
        self.last_pts = None;
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        if self.eos {
            return Ok(None);
        }

        for _ in 0..MAX_EMPTY_READS {
            let mut stream_flags: u32 = 0;
            let mut timestamp: i64 = 0;
            let mut sample: Option<IMFSample> = None;
            unsafe {
                self.reader.ReadSample(
                    self.stream,
                    0,
                    None,
                    Some(&mut stream_flags),
                    Some(&mut timestamp),
                    Some(&mut sample),
                )
            }
            .map_err(decode_err)?;

            // A fatal stream error (bitstream corruption, a decoder failure
            // mid-file). The reader is done producing frames; surface it as a
            // decode error rather than draining the read budget on it.
            if stream_flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                self.eos = true;
                return Err(DecodeError::Decode(
                    "source reader reported an unrecoverable stream error".into(),
                ));
            }

            // The decoder renegotiated (resolution / format / color changed
            // mid-stream); re-probe so frames after the change are described
            // correctly. The duration is already known, so no path is needed.
            if stream_flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                let duration = self.info.duration;
                let (info, visible) = probe(&self.reader, self.stream, None)?;
                self.info = info;
                self.visible = visible;
                self.info.duration = self.info.duration.or(duration);
            }

            if stream_flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                self.eos = true;
            }

            match sample {
                Some(sample) => {
                    let pts = RationalTime::new(timestamp, self.info.time_base);
                    let frame = self.sample_to_frame(&sample, pts)?;
                    self.last_pts = Some(pts);
                    return Ok(Some(frame));
                }
                // A stream tick or gap can return `S_OK` with no sample; keep
                // draining unless that empty read was also the end of stream.
                None if self.eos => return Ok(None),
                None => continue,
            }
        }

        Err(DecodeError::Decode(
            "source reader produced no frame within the read budget".into(),
        ))
    }

    fn frame_at(&mut self, target: RationalTime) -> Result<Option<VideoFrame>, DecodeError> {
        let last_pts = self.last_pts;
        // Copied out and written back: `frame_at_rolling` borrows `self` for
        // the decode walk, so it can't also borrow the field.
        let mut stats = self.seek_stats;
        let result = crate::seek::frame_at_rolling(self, last_pts, &mut stats, target);
        self.seek_stats = stats;
        result
    }

    fn frame_at_nearest(
        &mut self,
        target: RationalTime,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        let last_pts = self.last_pts;
        let mut stats = self.seek_stats;
        let result = crate::seek::frame_at_nearest_rolling(self, last_pts, &mut stats, target);
        self.seek_stats = stats;
        result
    }
}

pub(crate) fn open_err(error: windows::core::Error) -> DecodeError {
    DecodeError::Open(error.to_string())
}

pub(crate) fn decode_err(error: windows::core::Error) -> DecodeError {
    DecodeError::Decode(error.to_string())
}

pub(crate) fn seek_err(error: windows::core::Error) -> DecodeError {
    DecodeError::Seek(error.to_string())
}

#[cfg(test)]
#[path = "wmf_tests.rs"]
mod tests;
