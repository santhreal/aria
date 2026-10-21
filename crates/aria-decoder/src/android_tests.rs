//! Tests for [`android`].
//!
//! Declared from `android.rs` with `#[path]`, so `use super::*` still means that
//! module and nothing about these cases changed in the move.

use super::*;

#[test]
fn maps_color_standard_to_primaries_and_matrix() {
    assert_eq!(
        map_color_standard(COLOR_STANDARD_BT709),
        (ColorPrimaries::Bt709, MatrixCoefficients::Bt709)
    );
    assert_eq!(
        map_color_standard(COLOR_STANDARD_BT601_PAL),
        (ColorPrimaries::Bt601_625, MatrixCoefficients::Bt601)
    );
    assert_eq!(
        map_color_standard(COLOR_STANDARD_BT601_NTSC),
        (ColorPrimaries::Bt601_525, MatrixCoefficients::Bt601)
    );
    assert_eq!(
        map_color_standard(COLOR_STANDARD_BT2020),
        (ColorPrimaries::Bt2020, MatrixCoefficients::Bt2020Ncl)
    );
    assert_eq!(
        map_color_standard(0),
        (ColorPrimaries::Unspecified, MatrixCoefficients::Unspecified)
    );
}

#[test]
fn maps_color_transfer() {
    assert_eq!(
        map_color_transfer(COLOR_TRANSFER_SDR_VIDEO),
        TransferFunction::Bt709
    );
    assert_eq!(
        map_color_transfer(COLOR_TRANSFER_ST2084),
        TransferFunction::Pq
    );
    assert_eq!(
        map_color_transfer(COLOR_TRANSFER_HLG),
        TransferFunction::Hlg
    );
    assert_eq!(map_color_transfer(0), TransferFunction::Unspecified);
}

#[test]
fn maps_color_range() {
    assert_eq!(map_color_range(COLOR_RANGE_FULL), ColorRange::Full);
    assert_eq!(map_color_range(2), ColorRange::Limited);
    assert_eq!(map_color_range(0), ColorRange::Limited);
}

#[test]
fn maps_color_format_to_pixel_layout() {
    assert_eq!(
        color_format_to_pixel(COLOR_FORMAT_YUV420_SEMIPLANAR),
        Some(PixelFormat::Nv12)
    );
    assert_eq!(
        color_format_to_pixel(COLOR_FORMAT_YUV420_PLANAR),
        Some(PixelFormat::Yuv420p)
    );
    assert_eq!(color_format_to_pixel(0x7F42_0888), None);
}

#[test]
fn us_conversion_matches_time_base() {
    // 3 ticks already on the microsecond base is exactly 3 us.
    let exact = RationalTime::new(3, Rational::new(US_PER_SEC as i32, 1));
    assert_eq!(to_us(exact), 3);
    // Frame 30 at 30 fps is one second = 1e6 us.
    let one_second = RationalTime::new(30, Rational::FPS_30);
    assert_eq!(to_us(one_second), US_PER_SEC);
    // Frame 5 at 30 fps: 5/30 s -> 166_666 us (truncated toward zero).
    let frame_5 = RationalTime::new(5, Rational::FPS_30);
    assert_eq!(to_us(frame_5), 166_666);
}

#[test]
fn read_planes_nv12_copies_luma_and_chroma() {
    let stride = 8;
    let slice_height = 4;
    let chroma_rows = 2;
    let y_len = stride * slice_height; // 32
    let chroma_len = stride * chroma_rows; // 16
    let total_len = y_len + chroma_len; // 48

    let mut buffer = vec![0u8; total_len];
    for (i, byte) in buffer.iter_mut().enumerate() {
        *byte = (i as u8).wrapping_add(1);
    }

    // SAFETY: `buffer` has total_len (48) initialized bytes; stride 8, slice_height 4 matches layout.
    let planes = unsafe {
        read_planes(
            buffer.as_ptr(),
            buffer.len(),
            PixelFormat::Nv12,
            stride,
            slice_height,
        )
    }
    .expect("read_planes nv12 should succeed");

    assert_eq!(planes.len(), 2);
    assert_eq!(planes[0].stride, stride);
    assert_eq!(planes[0].rows, slice_height);
    assert_eq!(planes[0].data.as_slice(), &buffer[..y_len]);
    assert_eq!(planes[1].stride, stride);
    assert_eq!(planes[1].rows, chroma_rows);
    assert_eq!(planes[1].data.as_slice(), &buffer[y_len..total_len]);
}

