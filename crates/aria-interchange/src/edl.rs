//! CMX 3600 edit decision lists.
//!
//! An EDL holds one video track and up to four audio channels, with record
//! timecode at one rate the file does not state. Reading takes the rate from
//! the caller; writing uses the cut's. Source timecode counts from the start
//! of each file.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;

use aria_models::{MarkerColor, Rational, RationalTime, resample};

use crate::model::{Cut, Event, Lane, LaneKind, Mark, Read, Source, file_stem};
use crate::timecode;

/// The highest event number CMX 3600 allows.
const MAX_EVENTS: usize = 999;
/// The longest reel name CMX 3600 allows.
const REEL_LEN: usize = 8;
/// The channel field of audio lanes A1 to A4.
const AUDIO_CHANNELS: [&str; 4] = ["A", "A2", "A3", "A4"];

/// How an event begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transition {
    Cut,
    Dissolve(i64),
    Wipe(i64),
}

/// One event line, with the comment and speed lines after it.
#[derive(Debug)]
struct Line {
    number: String,
    reel: String,
    video: bool,
    sound: bool,
    audio: Option<usize>,
    transition: Transition,
    src_in: i64,
    rec_in: i64,
    rec_out: i64,
    from_name: Option<String>,
    to_name: Option<String>,
    file: Option<String>,
    /// Source frames per second from an `M2` line; negative plays backward.
    m2: Option<f64>,
}

/// A `LOC:` marker comment.
struct Loc {
    at: i64,
    name: String,
    color: Option<MarkerColor>,
}

/// Read `text` at `rate`. `name` titles the cut when the file has no
/// `TITLE:` line.
pub fn read(text: &str, rate: Rational, name: &str) -> Result<Read, String> {
    let mut skipped = Vec::new();
    let mut title = None;
    let mut drop = false;
    let mut rate = rate;
    let mut lines: Vec<Line> = Vec::new();
    let mut locs: Vec<Loc> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = strip_label(line, "TITLE:") {
            title = Some(rest.to_owned());
        } else if let Some(rest) = strip_label(line, "FCM:") {
            let upper = rest.to_ascii_uppercase();
            drop = upper.contains("DROP") && !upper.contains("NON");
            rate = if drop { drop_rate(rate) } else { rate };
        } else if let Some(comment) = line.strip_prefix('*') {
            if let Err(reason) = comment_line(comment.trim(), rate, drop, &mut lines, &mut locs) {
                skipped.push(format!("line {number}: {reason}"));
            }
        } else if let Some(rest) = line.strip_prefix("M2") {
            if let Err(reason) = speed_line(rest, &mut lines) {
                skipped.push(format!("line {number}: {reason}"));
            }
        } else if line.split_whitespace().next().is_some_and(is_event_number) {
            match event_line(line, rate, drop) {
                Ok(Some(event)) => lines.push(event),
                Ok(None) => skipped.push(format!("line {number}: a key, which is not read")),
                Err(reason) => skipped.push(format!("line {number}: {reason}")),
            }
        } else {
            skipped.push(format!("line {number}: {line:?} is not read"));
        }
    }
    if lines.is_empty() && locs.is_empty() {
        return Err("the file holds no EDL events".to_owned());
    }
    let offset = record_offset(&lines, rate, drop);
    let mut paths: HashMap<&str, &str> = HashMap::new();
    for line in &lines {
        if let Some(file) = &line.file {
            paths.entry(line.reel.as_str()).or_insert(file.as_str());
        }
    }
    let mut video = Vec::new();
    let mut audio: [Vec<Event>; 4] = Default::default();
    for (index, line) in lines.iter().enumerate() {
        // The zero-length outgoing half of a dissolve carries nothing of its
        // own, and black is a gap.
        if line.rec_out <= line.rec_in || is_black(&line.reel) {
            continue;
        }
        // The outgoing half of a dissolve is the line before it, numbered alike.
        let from = index
            .checked_sub(1)
            .map(|before| &lines[before])
            .filter(|before| before.number == line.number);
        let Some(mut event) = event_of(line, offset, rate, &paths, &mut skipped) else {
            continue;
        };
        if event.dissolve.is_some() {
            if from.is_some_and(|from| is_black(&from.reel)) {
                skipped.push(format!(
                    "event {}: a dissolve from black, read as a cut",
                    line.number
                ));
                event.dissolve = None;
            } else if matches!(line.transition, Transition::Wipe(_)) {
                skipped.push(format!("event {}: a wipe, read as a dissolve", line.number));
            }
        }
        if line.video {
            video.push(event.clone());
        }
        if let Some(channel) = line.audio {
            event.sound = true;
            audio[channel].push(event);
        }
    }
    let mut lanes = Vec::new();
    if !video.is_empty() {
        lanes.push(lane(LaneKind::Video, "V1".to_owned(), video));
    }
    for (channel, events) in audio.into_iter().enumerate() {
        if !events.is_empty() {
            lanes.push(lane(LaneKind::Audio, format!("A{}", channel + 1), events));
        }
    }
    let marks = locs
        .into_iter()
        .map(|loc| Mark {
            at: loc.at - offset,
            name: loc.name,
            comment: String::new(),
            color: loc.color,
        })
        .collect();
    let mut cut = Cut {
        name: title.unwrap_or_else(|| name.to_owned()),
        rate,
        size: None,
        lanes,
        marks,
    };
    cut.merge_picture_sound();
    Ok(Read { cut, skipped })
}

