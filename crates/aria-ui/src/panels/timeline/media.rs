//! Waveform bars and filmstrip cells for the clips currently on screen.

use gpui::Entity;

use crate::app::AppState;
use crate::filmstrips::FilmstripStore;
use crate::timeline_display::{StripCell, Thumbnails};
use crate::types::HeldEdit;

use super::clip::FilmCell;
use super::{BAR_STEP, CELL_W};

/// Slice of `clips` intersecting `[view_start, view_end)` in $O(\log N)$ time.
///
/// `UiTrack::clips` is projected from `Track::clips_ordered()`, which is sorted
/// by `timeline_start_ticks` and non-overlapping.
pub(crate) fn visible_clips_slice(
    clips: &[crate::types::UiClip],
    view_start: i64,
    view_end: i64,
) -> &[crate::types::UiClip] {
    if clips.is_empty() || view_end <= view_start {
        return &[];
    }
    let start = clips.partition_point(|clip| clip.timeline_end_ticks <= view_start);
    let count = clips[start..].partition_point(|clip| clip.timeline_start_ticks < view_end);
    &clips[start..start + count]
}

/// Waveform bar heights per visible clip, keyed by clip id. A clip an edge
/// drag or a slide in flight changes, `held`, plays the take its release
/// leaves it.
///
/// This is also where a decode is asked for: a clip is on screen, so its source
/// is worth reading. Peaks belong to the media, so two clips cut from one file
/// share the decode and each slices its own source window out of it.
#[allow(clippy::too_many_arguments)]
pub(super) fn clip_bars(
    state: &Entity<AppState>,
    store: &crate::types::UiEditorStore,
    visible_rows: &[usize],
    held: Option<&HeldEdit>,
    pps: f32,
    fps: f32,
    view_start: i64,
    view_end: i64,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> std::collections::HashMap<u64, Vec<f32>> {
    struct Visible {
        clip: u64,
        media: u64,
        from: f64,
        to: f64,
        width: f32,
        volume: f32,
        fade_in_ticks: i64,
        fade_out_ticks: i64,
        clip_ticks: i64,
    }
    let mut out = std::collections::HashMap::new();
    #[cfg(test)]
    super::lanes::built::media_rows(visible_rows);
    let scale = state.read(cx).ui_settings.timeline_display.waveform;
    if scale == crate::timeline_display::WaveformScale::Hidden {
        return out;
    }
    let visible: Vec<Visible> = visible_rows
        .iter()
        .filter_map(|row| store.sequence.tracks.get(*row))
        .filter(|track| track.waveform && !track.collapsed)
        .flat_map(|track| visible_clips_slice(&track.clips, view_start, view_end).iter())
        .map(|clip| {
            held.and_then(|held| held.card(clip.id))
                .map_or(clip, |card| &card.clip)
        })
        .filter(|clip| clip.kind == crate::types::UiClipKind::Audio)
        .filter_map(|clip| {
            let clip_ticks = (clip.timeline_end_ticks - clip.timeline_start_ticks).max(1);
            let width = (clip_ticks as f32 / fps) * pps;
            if width < 12.0 {
                return None;
            }
            Some(Visible {
                clip: clip.id,
                media: clip.media_id?,
                from: clip.source_start,
                to: clip.source_end,
                width,
                volume: clip.volume,
                fade_in_ticks: clip.fade_in_ticks,
                fade_out_ticks: clip.fade_out_ticks,
                clip_ticks,
            })
        })
        .take(64)
        .collect();
    for entry in &visible {
        if let Some(item) = store.media.iter().find(|item| item.id == entry.media) {
            crate::waveforms::request(
                state,
                cx,
                entry.media,
                std::path::PathBuf::from(&*item.path),
            );
        }
    }
    let app = state.read(cx);
    for entry in visible {
        let Some(waveform) = app.waveforms.get(entry.media) else {
            continue;
        };
        let count = (entry.width / BAR_STEP).floor().max(1.0) as usize;
        let mut bars = crate::waveforms::bars(waveform, entry.from, entry.to, count);
        crate::waveforms::apply_gain(
            &mut bars,
            entry.volume,
            entry.fade_in_ticks,
            entry.fade_out_ticks,
            entry.clip_ticks,
        );
        for bar in &mut bars {
            *bar = scale.height(*bar);
        }
        out.insert(entry.clip, bars);
    }
    out
}

/// How far, in cell widths, a missing cell takes a stand-in from: a zoom in
/// of up to four times draws every new cell from the strip it zoomed in on.
const STAND_IN_CELLS: f32 = 4.0;

/// The ticks a missing cell takes a stand-in from, at `px_per_tick`.
fn stand_in_reach(px_per_tick: f32) -> i64 {
    (STAND_IN_CELLS * CELL_W / px_per_tick).ceil() as i64
}

/// The filmstrip cells one draw of the timeline drew without their exact
/// frame, across every lane group it drew, and the ones of them it drew with
/// no picture. The draw asks the store for them once, after its last group:
/// an ask replaces the one before it, so a group that asked on its own would
/// take back what the groups before it asked for.
#[derive(Default)]
pub(super) struct Owed {
    missing: Vec<i64>,
    blank: Vec<i64>,
}

impl Owed {
    /// Ask the filmstrip store for every cell owed.
    pub(super) fn ask(self, state: &Entity<AppState>, cx: &mut gpui::App) {
        crate::filmstrips::ask(state, cx, self.missing, self.blank);
    }
}

/// Filmstrip cells per visible media clip, each the part of the clip card it
/// covers and the image drawn there. Only cells that overlap the visible ticks
/// are built or asked for. A clip an edge drag or a slide in flight changes,
/// `held`, draws each cell where its frame plays once the release lands, and
/// none past its ends.
///
/// A cell not rendered yet draws the last rendered cell of the same clip at
/// or before its tick, or else the first after it, within [`STAND_IN_CELLS`]
/// cell widths: after a zoom in, the frame the strip showed there before.
/// With none that near, the space stays empty. A stand-in never comes from
/// outside the clip, so no frame is drawn across the clip's cut. Each cell
/// drawn without its exact frame goes to `owed`, in the order it was drawn,
/// and to its empty ones as well when it was drawn with no picture.
#[allow(clippy::too_many_arguments)]
pub(super) fn clip_cells(
    state: &Entity<AppState>,
    store: &crate::types::UiEditorStore,
    visible_rows: &[usize],
    held: Option<&HeldEdit>,
    pps: f32,
    fps: f32,
    view_start: i64,
    view_end: i64,
    owed: &mut Owed,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> std::collections::HashMap<u64, Vec<FilmCell>> {
    let mut out = std::collections::HashMap::new();
    #[cfg(test)]
    super::lanes::built::media_rows(visible_rows);
    let visible: Vec<(u64, i64, i64, f32)> = visible_rows
        .iter()
        .filter_map(|row| store.sequence.tracks.get(*row))
        .filter(|track| track.filmstrip && !track.collapsed)
        .flat_map(|track| visible_clips_slice(&track.clips, view_start, view_end).iter())
        .filter(|clip| clip.kind == crate::types::UiClipKind::Media)
        .filter_map(|clip| {
            let span = (clip.timeline_end_ticks - clip.timeline_start_ticks).max(1);
            let width = (span as f32 / fps) * pps;
            if width < 24.0 {
                return None;
            }
            Some((
                clip.id,
                clip.timeline_start_ticks,
                clip.timeline_end_ticks,
                width,
            ))
        })
        .take(32)
        .collect();
    let thumbnails = state.read(cx).ui_settings.timeline_display.thumbnails;
    let px_per_tick = pps / fps;
    let reach = stand_in_reach(px_per_tick);
    let filmstrips = &state.read(cx).filmstrips;
    #[cfg(test)]
    let drawn_before = (owed.missing.len(), owed.blank.len());
    for (clip, start, end, width) in visible {
        let shown = strip_cells(
            thumbnails,
            start,
            end,
            width,
            px_per_tick,
            (view_start, view_end),
        );
        let mut cells = cell_pictures(
            filmstrips,
            shown,
            (start, end),
            reach,
            &mut owed.missing,
            &mut owed.blank,
        );
        if let Some(card) = held.and_then(|held| held.card(clip)) {
            let width =
                (card.clip.timeline_end_ticks - card.clip.timeline_start_ticks) as f32 / fps * pps;
            cells.retain_mut(|cell| {
                let ends = (
                    card.moved_x(cell.from, px_per_tick),
                    card.moved_x(cell.to, px_per_tick),
                );
                match ends {
                    (Some(head), Some(tail)) if (0.0..width).contains(&head.min(tail)) => {
                        (cell.from, cell.to) = (head.min(tail), head.max(tail));
                        true
                    }
                    _ => false,
                }
            });
        }
        if !cells.is_empty() {
            out.insert(clip, cells);
        }
    }
    #[cfg(test)]
    super::lanes::built::owed(
        &owed.missing[drawn_before.0..],
        &owed.blank[drawn_before.1..],
    );
    out
}

/// The cells of a clip from `start` to `end`, `width` pixels wide, that
/// overlap the visible ticks `view`, in pixels from the clip's head.
pub(super) fn strip_cells(
    thumbnails: Thumbnails,
    start: i64,
    end: i64,
    width: f32,
    px_per_tick: f32,
    view: (i64, i64),
) -> impl Iterator<Item = StripCell> {
    let shown = (
        (view.0 - start) as f32 * px_per_tick,
        (view.1 - start) as f32 * px_per_tick,
    );
    thumbnails
        .cells(start, end, width, CELL_W, px_per_tick, || {
            crate::filmstrips::cell_spans(start, end, width, CELL_W, view)
        })
        .into_iter()
        .filter(move |cell| cell.to > shown.0 && cell.from < shown.1)
}

/// The pictures of `cells`, the cells of the clip from `start` to `end`: each
/// cell's own or, while it is not rendered, the last rendered cell of the clip
/// at or before its tick, or else the first after it, no more than `reach`
/// ticks away. Pushes the tick of every cell drawn without its exact frame to
/// `missing`, and of every cell drawn with no picture also to `blank`.
fn cell_pictures(
    filmstrips: &FilmstripStore,
    cells: impl Iterator<Item = StripCell>,
    (start, end): (i64, i64),
    reach: i64,
    missing: &mut Vec<i64>,
    blank: &mut Vec<i64>,
) -> Vec<FilmCell> {
    cells
        .filter_map(|cell| {
            let picture = match filmstrips.get(cell.tick) {
                Some(own) => {
                    if own.snapped {
                        missing.push(cell.tick);
                    }
                    own
                }
                None => {
                    missing.push(cell.tick);
                    let within = (
                        start.max(cell.tick.saturating_sub(reach)),
                        end.min(cell.tick.saturating_add(reach).saturating_add(1)),
                    );
                    let Some(stand_in) = filmstrips.stand_in(cell.tick, within) else {
                        blank.push(cell.tick);
                        return None;
                    };
                    stand_in
                }
            };
            Some(FilmCell {
                from: cell.from,
                to: cell.to,
                image: picture.image.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "stand_in_tests.rs"]
mod stand_in_tests;
