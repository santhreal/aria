//! Media: import, relink, dropped paths, generators, placement, and how the bin
//! reads the pool.
//!
//! The filter, the order, and the properties rows live here rather than in the
//! panel: the panel draws a list it does not compute, so the match count cannot
//! disagree with the rows under it.

use std::path::PathBuf;

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_engine::{ApplyOutcome, EngineError};
use aria_models::{Generator, MediaId, RationalTime, TimeRange, TrackId, TrackKind};

use super::AppState;
use super::source_patch::SourcePlacement;
use gpui::SharedString;

impl AppState {
    pub(crate) fn ensure_kind_track(&mut self, kind: TrackKind) -> Result<TrackId, EngineError> {
        let ui_kind = crate::types::UiTrackKind::from_model(kind);
        let nearest = |tracks: &[crate::types::UiTrack]| {
            let mut lanes = tracks.iter().filter(|track| track.kind == ui_kind);
            match kind.zone() {
                aria_models::LaneZone::Floor | aria_models::LaneZone::Bus => lanes.next(),
                aria_models::LaneZone::Main
                | aria_models::LaneZone::Stack
                | aria_models::LaneZone::Ceiling => lanes.next_back(),
            }
            .map(|track| TrackId::from_raw(track.id))
        };
        if let Some(id) = nearest(&self.store.sequence.tracks) {
            return Ok(id);
        }
        let name = kind.default_name();
        self.add_track(kind, name)?;
        nearest(&self.store.sequence.tracks)
            .ok_or_else(|| EngineError::Export("no track after add".into()))
    }

    /// The first unlocked lane of `kind` with nothing on `start..end`, or a new
    /// lane of that kind when every one holds a clip there.
    pub(crate) fn free_kind_track(
        &mut self,
        kind: TrackKind,
        start: i64,
        end: i64,
    ) -> Result<TrackId, EngineError> {
        let ui_kind = crate::types::UiTrackKind::from_model(kind);
        let free = self.store.sequence.tracks.iter().find(|lane| {
            lane.kind == ui_kind
                && !lane.locked
                && lane.clips.iter().all(|clip| {
                    clip.timeline_end_ticks <= start || clip.timeline_start_ticks >= end
                })
        });
        if let Some(lane) = free {
            return Ok(TrackId::from_raw(lane.id));
        }
        match self.add_track(kind, kind.default_name())? {
            ApplyOutcome::Edited(aria_commands::EditOutcome::CreatedTrack(id)) => Ok(id),
            other => Err(EngineError::Refused(format!(
                "adding a lane produced {other:?}, not a lane"
            ))),
        }
    }

