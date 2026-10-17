//! Project and editor view queries over an immutable [`Project`] snapshot.
//!
//! Every function here is pure over [`Project`] or [`ViewState`]. [`ProjectHost`]
//! and [`PluginContext`] expose the same queries over the live session, gated on
//! the `project` read permission.

use aria_core::{RationalTime, TimeRange};
use aria_models::{
    Clip, ClipId, ClipSource, Marker, Project, SequenceId, Timeline, TrackId, clip_type_spec,
};
use serde::{Deserialize, Serialize};

use crate::editor_events::ViewState;

/// Summary of a timeline clip returned by project queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipRef {
    pub id: ClipId,
    pub track: TrackId,
    pub start: RationalTime,
    pub end: RationalTime,
    pub kind: String,
    pub name: String,
}

impl ClipRef {
    /// Build a [`ClipRef`] for `clip` on `track` in `project`.
    pub fn from_clip(project: &Project, track: TrackId, clip: &Clip) -> Self {
        let start = clip.timeline.start;
        let end = RationalTime::new(clip.timeline.end_tick(), start.rate);
        let kind = if let Some(clip_type) = &clip.clip_type {
            clip_type.clone()
        } else {
            match &clip.content {
                ClipSource::Media { .. } => "media".to_owned(),
                ClipSource::Sequence { .. } => "sequence".to_owned(),
                ClipSource::Multicam { .. } => "multicam".to_owned(),
                ClipSource::Generated(generator) => generator.spec().id.to_owned(),
            }
        };
        let name = if clip.name.is_none()
            && let Some(type_id) = &clip.clip_type
            && let Some(spec) = clip_type_spec(type_id)
        {
            spec.label.clone()
        } else {
            project.clip_title(clip).into_owned()
        };
        Self {
            id: clip.id,
            track,
            start,
            end,
            kind,
            name,
        }
    }
}

/// Sequence In and Out points from [`EditorView`](crate::EditorView).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InOut {
    pub mark_in: Option<RationalTime>,
    pub mark_out: Option<RationalTime>,
}

fn target_timeline(project: &Project, sequence: Option<SequenceId>) -> Option<&Timeline> {
    match sequence {
        Some(id) => project.sequence(id).map(|seq| &seq.timeline),
        None => Some(project.timeline()),
    }
}

fn half_open_overlaps(a: TimeRange, b: TimeRange) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let a_end = RationalTime::new(a.end_tick(), a.start.rate);
    let b_end = RationalTime::new(b.end_tick(), b.start.rate);
    a.start.compare(b_end).is_lt() && b.start.compare(a_end).is_lt()
}

fn half_open_contains(range: TimeRange, tick: RationalTime) -> bool {
    if range.is_empty() {
        return false;
    }
    let end = RationalTime::new(range.end_tick(), range.start.rate);
    range.start.compare(tick).is_le() && tick.compare(end).is_lt()
}

/// Look up `id` across the open sequence, root timeline, and nested sequences.
pub fn clip(project: &Project, id: ClipId) -> Option<ClipRef> {
    let active = project.timeline();
    if let Some(track) = active.track_of(id)
        && let Some(found) = active.clip(id)
    {
        return Some(ClipRef::from_clip(project, track, found));
    }
    let root = project.root_timeline();
    if let Some(track) = root.track_of(id)
        && let Some(found) = root.clip(id)
    {
        return Some(ClipRef::from_clip(project, track, found));
    }
    for seq in project.sequences() {
        if let Some(track) = seq.timeline.track_of(id)
            && let Some(found) = seq.timeline.clip(id)
        {
            return Some(ClipRef::from_clip(project, track, found));
        }
    }
    None
}

/// Clips on `sequence` (`None` for the open sequence) overlapping the half-open
/// `range`, in track order then timeline start order.
pub fn clips_in_range(
    project: &Project,
    sequence: Option<SequenceId>,
    range: TimeRange,
) -> Vec<ClipRef> {
    let Some(timeline) = target_timeline(project, sequence) else {
        return Vec::new();
    };
    if range.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for track in timeline.tracks_ordered() {
        for found in track.clips_ordered() {
            if half_open_overlaps(found.timeline, range) {
                out.push(ClipRef::from_clip(project, track.id, found));
            }
        }
    }
    out
}

