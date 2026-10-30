//! The mixer surface: what a strip reads, what a click on it sends, and what a
//! fader drag costs in history.
//!
//! The class these close is the one the mixer is most likely to grow: a control
//! that draws a number the mix does not use. Every case here reads the strip the
//! panel would draw — never the model directly — and then checks the command it
//! sent, so a strip that shows a value it never writes, or writes a value it
//! never shows, goes red.
//!
//! What they do not cover: the drawing itself. A strip's colours, its layout and
//! its hover fades need a render surface, and the panel is exercised through
//! `plugins::ui`'s panel census instead.

use crate::app::AppState;
use crate::app::mixer::{
    FaderTarget, frac_to_gain, frac_to_pan, gain_label, gain_to_frac, pan_label, pan_to_frac,
};
use crate::app::premiere_tests::with_clip;
use aria_models::{AudioSend, AutomationMode, TrackId, TrackKind};

/// The height a fader is drawn at in the panel, so a drag here travels the same
/// distance a pointer does there.
const TRAVEL: f32 = 132.0;

/// A project with one audio lane and `buses` submix buses.
///
/// Named lanes rather than ids, because the mixer addresses lanes by id and a
/// test that built the ids itself would not be reading what the panel reads.
fn with_lanes(buses: usize) -> (AppState, TrackId, Vec<TrackId>) {
    let (mut state, _clip) = with_clip();
    state.add_track(TrackKind::Audio, "A1").expect("audio lane");
    for at in 1..=buses {
        state
            .add_track(TrackKind::Submix, format!("Bus {at}"))
            .expect("bus lane");
    }
    let strips = state.mixer_strips().expect("a project with no cycles");
    let lane = strips
        .iter()
        .find(|strip| strip.name == "A1")
        .map(|strip| TrackId::from_raw(strip.track))
        .expect("the audio lane is in the mixer");
    let bus_ids = (1..=buses)
        .map(|at| {
            strips
                .iter()
                .find(|strip| strip.name == format!("Bus {at}"))
                .map(|strip| TrackId::from_raw(strip.track))
                .expect("each bus is in the mixer")
        })
        .collect();
    (state, lane, bus_ids)
}

/// The buses one lane's strip offers, read the way the panel reads them: off the
/// strip the projection already carries, not through a second lookup by id.
fn sends_offered(state: &AppState, track: TrackId) -> Vec<String> {
    state
        .mixer_strips()
        .expect("a project with no cycles")
        .iter()
        .find(|strip| strip.track == track.raw())
        .expect("the lane is in the mixer")
        .send_targets
        .iter()
        .map(|target| target.name.to_string())
        .collect()
}

/// One strip, holding the projection it came from, so a test can read a
/// temporary field by field the way the panel reads it.
struct Strip {
    mix: std::sync::Arc<[crate::app::mixer::MixerStrip]>,
    at: usize,
}

impl std::ops::Deref for Strip {
    type Target = crate::app::mixer::MixerStrip;

    fn deref(&self) -> &Self::Target {
        &self.mix[self.at]
    }
}

fn strip_named(state: &AppState, name: &str) -> Strip {
    let mix = state.mixer_strips().expect("no cycle");
    let at = mix
        .iter()
        .position(|strip| strip.name == name)
        .unwrap_or_else(|| panic!("no strip named {name}"));
    Strip { mix, at }
}

#[test]
fn the_mixer_holds_the_audio_lanes_and_the_buses_and_nothing_else() {
    let (state, _lane, _buses) = with_lanes(1);
    let names: Vec<String> = state
        .mixer_strips()
        .expect("no cycle")
        .iter()
        .map(|strip| strip.name.to_string())
        .collect();
    // "Main" is the fixture's video lane: it carries its clips' audio, so it has
    // a fader like any other source. Membership is the catalog's `in_mixer()`
    // answer, not a list here — a lane that carries no sound has no strip.
    assert_eq!(
        names,
        vec!["Bus 1".to_string(), "A1".to_string(), "Main".to_string()]
    );
}

#[test]
fn a_bus_is_drawn_after_everything_that_feeds_it() {
    let (mut state, lane, buses) = with_lanes(2);
    // A1 → Bus 2 → Bus 1: the strips must come out in that order, whatever
    // order the lanes were added in.
    state
        .set_track_output(lane, Some(buses[1]))
        .expect("route the lane");
    state
        .set_track_output(buses[1], Some(buses[0]))
        .expect("nest the bus");
    let order: Vec<String> = state
        .mixer_strips()
        .expect("no cycle")
        .iter()
        .map(|strip| strip.name.to_string())
        .collect();
    assert_eq!(
        order,
        vec![
            "A1".to_string(),
            "Main".to_string(),
            "Bus 2".to_string(),
            "Bus 1".to_string()
        ],
        "a bus sits after every lane that feeds it, and Bus 1 after Bus 2"
    );
}

