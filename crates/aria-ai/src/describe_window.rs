//! Which slice of the timeline a description covers, and what it left out.
//!
//! The description is pushed into every prompt, so its size is a running cost
//! rather than a one-off. A long project is where that stops being free: a
//! two-thousand-clip timeline serialized whole is most of a context window, and
//! the clips a request is about are almost always the ones under the playhead
//! or inside the range someone named.
//!
//! So a description is planned before it is built. Under the budget nothing is
//! windowed and the output is the whole project, byte for byte what it always
//! was. Over the budget the plan keeps the clips nearest the playhead, reports
//! the span they cover, and says how many it dropped. A caller that wants a
//! different slice asks for one by name.
//!
//! The count is the honest part. A window that quietly held back half the
//! timeline would make the model confident about a project it cannot see, so
//! every description that is not whole carries its own bounds, its clip count,
//! and the number of clips and markers outside it.

use std::borrow::Cow;

use aria_models::{Project, Rational};
use serde::{Deserialize, Serialize};

/// The most clips one description carries before it is windowed.
///
/// Measured against the prompt, not the timeline: at roughly 60 to 120 tokens
/// per clip, three hundred clips is already a large fraction of a small local
/// model's context, and every turn pays for it again.
pub const CLIP_BUDGET: usize = 300;

/// What a caller wants described.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SummaryRequest {
    /// The slice the session implies: the whole timeline when it fits
    /// [`CLIP_BUDGET`], otherwise the clips nearest the playhead.
    Session {
        /// Where the operator is looking, in seconds. A windowed description is
        /// centred on it.
        playhead_seconds: f64,
    },
    /// A named span, in seconds, because someone asked for it. Ends outside the
    /// timeline are clamped to it, a reversed pair is read in the order the
    /// timeline runs, and a non-finite end is read as the edge of the project:
    /// a range that cannot be read is still a question about somewhere.
    Range {
        /// Start of the span, in seconds.
        from_seconds: f64,
        /// End of the span, in seconds.
        to_seconds: f64,
    },
}

/// Why a description is not the whole project.
///
/// One reason per cause, and each has to be reachable: a reason nobody can
/// produce is a sentence the model never reads, and two reasons sharing a
/// wording is a distinction the model cannot act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowReason {
    /// The project holds more clips than one description carries.
    ClipBudget,
    /// A span was named, so only that span is described.
    RangeAsked,
}

impl WindowReason {
    /// Every reason a description can be windowed.
    pub const ALL: [Self; 2] = [Self::ClipBudget, Self::RangeAsked];

    /// The wording the model reads.
    pub const fn text(self) -> &'static str {
        match self {
            Self::ClipBudget => "the project holds more clips than one description carries",
            Self::RangeAsked => "a range was asked for",
        }
    }
}

/// The slice a windowed description covers, and the size of what it dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowSummary {
    /// Start of the described span, in seconds.
    pub start_seconds: f64,
    /// End of the described span, in seconds.
    pub end_seconds: f64,
    /// Exact start in frames at the project rate.
    pub start_frames: i64,
    /// Exact end in frames at the project rate.
    pub end_frames: i64,
    /// How many clips this description carries.
    pub described_clips: usize,
    /// How many clips the project holds that this description does not. Ask for
    /// a range to read them.
    pub omitted_clips: usize,
    /// How many ruler markers fall outside the span.
    pub omitted_markers: usize,
    /// Why the description is windowed.
    pub reason: Cow<'static, str>,
}

/// A planned window: the span, the clips kept, and the size of the remainder.
///
/// `kept` is ascending so the builder can binary-search it per clip instead of
/// hashing every id twice.
pub(crate) struct Plan {
    /// First frame described.
    pub start_frames: i64,
    /// Last frame described.
    pub end_frames: i64,
    /// Ids of the clips this description carries, ascending.
    pub kept: Vec<u64>,
    /// Clips the project holds that this description does not.
    pub omitted_clips: usize,
    /// Why the description is windowed.
    pub reason: WindowReason,
}

impl Plan {
    /// Whether `id` is one of the clips this description carries.
    pub(crate) fn keeps(&self, id: u64) -> bool {
        self.kept.binary_search(&id).is_ok()
    }

    /// Whether `tick` falls inside the described span.
    pub(crate) const fn holds(&self, tick: i64) -> bool {
        tick >= self.start_frames && tick <= self.end_frames
    }
}

/// One clip's extent, as the planner ranks it.
struct Span {
    /// The clip's id.
    id: u64,
    /// Timeline start, in frames.
    start: i64,
    /// Timeline end, in frames.
    end: i64,
}

/// How far `at` sits outside `[start, end]`, in frames. Zero inside.
const fn distance(start: i64, end: i64, at: i64) -> i64 {
    if at < start {
        start - at
    } else if at > end {
        at - end
    } else {
        0
    }
}

