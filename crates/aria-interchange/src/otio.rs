//! OpenTimelineIO JSON (`.otio`).
//!
//! A timeline's tracks hold clips, gaps and transitions in sequence. A clip's
//! source range is in its media's time, its duration is what it occupies on
//! the track, and a `LinearTimeWarp` scales how fast the media plays under
//! it. A transition sits on the cut between two items, `in_offset` before it
//! and `out_offset` after.

use std::io::{self, Write};

use aria_models::{MarkerColor, Rational, RationalTime};
use serde_json::{Map, Value, json};

use crate::model::{
    Cut, Event, Lane, LaneKind, Mark, Read, Source, file_stem, rational_of, speed_of,
};
use crate::url::{path_of_url, url_of};

/// The largest rate or time value read: past it, a file is not describing a
/// timeline.
const LIMIT: f64 = 1.0e12;
/// How deep nested stacks are walked to measure them.
const MAX_DEPTH: usize = 32;

/// A time as OpenTimelineIO writes it: `value` units at `rate` a second.
#[derive(Debug, Clone, Copy)]
struct Time {
    value: f64,
    rate: f64,
}

impl Time {
    fn seconds(self) -> f64 {
        self.value / self.rate
    }
}

/// What reading a track or a stack fills in.
struct Reader {
    rate: Rational,
    skipped: Vec<String>,
    marks: Vec<Mark>,
}

/// Read `text`. `rate` is the cut's rate when the file states none, and
/// `name` titles it when the timeline has no name.
pub fn read(text: &str, rate: Rational, name: &str) -> Result<Read, String> {
    let root: Value =
        serde_json::from_str(text).map_err(|err| format!("the file is not JSON: {err}"))?;
    if schema(&root).0 != "Timeline" {
        return Err("the file holds no OpenTimelineIO timeline".to_owned());
    }
    let stack = root
        .get("tracks")
        .filter(|tracks| schema(tracks).0 == "Stack")
        .ok_or("the timeline holds no track stack")?;
    let rate = match root.get("global_start_time").filter(|t| !t.is_null()) {
        Some(start) => rational_of(time(start)?.rate)?,
        None => first_rate(stack).map_or(Ok(rate), rational_of)?,
    };
    let mut reader = Reader {
        rate,
        skipped: Vec::new(),
        marks: Vec::new(),
    };
    reader.markers(stack, Some)?;
    let mut lanes = Vec::new();
    for (index, child) in children(stack).iter().enumerate() {
        match schema(child).0 {
            "Track" => {
                if let Some(lane) = reader.track(child, index)? {
                    lanes.push(lane);
                }
            }
            other => reader.skipped.push(format!(
                "track {}: a nested {other}, which is not read",
                index + 1
            )),
        }
    }
    // Video lanes first, in stack order, then audio lanes in stack order.
    lanes.sort_by_key(|lane: &Lane| lane.kind == LaneKind::Audio);
    let mut cut = Cut {
        name: text_of(&root, "name")
            .filter(|name| !name.is_empty())
            .unwrap_or(name)
            .to_owned(),
        rate,
        size: None,
        lanes,
        marks: reader.marks,
    };
    cut.merge_picture_sound();
    Ok(Read {
        cut,
        skipped: reader.skipped,
    })
}

impl Reader {
    fn frames(&self, seconds: f64) -> i64 {
        (seconds * self.rate.as_f64()).round() as i64
    }

    fn track(&mut self, track: &Value, index: usize) -> Result<Option<Lane>, String> {
        let kind = match text_of(track, "kind") {
            Some("Video") => LaneKind::Video,
            Some("Audio") => LaneKind::Audio,
            other => {
                self.skipped.push(format!(
                    "track {}: kind {:?} is not read",
                    index + 1,
                    other.unwrap_or("")
                ));
                return Ok(None);
            }
        };
        let name = text_of(track, "name")
            .filter(|name| !name.is_empty())
            .map_or_else(|| format!("Track {}", index + 1), str::to_owned);
        self.markers(track, Some)?;
        let mut events: Vec<Event> = Vec::new();
        let mut cursor = 0.0_f64;
        let mut dissolve: Option<i64> = None;
        for item in children(track) {
            let (kind_name, _) = schema(item);
            match kind_name {
                "Gap" => cursor += duration(item, 0)?,
                "Transition" => {
                    let before = time(item.get("in_offset").unwrap_or(&Value::Null))?.seconds();
                    let after = time(item.get("out_offset").unwrap_or(&Value::Null))?.seconds();
                    // An odd length splits a frame unevenly around the cut.
                    if (before - after).abs() * self.rate.as_f64() > 1.5 {
                        self.skipped
                            .push(format!("{name}: a transition off its cut, centered on it"));
                    }
                    dissolve = Some(self.frames(before + after)).filter(|frames| *frames > 0);
                }
                "Clip" => {
                    let length = duration(item, 0)?;
                    if let Some(event) = self.clip(item, &name, kind, cursor, length)? {
                        events.push(Event {
                            dissolve: dissolve.take(),
                            ..event
                        });
                    }
                    dissolve = None;
                    cursor += length;
                }
                other => {
                    self.skipped
                        .push(format!("{name}: a nested {other}, which is not read"));
                    cursor += duration(item, 0)?;
                    dissolve = None;
                }
            }
        }
        Ok(Some(Lane { kind, name, events }))
    }