#[test]
fn a_send_chip_cycles_off_post_pre_off() {
    let (mut state, lane, buses) = with_lanes(1);
    let bus = buses[0];
    assert!(strip_named(&state, "A1").sends.is_empty());

    state.cycle_send(lane, bus).expect("add the send");
    let post = strip_named(&state, "A1");
    assert_eq!(post.sends.len(), 1);
    assert!(!post.sends[0].pre_fader, "the first click is post-fader");
    assert_eq!(post.sends[0].to_name, "Bus 1");

    state.cycle_send(lane, bus).expect("make it pre-fader");
    assert!(strip_named(&state, "A1").sends[0].pre_fader);

    state.cycle_send(lane, bus).expect("drop the send");
    assert!(strip_named(&state, "A1").sends.is_empty());

    // Three clicks, three undo entries, and undoing the third brings the
    // pre-fader send back rather than a post-fader one.
    assert!(state.undo());
    let back = strip_named(&state, "A1");
    assert_eq!(back.sends.len(), 1);
    assert!(back.sends[0].pre_fader);
}

#[test]
fn a_send_offers_every_bus_but_never_the_lane_itself() {
    let (state, lane, buses) = with_lanes(2);
    let names = sends_offered(&state, lane);
    assert_eq!(names, vec!["Bus 1".to_string(), "Bus 2".to_string()]);
    // A bus can feed another bus, and cannot send to itself.
    let from_bus = sends_offered(&state, buses[0]);
    assert_eq!(from_bus, vec!["Bus 2".to_string()]);
}

#[test]
fn the_outputs_lead_with_the_master_and_mark_the_live_one() {
    let (mut state, lane, buses) = with_lanes(1);
    let targets = state.output_targets(lane);
    assert_eq!(targets[0].name, "Master");
    assert!(targets[0].current, "a fresh lane goes to the master");
    assert_eq!(targets[1].name, "Bus 1");

    state.set_track_output(lane, Some(buses[0])).expect("route");
    let targets = state.output_targets(lane);
    assert!(!targets[0].current);
    assert!(targets[1].current, "the bus is now the live output");
    assert_eq!(strip_named(&state, "A1").output_name, "Bus 1");
}

#[test]
fn riding_a_fader_is_one_undo_entry() {
    let (mut state, lane, _buses) = with_lanes(0);
    let before = state.history_entries().len();
    let target = FaderTarget::Lane(lane);

    state.begin_fader_drag(target, 100.0, TRAVEL);
    // Three moves down the strip: the strip follows every one of them, and the
    // engine sees none of them.
    assert!(state.drag_fader_to_pointer(120.0));
    assert!(state.drag_fader_to_pointer(140.0));
    assert!(state.drag_fader_to_pointer(160.0));
    assert_eq!(state.history_entries().len(), before);
    let drafted = state.fader_dragging().expect("a drag in flight").gain;
    assert!(drafted < 1.0, "dragging down is quieter, got {drafted}");
    // What the panel draws is the projection's stored value with the drag
    // drafted over it, which is why the strip moves while the engine has not
    // been told anything.
    assert_eq!(
        state.fader_value(target, strip_named(&state, "A1").gain),
        drafted,
        "the strip draws the dragged value, not the stored one"
    );
    assert_eq!(
        strip_named(&state, "A1").gain,
        1.0,
        "and the stored value has not moved yet"
    );

    state.commit_fader_drag().expect("commit");
    assert!(state.fader_dragging().is_none());
    assert_eq!(state.history_entries().len(), before + 1);
    assert_eq!(strip_named(&state, "A1").gain, drafted);
    assert!(state.undo());
    assert_eq!(strip_named(&state, "A1").gain, 1.0);
}

#[test]
fn a_press_that_moves_nothing_writes_nothing() {
    let (mut state, lane, _buses) = with_lanes(0);
    let before = state.history_entries().len();
    state.begin_fader_drag(FaderTarget::Lane(lane), 40.0, TRAVEL);
    // A click: the pointer never leaves the grab, and a non-finite position is
    // refused rather than committed.
    assert!(!state.drag_fader_to_pointer(40.0));
    assert!(!state.drag_fader_to_pointer(f32::NAN));
    state.commit_fader_drag().expect("release");
    assert_eq!(state.history_entries().len(), before);
    // Abandoning also writes nothing.
    state.begin_fader_drag(FaderTarget::Lane(lane), 40.0, TRAVEL);
    assert!(state.drag_fader_to_pointer(10.0));
    state.cancel_fader_drag();
    assert_eq!(state.history_entries().len(), before);
    assert_eq!(strip_named(&state, "A1").gain, 1.0);
    // And with nothing grabbed, a move is inert.
    assert!(!state.drag_fader_to_pointer(10.0));
}