    pub(crate) fn spans_on(&self, track: TrackId) -> Vec<(i64, i64)> {
        self.store
            .sequence
            .tracks
            .iter()
            .find(|item| item.id == track.raw())
            .map(|item| {
                item.clips
                    .iter()
                    .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn drop_target_track(
        &mut self,
        kind: TrackKind,
        track: Option<TrackId>,
    ) -> Result<TrackId, EngineError> {
        let ui_kind = crate::types::UiTrackKind::from_model(kind);
        let aimed = track.filter(|id| {
            self.store
                .sequence
                .tracks
                .iter()
                .any(|lane| lane.id == id.raw() && lane.kind == ui_kind)
        });
        if let Some(id) = aimed {
            return Ok(id);
        }
        if let Some(channel) = aria_models::SourceChannel::of_lane(kind)
            && let Some(id) = self.store.sequence.patched_lane(channel)
        {
            return Ok(TrackId::from_raw(id));
        }
        let mut matches = self
            .store
            .sequence
            .tracks
            .iter()
            .filter(|lane| lane.kind == ui_kind && lane.targeted);
        let targeted = match kind.zone() {
            aria_models::LaneZone::Floor | aria_models::LaneZone::Bus => matches.next(),
            aria_models::LaneZone::Main
            | aria_models::LaneZone::Stack
            | aria_models::LaneZone::Ceiling => matches.next_back(),
        }
        .map(|lane| TrackId::from_raw(lane.id));
        match targeted {
            Some(id) => Ok(id),
            None => self.ensure_kind_track(kind),
        }
    }

    /// Drop `media` at `at` on a new lane at the top or the bottom edge of the
    /// lanes, as one undo step with the lane.
    pub fn drop_media_on_new_lane(
        &mut self,
        media: MediaId,
        top: bool,
        at: i64,
    ) -> Result<(), EngineError> {
        let Some(placement) = self.media_placement(media, None, at) else {
            return Ok(());
        };
        self.place_on_new_lane(placement, top)?;
        Ok(())
    }

    /// Drop the open Source tab's marked range on a new lane at the top or the
    /// bottom edge of the lanes, covering unless `insert`, as one undo step
    /// with the lane.
    pub fn drop_source_on_new_lane(
        &mut self,
        top: bool,
        at: i64,
        insert: bool,
    ) -> Result<(), EngineError> {
        let Some(placement) = self.source_drop(None, at, insert) else {
            return Ok(());
        };
        let placed = self.place_on_new_lane(placement, top)?;
        self.source_dropped(placed, insert);
        Ok(())
    }

    /// Place `placement` on a new lane for its media at the top or the bottom
    /// edge of the lanes, as one `Source edit` undo step: Undo takes the lane
    /// back with the clips, and a placement that lands nothing leaves no lane.
    /// Returns whether anything landed.
    fn place_on_new_lane(
        &mut self,
        mut placement: SourcePlacement,
        top: bool,
    ) -> Result<bool, EngineError> {
        let Some(kind) = self.lane_kind_for(placement.media) else {
            return Ok(false);
        };
        let landed = self
            .edit_group(|state| {
                let lane = state.add_lane_of_kind(kind, edge_lane_index(kind, top))?;
                placement.aimed = Some(lane);
                let Some(lanes) = state.source_lanes(placement.media, placement.aimed)? else {
                    return Ok(None);
                };
                state.place_source_on(placement, lanes).map(Some)
            })
            .map(|(group, label)| match label {
                Some(label) => {
                    group.commit_as(label);
                    true
                }
                None => {
                    self.roll_back(group);
                    false
                }
            });
        self.sync_from_engine();
        landed
    }

    /// The kind of lane `media` lands on: audio for a sound file, video for
    /// anything with a picture. `None` when the media is not in the pool.
    fn lane_kind_for(&self, media: MediaId) -> Option<TrackKind> {
        let engine = self.engine.lock();
        let item = engine.project().media(media)?;
        Some(if item.kind() == aria_models::MediaKind::Audio {
            TrackKind::Audio
        } else {
            TrackKind::Video
        })
    }

    pub(crate) fn add_lane_of_kind(
        &mut self,
        kind: TrackKind,
        index: Option<usize>,
    ) -> Result<TrackId, EngineError> {
        let ui_kind = crate::types::UiTrackKind::from_model(kind);
        let existing = self
            .store
            .sequence
            .tracks
            .iter()
            .filter(|lane| lane.kind == ui_kind)
            .count();
        // The prefix is the lane kind's default name with its ordinal removed,
        // so one column sets both the first lane's name and every later one's.
        let prefix = kind.spec().name_stem();
        let name = format!("{prefix}{}", existing + 1);
        let outcome = self.apply_command(Command::Edit(EditCommand::AddTrack {
            kind,
            name,
            index,
            pinned: true,
        }))?;
        match outcome {
            ApplyOutcome::Edited(aria_commands::EditOutcome::CreatedTrack(id)) => Ok(id),
            _ => Err(EngineError::Export("no lane after add".into())),
        }
    }

    pub fn drop_media_on_timeline(
        &mut self,
        media: MediaId,
        track: Option<TrackId>,
        start_ticks: i64,
    ) -> Result<(), EngineError> {
        let Some(placement) = self.media_placement(media, track, start_ticks) else {
            return Ok(());
        };
        self.place_source(placement)?;
        Ok(())
    }

    /// All of `media` placed at `start_ticks` on `aimed`: Ripple opens a hole
    /// on the patched and sync-locked lanes first, Overwrite covers whatever is
    /// already in the span. `None` when `media` is not in the pool.
    fn media_placement(
        &self,
        media: MediaId,
        aimed: Option<TrackId>,
        start_ticks: i64,
    ) -> Option<SourcePlacement> {
        let source = self.engine.lock().project().media(media)?.full_range();
        Some(SourcePlacement {
            media,
            source,
            at: RationalTime::new(start_ticks.max(0), self.playhead.rate),
            fill: None,
            ripple: self.edit_mode == crate::types::EditMode::Ripple,
            aimed,
        })
    }

    pub fn focus_media_search(&mut self, focused: bool) {
        self.media_search_focus = focused;
        if focused {
            self.effects_browser.focused = false;
        }
    }

    pub fn clear_media_query(&mut self) {
        self.media_query.clear();
    }

    /// Select one pool item and nothing else.
    ///
    /// The right-hand button and every internal reveal write this; a left click
    /// on a card goes through [`crate::surfaces::pool_pick`], which is the only
    /// place a modified click is understood.
    pub fn select_pool_item(&mut self, media: u64) {
        self.pool_selection.replace(media);
    }

    pub fn in_open_bin(&self, bin: Option<u64>, deep: bool) -> bool {
        if !deep {
            return bin == self.current_bin;
        }
        let Some(open) = self.current_bin else {
            return true;
        };
        let Some(mut at) = bin else {
            return false;
        };
        // Bounded by the folder count: a cycle from a corrupt file walks out.
        for _ in 0..=self.store.bins.len() {
            if at == open {
                return true;
            }
            match self
                .store
                .bins
                .iter()
                .find(|item| item.id == at)
                .and_then(|item| item.parent)
            {
                Some(parent) => at = parent,
                None => return false,
            }
        }
        false
    }

    pub fn media_properties(&self) -> Vec<(&'static str, SharedString)> {
        let Some(item) = self
            .pool_selection
            .focus()
            .and_then(|id| self.store.media.iter().find(|item| item.id == id))
        else {
            return Vec::new();
        };
        let container = std::path::Path::new(&*item.path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_uppercase)
            .unwrap_or_else(|| "Unknown".into());
        let mut rows = vec![("Name", item.name.clone()), ("Container", container.into())];
        if item.kind == crate::types::UiClipKind::Audio {
            rows.push(("Frame size", "Audio only".into()));
        } else {
            rows.push((
                "Frame size",
                format!("{}×{}", item.width, item.height).into(),
            ));
            // An audio-only source is probed in millisecond ticks, which are
            // not frames, and a still in the tick base it was probed in; only
            // a moving picture has a frame rate to state.
            let rate = item.frame_rate;
            rows.push((
                "Frame rate",
                if item.still {
                    "Still image".into()
                } else {
                    format!(
                        "{:.3} fps",
                        f64::from(rate.num) / f64::from(rate.den.max(1))
                    )
                    .into()
                },
            ));
        }
        rows.push(("Duration", item.duration_label.clone()));
        if item.kind != crate::types::UiClipKind::Audio && !item.still {
            rows.push((
                "Video codec",
                codec_label(item.video_codec.as_deref()).into(),
            ));
        }
        rows.push((
            "Audio",
            match (item.has_audio, item.audio_codec.as_deref()) {
                (false, _) => "No".into(),
                (true, Some(codec)) => codec.into(),
                (true, None) => "Yes, codec unknown".into(),
            },
        ));
        rows.push((
            "Imported",
            match item.imported_at {
                Some(at) => crate::datefmt::format_unix_utc(at).into(),
                None => "Not recorded".into(),
            },
        ));
        rows.push((
            "Status",
            if item.missing { "Missing" } else { "Linked" }.into(),
        ));
        rows.push(("Path", item.path.clone()));
        rows
    }
}

/// Where in the stack a lane of `kind` added at the top or the bottom edge of
/// the lanes goes: `None` for the top of the stack, `Some(0)` for the bottom.
/// Visual lanes are drawn upward from the A/V divider and audio lanes downward
/// from it, so the top of the stack is the top of the video side and the
/// bottom of the audio side. The lane-zone invariant then seats the lane at
/// that end of its zone: a video lane under the text lanes at the top and over
/// the main track at the bottom, an audio lane against the divider at the top
/// and under the last audio lane at the bottom.
fn edge_lane_index(kind: TrackKind, top: bool) -> Option<usize> {
    let stack_top = top != kind.is_audio();
    if stack_top { None } else { Some(0) }
}

// Further methods are defined in sibling files; inherent methods keep their
// paths. The import module is public because the drop claim and three file
// dialogs read its format list.
#[path = "media_import.rs"]
pub(crate) mod import;

/// A codec name, or `Unknown` when the platform reported none. A guess from
/// the file extension would name the container, not the codec.
pub(crate) fn codec_label(codec: Option<&str>) -> String {
    match codec {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => "Unknown".to_string(),
    }
}

/// The facts a pool row states about its file, on the lines it draws them:
/// the picture's shape and length, then the streams in the file. A line holds
/// whole facts and never wraps, so a row too narrow for one ends it in an
/// ellipsis rather than starting the next line with a separator. Each word the
/// editor writes is capitalized, since any of them can start a line. The row of
/// a video with no audio track reads `Silent`, because that is the file found
/// missing halfway through a cut. A still states no sound: an image has none.
///
/// A proxied row says so last. The substitution changes which file the preview
/// decodes while every number above it still describes the original, so a row
/// that did not name it would be a row where "1920×1080" and the picture on
/// screen disagree about what is being played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MediaFacts {
    /// Shape and length: `1920×1080 · 00:00:02:00`, `Audio · 00:00:30:00`,
    /// or `Media Offline`.
    pub(crate) picture: String,
    /// The codecs, the sound and the proxy, when there is one to state.
    pub(crate) streams: Option<String>,
}

impl MediaFacts {
    /// Both lines as one, for a view that reads a row left to right.
    pub(crate) fn line(&self) -> String {
        match &self.streams {
            Some(streams) => format!("{} · {streams}", self.picture),
            None => self.picture.clone(),
        }
    }
}

/// The [`MediaFacts`] of one pool item.
pub(crate) fn media_facts(item: &crate::types::UiMediaItem) -> MediaFacts {
    if item.missing {
        return MediaFacts {
            picture: "Media Offline".to_string(),
            streams: None,
        };
    }

    let audio_only = item.kind == crate::types::UiClipKind::Audio;
    let picture = if audio_only {
        format!("Audio · {}", item.duration_label)
    } else {
        format!("{}×{} · {}", item.width, item.height, item.duration_label)
    };
    let mut streams = String::new();
    let mut append = |part: &str| {
        if !streams.is_empty() {
            streams.push_str(" · ");
        }
        streams.push_str(part);
    };
    if audio_only {
        if let Some(codec) = item.audio_codec.as_deref() {
            append(codec);
        }
    } else {
        if let Some(codec) = item.video_codec.as_deref() {
            append(codec);
        }
        if !item.still {
            append(match (item.has_audio, item.audio_codec.as_deref()) {
                (false, _) => "Silent",
                (true, Some(codec)) => codec,
                (true, None) => "Audio",
            });
        }
    }
    if item.proxied {
        append("Proxy");
    }
    MediaFacts {
        picture,
        streams: (!streams.is_empty()).then_some(streams),
    }
}

/// What the bin filter takes. A held key cannot grow the query without bound.
pub(crate) const MEDIA_QUERY_RULE: crate::app::line_text::LineRule =
    crate::app::line_text::LineRule::chars(64);

/// Whether one pool item answers the filter.
///
/// The query is matched against everything the bin can show for the item, so a
/// search for `1920` or `wav` finds media whose name says neither.
pub(super) fn media_matches(item: &crate::types::UiMediaItem, query: &str) -> bool {
    if item.name.to_lowercase().contains(query) || item.path.to_lowercase().contains(query) {
        return true;
    }
    let kind = match item.kind {
        crate::types::UiClipKind::Audio => "audio",
        crate::types::UiClipKind::Media
        | crate::types::UiClipKind::Text
        | crate::types::UiClipKind::Shape
        | crate::types::UiClipKind::Generator => "video",
    };
    if kind.contains(query) {
        return true;
    }
    if let Some(label) = item.label
        && label.id().contains(query)
    {
        return true;
    }
    format!("{}x{}", item.width, item.height).contains(query)
        || format!("{}×{}", item.width, item.height).contains(query)
}
