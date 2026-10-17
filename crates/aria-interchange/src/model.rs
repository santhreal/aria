//! A cut: one timeline as an interchange format describes it, apart from any
//! project. Readers produce one, writers consume one, and [`crate::import`]
//! and [`crate::project`] move it into and out of a project.

use std::path::PathBuf;

use aria_models::{MarkerColor, Rational, RationalTime};

/// One timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct Cut {
    pub name: String,
    /// Frames per second of every record-side position and length in the cut.
    pub rate: Rational,
    /// Frame width and height, when the format states them.
    pub size: Option<(u32, u32)>,
    /// Video lanes bottom to top, then audio lanes A1 to An.
    pub lanes: Vec<Lane>,
    pub marks: Vec<Mark>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneKind {
    Video,
    Audio,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lane {
    pub kind: LaneKind,
    pub name: String,
    /// In record order.
    pub events: Vec<Event>,
}

/// One clip on a lane.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub name: String,
    /// First record frame.
    pub start: i64,
    /// Record frames.
    pub duration: i64,
    pub source: Source,
    /// Source seconds per record second. Always positive.
    pub speed: Rational,
    /// The source window plays last frame first.
    pub reversed: bool,
    /// Record frames of a dissolve from the previous event on the lane into
    /// this one, centered on the cut between them.
    pub dissolve: Option<i64>,
    /// On a video lane, whether the file's own sound plays with the picture.
    /// Always true on an audio lane.
    pub sound: bool,
}

/// The file an event reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    /// The name the format files the source under: an EDL reel, an OTIO or
    /// FCPXML clip name, or the file stem.
    pub reel: String,
    /// The file as the format wrote it, absolute or relative to the file read.
    pub path: Option<PathBuf>,
    /// The first source frame of the window, counted from the start of the
    /// file.
    pub start: RationalTime,
}

/// One marker on the ruler.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    /// Record frame.
    pub at: i64,
    pub name: String,
    pub comment: String,
    pub color: Option<MarkerColor>,
}

/// A cut as a reader produced it, with what the file held that the cut
/// cannot.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub cut: Cut,
    pub skipped: Vec<String>,
}

impl Event {
    /// Record frame after the last one.
    pub fn end(&self) -> i64 {
        self.start.saturating_add(self.duration)
    }

    /// What identifies the source window: the reel, the file, and its head.
    fn source_key(&self) -> (&str, Option<&PathBuf>, RationalTime) {
        (
            &self.source.reel,
            self.source.path.as_ref(),
            self.source.start,
        )
    }

    /// Whether `other` plays the same source window over the same record
    /// frames: the picture's own sound written as a separate audio clip.
    fn same_take(&self, other: &Event) -> bool {
        self.start == other.start
            && self.duration == other.duration
            && self.speed == other.speed
            && self.reversed == other.reversed
            && self.source_key() == other.source_key()
    }
}

impl Cut {
    /// Fold every audio event that repeats a video event's take into that
    /// video event's sound, and drop audio lanes left empty. Formats that
    /// write sound apart from picture produce such pairs; a clip plays every
    /// channel of its file, so a second channel of the same take folds in too
    /// rather than playing the sound twice.
    pub fn merge_picture_sound(&mut self) {
        let (mut video, audio): (Vec<&mut Lane>, Vec<&mut Lane>) = self
            .lanes
            .iter_mut()
            .partition(|lane| lane.kind == LaneKind::Video);
        for lane in audio {
            lane.events.retain(|sound| {
                let take = video
                    .iter_mut()
                    .flat_map(|lane| lane.events.iter_mut())
                    .find(|picture| picture.same_take(sound));
                match take {
                    Some(picture) => {
                        picture.sound = true;
                        false
                    }
                    None => true,
                }
            });
        }
        self.lanes
            .retain(|lane| lane.kind == LaneKind::Video || !lane.events.is_empty());
    }

    /// The inverse of [`merge_picture_sound`](Self::merge_picture_sound):
    /// every video event with sound also as an audio event, on one audio lane
    /// per video lane, placed before the cut's own audio lanes. The video
    /// events keep `sound` set; a format that writes sound apart from picture
    /// writes these lanes and ignores the flag.
    pub fn split_picture_sound(&self) -> Vec<Lane> {
        self.lanes
            .iter()
            .filter(|lane| lane.kind == LaneKind::Video)
            .filter_map(|lane| {
                let events: Vec<Event> = lane
                    .events
                    .iter()
                    .filter(|event| event.sound)
                    .map(|event| Event {
                        dissolve: None,
                        ..event.clone()
                    })
                    .collect();
                (!events.is_empty()).then(|| Lane {
                    kind: LaneKind::Audio,
                    name: format!("{} sound", lane.name),
                    events,
                })
            })
            .collect()
    }

    /// The record frame after the last event or mark.
    pub fn end(&self) -> i64 {
        let events = self
            .lanes
            .iter()
            .flat_map(|lane| lane.events.iter().map(Event::end));
        let marks = self.marks.iter().map(|mark| mark.at.saturating_add(1));
        events.chain(marks).max().unwrap_or(0).max(0)
    }
}

/// `path`'s stem, or `fallback` when it has none.
pub(crate) fn file_stem(path: &std::path::Path, fallback: &str) -> String {
    path.file_stem().map_or_else(
        || fallback.to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}

/// An exact rate for `rate` frames a second: whole, NTSC, or to a
/// thousandth.
pub(crate) fn rational_of(rate: f64) -> Result<Rational, String> {
    if !(rate.is_finite() && rate > 0.0 && rate < 1.0e6) {
        return Err(format!("{rate} is not a frame rate"));
    }
    let whole = rate.round();
    if (rate - whole).abs() < 1.0e-6 {
        return Ok(Rational::new(whole as i32, 1));
    }
    let ntsc = (rate * 1.001).round();
    if (rate * 1.001 - ntsc).abs() < 1.0e-3 {
        return Ok(Rational::new(ntsc as i32 * 1000, 1001));
    }
    Ok(Rational::new((rate * 1000.0).round() as i32, 1000).reduced())
}

/// A playback speed to a thousandth, never below one thousandth.
pub(crate) fn speed_of(scalar: f64) -> Rational {
    Rational::new(((scalar * 1000.0).round() as i32).max(1), 1000).reduced()
}
