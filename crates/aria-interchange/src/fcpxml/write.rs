//! Writing FCPXML.

use std::collections::HashMap;
use std::fmt::{self, Write as _};
use std::io::{self, Write};
use std::path::Path;

use aria_models::Rational;

use super::{DISSOLVE_UID, VERSION};
use crate::model::{Cut, Event, Lane, LaneKind};
use crate::timecode;
use crate::url::url_of;

/// Exact seconds, `num / den` in lowest terms with `den` positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Secs {
    num: i128,
    den: i128,
}

impl Secs {
    const ZERO: Secs = Secs { num: 0, den: 1 };

    /// `num / den`. Every caller passes a positive `den`: the rates and
    /// speeds written are checked by [`writable`].
    fn new(num: i128, den: i128) -> Secs {
        let divisor = gcd(num, den) * den.signum();
        Secs {
            num: num / divisor,
            den: den / divisor,
        }
    }

    /// `units` at `rate` units a second.
    fn of(units: i64, rate: Rational) -> Secs {
        Secs::new(
            i128::from(units) * i128::from(rate.den),
            i128::from(rate.num),
        )
    }

    fn add(self, other: Secs) -> Secs {
        Secs::new(
            self.num * other.den + other.num * self.den,
            self.den * other.den,
        )
    }

    fn scale(self, by: Rational) -> Secs {
        Secs::new(self.num * i128::from(by.num), self.den * i128::from(by.den))
    }

    fn half(self) -> Secs {
        Secs::new(self.num, self.den * 2)
    }

    fn max(self, other: Secs) -> Secs {
        if self.num * other.den >= other.num * self.den {
            self
        } else {
            other
        }
    }
}

impl fmt::Display for Secs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}s", self.num)
        } else {
            write!(f, "{}/{}s", self.num, self.den)
        }
    }
}

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.unsigned_abs(), b.unsigned_abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    i128::try_from(a.max(1)).unwrap_or(1)
}

/// Whether `event` has a length, a source rate and a speed.
fn writable(event: &Event) -> bool {
    event.duration > 0 && event.source.start.rate.is_valid() && event.speed.is_valid()
}

/// Resource ids, `r1` upward.
struct Ids(usize);

impl Ids {
    fn next(&mut self) -> String {
        self.0 += 1;
        format!("r{}", self.0)
    }
}

/// One `asset` written.
struct AssetOut<'c> {
    id: String,
    reel: &'c str,
    path: Option<&'c Path>,
    rate: Rational,
    video: bool,
    audio: bool,
    /// The latest instant any clip reads.
    end: Secs,
    /// The `format` it references, or `None` for sound given by `audioRate`.
    format: Option<String>,
}

/// The `format`s and `asset`s written: the sequence's format first.
struct Resources<'c> {
    formats: Vec<(String, Rational)>,
    assets: Vec<AssetOut<'c>>,
    index: HashMap<(Option<&'c Path>, &'c str), usize>,
}

impl<'c> Resources<'c> {
    fn of(
        lanes: &[(Option<i32>, &'c Lane)],
        rate: Rational,
        ids: &mut Ids,
        skipped: &mut Vec<String>,
    ) -> Resources<'c> {
        let mut formats = vec![(ids.next(), rate)];
        let mut assets: Vec<AssetOut<'c>> = Vec::new();
        let mut index = HashMap::new();
        for &(_, lane) in lanes {
            for event in &lane.events {
                if !writable(event) {
                    if event.duration > 0 {
                        skipped.push(format!(
                            "{} {}: its source rate or speed is not a rate, not written",
                            lane.name, event.name
                        ));
                    }
                    continue;
                }
                let key = (event.source.path.as_deref(), event.source.reel.as_str());
                let at = *index.entry(key).or_insert_with(|| {
                    assets.push(AssetOut {
                        id: ids.next(),
                        reel: &event.source.reel,
                        path: event.source.path.as_deref(),
                        rate: event.source.start.rate,
                        video: false,
                        audio: false,
                        end: Secs::ZERO,
                        format: None,
                    });
                    assets.len() - 1
                });
                let asset = &mut assets[at];
                asset.video |= lane.kind == LaneKind::Video;
                asset.audio |= lane.kind == LaneKind::Audio || event.sound;
                let window = Secs::of(event.duration, rate).scale(event.speed);
                let head = Secs::of(event.source.start.value, event.source.start.rate);
                asset.end = asset.end.max(head.add(window));
            }
        }
        for asset in &mut assets {
            // Sound states its sample rate as `audioRate` rather than a format.
            if !asset.video {
                continue;
            }
            let reduced = asset.rate.reduced();
            let found = formats
                .iter()
                .find(|(_, rate)| rate.reduced() == reduced)
                .map(|(id, _)| id.clone());
            asset.format = Some(found.unwrap_or_else(|| {
                let id = ids.next();
                formats.push((id.clone(), asset.rate));
                id
            }));
        }
        Resources {
            formats,
            assets,
            index,
        }
    }

    fn asset(&self, event: &Event) -> Option<&AssetOut<'c>> {
        let key = (event.source.path.as_deref(), event.source.reel.as_str());
        self.index.get(&key).map(|&at| &self.assets[at])
    }
}

