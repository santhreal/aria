//! Encoder element selection for the GStreamer backend: the order candidates
//! are tried in, opening each on the decoders' device contexts, demoting a
//! plugin that fails to load, and applying the export's rate settings.

use std::sync::OnceLock;

use aria_core::{EncodeError, EncoderConfig, VideoCodec};
use gstreamer as gst;
use gstreamer::prelude::*;

use super::ensure_init;
use crate::EncoderChoice;

/// The encoder element for `codec` that takes `frames`, built and opened:
/// hardware first, software last.
///
/// A registered factory is not a working encoder. NVENC stays in the registry
/// cache when the driver and its library disagree, and VAAPI registers without
/// a device to open, so each candidate is built and moved to `READY` (where a
/// video encoder opens its device). An open encoder can still refuse the
/// export's frames: `nvh264enc` takes widths from 145 up, so its pipeline
/// fails negotiation on the first frame of a narrower export. The first
/// candidate that opens and takes `frames` is returned still open, so the
/// pipeline it joins does not open the device a second time, with why each
/// earlier one was passed over; when none does, the error lists every
/// candidate's reason. A caller that does not keep the element sets it to
/// `NULL` before dropping it.
pub(super) fn pick_encoder(
    codec: VideoCodec,
    frames: &gst::Caps,
) -> Result<(EncoderChoice, gst::Element), EncodeError> {
    let hint = match codec {
        VideoCodec::H264 => "Install gstreamer1.0-plugins-ugly (x264enc) or a VAAPI/NVENC plugin.",
        VideoCodec::H265 => "Install gstreamer1.0-plugins-bad (x265enc) or a VAAPI/NVENC plugin.",
    };
    let (element, opened, passed_over) =
        first_opened(candidates(codec), |name| open_encoder_element(name, frames)).map_err(
            |reasons| {
                EncodeError::unsupported(format!(
                    "no {} encoder opens here: {}. {hint}",
                    codec.label(),
                    reasons.join("; ")
                ))
            },
        )?;
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

/// Build `name` and open it, returning it in `READY` state when its sink pad
/// takes `frames`.
///
/// Closing `nvh264enc` takes 250 ms on an RTX 5090 and opening it again 75 ms,
/// so the element is kept open for the pipeline rather than closed and opened
/// again there. It opens on the device contexts the decoders keep, so an
/// export runs one CUDA context rather than one for the encoder beside the
/// decoders' one, and a decoder opened after it reuses the encoder's.
fn open_encoder_element(name: &'static str, frames: &gst::Caps) -> Result<gst::Element, String> {
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
    if !takes_frames(&element, frames) {
        let _ = element.set_state(gst::State::Null);
        return Err(format!("{name} does not take {frames}"));
    }
    Ok(element)
}

/// Whether the sink pad of `element`, open, takes `frames` in some pixel
/// format.
///
/// `nvh264enc` takes widths 145 to 4096 and heights 49 to 4096. The format is
/// left out of the comparison: `x265enc` takes no NV12 and is fed through a
/// `videoconvert`.
fn takes_frames(element: &gst::Element, frames: &gst::Caps) -> bool {
    let Some(sink) = element.static_pad("sink") else {
        return false;
    };
    let mut any_format = frames.clone();
    for structure in any_format.make_mut().iter_mut() {
        structure.remove_field("format");
        structure.remove_field("colorimetry");
    }
    sink.query_caps(None).can_intersect(&any_format)
}

/// Whether this host can encode `codec` at all: one of its elements opens and
/// takes frames of some size.
///
/// The one owner of that answer: a picker offering a codec the pipeline would
/// then refuse is worse than a picker with one entry.
///
/// Initializes GStreamer first. A registry lookup on an uninitialised library
/// panics, and the probe may run in a process that has not encoded anything
/// yet.
fn can_encode(codec: VideoCodec) -> bool {
    ensure_init().is_ok()
        && pick_encoder(codec, &gst::Caps::new_any()).is_ok_and(|(_, opened)| {
            let _ = opened.set_state(gst::State::Null);
            true
        })
}

/// The codec probe, run once per process.
static PROBE: aria_decoder::OnceJob = aria_decoder::OnceJob::new();

/// The codecs the probe found this host can encode, in the order it was given.
static PROBED: OnceLock<Vec<VideoCodec>> = OnceLock::new();

/// Start finding which of `claimed` this host can encode, on a thread of its
/// own. Returns at once; only the first call in a process starts the probe.
///
/// Each codec costs an encoder open and close, and `nvh264enc` closes in
/// 250 ms on an RTX 5090. A probe the process ends during returns before its
/// next codec and records no answer.
pub(crate) fn probe_codecs(claimed: &'static [VideoCodec]) {
    PROBE.start("aria-codec-probe", move || {
        let mut found = Vec::with_capacity(claimed.len());
        for &codec in claimed {
            if PROBE.stopping() {
                return;
            }
            if can_encode(codec) {
                found.push(codec);
            }
        }
        let _ = PROBED.set(found);
    });
}

/// What [`probe_codecs`] found, or `None` until it answers.
pub(crate) fn probed_codecs() -> Option<&'static [VideoCodec]> {
    PROBED.get().map(Vec::as_slice)
}