#[test]
fn a_send_level_is_dragged_on_the_send_it_belongs_to() {
    let (mut state, lane, buses) = with_lanes(1);
    state.cycle_send(lane, buses[0]).expect("add the send");
    let target = FaderTarget::Send {
        track: lane,
        index: 0,
    };
    state.begin_fader_drag(target, 100.0, TRAVEL);
    assert!(state.drag_fader_to_pointer(130.0));
    let drafted = state.fader_dragging().expect("in flight").gain;
    // The send's own row follows the drag; the lane's fader does not move.
    let strip = strip_named(&state, "A1");
    assert_eq!(
        state.fader_value(target, strip.sends[0].level),
        drafted,
        "the send's own row follows the drag"
    );
    assert_eq!(
        state.fader_value(FaderTarget::Lane(lane), strip.gain),
        1.0,
        "and the lane's own fader does not move with it"
    );
    state.commit_fader_drag().expect("commit");
    assert_eq!(strip_named(&state, "A1").sends[0].level, drafted);
}

#[test]
fn the_fader_snaps_to_unity_and_bottoms_out_at_silence() {
    // Unity has a detent: a position a hair either side of it reads exactly
    // unity, so a fader can be returned to 0.0 dB by hand.
    let unity = gain_to_frac(1.0);
    assert_eq!(frac_to_gain(unity), 1.0);
    assert_eq!(frac_to_gain(unity + 0.002), 1.0);
    assert_eq!(frac_to_gain(unity - 0.002), 1.0);
    // The bottom is silence itself, not a level that is merely quiet.
    assert_eq!(frac_to_gain(0.0), 0.0);
    assert_eq!(gain_label(0.0), "-inf");
    assert_eq!(gain_label(1.0), "0.0 dB");
    // Outside the detent a position round-trips through the scale, and the
    // top of the travel is the model's own send ceiling.
    assert_eq!(frac_to_gain(1.0), AudioSend::MAX_LEVEL);
    assert_eq!(gain_to_frac(AudioSend::MAX_LEVEL), 1.0);
    assert!(gain_to_frac(AudioSend::MAX_LEVEL * 0.99) < 1.0);
    let quiet = frac_to_gain(0.25);
    assert!((gain_to_frac(quiet) - 0.25).abs() < 0.01);
    assert!(gain_label(quiet).starts_with('-'));
}

#[test]
fn a_lane_that_cannot_reach_the_master_says_so() {
    let (mut state, lane, buses) = with_lanes(1);
    state
        .set_track_output(lane, Some(buses[0]))
        .expect("route the lane");
    assert!(strip_named(&state, "A1").reaches_master);

    state
        .set_track_muted(buses[0], true)
        .expect("mute the bus the lane feeds");
    let feeder = strip_named(&state, "A1");
    assert!(feeder.audible, "the lane itself is not muted");
    assert!(
        !feeder.reaches_master,
        "everything it produces stops at the muted bus"
    );

    // A send past the muted bus is a way back into the mix.
    state.cycle_send(lane, buses[0]).expect("add a send");
    assert!(!strip_named(&state, "A1").reaches_master);
}

#[test]
fn a_refused_route_reports_and_changes_nothing() {
    let (mut state, lane, buses) = with_lanes(2);
    state
        .set_track_output(buses[0], Some(buses[1]))
        .expect("nest the buses");
    // Bus 2 → Bus 1 would close the loop, so it is refused by name and the
    // routing is left as it was.
    assert!(state.set_track_output(buses[1], Some(buses[0])).is_err());
    assert_eq!(strip_named(&state, "Bus 2").output_name, "Master");
    assert_eq!(strip_named(&state, "Bus 1").output_name, "Bus 2");
    // A lane is not a bus, so it cannot be an output either.
    assert!(state.set_track_output(buses[0], Some(lane)).is_err());
    assert_eq!(strip_named(&state, "Bus 1").output_name, "Bus 2");
}

