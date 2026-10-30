//! Driving the source monitor's bar: the two marks, the clear, and the two
//! edits that land the marked source.
//!
//! # Why
//!
//! Both edits send one command, `Source edit`, and the state the row is pressed
//! against decides its payload: a ripple or a cover, and a span to fill when
//! the sequence carries a marked range as well as the source. There is no
//! fit-to-fill button; a fit is what Insert and Overwrite do with four points
//! of different lengths. That
//! makes the dressing part of the row ([`Dress`]), and it is why the same two
//! picks appear twice in the table. Every edit is also asserted to land on the
//! lanes the lane headers draw patched.
//!
//! The marking rows send nothing: a mark is the monitor's own view of the
//! source, held in session state, and no project changes when one moves. They
//! are in the table anyway, with what they must do instead, because a mark
//! button wired to the wrong end is exactly the defect a census of the edits
//! alone cannot see.

use std::collections::BTreeSet;
use std::sync::Arc;

use parking_lot::Mutex;

use aria_commands::{Command, EditCommand, Patch, SoundPlacement, SourceChannel};
use aria_plugin::CommandProposed;

use crate::app::AppState;
use crate::app::fixtures::marked_source;
use crate::app::sent_census::Watch;
use crate::surfaces::source_pick::{self, Pick};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "source_pick.rs";

/// The state a row is driven against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dress {
    /// The monitor open with a marked source, and nothing marked on the
    /// sequence: an edit lands at the playhead at the source's own length.
    Loaded,
    /// The same, with a range marked on the sequence: the destination has a
    /// length, so an edit retimes the source to fill it.
    Fitted,
}

impl Dress {
    /// Build the state this dressing names, with the pool id the monitor holds.
    fn state(self) -> (AppState, u64) {
        let (mut state, media) = marked_source();
        if self == Dress::Fitted {
            // A range wide enough that filling it retimes the source rather
            // than landing it at its own length.
            state.set_mark_in();
            state.set_playhead(48);
            state.set_mark_out();
            assert!(
                state.marked_range().is_some(),
                "the sequence carries a range to fill"
            );
        }
        (state, media)
    }
}

/// What a row does when it lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lands {
    /// One `Source edit`: a ripple or a cover, retimed to the sequence's
    /// marked range or at the source's own length.
    Edit { ripple: bool, fill: bool },
    /// Nothing on the project: the monitor's own marks move.
    Marks,
}

/// Every control the bar draws, with the state it is driven against and what it
/// must do there.
///
/// Both edits appear twice because the sequence's own marks change which command
/// they send, and a row that only reached one of the two would leave the other
/// unproven.
const ROWS: &[(Pick, Dress, Lands)] = &[
    (Pick::MarkIn, Dress::Loaded, Lands::Marks),
    (Pick::MarkOut, Dress::Loaded, Lands::Marks),
    (Pick::ClearMarks, Dress::Loaded, Lands::Marks),
    (
        Pick::Insert,
        Dress::Loaded,
        Lands::Edit {
            ripple: true,
            fill: false,
        },
    ),
    (
        Pick::Overwrite,
        Dress::Loaded,
        Lands::Edit {
            ripple: false,
            fill: false,
        },
    ),
    (
        Pick::Insert,
        Dress::Fitted,
        Lands::Edit {
            ripple: true,
            fill: true,
        },
    ),
    (
        Pick::Overwrite,
        Dress::Fitted,
        Lands::Edit {
            ripple: false,
            fill: true,
        },
    ),
];

/// Press one control the way the bar does.
fn press(state: &mut AppState, pick: Pick) {
    let what = pick.what();
    source_pick::apply(state, pick)
        .unwrap_or_else(|err| panic!("pressing {what} was refused: {err}"));
}

/// Press one control and return every edit it asked for, payload and all.
fn edits_sent(state: &mut AppState, pick: Pick) -> Vec<EditCommand> {
    let seen: Arc<Mutex<Vec<EditCommand>>> = Arc::default();
    let sink = Arc::clone(&seen);
    let registration = state
        .ctx
        .on_waterfall::<CommandProposed, _>(move |proposed| {
            if let Command::Edit(edit) = &proposed.command {
                sink.lock().push(edit.clone());
            }
        });
    press(state, pick);
    drop(registration);
    std::mem::take(&mut *seen.lock())
}

