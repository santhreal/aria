//! A pool source seen before it is placed: hover scrub on a tile, and the
//! source monitor.
//!
//! Both draw the same thing — one frame of one source at one source tick,
//! rendered by [`aria_engine::Engine::media_frame_fit`], which is the renderer's
//! own decode path with no timeline behind it. The tile asks for a thumbnail and
//! the monitor asks for a pane-sized frame; nothing here decodes anything
//! itself.
//!
//! Everything is keyed by source and tick, and held against a stamp of what the
//! source is — its path and whether the renderer substitutes a proxy — so a
//! relink or a proxy cannot leave a stale picture on screen and an edit on the
//! timeline cannot throw away pictures it did not change. One render is in
//! flight at a time per surface: a scrub across forty tiles must not queue forty
//! decodes.
//!
//! Renders are asked for from the render pass, the way filmstrip cells are. The
//! monitor's tick moves on hover as well as on a keypress, so there is no single
//! event to hang the request on.

use crate::app::AppState;
use aria_engine::EngineError;
use aria_models::MediaId;
use aria_storage::CacheId;
use gpui::{App, Entity, RenderImage};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
/// Thumbnail size on a pool tile. A 16:9 source fills it exactly; anything else
/// is fit inside it.
pub const THUMB_W: u32 = 120;
pub const THUMB_H: u32 = 68;
/// Largest frame the source monitor asks for. Every layout the shell allows is
/// smaller than this, and `Contain` does the rest.
const SOURCE_MAX_W: u32 = 1280;
const SOURCE_MAX_H: u32 = 720;
/// Thumbnails kept across all sources. A thumbnail is a few KB, so this bounds
/// the pool's picture memory at a few megabytes.
const MAX_THUMBS: usize = 256;
/// Rendered pool pictures, valid for one session revision.
///
/// The monitor keeps exactly one frame rather than a cache: it shows one tick at
/// a time, and a pane-sized frame is a thousand times a thumbnail.
#[derive(Default)]
pub struct PoolPreviewStore {
    /// What each pool entry's held pictures are of: a stamp of the source path
    /// and the proxy substitution behind it.
    ///
    /// Keyed per entry rather than by the session revision, because a thumbnail
    /// is a picture of a source: moving a clip on the timeline cannot change one,
    /// and clearing the whole pool on every edit meant an edit re-decoded every
    /// picture on screen, one at a time, through the same engine the preview
    /// needs.
    stamps: HashMap<u64, u64>,
    thumbs: HashMap<(u64, i64), Arc<RenderImage>>,
    /// The one thumbnail being rendered, if any.
    in_flight: Option<(u64, i64)>,
    /// Requests that produced no frame, so a source that cannot be decoded is
    /// not retried on every repaint for as long as its tile is on screen.
    empty: Vec<(u64, i64)>,
    /// The monitor's frame and what it is of.
    source: Option<((u64, i64), Arc<RenderImage>)>,
    source_in_flight: Option<(u64, i64)>,
    source_empty: Vec<(u64, i64)>,
    /// Where each tile's picture was last painted: `(left, width)` in window
    /// pixels. A pointer position means nothing without it, and a mouse move
    /// carries no element bounds.
    rects: HashMap<u64, (f32, f32)>,
    /// Where the monitor's scrub bar was last painted, same reason.
    source_bar: Option<(f32, f32)>,
}
impl PoolPreviewStore {
    pub fn get(&self, media: u64, tick: i64) -> Option<&Arc<RenderImage>> {
        self.thumbs.get(&(media, tick))
    }

    /// Record where `media`'s picture is painted. Layout, not content: this
    /// survives a revision change and never asks for a repaint.
    pub fn record_rect(&mut self, media: u64, left: f32, width: f32) {
        self.rects.insert(media, (left, width));
    }

    /// Where `media`'s picture was last painted, if it has been.
    pub fn rect(&self, media: u64) -> Option<(f32, f32)> {
        self.rects.get(&media).copied()
    }

    /// Record where the monitor's scrub bar is painted.
    pub fn record_source_bar(&mut self, left: f32, width: f32) {
        self.source_bar = Some((left, width));
    }

