//! Encoder element selection for the GStreamer backend: the order candidates
//! are tried in, opening each on the decoders' device contexts, demoting a
//! plugin that fails to load, and applying the export's rate settings.

use aria_core::{EncodeError, EncoderConfig, VideoCodec};
use gstreamer as gst;
use gstreamer::prelude::*;

use super::ensure_init;
use crate::EncoderChoice;

/// The encoder element for `codec`, built and opened: hardware first, software
/// last.
///
/// A registered factory is not a working encoder. NVENC stays in the registry
/// cache when the driver and its library disagree, and VAAPI registers without
/// a device to open, so each candidate is built and moved to `READY` (where a
/// video encoder opens its device). The first one that opens is returned still
/// open, so the pipeline it joins does not open the device a second time, with
/// why each earlier one did not; when none does, the error lists every
/// candidate's reason. A caller that does not keep the element sets it to
/// `NULL` before dropping it.
pub(super) fn pick_encoder(
    codec: VideoCodec,
) -> Result<(EncoderChoice, gst::Element), EncodeError> {
    let hint = match codec {
        VideoCodec::H264 => "Install gstreamer1.0-plugins-ugly (x264enc) or a VAAPI/NVENC plugin.",
        VideoCodec::H265 => "Install gstreamer1.0-plugins-bad (x265enc) or a VAAPI/NVENC plugin.",
    };
    let (element, opened, passed_over) = first_opened(candidates(codec), open_encoder_element)
        .map_err(|reasons| {
            EncodeError::unsupported(format!(
                "no {} encoder opens here: {}. {hint}",
                codec.label(),
                reasons.join("; ")
            ))
        })?;
    let choice = EncoderChoice {
        encoder: element.into(),
        hardware: Some(!SOFTWARE_ENCODERS.contains(&element)),
        passed_over,
    };
    Ok((choice, opened))
}

/// The encoder elements for `codec`, in the order they are tried: NVENC, then
/// VAAPI, then software.
///
/// NVENC precedes VAAPI because a host with both is an NVIDIA card beside an
/// integrated GPU, and VAAPI drives the integrated encoder. Measured with
/// `gst-launch-1.0` on a Ryzen 9 9950X (integrated Radeon) with an RTX 5090,
/// 600 frames of 1080p60 NV12 less the 0.65 s the source takes alone, over two
/// runs: `nvh264enc` 2.00 to 2.06 s, `nvcudah264enc` 2.05 to 2.35 s,
/// `vaapih264enc` 4.58 to 5.15 s, `x264enc` 4.29 to 4.70 s.
fn candidates(codec: VideoCodec) -> &'static [&'static str] {
    match codec {
        VideoCodec::H264 => &["nvh264enc", "nvcudah264enc", "vaapih264enc", "x264enc"],
        VideoCodec::H265 => &["nvh265enc", "nvcudah265enc", "vaapih265enc", "x265enc"],
    }
}

/// The software encoders in the candidate lists. Every other candidate runs on
/// a GPU or media engine.
const SOFTWARE_ENCODERS: &[&str] = &["x264enc", "x265enc"];

/// The `speed-preset` both software encoders run.
const SOFTWARE_SPEED_PRESET: &str = "veryfast";

