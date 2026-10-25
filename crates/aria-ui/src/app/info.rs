//! What the Info panel reads: the colour under the pointer in the Viewer, the
//! selected clip's times, the drag in progress, the faults the session holds,
//! what the last rendered frame cost, and the rate playback held.
//!
//! The pointer reading follows the frame on screen. A frame with CPU pixels is
//! read at once; a GPU-only frame is read one texel at a time on the preview
//! worker ([`crate::preview_worker::ProbeAsk`]), and a reading that arrives
//! after the pointer or the frame moved on is dropped. Nothing is read while
//! the panel's tab is closed or behind another tab.

use std::sync::Arc;

use aria_core::RgbaImage;
use aria_engine::frame_stages;

use super::AppState;
use super::canvas_overlay::OverlayDrag;
use crate::preview_worker::{ProbeAsk, Probed};
use crate::timecode::format_ticks;
use crate::types::UiClipKind;

/// The Viewer pixel under the pointer and the colour read there.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ViewerProbe {
    /// The canvas pixel under the pointer; `None` off the picture.
    pub at: Option<[u32; 2]>,
    /// The straight-alpha colour of the frame on screen at `at`, once read.
    pub color: Option<Probed>,
}

/// One labelled value of the Info panel.
#[derive(Debug, Clone, PartialEq)]
pub struct InfoRow {
    pub label: &'static str,
    pub value: String,
}

impl InfoRow {
    fn new(label: &'static str, value: impl Into<String>) -> Self {
        Self {
            label,
            value: value.into(),
        }
    }
}

/// A fault the session holds, with what it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct InfoFault {
    pub source: String,
    pub message: String,
}

/// The playback run as the Info panel states it.
#[derive(Debug, Clone, PartialEq)]
pub struct InfoPlayback {
    /// Whether the run holds the sequence's rate: renders inside budget and
    /// the frames that came due shown.
    pub holds_rate: bool,
    /// The rate shown against the rate asked for, then the late frames.
    pub rows: [InfoRow; 2],
}

/// Everything the Info panel draws, read without the session lock.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InfoReadout {
    /// The canvas pixel under the pointer.
    pub pointer: Option<[u32; 2]>,
    /// The colour there, straight alpha, once read from the frame on screen.
    pub color: Option<[u8; 4]>,
    /// The selected clip's name, times and source.
    pub clip: Vec<InfoRow>,
    /// The drag in progress: a trim on the timeline or a handle in the Viewer.
    pub drag: Vec<InfoRow>,
    /// Driver, plugin and mount faults, oldest source first.
    pub faults: Vec<InfoFault>,
    /// What the last rendered frame cost: one row per render stage in pipeline
    /// order, the whole frame, the part a smaller preview buys back, and the
    /// effect passes when it drew any. Empty until a frame is measured.
    pub frame: Vec<InfoRow>,
    /// The playback run against the sequence's rate. `None` until playback
    /// delivers a frame.
    pub playback: Option<InfoPlayback>,
}

/// The texel of a `frame`-sized picture that covers canvas pixel `at` of a
/// `canvas`-sized one. The Viewer renders below canvas size at a reduced
/// preview resolution, with the canvas aspect kept.
pub(crate) fn texel_of(at: [u32; 2], canvas: [u32; 2], frame: [u32; 2]) -> [u32; 2] {
    std::array::from_fn(|axis| {
        let scaled = u64::from(at[axis]) * u64::from(frame[axis]) / u64::from(canvas[axis].max(1));
        u32::try_from(scaled)
            .unwrap_or(u32::MAX)
            .min(frame[axis].saturating_sub(1))
    })
}

impl AppState {
    /// Note that the pointer is at window point `window` over the Viewer, or
    /// has left it with `None`. True when the Info panel needs a repaint.
    pub fn point_viewer(&mut self, window: Option<[f32; 2]>) -> bool {
        let canvas = self.canvas_size();
        let at = window.and_then(|point| {
            let [x, y] = self.overlay_map()?.to_canvas(point);
            let inside = x >= 0.0 && y >= 0.0 && x < canvas[0] as f32 && y < canvas[1] as f32;
            // Non-negative, so the cast floors.
            inside.then_some([x as u32, y as u32])
        });
        if at == self.viewer_probe.at {
            return false;
        }
        self.viewer_probe = ViewerProbe { at, color: None };
        self.probe_viewer();
        self.dock_tree.is_active(crate::dock::PANEL_INFO)
    }

    /// Read the colour under the pointer from the frame on screen: at once
    /// from CPU pixels, on the preview worker from a GPU-only frame.
    pub(crate) fn probe_viewer(&mut self) {
        let (Some(at), Some(tick)) = (self.viewer_probe.at, self.shown_tick) else {
            return;
        };
        if !self.dock_tree.is_active(crate::dock::PANEL_INFO) {
            return;
        }
        let canvas = self.canvas_size();
        if let Some(frame) = &self.last_frame {
            let [x, y] = texel_of(at, canvas, [frame.width, frame.height]);
            self.viewer_probe.color = Some(Probed {
                at,
                tick,
                rgba: frame.pixel(x, y),
            });
            return;
        }
        let Some(texture) = self.preview_worker.shown_texture() else {
            return;
        };
        let extent = texture.size();
        let frame = [extent.width.0, extent.height.0].map(|side| u32::try_from(side).unwrap_or(0));
        if frame.contains(&0) {
            return;
        }
        self.preview_worker.probe(ProbeAsk {
            texel: texel_of(at, canvas, frame),
            texture,
            at,
            tick,
        });
    }

