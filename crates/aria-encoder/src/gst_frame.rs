//! The NV12 frames the GStreamer backend pushes: the `appsrc` caps, the tight
//! buffer layout and the `GstVideoMeta` that states it, the buffer pool, and
//! the packing of a decoded or rendered frame into a buffer.

use aria_core::{EncodeError, PixelFormat, VideoFrame};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_video as gst_video;

use crate::yuv::{ChannelOrder, YuvMatrix, rgba_to_nv12_into, yuv420_len};

/// The size and plane layout of one pushed NV12 buffer.
///
/// Every frame is written tight: `Y` rows of `width` bytes, then the `UV`
/// rows of `width` bytes. GStreamer's default NV12 layout rounds each stride
/// up to 4 bytes, so at a width that is 2 more than a multiple of 4, such as
/// 854 or 1366, a buffer without a `GstVideoMeta` reads sheared and is
/// shorter than the frame `x264enc` maps, which fails the export. The buffer
/// holds GStreamer's default frame size, which `GstVideoMeta` requires, and a
/// frame whose tight layout differs from the default carries a
/// `GstVideoMeta` stating the tight strides and offsets.
#[derive(Debug, Clone, Copy)]
pub(super) struct FrameLayout {
    width: u32,
    height: u32,
    pub(super) buffer_bytes: usize,
    /// The tight `UV` offset and stride, when they differ from the defaults.
    tight: Option<(usize, i32)>,
}

impl FrameLayout {
    pub(super) fn new(width: u32, height: u32) -> Result<Self, EncodeError> {
        let info = gst_video::VideoInfo::builder(gst_video::VideoFormat::Nv12, width, height)
            .build()
            .map_err(|e| {
                EncodeError::unsupported(format!(
                    "GStreamer has no NV12 layout for a {width}x{height} frame ({e})."
                ))
            })?;
        let stride = width as usize;
        let uv_offset = stride * height as usize;
        let defaults = info.stride()[..2] == [stride as i32; 2] && info.offset()[1] == uv_offset;
        Ok(Self {
            width,
            height,
            buffer_bytes: info.size().max(yuv420_len(width, height)),
            tight: (!defaults).then_some((uv_offset, stride as i32)),
        })
    }

    /// Attach the tight layout to `buffer` when it differs from the default.
    pub(super) fn describe(&self, buffer: &mut gst::BufferRef) -> Result<(), EncodeError> {
        let Some((uv_offset, stride)) = self.tight else {
            return Ok(());
        };
        gst_video::VideoMeta::add_full(
            buffer,
            gst_video::VideoFrameFlags::empty(),
            gst_video::VideoFormat::Nv12,
            self.width,
            self.height,
            &[0, uv_offset],
            &[stride, stride],
        )
        .map(|_| ())
        .map_err(|e| EncodeError::Encode(format!("couldn't describe the NV12 frame layout: {e}")))
    }
}

pub(super) fn open_buffer_pool(caps: &gst::Caps, frame_bytes: usize) -> Option<gst::BufferPool> {
    let size = u32::try_from(frame_bytes).ok()?;
    let pool = gst::BufferPool::new();
    let mut cfg = pool.config();
    cfg.set_params(Some(caps), size, 0, 0);
    pool.set_config(cfg).ok()?;
    pool.set_active(true).ok()?;
    Some(pool)
}

pub(super) fn video_caps(
    width: u32,
    height: u32,
    rate: aria_core::Rational,
) -> Result<gst::Caps, EncodeError> {
    if rate.num <= 0 || rate.den <= 0 {
        return Err(EncodeError::unsupported(
            "encoder frame rate must be greater than zero.",
        ));
    }
    Ok(gst::Caps::builder("video/x-raw")
        .field("format", gst_video::VideoFormat::Nv12.to_str())
        .field("width", width as i32)
        .field("height", height as i32)
        .field("framerate", gst::Fraction::new(rate.num, rate.den))
        .build())
}

