//! Timeline Display Settings: how a clip's content strip draws video
//! thumbnails and audio waveforms, and how the view follows the playhead
//! during playback. A preference about the app, so it is stored in
//! [`crate::settings::UiSettings`] and survives a restart.

use serde::{Deserialize, Serialize};

/// Which frames a video clip's content strip shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Thumbnails {
    /// The clip's first frame at its left edge.
    Head,
    /// The first frame at the left edge and the last at the right.
    HeadAndTail,
    /// A frame every cell width along the clip.
    #[default]
    Continuous,
}

/// How an audio clip's waveform maps a peak to a bar height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveformScale {
    /// Bar height in decibels over [`WAVEFORM_FLOOR_DB`], so quiet passages
    /// stay visible.
    #[default]
    Logarithmic,
    /// Bar height proportional to the sample peak.
    Linear,
    /// No waveform: the clip draws its name and colour only, and its peaks
    /// are not decoded.
    Hidden,
}

/// How the view follows the playhead during playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Follow {
    /// The view jumps a page when the playhead nears an edge.
    #[default]
    Page,
    /// The view scrolls with the playhead held at its centre.
    Smooth,
}

/// The three display settings together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelineDisplay {
    pub thumbnails: Thumbnails,
    pub waveform: WaveformScale,
    pub follow: Follow,
}

/// The level a logarithmic waveform draws as an empty bar.
pub const WAVEFORM_FLOOR_DB: f32 = -48.0;

/// Where a page-followed playhead comes to rest: a fraction of the viewport in
/// from the left edge, so the frames about to play are already on screen.
const PAGE_REST: f32 = 0.15;

/// How near an edge the playhead may travel before a page follow moves the
/// view. Without a margin the view would move on the frame the playhead
/// touches the edge, a pixel of travel every frame instead of one jump.
const PAGE_MARGIN: f32 = 0.06;

impl Thumbnails {
    /// The menu label of this choice.
    pub fn label(self) -> &'static str {
        match self {
            Self::Head => "Head",
            Self::HeadAndTail => "Head and Tail",
            Self::Continuous => "Continuous",
        }
    }

    /// The cells of one clip `width` pixels wide, left to right.
    ///
    /// `continuous` is the `[tick, until)` spans a continuous strip draws, from
    /// [`crate::filmstrips::cell_spans`]; the head and tail forms ignore it.
    /// Neighboring continuous cells share an edge computed once, so they meet
    /// without a seam. A head or tail cell is one cell wide. A clip narrower
    /// than two cells shows its head only.
    pub fn cells(
        self,
        start: i64,
        end: i64,
        width: f32,
        cell_w: f32,
        px_per_tick: f32,
        continuous: impl FnOnce() -> Vec<(i64, i64)>,
    ) -> Vec<StripCell> {
        let head = StripCell {
            tick: start,
            from: 0.0,
            to: cell_w.min(width),
        };
        let x = |tick: i64| (tick - start) as f32 * px_per_tick;
        match self {
            Self::Head => vec![head],
            Self::HeadAndTail if end - start >= 2 && width >= 2.0 * cell_w => vec![
                head,
                StripCell {
                    tick: end - 1,
                    from: width - cell_w,
                    to: width,
                },
            ],
            Self::HeadAndTail => vec![head],
            Self::Continuous => continuous()
                .into_iter()
                .map(|(tick, until)| StripCell {
                    tick,
                    from: x(tick),
                    to: x(until),
                })
                .collect(),
        }
    }
}

/// One cell of a clip's content strip: the sequence tick whose frame it
/// shows, and the part of the clip it stands for, `[from, to)` in pixels from
/// the clip's head.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripCell {
    pub tick: i64,
    pub from: f32,
    pub to: f32,
}

impl WaveformScale {
    /// The status label of this choice.
    pub fn label(self) -> &'static str {
        match self {
            Self::Logarithmic => "logarithmic",
            Self::Linear => "linear",
            Self::Hidden => "hidden",
        }
    }

    /// The bar height, `0.0..=1.0`, for a sample peak `0.0..=1.0`.
    pub fn height(self, peak: f32) -> f32 {
        match self {
            Self::Linear => peak.clamp(0.0, 1.0),
            Self::Hidden => 0.0,
            Self::Logarithmic => {
                if peak <= 0.0 || !peak.is_finite() {
                    return 0.0;
                }
                let db = 20.0 * peak.min(1.0).log10();
                ((db - WAVEFORM_FLOOR_DB) / -WAVEFORM_FLOOR_DB).clamp(0.0, 1.0)
            }
        }
    }
}

