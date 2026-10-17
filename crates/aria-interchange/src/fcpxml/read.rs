//! Reading FCPXML.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use aria_models::{Rational, RationalTime};
use roxmltree::{Document, Node, ParsingOptions};

use crate::model::{
    Cut, Event, Lane, LaneKind, Mark, Read, Source, file_stem, rational_of, speed_of,
};
use crate::timecode;
use crate::url::path_of_url;

/// The most XML nodes read: far past any real timeline.
const NODES_LIMIT: u32 = 2_000_000;
/// The largest time read, in seconds.
const LIMIT: f64 = 1.0e12;
/// How deep connected clips and storylines nest before reading stops.
const MAX_DEPTH: usize = 32;
/// The fastest retime read.
const MAX_SPEED: f64 = 1000.0;

/// A `format` resource.
#[derive(Debug, Clone, Copy)]
struct Format {
    rate: Option<Rational>,
    size: Option<(u32, u32)>,
}

/// An `asset` resource.
#[derive(Debug, Clone)]
struct Asset {
    name: String,
    path: Option<PathBuf>,
    /// Seconds of the asset's first instant.
    start: f64,
    rate: Rational,
    video: bool,
    audio: bool,
}

/// A clip's own time as its media's: straight lines through its `timept`s,
/// or unchanged when it is not retimed.
struct TimeMap(Vec<(f64, f64)>);

impl TimeMap {
    fn of(node: Node) -> Result<TimeMap, String> {
        let Some(map) = child(node, "timeMap") else {
            return Ok(TimeMap(Vec::new()));
        };
        let mut points = Vec::new();
        for point in map.children().filter(|point| point.has_tag_name("timept")) {
            points.push((required(point, "time")?, required(point, "value")?));
        }
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(TimeMap(points))
    }

    fn at(&self, time: f64) -> f64 {
        match self.0.as_slice() {
            [] => time,
            [(_, value)] => *value,
            points => {
                let segment = points
                    .windows(2)
                    .position(|pair| time < pair[1].0)
                    .unwrap_or(points.len() - 2);
                let ((t0, v0), (t1, v1)) = (points[segment], points[segment + 1]);
                if t1 > t0 {
                    v0 + (time - t0) * (v1 - v0) / (t1 - t0)
                } else {
                    v1
                }
            }
        }
    }

    /// Whether the map is one straight line from `from` to `to`.
    fn is_linear(&self, from: f64, to: f64) -> bool {
        let slope = (self.at(to) - self.at(from)) / (to - from);
        self.0
            .windows(2)
            .filter(|pair| pair[1].0 > pair[0].0 && pair[1].0 > from && pair[0].0 < to)
            .all(|pair| {
                let segment = (pair[1].1 - pair[0].1) / (pair[1].0 - pair[0].0);
                (segment - slope).abs() <= 1.0e-6 * slope.abs().max(1.0)
            })
    }
}

/// What reading the sequence fills in.
struct Reader {
    rate: Rational,
    drop: bool,
    assets: HashMap<String, Asset>,
    /// Events by lane number: the spine is 0.
    video: BTreeMap<i32, Vec<Event>>,
    audio: BTreeMap<i32, Vec<Event>>,
    marks: Vec<Mark>,
    skipped: Vec<String>,
}

