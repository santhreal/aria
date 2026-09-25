//! Building the media support report.
//!
//! Split from the parent so both files stay well under the ceiling. Every row
//! here is derived at run time from a catalog or from the project: nothing in
//! this file names a container, a pixel layout or an output mode, so a row added
//! to `aria_decoder::image_catalog`, `aria_core::pixel_catalog` or
//! `OutputMode::ALL` appears in the report with no edit.
//!
//! The whole build is cheap enough for the draw thread. The two catalog walks
//! read `&'static` rows; the header read is [`SNIFF_LEN`]-bounded; the index
//! query is opened with a quarter-second lock wait so a detector mid-write
//! delays a report rather than stalling the window.

use std::time::Duration;

use aria_analysis::moments::{
    MAX_MOMENTS_QUERY_LIMIT, MomentQuery, MomentsIndex, MomentsIndexConfig,
};
use aria_core::{TimeRange, every_pixel_format, rate_eq, resample, time_sub};
use aria_decoder::{OutputMode, decoder_capability, every_image_format};
use aria_models::ClipSource;

use super::*;

/// How long the report's index query waits for a contended lock.
///
/// A quarter second: long enough to win the gap between two writes from a
/// running detector, short enough that the worst case is a visibly slow click
/// rather than a frozen window.
const INDEX_LOCK_WAIT: Duration = Duration::from_millis(250);

/// Hex characters of the content key the report shows.
///
/// Twelve is enough to tell two sources apart by eye and short enough to sit in
/// a value column. The full key is 64.
const KEY_PREFIX_CHARS: usize = 12;

impl AppState {
    /// Build the media support report.
    pub fn media_support_report(&mut self) {
        let mut lines = Vec::new();
        build_capability(&mut lines);
        build_containers(&mut lines);
        build_pixel_layouts(&mut lines);
        self.build_selected_source(&mut lines);
        self.build_coverage(&mut lines);
        let warnings = lines.iter().filter(|line| line.warn).count();
        self.media_reach.report = Some(MediaReport { lines });
        self.status_message = Some(if warnings == 0 {
            "Media support read; nothing to flag".into()
        } else {
            format!("Media support read; {warnings} thing(s) to look at")
        });
    }

    /// The selected clip's source, as the decoder reads it now.
    fn build_selected_source(&self, lines: &mut Vec<ReportLine>) {
        const SECTION: &str = "Selected source";
        let Some(item) = self.selected_media_item() else {
            lines.push(ReportLine {
                section: SECTION,
                label: "Selection".into(),
                value: "no clip selected, or the clip is generated".into(),
                warn: false,
            });
            return;
        };
        let path = std::path::PathBuf::from(&*item.path);
        lines.push(ReportLine {
            section: SECTION,
            label: item.name.to_string().into(),
            value: format!(
                "{}×{} at {}/{}, {} tick(s){}",
                item.width,
                item.height,
                item.frame_rate.num,
                item.frame_rate.den,
                item.duration_ticks,
                if item.has_audio { ", with audio" } else { "" }
            )
            .into(),
            warn: false,
        });
        if item.missing {
            lines.push(ReportLine {
                section: SECTION,
                label: "File".into(),
                value: format!("{} is not on disk", item.path).into(),
                warn: true,
            });
            return;
        }
        match sniff_image(&path) {
            Some(format) => {
                let spec = format.spec();
                let probed = probe_image(&path)
                    .map(|info| format!("{}×{}", info.width, info.height))
                    .unwrap_or_else(|err| format!("probe refused: {err}"));
                lines.push(ReportLine {
                    section: SECTION,
                    label: "Container".into(),
                    value: format!(
                        "{} ({}), probes {probed}{}",
                        spec.label,
                        spec.id,
                        if spec.animates {
                            ", animates — composites as its first frame"
                        } else {
                            ""
                        }
                    )
                    .into(),
                    warn: false,
                });
            }
            None => lines.push(ReportLine {
                section: SECTION,
                label: "Container".into(),
                value: "not a still-image container; read by the platform decoder".into(),
                warn: false,
            }),
        }
        match crate::app::analysis::content_key_for(&path) {
            Ok(key) => {
                let hex = key.encode_hex();
                let short = String::from_utf8_lossy(&hex[..KEY_PREFIX_CHARS]).into_owned();
                lines.push(ReportLine {
                    section: SECTION,
                    label: "Content key".into(),
                    value: short.into(),
                    warn: false,
                });
                lines.push(self.index_line(SECTION, key));
            }
            Err(err) => lines.push(ReportLine {
                section: SECTION,
                label: "Content key".into(),
                value: err.into(),
                warn: true,
            }),
        }
    }