    /// Where the monitor's scrub bar was last painted, if it has been.
    pub fn source_bar(&self) -> Option<(f32, f32)> {
        self.source_bar
    }

    /// Whether this thumbnail should be rendered now: nothing is held for it,
    /// it did not already come back empty, and nothing else is in flight.
    pub fn wants(&self, media: u64, tick: i64) -> bool {
        let key = (media, tick);
        self.in_flight.is_none() && !self.thumbs.contains_key(&key) && !self.empty.contains(&key)
    }

    /// The monitor's frame for exactly this tick.
    pub fn source(&self, media: u64, tick: i64) -> Option<&Arc<RenderImage>> {
        self.source
            .as_ref()
            .filter(|(key, _)| *key == (media, tick))
            .map(|(_, image)| image)
    }

    /// Any frame held for `media`, whatever tick it is of — what the monitor
    /// draws while the tick it was just scrubbed to is still rendering, so a
    /// scrub dims rather than blanks.
    pub fn source_stale(&self, media: u64) -> Option<&Arc<RenderImage>> {
        self.source
            .as_ref()
            .filter(|((held, _), _)| *held == media)
            .map(|(_, image)| image)
    }

    /// Whether the monitor's frame should be rendered now.
    pub fn wants_source(&self, media: u64, tick: i64) -> bool {
        let key = (media, tick);
        self.source_in_flight.is_none()
            && self.source.as_ref().map(|(held, _)| *held) != Some(key)
            && !self.source_empty.contains(&key)
    }

    /// The stamp for a pool entry: everything a picture of it depends on.
    ///
    /// The path and the proxy substitution are the whole answer — width, height
    /// and frame rate are read from that same file, and a file swapped
    /// underneath the editor is not something any key can see. `missing` is in
    /// it so a relink that restores a file asks again rather than serving the
    /// blank it got while the file was gone.
    pub fn stamp_of(item: &crate::types::UiMediaItem) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        item.path.hash(&mut hasher);
        item.proxied.hash(&mut hasher);
        item.missing.hash(&mut hasher);
        hasher.finish()
    }

    /// Learn what `media` is now, dropping its held pictures if the source
    /// behind them changed. Every other entry's pictures are kept.
    pub fn sync_media(&mut self, media: u64, stamp: u64) {
        if self.stamps.get(&media) == Some(&stamp) {
            return;
        }
        self.stamps.insert(media, stamp);
        self.thumbs.retain(|(held, _), _| *held != media);
        self.empty.retain(|(held, _)| *held != media);
        if self
            .source
            .as_ref()
            .is_some_and(|((held, _), _)| *held == media)
        {
            self.source = None;
        }
        self.source_empty.retain(|(held, _)| *held != media);
        // A render already in flight for this entry is of the old source. It is
        // dropped on arrival by the stamp check in `finish`/`finish_source`,
        // which is also what keeps `in_flight` honest: clearing it here would
        // let a second render start beside the one still running.
    }

    fn start(&mut self, media: u64, tick: i64) {
        self.in_flight = Some((media, tick));
    }

    fn finish(&mut self, stamp: u64, media: u64, tick: i64, image: Option<Arc<RenderImage>>) {
        let key = (media, tick);
        if self.in_flight == Some(key) {
            self.in_flight = None;
        }
        if self.stamps.get(&media) != Some(&stamp) {
            return;
        }
        match image {
            Some(image) => {
                if self.thumbs.len() >= MAX_THUMBS {
                    self.thumbs.clear();
                }
                self.thumbs.insert(key, image);
            }
            None => self.empty.push(key),
        }
    }

    fn start_source(&mut self, media: u64, tick: i64) {
        self.source_in_flight = Some((media, tick));
    }

    fn finish_source(
        &mut self,
        stamp: u64,
        media: u64,
        tick: i64,
        image: Option<Arc<RenderImage>>,
    ) {
        let key = (media, tick);
        if self.source_in_flight == Some(key) {
            self.source_in_flight = None;
        }
        if self.stamps.get(&media) != Some(&stamp) {
            return;
        }
        match image {
            Some(image) => self.source = Some((key, image)),
            None => self.source_empty.push(key),
        }
    }
}
/// What the source monitor is showing: a pool entry, and where in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceView {
    pub media: u64,
    pub name: gpui::SharedString,
    /// Length of the source in its own frame rate.
    pub duration_ticks: i64,
    /// The source tick on screen.
    pub tick: i64,
    /// The source in point, if one is set. Source ticks, like `tick`.
    pub mark_in: Option<i64>,
    /// The source out point, exclusive, if one is set.
    pub mark_out: Option<i64>,
}
/// The source tick under the pointer, for a row `width` px wide.
///
/// The last frame sits at the right edge rather than one frame past it, so a
/// scrub to the end of the row shows the end of the take.
pub fn scrub_tick(x: f32, width: f32, duration_ticks: i64) -> i64 {
    let last = duration_ticks.saturating_sub(1).max(0);
    if last == 0 || width <= 0.0 || !x.is_finite() {
        return 0;
    }
    let frac = (x / width).clamp(0.0, 1.0);
    ((frac * last as f32).round() as i64).clamp(0, last)
}