/// The drop-frame rate `rate` counts as, when `FCM: DROP FRAME` names it.
fn drop_rate(rate: Rational) -> Rational {
    match rate.nominal_fps() {
        30 => Rational::FPS_29_97,
        60 => Rational::FPS_59_94,
        _ => rate,
    }
}

fn lane(kind: LaneKind, name: String, mut events: Vec<Event>) -> Lane {
    events.sort_by_key(|event| event.start);
    Lane { kind, name, events }
}

/// `line` after a case-insensitive `label`, trimmed.
fn strip_label<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let head = line.get(..label.len())?;
    head.eq_ignore_ascii_case(label)
        .then(|| line[label.len()..].trim())
}

fn is_event_number(token: &str) -> bool {
    !token.is_empty() && token.len() <= 6 && token.bytes().all(|b| b.is_ascii_digit())
}

fn is_black(reel: &str) -> bool {
    ["BL", "BLK", "BLACK"]
        .iter()
        .any(|black| reel.eq_ignore_ascii_case(black))
}

/// The event `line` describes, or `None` for a key.
fn event_line(line: &str, rate: Rational, drop: bool) -> Result<Option<Line>, String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let fields = || {
        format!(
            "an event line holds eight or nine fields, not {}",
            tokens.len()
        )
    };
    let [number, reel, channels, transition, rest @ ..] = tokens.as_slice() else {
        return Err(fields());
    };
    let upper = transition.to_ascii_uppercase();
    if upper.starts_with('K') {
        return Ok(None);
    }
    let (transition, times) = match (upper.as_str(), rest) {
        ("C", times @ [_, _, _, _]) => (Transition::Cut, times),
        (code, [frames, times @ ..]) if times.len() == 4 => {
            let frames = frames
                .parse::<i64>()
                .ok()
                .filter(|frames| *frames >= 0)
                .ok_or_else(|| format!("{frames:?} is not a transition length"))?;
            match code {
                "D" => (Transition::Dissolve(frames), times),
                "C" => (Transition::Cut, times),
                wipe if wipe.starts_with('W') => (Transition::Wipe(frames), times),
                _ => return Err(format!("transition {transition:?} is not read")),
            }
        }
        (_, [_, _, _, _]) => return Err(format!("transition {transition:?} has no length")),
        _ => return Err(fields()),
    };
    let (video, sound, audio) = channels_of(channels)?;
    let tc = |text: &str| timecode::parse(text, rate, drop);
    let src_in = tc(times[0])?;
    tc(times[1])?;
    let rec_in = tc(times[2])?;
    let rec_out = tc(times[3])?;
    Ok(Some(Line {
        number: (*number).to_owned(),
        reel: (*reel).to_owned(),
        video,
        sound,
        audio,
        transition,
        src_in,
        rec_in,
        rec_out,
        from_name: None,
        to_name: None,
        file: None,
        m2: None,
    }))
}