/// Read `text`. `rate` is the cut's rate when the sequence names no format,
/// and `name` titles it when the project has no name.
pub fn read(text: &str, rate: Rational, name: &str) -> Result<Read, String> {
    // Entities are the one XML feature that expands a small file into a
    // large one, and Final Cut Pro writes none.
    if text.contains("<!ENTITY") {
        return Err("the file declares XML entities, which are not read".to_owned());
    }
    let options = ParsingOptions {
        allow_dtd: true,
        nodes_limit: NODES_LIMIT,
        ..ParsingOptions::default()
    };
    let doc = Document::parse_with_options(text, options)
        .map_err(|err| format!("the file is not XML: {err}"))?;
    let root = doc.root_element();
    if !root.has_tag_name("fcpxml") {
        return Err("the file holds no fcpxml element".to_owned());
    }
    if root
        .attribute("version")
        .is_some_and(|version| !version.starts_with("1."))
    {
        return Err("the file is not FCPXML version 1".to_owned());
    }
    let mut projects = root
        .descendants()
        .filter(|node| node.has_tag_name("project"));
    let project = projects.next().ok_or("the file holds no project")?;
    let sequence = child(project, "sequence").ok_or("the project holds no sequence")?;
    let mut skipped: Vec<String> = projects
        .map(|other| {
            format!(
                "project {}: not read; only the first project is",
                other.attribute("name").unwrap_or("")
            )
        })
        .collect();
    let mut formats = HashMap::new();
    let resources = child(root, "resources");
    for format in resources
        .iter()
        .flat_map(|resources| resources.children())
        .filter(|node| node.has_tag_name("format"))
    {
        if let Some(id) = format.attribute("id") {
            formats.insert(id, format_of(format));
        }
    }
    let sequence_format = sequence
        .attribute("format")
        .and_then(|id| formats.get(id))
        .copied();
    let rate = sequence_format
        .and_then(|format| format.rate)
        .unwrap_or(rate);
    let mut assets = HashMap::new();
    for asset in resources
        .iter()
        .flat_map(|resources| resources.children())
        .filter(|node| node.has_tag_name("asset"))
    {
        let Some(id) = asset.attribute("id") else {
            continue;
        };
        match asset_of(asset, &formats, rate) {
            Ok(read) => {
                assets.insert(id.to_owned(), read);
            }
            Err(reason) => skipped.push(format!(
                "asset {}: {reason}, not read",
                asset.attribute("name").unwrap_or(id)
            )),
        }
    }
    let mut reader = Reader {
        rate,
        drop: timecode::is_drop_rate(rate) && sequence.attribute("tcFormat") != Some("NDF"),
        assets,
        video: BTreeMap::new(),
        audio: BTreeMap::new(),
        marks: Vec::new(),
        skipped,
    };
    let tc_start = optional(sequence, "tcStart")?.unwrap_or(0.0);
    if let Some(spine) = child(sequence, "spine") {
        reader.spine(spine, 0, -tc_start, 0);
    }
    let mut lanes = Vec::new();
    for (index, (_, events)) in std::mem::take(&mut reader.video).into_iter().enumerate() {
        lanes.push(lane(LaneKind::Video, format!("V{}", index + 1), events));
    }
    // The audio lane nearest the picture is A1.
    for (index, (_, events)) in std::mem::take(&mut reader.audio)
        .into_iter()
        .rev()
        .enumerate()
    {
        lanes.push(lane(LaneKind::Audio, format!("A{}", index + 1), events));
    }
    let mut cut = Cut {
        name: project
            .attribute("name")
            .filter(|name| !name.is_empty())
            .unwrap_or(name)
            .to_owned(),
        rate,
        size: sequence_format.and_then(|format| format.size),
        lanes,
        marks: reader.marks,
    };
    cut.merge_picture_sound();
    Ok(Read {
        cut,
        skipped: reader.skipped,
    })
}

fn lane(kind: LaneKind, name: String, mut events: Vec<Event>) -> Lane {
    events.sort_by_key(|event| event.start);
    Lane { kind, name, events }
}

fn format_of(format: Node) -> Format {
    let rate = format
        .attribute("frameDuration")
        .and_then(seconds)
        .filter(|frame| *frame > 0.0)
        .and_then(|frame| rational_of(1.0 / frame).ok());
    let side = |name: &str| {
        format
            .attribute(name)
            .and_then(|text| text.parse::<u32>().ok())
            .filter(|side| (1..=65_536).contains(side))
    };
    Format {
        rate,
        size: side("width").zip(side("height")),
    }
}

