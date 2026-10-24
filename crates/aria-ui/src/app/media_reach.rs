//! Media support: what this build can decode, and what the selected source is.
//!
//! Six backend capabilities shipped without a way to reach them. Each one
//! answers a question to settle before an export, and each one had to be
//! answered by guessing:
//!
//! - `aria_decoder::decoder_capability` names the backend this build dispatches
//!   to and whether it hands back GPU surfaces or CPU planes. Nothing drew it,
//!   so "why is playback on the slow path here" had no answer in the editor.
//! - `every_image_format` / `every_pixel_format` enumerate the two catalogs the
//!   decoder actually dispatches on. Absent a listing, the only way to find out
//!   whether a build reads HEIC or 10-bit 4:2:2 was to import a file and see.
//! - `sniff_bytes`, `decode_image`, `decode_image_bytes`, `decode_animation` and
//!   `decode_animation_file` are five entry points onto the same file. A still
//!   that opens from a path and fails from memory is a real defect (the two go
//!   through different catalog columns) and it used to surface at export time.
//! - `audio_peaks` folds a whole stream into a fixed number of buckets, so the
//!   overview costs the same for a ten-second clip and a two-hour interview.
//!   The timeline uses the per-second twin, which is proportional to duration
//!   and therefore useless as a "where is the loud part" answer.
//! - `normalisation_gain` with `db_from_linear` / `linear_from_db` turns a
//!   measured level into a clip gain. The measurement was drawn by the Analysis
//!   panel and then went nowhere.
//! - `TimeRange::intersection` and `rate_eq` prove a clip's source range still
//!   lies inside the file it points at. Relinking to a shorter file leaves a
//!   clip reaching past the end, which used to fail at render time only.
//!
//! Everything here is either cheap enough for the draw thread (a header read, a
//! catalog walk, one index query with a short lock wait) or it goes to the job
//! board. `verify_selected_decode` and `file_loudness_overview` decode whole
//! files, so they are jobs: the completion detail is the answer, and the jobs
//! panel is where it lands.

use std::path::PathBuf;

use aria_analysis::scopes::{chroma709, luma709};
use aria_analysis::{
    MAX_NORMALISATION_BOOST_DB, Rgba8Frame, ScopeConfig, db_from_linear, linear_from_db,
    measure_scopes, normalisation_gain,
};
use aria_core::count::counted;
use aria_decoder::{
    LottieAnimation, decode_animation, decode_animation_file, decode_image, decode_image_bytes,
    probe_image, sniff_bytes, sniff_image,
};
use aria_jobs::JobCompletion;
use aria_models::{Generator, MAX_CLIP_VOLUME};

use super::AppState;
use super::picker::{Answer, Ask, FileSpec};
use crate::types::UiMediaItem;
use gpui::SharedString;

#[path = "media_reach_jobs.rs"]
mod jobs;
#[path = "media_reach_report.rs"]
mod report;

/// Widest still this build will fully decode on the job board, in pixels.
///
/// The gate is on decoded area rather than file size because a 40 kB PNG can
/// decompress to a gigabyte. Sixteen megapixels is above every camera still and
/// far below the point where a verify becomes a memory event.
const MAX_VERIFY_PIXELS: u64 = 16 << 20;

/// Buckets the whole-file loudness overview folds a stream into.
///
/// Fixed rather than proportional: the point of this readout is that the same
/// number of buckets covers the whole file whatever its length, so "63% of the
/// way in" means the same thing for every source.
const OVERVIEW_BUCKETS: usize = 200;

/// The level a bucket has to reach to count as loud, in dBFS.
const LOUD_THRESHOLD_DBFS: f32 = -6.0;

/// One row of the support report.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportLine {
    /// Which block of the panel this row belongs under. The panel derives its
    /// headings from the rows in order, so a new section needs no second list.
    pub section: &'static str,
    pub label: SharedString,
    pub value: SharedString,
    /// Whether the row states a problem rather than a fact.
    pub warn: bool,
}

/// What the last report read.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaReport {
    pub lines: Vec<ReportLine>,
}

impl MediaReport {
    /// The sections present, in the order their first row appears.
    ///
    /// Read from the rows rather than declared, so a section added by a row is
    /// drawn without editing the panel.
    pub fn sections(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for line in &self.lines {
            if !out.contains(&line.section) {
                out.push(line.section);
            }
        }
        out
    }

    /// Rows that state a problem, for the panel's summary line.
    pub fn warnings(&self) -> usize {
        self.lines.iter().filter(|line| line.warn).count()
    }
}

/// The exposure readout: what a pass over the frame on screen measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exposure {
    /// Mean luma code over every column that received samples.
    pub mean_luma: f32,
    /// The darkest and brightest column, as a fraction across the picture and
    /// the luma code there.
    pub darkest: (f32, f32),
    pub brightest: (f32, f32),
    /// Mean chroma of the frame on the Cb/Cr plane. Both near zero is neutral.
    pub cast: (f32, f32),
    /// Luma of mid grey under the same BT.709 weights, as the reference the
    /// mean is read against.
    pub neutral: u8,
    pub sampled: u32,
    pub timestamp_seconds: f64,
}

