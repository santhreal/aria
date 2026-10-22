//! The open timeline as a [`Cut`], with what the cut cannot hold.

use aria_models::{
    AudioLaneLinks, Clip, ClipSource, MediaSource, Param, Project, Timeline, Track, TrackKind,
    generator_spec,
};

use crate::model::{Cut, Event, Lane, LaneKind, Mark, Read, Source, file_stem};
use crate::timecode;

/// The catalog id of the transition a dissolve reads as and is written from.
pub(crate) const DISSOLVE: &str = "crossfade";

/// The active timeline of `project` as a cut. `skipped` holds one line per
/// lane, clip, transition or setting the cut cannot hold.
pub fn cut(project: &Project) -> Read {
    let timeline = project.timeline();
    let mut writer = Writer {
        project,
        timeline,
        links: timeline.audio_lane_links(),
        drop: timecode::is_drop_rate(timeline.frame_rate),
        skipped: Vec::new(),
    };
    let lanes = timeline
        .tracks_ordered()
        .filter_map(|track| writer.lane(track))
        .collect();
    let marks = timeline
        .markers()
        .iter()
        .map(|marker| Mark {
            at: marker.tick.value,
            name: marker.name.clone(),
            comment: marker.comment.clone(),
            color: Some(marker.color),
        })
        .collect();
    let (width, height) = aria_render::canvas_size(project);
    Read {
        cut: Cut {
            name: project.name.clone(),
            rate: timeline.frame_rate,
            size: Some((width, height)),
            lanes,
            marks,
        },
        skipped: writer.skipped,
    }
}

struct Writer<'a> {
    project: &'a Project,
    timeline: &'a Timeline,
    /// Which clips had their sound detached, read once for every clip.
    links: AudioLaneLinks,
    drop: bool,
    skipped: Vec<String>,
}

impl Writer<'_> {
    fn lane(&mut self, track: &Track) -> Option<Lane> {
        let kind = match track.kind {
            TrackKind::Video => LaneKind::Video,
            TrackKind::Audio => LaneKind::Audio,
            other => {
                if !track.is_empty() {
                    self.skipped.push(format!(
                        "{}: {} lanes are not written",
                        track.name,
                        other.label()
                    ));
                }
                return None;
            }
        };
        if kind == LaneKind::Video && !track.enabled {
            self.skipped
                .push(format!("{}: a hidden lane, written as shown", track.name));
        }
        if kind == LaneKind::Audio && track.muted {
            self.skipped
                .push(format!("{}: a muted lane, written as audible", track.name));
        }
        let events = track
            .clips_ordered()
            .filter_map(|clip| self.event(track, kind, clip))
            .collect();
        Some(Lane {
            kind,
            name: track.name.clone(),
            events,
        })
    }

    fn event(&mut self, track: &Track, kind: LaneKind, clip: &Clip) -> Option<Event> {
        let at = || {
            timecode::format(
                clip.timeline.start.value,
                self.timeline.frame_rate,
                self.drop,
            )
        };
        let (media_id, window) = match &clip.content {
            ClipSource::Media { media, source } => (*media, *source),
            ClipSource::Generated(generator) => {
                let label = generator_spec(generator).label;
                self.skipped.push(format!(
                    "{} {label} at {}: generated clips are not written",
                    track.name,
                    at()
                ));
                return None;
            }
            ClipSource::Multicam { .. } => {
                self.skipped.push(format!(
                    "{} at {}: multicam clips are not written",
                    track.name,
                    at()
                ));
                return None;
            }
            ClipSource::Sequence { .. } => {
                self.skipped.push(format!(
                    "{} at {}: nested sequences are not written",
                    track.name,
                    at()
                ));
                return None;
            }
        };
        let Some(media) = self.project.media(media_id) else {
            self.skipped.push(format!(
                "{} at {}: the clip's media is not in the project",
                track.name,
                at()
            ));
            return None;
        };
        let name = clip
            .name
            .clone()
            .unwrap_or_else(|| media.display_name().to_owned());
        if clip.freeze_frame {
            self.skipped.push(format!(
                "{} {name} at {}: freeze frames are not written",
                track.name,
                at()
            ));
            return None;
        }
        // A subclip's window counts from the start of its range in the file.
        let start = match &media.source_range {
            Some(range) => aria_models::RationalTime::new(
                range.start.value.saturating_add(window.start.value),
                window.start.rate,
            ),
            None => window.start,
        };
        let sound = match kind {
            LaneKind::Audio => true,
            LaneKind::Video => {
                media.has_audio
                    && self.links.carries_own_audio(track, clip)
                    && clip.volume.constant() != Some(0.0)
            }
        };
        let lost = lost(clip, kind, sound);
        if !lost.is_empty() {
            self.skipped.push(format!(
                "{} {name} at {}: {} not written",
                track.name,
                at(),
                lost.join(", ")
            ));
        }
        let dissolve = self.dissolve(track, clip, &name);
        Some(Event {
            name,
            start: clip.timeline.start.value,
            duration: clip.timeline.duration.value,
            source: source_of(media, start),
            speed: clip.speed,
            reversed: clip.reversed,
            dissolve,
            sound,
        })
    }

    /// The length of the transition into `clip`, if one leads into it.
    fn dissolve(&mut self, track: &Track, clip: &Clip, name: &str) -> Option<i64> {
        let transition = track
            .transitions()
            .iter()
            .find(|transition| transition.right == clip.id)?;
        if transition.transition_id != DISSOLVE {
            let label = aria_models::transition_spec(&transition.transition_id)
                .map(|spec| spec.label.clone())
                .or_else(|| {
                    aria_models::AudioTransition::from_id(&transition.transition_id)
                        .map(|audio| audio.label().to_owned())
                })
                .unwrap_or_else(|| transition.transition_id.clone());
            self.skipped.push(format!(
                "{} {name}: the {label} transition is written as a dissolve",
                track.name
            ));
        }
        Some(transition.duration)
    }
}