impl AppState {
    /// What a pool entry's pictures are of, read from the projection.
    ///
    /// The projection, not the engine: thumbnails are asked for from the render
    /// pass, and a render that took the engine lock would wait behind whatever
    /// decode is holding it. `None` means the entry left the pool between the
    /// frame that drew it and this request.
    pub(crate) fn media_stamp(&self, media: u64) -> Option<u64> {
        self.store
            .media
            .iter()
            .find(|item| item.id == media)
            .map(PoolPreviewStore::stamp_of)
    }

    pub fn mark_out_here(&mut self) {
        if self.source_view.is_some() {
            self.set_source_out();
            self.status_message = Some("Source Out".into());
        } else {
            self.set_mark_out();
        }
    }

    pub fn clear_mark_in_here(&mut self) {
        if let Some(view) = self.source_view.as_mut() {
            view.mark_in = None;
            self.status_message = Some("Cleared Source In".into());
        } else {
            self.clear_mark_in();
        }
    }

    pub fn clear_mark_out_here(&mut self) {
        if let Some(view) = self.source_view.as_mut() {
            view.mark_out = None;
            self.status_message = Some("Cleared Source Out".into());
        } else {
            self.clear_mark_out();
        }
    }

    pub fn source_range(&self) -> Option<(i64, i64)> {
        let view = self.source_view.as_ref()?;
        let start = view.mark_in.unwrap_or(0);
        let end = view.mark_out.unwrap_or_else(|| view.duration_ticks.max(1));
        (end > start).then_some((start, end))
    }

    pub fn scrub_source(&mut self, x: f32) {
        let Some((left, width)) = self.pool_preview.source_bar() else {
            return;
        };
        let Some((media, duration_ticks)) = self
            .source_view
            .as_ref()
            .map(|view| (view.media, view.duration_ticks))
        else {
            return;
        };
        let tick = scrub_tick(x - left, width, duration_ticks);
        self.hover_pool_item(media, tick);
    }

