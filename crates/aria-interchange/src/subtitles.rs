//! Subtitles: SubRip (`.srt`) and WebVTT (`.vtt`) cues read onto new text
//! tracks as text clips, and the text clips on the open timeline's text
//! tracks written as cues.
//!
//! Cue times are milliseconds. A cue lands on the nearest frame of the open
//! timeline and lasts at least one frame. Cues that overlap go to further
//! tracks, so none is shortened. Markup is removed: tags, `{\...}` overrides,
//! and WebVTT cue settings; the report counts the cues that held any.

use std::borrow::Cow;
use std::io::{self, Write};

use aria_commands::EditOutcome;
use aria_core::count::counted;
use aria_models::{
    ClipSource, Generator, Project, Rational, TextStyle, TimeRange, TrackId, TrackKind,
};
use aria_plugin::{ApplyOutcome, Command, EditCommand, TransactError, Transaction};

/// One cue: its span in milliseconds and its plain text, lines joined by
/// `\n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// The cues a file or a timeline held, with one line per item that did not
/// come through whole.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cues {
    pub cues: Vec<Cue>,
    pub skipped: Vec<String>,
}

/// One subtitle format: what it is called, and how it reads and writes.
#[derive(Debug)]
pub struct SubtitleFormat {
    /// The importer and exporter id.
    pub id: &'static str,
    /// The format as a menu lists it.
    pub name: &'static str,
    /// The extension read and written.
    pub extension: &'static str,
    pub read: fn(&str) -> Result<Cues, String>,
    pub write: fn(&[Cue], &mut dyn Write) -> io::Result<Vec<String>>,
}

/// Every subtitle format, in the order menus list them.
pub const SUBTITLE_FORMATS: [SubtitleFormat; 2] = [
    SubtitleFormat {
        id: "aria.interchange.srt",
        name: "SubRip subtitles",
        extension: "srt",
        read: read_srt,
        write: write_srt,
    },
    SubtitleFormat {
        id: "aria.interchange.vtt",
        name: "WebVTT subtitles",
        extension: "vtt",
        read: read_vtt,
        write: write_vtt,
    },
];

/// The most digits an hours field may hold: past it a time is not a cue's.
const HOUR_DIGITS: usize = 5;

/// Read SubRip text. A block without a timing line continues the cue before
/// it, as a blank line inside a cue's text reads.
pub fn read_srt(text: &str) -> Result<Cues, String> {
    let mut read = Reader::default();
    for (line, block) in blocks(text) {
        let Some(at) = block.iter().take(2).position(|l| l.contains("-->")) else {
            match read.cues.last_mut() {
                Some(cue) => {
                    let (more, marked) = payload(&block, false);
                    if !more.is_empty() {
                        cue.text.push('\n');
                        cue.text.push_str(&more);
                    }
                    read.styled += usize::from(marked);
                }
                None => read
                    .skipped
                    .push(format!("line {line}: no cue timing, text skipped")),
            }
            continue;
        };
        read.cue(line + at, block[at], &block[at + 1..], false);
    }
    read.finish()
}

/// Read WebVTT text. Comment, style and region blocks are passed over.
pub fn read_vtt(text: &str) -> Result<Cues, String> {
    let mut blocks = blocks(text);
    let Some((line, header)) = blocks.next() else {
        return Err("the file is empty".into());
    };
    if !keyword(header[0], "WEBVTT") {
        return Err("the file does not start with WEBVTT".into());
    }
    let mut read = Reader::default();
    if let Some(at) = header.iter().position(|l| l.contains("-->")) {
        read.skipped.push(format!(
            "line {}: a cue before the blank line that ends the header, skipped",
            line + at
        ));
    }
    let mut styles = 0usize;
    for (line, block) in blocks {
        let first = block[0];
        if keyword(first, "NOTE") {
            continue;
        }
        if keyword(first, "STYLE") || keyword(first, "REGION") {
            styles += 1;
            continue;
        }
        match block.iter().take(2).position(|l| l.contains("-->")) {
            Some(at) => read.cue(line + at, block[at], &block[at + 1..], true),
            None => read
                .skipped
                .push(format!("line {line}: no cue timing, text skipped")),
        }
    }
    if styles > 0 {
        read.skipped.push(format!(
            "{} dropped",
            counted(styles, "style or region block", "style or region blocks")
        ));
    }
    read.finish()
}

