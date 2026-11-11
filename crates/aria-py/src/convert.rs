//! Time, color, easing, track-kind and output-name conversions for the
//! Python API.

use std::path::Path;

use aria_models::Easing;
use aria_models::{Rational, RationalTime, TimeRange, TrackKind};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

/// The output `path` a call named `verb` writes, when its extension is
/// `extension`, case aside. The call writes one format whatever the name
/// says, so any other name is a `ValueError` stating the name to use, raised
/// before anything is written.
pub fn output_named<'a>(verb: &str, path: &'a str, extension: &str) -> PyResult<&'a Path> {
    let output = Path::new(path);
    if output
        .extension()
        .is_some_and(|named| named.eq_ignore_ascii_case(extension))
    {
        return Ok(output);
    }
    Err(PyValueError::new_err(format!(
        "{verb} writes a .{extension} file; name the output \"{}\" rather than \"{path}\"",
        output.with_extension(extension).display()
    )))
}

/// A `Project(fps=)` value as an exact rate: a whole number from 1 to
/// [`aria_render::MAX_FPS`], or a rate [`aria_core::NAMED_RATES`] names,
/// written as a number (`29.97`) or a string (`"29.97"`). `29.97` is
/// `30000/1001`, the rate `aria new --fps 29.97` and the editor set.
pub fn rate_from_fps(fps: &Bound<'_, PyAny>) -> PyResult<Rational> {
    let raw = if let Ok(whole) = fps.extract::<i64>() {
        whole.to_string()
    } else if let Ok(number) = fps.extract::<f64>() {
        // `Display` writes the shortest decimal that reads back, so the float
        // `29.97` is the text `29.97`.
        number.to_string()
    } else if let Ok(text) = fps.extract::<String>() {
        text
    } else {
        return Err(PyValueError::new_err(format!(
            "fps must be a number, got {}",
            fps.get_type().name()?
        )));
    };
    rate_from_spelling(&raw)
}

/// [`rate_from_fps`] for the decimal text of the value.
pub fn rate_from_spelling(raw: &str) -> PyResult<Rational> {
    Rational::from_fps(raw, aria_render::MAX_FPS).map_err(|_| {
        let named: Vec<&str> = aria_core::NAMED_RATES
            .iter()
            .map(|(key, _)| *key)
            .filter(|key| key.contains('.'))
            .collect();
        PyValueError::new_err(format!(
            "fps must be a whole number from 1 to {} or one of {}, got {raw}",
            aria_render::MAX_FPS,
            named.join(", ")
        ))
    })
}

/// A rate as the Python float of its frames a second: the decimal that names
/// it when [`aria_core::NAMED_RATES`] holds it, so `Project(fps=p.fps)`
/// rebuilds the same rate, else its value.
pub fn fps_float(rate: Rational) -> f64 {
    rate.spelling()
        .and_then(|key| key.parse().ok())
        .unwrap_or_else(|| rate.as_f64())
}

/// The most ticks a time from seconds may hold: `2^53`, the last whole number
/// an `f64` holds exactly. Past it two times a script wrote apart read back as
/// one tick, and the end of a span overflows `i64` arithmetic.
const MAX_TICKS: f64 = 9_007_199_254_740_992.0;