impl Follow {
    /// The menu label of this choice.
    pub fn label(self) -> &'static str {
        match self {
            Self::Page => "Page Scroll",
            Self::Smooth => "Smooth Scroll",
        }
    }

    /// The scroll offset that keeps a playhead at `at` px in view, or `None`
    /// when the view need not move.
    pub fn target(self, at: f32, scroll: f32, width: f32) -> Option<f32> {
        if width <= 0.0 {
            return None;
        }
        let target = match self {
            Self::Page => {
                let margin = width * PAGE_MARGIN;
                if at >= scroll + margin && at <= scroll + width - margin {
                    return None;
                }
                at - width * PAGE_REST
            }
            Self::Smooth => at - width / 2.0,
        }
        .max(0.0);
        ((target - scroll).abs() >= 1.0).then_some(target)
    }

    /// Whether the view glides to the target. A page jump glides; a smooth
    /// follow moves every frame and a glide would trail the playhead.
    pub fn glides(self) -> bool {
        matches!(self, Self::Page)
    }
}

#[cfg(test)]
mod tests {
    //! # Why
    //!
    //! Each setting maps a stored choice to pixels: which frames a strip asks
    //! for, how tall a bar is, where the view scrolls. A wrong mapping draws a
    //! frame at the wrong place or a view that never follows. These tests pin
    //! each mapping at its boundaries. They do not draw.

    use super::*;

    #[test]
    fn head_and_tail_place_the_last_frame_against_the_right_edge() {
        let cell = |tick, from, to| StripCell { tick, from, to };
        let continuous = || vec![(0, 10), (10, 20)];
        assert_eq!(
            Thumbnails::Head.cells(100, 200, 400.0, 40.0, 4.0, continuous),
            vec![cell(100, 0.0, 40.0)]
        );
        assert_eq!(
            Thumbnails::HeadAndTail.cells(100, 200, 400.0, 40.0, 4.0, continuous),
            vec![cell(100, 0.0, 40.0), cell(199, 360.0, 400.0)]
        );
        assert_eq!(
            Thumbnails::HeadAndTail.cells(100, 110, 60.0, 40.0, 6.0, continuous),
            vec![cell(100, 0.0, 40.0)],
            "too narrow for two cells: the head alone"
        );
        assert_eq!(
            Thumbnails::Head.cells(100, 105, 30.0, 40.0, 6.0, continuous),
            vec![cell(100, 0.0, 30.0)],
            "a head cell spans no more than its clip"
        );
        assert_eq!(
            Thumbnails::Continuous.cells(100, 200, 400.0, 40.0, 4.0, || {
                vec![(100, 110), (110, 120), (120, 200)]
            }),
            vec![
                cell(100, 0.0, 40.0),
                cell(110, 40.0, 80.0),
                cell(120, 80.0, 400.0)
            ]
        );
    }

    #[test]
    fn a_logarithmic_waveform_lifts_quiet_peaks_and_floors_silence() {
        let log = WaveformScale::Logarithmic;
        assert_eq!(log.height(1.0), 1.0);
        assert_eq!(log.height(0.0), 0.0);
        assert_eq!(log.height(f32::NAN), 0.0);
        // -24 dB is half way to the -48 dB floor.
        assert!((log.height(10f32.powf(-24.0 / 20.0)) - 0.5).abs() < 1e-4);
        assert_eq!(log.height(10f32.powf(-60.0 / 20.0)), 0.0, "below the floor");
        assert_eq!(WaveformScale::Linear.height(0.25), 0.25);
        assert_eq!(WaveformScale::Linear.height(1.5), 1.0);
        assert_eq!(WaveformScale::Hidden.height(1.0), 0.0);
    }

    #[test]
    fn page_follow_jumps_at_the_margin_and_smooth_follow_centres() {
        let page = Follow::Page;
        assert_eq!(page.target(500.0, 0.0, 1000.0), None, "inside the view");
        assert_eq!(page.target(950.0, 0.0, 1000.0), Some(800.0));
        assert_eq!(
            page.target(100.0, 600.0, 1000.0),
            Some(0.0),
            "behind the view"
        );
        assert_eq!(page.target(500.0, 0.0, 0.0), None, "no viewport");

        let smooth = Follow::Smooth;
        assert_eq!(smooth.target(500.0, 0.0, 1000.0), None, "already centered");
        assert_eq!(smooth.target(700.0, 0.0, 1000.0), Some(200.0));
        assert_eq!(
            smooth.target(100.0, 300.0, 1000.0),
            Some(0.0),
            "clamped at 0"
        );
        assert!(page.glides() && !smooth.glides());
    }

    #[test]
    fn a_settings_file_without_the_block_reads_the_defaults() {
        let read: TimelineDisplay = serde_json::from_str("{}").expect("an empty block");
        assert_eq!(read, TimelineDisplay::default());
        let read: TimelineDisplay =
            serde_json::from_str(r#"{"thumbnails":"head_and_tail","follow":"smooth"}"#)
                .expect("a partial block");
        assert_eq!(read.thumbnails, Thumbnails::HeadAndTail);
        assert_eq!(read.waveform, WaveformScale::Logarithmic);
        assert_eq!(read.follow, Follow::Smooth);
    }
}