/// The first candidate `open` succeeds on, in order, with the reasons of the
/// ones before it, or every reason when none succeeds.
///
/// Candidates after the first success are not tried, so a working hardware
/// encoder never pays for probing the software one.
fn first_opened<T>(
    candidates: &[&'static str],
    mut open: impl FnMut(&'static str) -> Result<T, String>,
) -> Result<(&'static str, T, Vec<String>), Vec<String>> {
    let mut reasons = Vec::with_capacity(candidates.len());
    for &name in candidates {
        match open(name) {
            Ok(opened) => return Ok((name, opened, reasons)),
            Err(reason) => reasons.push(reason),
        }
    }
    Err(reasons)
}

/// Whether every feature registered under `factory`'s plugin has rank `NONE`:
/// the state `aria_decoder`'s warm-up and [`demote_factory_plugin`] leave an
/// unloadable plugin in, so later opens do not map its `.so` again.
fn plugin_is_demoted(factory: &gst::ElementFactory) -> bool {
    let Some(plugin) = factory.plugin_name() else {
        return false;
    };
    let features = gst::Registry::get().features_by_plugin(&plugin);
    !features.is_empty() && features.iter().all(|f| f.rank() == gst::Rank::NONE)
}

/// Set every feature of `factory`'s plugin to `Rank::NONE` in this process's
/// registry after a failed load, so later candidates in the same plugin are
/// skipped without mapping the `.so` again.
fn demote_factory_plugin(factory: &gst::ElementFactory) {
    if let Some(plugin) = factory.plugin_name() {
        for feature in gst::Registry::get().features_by_plugin(&plugin) {
            feature.set_rank(gst::Rank::NONE);
        }
    }
}

/// Build `name` and open it, returning it in `READY` state.
///
/// Closing `nvh264enc` takes 250 ms on an RTX 5090 and opening it again 75 ms,
/// so the element is kept open for the pipeline rather than closed and opened
/// again there. It opens on the device contexts the decoders keep, so an
/// export runs one CUDA context rather than one for the encoder beside the
/// decoders' one, and a decoder opened after it reuses the encoder's.
fn open_encoder_element(name: &'static str) -> Result<gst::Element, String> {
    let is_hw = !SOFTWARE_ENCODERS.contains(&name);
    if is_hw {
        aria_decoder::await_warm_up();
    }
    let Some(factory) = gst::ElementFactory::find(name) else {
        return Err(format!("{name} is not installed"));
    };
    if is_hw && plugin_is_demoted(&factory) {
        return Err(format!("{name} failed to load (plugin demoted)"));
    }
    let element = match gst::ElementFactory::make(name).build() {
        Ok(element) => element,
        Err(e) => {
            if is_hw {
                demote_factory_plugin(&factory);
            }
            return Err(format!("{name} failed to load ({e})"));
        }
    };
    if !aria_decoder::open_on_kept_contexts(&element) {
        let _ = element.set_state(gst::State::Null);
        return Err(format!("{name} could not open its device"));
    }
    Ok(element)
}

/// Whether this host can encode `codec` at all: one of its elements opens.
///
/// The one owner of that answer: a picker offering a codec the pipeline would
/// then refuse is worse than a picker with one entry.
///
/// Initialises GStreamer first. A registry lookup on an uninitialised library
/// panics, and this is called from the export sheet, which may run in a process
/// that has not encoded anything yet.
pub(crate) fn can_encode(codec: VideoCodec) -> bool {
    ensure_init().is_ok()
        && pick_encoder(codec).is_ok_and(|(_, opened)| {
            let _ = opened.set_state(gst::State::Null);
            true
        })
}

/// Apply what the caller chose: the keyframe spacing and the bitrate.
///
/// Both are set through `find_property` rather than by element name, because
/// every encoder here spells the rate control differently and only the software
/// ones take the tuning knobs. A property an element does not have is left
/// alone rather than refused: the encode is still the codec that was asked for.
pub(super) fn configure_encoder(encoder: &gst::Element, config: &EncoderConfig, name: &str) {
    if name == "x264enc" || name == "x265enc" {
        encoder.set_property_from_str("speed-preset", SOFTWARE_SPEED_PRESET);
    }
    if name == "x264enc" {
        encoder.set_property_from_str("tune", "zerolatency");
        // x264's `veryfast` preset quantizes without trellis; the element's
        // `trellis` property defaults to on and replaces the preset's value.
        encoder.set_property("trellis", false);
    }
    if let Some(interval) = config.keyframe_interval {
        // x264enc / x265enc take a frame count; the hardware elements do not
        // carry this property at all.
        if encoder.find_property("key-int-max").is_some() {
            encoder.set_property("key-int-max", interval);
        }
    }
    if let Some(kbps) = config.bitrate_kbps {
        set_bitrate(encoder, name, kbps);
    }
}

/// Write `kbps` into whichever rate property this element carries.
///
/// Every encoder here spells rate control differently: `x264enc` takes an
/// `i32` of kilobits, `x265enc` and the VAAPI elements a `u32`, and the NVENC
/// elements need their rate-control mode moved off constant-quality or the
/// bitrate is a cap they never approach. The property's own type is read rather
/// than guessed from the element name, so a plugin that changes it does not
/// turn into a panic.
fn set_bitrate(encoder: &gst::Element, name: &str, kbps: u32) {
    if name.starts_with("nv") && encoder.find_property("rc-mode").is_some() {
        encoder.set_property_from_str("rc-mode", "cbr");
    }
    let Some(spec) = encoder.find_property("bitrate") else {
        return;
    };
    if spec.value_type() == gst::glib::Type::U32 {
        encoder.set_property("bitrate", kbps);
    } else if spec.value_type() == gst::glib::Type::I32 {
        encoder.set_property("bitrate", kbps as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::Rational;

    /// The walk returns the first candidate that opens and stops there; a
    /// candidate that fails (a registered NVENC whose driver does not answer)
    /// is passed over rather than returned for the pipeline to fail on, and its
    /// reason is kept with the pick.
    #[test]
    fn the_first_candidate_that_opens_is_picked_and_later_ones_are_not_tried() {
        let mut tried = Vec::new();
        let picked = first_opened(&["hw-a", "hw-b", "sw", "never"], |name| {
            tried.push(name);
            if name == "sw" {
                Ok(7)
            } else {
                Err(format!("{name} could not open its device"))
            }
        });
        assert_eq!(
            picked,
            Ok((
                "sw",
                7,
                vec![
                    "hw-a could not open its device".to_string(),
                    "hw-b could not open its device".to_string(),
                ]
            ))
        );
        assert_eq!(tried, ["hw-a", "hw-b", "sw"]);
    }

    #[test]
    fn when_nothing_opens_every_reason_is_kept_in_order() {
        let picked = first_opened::<()>(&["hw", "sw"], |name| Err(format!("{name} failed")));
        assert_eq!(
            picked,
            Err(vec!["hw failed".to_string(), "sw failed".to_string()])
        );
    }

    /// The pick a host makes from each codec's real candidate list, with
    /// `opens` standing in for which of its encoders open. A hybrid host (an
    /// NVIDIA card beside an integrated GPU) opens both NVENC and VAAPI and
    /// must export through NVENC, the faster of the two; a host whose NVENC
    /// does not open takes VAAPI over software; software is the last resort.
    #[test]
    fn nvenc_is_preferred_then_vaapi_then_software() {
        fn pick(codec: VideoCodec, opens: impl Fn(&str) -> bool) -> &'static str {
            first_opened(candidates(codec), |name| {
                if opens(name) {
                    Ok(())
                } else {
                    Err(format!("{name} shut"))
                }
            })
            .expect("one candidate opens")
            .0
        }
        for codec in VideoCodec::ALL {
            let picked = pick(codec, |_| true);
            assert!(
                picked.starts_with("nv"),
                "{codec:?}: hybrid host picked {picked}"
            );
            let picked = pick(codec, |name| !name.starts_with("nv"));
            assert!(
                picked.starts_with("vaapi"),
                "{codec:?}: no NVENC picked {picked}"
            );
            let picked = pick(codec, |name| SOFTWARE_ENCODERS.contains(&name));
            assert_eq!(Some(&picked), candidates(codec).last(), "{codec:?}");
        }
    }

    /// Whatever the picker hands to the pipeline is open on this machine, for
    /// every codec. Before the picker opened its candidates it returned the
    /// first registered name, which on a host with a stale NVENC registration
    /// could not even be built; before it kept the winner open, the pipeline
    /// paid for closing the device and opening it again.
    #[test]
    fn the_picked_encoder_opens_on_this_machine() {
        ensure_init().expect("GStreamer initializes");
        for codec in [VideoCodec::H264, VideoCodec::H265] {
            let Ok((choice, element)) = pick_encoder(codec) else {
                continue;
            };
            let name = &choice.encoder;
            assert_eq!(
                element.current_state(),
                gst::State::Ready,
                "{name} was picked for {codec:?} but is not open"
            );
            let _ = element.set_state(gst::State::Null);
        }
    }

    /// WHY: the picked encoder opened alone, with no bus to ask for a device
    /// context on, so every export made a CUDA context of its own beside the
    /// decoders' one: about 68 ms of each NVENC open on an RTX 5090, against
    /// 5 ms on a kept context. Holds that once one encoder has opened, every
    /// later pick opens on one device context, the one the process kept. The
    /// second and third picks are compared: another test in this binary can
    /// open an encoder beside the first pick and have its context kept
    /// instead of the first pick's. Checks nothing on a host whose picked
    /// encoder holds no device context, such as a software encoder.
    #[test]
    fn encoders_picked_in_a_row_open_on_one_device_context() {
        ensure_init().expect("GStreamer initializes");
        for codec in [VideoCodec::H264, VideoCodec::H265] {
            let Ok((choice, first)) = pick_encoder(codec) else {
                continue;
            };
            let _ = first.set_state(gst::State::Null);
            let held = || {
                let (_, element) = pick_encoder(codec).expect("the encoder opens again");
                let contexts = element.contexts();
                let _ = element.set_state(gst::State::Null);
                contexts
            };
            let second = held();
            let third = held();
            for context in &second {
                assert!(
                    third
                        .iter()
                        .any(|again| again.structure() == context.structure()),
                    "{} opened on a new {} context",
                    choice.encoder,
                    context.context_type()
                );
            }
        }
    }

    /// WHY: `configure_encoder` set `speed-preset=veryfast` on `x264enc`, and
    /// the element's `trellis` property, on by default, replaced the preset's
    /// trellis-off value: `x264_8_trellis_coefn` took 12.2% of the samples of
    /// a 1080p60 software export on Server. Closes leaving trellis on, and
    /// pins the preset, the tune, the keyframe interval and the bitrate the
    /// export requests, so a speed change cannot drop the preset to a lower
    /// quality one unnoticed. Does not measure encode throughput.
    #[test]
    fn x264enc_runs_the_veryfast_preset_without_trellis() {
        ensure_init().expect("GStreamer initializes");
        let Ok(encoder) = gst::ElementFactory::make("x264enc").build() else {
            return;
        };
        let config = EncoderConfig {
            video_codec: VideoCodec::H264,
            size: (1920, 1080),
            frame_rate: Rational::FPS_60,
            bitrate_kbps: Some(8_000),
            keyframe_interval: Some(60),
            color: aria_core::ColorSpace::BT709,
            audio: None,
        };
        configure_encoder(&encoder, &config, "x264enc");
        assert!(!encoder.property::<bool>("trellis"), "trellis is on");
        let preset = encoder.property_value("speed-preset");
        let (_, preset) =
            gst::glib::EnumValue::from_value(&preset).expect("speed-preset is an enum");
        assert_eq!(preset.nick(), "veryfast");
        let tune = encoder.property_value("tune");
        let flags = gst::glib::FlagsClass::with_type(tune.type_()).expect("tune is a flags type");
        assert!(
            flags.is_set_by_nick(&tune, "zerolatency"),
            "tune lacks zerolatency"
        );
        assert_eq!(encoder.property::<u32>("key-int-max"), 60);
        assert_eq!(encoder.property::<u32>("bitrate"), 8_000);
    }

    /// WHY: `open_encoder_element` built every hardware candidate on each
    /// pick, so a plugin that had failed to load (`nvcodec` on a host without
    /// an NVIDIA driver) mapped its `.so` again inside every export, about
    /// 150 ms on Server. Closes building a candidate whose plugin the decoder
    /// warm-up or an earlier failed build demoted, and a demotion that leaves
    /// a feature of the plugin ranked. `wavparse` (rank primary) stands in for
    /// a hardware candidate: every name outside `SOFTWARE_ENCODERS` takes the
    /// hardware path, and no test in this crate autoplugs its plugin. Does not
    /// cover the decoder warm-up's own demotion.
    #[test]
    fn a_candidate_of_a_demoted_plugin_is_refused_without_building_it() {
        ensure_init().expect("GStreamer initializes");
        let name = "wavparse";
        let Some(factory) = gst::ElementFactory::find(name) else {
            return;
        };
        let plugin = factory.plugin_name().expect("wavparse has a plugin");
        let ranks: Vec<(gst::PluginFeature, gst::Rank)> = gst::Registry::get()
            .features_by_plugin(&plugin)
            .into_iter()
            .map(|feature| {
                let rank = feature.rank();
                (feature, rank)
            })
            .collect();
        assert!(!plugin_is_demoted(&factory), "{plugin} starts demoted");
        let opened = open_encoder_element(name).expect("a ranked candidate builds and opens");
        let _ = opened.set_state(gst::State::Null);

        demote_factory_plugin(&factory);
        let demoted = plugin_is_demoted(&factory);
        let refused = open_encoder_element(name);
        for (feature, rank) in ranks {
            feature.set_rank(rank);
        }
        assert!(demoted, "a feature of {plugin} kept its rank");
        assert_eq!(
            refused.map(|_| ()),
            Err(format!("{name} failed to load (plugin demoted)"))
        );
    }

    /// WHY: the export reference lists the GStreamer elements in the order
    /// `pick_encoder` tries them and the preset the software ones run. Closes
    /// a candidate added, removed or reordered, a software element changed,
    /// or the preset changed without the reference following. Does not cover
    /// the prose about `tune` and trellis, which
    /// `x264enc_runs_the_veryfast_preset_without_trellis` pins in code.
    #[test]
    fn the_export_reference_lists_the_linux_encoder_elements() {
        let doc = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/reference/export-and-subtitles.md"
        ))
        .expect("read docs/reference/export-and-subtitles.md");
        let heading = "### Linux encoder elements (`aria_encoder::gst`)";
        let start = doc
            .find(heading)
            .expect("missing the Linux encoder section")
            + heading.len();
        let section = &doc[start..];
        let section = &section[..section.find("\n#").unwrap_or(section.len())];
        let rows: Vec<Vec<String>> = section
            .lines()
            .filter(|line| line.starts_with('|'))
            .skip(2)
            .map(|line| {
                line.trim_matches('|')
                    .split('|')
                    .map(|cell| cell.trim().to_string())
                    .collect()
            })
            .collect();
        let expected: Vec<Vec<String>> = VideoCodec::ALL
            .into_iter()
            .map(|codec| {
                let elements = candidates(codec);
                let software: Vec<&str> = elements
                    .iter()
                    .copied()
                    .filter(|name| SOFTWARE_ENCODERS.contains(name))
                    .collect();
                vec![
                    format!("`{}`", codec.label()),
                    elements
                        .iter()
                        .map(|name| format!("`{name}`"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    software
                        .iter()
                        .map(|name| format!("`{name}`"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    format!("`{SOFTWARE_SPEED_PRESET}`"),
                ]
            })
            .collect();
        assert_eq!(
            rows, expected,
            "docs/reference/export-and-subtitles.md Linux encoder table drifted from candidates / SOFTWARE_ENCODERS / SOFTWARE_SPEED_PRESET"
        );
    }
}
