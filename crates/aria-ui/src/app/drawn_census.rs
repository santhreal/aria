//! What the drawn surfaces send, measured by driving the rows they are built
//! from.
//!
//! # Why
//!
//! The palette, the menu bar and the chords are tables, so a census can walk
//! them. Everything else an operator presses — a lane header toggle, a mixer
//! strip control, an inspector block, a sidebar button — was a `div()` with its
//! action sealed inside a gpui closure. Nothing could enumerate those, so the
//! forward reach census could see about a third of the product and had to take
//! the rest on trust: ledger row 140 is that gap, and about forty
//! operator-reachable commands sat outside every measurement because of it.
//!
//! The fix is not a wider fixture. It is that a hand-built control is declared
//! as a row the panel draws *from*, so the same function the pointer runs can be
//! run by a test. This module drives every such registry against a dressed
//! editor state and reports the commands they asked for, which
//! `operator_surface_tests::reached` folds in beside the three tables.
//!
//! One rule for anything added here: drive the row's own `run`, never a
//! re-implementation of it. A census that calls `AppState` directly measures the
//! test, not the surface.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use super::AppState;
use super::premiere_tests::with_clip;
use crate::surfaces::lane_header::{LANE_TOGGLES, LaneFlags, LaneToggle};

/// Every command a drawn surface dispatched, across every registry below.
pub(super) fn dispatched() -> &'static BTreeSet<&'static str> {
    static CENSUS: LazyLock<BTreeSet<&'static str>> = LazyLock::new(|| {
        let mut out = BTreeSet::new();
        out.extend(lane_header());
        out.extend(align::dispatched());
        out.extend(analysis::dispatched());
        out.extend(bins::dispatched());
        out.extend(menu::dispatched());
        out.extend(labels::dispatched());
        out.extend(rename::dispatched());
        out.extend(pool::dispatched());
        out.extend(pool_pick::dispatched());
        out.extend(ramp::dispatched());
        out.extend(inspector::dispatched());
        out.extend(mixer::dispatched());
        out.extend(inserts::dispatched());
        out.extend(picks::dispatched());
        out.extend(rack::dispatched());
        out.extend(keys::dispatched());
        out.extend(blocks::dispatched());
        out.extend(markers::dispatched());
        out.extend(settings::dispatched());
        out.extend(sidebar::dispatched());
        out.extend(source::dispatched());
        out.extend(home::dispatched());
        out.extend(overlay::dispatched());
        out.extend(timeline::dispatched());
        out
    });
    &CENSUS
}

// --- reading a registry out of its own source ----------------------------
//
// Fail-by-default without a list kept by hand on either side: a registry's
// `what` is a total match, so the compiler forces an arm for every variant and
// these parsers force a stage for every arm. Adding a variant turns a census
// red until someone dresses a state for it.

/// One registry file's source.
fn surface_source(registry: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/surfaces")
        .join(registry);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("the registry `{registry}` is readable: {err}"))
}

/// Every arm of a registry's `what`, by the prose each returns.
fn arms_of(registry: &str) -> BTreeSet<String> {
    let source = surface_source(registry);
    let body = source
        .split_once("pub fn what(&self)")
        .unwrap_or_else(|| panic!("`{registry}` says what each row means"))
        .1
        .split_once("\n    }")
        .unwrap_or_else(|| panic!("`{registry}`'s `what` match ends"))
        .0;
    let mut out = BTreeSet::new();
    for rest in body.split("=> \"").skip(1) {
        let (prose, _) = rest.split_once('"').expect("the prose is a quoted string");
        out.insert(prose.to_string());
    }
    assert!(!out.is_empty(), "`{registry}` parsed to no rows at all");
    out
}

/// Every `AppState` method one registry's `apply` calls.
fn setters_of(registry: &str) -> BTreeSet<String> {
    let source = surface_source(registry);
    let body = source
        .split_once("pub fn apply(")
        .unwrap_or_else(|| panic!("`{registry}` applies a row"))
        .1;
    let mut out = BTreeSet::new();
    for rest in body.split("state.").skip(1) {
        let (name, _) = rest.split_once('(').expect("a call is a call");
        out.insert(name.to_string());
    }
    assert!(!out.is_empty(), "`{registry}`'s `apply` calls nothing");
    out
}