/// What a number of seconds a script passed measures, so a negative one is
/// stated against its own origin: a fade of `-1` is not a time before the
/// timeline starts.
#[derive(Clone, Copy, Debug)]
pub enum Seconds {
    /// A position on the timeline.
    Timeline,
    /// A position into a clip, from its first frame.
    Clip,
    /// A position in a media file.
    Media,
    /// A length, stated by the argument that holds it.
    Length(&'static str),
}

/// Seconds → tick count at `rate`, rounded. A time that is not finite, that
/// rounds below zero, or that holds more than `2^53` ticks is a `ValueError`
/// naming it, raised by the call that takes the time rather than later as an
/// overlap or an arithmetic overflow.
pub fn ticks(seconds: f64, rate: Rational, of: Seconds) -> PyResult<i64> {
    if !seconds.is_finite() {
        return Err(PyValueError::new_err(format!(
            "{seconds} is not a finite number of seconds"
        )));
    }
    if rate.den == 0 {
        return Ok(0);
    }
    let t = (seconds * f64::from(rate.num) / f64::from(rate.den)).round();
    if t < 0.0 {
        return Err(PyValueError::new_err(match of {
            Seconds::Timeline => format!("{seconds} s is before the start of the timeline"),
            Seconds::Clip => format!("{seconds} s is before the start of the clip"),
            Seconds::Media => format!("{seconds} s is before the start of the media"),
            Seconds::Length(what) => format!("{what} must be >= 0, got {seconds}"),
        }));
    }
    if t > MAX_TICKS {
        return Err(PyValueError::new_err(format!(
            "{seconds} s is past the longest time a project holds"
        )));
    }
    Ok(t as i64)
}

/// Tick count → seconds at `rate`.
pub fn seconds(tick: i64, rate: Rational) -> f64 {
    if rate.num == 0 {
        return 0.0;
    }
    tick as f64 * f64::from(rate.den) / f64::from(rate.num)
}

/// A `[start, start+duration)` timeline range from seconds (duration ≥ 1 tick).
pub fn span(start: f64, duration: f64, rate: Rational) -> PyResult<TimeRange> {
    let start = ticks(start, rate, Seconds::Timeline)?;
    let dur = ticks(duration, rate, Seconds::Length("duration"))?.max(1);
    Ok(TimeRange::at_rate(start, dur, rate))
}

/// Timeline instant from seconds.
pub fn time_at(seconds: f64, rate: Rational) -> PyResult<RationalTime> {
    Ok(RationalTime::new(
        ticks(seconds, rate, Seconds::Timeline)?,
        rate,
    ))
}

/// Clip-relative keyframe seconds → absolute timeline time for a clip
/// spanning `clip_start .. clip_start + clip_duration` ticks. The clip's
/// exclusive end (`rel == duration`) maps to its last animatable frame, so
/// `(duration, value)` keyframes the final frame instead of erroring.
pub fn clip_key_time(
    clip_start: i64,
    clip_duration: i64,
    rel_sec: f64,
    rate: Rational,
) -> PyResult<RationalTime> {
    let mut tick = ticks(rel_sec, rate, Seconds::Clip)?;
    if tick == clip_duration && tick > 0 {
        tick -= 1;
    }
    Ok(RationalTime::new(clip_start + tick, rate))
}

/// Positive playback speed as an approximate rational, between 0.05 and 100.0.
pub fn speed_from_f64(speed: f64) -> PyResult<Rational> {
    if !speed.is_finite() || speed < 0.05 || speed > 100.0 {
        return Err(PyValueError::new_err(
            "speed must be a finite number between 0.05 and 100.0",
        ));
    }
    let num = (speed * 1000.0).round() as i32;
    Ok(Rational::new(num, 1000).reduced())
}

/// Playback speed as a float multiplier.
pub fn speed_to_f64(speed: Rational) -> f64 {
    if speed.den == 0 {
        return 1.0;
    }
    f64::from(speed.num) / f64::from(speed.den)
}

/// A stored `f32` as the Python float that writes it: the shortest decimal
/// that reads back as the same `f32`. Widening with `f64::from` returns the
/// binary value, so `clip.opacity = 0.3` read back `0.30000001192092896` and
/// compared unequal to what the script wrote.
pub fn py_float(value: f32) -> f64 {
    use std::io::Write;
    // `Display` writes the shortest round-trip digits with no exponent: at
    // most 48 bytes for the smallest subnormal, 40 for the largest finite.
    let mut buf = [0u8; 64];
    let mut rest = &mut buf[..];
    if write!(rest, "{value}").is_err() {
        return f64::from(value);
    }
    let len = 64 - rest.len();
    std::str::from_utf8(&buf[..len])
        .ok()
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(f64::from(value))
}

/// [`py_float`] for each axis of a pair.
pub fn py_float2((x, y): (f32, f32)) -> (f64, f64) {
    (py_float(x), py_float(y))
}

/// Parse a track-kind string into a [`TrackKind`].
///
/// The wire names are the lane catalog's own `id` column, so a new lane kind is
/// nameable from Python without a row here.
pub fn parse_track_kind(kind: &str) -> PyResult<TrackKind> {
    let lowered = kind.to_ascii_lowercase();
    TrackKind::from_id(&lowered)
        .ok_or_else(|| PyValueError::new_err(format!("unknown track kind {lowered:?}")))
}

/// Track kind → API string.
///
/// The catalog's `id` column, not a second copy of it.
pub fn track_kind_name(kind: TrackKind) -> &'static str {
    kind.id()
}

/// [`parse_color`] with a default of opaque white when no color was given.
pub fn parse_color_opt(obj: Option<&Bound<'_, PyAny>>) -> PyResult<[u8; 4]> {
    match obj {
        Some(obj) => parse_color(obj),
        None => Ok([255, 255, 255, 255]),
    }
}

