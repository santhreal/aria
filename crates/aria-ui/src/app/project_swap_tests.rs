//! Replacing the open project drops every reference the session held into it.
//!
//! The class closed here: session state that names a clip, edge, gap, pool
//! item, folder, keyframe or Source tab by id, or a position on the timeline,
//! outliving the project it was read from. A project file keeps its ids and
//! every launch counts from 1 again, so after an open the id named another
//! project's clip: Delete removed a clip nobody picked, Paste duplicated it,
//! the Inspector showed it, the bin opened a folder of the new project, and a
//! Source tab stayed open on the incoming project's media with the old take's marks.
//! The In and Out marks, read by Play In to Out and the export's marked range,
//! were positions on the old timeline, and playback ran on into the incoming
//! project from the old playhead's clock. Each route that replaces the project
//! runs against a playing session holding every reference, each one naming an
//! entity the incoming project has under the same id.
//!
//! Not caught: a gesture still in flight when the project is replaced, and the
//! offline announcements, which every projection reconciles against the pool.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_models::{ClipId, ClipParam};
use aria_plugin::Origin;

use super::AppState;
use super::bins::BinRename;
use super::line_text::LineText;
use super::pool_preview::SourceView;
use super::premiere_tests::{place_media, save_fixture, with_clip};
use super::property_rows::{PropertyGroup, SelectedKeyframe, TimelineOutlineRow};
use crate::types::{SelectedEdge, SelectedGap, TrimSide};

/// The ids of the project on disk that every route brings in.
struct Incoming {
    clip: ClipId,
    track: u64,
    media: u64,
    bin: u64,
    project: PathBuf,
    template: PathBuf,
}

/// A session on an empty project, holding references to the entities of a
/// saved project and of a template made from it, with that project's folder
/// open or the pool root.
fn holding(folder_open: bool) -> (AppState, Incoming) {
    let (mut state, clip) = with_clip();
    state.copy_selected();
    let copied = state.clipboard.clone();
    let track = state.store.sequence.tracks[0].id;
    let media = state.store.media[0].id;
    state.create_bin().expect("a folder");
    let bin = state.current_bin.expect("the new folder is open");
    state.open_bin(None);
    let template = state.data_dir.join("other.ariat");
    state
        .save_template(template.clone(), "Other".into())
        .expect("save the template");
    let project = save_fixture(&mut state);
    state.new_project().expect("an empty project");

    state.selection = vec![clip];
    state.selected_edge = Some(SelectedEdge {
        clip: clip.raw(),
        side: TrimSide::Out,
    });
    state.selected_gap = Some(SelectedGap {
        track,
        start_ticks: 0,
        end_ticks: 1,
    });
    state.clipboard = copied;
    state.parent_whip = Some(clip);
    state.expanded_clips = Arc::new(vec![clip.raw()]);
    let key = SelectedKeyframe {
        clip,
        param: ClipParam::Opacity,
        tick: 0,
    };
    state.prop_rows.selected_keyframes.insert(key);
    state
        .prop_rows
        .graph_params
        .push((clip, ClipParam::Opacity));
    // Opening a folder drops the picked items and the rename, so it goes first.
    if folder_open {
        state.open_bin(Some(bin));
    }
    state.pool_selection.replace(media);
    state.bin_rename = Some(BinRename {
        bin,
        draft: LineText::selecting_all("Kept"),
    });
    state.relink_notes.insert(media, "not found".into());
    state.mark_in = Some(0);
    state.mark_out = Some(1);
    state.playing = true;
    state.sources.open(SourceView {
        media,
        name: "Kept".into(),
        duration_ticks: 10,
        tick: 0,
        mark_in: None,
        mark_out: None,
        clip: None,
    });
    let incoming = Incoming {
        clip,
        track,
        media,
        bin,
        project,
        template,
    };
    (state, incoming)
}

/// The references still held, by name.
fn held(state: &AppState) -> Vec<&'static str> {
    [
        ("clip selection", !state.selection.is_empty()),
        ("edge selection", state.selected_edge.is_some()),
        ("gap selection", state.selected_gap.is_some()),
        ("clipboard", state.clipboard.is_some()),
        ("parent pick whip", state.parent_whip.is_some()),
        ("open property rows", !state.expanded_clips.is_empty()),
        (
            "selected keyframes",
            !state.prop_rows.selected_keyframes.is_empty(),
        ),
        ("graph curves", !state.prop_rows.graph_params.is_empty()),
        ("picked pool items", !state.pool_selection.is_empty()),
        ("open folder", state.current_bin.is_some()),
        ("folder rename", state.bin_rename.is_some()),
        ("relink notes", !state.relink_notes.is_empty()),
        ("In mark", state.mark_in.is_some()),
        ("Out mark", state.mark_out.is_some()),
        ("playback", state.playing),
        ("Source tab", !state.sources.tabs().is_empty()),
    ]
    .into_iter()
    .filter_map(|(name, set)| set.then_some(name))
    .collect()
}