    /// What the moments index holds for this source, without waiting on a
    /// detector that is mid-write.
    fn index_line(
        &self,
        section: &'static str,
        key: aria_analysis::moments::MediaContentKey,
    ) -> ReportLine {
        let config = match MomentsIndexConfig::new(1, MAX_MOMENTS_QUERY_LIMIT, INDEX_LOCK_WAIT) {
            Ok(config) => config,
            Err(err) => {
                return ReportLine {
                    section,
                    label: "Moments index".into(),
                    value: format!("{err}").into(),
                    warn: true,
                };
            }
        };
        let opened =
            MomentsIndex::open_in_cache_root_with_config(self.analysis_cache_root(), config);
        let index = match opened {
            Ok(index) => index,
            Err(err) => {
                return ReportLine {
                    section,
                    label: "Moments index".into(),
                    value: format!("{err}").into(),
                    warn: true,
                };
            }
        };
        match index.query_with_limit(&MomentQuery::new(key), MAX_MOMENTS_QUERY_LIMIT) {
            Ok(page) => {
                let count = page.into_records().len();
                ReportLine {
                    section,
                    label: "Moments index".into(),
                    value: if count >= MAX_MOMENTS_QUERY_LIMIT {
                        format!("{count}+ record(s) — at the query ceiling").into()
                    } else {
                        format!("{count} record(s)").into()
                    },
                    warn: false,
                }
            }
            Err(err) => ReportLine {
                section,
                label: "Moments index".into(),
                value: format!("{err}").into(),
                warn: true,
            },
        }
    }

    /// Clips whose source range no longer lies inside the file they point at.
    ///
    /// Placement enforces that a source range is authored at the media's own
    /// rate, so a clip whose range is at a different rate has been relinked to a
    /// file with a different cadence: that is reported on its own, because a
    /// resampled comparison is an approximation and the operator should know the
    /// number came from one.
    fn build_coverage(&self, lines: &mut Vec<ReportLine>) {
        const SECTION: &str = "Source coverage";
        let engine = self.engine.lock();
        let project = engine.project();
        let mut checked = 0_usize;
        let mut flagged = 0_usize;
        for track in project.timeline().tracks_ordered() {
            for clip in track.clips_ordered() {
                let ClipSource::Media { media, source } = &clip.content else {
                    continue;
                };
                let Some(item) = project.media(*media) else {
                    lines.push(ReportLine {
                        section: SECTION,
                        label: format!("Clip {}", clip.id.raw()).into(),
                        value: "points at a pool entry the project no longer holds".into(),
                        warn: true,
                    });
                    flagged += 1;
                    continue;
                };
                checked += 1;
                let full = item.full_range();
                let rebased = if rate_eq(source.start.rate, item.frame_rate) {
                    *source
                } else {
                    lines.push(ReportLine {
                        section: SECTION,
                        label: item.path().display().to_string().into(),
                        value: format!(
                            "the clip's range is authored at {}/{} and the file reports {}/{}; \
                             compared by resampling",
                            source.start.rate.num,
                            source.start.rate.den,
                            item.frame_rate.num,
                            item.frame_rate.den
                        )
                        .into(),
                        warn: true,
                    });
                    flagged += 1;
                    TimeRange {
                        start: resample(source.start, item.frame_rate),
                        duration: resample(source.duration, item.frame_rate),
                    }
                };
                if let Some(overhang) = overhang(rebased, full) {
                    lines.push(ReportLine {
                        section: SECTION,
                        label: item.path().display().to_string().into(),
                        value: overhang.into(),
                        warn: true,
                    });
                    flagged += 1;
                }
            }
        }
        lines.push(ReportLine {
            section: SECTION,
            label: "Checked".into(),
            value: format!("{checked} media clip(s), {flagged} to look at").into(),
            warn: false,
        });
    }
}

