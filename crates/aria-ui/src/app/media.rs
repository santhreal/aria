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
use gpui::SharedString;

impl AppState {
    pub(crate) fn ensure_kind_track(&mut self, kind: TrackKind) -> Result<TrackId, EngineError> {
        let ui_kind = crate::types::UiTrackKind::from_model(kind);
        if let Some(id) = self
            .store
            .sequence
            .tracks
            .iter()
            .find(|track| track.kind == ui_kind)
            .map(|track| TrackId::from_raw(track.id))
        {
            return Ok(id);
        }
        let name = kind.default_name();
        self.add_track(kind, name)?;
        self.store
            .sequence
            .tracks
            .iter()
            .find(|track| track.kind == ui_kind)
            .map(|track| TrackId::from_raw(track.id))
            .ok_or_else(|| EngineError::Export("no track after add".into()))
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
        let targeted = self
            .store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.kind == ui_kind && lane.targeted)
            .map(|lane| TrackId::from_raw(lane.id));
        match targeted {
            Some(id) => Ok(id),
            None => self.ensure_kind_track(kind),
        }
    }

    pub fn drop_media_on_new_lane(
        &mut self,
        media: MediaId,
        top: bool,
        at: i64,
    ) -> Result<(), EngineError> {
        let kind = {
            let engine = self.engine.lock();
            let Some(item) = engine.project().media(media) else {
                return Ok(());
            };
            if item.kind() == aria_models::MediaKind::Audio {
                TrackKind::Audio
            } else {
                TrackKind::Video
            }
        };
        let index = if top { Some(0) } else { None };
        let track = self.add_lane_of_kind(kind, index)?;
        self.drop_media_on_timeline(media, Some(track), at)
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
        // The prefix is the lane kind's own default name with its ordinal
        // removed, so one column decides both the first lane's name and every
        // later one's. The match this replaced named effect and filter lanes
        // "V", because they fell through to the video arm.
        let prefix = kind
            .default_name()
            .trim_end_matches(|c: char| c.is_ascii_digit());
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
        let (source, kind) = {
            let engine = self.engine.lock();
            let Some(item) = engine.project().media(media) else {
                return Ok(());
            };
            (
                item.full_range(),
                if item.kind() == aria_models::MediaKind::Audio {
                    TrackKind::Audio
                } else {
                    TrackKind::Video
                },
            )
        };
        let track = self.drop_target_track(kind, track)?;
        let at = RationalTime::new(start_ticks.max(0), self.playhead.rate);
        // Ripple pushes downstream clips right by the new clip's length;
        // Overwrite covers whatever is already in the span it lands on.
        let command = match self.edit_mode {
            crate::types::EditMode::Ripple => EditCommand::RippleInsert {
                track,
                media,
                source,
                at,
            },
            crate::types::EditMode::Overwrite => EditCommand::OverwriteClip {
                track,
                media,
                source,
                at,
            },
        };
        self.apply_command(Command::Edit(command))?;
        Ok(())
    }

    pub fn focus_media_search(&mut self, focused: bool) {
        self.media_search_focus = focused;
    }

    pub fn media_query_push(&mut self, ch: char) {
        if self.media_query.chars().count() >= MEDIA_QUERY_MAX_LEN {
            return;
        }
        self.media_query.push(ch);
    }

    pub fn media_query_pop(&mut self) {
        self.media_query.pop();
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
            .unwrap_or_else(|| "unknown".into());
        let mut rows = vec![("Name", item.name.clone()), ("Container", container.into())];
        if item.kind == crate::types::UiClipKind::Audio {
            rows.push(("Frame size", "audio only".into()));
        } else {
            rows.push((
                "Frame size",
                format!("{}×{}", item.width, item.height).into(),
            ));
            // An audio-only source is probed in millisecond ticks, which are
            // not frames; only a picture has a frame rate to state.
            let rate = item.frame_rate;
            rows.push((
                "Frame rate",
                format!(
                    "{:.3} fps",
                    f64::from(rate.num) / f64::from(rate.den.max(1))
                )
                .into(),
            ));
        }
        rows.push(("Duration", item.duration_label.clone()));
        if item.kind != crate::types::UiClipKind::Audio {
            rows.push((
                "Video codec",
                codec_label(item.video_codec.as_deref()).into(),
            ));
        }
        rows.push((
            "Audio",
            match (item.has_audio, item.audio_codec.as_deref()) {
                (false, _) => "no".into(),
                (true, Some(codec)) => codec.into(),
                (true, None) => "yes, codec unknown".into(),
            },
        ));
        rows.push((
            "Imported",
            match item.imported_at {
                Some(at) => crate::datefmt::format_unix_utc(at).into(),
                None => "not recorded".into(),
            },
        ));
        rows.push((
            "Status",
            if item.missing { "missing" } else { "linked" }.into(),
        ));
        rows.push(("Path", item.path.clone()));
        rows
    }
}

// Further methods live in sibling files; inherent methods keep their paths. The
// import module is public because the format list it owns is read by the drop
// claim and by three file dialogs.
#[path = "media_import.rs"]
pub(crate) mod import;

/// A codec name, or an honest statement that the platform did not report one.
/// A guess from the file extension would name the container, not the codec.
pub fn codec_label(codec: Option<&str>) -> String {
    match codec {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => "unknown".to_string(),
    }
}

/// The one-line caption under a pool row: what the pool can prove about the
/// file. Shape, length, codec, and whether there is sound in it — a video with
/// no audio track is the one that surprises an operator halfway through a cut.
///
/// A proxied row says so last. The substitution changes which file the preview
/// decodes while every number above it still describes the original, so a row
/// that did not name it would be a row where "1920×1080" and the picture on
/// screen disagree about what is being played.
pub fn media_caption(item: &crate::types::UiMediaItem) -> String {
    use std::fmt::Write as _;

    if item.missing {
        return "media missing".to_string();
    }

    let audio_only = item.kind == crate::types::UiClipKind::Audio;
    let mut caption = String::with_capacity(64);
    if audio_only {
        caption.push_str("Audio");
    } else {
        write!(caption, "{}×{}", item.width, item.height).expect("writing to a String cannot fail");
    }
    let mut append = |part: &str| {
        caption.push_str("  ·  ");
        caption.push_str(part);
    };
    append(&item.duration_label);

    if audio_only {
        if let Some(codec) = item.audio_codec.as_deref() {
            append(codec);
        }
    } else {
        if let Some(codec) = item.video_codec.as_deref() {
            append(codec);
        }
        append(match (item.has_audio, item.audio_codec.as_deref()) {
            (false, _) => "silent",
            (true, Some(codec)) => codec,
            (true, None) => "audio",
        });
    }
    if item.proxied {
        append("proxy");
    }
    caption
}

/// Longest accepted filter. A held key cannot grow the query without bound.
const MEDIA_QUERY_MAX_LEN: usize = 64;

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
    if let Some(label) = item.label {
        if label.name().contains(query) {
            return true;
        }
    }
    format!("{}x{}", item.width, item.height).contains(query)
        || format!("{}×{}", item.width, item.height).contains(query)
}