/// The one `Source edit` among `sent`, as `(ripple, fill, track, sound)`.
fn source_edit(sent: &[EditCommand]) -> (bool, bool, Option<u64>, SoundPlacement) {
    let edits: Vec<_> = sent
        .iter()
        .filter_map(|edit| match edit {
            EditCommand::SourceEdit {
                ripple,
                fill,
                track,
                sound,
                ..
            } => Some((*ripple, fill.is_some(), track.map(|id| id.raw()), *sound)),
            _ => None,
        })
        .collect();
    assert_eq!(edits.len(), 1, "one Source edit among {sent:?}");
    edits[0]
}

/// The monitor's own marks, as the bar draws them.
fn marks(state: &AppState) -> (Option<i64>, Option<i64>) {
    let view = state
        .sources
        .focused()
        .expect("the monitor stays open across a press");
    (view.mark_in, view.mark_out)
}

/// Every command the bar's controls asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for (pick, dress, _) in ROWS {
        let (mut state, _) = dress.state();
        let watch = Watch::on(&state);
        press(&mut state, *pick);
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_control_the_bar_draws_is_pressed() {
    // Fail by default on a new arm: `what` must cover it to compile, so it is
    // named here whether or not the table drives it.
    let pressed: BTreeSet<String> = ROWS
        .iter()
        .map(|(pick, _, _)| pick.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !pressed.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "these monitor controls are drawn and the table drives none of them: \
         {missing:?}. The answer is a row with the state it needs, never an \
         exception"
    );
}

#[test]
fn an_edit_row_sends_the_edit_the_table_names() {
    // The whole point of the second dressing: the same button is a different
    // edit depending on what is marked, and a fit that quietly became an
    // overwrite would leave the clip at the wrong length.
    for (pick, dress, lands) in ROWS {
        let Lands::Edit { ripple, fill } = *lands else {
            continue;
        };
        let (mut state, _) = dress.state();
        let what = pick.what();
        let steps = state.history_depth();
        let sent = edits_sent(&mut state, *pick);
        let (sent_ripple, sent_fill, track, sound) = source_edit(&sent);
        assert_eq!(
            (sent_ripple, sent_fill),
            (ripple, fill),
            "{what} against {dress:?} sent {sent:?}"
        );
        // The picture lands on the lane its header draws lit. The fixture's
        // sequence has no audio lane, so the sound lands on a new one, linked
        // to the picture, and the new lane is part of the edit's undo step.
        let sequence = &state.store.sequence;
        assert!(track.is_some(), "{what} placed the picture");
        assert_eq!(track, sequence.patched_lane(SourceChannel::Video), "{what}");
        let sound_lane = sequence.patched_lane(SourceChannel::Audio);
        assert!(sound_lane.is_some(), "{what} added no audio lane");
        assert_eq!(sound.lane().map(|id| id.raw()), sound_lane, "{what}");
        let history = state.history_entries();
        assert_eq!(
            (
                state.history_depth(),
                history.last().map(|step| step.label.as_str())
            ),
            (steps + 1, Some("Source edit")),
            "{what} is one undo step named for the edit"
        );
    }
}

#[test]
fn an_audio_lane_on_auto_takes_the_sound_as_a_linked_clip() {
    let (mut state, _) = Dress::Loaded.state();
    let sound_lane = state
        .add_lane_of_kind(aria_models::TrackKind::Audio, None)
        .expect("an audio lane");
    let sent = edits_sent(&mut state, Pick::Overwrite);
    let (_, _, track, sound) = source_edit(&sent);
    assert_eq!(sound, SoundPlacement::Lane(sound_lane));
    let picture = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|lane| Some(lane.id) == track)
        .and_then(|lane| lane.clips.iter().max_by_key(|clip| clip.id))
        .expect("the placed picture");
    let sound_clip = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|lane| lane.id == sound_lane.raw())
        .and_then(|lane| lane.clips.first())
        .expect("the placed sound");
    assert!(picture.linked_to.is_some(), "the picture is linked");
    assert_eq!(
        picture.linked_to, sound_clip.linked_to,
        "picture and sound share one link"
    );
}

