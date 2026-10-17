//! Per-character-range style runs ([`TextStyleRun`]) on [`TextStyle`].

use serde::{Deserialize, Serialize};

use super::{MAX_TEXT_LETTER_SPACING, MAX_TEXT_SIZE, TextStyle, validate_range};
use crate::error::ModelError;

pub const TEXT_RUN_VERSION: u32 = 1;
pub const MAX_TEXT_RUNS: usize = 1024;

const fn default_text_run_version() -> u32 {
    TEXT_RUN_VERSION
}

fn deserialize_text_run_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = u32::deserialize(deserializer)?;
    if v != TEXT_RUN_VERSION {
        return Err(serde::de::Error::custom(format!(
            "unsupported text style run version {v} (expected {TEXT_RUN_VERSION})"
        )));
    }
    Ok(v)
}

/// Per-character-range style override on a [`TextStyle`].
///
/// `start..end` are character offsets (`char` indices, `start < end`). Any
/// `Some` field overrides the clip-wide default on [`TextStyle`] for that
/// character range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyleRun {
    pub start: usize,
    pub end: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<[u8; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter_spacing: Option<f32>,
    #[serde(
        default = "default_text_run_version",
        deserialize_with = "deserialize_text_run_version"
    )]
    pub version: u32,
}

impl TextStyleRun {
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            font: None,
            size: None,
            bold: None,
            italic: None,
            underline: None,
            fill: None,
            letter_spacing: None,
            version: TEXT_RUN_VERSION,
        }
    }

    fn same_overrides(&self, other: &Self) -> bool {
        self.font == other.font
            && self.size == other.size
            && self.bold == other.bold
            && self.italic == other.italic
            && self.underline == other.underline
            && self.fill == other.fill
            && self.letter_spacing == other.letter_spacing
    }
}

impl TextStyle {
    pub(super) fn validate_runs(&self) -> Result<(), ModelError> {
        if self.runs.len() > MAX_TEXT_RUNS {
            return Err(ModelError::InvalidParam(format!(
                "text style has {} runs (max {MAX_TEXT_RUNS})",
                self.runs.len()
            )));
        }
        for run in &self.runs {
            if run.version != TEXT_RUN_VERSION {
                return Err(ModelError::InvalidParam(format!(
                    "unsupported text style run version {} (expected {TEXT_RUN_VERSION})",
                    run.version
                )));
            }
            if run.start >= run.end {
                return Err(ModelError::InvalidParam(format!(
                    "text style run range {}..{} must have start < end",
                    run.start, run.end
                )));
            }
            if let Some(size) = run.size {
                validate_range(size, f32::EPSILON, MAX_TEXT_SIZE, "text run size")?;
            }
            if let Some(ls) = run.letter_spacing
                && (!ls.is_finite() || ls.abs() > MAX_TEXT_LETTER_SPACING)
            {
                return Err(ModelError::InvalidParam(format!(
                    "text run letter spacing must be finite and within -{MAX_TEXT_LETTER_SPACING}..={MAX_TEXT_LETTER_SPACING} reference px"
                )));
            }
        }
        Ok(())
    }