fn asset_of(
    asset: Node,
    formats: &HashMap<&str, Format>,
    fallback: Rational,
) -> Result<Asset, String> {
    let flag = |name: &str| asset.attribute(name).map(|value| value == "1");
    let (video, audio) = match (flag("hasVideo"), flag("hasAudio")) {
        (None, None) => (true, true),
        (video, audio) => (video.unwrap_or(false), audio.unwrap_or(false)),
    };
    let rate = asset
        .attribute("format")
        .and_then(|id| formats.get(id))
        .and_then(|format| format.rate)
        .or_else(|| asset.attribute("audioRate").and_then(sample_rate))
        .unwrap_or(fallback);
    let src = asset.attribute("src").or_else(|| {
        let mut reps = asset
            .children()
            .filter(|node| node.has_tag_name("media-rep"));
        let first = reps.clone().next();
        reps.find(|rep| rep.attribute("kind") == Some("original-media"))
            .or(first)
            .and_then(|rep| rep.attribute("src"))
    });
    let path = src.and_then(path_of_url);
    let name = asset
        .attribute("name")
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .or_else(|| path.as_deref().map(|path| file_stem(path, "")))
        .unwrap_or_default();
    Ok(Asset {
        name,
        path,
        start: optional(asset, "start")?.unwrap_or(0.0),
        rate,
        video,
        audio,
    })
}

/// An `audioRate` such as `48000` or `44.1k`.
fn sample_rate(text: &str) -> Option<Rational> {
    let (number, scale) = match text.strip_suffix('k') {
        Some(thousands) => (thousands, 1000.0),
        None => (text, 1.0),
    };
    let rate = number.parse::<f64>().ok()? * scale;
    (rate >= 1.0).then(|| rational_of(rate).ok()).flatten()
}

impl Reader {
    fn frames(&self, seconds: f64) -> i64 {
        (seconds * self.rate.as_f64()).round() as i64
    }

    /// Note that `what` about the element named `name` at `at` record
    /// seconds was not read as written.
    fn report(&mut self, name: &str, at: f64, what: &str) {
        let tc = timecode::format(self.frames(at), self.rate, self.drop);
        self.skipped.push(format!("{name} at {tc}: {what}"));
    }

