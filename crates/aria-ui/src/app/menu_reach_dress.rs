//! What a menu row is driven against.
//!
//! # Why
//!
//! Every row used to be run against one fixture: one silent still on one lane,
//! nothing on the clipboard, no marker, no source loaded, no nest. Fifteen rows
//! were therefore declared `NoEdit` for a reason that was about the fixture and
//! not about the row, which made the table say two different things with one
//! word: "this row moves the view" and "this row does something we did not dress
//! for". The second reading hides a broken row, because a row that sends nothing
//! looks exactly like a row that had nothing to act on.
//!
//! So the state a row needs is a column of the table. A row declares what it
//! wants, the dressing supplies it, and `NoEdit` goes back to meaning what it
//! says: this row moves the view, opens a route, or writes to disk.

use std::path::PathBuf;

use aria_models::ClipId;

use super::{Answer, AppState, own_scratch, saved};
use crate::app::fixtures;
use crate::app::premiere_tests::with_clip;

/// The state one menu row is driven against.
///
/// Each variant exists because some row is inert without it. The naming is after
/// what the state holds, not after the row that wanted it, so a second row
/// needing the same thing reuses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Dress {
    /// One still clip on one lane, selected. What most rows need.
    Clip,
    /// That clip, and a copy of it on the clipboard.
    Copied,
    /// That clip copied, with a range marked on clear lane for the copy to be
    /// retimed into.
    Fitted,
    /// A clip with a picture and sound, so an audio extraction has something to
    /// detach and a freeze frame has a frame to take.
    Sound,
    /// Two clips selected together, unlinked.
    Pair,
    /// Two clips linked as one group.
    Linked,
    /// Two clips abutting on one lane, the left one selected, with no blend on
    /// the junction between them.
    Cut,
    /// The same junction, already carrying a blend.
    Blended,
    /// A marker under the playhead.
    Marked,
    /// A source loaded in the monitor with an in and an out marked, so a
    /// three-point edit has a range to take.
    Source,
    /// A nest clip on the timeline, selected, with the root timeline open.
    Nest,
    /// That nest, opened, so leaving it has somewhere to return to.
    Inside,
    /// A saved project whose selected clip already has its render on disk.
    Rendered,
    /// The same, with the render already swapped in, so restoring the clip has
    /// something to put back.
    Baked,
}