#[test]
fn read_planes_yuv420p_copies_three_planes() {
    let stride = 8;
    let slice_height = 4;
    let chroma_stride = 4;
    let chroma_rows = 2;
    let y_len = stride * slice_height; // 32
    let chroma_len = chroma_stride * chroma_rows; // 8
    let total_len = y_len + 2 * chroma_len; // 48

    let mut buffer = vec![0u8; total_len];
    for (i, byte) in buffer.iter_mut().enumerate() {
        *byte = (i as u8).wrapping_add(10);
    }

    // SAFETY: `buffer` has total_len (48) initialized bytes; stride 8, slice_height 4 matches layout.
    let planes = unsafe {
        read_planes(
            buffer.as_ptr(),
            buffer.len(),
            PixelFormat::Yuv420p,
            stride,
            slice_height,
        )
    }
    .expect("read_planes yuv420p should succeed");

    assert_eq!(planes.len(), 3);
    assert_eq!(planes[0].stride, stride);
    assert_eq!(planes[0].rows, slice_height);
    assert_eq!(planes[0].data.as_slice(), &buffer[..y_len]);
    assert_eq!(planes[1].stride, chroma_stride);
    assert_eq!(planes[1].rows, chroma_rows);
    assert_eq!(
        planes[1].data.as_slice(),
        &buffer[y_len..y_len + chroma_len]
    );
    assert_eq!(planes[2].stride, chroma_stride);
    assert_eq!(planes[2].rows, chroma_rows);
    assert_eq!(
        planes[2].data.as_slice(),
        &buffer[y_len + chroma_len..total_len]
    );
}

#[test]
fn read_planes_rejects_insufficient_buffer() {
    let buffer = vec![0u8; 30]; // needs 48 bytes for 8x4 NV12
    // SAFETY: `buffer.as_ptr()` points to 30 valid bytes, available is 30.
    let result = unsafe { read_planes(buffer.as_ptr(), buffer.len(), PixelFormat::Nv12, 8, 4) };
    assert!(result.is_err());
}

#[test]
fn read_planes_rejects_arithmetic_overflow() {
    let buffer = vec![0u8; 16];
    // SAFETY: `buffer.as_ptr()` points to 16 valid bytes.
    let result = unsafe {
        read_planes(
            buffer.as_ptr(),
            buffer.len(),
            PixelFormat::Nv12,
            usize::MAX,
            4,
        )
    };
    assert!(result.is_err());
}

#[test]
fn read_planes_rejects_packed_rgb() {
    let buffer = vec![0u8; 64];
    // SAFETY: `buffer.as_ptr()` points to 64 valid bytes.
    let result = unsafe { read_planes(buffer.as_ptr(), buffer.len(), PixelFormat::Rgba8, 8, 4) };
    assert!(result.is_err());
}

#[test]
fn append_pcm_16bit_decoding_and_mixing() {
    // Two stereo 16-bit PCM samples: (L=0, R=32767), (L=-32768, R=16384)
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0i16.to_ne_bytes());
    bytes.extend_from_slice(&32767i16.to_ne_bytes());
    bytes.extend_from_slice(&(-32768i16).to_ne_bytes());
    bytes.extend_from_slice(&16384i16.to_ne_bytes());

    let mut mixed = Vec::new();
    android_audio::append_pcm(
        bytes.as_ptr(),
        bytes.len(),
        android_audio::ENCODING_PCM_16BIT,
        2, // src_channels
        2, // out_channels
        &mut mixed,
    );

    assert_eq!(mixed.len(), 4);
    assert!((mixed[0] - 0.0).abs() < 1e-4);
    assert!((mixed[1] - (32767.0 / 32768.0)).abs() < 1e-4);
    assert!((mixed[2] - (-1.0)).abs() < 1e-4);
    assert!((mixed[3] - (16384.0 / 32768.0)).abs() < 1e-4);

    // Test mono to stereo upmix
    let mut mono_bytes = Vec::new();
    mono_bytes.extend_from_slice(&16384i16.to_ne_bytes());
    let mut mono_mixed = Vec::new();
    android_audio::append_pcm(
        mono_bytes.as_ptr(),
        mono_bytes.len(),
        android_audio::ENCODING_PCM_16BIT,
        1, // src_channels (mono)
        2, // out_channels (stereo)
        &mut mono_mixed,
    );
    assert_eq!(mono_mixed.len(), 2);
    assert!((mono_mixed[0] - 0.5).abs() < 1e-4);
    assert!((mono_mixed[1] - 0.5).abs() < 1e-4);
}

#[test]
fn append_pcm_float_decoding() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0.75f32.to_ne_bytes());
    bytes.extend_from_slice(&(-0.25f32).to_ne_bytes());

    let mut mixed = Vec::new();
    android_audio::append_pcm(
        bytes.as_ptr(),
        bytes.len(),
        android_audio::ENCODING_PCM_FLOAT,
        2,
        2,
        &mut mixed,
    );

    assert_eq!(mixed.len(), 2);
    assert_eq!(mixed[0], 0.75);
    assert_eq!(mixed[1], -0.25);
}