/// What a channel field addresses: the picture, the picture's own sound, and
/// an audio channel.
fn channels_of(channels: &str) -> Result<(bool, bool, Option<usize>), String> {
    Ok(match channels.to_ascii_uppercase().as_str() {
        "V" => (true, false, None),
        "B" | "AA/V" | "A/V" | "A2/V" => (true, true, None),
        "A" | "AA" | "A1" => (false, true, Some(0)),
        "A2" => (false, true, Some(1)),
        "A3" => (false, true, Some(2)),
        "A4" => (false, true, Some(3)),
        _ => return Err(format!("channels {channels:?} are not read")),
    })
}

fn comment_line(
    comment: &str,
    rate: Rational,
    drop: bool,
    lines: &mut [Line],
    locs: &mut Vec<Loc>,
) -> Result<(), String> {
    if let Some(rest) = strip_label(comment, "LOC:") {
        let (time, rest) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let at = timecode::parse(time, rate, drop)?;
        let rest = rest.trim();
        let (first, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let (color, name) = match avid_color(first) {
            Some(color) => (Some(color), after.trim()),
            None => (None, rest),
        };
        locs.push(Loc {
            at,
            name: name.to_owned(),
            color,
        });
        return Ok(());
    }
    let Some(line) = lines.last_mut() else {
        return Ok(());
    };
    if let Some(name) = strip_label(comment, "FROM CLIP NAME:") {
        line.from_name = Some(name.to_owned());
    } else if let Some(name) = strip_label(comment, "TO CLIP NAME:") {
        line.to_name = Some(name.to_owned());
    } else if let Some(file) = strip_label(comment, "SOURCE FILE:") {
        line.file = Some(file.to_owned());
    }
    Ok(())
}

/// Attach an `M2` line to the latest event on its reel.
fn speed_line(rest: &str, lines: &mut [Line]) -> Result<(), String> {
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let [reel, fps, ..] = tokens.as_slice() else {
        return Err("an M2 line holds a reel and a speed".to_owned());
    };
    let fps: f64 = fps
        .parse()
        .ok()
        .filter(|fps: &f64| fps.is_finite() && fps.abs() < 10_000.0)
        .ok_or_else(|| format!("{fps:?} is not a speed"))?;
    let line = lines
        .iter_mut()
        .rev()
        .find(|line| line.reel == *reel)
        .ok_or_else(|| format!("M2 names reel {reel:?}, which no event before it reads"))?;
    line.m2 = Some(fps);
    Ok(())
}

/// The record timecode of frame zero: the whole hour at or before the
/// earliest record in.
fn record_offset(lines: &[Line], rate: Rational, drop: bool) -> i64 {
    let Some(first) = lines.iter().map(|line| line.rec_in).min() else {
        return 0;
    };
    let text = timecode::format(first, rate, drop);
    let hours = &text[..text.len() - 9];
    timecode::parse(&format!("{hours}:00:00:00"), rate, drop).unwrap_or(0)
}

fn event_of(
    line: &Line,
    offset: i64,
    rate: Rational,
    paths: &HashMap<&str, &str>,
    skipped: &mut Vec<String>,
) -> Option<Event> {
    let (speed, reversed) = match line.m2 {
        None => (Rational::new(1, 1), false),
        Some(fps) if fps == 0.0 => {
            skipped.push(format!(
                "event {}: a freeze frame, which is not read",
                line.number
            ));
            return None;
        }
        Some(fps) => (speed_of(fps, rate), fps < 0.0),
    };
    let name = match line.transition {
        Transition::Cut => line.from_name.as_deref(),
        Transition::Dissolve(_) | Transition::Wipe(_) => {
            line.to_name.as_deref().or(line.from_name.as_deref())
        }
    };
    let path = line
        .file
        .as_deref()
        .or_else(|| paths.get(line.reel.as_str()).copied());
    Some(Event {
        name: name.unwrap_or(&line.reel).to_owned(),
        start: line.rec_in - offset,
        duration: line.rec_out - line.rec_in,
        source: Source {
            reel: line.reel.clone(),
            path: path.map(PathBuf::from),
            start: RationalTime::new(line.src_in, rate),
        },
        speed,
        reversed,
        dissolve: match line.transition {
            Transition::Dissolve(frames) | Transition::Wipe(frames) if frames > 0 => Some(frames),
            _ => None,
        },
        sound: line.sound,
    })
}

/// `fps` source frames a second as a multiple of `rate`, to a tenth of a
/// frame.
fn speed_of(fps: f64, rate: Rational) -> Rational {
    let tenths = (fps.abs() * 10.0).round() as i32;
    let nominal = i32::try_from(rate.nominal_fps()).unwrap_or(1);
    Rational::new(tenths.max(1), nominal.saturating_mul(10)).reduced()
}

fn avid_color(word: &str) -> Option<MarkerColor> {
    match word.to_ascii_uppercase().as_str() {
        "RED" => Some(MarkerColor::Red),
        "GREEN" => Some(MarkerColor::Green),
        "BLUE" => Some(MarkerColor::Blue),
        "CYAN" => Some(MarkerColor::Teal),
        "MAGENTA" => Some(MarkerColor::Purple),
        "YELLOW" => Some(MarkerColor::Yellow),
        _ => None,
    }
}

fn avid_color_name(color: Option<MarkerColor>) -> &'static str {
    match color {
        Some(MarkerColor::Red) | None => "RED",
        Some(MarkerColor::Green) => "GREEN",
        Some(MarkerColor::Blue) => "BLUE",
        Some(MarkerColor::Teal) => "CYAN",
        Some(MarkerColor::Purple | MarkerColor::Pink) => "MAGENTA",
        Some(MarkerColor::Yellow | MarkerColor::Orange) => "YELLOW",
    }
}