/// The source of every panel file a census reads: one file, a directory of them,
/// or every part of a panel that has been split.
///
/// A panel is a directory when it grew into one and a single file when it did
/// not, so `under` names either: `"settings_dialog"` reads the directory and
/// `"markers.rs"` reads that one file. A test file beside a panel is not a
/// panel: it drives the model directly on purpose, which is what a test of the
/// model does, so this reads only the files that draw.
///
/// The third case is why `under` is not just a path. A panel over the file
/// ceiling is split into `<stem>_<part>.rs`, and a guard that named the original
/// file would then read half a panel and pass. Naming the stem covers the parts
/// as they appear.
fn panel_sources(under: &str) -> Vec<(String, String)> {
    let panels = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/panels");
    let named = panels.join(under);
    if named.is_file() {
        return vec![(
            under.to_string(),
            std::fs::read_to_string(&named).expect("a panel file is readable"),
        )];
    }
    let (dir, stem) = if named.is_dir() {
        (named, None)
    } else {
        (panels, Some(under))
    };
    let mut files = Vec::new();
    crate::test_support::collect_rs(&dir, &mut files);
    let mut out = Vec::new();
    for path in files {
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
        else {
            continue;
        };
        let drawn = name != "tests.rs"
            && !name.ends_with("_tests.rs")
            && stem.is_none_or(|stem| name.starts_with(stem));
        if drawn {
            out.push((
                name,
                std::fs::read_to_string(&path).expect("a panel file is readable"),
            ));
        }
    }
    assert!(
        !out.is_empty(),
        "no panel files for {under} under {}",
        dir.display()
    );
    out
}

/// Hold a panel, or a directory of them, to calling none of the methods a
/// registry owns.
///
/// This is the contract every registry in `surfaces` exists for: the panel
/// builds a payload and hands it over. It reads source text, so it cannot see a
/// call reached through an alias or made from another directory.
fn no_setter_escapes(registry: &str, under: &str) {
    let owned = setters_of(registry);
    for (name, source) in panel_sources(under) {
        for line in source.lines() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            for setter in &owned {
                assert!(
                    !code.contains(&format!("state.{setter}(")),
                    "`{name}` calls `{setter}` itself, so `{registry}` is no \
                     longer the only writer and a census cannot enumerate what \
                     the panel offers"
                );
            }
        }
    }
}

// One surface per file: each drives its own registry, and each carries the tests
// that say what that registry must hold. The dressed states they press against
// live one level up in `crate::app::fixtures`, because a mixer strip, a menu row
// and an inspector chip all want the same clip with sound.
#[path = "drawn_census_align.rs"]
mod align;
#[path = "drawn_census_analysis.rs"]
mod analysis;
#[path = "drawn_census_bins.rs"]
mod bins;
#[path = "drawn_census_blocks.rs"]
mod blocks;
#[path = "drawn_census_home.rs"]
mod home;
#[path = "drawn_census_inserts.rs"]
mod inserts;
#[path = "drawn_census_inspector.rs"]
mod inspector;
#[path = "drawn_census_keys.rs"]
mod keys;
#[path = "drawn_census_labels.rs"]
mod labels;
#[path = "drawn_census_markers.rs"]
mod markers;
#[path = "drawn_census_menu.rs"]
mod menu;
#[path = "drawn_census_mixer.rs"]
mod mixer;
#[path = "drawn_census_overlay.rs"]
mod overlay;
#[path = "drawn_census_picks.rs"]
mod picks;
#[path = "drawn_census_pool.rs"]
mod pool;
#[path = "drawn_census_pool_pick.rs"]
mod pool_pick;
#[path = "drawn_census_rack.rs"]
mod rack;
#[path = "drawn_census_ramp.rs"]
mod ramp;
#[path = "drawn_census_rename.rs"]
mod rename;
#[path = "drawn_census_settings.rs"]
mod settings;
#[path = "drawn_census_sidebar.rs"]
mod sidebar;
#[path = "drawn_census_source.rs"]
mod source;
#[path = "drawn_census_timeline.rs"]
mod timeline;