    pub fn cut_source_in(&mut self, mode: crate::types::EditMode) -> Result<(), EngineError> {
        let Some(media) = self.source_view.as_ref().map(|view| view.media) else {
            return Ok(());
        };
        let Some((start, end)) = self.source_range() else {
            self.status_message = Some("the marked range is empty".into());
            return Ok(());
        };
        let media = MediaId::from_raw(media);
        let (rate, kind) = {
            let engine = self.engine.lock();
            let Some(item) = engine.project().media(media) else {
                return Ok(());
            };
            (
                item.frame_rate,
                if item.kind() == aria_models::MediaKind::Audio {
                    aria_models::TrackKind::Audio
                } else {
                    aria_models::TrackKind::Video
                },
            )
        };
        let source = aria_models::TimeRange::at_rate(start, end - start, rate);
        let track = self.drop_target_track(kind, None)?;
        let fill = self.marked_range();
        let at = aria_models::RationalTime::new(
            fill.map(|(from, _)| from)
                .or(self.mark_in)
                .unwrap_or(self.playhead.value)
                .max(0),
            self.playhead.rate,
        );
        let ripple = mode == crate::types::EditMode::Ripple;
        let command = match fill {
            Some((from, to)) => aria_commands::EditCommand::FitToFill {
                track,
                media,
                source,
                at,
                duration: aria_models::RationalTime::new(to - from, self.playhead.rate),
                ripple,
            },
            None if ripple => aria_commands::EditCommand::RippleInsert {
                track,
                media,
                source,
                at,
            },
            None => aria_commands::EditCommand::OverwriteClip {
                track,
                media,
                source,
                at,
            },
        };
        self.apply_command(aria_commands::Command::Edit(command))?;
        self.status_message = Some(
            match (fill.is_some(), ripple) {
                (true, _) => "Fit to fill",
                (false, true) => "Insert",
                (false, false) => "Overwrite",
            }
            .into(),
        );
        Ok(())
    }

    pub fn match_frame(&mut self) -> bool {
        let playhead = self.playhead.value;
        let selected = self.selected_clip();
        let found = {
            let engine = self.engine.lock();
            let project = engine.project();
            // The selection wins when it is under the playhead, because that is
            // the clip you are looking at; otherwise the topmost one is, since
            // that is the picture the preview is showing.
            let mut under: Vec<&aria_models::Clip> = project
                .timeline()
                .tracks_ordered()
                .flat_map(|lane| lane.clips())
                .filter(|clip| {
                    clip.timeline.start.value <= playhead && playhead < clip.timeline.end_tick()
                })
                .collect();
            let chosen = selected
                .and_then(|id| under.iter().copied().find(|clip| clip.id == id))
                .or_else(|| under.pop());
            chosen.and_then(|clip| match &clip.content {
                aria_models::ClipSource::Media { media, source } => Some((
                    *media,
                    source.start.value + clip.scale_by_speed(playhead - clip.timeline.start.value),
                    source.start.value,
                    source.end_tick(),
                )),
                _ => None,
            })
        };
        let Some((media, tick, take_in, take_out)) = found else {
            self.status_message = Some("no media clip under the playhead".into());
            return false;
        };
        self.select_pool_item(media.raw());
        if !self
            .source_view
            .as_ref()
            .is_some_and(|view| view.media == media.raw())
        {
            self.source_view = None;
            if !self.toggle_source_monitor() {
                return false;
            }
        }
        let Some(view) = self.source_view.as_mut() else {
            return false;
        };
        view.tick = tick.clamp(0, view.duration_ticks.saturating_sub(1).max(0));
        view.mark_in = Some(take_in);
        view.mark_out = Some(take_out);
        self.status_message = Some("Match Frame".into());
        true
    }

    pub fn close_source_monitor(&mut self) {
        self.source_view = None;
    }
}

// Further methods live in sibling files; inherent methods keep their paths.
#[path = "pool_preview_source.rs"]
mod source;
/// Render one pool thumbnail if the store wants it. Safe to call from a render:
/// the request is marked in flight before the thread starts.
pub fn request_thumb(state: &Entity<AppState>, cx: &mut App, media: u64, tick: i64) {
    render_into(state, cx, media, tick, THUMB_W, THUMB_H, false);
}
/// Render the frame the source monitor is showing, if it does not have it.
pub fn request_source(state: &Entity<AppState>, cx: &mut App, media: u64, tick: i64) {
    render_into(state, cx, media, tick, SOURCE_MAX_W, SOURCE_MAX_H, true);
}
#[cfg(test)]
#[path = "pool_preview_tests.rs"]
mod pool_preview_tests;

// Helpers live in sibling files, declared here. Each is `pub(super)`, so the
// parent and the siblings reach it by name and nothing is left unreachable.
#[path = "pool_preview_render.rs"]
mod render;
use render::render_into;

// Poster frames that outlive the session, and the container they are written in.
#[path = "pool_preview_disk.rs"]
mod disk;
