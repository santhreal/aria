//! One row per render stage a frame's cost is split into.
//!
//! [`Engine::last_frame_stats`](crate::Engine::last_frame_stats) has always
//! reported where a preview or export frame spent its time, and nothing read it:
//! a slow scrub gave no way to tell a decode-bound source
//! from a raster-bound title from a composite-bound stack of effects, which is
//! the one fact that decides what to do about it.
//!
//! A row names one stage and reads it, so a readout renders the table instead of
//! writing a label beside each field. [`FrameStats`] is a struct rather than an
//! enum, so the compile-time gate is [`covered`]: it destructures every field by
//! name, and a stage added to the struct stops compiling until it names a row.

use aria_render::FrameStats;

/// What one render stage is called and how its cost reads.
#[derive(Debug, Clone, Copy)]
pub struct FrameStageSpec {
    /// Stable wire / UI id.
    pub id: &'static str,
    /// Operator-facing label.
    pub label: &'static str,
    /// What the stage covers, for a readout that has room to say.
    pub note: &'static str,
    /// Whether the stage shrinks when the frame is rendered smaller. A
    /// resolution ladder can only buy back the ones that do.
    pub scales_with_output: bool,
    /// This stage's share of the frame, in milliseconds.
    pub read: fn(&FrameStats) -> f64,
}

/// The stages, in pipeline order.
pub const SHIPPED: &[FrameStageSpec] = &[
    FrameStageSpec {
        id: "decode",
        label: "Decode",
        note: "media decode summed across layers",
        scales_with_output: false,
        read: |stats| stats.decode_ms,
    },
    FrameStageSpec {
        id: "raster",
        label: "Raster",
        note: "text, shape, and still realize",
        scales_with_output: true,
        read: |stats| stats.raster_ms,
    },
    FrameStageSpec {
        id: "composite",
        label: "Composite",
        note: "GPU composite and readback",
        scales_with_output: true,
        read: |stats| stats.composite_ms,
    },
];

/// Every stage, in pipeline order. A readout walks this instead of naming
/// fields, so a stage added to [`FrameStats`] appears there with one row.
pub const fn frame_stages() -> &'static [FrameStageSpec] {
    SHIPPED
}

/// The compile-time gate: every field of [`FrameStats`] is bound by name here
/// and the milliseconds are summed through the rows, so a stage added to the
/// struct breaks this destructure until a row reads it.
///
/// A field that is not a duration is bound and named as such: the effect-pass
/// counts describe how the composite stage was dispatched, not a slice of the
/// frame, and adding them to a total of milliseconds would be nonsense. They
/// still have to be mentioned, which is the point — a count added here without
/// a reader anywhere is a decision someone has to write down.
///
/// Returns the whole-frame cost the rows account for, which is what
/// [`FrameStats::total_ms`] reports independently — the test below holds the two
/// against each other.
#[cfg(test)]
pub fn covered(stats: &FrameStats) -> f64 {
    let FrameStats {
        decode_ms: _,
        raster_ms: _,
        composite_ms: _,
        // Counts, not durations: read by the Info panel, summed nowhere.
        effect_passes: _,
        effect_pass_bind_groups: _,
        // A count, not a duration: `CellRenderer::render_fit` reads it to
        // mark a filmstrip cell snapped, summed nowhere.
        snapped_frames: _,
    } = stats;
    SHIPPED.iter().map(|stage| (stage.read)(stats)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stage that stops being summed, or a field that grows a second reader,
    /// shows up as a total that no longer matches the model's own. Derived from
    /// the table on one side and from the struct on the other: nothing here
    /// names a stage or counts them.
    #[test]
    fn the_rows_account_for_the_whole_frame() {
        let stats = FrameStats {
            decode_ms: 3.0,
            raster_ms: 5.0,
            composite_ms: 11.0,
            // Counts a total of milliseconds must ignore: a pass count folded
            // into the frame cost would read as 24 ms of work nobody did.
            effect_passes: 24,
            effect_pass_bind_groups: 2,
            snapped_frames: 1,
        };
        assert_eq!(covered(&stats), stats.total_ms());
        assert_eq!(covered(&FrameStats::default()), 0.0);
    }

    /// The scaled subset must be exactly the stages declared to scale, so the
    /// column and `scaled_cost_ms` cannot drift into disagreeing about what a
    /// resolution ladder buys back.
    #[test]
    fn the_scaling_column_matches_the_scaled_cost() {
        let stats = FrameStats {
            decode_ms: 3.0,
            raster_ms: 5.0,
            composite_ms: 11.0,
            effect_passes: 24,
            effect_pass_bind_groups: 2,
            snapped_frames: 1,
        };
        let scaled: f64 = SHIPPED
            .iter()
            .filter(|stage| stage.scales_with_output)
            .map(|stage| (stage.read)(&stats))
            .sum();
        assert_eq!(scaled, stats.scaled_cost_ms());
    }

    #[test]
    fn every_stage_is_uniquely_named() {
        let ids: std::collections::BTreeSet<_> = SHIPPED.iter().map(|stage| stage.id).collect();
        assert_eq!(ids.len(), SHIPPED.len(), "two stages share an id");
        let labels: std::collections::BTreeSet<_> =
            SHIPPED.iter().map(|stage| stage.label).collect();
        assert_eq!(labels.len(), SHIPPED.len(), "two stages share a label");
        for stage in SHIPPED {
            assert!(!stage.label.is_empty(), "'{}' has no label", stage.id);
            assert!(!stage.note.is_empty(), "'{}' has no note", stage.id);
        }
    }
}