/// Clips on `sequence` (`None` for the open sequence) covering `tick` (`start <= tick < end`),
/// in track order then timeline start order.
pub fn clips_at(
    project: &Project,
    sequence: Option<SequenceId>,
    tick: RationalTime,
) -> Vec<ClipRef> {
    let Some(timeline) = target_timeline(project, sequence) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for track in timeline.tracks_ordered() {
        for found in track.clips_ordered() {
            if half_open_contains(found.timeline, tick) {
                out.push(ClipRef::from_clip(project, track.id, found));
            }
        }
    }
    out
}

/// Resolve `selection` (from [`EditorView`](crate::EditorView)) to [`ClipRef`]s,
/// dropping any clip ids no longer in `project`.
pub fn selected_clips(project: &Project, selection: &[ClipId]) -> Vec<ClipRef> {
    selection
        .iter()
        .filter_map(|&id| clip(project, id))
        .collect()
}

/// Ruler markers on `sequence` (`None` for the open sequence) overlapping the
/// half-open `range`, in timeline order.
pub fn markers_in_range(
    project: &Project,
    sequence: Option<SequenceId>,
    range: TimeRange,
) -> Vec<Marker> {
    let Some(timeline) = target_timeline(project, sequence) else {
        return Vec::new();
    };
    if range.is_empty() {
        return Vec::new();
    }
    timeline
        .markers()
        .iter()
        .filter(|marker| {
            if marker.duration.value > 0 {
                if let Ok(span) = TimeRange::new(marker.tick, marker.duration) {
                    half_open_overlaps(span, range)
                } else {
                    false
                }
            } else {
                half_open_contains(range, marker.tick)
            }
        })
        .cloned()
        .collect()
}