    /// Keep a colour the preview worker read when it is still the pixel under
    /// the pointer in the frame on screen. True when it was kept.
    pub(crate) fn adopt_probe(&mut self, probed: Option<Probed>) -> bool {
        let Some(probed) = probed else {
            return false;
        };
        if self.viewer_probe.at != Some(probed.at) || self.shown_tick != Some(probed.tick) {
            return false;
        }
        self.viewer_probe.color = Some(probed);
        true
    }

    /// The frame on screen as straight-alpha pixels: its CPU copy when it has
    /// one, else a read of its whole GPU texture. A GPU-only frame blocks on
    /// the GPU queue, so this serves explicit commands, never a paint.
    pub(crate) fn picture_on_screen(&self) -> Option<Arc<RgbaImage>> {
        if let Some(frame) = &self.last_frame {
            return Some(Arc::clone(frame));
        }
        let texture = self.preview_worker.shown_texture()?;
        let device = self.preview_worker.gpu_device()?;
        let extent = texture.size();
        let size = [extent.width.0, extent.height.0].map(|side| u32::try_from(side).unwrap_or(0));
        crate::preview_worker::read_texels(&device, &texture, [0, 0], size).map(Arc::new)
    }

    /// What the Info panel shows now.
    pub fn info_readout(&self) -> InfoReadout {
        let probe = self.viewer_probe;
        InfoReadout {
            pointer: probe.at,
            color: probe
                .color
                .filter(|read| Some(read.at) == probe.at)
                .map(|read| read.rgba),
            clip: self.clip_rows(),
            drag: self.drag_rows(),
            faults: self.info_faults(),
            frame: self.frame_rows(),
            playback: self.playback_info(),
        }
    }

    /// What the last rendered frame cost, row by row: the Info panel draws
    /// these rows and Last Frame Cost prints them on the status line.
    ///
    /// Recorded where the frame landed, so this is a field read: a panel drawn
    /// while a preview decode holds the session still draws. A zeroed split is
    /// no frame measured, not a frame that cost nothing, so it has no rows.
    pub(crate) fn frame_rows(&self) -> Vec<InfoRow> {
        let stats = &self.frame_stats;
        if stats.total_ms() <= 0.0 {
            return Vec::new();
        }
        let ms = |value: f64| format!("{value:.2} ms");
        let mut rows: Vec<InfoRow> = frame_stages()
            .iter()
            .map(|stage| InfoRow::new(stage.label, ms((stage.read)(stats))))
            .collect();
        rows.push(InfoRow::new("Frame", ms(stats.total_ms())));
        rows.push(InfoRow::new("Scales with size", ms(stats.scaled_cost_ms())));
        // Counts, not stages: a scene without effects has none to state, and
        // "0 passes" would read as a measurement.
        if stats.effect_passes > 0 {
            rows.push(InfoRow::new(
                "Effect passes",
                stats.effect_passes.to_string(),
            ));
            rows.push(InfoRow::new(
                "Bind groups built",
                stats.effect_pass_bind_groups.to_string(),
            ));
        }
        rows
    }

    /// The last frame's cost on the status line, as the Info panel states it.
    pub fn report_render_cost(&mut self) {
        let rows = self.frame_rows();
        self.status_message = Some(if rows.is_empty() {
            "No frame has been rendered yet".to_string()
        } else {
            rows.iter()
                .map(|row| format!("{} {}", row.label, row.value))
                .collect::<Vec<_>>()
                .join(" · ")
        });
    }

    /// The playback run, once playback has delivered a frame: a paused editor
    /// has no rate to miss.
    fn playback_info(&self) -> Option<InfoPlayback> {
        let report = self.playback_report();
        if report.is_empty() {
            return None;
        }
        Some(InfoPlayback {
            // One late or dropped frame in twenty is the machine, not the
            // editor.
            holds_rate: report.holds_rate(0.05),
            rows: [
                InfoRow::new(
                    "Rate",
                    format!("{:.1} of {:.0} fps", report.achieved_fps, report.target_fps),
                ),
                InfoRow::new(
                    "Late",
                    format!(
                        "{} of {} frames, worst {:.1} ms",
                        report.late, report.frames, report.worst_ms
                    ),
                ),
            ],
        })
    }

