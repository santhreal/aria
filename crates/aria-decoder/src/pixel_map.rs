use aria_core::PixelFormat;

/// Resolve the two CPU layouts MediaCodec can expose through a byte buffer.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn android_yuv420_layout(planar: bool, semiplanar: bool) -> Option<PixelFormat> {
    match (planar, semiplanar) {
        (true, false) => Some(PixelFormat::Yuv420p),
        (false, true) => Some(PixelFormat::Nv12),
        (false, false) | (true, true) => None,
    }
}

/// Resolve the two biplanar Media Foundation subtypes the decoder requests.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn wmf_biplanar_layout(nv12: bool, p010: bool) -> Option<PixelFormat> {
    match (nv12, p010) {
        (true, false) => Some(PixelFormat::Nv12),
        (false, true) => Some(PixelFormat::P010),
        (false, false) | (true, true) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unknown and contradictory platform tags must fail closed instead of
    /// being interpreted with the byte layout of a different format.
    #[test]
    fn every_platform_pixel_layout_is_explicit() {
        assert_eq!(
            android_yuv420_layout(true, false),
            Some(PixelFormat::Yuv420p)
        );
        assert_eq!(android_yuv420_layout(false, true), Some(PixelFormat::Nv12));
        assert_eq!(android_yuv420_layout(false, false), None);
        assert_eq!(android_yuv420_layout(true, true), None);

        assert_eq!(wmf_biplanar_layout(true, false), Some(PixelFormat::Nv12));
        assert_eq!(wmf_biplanar_layout(false, true), Some(PixelFormat::P010));
        assert_eq!(wmf_biplanar_layout(false, false), None);
        assert_eq!(wmf_biplanar_layout(true, true), None);
    }
}
