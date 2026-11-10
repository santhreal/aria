//! The elements an export builds from the GStreamer registry besides the
//! encoders, each with the Debian and Ubuntu package that installs it.

use aria_core::EncodeError;
use gstreamer as gst;

/// An element the encode pipeline builds by factory name. The video and AAC
/// encoders are chosen from candidate lists of their own, with their own
/// install hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Element {
    Queue,
    FileSink,
    VideoConvert,
    AudioConvert,
    AudioResample,
    Mp4Mux,
    AacParse,
    H264Parse,
    H265Parse,
}

impl Element {
    #[cfg(test)]
    const ALL: [Self; 9] = [
        Self::Queue,
        Self::FileSink,
        Self::VideoConvert,
        Self::AudioConvert,
        Self::AudioResample,
        Self::Mp4Mux,
        Self::AacParse,
        Self::H264Parse,
        Self::H265Parse,
    ];

    const fn factory(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::FileSink => "filesink",
            Self::VideoConvert => "videoconvert",
            Self::AudioConvert => "audioconvert",
            Self::AudioResample => "audioresample",
            Self::Mp4Mux => "mp4mux",
            Self::AacParse => "aacparse",
            Self::H264Parse => "h264parse",
            Self::H265Parse => "h265parse",
        }
    }

    /// The package that installs the element's plugin.
    const fn package(self) -> &'static str {
        match self {
            Self::Queue | Self::FileSink => "libgstreamer1.0-0",
            Self::VideoConvert | Self::AudioConvert | Self::AudioResample => {
                "gstreamer1.0-plugins-base"
            }
            Self::Mp4Mux | Self::AacParse => "gstreamer1.0-plugins-good",
            Self::H264Parse | Self::H265Parse => "gstreamer1.0-plugins-bad",
        }
    }
}

/// Builds `element`, or an error stating the package that installs it.
pub(super) fn make(element: Element) -> Result<gst::Element, EncodeError> {
    gst::ElementFactory::make(element.factory())
        .build()
        .map_err(|e| {
            EncodeError::unsupported(format!(
                "GStreamer element {} is missing ({e}). Install {}.",
                element.factory(),
                element.package()
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::Element;
    use aria_core::{AudioEncoderConfig, ColorSpace, EncoderConfig, Rational, VideoCodec};
    use gstreamer as gst;
    use gstreamer::prelude::*;

    /// The package GStreamer's own plugin metadata states for `element` on
    /// this host, or `None` when the host lacks the plugin.
    fn host_package(element: Element) -> Option<&'static str> {
        let factory = gst::ElementFactory::find(element.factory())?;
        let source = factory.plugin()?.source();
        Some(match source.as_str() {
            "gstreamer" => "libgstreamer1.0-0",
            "gst-plugins-base" => "gstreamer1.0-plugins-base",
            "gst-plugins-good" => "gstreamer1.0-plugins-good",
            "gst-plugins-bad" => "gstreamer1.0-plugins-bad",
            "gst-plugins-ugly" => "gstreamer1.0-plugins-ugly",
            other => panic!(
                "{} comes from {other}, which no row maps",
                element.factory()
            ),
        })
    }

    /// WHY: closes the class of a missing export element whose error names a
    /// package that does not install it: `h264parse` and `h265parse` come from
    /// gst-plugins-bad, and every missing element read "Install
    /// gstreamer1.0-plugins-base / -good". Each row is held to the plugin
    /// metadata of the host's registry, and every element of an export
    /// pipeline with sound, for each codec, must be a row, an encoder, or the
    /// `appsrc` the binary links.
    ///
    /// What this does not catch: a distribution that packages a plugin under
    /// another name.
    #[test]
    fn every_export_element_states_the_package_that_installs_it() {
        super::super::ensure_init().expect("GStreamer initializes");
        let mut held = 0;
        for element in Element::ALL {
            if let Some(package) = host_package(element) {
                assert_eq!(element.package(), package, "{}", element.factory());
                held += 1;
            }
        }
        assert_eq!(
            held,
            Element::ALL.len(),
            "the test host lacks an export element"
        );

        for codec in VideoCodec::ALL {
            let path = std::env::temp_dir().join(format!("aria-element-census-{codec:?}.mp4"));
            let mut config = EncoderConfig::new((64, 64), Rational::FPS_30, ColorSpace::BT709)
                .with_audio(AudioEncoderConfig::new(48_000, 2));
            config.video_codec = codec;
            let encoder = super::super::GstEncoder::open(&path, config).expect("the export opens");
            let encoders: Vec<&str> = super::super::GstEncoder::video_candidates(codec)
                .iter()
                .copied()
                .chain(super::super::GstEncoder::AAC_ENCODERS.iter().copied())
                .collect();
            let mut built = Vec::new();
            for child in encoder.pipeline.iterate_recurse().into_iter().flatten() {
                // An element class records its factory the first time that
                // factory builds one, and `AppSrc::builder` builds the type
                // directly, so an `appsrc` may report no factory.
                let factory = match child.factory() {
                    Some(factory) => factory.name().to_string(),
                    None if child.is::<gstreamer_app::AppSrc>() => "appsrc".to_owned(),
                    None => panic!(
                        "{codec:?}: the export builds {}, which has no factory",
                        child.name()
                    ),
                };
                let known = Element::ALL.iter().any(|e| e.factory() == factory)
                    || encoders.contains(&factory.as_str())
                    || factory == "appsrc";
                assert!(
                    known,
                    "{codec:?}: the export builds {factory}, which no row states"
                );
                built.push(factory);
            }
            drop(encoder);
            let _ = std::fs::remove_file(&path);
            let parse = match codec {
                VideoCodec::H264 => "h264parse",
                VideoCodec::H265 => "h265parse",
            };
            for factory in [parse, "mp4mux", "aacparse", "audioconvert", "filesink"] {
                assert!(
                    built.iter().any(|b| b.as_str() == factory),
                    "{codec:?}: no {factory} in {built:?}"
                );
            }
        }
    }

    /// Every package an export element needs is in the documented install
    /// command, except GStreamer's own library.
    #[test]
    fn the_install_guide_lists_every_export_package() {
        let doc = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/install.md"),
        )
        .expect("docs/install.md reads");
        let command = doc
            .split("```bash")
            .nth(1)
            .and_then(|rest| rest.split("```").next())
            .expect("docs/install.md opens with the package install command");
        for element in Element::ALL {
            let package = element.package();
            if package != "libgstreamer1.0-0" {
                assert!(
                    command.contains(package),
                    "{} needs {package}, which the install command omits",
                    element.factory()
                );
            }
        }
    }
}