    /// The event `clip` places at `at` seconds for `length` seconds, or
    /// `None` when it is skipped.
    fn clip(
        &mut self,
        clip: &Value,
        lane: &str,
        kind: LaneKind,
        at: f64,
        length: f64,
    ) -> Result<Option<Event>, String> {
        let name = text_of(clip, "name").unwrap_or("").to_owned();
        let label = || {
            if name.is_empty() {
                format!("{lane}: a clip")
            } else {
                format!("{lane} {name}")
            }
        };
        if clip.get("enabled").and_then(Value::as_bool) == Some(false) {
            self.skipped
                .push(format!("{}: disabled, not read", label()));
            return Ok(None);
        }
        let reference = media_reference(clip);
        let (reference_kind, _) = reference.map_or(("MissingReference", ""), schema);
        let path = match reference_kind {
            "ExternalReference" => reference
                .and_then(|reference| text_of(reference, "target_url"))
                .and_then(path_of_url),
            "MissingReference" => None,
            other => {
                self.skipped
                    .push(format!("{}: a {other}, which is not read", label()));
                return Ok(None);
            }
        };
        let range = clip
            .get("source_range")
            .filter(|range| !range.is_null())
            .or_else(|| reference.and_then(|reference| reference.get("available_range")))
            .filter(|range| !range.is_null())
            .ok_or_else(|| format!("{}: no source range", label()))?;
        let mut start = time(range.get("start_time").unwrap_or(&Value::Null))?;
        // Markers are in the media's own time, before the available-range offset.
        let marker_start = start.seconds();
        if let Some(available) = reference
            .and_then(|reference| reference.get("available_range"))
            .filter(|range| !range.is_null())
            .and_then(|range| range.get("start_time"))
        {
            let available = time(available)?;
            start.value -= available.value * start.rate / available.rate;
        }
        let source_rate = rational_of(start.rate)?;
        let (mut speed, mut reversed) = (Rational::new(1, 1), false);
        for effect in clip
            .get("effects")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match schema(effect).0 {
                "LinearTimeWarp" => {
                    let scalar = effect
                        .get("time_scalar")
                        .and_then(Value::as_f64)
                        .filter(|s| s.is_finite() && s.abs() < 1000.0)
                        .ok_or_else(|| format!("{}: a time warp with no scalar", label()))?;
                    if scalar == 0.0 {
                        self.skipped
                            .push(format!("{}: a freeze frame, not read", label()));
                        return Ok(None);
                    }
                    speed = speed_of(scalar.abs());
                    reversed = scalar < 0.0;
                }
                "FreezeFrame" => {
                    self.skipped
                        .push(format!("{}: a freeze frame, not read", label()));
                    return Ok(None);
                }
                other => {
                    let effect_name = text_of(effect, "effect_name").unwrap_or(other);
                    self.skipped
                        .push(format!("{}: effect {effect_name}, not read", label()));
                }
            }
        }
        let first = self.frames(at);
        let end = self.frames(at + length);
        if end <= first {
            return Ok(None);
        }
        let source_start = start.value.round() as i64;
        let factor = f64::from(speed.num) / f64::from(speed.den);
        self.markers(clip, |source_seconds| {
            let offset = (source_seconds - marker_start) / factor;
            (0.0..=length).contains(&offset).then_some(at + offset)
        })?;
        let reel = reference
            .and_then(|reference| text_of(reference, "name"))
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .or_else(|| path.as_ref().map(|path| file_stem(path, &name)))
            .unwrap_or_else(|| name.clone());
        Ok(Some(Event {
            name,
            start: first,
            duration: end - first,
            source: Source {
                reel,
                path,
                start: RationalTime::new(source_start, source_rate),
            },
            speed,
            reversed,
            dissolve: None,
            sound: kind == LaneKind::Audio,
        }))
    }

    /// Read the markers on `item`. `place` maps a marker's time in seconds,
    /// in the item's own time, to the record seconds it lands on, or `None`
    /// to drop it.
    fn markers(&mut self, item: &Value, place: impl Fn(f64) -> Option<f64>) -> Result<(), String> {
        for marker in item
            .get("markers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let range = marker.get("marked_range").unwrap_or(&Value::Null);
            let at = time(range.get("start_time").unwrap_or(&Value::Null))?.seconds();
            let Some(record) = place(at) else {
                continue;
            };
            self.marks.push(Mark {
                at: self.frames(record),
                name: text_of(marker, "name").unwrap_or("").to_owned(),
                comment: text_of(marker, "comment").unwrap_or("").to_owned(),
                color: text_of(marker, "color").and_then(color_of),
            });
        }
        Ok(())
    }
}