pub(super) fn write_frame_to_nv12(
    frame: &VideoFrame,
    width: u32,
    height: u32,
    matrix: YuvMatrix,
    out: &mut [u8],
) -> Result<(), EncodeError> {
    let cpu = frame.cpu().ok_or_else(|| {
        EncodeError::unsupported("Linux encoder accepts CPU frames only. Export from a CPU render.")
    })?;
    match frame.format {
        PixelFormat::Nv12 => write_packed_nv12(&cpu.planes, width, height, out),
        PixelFormat::Rgba8 | PixelFormat::Bgra8 => {
            let plane = cpu
                .planes
                .first()
                .ok_or_else(|| EncodeError::Encode("RGB frame has no plane.".into()))?;
            let order = if frame.format == PixelFormat::Bgra8 {
                ChannelOrder::Bgra
            } else {
                ChannelOrder::Rgba
            };
            rgba_to_nv12_into(&plane.data, width, height, plane.stride, matrix, order, out)
        }
        other => Err(EncodeError::unsupported(format!(
            "Linux encoder cannot take {other:?} frames. Export RGBA or NV12."
        ))),
    }
}

fn write_packed_nv12(
    planes: &[aria_core::Plane],
    width: u32,
    height: u32,
    out: &mut [u8],
) -> Result<(), EncodeError> {
    if planes.len() < 2 {
        return Err(EncodeError::Encode("NV12 frame is missing a plane.".into()));
    }
    let y_stride = width as usize;
    let uv_stride = width as usize;
    let y_rows = height as usize;
    let uv_rows = height.div_ceil(2) as usize;
    let y_len = y_stride * y_rows;
    let need = y_len + uv_stride * uv_rows;
    if out.len() < need {
        return Err(EncodeError::Encode(
            "NV12 encode buffer is too small.".into(),
        ));
    }
    copy_plane(&planes[0], &mut out[..y_len], y_stride, y_rows);
    copy_plane(&planes[1], &mut out[y_len..need], uv_stride, uv_rows);
    Ok(())
}