/// One item of the spine: a clip or a gap.
struct Slot<'c> {
    offset: i64,
    duration: i64,
    /// The slot's own time at its first frame.
    start: Secs,
    event: Option<&'c Event>,
    /// Connected clips, then markers, written.
    inner: String,
}

impl Slot<'_> {
    fn gap(offset: i64, duration: i64) -> Self {
        Slot {
            offset,
            duration,
            start: Secs::ZERO,
            event: None,
            inner: String::new(),
        }
    }
}

/// Where a clip is written: its lane when connected, its offset in its
/// parent's time, and whether it is picture or sound.
#[derive(Clone, Copy)]
struct Placement {
    lane: Option<i32>,
    offset: Secs,
    kind: LaneKind,
}

/// Write `cut`, returning what FCPXML could not hold.
pub fn write(cut: &Cut, out: &mut dyn Write) -> io::Result<Vec<String>> {
    let rate = cut.rate;
    if !rate.is_valid() {
        return Err(io::Error::other("the timeline has no frame rate"));
    }
    let mut skipped = Vec::new();
    // The first video lane is the spine; the others connect above it, and
    // audio lanes connect below it.
    let mut lanes: Vec<(Option<i32>, &Lane)> = Vec::new();
    let mut video = cut.lanes.iter().filter(|lane| lane.kind == LaneKind::Video);
    let spine_lane = video.next();
    lanes.extend(spine_lane.map(|lane| (None, lane)));
    lanes.extend((1..).zip(video).map(|(number, lane)| (Some(number), lane)));
    let audio = cut.lanes.iter().filter(|lane| lane.kind == LaneKind::Audio);
    lanes.extend((1..).zip(audio).map(|(number, lane)| (Some(-number), lane)));
    let mut ids = Ids(0);
    let resources = Resources::of(&lanes, rate, &mut ids, &mut skipped);
    let dissolve_id = ids.next();

    let mut slots = spine(spine_lane, cut.end(), &mut skipped);
    for &(number, lane) in &lanes {
        let Some(number) = number else {
            continue;
        };
        if lane.events.iter().any(|event| event.dissolve.is_some()) {
            skipped.push(format!(
                "{}: dissolves not written; FCPXML connected clips hold no transitions",
                lane.name
            ));
        }
        for event in lane.events.iter().filter(|event| writable(event)) {
            let (Some(at), Some(asset)) = (slot_at(&slots, event.start), resources.asset(event))
            else {
                skipped.push(format!(
                    "{} {}: before the start, not written",
                    lane.name, event.name
                ));
                continue;
            };
            let slot = &mut slots[at];
            let place = Placement {
                lane: Some(number),
                offset: slot.start.add(Secs::of(event.start - slot.offset, rate)),
                kind: lane.kind,
            };
            clip(&mut slot.inner, 7, event, asset, place, rate, "");
        }
    }
    if cut.marks.iter().any(|mark| mark.color.is_some()) {
        skipped.push("markers: colors not written; FCPXML markers have none".to_owned());
    }
    for mark in &cut.marks {
        let Some(at) = slot_at(&slots, mark.at) else {
            skipped.push(format!(
                "marker {}: before the start, not written",
                mark.name
            ));
            continue;
        };
        let slot = &mut slots[at];
        let start = slot.start.add(Secs::of(mark.at - slot.offset, rate));
        let _ = write!(
            slot.inner,
            "{}<marker start=\"{start}\" duration=\"{}\" value=\"",
            indent(7),
            Secs::of(1, rate)
        );
        escape(&mark.name, &mut slot.inner);
        slot.inner.push('"');
        if !mark.comment.is_empty() {
            slot.inner.push_str(" note=\"");
            escape(&mark.comment, &mut slot.inner);
            slot.inner.push('"');
        }
        slot.inner.push_str("/>\n");
    }

    let mut xml = String::with_capacity(4096);
    let dissolves = slots
        .windows(2)
        .any(|pair| pair[0].event.is_some() && pair[1].event.is_some_and(|e| e.dissolve.is_some()));
    resources_xml(
        &mut xml,
        cut,
        &resources,
        dissolves.then_some(dissolve_id.as_str()),
    );
    xml.push_str("    <library>\n        <event name=\"");
    let title = if cut.name.is_empty() {
        "Timeline"
    } else {
        cut.name.as_str()
    };
    escape(title, &mut xml);
    xml.push_str("\">\n            <project name=\"");
    escape(title, &mut xml);
    let tc_format = if timecode::is_drop_rate(rate) {
        "DF"
    } else {
        "NDF"
    };
    let _ = writeln!(
        xml,
        "\">\n{}<sequence format=\"{}\" duration=\"{}\" tcStart=\"0s\" tcFormat=\"{tc_format}\">",
        indent(4),
        resources.formats[0].0,
        Secs::of(cut.end(), rate)
    );
    let _ = writeln!(xml, "{}<spine>", indent(5));
    for (index, slot) in slots.iter().enumerate() {
        let Some(event) = slot.event else {
            let _ = write!(
                xml,
                "{}<gap name=\"Gap\" offset=\"{}\" start=\"0s\" duration=\"{}\"",
                indent(6),
                Secs::of(slot.offset, rate),
                Secs::of(slot.duration, rate)
            );
            if slot.inner.is_empty() {
                xml.push_str("/>\n");
            } else {
                let _ = write!(xml, ">\n{}{}</gap>\n", slot.inner, indent(6));
            }
            continue;
        };
        let Some(asset) = resources.asset(event) else {
            continue;
        };
        if let Some(frames) = event.dissolve {
            if index > 0 && slots[index - 1].event.is_some() {
                transition(&mut xml, event.start, frames, rate, &dissolve_id);
            } else {
                skipped.push(format!(
                    "{} {}: a dissolve from no clip, not written",
                    spine_lane.map_or("", |lane| lane.name.as_str()),
                    event.name
                ));
            }
        }
        let place = Placement {
            lane: None,
            offset: Secs::of(slot.offset, rate),
            kind: LaneKind::Video,
        };
        clip(&mut xml, 6, event, asset, place, rate, &slot.inner);
    }
    let _ = writeln!(xml, "{}</spine>", indent(5));
    let _ = writeln!(xml, "{}</sequence>", indent(4));
    xml.push_str("            </project>\n        </event>\n    </library>\n</fcpxml>\n");
    out.write_all(xml.as_bytes())?;
    Ok(skipped)
}

