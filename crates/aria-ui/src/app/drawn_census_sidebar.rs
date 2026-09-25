//! Driving the pool sidebar's own controls: Make Subclip, the multicam button,
//! and a group's angle chips.
//!
//! # Why
//!
//! Three commands claimed these buttons and nothing pressed them, and two of the
//! three threw their result away, so a refusal and a success looked the same on
//! screen. The controls are a registry now
//! ([`sidebar_pick`](crate::surfaces::sidebar_pick)) and this presses every one
//! of them, reading the arms back out of the registry's own source.
//!
//! Each row is pressed against a project that offers it, which is what the panel
//! does: Make Subclip is only drawn while the monitor holds a marked range, and
//! an angle chip only exists once a group does. The group the angle rows are
//! pressed on is built by the button above them, so the census walks the same
//! order an operator does.

use std::collections::BTreeSet;

use aria_models::MulticamGroupId;

use crate::app::AppState;
use crate::app::fixtures::marked_source;
use crate::app::sent_census::Watch;
use crate::surfaces::sidebar_pick::{self, Pick};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "sidebar_pick.rs";

/// A project with the monitor marked, a pool of two items, and one multicam
/// group built from them.
///
/// Two items because a group of one angle has nothing to cut to: the second
/// angle is what makes the angle chips worth pressing. Both are selected, which
/// is what the button reads: the pool holds a selection in pick order, so the
/// angles are in the order they were picked.
fn with_group() -> (AppState, MulticamGroupId) {
    let (mut state, first) = marked_source();
    let second = state
        .import_media(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/texture.png"),
        )
        .expect("a second pool item to be an angle");
    state.sync_from_engine();
    state.select_pool_item(first);
    state.pool_selection.toggle(second.raw());
    press(&mut state, Pick::MulticamGroup);
    let group = state
        .multicam_groups()
        .last()
        .map(|group| MulticamGroupId::from_raw(group.id))
        .expect("the button built a group");
    with_sound(&mut state, [first, second.raw()]);
    (state, group)
}

/// Give both angles sound the sync row can measure.
///
/// The fixture files are a marked source and a texture, neither of which the
/// decoder would find audio in, and a census that pressed Sync Audio against
/// silence would only ever prove the refusal. The second envelope is the first
/// shifted, which is exactly the situation the row exists for: two cameras that
/// did not start rolling together. Pseudo-random rather than a repeating tick,
/// because a sound that repeats matches everywhere and the row declines it.
fn with_sound(state: &mut AppState, media: [u64; 2]) {
    let mut seed = 0xa11cu32;
    let peaks: Vec<f32> = (0..600)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 16) as f32 / 65_535.0
        })
        .collect();
    let shifted = peaks[40..].to_vec();
    for (id, peaks) in media.into_iter().zip([peaks, shifted]) {
        state.waveforms.insert(
            id,
            crate::waveforms::Waveform {
                peaks: std::sync::Arc::new(peaks),
                per_second: 40.0,
            },
        );
    }
}

/// Every control the sidebar draws, in the order it draws them.
///
/// The angle rows come off the group the state holds rather than out of a list
/// here: a chip the panel never drew proves nothing.
fn picks(state: &AppState) -> Vec<Pick> {
    let mut out = vec![Pick::Subclip, Pick::MulticamGroup];
    for row in state.multicam_groups().iter() {
        let group = MulticamGroupId::from_raw(row.id);
        out.push(Pick::MulticamPlace { group });
        out.push(Pick::MulticamSync { group });
        for angle in 0..row.angles.len() {
            out.push(Pick::MulticamAngle { group, angle });
            out.push(Pick::MulticamNudge {
                group,
                angle,
                frames: 1,
            });
        }
    }
    out
}

/// Press one control the way the sidebar does.
fn press(state: &mut AppState, pick: Pick) {
    let what = pick.what();
    sidebar_pick::apply(state, pick)
        .unwrap_or_else(|err| panic!("pressing {what} was refused: {err}"));
}

/// The `index`th control, and a project dressed for pressing it.
///
/// A group id belongs to the session that built it, so the pick is read off the
/// state it is pressed against rather than off an earlier build.
fn staged(index: usize) -> (AppState, Pick) {
    let (state, _) = with_group();
    let pick = picks(&state)[index];
    (state, pick)
}

/// How many controls the sidebar draws on a dressed project.
fn drawn() -> usize {
    picks(&with_group().0).len()
}

/// Every command the sidebar's controls asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for index in 0..drawn() {
        let (mut state, pick) = staged(index);
        let watch = Watch::on(&state);
        press(&mut state, pick);
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_control_the_sidebar_draws_is_pressed() {
    // Fail by default on a new arm: `what` must cover it to compile, so it is
    // named here whether or not anyone pressed it.
    let pressed: BTreeSet<String> = picks(&with_group().0)
        .into_iter()
        .map(|pick| pick.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !pressed.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "these sidebar controls are drawn and nothing presses them: {missing:?}. \
         The answer is a dressed project this census builds, never an exception"
    );
}

