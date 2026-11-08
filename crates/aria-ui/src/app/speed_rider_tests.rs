//! Contracts for the lanes a speed change moves in Ripple mode.
//!
//! WHY: a speed or a ramp that lengthened a clip in Ripple mode moved the
//! clips after it on its own lane and on its linked clips' lanes only, so a
//! sync-locked lane, which every other ripple moves, stayed behind and fell out
//! of step with the clips after the retimed one. The class closed is a ripple
//! on the speed paths selecting its lanes by another rule than a ripple trim of
//! the clip's tail: Speed/Duration and a ramp both take the trim's lanes now.
//!
//! Not covered: a locked sync-locked lane, and a clip lying across the old end
//! on a sync-locked lane.

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, MediaId, TrackId, TrackKind, speed_preset};

use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::types::EditMode;

/// Where each clip on `lane` sits, in start order, as the store holds it.
fn spans(state: &AppState, lane: TrackId) -> Vec<(i64, i64)> {
    let mut spans: Vec<(i64, i64)> = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.id == lane.raw())
        .expect("lane projected")
        .clips
        .iter()
        .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
        .collect();
    spans.sort_unstable();
    spans
}

/// A picture lane added above the others, holding a copy of `like` at the same
/// place, with its sync lock set or cleared.
fn lane_with_copy(state: &mut AppState, like: ClipId, sync_lock: bool) -> TrackId {
    let lane = state
        .add_lane_of_kind(TrackKind::Video, None)
        .expect("add a lane");
    let (media, source, start) = {
        let engine = state.engine().lock();
        let clip = engine.project().clip(like).expect("the clip to copy");
        let aria_models::ClipSource::Media { media, .. } = &clip.content else {
            panic!("the clip to copy is not media");
        };
        (
            *media,
            clip.source_range().expect("a media clip"),
            clip.timeline.start,
        )
    };
    state
        .apply_command(Command::Edit(EditCommand::AddClip {
            track: lane,
            media,
            source,
            start,
        }))
        .expect("place the copy");
    if !sync_lock {
        state
            .toggle_track_sync_lock(lane)
            .expect("clear the sync lock");
    }
    state.sync_from_engine();
    lane
}

/// A speed change that lengthens the clip: Speed/Duration at 50 %, or the Hero
/// ramp.
#[derive(Debug, Clone, Copy)]
enum Retime {
    Slower,
    Ramp,
}

#[test]
fn a_clip_lengthened_by_speed_in_ripple_mode_moves_every_sync_locked_lane() {
    for retime in [Retime::Slower, Retime::Ramp] {
        let (mut state, first) = with_clip();
        let first_ui = state.ui_clip_ref(first).expect("first clip").clone();
        let cut = first_ui.timeline_end_ticks;
        let media = MediaId::from_raw(first_ui.media_id.expect("media"));
        let lane = TrackId::from_raw(first_ui.track_id);
        state
            .drop_media_on_timeline(media, Some(lane), cut)
            .expect("place the neighbor");
        let neighbor = state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .find(|clip| clip.id != first.raw() && clip.timeline_start_ticks == cut)
            .map(|clip| ClipId::from_raw(clip.id))
            .expect("the neighbor");
        let end = state
            .ui_clip_ref(neighbor)
            .expect("neighbor")
            .timeline_end_ticks;
        let riding = lane_with_copy(&mut state, neighbor, true);
        let pinned = lane_with_copy(&mut state, neighbor, false);
        state.select_clip(first);
        state.edit_mode = EditMode::Ripple;

        match retime {
            Retime::Slower => {
                state.open_speed_duration();
                state.set_speed_duration_percent(50.0).expect("50%");
                state.apply_speed_duration();
                assert_eq!(state.speed_duration.error, None, "{retime:?}");
            }
            Retime::Ramp => state
                .apply_speed_curve_edit(first, Some(speed_preset("hero").expect("the Hero preset")))
                .expect("hero in Ripple mode"),
        }
        let added = state.ui_clip_ref(first).expect("first").timeline_end_ticks - cut;
        assert!(added > 0, "{retime:?} did not lengthen the clip");
        assert_eq!(
            spans(&state, lane),
            vec![(0, cut + added), (cut + added, end + added)],
            "{retime:?}: the neighbor moves by the added length"
        );
        assert_eq!(
            spans(&state, riding),
            vec![(cut + added, end + added)],
            "{retime:?}: a sync-locked lane moves with the clips after the retimed one"
        );
        assert_eq!(
            spans(&state, pinned),
            vec![(cut, end)],
            "{retime:?}: a lane with its sync lock cleared stays"
        );

        assert!(state.undo(), "{retime:?}: undo the speed change");
        assert_eq!(
            [spans(&state, lane), spans(&state, riding)],
            [vec![(0, cut), (cut, end)], vec![(cut, end)]],
            "{retime:?}: one undo restores every lane"
        );
    }
}