type Route = (&'static str, fn(&mut AppState, &Path, &Path));

/// Every way the open project is replaced.
fn routes() -> [Route; 5] {
    [
        ("File > Open", |state, project, _| {
            state.open_project(project.to_path_buf()).expect("open");
        }),
        ("open without media", |state, project, _| {
            state.load_project(project.to_path_buf()).expect("load");
        }),
        ("File > New", |state, _, _| {
            state.new_project().expect("new");
        }),
        ("File > Use Template", |state, _, template| {
            state
                .apply_template(template.to_path_buf(), Vec::new())
                .expect("apply the template");
        }),
        ("a plugin's open", |state, project, _| {
            state
                .host
                .open(&Origin::plugin("example.opener"), project)
                .expect("the plugin opens");
            state.sync_from_plugin_host();
        }),
    ]
}

#[test]
fn replacing_the_project_drops_every_reference_into_the_old_one() {
    for folder_open in [true, false] {
        for (route, replace) in routes() {
            let (mut state, incoming) = holding(folder_open);
            let fixture = held(&state).len();
            assert_eq!(fixture, 15 + usize::from(folder_open), "{route}");
            replace(&mut state, &incoming.project, &incoming.template);
            assert_eq!(
                held(&state),
                Vec::<&str>::new(),
                "{route} with the folder open: {folder_open}"
            );
        }
    }
}

/// Every route leaves the store a fresh projection of the incoming project.
///
/// Dropping the selection runs after the engine is replaced and before the
/// store is rebuilt. It used to key the old store to the new engine, so the
/// next poll found nothing to do and the panels drew the project that was
/// left: its lanes, its pool, its clips. The session here holds a sound clip
/// the incoming project lacks, so a kept store differs from a fresh one.
#[test]
fn every_route_draws_the_incoming_project() {
    let sound = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-sine-48k-2s.wav");
    for (route, replace) in routes() {
        let (mut state, incoming) = holding(false);
        place_media(&mut state, sound.clone());
        // Saved: a plugin's open fails over unsaved changes.
        state.project_path = Some(state.data_dir.join("left.aria"));
        state.save_project().expect("the project left saves");
        replace(&mut state, &incoming.project, &incoming.template);
        state.sync_from_engine();
        let drawn = state.store.clone();
        state.projected = None;
        state.sync_from_engine();
        assert_eq!(
            *drawn, *state.store,
            "{route} kept a store of the project it left"
        );
    }
}

/// The incoming project has every entity the references name, so a kept
/// reference lands on a stranger rather than on nothing.
#[test]
fn the_incoming_project_has_what_the_references_name() {
    let (mut state, incoming) = holding(true);
    state.open_project(incoming.project.clone()).expect("open");
    let store = &state.store;
    let mut clips = store.sequence.tracks.iter().flat_map(|track| &track.clips);
    assert!(clips.any(|clip| clip.id == incoming.clip.raw()));
    assert!(store.sequence.tracks.iter().any(|t| t.id == incoming.track));
    assert!(store.media.iter().any(|item| item.id == incoming.media));
    assert!(store.bins.iter().any(|item| item.id == incoming.bin));
}

/// A clip of the incoming project opens with the default groups, never the
/// groups a clip with its id had open in the project left.
///
/// The class closed here: each clip's open groups and revealed properties
/// outliving the project its rows were opened in, as the open rows did not,
/// so a clip opened by a double-click after an open showed the Effects group
/// the old clip had open, and toggling that group closed it. Each route that
/// brings the clip in is read.
///
/// Not caught: a route whose incoming project lacks the clip, as File > New's
/// does, since nothing there opens under the id.
#[test]
fn a_clip_opened_after_a_swap_shows_its_default_groups() {
    let mut read = Vec::new();
    for (route, replace) in routes() {
        let (mut state, incoming) = holding(false);
        state.toggle_property_group(incoming.clip, PropertyGroup::Effects);
        replace(&mut state, &incoming.project, &incoming.template);
        state.sync_from_engine();
        if state.ui_clip_ref(incoming.clip).is_none() {
            continue;
        }
        state.toggle_expanded(incoming.clip.raw());
        let open: Vec<PropertyGroup> = state
            .track_outline_rows(incoming.track)
            .into_iter()
            .filter_map(|row| match row {
                TimelineOutlineRow::GroupHeader {
                    group,
                    expanded: true,
                    ..
                } => Some(group),
                _ => None,
            })
            .collect();
        assert_eq!(open, [PropertyGroup::Transform], "{route}");
        read.push(route);
    }
    assert!(
        read.contains(&"File > Open"),
        "no route brought the clip in: {read:?}"
    );
}