/// `text` on one line: control characters become spaces.
fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Reel names, one per file: the file stem cut to eight letters, digits
/// and underscores, numbered when two files would share one.
#[derive(Default)]
struct Reels {
    by_key: HashMap<String, String>,
    taken: HashMap<String, String>,
}

impl Reels {
    fn reel(&mut self, source: &Source) -> String {
        let key = source
            .path
            .as_ref()
            .map_or_else(|| source.reel.clone(), |path| path.display().to_string());
        if let Some(reel) = self.by_key.get(&key) {
            return reel.clone();
        }
        let stem = source
            .path
            .as_ref()
            .map_or_else(|| source.reel.clone(), |path| file_stem(path, &source.reel));
        let mut base: String = stem
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .take(REEL_LEN)
            .collect();
        if base.is_empty() || is_black(&base) {
            base.push_str("AX");
            base.truncate(REEL_LEN);
        }
        let mut reel = base.clone();
        let mut counter = 2_u32;
        while self.taken.get(&reel).is_some_and(|owner| *owner != key) {
            let suffix = counter.to_string();
            let keep = REEL_LEN.saturating_sub(suffix.len()).min(base.len());
            reel = format!("{}{suffix}", &base[..keep]);
            counter += 1;
        }
        self.taken.insert(reel.clone(), key.clone());
        self.by_key.insert(key, reel.clone());
        reel
    }
}