/// Block until a started probe finishes; return at once when none was started.
pub(crate) fn await_codec_probe() {
    PROBE.wait();
}

/// How one candidate element spells the two rate settings an export chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Knobs {
    /// The property holding the most frames between two keyframes.
    keyframes: &'static str,
    /// The rate-control enum and the nick that makes `bitrate` the target,
    /// for an element whose default mode reads `bitrate` as a cap or ignores
    /// it. The software encoders target `bitrate` once it is set.
    rate_mode: Option<(&'static str, &'static str)>,
}

/// The rate properties of `name`, one row per element in [`candidates`].
///
/// The names are read from `gst-inspect-1.0` 1.24: `nvh26xenc` take
/// `gop-size` (`gint`) and `rc-mode`, `nvcudah26xenc` `gop-size` and
/// `rate-control`, `vaapih26xenc` `keyframe-period` (`guint`) and
/// `rate-control`, `x264enc` `key-int-max` (`guint`) and `x265enc`
/// `key-int-max` (`gint`).
fn knobs(name: &str) -> Option<Knobs> {
    let knobs = |keyframes, rate_mode| Knobs {
        keyframes,
        rate_mode,
    };
    Some(match name {
        "nvh264enc" | "nvh265enc" => knobs("gop-size", Some(("rc-mode", "cbr"))),
        "nvcudah264enc" | "nvcudah265enc" => knobs("gop-size", Some(("rate-control", "cbr"))),
        "vaapih264enc" | "vaapih265enc" => knobs("keyframe-period", Some(("rate-control", "cbr"))),
        "x264enc" | "x265enc" => knobs("key-int-max", None),
        _ => return None,
    })
}

/// Apply what the caller chose: the keyframe spacing and the bitrate.
///
/// Each element is set through its row in [`knobs`]. A setting the element
/// has no property for, or a value outside the property's range, fails the
/// export here, before the pipeline creates the file: the encode never runs
/// with a spacing or a rate other than the one asked for.
pub(super) fn configure_encoder(
    encoder: &gst::Element,
    config: &EncoderConfig,
    name: &str,
) -> Result<(), EncodeError> {
    if name == "x264enc" || name == "x265enc" {
        encoder.set_property_from_str("speed-preset", SOFTWARE_SPEED_PRESET);
    }
    if name == "x264enc" {
        encoder.set_property_from_str("tune", "zerolatency");
        // x264's `veryfast` preset quantizes without trellis; the element's
        // `trellis` property defaults to on and replaces the preset's value.
        encoder.set_property("trellis", false);
    }
    if config.keyframe_interval.is_none() && config.bitrate_kbps.is_none() {
        return Ok(());
    }
    let knobs = knobs(name).ok_or_else(|| {
        EncodeError::unsupported(format!("{name} has no known keyframe or bitrate property"))
    })?;
    if let Some(frames) = config.keyframe_interval {
        set_count(
            encoder,
            name,
            knobs.keyframes,
            frames,
            "frames between keyframes",
        )?;
    }
    if let Some(kbps) = config.bitrate_kbps {
        if let Some((mode, nick)) = knobs.rate_mode {
            let takes_nick = encoder.find_property(mode).is_some_and(|spec| {
                gst::glib::EnumClass::with_type(spec.value_type())
                    .is_some_and(|class| class.value_by_nick(nick).is_some())
            });
            if !takes_nick {
                return Err(EncodeError::unsupported(format!(
                    "{name} has no {mode} of {nick} to target a bitrate"
                )));
            }
            encoder.set_property_from_str(mode, nick);
        }
        set_count(encoder, name, "bitrate", kbps, "kbit/s")?;
    }
    Ok(())
}