    /// Apply `update` to the character range `start..end`, splitting any
    /// partially-overlapping runs so existing overrides outside or inside the
    /// selection are preserved.
    pub fn apply_range_override(
        &mut self,
        start: usize,
        end: usize,
        mut update: impl FnMut(&mut TextStyleRun),
    ) {
        if start >= end {
            return;
        }
        let mut cuts = vec![start, end];
        for r in &self.runs {
            if r.start > start && r.start < end {
                cuts.push(r.start);
            }
            if r.end > start && r.end < end {
                cuts.push(r.end);
            }
        }
        cuts.sort_unstable();
        cuts.dedup();

        let mut split_runs = Vec::with_capacity(self.runs.len() + cuts.len());
        for r in self.runs.drain(..) {
            if r.end <= start || r.start >= end {
                split_runs.push(r);
                continue;
            }
            if r.start < start {
                let mut left = r.clone();
                left.end = start;
                split_runs.push(left);
            }
            for pair in cuts.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if r.start <= a && r.end >= b {
                    let mut mid = r.clone();
                    mid.start = a;
                    mid.end = b;
                    split_runs.push(mid);
                }
            }
            if r.end > end {
                let mut right = r;
                right.start = end;
                split_runs.push(right);
            }
        }

        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if let Some(existing) = split_runs.iter_mut().find(|r| r.start == a && r.end == b) {
                update(existing);
            } else {
                let mut run = TextStyleRun::new(a, b);
                update(&mut run);
                split_runs.push(run);
            }
        }
        split_runs.sort_by_key(|r| (r.start, r.end));
        let mut merged: Vec<TextStyleRun> = Vec::with_capacity(split_runs.len());
        for r in split_runs {
            if let Some(prev) = merged.last_mut()
                && prev.end == r.start
                && prev.same_overrides(&r)
            {
                prev.end = r.end;
            } else {
                merged.push(r);
            }
        }
        self.runs = merged;
    }

    /// Shift or trim character-range runs when `edit_start..old_end` in the
    /// clip's text is replaced by `inserted_chars` characters.
    ///
    /// Inserted text takes the style of the character it is typed after: a
    /// run ending at the caret grows over it, and a run starting at the caret
    /// moves past it. At offset 0 there is no character before, so the text
    /// takes the style of the run that starts there. Text typed over a range
    /// takes the style of the range's first character. Runs stay disjoint.
    pub fn adjust_runs_for_edit(
        &mut self,
        edit_start: usize,
        old_end: usize,
        inserted_chars: usize,
    ) {
        let old_end = old_end.max(edit_start);
        let removed = old_end - edit_start;
        let map_pos = |pos: usize, is_start: bool| -> usize {
            if pos < edit_start {
                pos
            } else if pos == edit_start {
                let shift = removed == 0 && (!is_start || edit_start > 0);
                if shift { pos + inserted_chars } else { pos }
            } else if pos >= old_end {
                pos - removed + inserted_chars
            } else {
                edit_start + inserted_chars
            }
        };
        for r in &mut self.runs {
            r.start = map_pos(r.start, true);
            r.end = map_pos(r.end, false);
        }
        self.runs.retain(|r| r.start < r.end);
    }

    /// The run holding character `index`, `None` where the clip-wide style
    /// applies.
    pub fn run_at(&self, index: usize) -> Option<&TextStyleRun> {
        self.runs.iter().find(|r| r.start <= index && index < r.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Generator, Mask, MaskKind, Project, Rational, ShapePathPoint, TimeRange, TrackKind,
    };

    #[test]
    fn overlapping_range_overrides_split_and_compose() {
        let mut style = TextStyle::default();
        style.apply_range_override(0, 6, |r| r.bold = Some(true));
        style.apply_range_override(2, 4, |r| r.fill = Some([255, 0, 0, 255]));
        assert_eq!(style.runs.len(), 3);
        assert_eq!(
            (
                style.runs[0].start,
                style.runs[0].end,
                style.runs[0].bold,
                style.runs[0].fill
            ),
            (0, 2, Some(true), None)
        );
        assert_eq!(
            (
                style.runs[1].start,
                style.runs[1].end,
                style.runs[1].bold,
                style.runs[1].fill
            ),
            (2, 4, Some(true), Some([255, 0, 0, 255]))
        );
        assert_eq!(
            (
                style.runs[2].start,
                style.runs[2].end,
                style.runs[2].bold,
                style.runs[2].fill
            ),
            (4, 6, Some(true), None)
        );
    }

    #[test]
    fn adjust_runs_for_edit_shifts_and_drops_deleted_spans() {
        let mut style = TextStyle::default();
        style.apply_range_override(2, 5, |r| r.italic = Some(true));
        style.apply_range_override(6, 9, |r| r.underline = Some(true));
        style.adjust_runs_for_edit(0, 0, 2);
        assert_eq!((style.runs[0].start, style.runs[0].end), (4, 7));
        assert_eq!((style.runs[1].start, style.runs[1].end), (8, 11));
        style.adjust_runs_for_edit(4, 7, 0);
        assert_eq!(style.runs.len(), 1);
        assert_eq!((style.runs[0].start, style.runs[0].end), (5, 8));
    }

    fn spans(style: &TextStyle) -> Vec<(usize, usize)> {
        style.runs.iter().map(|r| (r.start, r.end)).collect()
    }

    /// Typed text takes the style of the character before the caret: the run
    /// ending at the caret grows and the run starting there moves.
    #[test]
    fn typing_at_a_run_boundary_extends_the_run_before_it() {
        let mut style = TextStyle::default();
        style.apply_range_override(2, 5, |r| r.bold = Some(true));
        style.apply_range_override(5, 8, |r| r.italic = Some(true));
        style.adjust_runs_for_edit(5, 5, 2);
        assert_eq!(spans(&style), [(2, 7), (7, 10)]);
        assert_eq!(style.run_at(6).and_then(|r| r.bold), Some(true));
        assert_eq!(style.run_at(7).and_then(|r| r.italic), Some(true));
    }

    /// At offset 0 nothing precedes the caret, so the text joins the run that
    /// starts there rather than landing unstyled.
    #[test]
    fn typing_at_the_start_joins_the_first_run() {
        let mut style = TextStyle::default();
        style.apply_range_override(0, 3, |r| r.bold = Some(true));
        style.adjust_runs_for_edit(0, 0, 2);
        assert_eq!(spans(&style), [(0, 5)]);
        // A run that starts later moves instead.
        let mut later = TextStyle::default();
        later.apply_range_override(1, 3, |r| r.bold = Some(true));
        later.adjust_runs_for_edit(0, 0, 2);
        assert_eq!(spans(&later), [(3, 5)]);
    }

    /// Typing over a selection that crosses two runs gives the new text the
    /// first selected character's run, and the runs stay disjoint.
    #[test]
    fn typing_over_two_runs_keeps_them_disjoint() {
        let mut style = TextStyle::default();
        style.apply_range_override(0, 4, |r| r.bold = Some(true));
        style.apply_range_override(4, 8, |r| r.italic = Some(true));
        style.adjust_runs_for_edit(2, 6, 1);
        assert_eq!(spans(&style), [(0, 3), (3, 5)]);
        assert_eq!(style.run_at(2).and_then(|r| r.bold), Some(true));
        assert_eq!(style.run_at(3).and_then(|r| r.italic), Some(true));
        // A run wholly inside the replaced range is gone.
        let mut inside = TextStyle::default();
        inside.apply_range_override(3, 5, |r| r.bold = Some(true));
        inside.adjust_runs_for_edit(2, 6, 1);
        assert!(inside.runs.is_empty());
    }

    #[test]
    fn stale_text_run_and_path_mask_versions_are_rejected_on_load() {
        let mut project = Project::new("versioned", Rational::FPS_24);
        let mut style = TextStyle::default();
        style.apply_range_override(0, 3, |r| r.bold = Some(true));
        let generator = Generator::Text {
            content: "Hello".into(),
            style,
        };
        let lane = TrackKind::for_generator(&generator).expect("a text lane kind");
        let track = project.add_track(lane, "T1");
        project
            .add_generated(
                track,
                generator,
                TimeRange::at_rate(0, 24, Rational::FPS_24),
            )
            .unwrap();
        let saved = project.save_to_string().unwrap();
        let roundtrip = Project::load_from_string(&saved).unwrap();
        let clip = roundtrip
            .timeline()
            .tracks_ordered()
            .next()
            .unwrap()
            .clips()
            .next()
            .unwrap();
        let crate::ClipSource::Generated(Generator::Text { style, .. }) = &clip.content else {
            panic!("expected text clip");
        };
        assert_eq!(style.runs.len(), 1);
        assert_eq!(style.runs[0].version, TEXT_RUN_VERSION);

        // Only the run's version goes stale: every other field of the saved
        // project is current, so a refusal is the run's.
        let mut stale_run: serde_json::Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(stale_versions(&mut stale_run, "runs", "version"), 1);
        assert!(Project::load_from_string(&stale_run.to_string()).is_err());
        assert!(Project::load_from_string(&saved).is_ok());

        let mask = Mask::from_path(
            vec![
                ShapePathPoint::corner([0.1, 0.1]),
                ShapePathPoint::corner([0.9, 0.1]),
                ShapePathPoint::corner([0.5, 0.9]),
            ],
            true,
        );
        let mask_json = serde_json::to_string(&mask).unwrap();
        let decoded: Mask = serde_json::from_str(&mask_json).unwrap();
        assert_eq!(decoded.kind, MaskKind::Path);
        assert_eq!(decoded.path_at(0.0).len(), 3);

        let stale_mask = r#"{"kind":"path","path":[{"anchor":[0.1,0.1],"handle_in":[0.1,0.1],"handle_out":[0.1,0.1]}],"path_closed":true,"path_version":0}"#;
        assert!(serde_json::from_str::<Mask>(stale_mask).is_err());
    }

    /// Set `field` to 0 on every object of every `key` array in `value`, and
    /// count them.
    fn stale_versions(value: &mut serde_json::Value, key: &str, field: &str) -> usize {
        match value {
            serde_json::Value::Object(map) => map
                .iter_mut()
                .map(|(name, child)| match (name == key, child) {
                    (true, serde_json::Value::Array(items)) => items
                        .iter_mut()
                        .filter_map(|item| item.as_object_mut()?.get_mut(field))
                        .map(|version| *version = serde_json::Value::from(0))
                        .count(),
                    (_, child) => stale_versions(child, key, field),
                })
                .sum(),
            serde_json::Value::Array(items) => items
                .iter_mut()
                .map(|item| stale_versions(item, key, field))
                .sum(),
            _ => 0,
        }
    }
}