/// Press every toggle in a lane header, once per row, on the fixture's lane.
///
/// Each row is pressed against a state of its own: a mute that lands and a lock
/// that lands change what the next row would ask for, and the census is after
/// what each control sends, not what a sequence of presses sends.
fn lane_header() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for row in LANE_TOGGLES {
        let (mut state, _) = with_clip();
        let flags = drawn_on(&state, row);
        let watch = super::sent_census::Watch::on(&state);
        if let Err(err) = (row.run)(&mut state, flags) {
            panic!("the lane header's {} toggle was refused: {err}", row.id);
        }
        out.extend(watch.take());
    }
    out
}

/// The first lane of `state` whose header draws `row`, as the header copies it.
///
/// A row a lane does not draw cannot be pressed there, so a census that pressed
/// it anyway would measure a click no operator can make.
fn drawn_on(state: &AppState, row: &LaneToggle) -> LaneFlags {
    state
        .store
        .sequence
        .tracks
        .iter()
        .map(LaneFlags::of)
        .find(|lane| (row.shown)(*lane))
        .unwrap_or_else(|| panic!("no lane in the fixture draws the {} toggle", row.id))
}

#[test]
fn every_lane_header_toggle_sends_a_command() {
    // A toggle that asks for nothing is a dead control: it draws, it lights, and
    // the lane is unchanged. Driven per row rather than as a set, so the failure
    // names the toggle.
    for row in LANE_TOGGLES {
        let (mut state, _) = with_clip();
        let lane = drawn_on(&state, row);
        let watch = super::sent_census::Watch::on(&state);
        (row.run)(&mut state, lane).expect("a header toggle was refused");
        assert!(
            !watch.take().is_empty(),
            "the {} toggle sent no command",
            row.id
        );
    }
}

#[test]
fn a_toggle_acts_on_the_lane_it_was_drawn_for() {
    // The flags are copied while the header draws and carried into the press. If
    // a row re-read the state instead, it would act on whatever the next frame
    // projected — and a mute pressed twice in one frame would toggle nothing.
    let (mut state, _) = with_clip();
    let lane = LaneFlags::of(
        state
            .store
            .sequence
            .tracks
            .first()
            .expect("the fixture projects a lane"),
    );
    let mute = LANE_TOGGLES
        .iter()
        .find(|row| row.id == "mute")
        .expect("the header has a mute toggle");
    assert!(!lane.muted, "the fixture's lane starts muted");

    (mute.run)(&mut state, lane).expect("mute lands");
    state.sync_from_engine();
    assert!(
        state
            .store
            .sequence
            .tracks
            .first()
            .expect("the lane is still projected")
            .muted,
        "pressing mute left the lane unmuted"
    );

    // The same flags again: the control the operator saw said "muted: false", so
    // pressing it a second time from that same draw asks for the same thing.
    (mute.run)(&mut state, lane).expect("mute lands again");
    state.sync_from_engine();
    assert!(
        state
            .store
            .sequence
            .tracks
            .first()
            .expect("the lane is still projected")
            .muted,
        "the second press read the state instead of the draw it came from"
    );
}

#[test]
fn the_lane_header_census_is_not_empty() {
    // Every assertion above is vacuous if the registry empties out.
    assert!(!LANE_TOGGLES.is_empty());
    assert!(!lane_header().is_empty(), "the lane header sent nothing");
}

#[test]
fn every_lane_toggle_has_its_own_element_id() {
    // gpui keys hover and animation state by element id, so two toggles sharing
    // one id light up together.
    let mut ids: Vec<&str> = LANE_TOGGLES.iter().map(|row| row.id).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "two lane toggles share an element id");
}