/// Write `value` into the integer property `property`, in the property's
/// own type, or fail naming the element, the property and its range.
fn set_count(
    encoder: &gst::Element,
    name: &str,
    property: &str,
    value: u32,
    unit: &str,
) -> Result<(), EncodeError> {
    let spec = encoder
        .find_property(property)
        .ok_or_else(|| EncodeError::unsupported(format!("{name} has no {property} property")))?;
    let out_of_range = |min: i64, max: i64| {
        EncodeError::unsupported(format!(
            "{name} takes {property} from {min} to {max} {unit}; the export asks for {value}"
        ))
    };
    if let Some(range) = spec.downcast_ref::<gst::glib::ParamSpecUInt>() {
        if value < range.minimum() || value > range.maximum() {
            return Err(out_of_range(range.minimum().into(), range.maximum().into()));
        }
        encoder.set_property(property, value);
    } else if let Some(range) = spec.downcast_ref::<gst::glib::ParamSpecInt>() {
        let signed = i32::try_from(value)
            .ok()
            .filter(|signed| (range.minimum()..=range.maximum()).contains(signed))
            .ok_or_else(|| out_of_range(range.minimum().into(), range.maximum().into()))?;
        encoder.set_property(property, signed);
    } else {
        return Err(EncodeError::unsupported(format!(
            "{name}'s {property} is a {}, not an integer",
            spec.value_type()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::Rational;

    /// NV12 frames of `width` by `height` at 30 fps, as an export pushes them.
    fn frames(width: u32, height: u32) -> gst::Caps {
        super::super::frame::video_caps(
            width,
            height,
            Rational::new(30, 1),
            crate::yuv::YuvMatrix::Bt709,
        )
        .expect("a positive rate builds caps")
    }

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
            let Ok((choice, element)) = pick_encoder(codec, &frames(1920, 1080)) else {
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

    /// WHY: an encoder that opened was picked whatever the export's size, so on
    /// a host with NVENC a 128 by 72 export went to `nvh264enc`, which takes
    /// widths from 145: the pipeline failed negotiation on the first frame and
    /// the export waited forever on a later push. Closes a candidate picked
    /// for frames its open sink pad does not take, and a size check that also
    /// compares the pixel format, which would pass over `x265enc` (no NV12, fed
    /// through a `videoconvert`) at every size. `x265enc`, which takes sizes
    /// from 16 by 16, stands in for a hardware encoder's size floor on a host
    /// with no NVENC. Checks nothing on a host without `x265enc`.
    #[test]
    fn a_candidate_is_passed_over_for_frames_its_sink_does_not_take() {
        ensure_init().expect("GStreamer initializes");
        if gst::ElementFactory::find("x265enc").is_none() {
            return;
        }
        let small = frames(8, 8);
        assert_eq!(
            open_encoder_element("x265enc", &small).map(|_| ()),
            Err(format!("x265enc does not take {small}"))
        );
        let floor = open_encoder_element("x265enc", &frames(16, 16))
            .expect("x265enc takes 16 by 16 frames, through a videoconvert");
        assert_eq!(floor.current_state(), gst::State::Ready);
        let _ = floor.set_state(gst::State::Null);
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
            let Ok((choice, first)) = pick_encoder(codec, &frames(1920, 1080)) else {
                continue;
            };
            let _ = first.set_state(gst::State::Null);
            let held = || {
                let (_, element) =
                    pick_encoder(codec, &frames(1920, 1080)).expect("the encoder opens again");
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
        configure_encoder(&encoder, &config, "x264enc").expect("configures");
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

    /// An export configuration asking for `frames` between keyframes and
    /// `kbps` of video.
    fn rated(codec: VideoCodec, frames: u32, kbps: u32) -> EncoderConfig {
        EncoderConfig::new(
            (640, 360),
            Rational::new(24, 1),
            aria_core::ColorSpace::BT709,
        )
        .with_codec(codec)
        .with_keyframe_interval(frames)
        .with_bitrate_kbps(kbps)
    }

    /// An integer property of `element` as an `i64`, whatever its width.
    fn count(element: &gst::Element, property: &str) -> i64 {
        let value = element.property_value(property);
        value
            .get::<u32>()
            .map(i64::from)
            .or_else(|_| value.get::<i32>().map(i64::from))
            .unwrap_or_else(|_| panic!("{property} is not an integer: {value:?}"))
    }

    /// WHY: `configure_encoder` set `key-int-max` and skipped every element
    /// without it, so NVENC (`gop-size`) and VAAPI (`keyframe-period`) wrote
    /// their default spacing: an H.265 export asking for a keyframe every 30
    /// frames came out with keyframes 75 frames apart. It also wrote a `u32`
    /// into `x265enc`'s `gint` `key-int-max`, which GLib rejects with a panic.
    ///
    /// Closes the class "a candidate encoder ignores or misreads a rate
    /// setting": every candidate of every codec has a row in `knobs`, read
    /// from `candidates` at run time so a new candidate is red until it gets
    /// one, and every candidate this host builds reads back the spacing, the
    /// bitrate and the rate-control mode it was given. Not caught: the
    /// spacing in an encoded file, which `tests/roundtrip.rs` reads; elements
    /// this host does not have (VAAPI on the test hosts).
    #[test]
    fn every_candidate_takes_the_spacing_and_bitrate_it_is_given() {
        ensure_init().expect("GStreamer initializes");
        let mut built = Vec::new();
        for codec in VideoCodec::ALL {
            for &name in candidates(codec) {
                let knobs = knobs(name).unwrap_or_else(|| panic!("{name} has no rate row"));
                let Ok(encoder) = gst::ElementFactory::make(name).build() else {
                    continue;
                };
                configure_encoder(&encoder, &rated(codec, 30, 8_000), name)
                    .unwrap_or_else(|err| panic!("{name}: {err}"));
                assert_eq!(count(&encoder, knobs.keyframes), 30, "{name} spacing");
                assert_eq!(count(&encoder, "bitrate"), 8_000, "{name} bitrate");
                if let Some((mode, nick)) = knobs.rate_mode {
                    let value = encoder.property_value(mode);
                    let (_, set) =
                        gst::glib::EnumValue::from_value(&value).expect("rate control is an enum");
                    assert_eq!(set.nick(), nick, "{name} {mode}");
                }
                built.push(name);
            }
        }
        assert!(
            built.contains(&"x264enc") || built.contains(&"x265enc"),
            "no software encoder built here; built {built:?}"
        );
    }

    /// A spacing or bitrate outside an element's range fails the export by
    /// name instead of GLib clamping it or warning and leaving the default:
    /// `x265enc` takes at most 102400 kbit/s, `x264enc` no zero bitrate.
    #[test]
    fn a_value_outside_the_elements_range_fails_by_name() {
        ensure_init().expect("GStreamer initializes");
        for (codec, name, kbps, range) in [
            (
                VideoCodec::H265,
                "x265enc",
                200_000,
                "from 1 to 102400 kbit/s",
            ),
            (VideoCodec::H264, "x264enc", 0, "from 1 to 2048000 kbit/s"),
        ] {
            let Ok(encoder) = gst::ElementFactory::make(name).build() else {
                continue;
            };
            let err = configure_encoder(&encoder, &rated(codec, 30, kbps), name)
                .expect_err("out of range");
            let text = err.to_string();
            assert!(
                text.contains(&format!(
                    "{name} takes bitrate {range}; the export asks for {kbps}"
                )),
                "{text}"
            );
        }
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
        let any = gst::Caps::new_any();
        let opened = open_encoder_element(name, &any).expect("a ranked candidate builds and opens");
        let _ = opened.set_state(gst::State::Null);

        demote_factory_plugin(&factory);
        let demoted = plugin_is_demoted(&factory);
        let refused = open_encoder_element(name, &any);
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