/// The spine: `lane`'s clips end to end with gaps between, to `total`.
fn spine<'c>(lane: Option<&'c Lane>, total: i64, skipped: &mut Vec<String>) -> Vec<Slot<'c>> {
    let mut slots = Vec::new();
    let mut cursor = 0;
    for event in lane.into_iter().flat_map(|lane| &lane.events) {
        if !writable(event) {
            continue;
        }
        if event.start < cursor {
            skipped.push(format!(
                "{} {}: overlaps the clip before it, not written",
                lane.map_or("", |lane| lane.name.as_str()),
                event.name
            ));
            continue;
        }
        if event.start > cursor {
            slots.push(Slot::gap(cursor, event.start - cursor));
        }
        slots.push(Slot {
            start: local_start(event),
            event: Some(event),
            ..Slot::gap(event.start, event.duration)
        });
        cursor = event.end();
    }
    if total > cursor {
        slots.push(Slot::gap(cursor, total - cursor));
    }
    slots
}

/// The spine item under `frame`.
fn slot_at(slots: &[Slot], frame: i64) -> Option<usize> {
    if frame < 0 {
        return None;
    }
    slots
        .partition_point(|slot| slot.offset <= frame)
        .checked_sub(1)
}

fn resources_xml(xml: &mut String, cut: &Cut, resources: &Resources, dissolve: Option<&str>) {
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n\n");
    let _ = writeln!(xml, "<fcpxml version=\"{VERSION}\">");
    xml.push_str("    <resources>\n");
    for (index, (id, rate)) in resources.formats.iter().enumerate() {
        let _ = write!(
            xml,
            "        <format id=\"{id}\" frameDuration=\"{}\"",
            Secs::of(1, *rate)
        );
        if let (0, Some((width, height))) = (index, cut.size) {
            let _ = write!(xml, " width=\"{width}\" height=\"{height}\"");
        }
        xml.push_str("/>\n");
    }
    for asset in &resources.assets {
        let _ = write!(xml, "        <asset id=\"{}\" name=\"", asset.id);
        escape(asset.reel, xml);
        let _ = write!(xml, "\" start=\"0s\" duration=\"{}\"", asset.end);
        if asset.video {
            xml.push_str(" hasVideo=\"1\"");
        }
        if asset.audio {
            xml.push_str(" hasAudio=\"1\"");
        }
        match &asset.format {
            Some(format) => {
                let _ = write!(xml, " format=\"{format}\"");
            }
            None => {
                let _ = write!(xml, " audioRate=\"{}\"", audio_rate(asset.rate));
            }
        }
        xml.push_str(">\n            <media-rep kind=\"original-media\" src=\"");
        match asset.path {
            Some(path) => escape(&url_of(path), xml),
            None => escape(asset.reel, xml),
        }
        xml.push_str("\"/>\n        </asset>\n");
    }
    if let Some(id) = dissolve {
        let _ = writeln!(
            xml,
            "        <effect id=\"{id}\" name=\"Cross Dissolve\" uid=\"{DISSOLVE_UID}\"/>"
        );
    }
    xml.push_str("    </resources>\n");
}