    fn report_node(&mut self, node: Node, at: f64, what: &str) {
        let name = node
            .attribute("name")
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| node.tag_name().name());
        self.report(name, at, what);
    }

    /// Read the items of a storyline whose time 0 lands at `origin` record
    /// seconds.
    fn spine(&mut self, spine: Node, lane: i32, origin: f64, depth: usize) {
        // The record seconds a transition begins at, and its length.
        let mut transition: Option<(f64, f64)> = None;
        let mut after_clip = false;
        for item in spine.children().filter(Node::is_element) {
            if item.has_tag_name("transition") {
                let read = optional(item, "offset").and_then(|offset| {
                    Ok((origin + offset.unwrap_or(0.0), required(item, "duration")?))
                });
                match read {
                    Ok((begin, _)) if !after_clip => {
                        self.report("transition", begin, "a transition from nothing, not read");
                    }
                    Ok(found) => transition = Some(found),
                    Err(reason) => self.report("transition", origin, &reason),
                }
                continue;
            }
            let at = origin + optional(item, "offset").ok().flatten().unwrap_or(0.0);
            let dissolve = transition
                .map(|(_, length)| (length * self.rate.as_f64()).round() as i64)
                .filter(|frames| *frames > 0);
            let placed = match self.item(item, lane, origin, depth, dissolve) {
                Ok(placed) => placed,
                Err(reason) => {
                    self.report_node(item, at, &reason);
                    false
                }
            };
            if let Some((begin, length)) = transition.take() {
                if !placed {
                    self.report("transition", begin, "a transition into no clip, not read");
                } else if ((begin + length / 2.0 - at) * self.rate.as_f64()).abs() >= 1.0 {
                    self.report("transition", begin, "off its cut, read centered on it");
                }
            }
            after_clip = placed;
        }
        if let Some((begin, _)) = transition {
            self.report("transition", begin, "a transition to nothing, not read");
        }
    }

    /// Read one story element placed in a parent whose time 0 lands at
    /// `origin` record seconds. Returns whether it became an event.
    fn item(
        &mut self,
        node: Node,
        lane: i32,
        origin: f64,
        depth: usize,
        dissolve: Option<i64>,
    ) -> Result<bool, String> {
        if depth > MAX_DEPTH {
            return Err(format!("nested deeper than {MAX_DEPTH}, not read"));
        }
        let tag = node.tag_name().name();
        let at = origin + optional(node, "offset")?.unwrap_or(0.0);
        if node.attribute("enabled") == Some("0") {
            self.report_node(node, at, "disabled, not read");
            return Ok(false);
        }
        match tag {
            "spine" => {
                let first = node
                    .children()
                    .filter(Node::is_element)
                    .find_map(|item| item.attribute("offset"))
                    .and_then(seconds)
                    .unwrap_or(0.0);
                self.spine(node, lane, at - first, depth + 1);
                Ok(false)
            }
            "gap" | "title" => {
                if tag == "title" {
                    self.report_node(node, at, "a title, not read");
                }
                let start = optional(node, "start")?.unwrap_or(0.0);
                let duration = required(node, "duration")?;
                self.anchored(node, lane, at - start, (start, duration), depth, false);
                Ok(false)
            }
            "asset-clip" | "clip" | "video" | "audio" => self.clip(node, lane, at, depth, dissolve),
            "ref-clip" | "mc-clip" | "sync-clip" | "audition" | "live-drawing" => {
                self.report_node(node, at, &format!("a {tag}, not read"));
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn clip(
        &mut self,
        node: Node,
        lane: i32,
        at: f64,
        depth: usize,
        dissolve: Option<i64>,
    ) -> Result<bool, String> {
        let tag = node.tag_name().name();
        let duration = required(node, "duration")?;
        let map = TimeMap::of(node)?;
        // A `clip` shows the `video` or `audio` element inside it; the
        // others name their media themselves.
        let content = if tag == "clip" {
            let primary = node
                .children()
                .find(|child| child.attribute("lane").is_none() && is_story(*child));
            match primary.filter(|child| matches!(child.tag_name().name(), "video" | "audio")) {
                Some(primary) => Some(primary),
                None => {
                    self.report_node(node, at, "a compound clip, not read");
                    return Ok(false);
                }
            }
        } else {
            None
        };
        let media = content.unwrap_or(node);
        let asset = media
            .attribute("ref")
            .and_then(|id| self.assets.get(id))
            .ok_or("its media reference names no asset")?
            .clone();
        let src_enable = node.attribute("srcEnable").unwrap_or("all");
        let (kind, sound) = match (media.tag_name().name(), src_enable) {
            ("audio", _) | (_, "audio") => (LaneKind::Audio, true),
            ("video", _) | (_, "video") => (LaneKind::Video, false),
            _ if asset.video => (LaneKind::Video, asset.audio),
            _ => (LaneKind::Audio, true),
        };
        if kind == LaneKind::Video && !asset.video {
            self.report_node(node, at, "its media has no picture, not read");
            return Ok(false);
        }
        let start =
            optional(node, "start")?.unwrap_or(if content.is_some() { 0.0 } else { asset.start });
        // Where the clip's own time sits in the content's.
        let (content_offset, content_start) = match content {
            Some(content) => (
                optional(content, "offset")?.unwrap_or(0.0),
                optional(content, "start")?.unwrap_or(asset.start),
            ),
            None => (0.0, 0.0),
        };
        let to_media = |time: f64| content_start + map.at(time) - content_offset;
        let (head, tail) = (to_media(start), to_media(start + duration));
        let first = self.frames(at);
        let end = self.frames(at + duration);
        let span = (tail - head).abs();
        if end <= first || duration <= 0.0 {
            return Ok(false);
        }
        if !(head.is_finite() && tail.is_finite() && head.abs() < LIMIT && tail.abs() < LIMIT) {
            return Err("its timing is out of range".to_owned());
        }
        let speed = span / duration;
        if speed * asset.rate.as_f64() * duration < 0.5 {
            self.report_node(node, at, "a freeze frame, not read");
            return Ok(false);
        }
        if speed > MAX_SPEED {
            return Err(format!("plays faster than {MAX_SPEED} times, not read"));
        }
        if !map.is_linear(start, start + duration) {
            self.report_node(node, at, "a speed ramp, read at its average speed");
        }
        let effects: Vec<&str> = node
            .children()
            .chain(content.iter().flat_map(|content| content.children()))
            .map(|child| child.tag_name().name())
            .filter(|name| name.starts_with("filter-") || name.starts_with("adjust-"))
            .collect();
        if !effects.is_empty() {
            self.report_node(node, at, &format!("{}, not read", effects.join(", ")));
        }
        let name = node
            .attribute("name")
            .filter(|name| !name.is_empty())
            .unwrap_or(&asset.name)
            .to_owned();
        let source_start = ((head.min(tail) - asset.start) * asset.rate.as_f64()).round() as i64;
        let event = Event {
            name,
            start: first,
            duration: end - first,
            source: Source {
                reel: asset.name,
                path: asset.path,
                start: RationalTime::new(source_start, asset.rate),
            },
            speed: speed_of(speed),
            reversed: tail < head,
            dissolve,
            sound,
        };
        let lanes = match kind {
            LaneKind::Video => &mut self.video,
            LaneKind::Audio => &mut self.audio,
        };
        lanes.entry(lane).or_default().push(event);
        let origin = at - start;
        // A picture-only `clip` drops the sound components inside it. The
        // audio connected to an `asset-clip` is other clips, and stays.
        let picture_only = content.is_some() && src_enable == "video";
        self.anchored(node, lane, origin, (start, duration), depth, picture_only);
        if let Some(content) = content {
            // The span of the content's own time the clip shows.
            let shown = content_start + start - content_offset;
            self.anchored(
                content,
                lane,
                origin + content_offset - content_start,
                (shown, duration),
                depth,
                picture_only,
            );
        }
        Ok(true)
    }

    /// Read the markers and connected clips of `node`, whose time 0 lands at
    /// `origin` record seconds and which shows `shown.0` for `shown.1`
    /// seconds of its own time. A picture-only clip drops its own sound.
    fn anchored(
        &mut self,
        node: Node,
        lane: i32,
        origin: f64,
        shown: (f64, f64),
        depth: usize,
        picture_only: bool,
    ) {
        for child in node.children().filter(Node::is_element) {
            let name = child.tag_name().name();
            if matches!(name, "marker" | "chapter-marker") {
                if let Err(reason) = self.marker(child, origin, shown) {
                    self.report("marker", origin + shown.0, &reason);
                }
                continue;
            }
            let Some(raw) = child.attribute("lane") else {
                continue;
            };
            if picture_only && name == "audio" {
                continue;
            }
            let result = raw
                .parse::<i32>()
                .ok()
                .and_then(|offset| lane.checked_add(offset))
                .ok_or_else(|| "its lane is not a whole number".to_owned())
                .and_then(|child_lane| self.item(child, child_lane, origin, depth + 1, None));
            if let Err(reason) = result {
                self.report_node(child, origin, &reason);
            }
        }
    }

    fn marker(&mut self, marker: Node, origin: f64, shown: (f64, f64)) -> Result<(), String> {
        let at = required(marker, "start")?;
        if at < shown.0 || at > shown.0 + shown.1 {
            return Ok(());
        }
        self.marks.push(Mark {
            at: self.frames(origin + at),
            name: marker.attribute("value").unwrap_or("").to_owned(),
            comment: marker.attribute("note").unwrap_or("").to_owned(),
            color: None,
        });
        Ok(())
    }
}

/// Whether `node` is a story element: a clip, a gap, a title or a
/// storyline.
fn is_story(node: Node) -> bool {
    matches!(
        node.tag_name().name(),
        "asset-clip"
            | "clip"
            | "video"
            | "audio"
            | "gap"
            | "title"
            | "ref-clip"
            | "mc-clip"
            | "sync-clip"
            | "audition"
            | "live-drawing"
            | "spine"
    )
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|child| child.has_tag_name(name))
}

/// The seconds an FCPXML time names: `N/Ds`, `Ns`, or a decimal.
fn seconds(text: &str) -> Option<f64> {
    let body = text.strip_suffix('s')?;
    let value = match body.split_once('/') {
        Some((num, den)) => {
            let num = num.parse::<i64>().ok()?;
            let den = den.parse::<i64>().ok().filter(|den| *den > 0)?;
            num as f64 / den as f64
        }
        None => body.parse::<f64>().ok()?,
    };
    (value.is_finite() && value.abs() < LIMIT).then_some(value)
}

fn optional(node: Node, name: &str) -> Result<Option<f64>, String> {
    node.attribute(name)
        .map(|text| seconds(text).ok_or_else(|| format!("its {name} is not a time")))
        .transpose()
}

fn required(node: Node, name: &str) -> Result<f64, String> {
    optional(node, name)?.ok_or_else(|| format!("it has no {name}"))
}