/// Write `cues` as SubRip, numbered from 1.
pub fn write_srt(cues: &[Cue], out: &mut dyn Write) -> io::Result<Vec<String>> {
    let mut blank = 0usize;
    for (index, cue) in cues.iter().enumerate() {
        writeln!(out, "{}", index + 1)?;
        writeln!(
            out,
            "{} --> {}",
            clock(cue.start_ms, ','),
            clock(cue.end_ms, ',')
        )?;
        blank += usize::from(write_lines(out, &cue.text, verbatim)?);
        writeln!(out)?;
    }
    Ok(blank_report(blank))
}

/// Write `cues` as WebVTT, with `&`, `<` and `>` escaped.
pub fn write_vtt(cues: &[Cue], out: &mut dyn Write) -> io::Result<Vec<String>> {
    writeln!(out, "WEBVTT")?;
    writeln!(out)?;
    let mut blank = 0usize;
    for cue in cues {
        writeln!(
            out,
            "{} --> {}",
            clock(cue.start_ms, '.'),
            clock(cue.end_ms, '.')
        )?;
        blank += usize::from(write_lines(out, &cue.text, escape_vtt)?);
        writeln!(out)?;
    }
    Ok(blank_report(blank))
}

/// The text clips on the shown text tracks of the open timeline, in start
/// order, with what a subtitle file cannot hold.
pub fn cues(project: &Project) -> Cues {
    let timeline = project.timeline();
    let mut cues = Vec::new();
    let mut skipped = Vec::new();
    let mut styled = 0usize;
    let default_style = TextStyle::default();
    for track in timeline.tracks_ordered() {
        if track.kind != TrackKind::Text {
            continue;
        }
        if !track.enabled {
            if !track.is_empty() {
                skipped.push(format!("{}: a hidden text track, not written", track.name));
            }
            continue;
        }
        for clip in track.clips_ordered() {
            let start_ms = millis(clip.timeline.start.value, clip.timeline.start.rate);
            let ClipSource::Generated(Generator::Text { content, style }) = &clip.content else {
                skipped.push(format!(
                    "{} at {}: only text clips are written",
                    track.name,
                    clock(start_ms, '.')
                ));
                continue;
            };
            if content.trim().is_empty() {
                skipped.push(format!(
                    "{} at {}: a text clip with no text",
                    track.name,
                    clock(start_ms, '.')
                ));
                continue;
            }
            styled += usize::from(*style != default_style);
            let end = clip
                .timeline
                .start
                .value
                .saturating_add(clip.timeline.duration.value);
            cues.push(Cue {
                start_ms,
                end_ms: millis(end, clip.timeline.start.rate),
                text: content.clone(),
            });
        }
    }
    cues.sort_by_key(|cue| (cue.start_ms, cue.end_ms));
    if styled > 0 {
        skipped.push(format!(
            "{}: font, color, size and position are not written",
            counted(styled, "text clip", "text clips")
        ));
    }
    Cues { cues, skipped }
}

/// Place `cues` on new text tracks named after `name`, one text clip per cue
/// at the open timeline's rate. A cue that overlaps the last clip on every
/// track so far starts another track.
pub fn place(cues: &[Cue], name: &str, tx: &mut Transaction<'_>) -> Result<(), TransactError> {
    let rate = tx.project().timeline().frame_rate;
    let mut order: Vec<&Cue> = cues.iter().collect();
    order.sort_by_key(|cue| (cue.start_ms, cue.end_ms));
    // Each track and the frame its last clip ends on.
    let mut rows: Vec<(TrackId, i64)> = Vec::new();
    for cue in order {
        let start = frames(cue.start_ms, rate);
        let end = frames(cue.end_ms, rate).max(start + 1);
        let row = match rows.iter().position(|(_, last)| *last <= start) {
            Some(row) => row,
            None => {
                let track = add_track(tx, name, rows.len())?;
                rows.push((track, 0));
                rows.len() - 1
            }
        };
        tx.apply(Command::Edit(EditCommand::AddGenerated {
            track: rows[row].0,
            generator: Generator::text(cue.text.clone()),
            timeline: TimeRange::at_rate(start, end - start, rate),
        }))?;
        rows[row].1 = end;
    }
    Ok(())
}