#[test]
fn buses_are_named_in_sequence_and_open_the_mixer() {
    let (mut state, _clip) = with_clip();
    state.add_submix_lane().expect("first bus");
    state.add_submix_lane().expect("second bus");
    let names: Vec<String> = state
        .mixer_strips()
        .expect("no cycle")
        .iter()
        .map(|strip| strip.name.to_string())
        .collect();
    assert_eq!(
        names,
        vec!["Bus 1".to_string(), "Bus 2".to_string(), "Main".to_string()],
        "each bus is named after how many the project already had"
    );

    // The palette row is the reachable path to both the pane and the bus.
    let row = crate::commands::BUILTIN_COMMANDS
        .iter()
        .find(|row| row.id == "pal-mixer-add-bus")
        .expect("the palette offers a bus");
    (row.run)(&mut state);
    assert_eq!(state.active_panel, crate::types::ActivePanel::Mixer);
    assert!(state.dock_tree.is_active(crate::dock::PANEL_MIXER));
    assert_eq!(state.mixer_strips().expect("no cycle").len(), 4);
}

#[test]
fn the_mixer_pane_toggles_back_to_the_program_monitor() {
    use crate::types::ActivePanel;
    assert_eq!(
        ActivePanel::Mixer.toggle_to(ActivePanel::Mixer),
        ActivePanel::Preview
    );
    assert_eq!(
        ActivePanel::Timeline.toggle_to(ActivePanel::Mixer),
        ActivePanel::Mixer
    );
    let (mut state, _, _) = with_lanes(0);
    state.focus_pane(ActivePanel::Mixer);
    assert_eq!(state.active_panel, ActivePanel::Mixer);
    assert!(state.dock_tree.is_active(crate::dock::PANEL_MIXER));
    state.close_focused().expect("close mixer tab");
    assert!(!state.dock_tree.is_open(crate::dock::PANEL_MIXER));
    assert_eq!(state.active_panel, ActivePanel::Preview);
}

#[test]
fn pan_has_a_center_detent_and_drags_lane_and_master_as_one_undo_entry() {
    assert_eq!(pan_to_frac(0.0), 0.5);
    assert_eq!(frac_to_pan(0.5), 0.0);
    assert_eq!(frac_to_pan(0.502), 0.0);
    assert_eq!(frac_to_pan(0.498), 0.0);
    assert_eq!(pan_label(0.0), "C");
    assert_eq!(pan_label(-0.5), "L50");
    assert_eq!(pan_label(0.75), "R75");

    let (mut state, lane, _buses) = with_lanes(0);
    let before = state.history_entries().len();
    let target = FaderTarget::LanePan(lane);
    state.begin_fader_drag(target, 50.0, TRAVEL);
    assert!(state.drag_fader_to_pointer(80.0));
    assert!(state.drag_fader_to_pointer(100.0));
    assert_eq!(state.history_entries().len(), before);
    let drafted = state.fader_dragging().expect("pan drag").gain;
    assert!(drafted > 0.0, "dragging right pans right, got {drafted}");
    assert_eq!(
        state.fader_value(target, strip_named(&state, "A1").pan),
        drafted
    );
    state.commit_fader_drag().expect("commit pan");
    assert_eq!(state.history_entries().len(), before + 1);
    assert_eq!(strip_named(&state, "A1").pan, drafted);
    assert!(state.undo());
    assert_eq!(strip_named(&state, "A1").pan, 0.0);
}

#[test]
fn mute_and_solo_in_place_update_strip_audibility_and_master_strip() {
    let (mut state, lane, buses) = with_lanes(1);
    assert!(!strip_named(&state, "A1").muted);
    assert!(!strip_named(&state, "A1").solo);
    assert!(strip_named(&state, "Main").audible);

    state.toggle_strip_solo(lane).expect("solo A1");
    assert!(strip_named(&state, "A1").solo);
    assert!(strip_named(&state, "A1").audible);
    assert!(
        !strip_named(&state, "Main").audible,
        "solo-in-place silences unsoloed source lanes"
    );
    assert!(
        strip_named(&state, "Bus 1").audible,
        "submix buses stay open when a source lane is soloed"
    );
    state.toggle_strip_solo(lane).expect("unsolo A1");
    assert!(strip_named(&state, "Main").audible);

    state.toggle_strip_mute(None).expect("mute master");
    assert!(state.master_chain().muted);
    state.toggle_strip_mute(None).expect("unmute master");
    assert!(!state.master_chain().muted);
    state
        .set_strip_automation_mode(None, AutomationMode::Read)
        .expect("master automation mode");
    assert_eq!(state.master_chain().automation, AutomationMode::Read);
    let _ = buses;
}

/// What each strip's meter reads. Declared here because a meter is part of the
/// strip, and split out to keep this file under the ceiling.
#[path = "audio_submix_meters_tests.rs"]
mod meters;