fn source_of(media: &MediaSource, start: aria_models::RationalTime) -> Source {
    Source {
        reel: file_stem(media.path(), "untitled"),
        path: Some(media.path.clone()),
        start,
    }
}

/// What of `clip` an event cannot hold, by what a viewer would call it.
fn lost(clip: &Clip, kind: LaneKind, sound: bool) -> Vec<&'static str> {
    let visual = kind == LaneKind::Video;
    let effects = !clip.effects.is_empty()
        || !clip.styles.is_empty()
        || !clip.masks.is_empty()
        || clip.chroma_key.is_some()
        || clip.matte.is_some()
        || clip.stabilize.is_some()
        || clip.filter.is_some()
        || clip.lut.is_some()
        || !clip.adjust.is_neutral()
        || clip.animation_in.is_some()
        || clip.animation_out.is_some()
        || clip.animation_combo.is_some()
        || !clip.blend_mode.is_normal()
        || !clip.motion_blur.is_default();
    let framing = !clip.transform.is_identity()
        || !matches!(&clip.crop, Param::Constant(crop) if crop.is_full())
        || clip.flip_h
        || clip.flip_v;
    // A silenced picture is carried as a picture without sound.
    let volume = match clip.volume.constant() {
        Some(volume) => volume != 1.0 && !(visual && volume == 0.0),
        None => true,
    };
    let audio = volume
        || clip.pan.constant() != Some(0.0)
        || clip.fade_in != 0
        || clip.fade_out != 0
        || !clip.audio_effects.is_empty()
        || clip.denoise;
    [
        (visual && effects, "effects"),
        (visual && framing, "position, scale and crop"),
        (sound && audio, "volume, pan, fades and audio effects"),
        (
            clip.has_speed_curve(),
            "the speed ramp (written at its base speed)",
        ),
    ]
    .into_iter()
    .filter_map(|(lost, what)| lost.then_some(what))
    .collect()
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