/// The Media Support dialog's state.
pub struct MediaReachForm {
    /// The last report built, or `None` before one was asked for.
    pub report: Option<MediaReport>,
    /// The level `normalise_selected_clip` aims the selection at, in dBFS.
    pub target_dbfs: f32,
    /// The last exposure pass, or `None` before one was asked for.
    pub exposure: Option<Exposure>,
}

impl Default for MediaReachForm {
    fn default() -> Self {
        Self {
            report: None,
            // Broadcast-ish and the middle of `TARGETS`, so the first click in
            // either direction is a real choice rather than a no-op at an end.
            target_dbfs: -14.0,
            exposure: None,
        }
    }
}

impl MediaReachForm {
    /// The levels the target chip cycles, quietest first.
    ///
    /// Streaming platforms normalise to a band around -14 LUFS; -23 is the EBU
    /// broadcast figure; -6 is as hot as a peak target goes before a limiter is
    /// doing the work instead.
    pub const TARGETS: [f32; 5] = [-23.0, -18.0, -14.0, -9.0, -6.0];

    /// What the chip reads.
    pub fn target_label(&self) -> String {
        format!("{:.0} dBFS", self.target_dbfs)
    }

    /// Which of [`Self::TARGETS`] is showing, so the panel's dots mark the live
    /// one. `None` only if the field was set to something off the table.
    pub fn target_at(&self) -> Option<usize> {
        Self::TARGETS
            .iter()
            .position(|level| (*level - self.target_dbfs).abs() < f32::EPSILON)
    }
}

impl AppState {
    /// The pool entry the selected clip plays, when it plays one.
    pub(super) fn selected_media_item(&self) -> Option<&UiMediaItem> {
        let media = self.selected_ui_clip()?.media_id?;
        self.store.media.iter().find(|item| item.id == media)
    }

    /// Step the normalise target to the next level up, wrapping at the top.
    pub fn cycle_normalise_target(&mut self) {
        let at = self.media_reach.target_at().unwrap_or(0);
        let next = (at + 1) % MediaReachForm::TARGETS.len();
        self.media_reach.target_dbfs = MediaReachForm::TARGETS[next];
    }

    /// Set the selected clip's gain so its measured peak lands on the target.
    ///
    /// Reads the loudness contour the Analysis panel measured, rather than
    /// measuring again: the contour is the only measurement in the session that
    /// covers the whole clip, and normalising off a single frame would be a
    /// different and worse answer.
    pub fn normalise_selected_clip(&mut self) {
        let Some(ui) = self.selected_ui_clip() else {
            self.status_message = Some("Select a clip to normalise".into());
            return;
        };
        let Some(measured) = self.loudness_peak_dbfs() else {
            self.status_message =
                Some("Measure loudness in the Analysis panel first, then normalise".into());
            return;
        };
        let target = self.media_reach.target_dbfs;
        let gain = match normalisation_gain(measured, target) {
            Ok(gain) => gain,
            Err(err) => {
                self.status_message = Some(format!("Cannot normalise: {err}"));
                return;
            }
        };
        let applied = gain * ui.volume.max(0.0);
        if let Err(err) = self.set_selected_volume(applied) {
            self.report_error(&err);
            return;
        }
        // What the clip ended up at, not what was asked for: the gain caps at
        // `MAX_NORMALISATION_BOOST_DB` and the clip volume caps again, so the
        // sentence has to be read back off the number that landed.
        let landed = applied.min(MAX_CLIP_VOLUME);
        let wanted = target - measured;
        let capped = wanted > MAX_NORMALISATION_BOOST_DB;
        let level = db_from_linear(landed)
            .map(|db| format!("{db:+.1} dB"))
            .unwrap_or_else(|| "silence".to_string());
        self.status_message = Some(if capped {
            format!(
                "Normalised to {level}; the {wanted:+.1} dB this needed is over the \
                 {MAX_NORMALISATION_BOOST_DB:+.0} dB boost cap"
            )
        } else {
            format!("Normalised to {level} for {target:.0} dBFS from {measured:.1} dBFS")
        });
    }