/// Read a span bound in seconds as a frame index, clamping what cannot be read.
///
/// A non-finite or absurd bound is a malformed question rather than a reason to
/// describe nothing, so it resolves to `fallback` (the edge of the timeline).
fn frame_at(seconds: f64, rate: Rational, fallback: i64) -> i64 {
    if !seconds.is_finite() {
        return fallback;
    }
    let frames = seconds * f64::from(rate.num) / f64::from(rate.den);
    if !(-(2f64.powi(53))..=2f64.powi(53)).contains(&frames) {
        return fallback;
    }
    frames.round().max(0.0) as i64
}

/// Every clip on the timeline, with its extent, in track then start order.
fn spans(project: &Project) -> Vec<Span> {
    let mut spans = Vec::new();
    for track in project.timeline().tracks_ordered() {
        for clip in track.clips_ordered() {
            let start = clip.timeline.start.value;
            spans.push(Span {
                id: clip.id.raw(),
                start,
                end: start + clip.timeline.duration.value,
            });
        }
    }
    spans
}

/// Keep the `budget` spans nearest `at`, and report the rest.
///
/// Ranking is by distance, then by start, then by id, so the same project and
/// the same playhead always produce the same description: an eval that asserts
/// a windowed summary verbatim has to stay assertable.
fn nearest(mut spans: Vec<Span>, at: i64, budget: usize) -> (Vec<u64>, i64, i64, usize) {
    spans.sort_by_key(|span| (distance(span.start, span.end, at), span.start, span.id));
    let omitted = spans.len().saturating_sub(budget);
    spans.truncate(budget);
    let start = spans.iter().map(|span| span.start).min().unwrap_or(0);
    let end = spans.iter().map(|span| span.end).max().unwrap_or(0);
    let mut kept: Vec<u64> = spans.iter().map(|span| span.id).collect();
    kept.sort_unstable();
    (kept, start, end, omitted)
}

/// Read a `describe_project` call's arguments as the slice it asks for.
///
/// Either bound alone names a span: a start with no end runs to the end of the
/// project, an end with no start from its beginning. Neither bound is the
/// session's own window, which is how the tool behaved before it took any
/// arguments at all. A bound that is not a number is not a bound, so a model
/// that sends `"from_seconds": "start"` gets the session window rather than a
/// rejection: the tool is the one that reads state, and refusing to read is a
/// worse answer than reading the default slice.
pub fn requested(arguments: &serde_json::Value, playhead_seconds: f64) -> SummaryRequest {
    let read = |key: &str| arguments.get(key).and_then(serde_json::Value::as_f64);
    match (read("from_seconds"), read("to_seconds")) {
        (None, None) => SummaryRequest::Session { playhead_seconds },
        (from, to) => SummaryRequest::Range {
            from_seconds: from.unwrap_or(0.0),
            to_seconds: to.unwrap_or(f64::INFINITY),
        },
    }
}

/// Choose the window for `request`, or `None` to describe the whole project.
pub(crate) fn plan(project: &Project, request: SummaryRequest) -> Option<Plan> {
    let rate = project.timeline().frame_rate;
    match request {
        SummaryRequest::Session { playhead_seconds } => {
            let spans = spans(project);
            if spans.len() <= CLIP_BUDGET {
                return None;
            }
            let at = frame_at(playhead_seconds, rate, 0);
            let (kept, start_frames, end_frames, omitted_clips) = nearest(spans, at, CLIP_BUDGET);
            Some(Plan {
                start_frames,
                end_frames,
                kept,
                omitted_clips,
                reason: WindowReason::ClipBudget,
            })
        }
        SummaryRequest::Range {
            from_seconds,
            to_seconds,
        } => {
            let all = spans(project);
            let content_end = all.iter().map(|span| span.end).max().unwrap_or(0);
            let first = frame_at(from_seconds, rate, 0);
            let last = frame_at(to_seconds, rate, content_end);
            let (start_frames, end_frames) = if first <= last {
                (first, last)
            } else {
                (last, first)
            };
            // A named span is described whole, up to the same budget: the model
            // can ask for a narrower one, and a range wide enough to blow the
            // budget is the budget case wearing a different hat.
            let total = all.len();
            let inside: Vec<Span> = all
                .into_iter()
                .filter(|span| span.start < end_frames && span.end > start_frames)
                .collect();
            let outside = total - inside.len();
            let centre = start_frames + (end_frames - start_frames) / 2;
            let (kept, _, _, over_budget) = nearest(inside, centre, CLIP_BUDGET);
            Some(Plan {
                start_frames,
                end_frames,
                kept,
                omitted_clips: outside + over_budget,
                reason: WindowReason::RangeAsked,
            })
        }
    }
}

#[cfg(test)]
#[path = "describe_window_tests.rs"]
mod tests;