/// Parse RGB/RGBA tuple, hex string, or a small named set into `[u8; 4]`.
pub fn parse_color(obj: &Bound<'_, PyAny>) -> PyResult<[u8; 4]> {
    if let Ok(s) = obj.extract::<String>() {
        return parse_color_str(&s);
    }
    if let Ok((r, g, b)) = obj.extract::<(u8, u8, u8)>() {
        return Ok([r, g, b, 255]);
    }
    if let Ok((r, g, b, a)) = obj.extract::<(u8, u8, u8, u8)>() {
        return Ok([r, g, b, a]);
    }
    Err(PyValueError::new_err(
        "color must be (r,g,b), (r,g,b,a), a hex string, or a named color",
    ))
}

pub fn parse_color_str(s: &str) -> PyResult<[u8; 4]> {
    let lower = s.to_ascii_lowercase();
    if let Some(rgba) = named_color(&lower) {
        return Ok(rgba);
    }
    let hex = lower.strip_prefix('#').unwrap_or(&lower);
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|_| PyValueError::new_err(format!("invalid color {s:?}")))?;
            let g = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|_| PyValueError::new_err(format!("invalid color {s:?}")))?;
            let b = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|_| PyValueError::new_err(format!("invalid color {s:?}")))?;
            Ok([r, g, b, 255])
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|_| PyValueError::new_err(format!("invalid color {s:?}")))?;
            let g = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|_| PyValueError::new_err(format!("invalid color {s:?}")))?;
            let b = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|_| PyValueError::new_err(format!("invalid color {s:?}")))?;
            let a = u8::from_str_radix(&hex[6..8], 16)
                .map_err(|_| PyValueError::new_err(format!("invalid color {s:?}")))?;
            Ok([r, g, b, a])
        }
        _ => Err(PyValueError::new_err(format!("invalid color {s:?}"))),
    }
}

fn named_color(name: &str) -> Option<[u8; 4]> {
    Some(match name {
        "white" => [255, 255, 255, 255],
        "black" => [0, 0, 0, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 255, 0, 255],
        "blue" => [0, 0, 255, 255],
        "transparent" => [0, 0, 0, 0],
        _ => return None,
    })
}

/// Parse easing from a string or `(x1, y1, x2, y2)` bezier tuple.
pub fn parse_easing(obj: &Bound<'_, PyAny>) -> PyResult<Easing> {
    if let Ok(s) = obj.extract::<String>() {
        return Ok(match s.to_ascii_lowercase().as_str() {
            "linear" => Easing::Linear,
            "ease_in" => Easing::EaseIn,
            "ease_out" => Easing::EaseOut,
            "ease_in_out" => Easing::EaseInOut,
            other => {
                return Err(PyValueError::new_err(format!("unknown easing {other:?}")));
            }
        });
    }
    if let Ok((x1, y1, x2, y2)) = obj.extract::<(f32, f32, f32, f32)>() {
        let easing = Easing::Bezier {
            points: [x1, y1, x2, y2],
        };
        easing
            .validate()
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        return Ok(easing);
    }
    Err(PyValueError::new_err(
        "easing must be 'linear', 'ease_in', 'ease_out', 'ease_in_out', or (x1,y1,x2,y2)",
    ))
}