#[test]
fn every_control_sends_something() {
    for index in 0..drawn() {
        let (mut state, pick) = staged(index);
        let watch = Watch::on(&state);
        let what = pick.what();
        press(&mut state, pick);
        assert!(
            !watch.take().is_empty(),
            "pressing {what} sent nothing, so the control is inert"
        );
    }
}

#[test]
fn the_group_holds_one_angle_for_every_pool_item() {
    // The rule the panel used to own and lose: with nothing selected the button
    // groups the pool, so the fixture's two items are two angles. A button that
    // grouped only the first would still build a group, and every other test
    // here would pass.
    let (state, group) = with_group();
    let groups = state.multicam_groups();
    let built = groups
        .iter()
        .find(|row| row.id == group.raw())
        .expect("the group the button built");
    assert_eq!(
        built.angles.len(),
        state.store.media.len(),
        "the group took {} of {} pool items",
        built.angles.len(),
        state.store.media.len()
    );
}

#[test]
fn the_button_says_so_rather_than_building_an_empty_group() {
    // An empty pool is the one state the model refuses, and the panel used to
    // discard that refusal. The registry answers before the model does, so the
    // press is silent and the operator is told why.
    let mut state = AppState::new(
        aria_plugin::PluginContext::new(),
        crate::scratch_pool::scratch("sidebar-empty-pool"),
    )
    .expect("a session");
    state.new_project().expect("a project");
    let watch = Watch::on(&state);
    press(&mut state, Pick::MulticamGroup);
    assert!(
        watch.take().is_empty(),
        "a group was built out of an empty pool"
    );
    assert_eq!(
        state.status_message.as_deref(),
        Some("Import media to build a multicam group"),
        "the button did nothing and said nothing"
    );
}

#[test]
fn the_census_reaches_every_command_these_controls_claim() {
    let sent = dispatched();
    for label in [
        "Create subclip",
        "Create multicam group",
        "Switch multicam angle",
        "Place multicam clip",
    ] {
        assert!(
            sent.contains(label),
            "the sidebar census pressed every control it draws and none of them \
             sent `{label}`"
        );
    }
}

/// Every clip on the sequence as `(id, length in ticks)`.
fn clip_spans(state: &AppState) -> Vec<(u64, i64)> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| (clip.id, clip.timeline_end_ticks - clip.timeline_start_ticks))
        .collect()
}

#[test]
fn a_placed_group_is_as_long_as_its_longest_camera() {
    // The length is derived, not assumed: a group is as long as the last frame
    // any of its cameras can still answer for, so a placement cannot end before
    // a cut to a longer camera does. The clip that lands is measured against the
    // group's own span rather than against the first angle's file.
    let (mut state, group) = with_group();
    let want = {
        let engine = state.engine().lock();
        engine
            .project()
            .multicam_group_span(group)
            .expect("the group has angles, so it has a span")
    };
    let before = clip_spans(&state);
    press(&mut state, Pick::MulticamPlace { group });
    state.sync_from_engine();
    let after = clip_spans(&state);
    let landed: Vec<i64> = after
        .iter()
        .filter(|(id, _)| !before.iter().any(|(was, _)| was == id))
        .map(|(_, len)| *len)
        .collect();
    assert_eq!(
        landed,
        vec![want.duration.value],
        "one press puts one clip on a lane, as long as the group reaches"
    );
}

#[test]
fn a_group_takes_a_lane_of_its_own_rather_than_being_refused() {
    // A placement is an add, so a span that is already occupied would be
    // refused, and a button that fails on any full timeline is not a control.
    // With the playhead inside the fixture's own clip the group gets a lane; past
    // everything on the lane it lands on the lane that is already there.
    let (mut state, group) = with_group();
    let lanes = state.store.sequence.tracks.len();
    press(&mut state, Pick::MulticamPlace { group });
    state.sync_from_engine();
    assert_eq!(
        state.store.sequence.tracks.len(),
        lanes + 1,
        "the playhead sits inside a clip, so the group needed a lane of its own"
    );

    let free = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| clip.timeline_end_ticks)
        .max()
        .expect("the sequence holds clips");
    let lanes = state.store.sequence.tracks.len();
    state.set_playhead(free);
    state.sync_from_engine();
    press(&mut state, Pick::MulticamPlace { group });
    state.sync_from_engine();
    assert_eq!(
        state.store.sequence.tracks.len(),
        lanes,
        "nothing was under the playhead, so the group should not have added a lane"
    );
}

#[test]
fn neither_sidebar_file_calls_a_setter_itself() {
    for panel in ["sidebar_multicam.rs", "sidebar_subclip.rs"] {
        super::no_setter_escapes(REGISTRY, panel);
    }
}
