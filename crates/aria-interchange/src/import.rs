//! Placing a cut in the open project: each lane of the cut becomes new
//! tracks, each event a clip on one, reading the file its source names.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use aria_commands::{EditOutcome, ProjectCommand};
use aria_models::{
    CanvasSettings, ClipId, MAX_SPEED, MIN_SPEED, MediaId, MediaSource, Project, Rational,
    RationalTime, TimeRange, TrackId, TrackKind, rate_eq, resample, retimed_duration,
};
use aria_plugin::{ApplyOutcome, Command, EditCommand, TransactError, Transaction};

use crate::model::{Cut, Event, Lane, LaneKind, Mark, file_stem, speed_of};
use crate::project::DISSOLVE;
use crate::timecode;

/// What placing a clip needs of one file in the pool.
struct Media {
    id: MediaId,
    rate: Rational,
    /// Source ticks the file holds; `None` for a still, which fits any window.
    length: Option<i64>,
    has_audio: bool,
    /// The file name, and its stem: names an event needs no clip name for.
    name: String,
    stem: String,
}

impl Media {
    fn of(media: &MediaSource) -> Self {
        Self {
            id: media.id,
            rate: media.frame_rate,
            length: (!media.is_image).then_some(media.duration.value),
            has_audio: media.has_audio,
            name: media.display_name().to_owned(),
            stem: file_stem(media.path(), ""),
        }
    }
}

/// How a source is told apart: by its file, or by its reel when it names none.
#[derive(Hash, PartialEq, Eq)]
enum Key {
    Path(PathBuf),
    Reel(String),
}

/// Where each event's file is: an index into the pool, or the line that
/// reports it missing.
type Found = Result<usize, usize>;

/// One track a lane fills. Events of a lane that overlap land on tracks of
/// their own.
struct Row {
    track: TrackId,
    /// The last clip placed and the timeline frame after it; `None` while
    /// the engine has refused every clip offered to the track.
    last: Option<(ClipId, i64)>,
}

impl Row {
    /// Whether a clip starting at timeline frame `start` fits after the last.
    fn free(&self, start: i64) -> bool {
        self.last.is_none_or(|(_, end)| end <= start)
    }
}

/// What of an event its file plays.
struct Fit {
    speed: Rational,
    /// First source tick.
    first: i64,
    /// Timeline frames.
    duration: i64,
}

/// One clip to place.
struct Placement<'p> {
    /// Where the clip is, as a report line begins.
    place: &'p str,
    track: TrackId,
    kind: LaneKind,
    /// First timeline frame.
    start: i64,
}

/// Place `cut` in the project through `tx`, reading relative source paths
/// against `base`. Returns one line per event, mark or setting not placed as
/// the file describes it.
pub fn place(
    cut: &Cut,
    base: &Path,
    tx: &mut Transaction<'_>,
) -> Result<Vec<String>, TransactError> {
    let mut placer = Placer {
        tx,
        from: cut.rate,
        rate: cut.rate,
        drop: timecode::is_drop_rate(cut.rate),
        skipped: Vec::new(),
    };
    placer.adopt(cut)?;
    let (pool, found) = placer.media(cut, base)?;
    for (lane, found) in cut.lanes.iter().zip(&found) {
        placer.lane(lane, &pool, found)?;
    }
    for mark in &cut.marks {
        placer.mark(mark)?;
    }
    Ok(placer.skipped)
}

struct Placer<'t, 'a> {
    tx: &'t mut Transaction<'a>,
    /// The cut's rate.
    from: Rational,
    /// The timeline's rate.
    rate: Rational,
    /// Whether report timecodes count drop-frame.
    drop: bool,
    skipped: Vec<String>,
}