impl Dress {
    /// Build it, playhead included.
    ///
    /// Where the playhead stands is part of the dressing, not a step the caller
    /// adds afterwards: most rows want it inside the selected clip, and a paste
    /// or a second nest wants clear lane past the end, because dropping a clip
    /// on top of the one it was copied from is refused.
    pub(super) fn state(self) -> AppState {
        match self {
            Self::Clip => {
                let (mut state, _) = with_clip();
                inside_the_clip(&mut state);
                state
            }
            Self::Copied => {
                let (mut state, left) = fixtures::blended_junction();
                let right = state
                    .store
                    .sequence
                    .tracks
                    .iter()
                    .flat_map(|track| track.clips.iter())
                    .map(|clip| ClipId::from_raw(clip.id))
                    .find(|clip| *clip != left)
                    .expect("the junction fixture holds two clips");
                state.select_clip(left);
                state.copy_selected();
                // The paste's target is the other clip, so pasting attributes
                // has somewhere to put them, and the playhead is past both so
                // pasting a clip lands on clear lane.
                state.select_clip(right);
                past_the_end(&mut state);
                state
            }
            Self::Fitted => {
                let mut state = Self::Copied.state();
                // The range has to be a gap the sequence runs through, not lane
                // past its end: a mark is set where the playhead stands, and the
                // playhead cannot stand past the last frame. So a second lane
                // carries the sequence on while the copied clip's own lane is
                // free, and that free span is what the copy is retimed into.
                let from = state.store.sequence.duration_ticks;
                another_lane_running_past(&mut state, from);
                let to = state.store.sequence.duration_ticks;
                assert!(to > from, "the second lane did not carry the sequence on");
                state.set_playhead(from);
                state.set_mark_in();
                state.set_playhead(to);
                state.set_mark_out();
                state
            }
            Self::Sound => {
                let (mut state, _) = fixtures::with_video();
                inside_the_clip(&mut state);
                state
            }
            Self::Pair => {
                let mut state = fixtures::linked_pair();
                state
                    .unlink_selected()
                    .expect("the fixture's pair comes apart");
                inside_the_clip(&mut state);
                state
            }
            Self::Linked => {
                let mut state = fixtures::linked_pair();
                inside_the_clip(&mut state);
                state
            }
            Self::Cut => {
                let (mut state, clip) = fixtures::blended_junction();
                state.select_clip(clip);
                state
                    .clear_selected_transition()
                    .expect("the fixture's blend comes off");
                inside_the_clip(&mut state);
                state
            }
            Self::Blended => {
                let (mut state, clip) = fixtures::blended_junction();
                state.select_clip(clip);
                inside_the_clip(&mut state);
                state
            }
            Self::Marked => {
                let (mut state, _) = with_clip();
                inside_the_clip(&mut state);
                state
                    .add_marker_at_playhead()
                    .expect("a marker lands at the playhead");
                state
            }
            Self::Source => {
                let (mut state, _) = with_clip();
                let media = state
                    .store
                    .media
                    .first()
                    .map(|item| item.id)
                    .expect("the fixture imported one pool item");
                state.select_pool_item(media);
                assert!(
                    state.toggle_source_monitor(),
                    "the fixture's pool item cannot be previewed, so no row that \
                     takes a source range can be driven"
                );
                state.set_source_in();
                state.set_source_out();
                // A three-point edit lands at the playhead, so it stands where
                // there is lane to land on.
                past_the_end(&mut state);
                state
            }
            // A second copy of a nest goes in at the playhead, and the nest
            // clip is already standing where it was built.
            Self::Nest => {
                let mut state = nested();
                past_the_end(&mut state);
                state
            }
            Self::Inside => {
                let mut state = nested();
                state.open_selected_nest().expect("the nest opens");
                state
            }
            Self::Rendered => {
                let (mut state, _) = fixtures::adopted_render("menu-bake");
                inside_the_clip(&mut state);
                state
            }
            Self::Baked => {
                let (mut state, _) = fixtures::adopted_render("menu-restore");
                state
                    .render_and_replace()
                    .expect("adopt the render standing on disk");
                inside_the_clip(&mut state);
                state
            }
        }
    }
}

/// Put the playhead in the middle of the selected clip.
///
/// Several rows are inert outside it, and an inert row reads identically to a
/// broken one.
fn inside_the_clip(state: &mut AppState) {
    if let Some(ui) = state.selected_ui_clip() {
        let mid = ui.timeline_start_ticks + (ui.timeline_end_ticks - ui.timeline_start_ticks) / 2;
        state.set_playhead(mid);
    }
}

/// Put the playhead past everything on the timeline, so a row that places a clip
/// has empty lane to place it on.
fn past_the_end(state: &mut AppState) {
    let end = state.store.sequence.duration_ticks;
    state.set_playhead(end);
}

/// Put a clip on a lane of its own, starting where the sequence currently ends,
/// so the sequence runs on past `from` while every other lane is free there.
fn another_lane_running_past(state: &mut AppState, from: i64) {
    let before: Vec<u64> = lane_ids(state);
    state
        .add_track(aria_models::TrackKind::Video, "V2")
        .expect("a lane to carry the sequence on");
    state.sync_from_engine();
    // The new lane is found by difference rather than by position: a lane is
    // added at the top of the stack, so the last row is the oldest one, and a
    // clip dropped there would land on the lane the copy came from.
    let lane = lane_ids(state)
        .into_iter()
        .find(|id| !before.contains(id))
        .map(aria_models::TrackId::from_raw)
        .expect("the lane that was just added");
    let media = state
        .store
        .media
        .first()
        .expect("the fixture imports one pool item")
        .id;
    state
        .drop_media_on_timeline(aria_models::MediaId::from_raw(media), Some(lane), from)
        .expect("place a clip on the new lane");
    state.sync_from_engine();
}