#[test]
fn pressing_a_toggle_flips_the_state_it_lights_on() {
    // A control lights on the state its own press produces. Wire `lit` to a
    // neighbouring flag and the button stays dark while the lane obeys it, which
    // is the failure an operator reads as the click not landing.
    for row in LANE_TOGGLES {
        let (mut state, _) = with_clip();
        let before = drawn_on(&state, row);
        (row.run)(&mut state, before).expect("a header toggle was refused");
        state.sync_from_engine();
        let after = LaneFlags::of(
            state
                .store
                .sequence
                .tracks
                .iter()
                .find(|lane| lane.id == before.id)
                .expect("the lane is still projected"),
        );
        assert_ne!(
            (row.lit)(before),
            (row.lit)(after),
            "the {} toggle draws the same either side of its own press",
            row.id
        );
    }
}

#[test]
fn each_toggle_lights_on_one_lane_flag() {
    // Read two flags and the button lies about one of them; read none and it is
    // a lamp. Measured by flipping each flag in turn from an all-off lane, so a
    // new toggle is covered the moment it is registered.
    let off = LaneFlags {
        id: 0,
        muted: false,
        solo: false,
        locked: false,
        enabled: false,
        collapsed: false,
        targeted: false,
        sync_lock: false,
        channel: Some(aria_models::SourceChannel::Video),
        patched: false,
    };
    let flags: [(&str, fn(&mut LaneFlags)); 8] = [
        ("muted", |lane| lane.muted = true),
        ("solo", |lane| lane.solo = true),
        ("locked", |lane| lane.locked = true),
        ("enabled", |lane| lane.enabled = true),
        ("collapsed", |lane| lane.collapsed = true),
        ("targeted", |lane| lane.targeted = true),
        ("sync_lock", |lane| lane.sync_lock = true),
        ("patched", |lane| lane.patched = true),
    ];
    for row in LANE_TOGGLES {
        let mut read: Vec<&str> = Vec::new();
        for (name, flip) in flags {
            let mut one = off;
            flip(&mut one);
            if (row.lit)(one) != (row.lit)(off) {
                read.push(name);
            }
        }
        assert_eq!(
            read.len(),
            1,
            "the {} toggle lights on {read:?} rather than on one lane flag",
            row.id
        );
    }
}

#[test]
fn a_hidden_lane_lights_the_visibility_toggle() {
    // The one control whose lit state is not its flag: the eye reads as an alarm,
    // because a lane that is not drawing is the thing an operator is hunting for.
    let mut lane = LaneFlags {
        id: 0,
        muted: false,
        solo: false,
        locked: false,
        enabled: true,
        collapsed: false,
        targeted: false,
        sync_lock: false,
        channel: None,
        patched: false,
    };
    let vis = LANE_TOGGLES
        .iter()
        .find(|row| row.id == "vis")
        .expect("the header has a visibility toggle");
    assert!(
        !(vis.lit)(lane),
        "a visible lane draws its eye lit, so nothing marks a hidden one"
    );
    lane.enabled = false;
    assert!((vis.lit)(lane), "a hidden lane draws no mark on its header");
}

#[test]
fn the_lane_header_is_built_from_the_registry() {
    // Every test above drives the rows. They say nothing about the panel unless
    // the panel is built from the same rows, and a header that stops consuming
    // the registry loses seven controls without turning anything red. So the two
    // structural facts worth pinning: the file that draws lane headers walks the
    // whole list, and it puts what it built into the header.
    //
    // Not covered: a drawn control that ignores its row's `run` and calls the
    // session itself. Nothing in the language forbids that, and no census can see
    // it, because the census drives the row.
    let header =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/panels/timeline/headers.rs");
    let drawn = std::fs::read_to_string(&header).expect("the lane header module is readable");
    for line in ["for toggle in LANE_TOGGLES {", ".children(toggles)"] {
        assert!(
            drawn.contains(line),
            "{} has no `{line}`, so the census measures rows the header does not draw",
            header.display()
        );
    }
}