/// A dissolve of `frames` centered on the cut at `junction`.
fn transition(xml: &mut String, junction: i64, frames: i64, rate: Rational, effect: &str) {
    let _ = writeln!(
        xml,
        "{}<transition name=\"Cross Dissolve\" offset=\"{}\" duration=\"{}\">",
        indent(6),
        Secs::of(2 * junction - frames, rate).half(),
        Secs::of(frames, rate)
    );
    let _ = writeln!(
        xml,
        "{}<filter-video ref=\"{effect}\" name=\"Cross Dissolve\"/>",
        indent(7)
    );
    let _ = writeln!(xml, "{}</transition>", indent(6));
}

fn is_retimed(event: &Event) -> bool {
    event.reversed || event.speed.num != event.speed.den
}

/// A clip's own time at its first frame: its media's, or zero for a
/// retimed clip, whose `timeMap` starts at zero.
fn local_start(event: &Event) -> Secs {
    if is_retimed(event) {
        Secs::ZERO
    } else {
        Secs::of(event.source.start.value, event.source.start.rate)
    }
}

/// The `audioRate` of sound read at `rate` ticks a second: the rate itself
/// when it counts samples, else 48 kHz, a whole number of samples per
/// millisecond tick.
fn audio_rate(rate: Rational) -> i32 {
    let reduced = rate.reduced();
    if reduced.den == 1 && reduced.num >= 8000 {
        reduced.num
    } else {
        48_000
    }
}

fn indent(depth: usize) -> &'static str {
    const SPACES: &str = "                                        ";
    &SPACES[..(depth * 4).min(SPACES.len())]
}

/// Write `event` as an `asset-clip` at `depth`, with `inner` inside it.
fn clip(
    xml: &mut String,
    depth: usize,
    event: &Event,
    asset: &AssetOut,
    place: Placement,
    rate: Rational,
    inner: &str,
) {
    let length = Secs::of(event.duration, rate);
    let _ = write!(xml, "{}<asset-clip ref=\"{}\"", indent(depth), asset.id);
    if let Some(lane) = place.lane {
        let _ = write!(xml, " lane=\"{lane}\"");
    }
    let _ = write!(xml, " offset=\"{}\" name=\"", place.offset);
    escape(&event.name, xml);
    let _ = write!(
        xml,
        "\" start=\"{}\" duration=\"{length}\"",
        local_start(event)
    );
    match (place.kind, event.sound) {
        (LaneKind::Audio, _) => xml.push_str(" srcEnable=\"audio\""),
        (LaneKind::Video, false) => xml.push_str(" srcEnable=\"video\""),
        (LaneKind::Video, true) => {}
    }
    let retimed = is_retimed(event);
    if inner.is_empty() && !retimed {
        xml.push_str("/>\n");
        return;
    }
    xml.push_str(">\n");
    if retimed {
        let head = Secs::of(event.source.start.value, event.source.start.rate);
        let tail = head.add(length.scale(event.speed));
        let (first, last) = if event.reversed {
            (tail, head)
        } else {
            (head, tail)
        };
        let _ = writeln!(xml, "{}<timeMap>", indent(depth + 1));
        let _ = writeln!(
            xml,
            "{}<timept time=\"0s\" value=\"{first}\" interp=\"linear\"/>",
            indent(depth + 2)
        );
        let _ = writeln!(
            xml,
            "{}<timept time=\"{length}\" value=\"{last}\" interp=\"linear\"/>",
            indent(depth + 2)
        );
        let _ = writeln!(xml, "{}</timeMap>", indent(depth + 1));
    }
    xml.push_str(inner);
    let _ = writeln!(xml, "{}</asset-clip>", indent(depth));
}

/// `text` as XML attribute content.
fn escape(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            // XML 1.0 has no form for the other control characters.
            ch if u32::from(ch) < 0x20 => out.push(' '),
            '\u{FFFE}' | '\u{FFFF}' => out.push(' '),
            ch => out.push(ch),
        }
    }
}