#[test]
fn an_edit_lands_on_the_patched_lane_and_leaves_an_off_channel_out() {
    // Every patch the header can set, per channel: a lane of the channel's
    // kind other than the one Auto picks, and off. The edit must follow it.
    for channel in SourceChannel::ALL {
        let (mut state, _) = Dress::Loaded.state();
        let kind = channel.lane_kind();
        let extra = state.add_lane_of_kind(kind, None).expect("a second lane");
        state
            .set_source_patch(channel, Patch::Track(extra))
            .expect("patch");
        let sent = edits_sent(&mut state, Pick::Overwrite);
        let (_, _, track, sound) = source_edit(&sent);
        let landed = match channel {
            SourceChannel::Video => track,
            SourceChannel::Audio => sound.lane().map(|id| id.raw()),
        };
        assert_eq!(
            landed,
            Some(extra.raw()),
            "{channel:?} patched to {extra:?}"
        );

        state
            .set_source_patch(channel, Patch::Off)
            .expect("patch off");
        let sent = edits_sent(&mut state, Pick::Overwrite);
        let (_, _, track, sound) = source_edit(&sent);
        match channel {
            SourceChannel::Video => {
                assert_eq!(track, None, "the picture patched off is left out");
                assert!(
                    sound.lane().is_some(),
                    "the sound lands on a lane of its own, {sound:?}"
                );
            }
            SourceChannel::Audio => {
                assert_eq!(
                    sound,
                    SoundPlacement::Off,
                    "the sound patched off is left out"
                );
                assert!(track.is_some(), "the picture still lands");
            }
        }
    }
}

#[test]
fn both_channels_off_sends_nothing() {
    let (mut state, _) = Dress::Loaded.state();
    for channel in SourceChannel::ALL {
        state
            .set_source_patch(channel, Patch::Off)
            .expect("patch off");
    }
    for pick in [Pick::Insert, Pick::Overwrite] {
        let what = pick.what();
        assert!(
            edits_sent(&mut state, pick).is_empty(),
            "{what} with both channels off asked for an edit"
        );
    }
}

#[test]
fn a_marking_row_moves_a_mark_and_asks_for_nothing() {
    // A mark is session state, so the fingerprint is the monitor's own pair. A
    // marking row that reached the engine would put an undo entry on the stack
    // for a click that edited no project.
    for (pick, dress, lands) in ROWS {
        if *lands != Lands::Marks {
            continue;
        }
        let (mut state, media) = dress.state();
        // Park the monitor where neither mark is. A mark button writes the
        // monitor's own tick, so pressing it where the mark already stands is
        // the same value again: an idempotent press proves nothing about which
        // end the button owns.
        state.hover_pool_item(media, 30);
        let before = marks(&state);
        let watch = Watch::on(&state);
        let what = pick.what();
        press(&mut state, *pick);
        assert!(
            watch.take().is_empty(),
            "{what} reached the engine, so marking the source writes history"
        );
        assert_ne!(
            before,
            marks(&state),
            "{what} left both marks where they were"
        );
    }
}

#[test]
fn the_marks_land_on_the_ends_they_name() {
    // Which end each button writes, asserted once: the two are drawn side by
    // side and swapping them is invisible to every test above.
    let (mut state, _) = Dress::Loaded.state();
    press(&mut state, Pick::ClearMarks);
    assert_eq!(marks(&state), (None, None), "the clear drops both");

    press(&mut state, Pick::MarkIn);
    let (in_point, out_point) = marks(&state);
    assert!(in_point.is_some(), "In wrote no in point");
    assert_eq!(out_point, None, "In wrote the out point too");

    press(&mut state, Pick::MarkOut);
    let (_, out_point) = marks(&state);
    assert!(out_point.is_some(), "Out wrote no out point");
}

#[test]
fn the_census_reaches_every_command_these_controls_claim() {
    let sent = dispatched();
    {
        let label = "Source edit";
        assert!(
            sent.contains(label),
            "the monitor census pressed every control the bar draws and none of \
             them sent `{label}`"
        );
    }
}

#[test]
fn the_monitor_calls_no_setter_itself() {
    // The bar is drawn across `preview.rs` and its `preview_<part>.rs` splits;
    // the stem reads every part, including any later split.
    super::no_setter_escapes(REGISTRY, "preview.rs");
    super::no_setter_escapes(REGISTRY, "preview_");
}