fn copy_plane(plane: &aria_core::Plane, dest: &mut [u8], dest_stride: usize, rows: usize) {
    let copy_w = dest_stride.min(plane.stride);
    if plane.stride == dest_stride && copy_w == dest_stride && plane.rows >= rows {
        let len = rows * dest_stride;
        dest[..len].copy_from_slice(&plane.data[..len]);
        return;
    }
    for row in 0..rows.min(plane.rows) {
        let src = &plane.data[row * plane.stride..row * plane.stride + copy_w];
        dest[row * dest_stride..row * dest_stride + copy_w].copy_from_slice(src);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::{Rational, RationalTime};

    use crate::gst::ensure_init;

    /// WHY: export passes decoded NV12 planes to `push` as they are, and
    /// `push` packs them into the `width`-stride buffer the `appsrc` caps
    /// describe. Closes copying a decoder's row padding into the picture,
    /// copying rows past the frame height from a plane coded taller, and the
    /// contiguous fast path taking a padded plane. Does not cover the encoded
    /// bitstream.
    #[test]
    fn nv12_planes_pack_to_the_frame_width_without_row_padding() {
        use aria_core::{ColorSpace, CpuImage, FrameData, Plane, Rect, Rotation};

        let (width, height) = (6_u32, 4_u32);
        let (w, y_rows, uv_rows) = (width as usize, height as usize, height as usize / 2);
        let plane = |stride: usize, rows: usize, base: u8| {
            let data = (0..stride * rows)
                .map(|i| base.wrapping_add(i as u8))
                .collect();
            Plane::new(data, stride, rows)
        };
        let picture = |plane: &Plane, rows: usize| -> Vec<u8> {
            plane
                .data
                .chunks(plane.stride)
                .take(rows)
                .flat_map(|row| &row[..w])
                .copied()
                .collect()
        };
        let cases = [
            ("tight", (w, y_rows), (w, uv_rows)),
            ("padded rows", (w + 2, y_rows), (w + 2, uv_rows)),
            ("coded taller", (w, y_rows + 4), (w, uv_rows + 2)),
            (
                "padded and taller",
                (w + 10, y_rows + 4),
                (w + 10, uv_rows + 2),
            ),
        ];
        for (name, (y_stride, y_coded), (uv_stride, uv_coded)) in cases {
            let y = plane(y_stride, y_coded, 16);
            let uv = plane(uv_stride, uv_coded, 128);
            let mut expected = picture(&y, y_rows);
            expected.extend(picture(&uv, uv_rows));
            let frame = VideoFrame::new(
                RationalTime::new(0, Rational::FPS_30),
                PixelFormat::Nv12,
                ColorSpace::BT709,
                (width, height),
                Rect::from_size(width, height),
                Rotation::None,
                FrameData::Cpu(CpuImage::new(vec![y, uv])),
            );
            let mut out = vec![0xEE; yuv420_len(width, height)];
            write_frame_to_nv12(&frame, width, height, YuvMatrix::Bt709, &mut out)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(out, expected, "{name} planes packed wrong");
        }
    }

    /// WHY: `push` writes each frame tight, `width` bytes a row, while
    /// GStreamer's default NV12 layout rounds each stride up to 4 bytes. At a
    /// width of 2 more than a multiple of 4 (854, 1366) the encoder read the
    /// frame sheared, and `x264enc` failed it as shorter than its layout.
    /// This maps a buffer `push` would send the way an encoder maps it, with
    /// the `appsrc` caps, at every even size up to 64x8, so each residue of
    /// the width and height against GStreamer's alignment is covered, and
    /// checks every plane row reads the tight bytes written. Does not cover
    /// an encoder element that ignores `GstVideoMeta`;
    /// `tests/roundtrip.rs` encodes such a width end to end.
    #[test]
    fn a_pushed_frame_reads_back_tight_at_every_even_size() {
        use gst_video::VideoFrameExt;
        ensure_init().expect("GStreamer");
        for width in (2..=64_u32).step_by(2) {
            for height in (2..=8_u32).step_by(2) {
                let layout = FrameLayout::new(width, height).expect("layout");
                let (w, h) = (width as usize, height as usize);
                let tight: Vec<u8> = (0..w * h * 3 / 2).map(|i| (i % 251) as u8).collect();
                let mut buffer = gst::Buffer::with_size(layout.buffer_bytes).expect("buffer");
                {
                    let buffer = buffer.get_mut().expect("writable");
                    buffer.map_writable().expect("map").as_mut_slice()[..tight.len()]
                        .copy_from_slice(&tight);
                    layout.describe(buffer).expect("describe");
                }
                let caps = video_caps(width, height, Rational::FPS_30).expect("caps");
                let info = gst_video::VideoInfo::from_caps(&caps).expect("info");
                let frame = gst_video::VideoFrame::from_buffer_readable(buffer, &info)
                    .unwrap_or_else(|_| panic!("{width}x{height}: the encoder cannot map it"));
                let (y, uv) = tight.split_at(w * h);
                for (plane, (expected, rows)) in [(y, h), (uv, h / 2)].into_iter().enumerate() {
                    let stride = frame.plane_stride()[plane] as usize;
                    let data = frame.plane_data(plane as u32).expect("plane");
                    for row in 0..rows {
                        assert_eq!(
                            &data[row * stride..row * stride + w],
                            &expected[row * w..(row + 1) * w],
                            "{width}x{height}: plane {plane} row {row} reads other bytes"
                        );
                    }
                }
            }
        }
    }

    /// WHY: `push` takes its buffers from a `gst::BufferPool` and attaches a
    /// `GstVideoMeta` to each. The pool drops that meta when the buffer
    /// returns; if it did not, every reuse would stack one more meta on the
    /// same buffer for the length of the export.
    #[test]
    fn a_reused_pool_buffer_holds_one_layout_meta() {
        ensure_init().expect("GStreamer");
        let (width, height) = (854, 480);
        let layout = FrameLayout::new(width, height).expect("layout");
        assert!(layout.tight.is_some(), "an 854-wide frame needs its layout");
        let caps = video_caps(width, height, Rational::FPS_30).expect("caps");
        let pool = open_buffer_pool(&caps, layout.buffer_bytes).expect("pool");
        let mut first = None;
        for round in 0..4 {
            let mut buffer = pool.acquire_buffer(None).expect("acquire");
            let ptr = buffer.as_ptr();
            assert_eq!(
                *first.get_or_insert(ptr),
                ptr,
                "round {round}: another buffer"
            );
            let buffer = buffer.get_mut().expect("writable");
            layout.describe(buffer).expect("describe");
            let metas = buffer.iter_meta::<gst_video::VideoMeta>().count();
            assert_eq!(metas, 1, "round {round}: {metas} layout metas");
        }
        pool.set_active(false).expect("deactivate");
    }
}