/// Extract `(time, value[, easing])` keyframe pairs from a Python list.
pub fn parse_keyframe_pairs(
    _py: Python,
    list: &Bound<'_, PyList>,
    default_easing: Easing,
    parse_value: impl Fn(&Bound<'_, PyAny>) -> PyResult<aria_models::ParamValue>,
) -> PyResult<Vec<(f64, aria_models::ParamValue, Easing)>> {
    let mut out = Vec::with_capacity(list.len());
    for item in list.iter() {
        let tuple = item.cast::<PyTuple>()?;
        let t: f64 = tuple.get_item(0)?.extract()?;
        let value = parse_value(&tuple.get_item(1)?)?;
        let easing = if tuple.len() >= 3 {
            parse_easing(&tuple.get_item(2)?)?
        } else {
            default_easing
        };
        out.push((t, value, easing));
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for w in out.windows(2) {
        if (w[0].0 - w[1].0).abs() < f64::EPSILON {
            return Err(PyValueError::new_err(
                "duplicate keyframe times in one call",
            ));
        }
    }
    Ok(out)
}

/// Build a catalog entry dict for IDE-friendly introspection.
pub fn param_spec_dict(
    py: Python,
    name: &str,
    label: &str,
    default: f32,
    min: f32,
    max: f32,
) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("name", name)?;
    dict.set_item("label", label)?;
    dict.set_item("default", py_float(default))?;
    dict.set_item("min", py_float(min))?;
    dict.set_item("max", py_float(max))?;
    Ok(dict.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const R30: Rational = Rational::new(30, 1);

    #[test]
    fn ticks_round_trip_at_30fps() {
        assert_eq!(ticks(1.0, R30, Seconds::Timeline).ok(), Some(30));
        assert!((seconds(30, R30) - 1.0).abs() < 1e-9);
    }

    /// WHY: `ticks` clamped and saturated: `start=nan` read as tick 0 and
    /// failed as an overlap with the clip there, `-5` read as tick 0, and
    /// `inf` saturated to `i64::MAX` and failed as "time arithmetic overflow".
    /// A time that rounds to a tick from 0 to `2^53` reads as that tick, every
    /// other time is an error at each boundary, and `span`, `time_at` and
    /// `clip_key_time` pass the error on. Not caught: a call site that reads
    /// seconds without these helpers.
    #[test]
    fn a_time_is_a_tick_from_zero_to_the_ceiling_or_an_error() {
        assert_eq!(ticks(0.4999 / 30.0, R30, Seconds::Timeline).ok(), Some(0));
        assert_eq!(ticks(0.5001 / 30.0, R30, Seconds::Timeline).ok(), Some(1));
        assert_eq!(ticks(-0.4999 / 30.0, R30, Seconds::Timeline).ok(), Some(0));
        assert!(ticks(-0.5001 / 30.0, R30, Seconds::Timeline).is_err());
        assert!(ticks(-5.0, R30, Seconds::Timeline).is_err());
        let one = Rational::new(1, 1);
        assert_eq!(ticks(MAX_TICKS, one, Seconds::Timeline).ok(), Some(1 << 53));
        assert!(ticks(MAX_TICKS + 2.0, one, Seconds::Timeline).is_err());
        assert!(ticks(f64::MAX, one, Seconds::Timeline).is_err());
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(ticks(bad, R30, Seconds::Timeline).is_err(), "{bad}");
            assert!(span(bad, 1.0, R30).is_err(), "span start {bad}");
            assert!(span(0.0, bad, R30).is_err(), "span duration {bad}");
            assert!(time_at(bad, R30).is_err(), "time_at {bad}");
            assert!(clip_key_time(0, 60, bad, R30).is_err(), "key {bad}");
        }
        assert!(time_at(-1.0, R30).is_err());
        assert!(clip_key_time(0, 60, -1.0, R30).is_err());
    }

    #[test]
    fn seconds_guards_zero_rate() {
        assert_eq!(seconds(30, Rational::new(0, 1)), 0.0);
    }

    /// WHY: `Project(fps=29.97)` raised a TypeError, `fps=0` built a 1 fps
    /// project, and `p.fps` read a 29.97 project as `29.970029970029973`, so
    /// `Project(fps=p.fps)` could not rebuild its rate. Every named rate reads
    /// exact from its decimal and from the float of it, and reads back as that
    /// decimal; a whole number up to the ceiling reads as itself; zero, a
    /// negative, an unnamed fraction, one past the ceiling, and a non-number
    /// are errors. Not caught: an argument type other than a number or a
    /// string (the Python suite covers it).
    #[test]
    fn a_frame_rate_reads_exact_and_back_as_its_decimal() {
        for (key, rate) in aria_core::NAMED_RATES {
            assert_eq!(rate_from_spelling(key).ok(), Some(rate), "{key}");
            let float: f64 = key.parse().expect("a decimal");
            assert_eq!(rate_from_spelling(&float.to_string()).ok(), Some(rate));
            assert_eq!(fps_float(rate), float, "{key}");
        }
        for fps in [1, 12, aria_render::MAX_FPS] {
            let rate = Rational::new(fps, 1);
            assert_eq!(rate_from_spelling(&fps.to_string()).ok(), Some(rate));
            assert_eq!(fps_float(rate), f64::from(fps));
        }
        let over = (aria_render::MAX_FPS + 1).to_string();
        for bad in [
            "0",
            "-24",
            "29.98",
            "12.5",
            over.as_str(),
            "NaN",
            "inf",
            "fast",
            "",
        ] {
            assert!(rate_from_spelling(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn span_has_at_least_one_tick() {
        let range = span(1.0, 0.0, R30).expect("a span");
        assert_eq!(range.start.value, 30);
        assert_eq!(range.duration.value, 1);
    }

    #[test]
    fn clip_key_time_offsets_from_clip_start() {
        assert_eq!(
            clip_key_time(60, 60, 1.0, R30).ok().map(|t| t.value),
            Some(90)
        );
    }

    #[test]
    fn clip_key_time_clamps_exclusive_end_to_last_frame() {
        let at = |rel: f64| clip_key_time(0, 60, rel, R30).ok().map(|t| t.value);
        // A keyframe at the clip's full duration lands on the final frame.
        assert_eq!(at(2.0), Some(59));
        // Past-the-end stays out of range so the model can reject it.
        assert_eq!(at(2.5), Some(75));
    }

    #[test]
    fn speed_round_trip() {
        let r = speed_from_f64(2.0).unwrap();
        assert!((speed_to_f64(r) - 2.0).abs() < 0.01);
        let r = speed_from_f64(0.25).unwrap();
        assert!((speed_to_f64(r) - 0.25).abs() < 0.01);
    }

    #[test]
    fn speed_rejects_non_positive_and_non_finite() {
        assert!(speed_from_f64(0.0).is_err());
        assert!(speed_from_f64(-1.0).is_err());
        assert!(speed_from_f64(0.01).is_err());
        assert!(speed_from_f64(101.0).is_err());
        assert!(speed_from_f64(f64::NAN).is_err());
        assert!(speed_from_f64(f64::INFINITY).is_err());
        assert!(speed_from_f64(f64::NEG_INFINITY).is_err());
        assert!(speed_from_f64(0.05).is_ok());
        assert!(speed_from_f64(100.0).is_ok());
    }

    #[test]
    fn speed_to_f64_guards_zero_denominator() {
        assert_eq!(speed_to_f64(Rational::new(1, 0)), 1.0);
    }

    #[test]
    fn track_kind_round_trips() {
        for name in [
            "video",
            "audio",
            "text",
            "sticker",
            "effect",
            "filter",
            "adjustment",
        ] {
            let kind = parse_track_kind(name).unwrap();
            assert_eq!(track_kind_name(kind), name);
        }
        assert_eq!(
            parse_track_kind("VIDEO").unwrap(),
            TrackKind::Video,
            "kind parsing is case-insensitive"
        );
        assert!(parse_track_kind("subtitle").is_err());
    }

    #[test]
    fn color_str_hex_and_named() {
        assert_eq!(parse_color_str("#ffffff").unwrap(), [255, 255, 255, 255]);
        assert_eq!(parse_color_str("ff8000").unwrap(), [255, 128, 0, 255]);
        assert_eq!(parse_color_str("#ff8000c8").unwrap(), [255, 128, 0, 200]);
        assert_eq!(parse_color_str("WHITE").unwrap(), [255, 255, 255, 255]);
        assert_eq!(parse_color_str("transparent").unwrap(), [0, 0, 0, 0]);
        assert!(parse_color_str("#12345").is_err());
        assert!(parse_color_str("#gggggg").is_err());
        assert!(parse_color_str("chartreuse").is_err());
    }

    /// WHY: every float getter widened the stored `f32`, so a script that
    /// wrote `clip.opacity = 0.3` read back `0.30000001192092896`. A decimal
    /// a script writes reads back equal, and every `f32`, swept across its
    /// bit patterns with each boundary, narrows back to itself, so the read
    /// never states a different stored value. Not caught: a getter that
    /// returns an `f32` without this conversion (the Python suite covers the
    /// getters).
    #[test]
    fn a_stored_float_reads_back_as_the_decimal_that_wrote_it() {
        for written in [0.3, 1.1, 12.7, 0.1, -2.5e-7, 123_456.79, 0.0, -0.4, 1e30] {
            assert_eq!(py_float(written as f32), written, "{written}");
        }
        let edges = [
            f32::MIN_POSITIVE,
            f32::from_bits(1),
            f32::MAX,
            f32::MIN,
            -0.0,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        let sweep = (0..u32::MAX / 65_521).map(|n| f32::from_bits(n * 65_521));
        for value in edges.into_iter().chain(sweep) {
            let read = py_float(value);
            if value.is_nan() {
                assert!(read.is_nan());
            } else {
                assert_eq!((read as f32).to_bits(), value.to_bits(), "{value:e}");
            }
        }
    }
}