/// A new text track for row `row`: `name`, numbered past the first.
fn add_track(tx: &mut Transaction<'_>, name: &str, row: usize) -> Result<TrackId, TransactError> {
    let name = match row {
        0 => name.to_owned(),
        row => format!("{name} ({})", row + 1),
    };
    match tx.apply(Command::Edit(EditCommand::AddTrack {
        kind: TrackKind::Text,
        name: name.clone(),
        index: None,
        pinned: false,
    }))? {
        ApplyOutcome::Edited(EditOutcome::CreatedTrack(track)) => Ok(track),
        other => Err(TransactError::Aborted(format!(
            "adding track {name} produced {other:?}"
        ))),
    }
}

/// The cues read so far and what was dropped.
#[derive(Default)]
struct Reader {
    cues: Vec<Cue>,
    skipped: Vec<String>,
    /// Cues that held markup.
    styled: usize,
    /// WebVTT cues that held settings.
    settings: usize,
}

impl Reader {
    /// Read one cue from its timing line, at 1-based `line`, and its text.
    fn cue(&mut self, line: usize, timing_line: &str, text: &[&str], vtt: bool) {
        let Some((start_ms, end_ms, settings)) = timing(timing_line) else {
            self.skipped
                .push(format!("line {line}: {timing_line:?} is not a cue timing"));
            return;
        };
        if end_ms <= start_ms {
            self.skipped
                .push(format!("line {line}: the cue ends before it starts"));
            return;
        }
        let (text, marked) = payload(text, vtt);
        if text.is_empty() {
            self.skipped
                .push(format!("line {line}: the cue has no text"));
            return;
        }
        self.styled += usize::from(marked);
        self.settings += usize::from(vtt && settings);
        self.cues.push(Cue {
            start_ms,
            end_ms,
            text,
        });
    }

    fn finish(mut self) -> Result<Cues, String> {
        if self.cues.is_empty() {
            return Err(match self.skipped.first() {
                Some(first) => format!("no cue reads; {first}"),
                None => "the file holds no cues".into(),
            });
        }
        if self.styled > 0 {
            self.skipped.push(format!(
                "{}: styling tags dropped",
                counted(self.styled, "cue", "cues")
            ));
        }
        if self.settings > 0 {
            self.skipped.push(format!(
                "{}: position and alignment settings dropped",
                counted(self.settings, "cue", "cues")
            ));
        }
        Ok(Cues {
            cues: self.cues,
            skipped: self.skipped,
        })
    }
}

/// The runs of non-blank lines in `text`, each with the 1-based number of
/// its first line.
fn blocks(text: &str) -> impl Iterator<Item = (usize, Vec<&str>)> {
    let mut lines = text.lines().enumerate().peekable();
    std::iter::from_fn(move || {
        while lines.next_if(|(_, line)| line.trim().is_empty()).is_some() {}
        let (first, line) = lines.next()?;
        let mut block = vec![line.trim_end()];
        while let Some((_, line)) = lines.next_if(|(_, line)| !line.trim().is_empty()) {
            block.push(line.trim_end());
        }
        Some((first + 1, block))
    })
}

/// Whether `line` is `word` alone or followed by a space or a tab.
fn keyword(line: &str, word: &str) -> bool {
    line.strip_prefix(word)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
}

/// A timing line: start and end in milliseconds, and whether anything
/// follows the end time.
fn timing(line: &str) -> Option<(i64, i64, bool)> {
    let (start, rest) = line.split_once("-->")?;
    let mut rest = rest.split_whitespace();
    let end = rest.next()?;
    Some((
        timestamp(start.trim())?,
        timestamp(end)?,
        rest.next().is_some(),
    ))
}

/// `H:MM:SS,mmm`, `H:MM:SS.mmm` or `MM:SS.mmm` as milliseconds. The
/// fraction is decimal seconds of one to three digits.
fn timestamp(text: &str) -> Option<i64> {
    let (clock, fraction) = text.rsplit_once([',', '.'])?;
    if fraction.is_empty() || fraction.len() > 3 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let millis: i64 = format!("{fraction:0<3}").parse().ok()?;
    let mut fields = clock.rsplit(':');
    let seconds = field(fields.next()?, 2)?;
    let minutes = field(fields.next()?, 2)?;
    let hours = match fields.next() {
        Some(hours) => field(hours, HOUR_DIGITS)?,
        None => 0,
    };
    if fields.next().is_some() || minutes >= 60 || seconds >= 60 {
        return None;
    }
    Some(((hours * 60 + minutes) * 60 + seconds) * 1000 + millis)
}

