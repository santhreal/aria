//! Waveform bars and filmstrip cells for the clips currently on screen.

use gpui::Entity;

use crate::app::AppState;

use super::{BAR_STEP, CELL_W};

/// Waveform bar heights per visible clip, keyed by clip id.
///
/// This is also where a decode is asked for: a clip is on screen, so its source
/// is worth reading. Peaks belong to the media, so two clips cut from one file
/// share the decode and each slices its own source window out of it.
pub(super) fn clip_bars(
    state: &Entity<AppState>,
    store: &crate::types::UiEditorStore,
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
    let visible: Vec<Visible> = store
        .sequence
        .tracks
        .iter()
        .filter(|track| track.waveform)
        .flat_map(|track| track.clips.iter())
        .filter(|clip| clip.timeline_end_ticks > view_start && clip.timeline_start_ticks < view_end)
        .filter(|clip| clip.kind == crate::types::UiClipKind::Audio)
        .filter_map(|clip| {
            let clip_ticks = (clip.timeline_end_ticks - clip.timeline_start_ticks).max(1);
            Some(Visible {
                clip: clip.id,
                media: clip.media_id?,
                from: clip.source_start,
                to: clip.source_end,
                width: (clip_ticks as f32 / fps) * pps,
                volume: clip.volume,
                fade_in_ticks: clip.fade_in_ticks,
                fade_out_ticks: clip.fade_out_ticks,
                clip_ticks,
            })
        })
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
        out.insert(entry.clip, bars);
    }
    out
}

/// Filmstrip cells per visible media clip: the x offset inside the clip card
/// and the image to draw there.
///
/// At most one cell is asked for per frame, and a rendered cell is never
/// substituted for a neighbour: a cell is drawn where it belongs or the space
/// stays empty, because a frame drawn at the wrong tick lies about the cut.
pub(super) fn clip_cells(
    state: &Entity<AppState>,
    store: &crate::types::UiEditorStore,
    pps: f32,
    fps: f32,
    view_start: i64,
    view_end: i64,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> std::collections::HashMap<u64, Vec<(f32, std::sync::Arc<gpui::RenderImage>)>> {
    let mut out = std::collections::HashMap::new();
    let visible: Vec<(u64, i64, i64, f32)> = store
        .sequence
        .tracks
        .iter()
        .filter(|track| track.filmstrip)
        .flat_map(|track| track.clips.iter())
        .filter(|clip| clip.timeline_end_ticks > view_start && clip.timeline_start_ticks < view_end)
        .filter(|clip| clip.kind == crate::types::UiClipKind::Media)
        .map(|clip| {
            let span = (clip.timeline_end_ticks - clip.timeline_start_ticks).max(1);
            (
                clip.id,
                clip.timeline_start_ticks,
                clip.timeline_end_ticks,
                (span as f32 / fps) * pps,
            )
        })
        .collect();
    for (clip, start, end, width) in visible {
        let ticks = crate::filmstrips::cell_ticks(start, end, width, CELL_W);
        let mut cells = Vec::new();
        for tick in ticks {
            let ready = state
                .read(cx)
                .filmstrips
                .get(tick)
                .map(|cell| cell.image.clone());
            match ready {
                Some(image) => {
                    let x = ((tick - start) as f32 / fps) * pps;
                    cells.push((x, image));
                }
                None => crate::filmstrips::request(state, cx, tick),
            }
        }
        if !cells.is_empty() {
            out.insert(clip, cells);
        }
    }
    out
}