impl Placer<'_, '_> {
    /// Apply `command`. An engine rejection or a refusal is reported against
    /// `place`, and placing goes on.
    fn soft(
        &mut self,
        place: &str,
        command: Command,
    ) -> Result<Option<ApplyOutcome>, TransactError> {
        match self.tx.apply(command) {
            Ok(outcome) => Ok(Some(outcome)),
            Err(err @ (TransactError::Failed { .. } | TransactError::Refused { .. })) => {
                self.skipped.push(format!("{place}: {err}"));
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    /// `frames` of the cut as timeline frames.
    fn at(&self, frames: i64) -> i64 {
        if rate_eq(self.from, self.rate) {
            frames
        } else {
            resample(RationalTime::new(frames, self.from), self.rate).value
        }
    }

    /// `frames` of the cut as the file's timecode, for a report line.
    fn timecode(&self, frames: i64) -> String {
        timecode::format(frames, self.from, self.drop)
    }

    /// An empty timeline takes the cut's rate and frame size. One that holds
    /// clips keeps its own, and the cut is converted to its rate.
    fn adopt(&mut self, cut: &Cut) -> Result<(), TransactError> {
        let (rate, empty, canvas, size) = {
            let project = self.tx.project();
            let timeline = project.timeline();
            (
                timeline.frame_rate,
                timeline.clip_count() == 0,
                timeline.canvas(),
                aria_render::canvas_size(&project),
            )
        };
        self.rate = rate;
        if !empty {
            if !rate_eq(rate, cut.rate) {
                self.skipped.push(format!(
                    "the file runs at {} fps; its cut is placed at the timeline's {} fps",
                    fps(cut.rate),
                    fps(rate)
                ));
            }
            return Ok(());
        }
        if !rate_eq(rate, cut.rate) {
            let command = Command::Edit(EditCommand::SetSequenceRate {
                frame_rate: cut.rate,
            });
            if self.soft("the timeline rate", command)?.is_some() {
                self.rate = self.tx.project().timeline().frame_rate;
            }
        }
        let Some((width, height)) = cut.size else {
            return Ok(());
        };
        if (width, height) == size {
            return Ok(());
        }
        if !CanvasSettings::resolution_is_valid([width, height]) {
            self.skipped.push(format!(
                "the file's {width}x{height} frame is not a canvas size; the canvas is unchanged"
            ));
            return Ok(());
        }
        let command = Command::Edit(EditCommand::SetCanvas {
            aspect: canvas.aspect,
            background: canvas.background,
            resolution: Some([width, height]),
        });
        self.soft("the canvas", command)?;
        Ok(())
    }

    /// The pool file each event reads, lane by lane, importing each file
    /// once. A source with no file is reported once, with how many events it
    /// leaves out.
    fn media(
        &mut self,
        cut: &Cut,
        base: &Path,
    ) -> Result<(Vec<Media>, Vec<Vec<Found>>), TransactError> {
        let mut known: HashMap<Key, Found> = HashMap::new();
        let mut missing: Vec<(String, usize)> = Vec::new();
        let mut pool: Vec<Media> = Vec::new();
        let mut lanes = Vec::with_capacity(cut.lanes.len());
        for lane in &cut.lanes {
            let mut events = Vec::with_capacity(lane.events.len());
            for event in &lane.events {
                let source = &event.source;
                let key = match &source.path {
                    Some(path) if path.is_absolute() => Key::Path(path.clone()),
                    Some(path) => Key::Path(base.join(path)),
                    None => Key::Reel(source.reel.clone()),
                };
                let found = match known.entry(key) {
                    Entry::Occupied(entry) => *entry.get(),
                    Entry::Vacant(entry) => {
                        let media = match entry.key() {
                            Key::Path(path) => self.file(&source.reel, path, base)?,
                            Key::Reel(reel) => self.reel(reel),
                        };
                        let found = match media {
                            Ok(media) => {
                                pool.push(media);
                                Ok(pool.len() - 1)
                            }
                            Err(reason) => {
                                missing.push((reason, 0));
                                Err(missing.len() - 1)
                            }
                        };
                        *entry.insert(found)
                    }
                };
                if let Err(line) = found {
                    missing[line].1 += 1;
                }
                events.push(found);
            }
            lanes.push(events);
        }
        for (reason, events) in missing {
            let plural = if events == 1 { "" } else { "s" };
            self.skipped
                .push(format!("{reason}; {events} event{plural} not placed"));
        }
        Ok((pool, lanes))
    }

    /// The pool file at `path`, imported when the pool does not hold it. A
    /// file that is not at `path` is looked for beside the timeline file.
    fn file(
        &mut self,
        reel: &str,
        path: &Path,
        base: &Path,
    ) -> Result<Result<Media, String>, TransactError> {
        let beside = path
            .file_name()
            .map(|name| base.join(name))
            .filter(|beside| beside != path);
        for candidate in std::iter::once(path).chain(beside.as_deref()) {
            if let Some(media) = pooled(&self.tx.project(), candidate) {
                return Ok(Ok(media));
            }
            if candidate.is_file() {
                return self.import(reel, candidate);
            }
        }
        Ok(Err(format!("{reel}: no file at {}", path.display())))
    }

    fn import(&mut self, reel: &str, path: &Path) -> Result<Result<Media, String>, TransactError> {
        let command = Command::Project(ProjectCommand::Import {
            path: path.to_path_buf(),
        });
        let media = match self.tx.apply(command) {
            Ok(ApplyOutcome::Imported { media }) => media,
            Ok(other) => {
                return Ok(Err(format!(
                    "{reel}: importing {} produced {other:?}",
                    path.display()
                )));
            }
            Err(err @ (TransactError::Failed { .. } | TransactError::Refused { .. })) => {
                return Ok(Err(format!("{reel}: {err}")));
            }
            Err(err) => return Err(err),
        };
        let project = self.tx.project();
        Ok(project
            .media(media)
            .map(Media::of)
            .ok_or_else(|| format!("{reel}: {} left no file in the pool", path.display())))
    }

    /// The pool file whose name, less its extension, is `reel`.
    fn reel(&self, reel: &str) -> Result<Media, String> {
        let project = self.tx.project();
        project
            .media_iter()
            .find(|media| {
                !media.is_subclip() && media.path().file_stem() == Some(OsStr::new(reel))
            })
            .map(Media::of)
            .ok_or_else(|| {
                format!("{reel}: the file names no media for this reel, and no file in the pool is named {reel}")
            })
    }

    fn lane(&mut self, lane: &Lane, pool: &[Media], found: &[Found]) -> Result<(), TransactError> {
        let mut order: Vec<usize> = (0..lane.events.len()).collect();
        order.sort_by_key(|&index| lane.events[index].start);
        let mut rows: Vec<Row> = Vec::new();
        for index in order {
            let Ok(slot) = found[index] else {
                continue;
            };
            let media = &pool[slot];
            let event = &lane.events[index];
            let place = format!(
                "{} {} at {}",
                lane.name,
                event.name,
                self.timecode(event.start)
            );
            let start = self.at(event.start);
            let end = self.at(event.end());
            if start < 0 {
                self.skipped
                    .push(format!("{place}: before the start of the timeline"));
                continue;
            }
            if end <= start {
                self.skipped.push(format!(
                    "{place}: shorter than a frame at the timeline rate"
                ));
                continue;
            }
            // Fit before taking a track, so an event its file cannot play
            // leaves no empty track behind.
            let Some(fit) = self.fit(&place, event, media, end - start) else {
                continue;
            };
            let row = rows.iter().position(|row| row.free(start));
            let track = match row {
                Some(row) => rows[row].track,
                None => self.track(lane, rows.len())?,
            };
            let placement = Placement {
                place: &place,
                track,
                kind: lane.kind,
                start,
            };
            let placed = self.clip(&placement, &fit, event, media)?;
            let before = row.and_then(|row| rows[row].last);
            match (row, placed) {
                (Some(row), Some(_)) => rows[row].last = placed,
                (Some(_), None) => {}
                // A track the engine refused this clip on takes the next one.
                (None, _) => rows.push(Row {
                    track,
                    last: placed,
                }),
            }
            if placed.is_none() {
                continue;
            }
            let Some(frames) = event.dissolve else {
                continue;
            };
            match (before, lane.kind) {
                (Some((left, left_end)), LaneKind::Video) if left_end == start => {
                    self.dissolve(&place, left, frames)?;
                }
                (Some((_, left_end)), LaneKind::Audio) if left_end == start => {
                    self.skipped
                        .push(format!("{place}: an audio crossfade, placed as a cut"));
                }
                _ => self.skipped.push(format!(
                    "{place}: a dissolve with no clip before it, placed as a cut"
                )),
            }
        }
        Ok(())
    }

    /// A new track for row `row` of `lane`: the lane's own name, numbered
    /// past the first.
    fn track(&mut self, lane: &Lane, row: usize) -> Result<TrackId, TransactError> {
        let name = match row {
            0 => lane.name.clone(),
            row => format!("{} ({})", lane.name, row + 1),
        };
        let kind = match lane.kind {
            LaneKind::Video => TrackKind::Video,
            LaneKind::Audio => TrackKind::Audio,
        };
        let command = Command::Edit(EditCommand::AddTrack {
            kind,
            name,
            index: None,
            pinned: false,
        });
        match self.tx.apply(command)? {
            ApplyOutcome::Edited(EditOutcome::CreatedTrack(track)) => Ok(track),
            other => Err(TransactError::Aborted(format!(
                "adding track {} produced {other:?}",
                lane.name
            ))),
        }
    }

    /// What of `event` its file plays over `duration` timeline frames, at a
    /// speed clips play at. `None`, reported, when it plays none of it.
    fn fit(&mut self, place: &str, event: &Event, media: &Media, duration: i64) -> Option<Fit> {
        let speed = self.speed(place, event.speed);
        let first = resample(event.source.start, media.rate).value;
        if first < 0 {
            self.skipped.push(format!(
                "{place}: its source starts before the start of {}",
                media.name
            ));
            return None;
        }
        let Some(length) = media.length else {
            return Some(Fit {
                speed,
                first,
                duration,
            });
        };
        let room = length - first;
        if room <= 0 {
            self.skipped.push(format!(
                "{place}: its source starts after the end of {}",
                media.name
            ));
            return None;
        }
        if self.window(media, speed, duration) <= room {
            return Some(Fit {
                speed,
                first,
                duration,
            });
        }
        let unplayed = room.saturating_mul(i64::from(speed.den)) / i64::from(speed.num);
        let mut fit = resample(RationalTime::new(unplayed, media.rate), self.rate)
            .value
            .min(duration);
        while fit > 0 && self.window(media, speed, fit) > room {
            fit -= 1;
        }
        if fit <= 0 {
            self.skipped.push(format!(
                "{place}: {} holds less than a frame of it",
                media.name
            ));
            return None;
        }
        self.skipped.push(format!(
            "{place}: {} ends {} frames early; the clip is shortened",
            media.name,
            duration - fit
        ));
        Some(Fit {
            speed,
            first,
            duration: fit,
        })
    }

    /// Source ticks of `media` that `frames` of timeline play at `speed`, as
    /// the engine counts them when it trims a retimed clip.
    fn window(&self, media: &Media, speed: Rational, frames: i64) -> i64 {
        let ticks = resample(RationalTime::new(frames, self.rate), media.rate).value;
        (ticks.saturating_mul(i64::from(speed.num)) / i64::from(speed.den)).max(1)
    }

    /// Place one fitted event, returning the clip and the timeline frame
    /// after it.
    fn clip(
        &mut self,
        at: &Placement<'_>,
        fit: &Fit,
        event: &Event,
        media: &Media,
    ) -> Result<Option<(ClipId, i64)>, TransactError> {
        let place = at.place;
        let rate = self.rate;
        let speed = fit.speed;
        let mut duration = fit.duration;
        let first = fit.first;
        let ticks = self.window(media, speed, duration);
        let command = Command::Edit(EditCommand::AddClip {
            track: at.track,
            media: media.id,
            source: TimeRange::at_rate(first, ticks, media.rate),
            start: RationalTime::new(at.start, rate),
        });
        let Some(ApplyOutcome::Edited(EditOutcome::Created(clip))) = self.soft(place, command)?
        else {
            return Ok(None);
        };
        let mut placed = resample(RationalTime::new(ticks, media.rate), rate)
            .value
            .max(1);
        if speed != Rational::new(1, 1) || event.reversed {
            let command = Command::Edit(EditCommand::SetClipSpeed {
                clip,
                speed,
                reversed: event.reversed,
            });
            if self.soft(place, command)?.is_some() {
                placed = retimed_duration(placed, speed, 1.0, false);
            }
        }
        if placed != duration {
            let command = Command::Edit(EditCommand::TrimClip {
                clip,
                timeline: TimeRange::at_rate(at.start, duration, rate),
            });
            if self.soft(place, command)?.is_none() {
                duration = placed;
            }
        }
        if at.kind == LaneKind::Video && !event.sound && media.has_audio {
            let silent = RationalTime::new(0, rate);
            let command = Command::Edit(EditCommand::SetClipAudio {
                clip,
                volume: Some(0.0),
                fade_in: silent,
                fade_out: silent,
            });
            self.soft(place, command)?;
        }
        if !event.name.is_empty() && event.name != media.name && event.name != media.stem {
            let command = Command::Edit(EditCommand::SetClipName {
                clip,
                name: Some(event.name.clone()),
            });
            self.soft(place, command)?;
        }
        Ok(Some((clip, at.start + duration)))
    }

    /// `speed`, held within what the engine plays.
    fn speed(&mut self, place: &str, speed: Rational) -> Rational {
        let value = speed.as_f64();
        let (bound, word) = if value < f64::from(MIN_SPEED) {
            (MIN_SPEED, "slower")
        } else if value > f64::from(MAX_SPEED) {
            (MAX_SPEED, "faster")
        } else {
            return speed;
        };
        self.skipped.push(format!(
            "{place}: {value}x is {word} than clips play; placed at {bound}x"
        ));
        speed_of(f64::from(bound))
    }

    /// A dissolve of `frames` cut frames over the cut after `left`.
    fn dissolve(&mut self, place: &str, left: ClipId, frames: i64) -> Result<(), TransactError> {
        let command = Command::Edit(EditCommand::AddTransition {
            clip: left,
            transition_id: DISSOLVE.to_owned(),
        });
        if self.soft(place, command)?.is_none() {
            return Ok(());
        }
        let command = Command::Edit(EditCommand::SetTransition {
            clip: left,
            duration: self.at(frames).max(1),
        });
        self.soft(place, command)?;
        Ok(())
    }

    fn mark(&mut self, mark: &Mark) -> Result<(), TransactError> {
        let place = format!("marker {} at {}", mark.name, self.timecode(mark.at));
        let at = self.at(mark.at);
        if at < 0 {
            self.skipped
                .push(format!("{place}: before the start of the timeline"));
            return Ok(());
        }
        let at = RationalTime::new(at, self.rate);
        let command = Command::Edit(EditCommand::AddMarker {
            at,
            name: mark.name.clone(),
            color: mark.color,
        });
        let Some(ApplyOutcome::Edited(EditOutcome::CreatedMarker(marker))) =
            self.soft(&place, command)?
        else {
            return Ok(());
        };
        if mark.comment.is_empty() {
            return Ok(());
        }
        let color = match mark.color {
            Some(color) => Some(color),
            None => self
                .tx
                .project()
                .timeline()
                .marker(marker)
                .map(|marker| marker.color),
        };
        let Some(color) = color else {
            return Ok(());
        };
        let command = Command::Edit(EditCommand::SetMarker {
            marker,
            at,
            name: mark.name.clone(),
            color,
            comment: mark.comment.clone(),
        });
        self.soft(&place, command)?;
        Ok(())
    }
}

/// The pool file at `path`. A subclip reads as the file it was cut from, so
/// source frames count from the start of the file.
fn pooled(project: &Project, path: &Path) -> Option<Media> {
    let id = project.find_media_by_path(path)?;
    let media = project.media(id)?;
    let media = match media.parent {
        Some(parent) => project.media(parent)?,
        None => media,
    };
    Some(Media::of(media))
}

fn fps(rate: Rational) -> String {
    let value = (rate.as_f64() * 1000.0).round() / 1000.0;
    format!("{value}")
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
