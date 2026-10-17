//! Dispatch order of registered decoders: claimants of a path's extension are
//! tried by priority, then registration order, before the platform decoder;
//! `Unsupported` passes a path on, and a dropped handle unregisters.
//!
//! The registry is process-wide, so each test claims extensions no other test
//! uses. Not covered: the platform decoders themselves.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_core::{
    ColorSpace, DecodeError, PixelFormat, Rational, RationalTime, Rotation, SourceInfo,
    VideoDecoder, VideoFrame,
};

use super::{ExternalDecoder, dispatch, external_decoder_extensions, register_external_decoder};
use crate::probe::MediaProbe;

/// Answers every call with `width` as its marker, or with `fail`.
struct Marker {
    width: u32,
    fail: Option<fn() -> DecodeError>,
}

struct Opened(SourceInfo);

impl VideoDecoder for Opened {
    fn info(&self) -> &SourceInfo {
        &self.0
    }
    fn seek(&mut self, _target: RationalTime) -> Result<(), DecodeError> {
        Ok(())
    }
    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        Ok(None)
    }
}

impl ExternalDecoder for Marker {
    fn probe(&self, _path: &Path) -> Result<MediaProbe, DecodeError> {
        if let Some(fail) = self.fail {
            return Err(fail());
        }
        Ok(MediaProbe {
            width: self.width,
            height: 1,
            frame_rate: Rational::FPS_30,
            frame_count: 1,
            has_audio: false,
            is_image: false,
            video_codec: None,
            audio_codec: None,
        })
    }

    fn open_video(&self, _path: &Path) -> Result<Box<dyn VideoDecoder>, DecodeError> {
        if let Some(fail) = self.fail {
            return Err(fail());
        }
        Ok(Box::new(Opened(SourceInfo {
            coded_size: (self.width, 1),
            display_size: (self.width, 1),
            rotation: Rotation::None,
            pixel_format: PixelFormat::Yuv420p,
            color: ColorSpace::BT709,
            frame_rate: Rational::FPS_30,
            time_base: Rational::FPS_30,
            duration: None,
        })))
    }
}

fn marker(width: u32) -> Arc<dyn ExternalDecoder> {
    Arc::new(Marker { width, fail: None })
}

fn failing(fail: fn() -> DecodeError) -> Arc<dyn ExternalDecoder> {
    Arc::new(Marker {
        width: 0,
        fail: Some(fail),
    })
}

fn exts(list: &[&str]) -> Vec<String> {
    list.iter().map(|ext| (*ext).to_owned()).collect()
}

fn path(ext: &str) -> PathBuf {
    PathBuf::from(format!("/nonexistent/clip.{ext}"))
}

/// The width each entry point reports for `path`, or its error text.
fn widths(path: &Path) -> [Result<u32, String>; 2] {
    let probed = crate::probe(path)
        .map(|probe| probe.width)
        .map_err(|err| err.to_string());
    let opened = crate::open_video_decoder(path, crate::OutputMode::Gpu)
        .map(|decoder| decoder.info().display_size.0)
        .map_err(|err| err.to_string());
    [probed, opened]
}

#[test]
fn a_claimant_answers_before_the_platform_decoder_until_dropped() {
    let path = path("xclaimone");
    let handle = register_external_decoder("one", &exts(&["XCLAIMONE"]), 0, marker(7));
    assert_eq!(widths(&path), [Ok(7), Ok(7)]);
    // The extension match ignores case on both sides.
    assert_eq!(
        widths(&PathBuf::from("/nonexistent/CLIP.XClaimOne"))[0],
        Ok(7)
    );
    assert!(external_decoder_extensions().contains(&"xclaimone".to_owned()));
    drop(handle);
    for result in widths(&path) {
        assert!(
            result.is_err(),
            "unregistered decoder still answered: {result:?}"
        );
    }
    assert!(!external_decoder_extensions().contains(&"xclaimone".to_owned()));
}

#[test]
fn a_claimant_of_another_extension_is_not_asked() {
    let _handle = register_external_decoder("other", &exts(&["xclaimother"]), 9, marker(3));
    for result in widths(&path("xclaimnone")) {
        assert!(
            result.is_err(),
            "unclaimed path routed to a claimant: {result:?}"
        );
    }
    assert!(widths(&PathBuf::from("/nonexistent/no_extension"))[0].is_err());
}

#[test]
fn higher_priority_first_then_registration_order() {
    let path = path("xclaimorder");
    let ext = exts(&["xclaimorder"]);
    let low = register_external_decoder("low", &ext, -1, marker(1));
    let first = register_external_decoder("first", &ext, 5, marker(2));
    let second = register_external_decoder("second", &ext, 5, marker(3));
    assert_eq!(widths(&path), [Ok(2), Ok(2)]);
    drop(first);
    assert_eq!(widths(&path), [Ok(3), Ok(3)]);
    drop(second);
    assert_eq!(widths(&path), [Ok(1), Ok(1)]);
    drop(low);
}

#[test]
fn unsupported_passes_on_silently_and_other_errors_are_reported() {
    let path = path("xclaimfail");
    let ext = exts(&["xclaimfail"]);
    let _skip = register_external_decoder(
        "skip",
        &ext,
        3,
        failing(|| DecodeError::unsupported("not mine")),
    );
    let _broken = register_external_decoder(
        "broken",
        &ext,
        2,
        failing(|| DecodeError::Decode("bad header".into())),
    );
    // Every claimant fails and the platform decoder cannot open a missing
    // file: the error holds the broken claimant's reason, not the skip's.
    for result in widths(&path) {
        let err = result.expect_err("no decoder can open the file");
        assert!(
            err.contains("decoder broken: decode failed: bad header"),
            "{err}"
        );
        assert!(!err.contains("not mine"), "{err}");
    }
    let _good = register_external_decoder("good", &ext, 1, marker(4));
    assert_eq!(widths(&path), [Ok(4), Ok(4)]);
}

#[test]
fn the_native_error_is_returned_unchanged_without_claimant_failures() {
    let native = || -> Result<u32, DecodeError> { Err(DecodeError::Io("gone".into())) };
    let err = dispatch(&path("xclaimunused"), |_| Ok(1), native).expect_err("no claimant");
    assert_eq!(err.to_string(), "source i/o error: gone");
}