    fn clip_rows(&self) -> Vec<InfoRow> {
        let Some(clip) = self.selected_ui_clip_ref() else {
            return Vec::new();
        };
        let rate = self.store.sequence.frame_rate;
        let mut rows = vec![
            InfoRow::new("Name", clip.name.as_ref()),
            InfoRow::new(
                "In",
                format_ticks(clip.timeline_start_ticks, rate.num, rate.den),
            ),
            InfoRow::new(
                "Out",
                format_ticks(clip.timeline_end_ticks, rate.num, rate.den),
            ),
            InfoRow::new("Duration", clip.duration_label.as_ref()),
        ];
        let media = clip
            .media_id
            .and_then(|id| self.store.media.iter().find(|item| item.id == id));
        if let Some(media) = media {
            rows.push(InfoRow::new("Source", media.name.as_ref()));
            let (num, den) = (media.frame_rate.num, media.frame_rate.den);
            // A sound source counts its own ticks in milliseconds; a picture
            // source in frames at its own rate.
            let at = |seconds: f64| match media.kind {
                UiClipKind::Audio => format!("{seconds:.3} s"),
                _ => format_ticks(
                    (seconds * f64::from(num) / f64::from(den.max(1))).round() as i64,
                    num,
                    den,
                ),
            };
            rows.push(InfoRow::new("Source In", at(clip.source_start)));
            rows.push(InfoRow::new("Source Out", at(clip.source_end)));
        }
        let others = self.selection.len().saturating_sub(1);
        if others > 0 {
            rows.push(InfoRow::new("Also selected", format!("{others} more")));
        }
        rows
    }

    fn drag_rows(&self) -> Vec<InfoRow> {
        if let Some(trim) = &self.trim_feedback {
            let mut rows = vec![
                InfoRow::new("Tool", trim.tool_label.as_ref()),
                InfoRow::new("Clip", trim.clip_name.as_ref()),
                InfoRow::new("Change", trim.delta_label.as_ref()),
            ];
            if !trim.detail_label.is_empty() {
                rows.push(InfoRow::new("Detail", trim.detail_label.as_ref()));
            }
            return rows;
        }
        let Some(drag) = self.overlay_drag else {
            return Vec::new();
        };
        let [width, height] = self.canvas_size().map(|side| side as f32);
        let pixels =
            |delta: [f32; 2]| format!("{:+.0} px, {:+.0} px", delta[0] * width, delta[1] * height);
        let percent =
            |pair: [f32; 2]| format!("{:.1} %, {:.1} %", pair[0] * 100.0, pair[1] * 100.0);
        match drag {
            OverlayDrag::Crop { draft, .. } => vec![
                InfoRow::new("Crop at", percent([draft.x, draft.y])),
                InfoRow::new("Crop size", percent([draft.w, draft.h])),
            ],
            OverlayDrag::Mask {
                center,
                size,
                rotation,
                ..
            } => vec![
                InfoRow::new("Mask center", percent(center)),
                InfoRow::new("Mask size", percent(size)),
                InfoRow::new("Mask rotation", format!("{rotation:.1}°")),
            ],
            OverlayDrag::Transform {
                handle: None,
                start,
                draft,
                ..
            } => vec![InfoRow::new(
                "Move",
                pixels([
                    draft.position_x - start.position_x,
                    draft.position_y - start.position_y,
                ]),
            )],
            OverlayDrag::Transform {
                handle: Some(_),
                start,
                draft,
                ..
            } => vec![InfoRow::new(
                "Scale",
                format!(
                    "{:.1} % to {:.1} %",
                    start.scale_x * 100.0,
                    draft.scale_x * 100.0
                ),
            )],
            OverlayDrag::TypeHandle { start, draft, .. } => vec![InfoRow::new(
                "Handle",
                format!(
                    "{:.3}, {:.3} to {:.3}, {:.3}",
                    start[0], start[1], draft[0], draft[1]
                ),
            )],
            OverlayDrag::PathKey { start, draft, .. } => vec![InfoRow::new(
                "Move key",
                pixels([draft[0] - start[0], draft[1] - start[1]]),
            )],
            OverlayDrag::PathTangent { draft, .. } => vec![
                InfoRow::new("Tangent in", pixels(draft.in_t)),
                InfoRow::new("Tangent out", pixels(draft.out_t)),
            ],
        }
    }

    fn info_faults(&self) -> Vec<InfoFault> {
        let mut drivers: Vec<InfoFault> = aria_render::driver_faults()
            .into_iter()
            .filter_map(|fault| {
                // The record is process-wide: a clip not in this project is
                // another session's.
                let clip = self.ui_clip_ref(fault.clip)?;
                let param = aria_models::param_spec(fault.param).map_or_else(
                    || format!("{:?}", fault.param),
                    |spec| spec.label.to_string(),
                );
                let stopped = if fault.stopped { " (stopped)" } else { "" };
                Some(InfoFault {
                    source: format!("{} {param} driver", clip.name),
                    message: format!("{}{stopped}", fault.message),
                })
            })
            .collect();
        drivers.sort_by(|a, b| a.source.cmp(&b.source));
        let mounts = self.ctx.failures().into_iter().map(|failure| InfoFault {
            source: failure.name,
            message: failure.reason,
        });
        let plugins = self.ctx.faults().into_iter().map(|fault| InfoFault {
            source: fault.plugin.into_owned(),
            message: fault.message,
        });
        mounts.chain(plugins).chain(drivers).collect()
    }
}

#[cfg(test)]
#[path = "info_tests.rs"]
mod tests;