/// How much of `source` falls outside `full`, as a sentence, or `None` when it
/// is fully covered.
fn overhang(source: TimeRange, full: TimeRange) -> Option<String> {
    let seconds = |ticks: i64, rate: aria_core::Rational| ticks as f64 * rate.seconds_per_unit();
    let overlap = source.intersection(full).ok()?;
    let Some(overlap) = overlap else {
        return Some(format!(
            "the clip plays ticks {}..{} and the file holds {}..{} — no overlap at all",
            source.start.value,
            source.end_tick(),
            full.start.value,
            full.end_tick()
        ));
    };
    if overlap == source {
        return None;
    }
    let source_end = source.end().ok()?;
    let full_end = full.end().ok()?;
    let past = time_sub(&source_end, &full_end).ok()?;
    if past.value > 0 {
        return Some(format!(
            "reaches {:.2}s past the end of the file ({} tick(s))",
            seconds(past.value, past.rate),
            past.value
        ));
    }
    let before = time_sub(&full.start, &source.start).ok()?;
    Some(format!(
        "starts {:.2}s before the file does ({} tick(s))",
        seconds(before.value, before.rate),
        before.value
    ))
}

/// Which backend this build dispatches to, and what it can hand back.
fn build_capability(lines: &mut Vec<ReportLine>) {
    const SECTION: &str = "Build";
    let capability = decoder_capability();
    lines.push(ReportLine {
        section: SECTION,
        label: "Decoder backend".into(),
        value: capability.backend.to_string().into(),
        warn: false,
    });
    for mode in OutputMode::ALL {
        let supported = capability.supports(mode);
        let fallback = capability.best(mode);
        lines.push(ReportLine {
            section: SECTION,
            label: mode.label().to_string().into(),
            value: match (supported, fallback) {
                (true, _) => "available".into(),
                (false, Some(other)) => {
                    format!("unavailable; falls back to {}", other.label()).into()
                }
                (false, None) => "unavailable, and there is no fallback".into(),
            },
            warn: !supported,
        });
    }
}

/// Every still-image container the decoder dispatches on, with the columns that
/// decide what can be done with it.
fn build_containers(lines: &mut Vec<ReportLine>) {
    const SECTION: &str = "Containers";
    for format in every_image_format() {
        let spec = format.spec();
        let mut reach: Vec<&str> = Vec::new();
        if spec.portable_probe.is_some() {
            reach.push("probe");
        }
        if spec.portable_decode.is_some() {
            reach.push("path");
        }
        if spec.portable_decode_bytes.is_some() {
            reach.push("memory");
        }
        if spec.portable_animate.is_some() {
            reach.push("animate");
        }
        let portable = if reach.is_empty() {
            "platform backend only".to_string()
        } else {
            format!("portable: {}", reach.join(", "))
        };
        lines.push(ReportLine {
            section: SECTION,
            label: spec.label.to_string().into(),
            value: format!(
                ".{} · {} · {portable}",
                spec.extensions.join(" ."),
                if spec.animates {
                    "multi-frame"
                } else {
                    "single frame"
                }
            )
            .into(),
            // A container that animates with no portable animated decoder shows
            // its first frame off-platform. That is the honest answer, not a
            // defect, but it is worth stating before an export.
            warn: spec.animates && spec.portable_animate.is_none(),
        });
    }
}

/// Every plane layout that can cross the decode seam.
fn build_pixel_layouts(lines: &mut Vec<ReportLine>) {
    const SECTION: &str = "Pixel layouts";
    for format in every_pixel_format() {
        let spec = format.spec();
        let sampling = spec
            .subsampling
            .map(|subsampling| subsampling.label().to_string())
            .unwrap_or_else(|| "no chroma plane".to_string());
        lines.push(ReportLine {
            section: SECTION,
            label: spec.label.to_string().into(),
            value: format!(
                "{} plane(s) · {}-bit in {} byte(s) · {sampling} · {}{}",
                spec.planes,
                spec.bit_depth,
                spec.bytes_per_sample,
                if spec.is_yuv { "YUV" } else { "RGB" },
                if format.requires_even_dimensions() {
                    " · even dimensions"
                } else {
                    ""
                }
            )
            .into(),
            warn: false,
        });
    }
}