/// A field of one to `digits` ASCII digits.
fn field(text: &str, digits: usize) -> Option<i64> {
    if text.is_empty() || text.len() > digits || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The plain text of a cue's lines, joined by `\n`, and whether any markup
/// was removed. WebVTT entities are decoded when `vtt` is set.
fn payload(lines: &[&str], vtt: bool) -> (String, bool) {
    let mut text = String::new();
    let mut marked = false;
    for line in lines {
        let start = text.len();
        if start > 0 {
            text.push('\n');
        }
        marked |= plain(line, vtt, &mut text);
        if text[start..].trim().is_empty() {
            text.truncate(start);
        }
    }
    (text.trim().to_owned(), marked)
}

/// Append `line` to `out` without `<...>` tags or `{\...}` overrides,
/// decoding entities when `vtt` is set. Returns whether anything was
/// removed. A `<` or `{` that opens nothing is kept.
fn plain(line: &str, vtt: bool, out: &mut String) -> bool {
    let mut marked = false;
    let mut rest = line;
    while let Some(at) = rest.find(['<', '{', '&']) {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let close = match tail.as_bytes()[0] {
            // A tag opens with a letter, a digit, or `/`: `a < b` is text.
            b'<' if tail[1..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '/') => {
                tail.find('>')
            }
            b'{' if tail.starts_with("{\\") => tail.find('}'),
            b'&' if vtt => match entity(tail) {
                Some((decoded, len)) => {
                    out.push(decoded);
                    rest = &tail[len..];
                    continue;
                }
                None => None,
            },
            _ => None,
        };
        match close {
            Some(close) => {
                marked = true;
                rest = &tail[close + 1..];
            }
            None => {
                out.push_str(&tail[..1]);
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    marked
}

/// The character a WebVTT entity at the start of `text` stands for, and the
/// entity's length.
fn entity(text: &str) -> Option<(char, usize)> {
    let end = text.find(';')?;
    let name = &text[1..end];
    let decoded = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "nbsp" => '\u{a0}',
        "lrm" => '\u{200e}',
        "rlm" => '\u{200f}',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some((decoded, end + 1))
}

/// Write each non-blank line of `text` through `escape`. Returns whether a
/// blank line was left out: a blank line would end the cue.
fn write_lines(
    out: &mut dyn Write,
    text: &str,
    escape: fn(&str) -> Cow<'_, str>,
) -> io::Result<bool> {
    let mut blank = false;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank = true;
            continue;
        }
        writeln!(out, "{}", escape(line))?;
    }
    Ok(blank)
}

fn blank_report(blank: usize) -> Vec<String> {
    match blank {
        0 => Vec::new(),
        blank => vec![format!(
            "{}: blank lines inside the text are not written",
            counted(blank, "cue", "cues")
        )],
    }
}

/// `line` as it is: SubRip reserves no characters.
fn verbatim(line: &str) -> Cow<'_, str> {
    Cow::Borrowed(line)
}

/// `line` with the characters WebVTT reserves escaped.
fn escape_vtt(line: &str) -> Cow<'_, str> {
    if !line.contains(['&', '<', '>']) {
        return line.into();
    }
    line.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .into()
}

/// `ms` as `HH:MM:SS<separator>mmm`.
fn clock(ms: i64, separator: char) -> String {
    let ms = ms.max(0);
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// `ms` milliseconds as the nearest whole frame at `rate`.
fn frames(ms: i64, rate: Rational) -> i64 {
    nearest(
        i128::from(ms) * i128::from(rate.num),
        1000 * i128::from(rate.den),
    )
}

/// `frames` at `rate` as the nearest millisecond.
fn millis(frames: i64, rate: Rational) -> i64 {
    nearest(
        i128::from(frames) * 1000 * i128::from(rate.den),
        i128::from(rate.num),
    )
}

/// `num / den` rounded half away from zero, for a positive `den`.
fn nearest(num: i128, den: i128) -> i64 {
    let rounded = if num >= 0 {
        (2 * num + den) / (2 * den)
    } else {
        -((-2 * num + den) / (2 * den))
    };
    i64::try_from(rounded).unwrap_or(if rounded < 0 { i64::MIN } else { i64::MAX })
}

#[cfg(test)]
#[path = "subtitles_tests.rs"]
mod tests;