/// The schema name and version of `value`, such as `("Clip", "2")`.
fn schema(value: &Value) -> (&str, &str) {
    let full = text_of(value, "OTIO_SCHEMA").unwrap_or("");
    full.split_once('.').unwrap_or((full, ""))
}

fn text_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn children(value: &Value) -> &[Value] {
    value
        .get("children")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn time(value: &Value) -> Result<Time, String> {
    let number = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && n.abs() < LIMIT)
    };
    match (number("value"), number("rate")) {
        (Some(value), Some(rate)) if rate > 0.0 => Ok(Time { value, rate }),
        _ => Err("a time with no finite value or positive rate".to_owned()),
    }
}

/// The seconds `item` occupies on its track: its source range, or for a
/// nested stack or track, what its children occupy.
fn duration(item: &Value, depth: usize) -> Result<f64, String> {
    if let Some(range) = item.get("source_range").filter(|range| !range.is_null()) {
        let length = time(range.get("duration").unwrap_or(&Value::Null))?.seconds();
        return if length >= 0.0 {
            Ok(length)
        } else {
            Err("a negative duration".to_owned())
        };
    }
    if depth >= MAX_DEPTH {
        return Err(format!("stacks nest deeper than {MAX_DEPTH}"));
    }
    match schema(item).0 {
        "Track" => children(item).iter().try_fold(0.0, |total, child| {
            Ok(if schema(child).0 == "Transition" {
                total
            } else {
                total + duration(child, depth + 1)?
            })
        }),
        "Stack" => children(item).iter().try_fold(0.0_f64, |longest, child| {
            Ok(longest.max(duration(child, depth + 1)?))
        }),
        "Clip" => {
            let range = media_reference(item)
                .and_then(|reference| reference.get("available_range"))
                .filter(|range| !range.is_null())
                .ok_or("a clip with no range")?;
            Ok(time(range.get("duration").unwrap_or(&Value::Null))?.seconds())
        }
        _ => Ok(0.0),
    }
}

/// The rate of the first clip's or gap's duration under `stack`.
fn first_rate(stack: &Value) -> Option<f64> {
    children(stack).iter().find_map(|track| {
        children(track).iter().find_map(|item| {
            let range = item.get("source_range")?;
            time(range.get("duration")?).ok().map(|time| time.rate)
        })
    })
}

/// The media reference a clip plays: `Clip.2`'s active one, or `Clip.1`'s.
fn media_reference(clip: &Value) -> Option<&Value> {
    if let Some(references) = clip.get("media_references").and_then(Value::as_object) {
        let key = text_of(clip, "active_media_reference_key").unwrap_or("DEFAULT_MEDIA");
        return references.get(key);
    }
    clip.get("media_reference")
        .filter(|reference| !reference.is_null())
}

fn color_of(name: &str) -> Option<MarkerColor> {
    Some(match name {
        "PINK" => MarkerColor::Pink,
        "RED" => MarkerColor::Red,
        "ORANGE" => MarkerColor::Orange,
        "YELLOW" => MarkerColor::Yellow,
        "GREEN" => MarkerColor::Green,
        "CYAN" => MarkerColor::Teal,
        "BLUE" => MarkerColor::Blue,
        "PURPLE" | "MAGENTA" => MarkerColor::Purple,
        _ => return None,
    })
}

fn color_name(color: Option<MarkerColor>) -> &'static str {
    match color {
        Some(MarkerColor::Pink) => "PINK",
        Some(MarkerColor::Red) | None => "RED",
        Some(MarkerColor::Orange) => "ORANGE",
        Some(MarkerColor::Yellow) => "YELLOW",
        Some(MarkerColor::Green) => "GREEN",
        Some(MarkerColor::Teal) => "CYAN",
        Some(MarkerColor::Blue) => "BLUE",
        Some(MarkerColor::Purple) => "PURPLE",
    }
}

fn rational_time(value: f64, rate: Rational) -> Value {
    json!({ "OTIO_SCHEMA": "RationalTime.1", "rate": rate.as_f64(), "value": value })
}

fn time_range(start: Value, duration: Value) -> Value {
    json!({ "OTIO_SCHEMA": "TimeRange.1", "start_time": start, "duration": duration })
}