    /// Measure the frame on screen and keep the readout.
    ///
    /// The scope strip paints textures and drops the measurement, so this takes
    /// its own pass. Same frame, same config: the numbers under the picture and
    /// the picture agree because both come from the frame on screen, read out
    /// of its GPU texture when it has no CPU copy.
    pub fn measure_exposure(&mut self) {
        let Some(frame) = self.picture_on_screen() else {
            self.status_message = Some("No frame on screen to measure".into());
            return;
        };
        let Ok(view) = Rgba8Frame::new(
            &frame.pixels,
            frame.width as usize,
            frame.height as usize,
            frame.width as usize * 4,
            0.0,
        ) else {
            self.status_message = Some("The frame on screen is not readable as RGBA8".into());
            return;
        };
        let scopes = measure_scopes(view, ScopeConfig::default());
        let columns = scopes.waveform.columns();
        let mut total = 0.0_f32;
        let mut counted = 0_u32;
        let mut darkest = (0.0_f32, f32::MAX);
        let mut brightest = (0.0_f32, f32::MIN);
        for column in 0..columns {
            let Some(mean) = scopes.waveform.column_mean(column) else {
                continue;
            };
            total += mean;
            counted += 1;
            let across = if columns > 1 {
                column as f32 / (columns - 1) as f32
            } else {
                0.0
            };
            if mean < darkest.1 {
                darkest = (across, mean);
            }
            if mean > brightest.1 {
                brightest = (across, mean);
            }
        }
        if counted == 0 {
            self.status_message = Some("Nothing in the frame carried opaque pixels".into());
            return;
        }
        self.media_reach.exposure = Some(Exposure {
            mean_luma: total / counted as f32,
            darkest,
            brightest,
            cast: mean_cast(&scopes.histogram),
            neutral: luma709(128, 128, 128),
            sampled: scopes.sampled,
            timestamp_seconds: scopes.timestamp_seconds,
        });
    }

    /// Pick a Lottie composition and place it at its own intrinsic size.
    ///
    /// The generator carries a width and a height, and every existing way in
    /// filled them from the canvas, because the composition's real size is
    /// inside the JSON and nothing read it. This reads it, and rasterizes the
    /// first sampled frame before placing, so a composition this build cannot
    /// draw is refused at import instead of appearing as an empty rectangle.
    pub fn import_lottie(&mut self) {
        let spec = FileSpec::new("Import Lottie Animation").filter("Lottie", &["json", "lottie"]);
        self.request_pick(Ask::OpenFile(spec), |state, answer: Answer| {
            let Some(path) = answer.one() else {
                return;
            };
            let mut animation = match LottieAnimation::load(&path) {
                Ok(animation) => animation,
                Err(err) => {
                    state.status_message = Some(format!("{}: {err}", path.display()));
                    return;
                }
            };
            let (width, height) = animation.intrinsic_size();
            let first = animation.frame_index_at(0.0);
            if let Err(err) = animation.render_frame(first) {
                state.status_message =
                    Some(format!("{} does not rasterize: {err}", path.display()));
                return;
            }
            let seconds = animation.duration_seconds();
            let frames = animation.frame_count();
            let generator = Generator::lottie(path.to_string_lossy().to_string(), width, height);
            if let Err(err) = state.add_generated(generator) {
                state.report_error(&err);
                return;
            }
            state.status_message = Some(format!(
                "Placed {width}×{height} Lottie, {} over {seconds:.2}s",
                counted(frames, "frame", "frames")
            ));
        });
    }

    /// Import stills and animations, offering every container the decoder
    /// actually recognizes.
    ///
    /// The pool's own import filter is a hand-written list that predates half
    /// the catalog, so a GIF or a HEIC the decoder reads fine was not offered by
    /// the dialog. This filter is `image_extensions()`, which is the catalog's
    /// own extension column: a container added upstream becomes pickable with no
    /// edit here and none in the dialog.
    pub fn import_still_image(&mut self) {
        let extensions = aria_decoder::image_extensions();
        let spec = FileSpec::new("Import Still or Animation").filter("Images", &extensions);
        self.request_pick(Ask::OpenFiles(spec), |state, answer: Answer| {
            let paths = answer.all();
            if paths.is_empty() {
                return;
            }
            let wanted = paths.len();
            let mut imported = 0_usize;
            for path in paths {
                match state.import_media(path) {
                    Ok(_) => imported += 1,
                    Err(err) => {
                        state.report_error(&err);
                        break;
                    }
                }
            }
            state.status_message = Some(format!(
                "Imported {imported} of {}",
                counted(wanted, "file", "files")
            ));
        });
    }
}

/// Mean chroma of a frame, from the histogram's per-channel bins.
///
/// The bins are a distribution over code values, so the mean code per channel
/// is the bin index weighted by its count. Feeding those three means through the
/// same BT.709 matrix the vectorscope uses gives the frame's average cast, which
/// is the number a colour decision is made on — a vectorscope shows the spread
/// and hides the centre.
fn mean_cast(histogram: &aria_analysis::Histogram) -> (f32, f32) {
    let channel = |bins: &[u32]| -> u8 {
        let count: u64 = bins.iter().map(|hits| u64::from(*hits)).sum();
        if count == 0 {
            return 0;
        }
        let total: u64 = bins
            .iter()
            .enumerate()
            .map(|(bin, hits)| bin as u64 * u64::from(*hits))
            .sum();
        (total / count).min(255) as u8
    };
    chroma709(
        channel(&histogram.red),
        channel(&histogram.green),
        channel(&histogram.blue),
    )
}