/// Every video lane's id, in stack order.
fn lane_ids(state: &AppState) -> Vec<u64> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|lane| matches!(lane.kind, crate::types::UiTrackKind::Video))
        .map(|lane| lane.id)
        .collect()
}

/// The fixture clip nested, with the nest clip selected.
fn nested() -> AppState {
    let (mut state, _) = with_clip();
    state
        .nest_selection("Act one")
        .expect("nest the fixture's clip");
    let nest = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.nested_sequence.is_some())
        .map(|clip| ClipId::from_raw(clip.id))
        .expect("the selection is replaced by one nest clip");
    state.select_clip(nest);
    state
}

/// The state a row that asks the operator for a path is driven against, with the
/// answer already supplied.
///
/// A file dialog is not a surface a test can open, so the row would otherwise
/// reach nothing and its command would read as unreached. The answer stands in
/// for the dialog: the row asks, the state hands back the path this dressing
/// named, and what the row then does is measured like any other row's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Answered {
    /// A saved session, answered with a second project on disk that has two
    /// clips, so loading it is visible in the clip count.
    Open,
    /// A saved session, answered with a name it has not been written under.
    SaveAs,
    /// The fixture, answered with a bundle path.
    SaveTemplate,
    /// An empty session with a bundle already on disk, answered with it.
    UseTemplate,
    /// The fixture, answered with a file staged beside it.
    Import,
    /// The fixture with its media moved out from under it, answered with the
    /// folder it moved to.
    RelinkAll,
}

impl Answered {
    /// Build it, and report the path the answer names so a test can assert
    /// against the same one the row was handed.
    pub(super) fn state(self) -> (AppState, PathBuf) {
        match self {
            Self::Open => {
                let path = a_project_of_two_clips();
                let mut state = saved();
                state.supply_pick(Answer::file(&path));
                (state, path)
            }
            Self::SaveAs => {
                let path = own_scratch("save-as").join("renamed.aria");
                let mut state = saved();
                state.supply_pick(Answer::file(&path));
                (state, path)
            }
            Self::SaveTemplate => {
                let path = own_scratch("template").join("menu.ariat");
                let (mut state, _) = with_clip();
                state.supply_pick(Answer::file(&path));
                (state, path)
            }
            Self::UseTemplate => {
                let path = own_scratch("use-template").join("menu.ariat");
                let (mut state, _) = with_clip();
                state
                    .save_template(path.clone(), "Menu".into())
                    .expect("save a bundle to open");
                state.new_project().expect("empty the session");
                state.supply_pick(Answer::file(&path));
                (state, path)
            }
            Self::Import => {
                let staged = own_scratch("import").join("second.png");
                std::fs::copy(super::fixture(), &staged).expect("stage a file to import");
                let (mut state, _) = with_clip();
                state.supply_pick(Answer::files(vec![staged.clone()]));
                (state, staged)
            }
            Self::RelinkAll => {
                let source = own_scratch("relink").join("clip.png");
                let moved = own_scratch("relink-moved");
                std::fs::copy(super::fixture(), &source).expect("stage a file to lose");
                let (mut state, _) = with_clip();
                state.import_media(source.clone()).expect("import it");
                std::fs::rename(&source, moved.join("clip.png"))
                    .expect("move it out from under the project");
                crate::projection::invalidate_media_statuses();
                state.rebuild_store();
                state.supply_pick(Answer::file(&moved));
                (state, moved)
            }
        }
    }
}

/// A second project written to disk, told apart from the fixture by its clip
/// count.
fn a_project_of_two_clips() -> PathBuf {
    let (mut state, _) = with_clip();
    inside_the_clip(&mut state);
    state
        .split_at_playhead()
        .expect("split so it has two clips");
    let path = own_scratch("other").join("two-clips.aria");
    state.project_path = Some(path.clone());
    state.save_project().expect("write the other project");
    path
}