/// Write `cut`. Everything a cut holds has an OpenTimelineIO form, so nothing
/// is reported.
pub fn write(cut: &Cut, out: &mut dyn Write) -> io::Result<Vec<String>> {
    let rate = cut.rate;
    let video = cut.lanes.iter().filter(|lane| lane.kind == LaneKind::Video);
    let sound = cut.split_picture_sound();
    let audio = sound
        .iter()
        .chain(cut.lanes.iter().filter(|lane| lane.kind == LaneKind::Audio));
    let tracks: Vec<Value> = video.chain(audio).map(|lane| track(lane, rate)).collect();
    let markers: Vec<Value> = cut
        .marks
        .iter()
        .map(|mark| {
            json!({
                "OTIO_SCHEMA": "Marker.2",
                "metadata": {},
                "name": mark.name,
                "color": color_name(mark.color),
                "comment": mark.comment,
                "marked_range": time_range(
                    rational_time(mark.at as f64, rate),
                    rational_time(0.0, rate),
                ),
            })
        })
        .collect();
    let timeline = json!({
        "OTIO_SCHEMA": "Timeline.1",
        "metadata": {},
        "name": cut.name,
        "global_start_time": rational_time(0.0, rate),
        "tracks": {
            "OTIO_SCHEMA": "Stack.1",
            "metadata": {},
            "name": "tracks",
            "source_range": null,
            "effects": [],
            "markers": markers,
            "enabled": true,
            "children": tracks,
        },
    });
    serde_json::to_writer_pretty(&mut *out, &timeline).map_err(io::Error::other)?;
    writeln!(out)?;
    Ok(Vec::new())
}

fn track(lane: &Lane, rate: Rational) -> Value {
    let mut children = Vec::new();
    let mut cursor = 0_i64;
    let mut previous_end: Option<i64> = None;
    for event in &lane.events {
        if event.start > cursor {
            children.push(json!({
                "OTIO_SCHEMA": "Gap.1",
                "metadata": {},
                "name": "",
                "source_range": time_range(
                    rational_time(0.0, rate),
                    rational_time((event.start - cursor) as f64, rate),
                ),
                "effects": [],
                "markers": [],
                "enabled": true,
            }));
        }
        if let Some(frames) = event.dissolve.filter(|_| previous_end == Some(event.start)) {
            let before = frames / 2;
            children.push(json!({
                "OTIO_SCHEMA": "Transition.1",
                "metadata": {},
                "name": "",
                "transition_type": "SMPTE_Dissolve",
                "in_offset": rational_time(before as f64, rate),
                "out_offset": rational_time((frames - before) as f64, rate),
            }));
        }
        children.push(clip(event, rate));
        cursor = cursor.max(event.end());
        previous_end = Some(event.end());
    }
    json!({
        "OTIO_SCHEMA": "Track.1",
        "metadata": {},
        "name": lane.name,
        "source_range": null,
        "effects": [],
        "markers": [],
        "enabled": true,
        "kind": match lane.kind {
            LaneKind::Video => "Video",
            LaneKind::Audio => "Audio",
        },
        "children": children,
    })
}

fn clip(event: &Event, rate: Rational) -> Value {
    let source_rate = event.source.start.rate;
    // The duration at the source's rate, so both ends of the range share one.
    let length = event.duration as f64 * source_rate.as_f64() / rate.as_f64();
    let mut effects = Vec::new();
    if event.speed != Rational::new(1, 1) || event.reversed {
        let scalar = f64::from(event.speed.num) / f64::from(event.speed.den);
        effects.push(json!({
            "OTIO_SCHEMA": "LinearTimeWarp.1",
            "metadata": {},
            "name": "",
            "effect_name": "LinearTimeWarp",
            "time_scalar": if event.reversed { -scalar } else { scalar },
        }));
    }
    let mut reference = Map::new();
    reference.insert("OTIO_SCHEMA".into(), json!("ExternalReference.1"));
    reference.insert("metadata".into(), json!({}));
    reference.insert("name".into(), json!(event.source.reel));
    reference.insert("available_range".into(), Value::Null);
    reference.insert("available_image_bounds".into(), Value::Null);
    match &event.source.path {
        Some(path) => {
            reference.insert("target_url".into(), json!(url_of(path)));
        }
        None => {
            reference.insert("OTIO_SCHEMA".into(), json!("MissingReference.1"));
        }
    }
    json!({
        "OTIO_SCHEMA": "Clip.2",
        "metadata": {},
        "name": event.name,
        "source_range": time_range(
            rational_time(event.source.start.value as f64, source_rate),
            rational_time(length, source_rate),
        ),
        "effects": effects,
        "markers": [],
        "enabled": true,
        "media_references": { "DEFAULT_MEDIA": Value::Object(reference) },
        "active_media_reference_key": "DEFAULT_MEDIA",
    })
}

#[cfg(test)]
#[path = "otio_tests.rs"]
mod tests;