/// Write `cut`, returning what the EDL could not hold.
pub fn write(cut: &Cut, out: &mut dyn Write) -> io::Result<Vec<String>> {
    let mut skipped = Vec::new();
    let rate = cut.rate;
    let drop = timecode::is_drop_rate(rate);
    let offset = timecode::parse("01:00:00:00", rate, drop).unwrap_or(0);
    let tc = |frames: i64| timecode::format(frames, rate, drop);
    writeln!(out, "TITLE: {}", one_line(&cut.name))?;
    let fcm = if drop { "DROP FRAME" } else { "NON-DROP FRAME" };
    writeln!(out, "FCM: {fcm}")?;
    writeln!(out)?;
    // An empty lane takes no slot: the picture is the first lane that holds
    // events, and the channels go to the first four sound lanes that do.
    let held = |kind: LaneKind| {
        cut.lanes
            .iter()
            .filter(move |lane| lane.kind == kind && !lane.events.is_empty())
    };
    let mut video = held(LaneKind::Video);
    let picture = video.next();
    for lane in video {
        skipped.push(format!("{}: an EDL holds one video track", lane.name));
    }
    let mut audio = held(LaneKind::Audio);
    let channels: Vec<(&Lane, Option<&str>)> = picture
        .map(|lane| (lane, None))
        .into_iter()
        .chain(
            audio
                .by_ref()
                .take(AUDIO_CHANNELS.len())
                .zip(AUDIO_CHANNELS)
                .map(|(lane, chan)| (lane, Some(chan))),
        )
        .collect();
    for lane in audio {
        skipped.push(format!("{}: an EDL holds four audio channels", lane.name));
    }
    let mut reels = Reels::default();
    let mut number = 0_usize;
    for (lane, channel) in channels {
        let mut previous: Option<&Event> = None;
        for event in &lane.events {
            number += 1;
            let reel = reels.reel(&event.source);
            let chan = channel.unwrap_or(if event.sound { "B" } else { "V" });
            let src_in = resample(event.source.start, rate).value;
            let src_out = src_in + event.duration;
            let rec_in = offset + event.start;
            let rec_out = offset + event.end();
            let from = previous.filter(|prev| prev.end() == event.start);
            if event.dissolve.is_some() && from.is_none() {
                skipped.push(format!(
                    "{} {}: a dissolve with no clip before it, written as a cut",
                    lane.name, event.name
                ));
            }
            match (event.dissolve, from) {
                (Some(frames), Some(prev)) => {
                    let prev_reel = reels.reel(&prev.source);
                    let prev_out = resample(prev.source.start, rate).value + prev.duration;
                    writeln!(
                        out,
                        "{number:03}  {prev_reel:<8} {chan:<5} C        {} {} {} {}",
                        tc(prev_out),
                        tc(prev_out),
                        tc(rec_in),
                        tc(rec_in)
                    )?;
                    writeln!(
                        out,
                        "{number:03}  {reel:<8} {chan:<5} D    {frames:03} {} {} {} {}",
                        tc(src_in),
                        tc(src_out),
                        tc(rec_in),
                        tc(rec_out)
                    )?;
                    speed(out, &reel, event, rate, &tc(src_in))?;
                    writeln!(out, "* FROM CLIP NAME: {}", one_line(&prev.name))?;
                    writeln!(out, "* TO CLIP NAME: {}", one_line(&event.name))?;
                }
                _ => {
                    writeln!(
                        out,
                        "{number:03}  {reel:<8} {chan:<5} C        {} {} {} {}",
                        tc(src_in),
                        tc(src_out),
                        tc(rec_in),
                        tc(rec_out)
                    )?;
                    speed(out, &reel, event, rate, &tc(src_in))?;
                    writeln!(out, "* FROM CLIP NAME: {}", one_line(&event.name))?;
                }
            }
            if let Some(path) = &event.source.path {
                writeln!(
                    out,
                    "* SOURCE FILE: {}",
                    one_line(&path.display().to_string())
                )?;
            }
            previous = Some(event);
        }
    }
    if number > MAX_EVENTS {
        skipped.push(format!(
            "{number} events: numbers past {MAX_EVENTS} have four digits, which strict CMX 3600 readers refuse"
        ));
    }
    let mut notes = 0;
    for mark in &cut.marks {
        let color = avid_color_name(mark.color);
        writeln!(
            out,
            "* LOC: {} {color:<7} {}",
            tc(offset + mark.at),
            one_line(&mark.name)
        )?;
        notes += usize::from(!mark.comment.is_empty());
    }
    if notes > 0 {
        let plural = if notes == 1 { "" } else { "s" };
        skipped.push(format!(
            "{notes} marker note{plural}: an EDL marker holds a name only"
        ));
    }
    Ok(skipped)
}

/// The `M2` line for a retimed event.
fn speed(
    out: &mut dyn Write,
    reel: &str,
    event: &Event,
    rate: Rational,
    src_in: &str,
) -> io::Result<()> {
    if event.speed == Rational::new(1, 1) && !event.reversed {
        return Ok(());
    }
    let factor = f64::from(event.speed.num) / f64::from(event.speed.den.max(1));
    let fps = factor * rate.nominal_fps() as f64;
    let fps = if event.reversed { -fps } else { fps };
    writeln!(
        out,
        "M2   {reel:<8}       {fps:05.1}                {src_in}"
    )
}

#[cfg(test)]
#[path = "edl_tests.rs"]
mod tests;