/// Extract sequence In and Out points from `view`.
pub fn in_out(view: &ViewState) -> InOut {
    InOut {
        mark_in: view.mark_in,
        mark_out: view.mark_out,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use aria_core::Rational;
    use aria_models::{Clip, Marker, MarkerColor, MediaSource, Project, Track, TrackKind};

    use super::*;

    fn t(frames: i64) -> RationalTime {
        RationalTime::new(frames, Rational::FPS_24)
    }

    fn tr(start: i64, duration: i64) -> TimeRange {
        TimeRange::new(t(start), t(duration)).expect("valid range")
    }

    /// Build a fixture project with two video tracks, one audio track, a nested
    /// sequence, and ruler markers so query tests exercise half-open clip edges,
    /// multi-track ordering, nested sequences, and stale selection ids.
    fn fixture_project() -> (Project, [TrackId; 3], [ClipId; 4], SequenceId, ClipId) {
        let mut project = Project::new("Query Fixture", Rational::FPS_24);
        let v1 = project.add_track(TrackKind::Video, "V1");
        let v2 = project.add_track(TrackKind::Video, "V2");
        let a1 = project.add_track(TrackKind::Audio, "A1");

        let media_a = project.add_media(MediaSource::new(
            PathBuf::from("/media/Intro.mp4"),
            1920,
            1080,
            Rational::FPS_24,
            200,
            false,
        ));
        let media_b = project.add_media(MediaSource::new(
            PathBuf::from("/media/Cutaway.mp4"),
            1920,
            1080,
            Rational::FPS_24,
            200,
            false,
        ));
        let media_audio = project.add_media(MediaSource::new(
            PathBuf::from("/media/Bed.wav"),
            0,
            0,
            Rational::FPS_24,
            200,
            true,
        ));

        // V1: [10, 20) and [30, 40)
        let c1 = project
            .timeline_mut()
            .add_clip(v1, Clip::from_media(media_a, tr(0, 10), tr(10, 10)))
            .expect("add c1");
        let mut second_clip = Clip::from_media(media_a, tr(10, 10), tr(30, 10));
        second_clip.name = Some("Second Shot".to_owned());
        let c2 = project
            .timeline_mut()
            .add_clip(v1, second_clip)
            .expect("add c2");
        // V2: [15, 35)
        let c3 = project
            .timeline_mut()
            .add_clip(v2, Clip::from_media(media_b, tr(0, 20), tr(15, 20)))
            .expect("add c3");
        // A1: [0, 25)
        let c4 = project
            .timeline_mut()
            .add_clip(a1, Clip::from_media(media_audio, tr(0, 25), tr(0, 25)))
            .expect("add c4");

        // Ruler markers on root timeline at 10 (point), [20, 25) (range), and 40 (point).
        project
            .timeline_mut()
            .add_marker(Marker::new(t(10), "Beat 1", MarkerColor::Teal))
            .expect("marker 10");
        project
            .timeline_mut()
            .add_marker(Marker::new(t(20), "Span", MarkerColor::Orange).with_duration(t(5)))
            .expect("marker 20..25");
        project
            .timeline_mut()
            .add_marker(Marker::new(t(40), "End", MarkerColor::Red))
            .expect("marker 40");

        // Secondary sequence with one clip [5, 15).
        let seq_id = project.create_sequence("Nested", Rational::FPS_24);
        let nested_track = project
            .sequence_mut(seq_id)
            .expect("nested sequence")
            .timeline
            .add_track(Track::new(TrackKind::Video, "NV1"));
        let nested_clip = project
            .sequence_mut(seq_id)
            .expect("nested sequence")
            .timeline
            .add_clip(
                nested_track,
                Clip::from_media(media_b, tr(0, 10), tr(5, 10)),
            )
            .expect("nested clip");

        (project, [v1, v2, a1], [c1, c2, c3, c4], seq_id, nested_clip)
    }

    #[test]
    fn clip_lookup_resolves_across_root_and_sequences() {
        let (project, [v1, v2, _a1], [c1, c2, c3, _c4], _seq_id, nested_clip) = fixture_project();

        let ref1 = clip(&project, c1).expect("c1 exists");
        assert_eq!(ref1.id, c1);
        assert_eq!(ref1.track, v1);
        assert_eq!(ref1.start, t(10));
        assert_eq!(ref1.end, t(20));
        assert_eq!(ref1.kind, "media");
        assert_eq!(ref1.name, "Intro.mp4");

        let ref2 = clip(&project, c2).expect("c2 exists");
        assert_eq!(ref2.name, "Second Shot");

        let ref3 = clip(&project, c3).expect("c3 exists");
        assert_eq!(ref3.track, v2);
        assert_eq!(ref3.start, t(15));
        assert_eq!(ref3.end, t(35));

        let nested = clip(&project, nested_clip).expect("nested clip found");
        assert_eq!(nested.id, nested_clip);
        assert_eq!(nested.start, t(5));
        assert_eq!(nested.end, t(15));

        assert!(clip(&project, ClipId::from_raw(999_999)).is_none());
    }

    #[test]
    fn clips_in_range_half_open_edges_and_multi_track_order() {
        let (project, _tracks, [c1, c2, c3, c4], seq_id, nested_clip) = fixture_project();

        // Range [0, 10) touches c1's left edge [10, 20) without overlapping it;
        // only c4 [0, 25) on A1 overlaps.
        let ids_0_10: Vec<ClipId> = clips_in_range(&project, None, tr(0, 10))
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids_0_10, vec![c4]);

        // Range [20, 30) touches c1's right edge 20 and c2's left edge 30 without
        // overlapping either; c4 [0, 25) on A1 and c3 [15, 35) on V2 overlap.
        let ids_20_30: Vec<ClipId> = clips_in_range(&project, None, tr(20, 10))
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids_20_30, vec![c4, c3]);

        // Range [40, 50) touches c2's right edge 40 and misses every clip.
        assert!(clips_in_range(&project, None, tr(40, 10)).is_empty());

        // Empty range [15, 15) overlaps nothing.
        assert!(clips_in_range(&project, None, tr(15, 0)).is_empty());

        // Straddling range [12, 32) overlaps c4 on A1, c1 and c2 on V1, and c3 on V2,
        // returned in track order (A1, V1, V2) then time order (c1 before c2).
        let ids_12_32: Vec<ClipId> = clips_in_range(&project, None, tr(12, 20))
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids_12_32, vec![c4, c1, c2, c3]);

        // Explicit sequence id queries that sequence's timeline.
        let nested_ids: Vec<ClipId> = clips_in_range(&project, Some(seq_id), tr(0, 10))
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(nested_ids, vec![nested_clip]);
    }

    #[test]
    fn clips_at_respects_half_open_boundaries() {
        let (project, _tracks, [c1, _c2, c3, c4], _seq_id, _nested_clip) = fixture_project();

        // At tick 10: c4 [0, 25) on A1 is active and c1 [10, 20) on V1 starts.
        let at_10: Vec<ClipId> = clips_at(&project, None, t(10))
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(at_10, vec![c4, c1]);

        // At tick 20: c1 [10, 20) has ended; c4 [0, 25) on A1 and c3 [15, 35) on V2 cover tick 20.
        let at_20: Vec<ClipId> = clips_at(&project, None, t(20))
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(at_20, vec![c4, c3]);
    }

    #[test]
    fn selected_clips_drops_deleted_ids_and_preserves_selection_order() {
        let (mut project, _tracks, [c1, c2, c3, _c4], _seq_id, _nested_clip) = fixture_project();
        project.timeline_mut().remove_clip(c2);

        let resolved = selected_clips(&project, &[c3, c2, ClipId::from_raw(888_888), c1]);
        let ids: Vec<ClipId> = resolved.into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![c3, c1]);
    }

    #[test]
    fn markers_in_range_and_in_out_queries() {
        let (project, _tracks, _clips, _seq_id, _nested_clip) = fixture_project();

        // [0, 10) misses point marker at 10 (half-open).
        assert!(markers_in_range(&project, None, tr(0, 10)).is_empty());

        // [10, 20) includes point marker at 10 and misses range marker [20, 25).
        let m_10_20: Vec<String> = markers_in_range(&project, None, tr(10, 10))
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(m_10_20, vec!["Beat 1"]);

        // [24, 41) overlaps range marker [20, 25) and includes point marker at 40.
        let m_24_41: Vec<String> = markers_in_range(&project, None, tr(24, 17))
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(m_24_41, vec!["Span", "End"]);

        let view = ViewState {
            selection: Vec::new(),
            playhead: t(12),
            mark_in: Some(t(5)),
            mark_out: Some(t(25)),
            playing: false,
            speed: 1,
        };
        assert_eq!(
            in_out(&view),
            InOut {
                mark_in: Some(t(5)),
                mark_out: Some(t(25)),
            }
        );
    }

    #[test]
    fn plugin_context_queries_refuse_without_project_read_permission() {
        use aria_engine::{Engine, EngineConfig};
        use parking_lot::Mutex;
        use std::sync::Arc;

        use crate::{
            EditorView, Plugin, PluginContext, PluginError, PluginManifest, ProjectHost, ViewFrame,
        };

        let (project, _tracks, [c1, c2, c3, _c4], _seq_id, _nested_clip) = fixture_project();
        let ctx = PluginContext::new();
        let mut engine = Engine::new(EngineConfig::default()).expect("engine");
        engine.reset_project(project);
        let engine = Arc::new(Mutex::new(engine));
        let _host_reg = ctx.register(ProjectHost::new(engine, &ctx));
        let view = EditorView::new();
        view.publish(
            ViewFrame {
                selection: &[c3, ClipId::from_raw(777_777), c1],
                playhead: t(15),
                mark_in: Some(t(10)),
                mark_out: Some(t(30)),
                playing: false,
                speed: 1,
            },
            ctx.events(),
        );
        let _view_reg = ctx.register(view);

        struct UnpermittedProbe {
            manifest: PluginManifest,
            clip_id: ClipId,
            deleted_id: ClipId,
        }
        impl Plugin for UnpermittedProbe {
            fn manifest(&self) -> &PluginManifest {
                &self.manifest
            }

            fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
                assert!(ctx.clip(self.clip_id).is_none());
                assert!(ctx.clips_in_range(None, tr(0, 40)).is_empty());
                assert!(ctx.clips_at(None, t(15)).is_empty());
                assert!(ctx.selected_clips().is_empty());
                assert!(ctx.markers_in_range(None, tr(0, 50)).is_empty());
                assert!(ctx.in_out().is_none());
                let _ = self.deleted_id;
                Ok(())
            }
        }

        ctx.mount(&UnpermittedProbe {
            manifest: PluginManifest::new(
                "test.unpermitted-queries",
                "Unpermitted Queries",
                "0.1.0",
                &[],
                &[],
            ),
            clip_id: c1,
            deleted_id: c2,
        })
        .expect("mount unpermitted probe");
        assert!(
            ctx.denials()
                .iter()
                .any(|d| d.plugin == "test.unpermitted-queries"
                    && d.service.ends_with("ProjectHost")),
            "expected ProjectHost capability denial, got {:?}",
            ctx.denials()
        );

        struct PermittedProbe {
            manifest: PluginManifest,
            c1: ClipId,
            c3: ClipId,
        }
        impl Plugin for PermittedProbe {
            fn manifest(&self) -> &PluginManifest {
                &self.manifest
            }

            fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
                assert_eq!(ctx.clip(self.c1).map(|c| c.id), Some(self.c1));
                let sel: Vec<ClipId> = ctx.selected_clips().into_iter().map(|c| c.id).collect();
                assert_eq!(sel, vec![self.c3, self.c1]);
                assert_eq!(
                    ctx.in_out(),
                    Some(InOut {
                        mark_in: Some(t(10)),
                        mark_out: Some(t(30)),
                    })
                );
                Ok(())
            }
        }

        ctx.mount(&PermittedProbe {
            manifest: PluginManifest::new(
                "test.permitted-queries",
                "Permitted Queries",
                "0.1.0",
                &[],
                &["ProjectHost", "EditorView"],
            ),
            c1,
            c3,
        })
        .expect("mount permitted probe");
    }

    #[test]
    fn resolve_dropped_runs_higher_priority_first_and_ties_by_mount_order() {
        use crate::{DropAction, Dropped, PluginContext, ResolveDropped};
        use parking_lot::Mutex;
        use std::sync::Arc;

        let ctx = PluginContext::new();
        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        // Mounted first at default priority 0.
        let o1 = Arc::clone(&order);
        let _h1 = ctx.on_serial::<ResolveDropped, DropAction, _>(move |ev| {
            o1.lock().push("prio0_first");
            match &ev.item {
                Dropped::Path(_) => Some(DropAction::ImportMedia),
                Dropped::Url(_) => None,
            }
        });

        // Mounted second at priority 10: must run before prio 0 even though mounted later.
        let o2 = Arc::clone(&order);
        let _h2 = ctx.on_serial_with_priority::<ResolveDropped, DropAction, _>(10, move |ev| {
            o2.lock().push("prio10_first");
            match &ev.item {
                Dropped::Path(p) if p.extension().and_then(|e| e.to_str()) == Some("aria") => {
                    Some(DropAction::OpenProject)
                }
                Dropped::Url(u) if u.starts_with("https://example.com/") => {
                    Some(DropAction::Handled)
                }
                _ => None,
            }
        });

        // Mounted third at priority 10: ties with prio10_first, so runs after it and before prio0_first.
        let o3 = Arc::clone(&order);
        let _h3 = ctx.on_serial_with_priority::<ResolveDropped, DropAction, _>(10, move |_ev| {
            o3.lock().push("prio10_second");
            None
        });

        let claimed_proj = ctx
            .events()
            .serial::<ResolveDropped, DropAction>(&ResolveDropped::path("/projects/demo.aria"));
        assert_eq!(claimed_proj, Some(DropAction::OpenProject));
        assert_eq!(*order.lock(), vec!["prio10_first"]);

        order.lock().clear();
        let claimed_media = ctx
            .events()
            .serial::<ResolveDropped, DropAction>(&ResolveDropped::path("/media/clip.mp4"));
        assert_eq!(claimed_media, Some(DropAction::ImportMedia));
        assert_eq!(
            *order.lock(),
            vec!["prio10_first", "prio10_second", "prio0_first"]
        );

        order.lock().clear();
        let claimed_url = ctx
            .events()
            .serial::<ResolveDropped, DropAction>(&ResolveDropped::url(
                "https://example.com/asset",
            ));
        assert_eq!(claimed_url, Some(DropAction::Handled));
        assert_eq!(*order.lock(), vec!["prio10_first"]);
    }
}
