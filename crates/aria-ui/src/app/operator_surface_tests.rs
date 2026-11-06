//! The forward direction of `Reach`: what the editor's surfaces actually send,
//! measured against what the command rows claim.
//!
//! # Why
//!
//! `Reach` is a promise made in the crate that owns the commands, and until now
//! only its inverse was checked: a palette row may not send a command declared
//! unreachable. Nothing looked the other way, so a command declared
//! operator-reachable with no control anywhere was invisible to the suite.
//! `OpenSequence`, `CreateNestedSequence`, `PlaceNestedSequence` and
//! `AddMulticamClip` all shipped that way.
//!
//! Each row now names the surface it is reached through, and three of those
//! surfaces are tables a test can drive: the palette (`BUILTIN_COMMANDS`), the
//! menu bar, and the shipped chords. What they send is observed at
//! `apply_command` rather than read out of the history list, so a row that
//! saves, opens or exports counts as a surface even though it writes no history
//! entry.
//!
//! Two things are asserted from that measurement. A command a driven surface
//! sends must not be declared `Planned` or `Internal` — a claim that nothing
//! reaches it, contradicted by something reaching it. And a command whose row
//! names one of those three tables must actually be sent by one, so a row
//! cannot claim the palette and be reached from nowhere.
//!
//! Every hand-built control that sends a command dispatches through a registry
//! under `surfaces`, and `drawn_census` presses those registries. This closes the
//! old blind spot for inspector blocks, context-menu items, canvas handles, the
//! export dialog, clip fades, and the speed curve.
//!
//! A driven row can still be disabled by the fixture it receives. The census
//! therefore dresses each registry with the state its rows require rather than
//! treating an inert press as coverage.

use std::collections::BTreeSet;

use aria_commands::{Reach, edit_specs, project_specs};

use super::premiere_tests::with_clip;
use crate::keys::KeyBody;
use crate::keys::bindings::SHIPPED_BINDINGS;

/// The words a driven surface may state in a UI reach declaration.
///
/// This is a closed allowlist, not an exception table. Every word corresponds to
/// a registry the census presses below. A command that states a new surface
/// turns the suite red until that surface has a driven registry and its word is
/// added here.
///
/// `menu's` covers the bar wholesale, and it can: the menu census declares a
/// state for every row the bar draws, and a row with no declaration fails
/// `the_table_names_every_menu_row_and_nothing_else`. A row that opens a file
/// dialog is driven with the answer supplied, so it dispatches like any other.
/// `settings dialog's` covers the project section for the same reason: every
/// aspect, size, rate and layout row it draws is pressed, off the catalogs the
/// panel loops over. `markers panel` covers that panel's three edits: the add
/// button, a marker's colour swatch and its delete button, each pressed against
/// a marker the panel would have drawn. `source monitor's` covers the monitor's
/// bar: both marks, the clear, the subclip, and the two edits, pressed with and
/// without a range marked on the sequence, because that range is what turns
/// either edit into a fit. `sidebar's` covers the pool sidebar's own buttons:
/// the multicam button and one chip per angle of the group it built.
/// `analysis panel` covers that panel's clears, filters, seeks
/// and both edits, pressed on a project whose index the fixture wrote, plus the
/// beat run's own write, which is the surface `Set beats` names. `on the
/// timeline` covers the clip drags: every body drag, every edge drag and both
/// edges of a transition's window, pressed in ticks against two abutting clips
/// with a blend at the cut, with linked selection on and off. `handles on the
/// preview` covers the canvas overlay: the move, the four scale corners, the
/// eight crop handles and the three mask handles, each dragged
/// press-move-release against a stated picture rectangle.
/// `Home screen's` covers the route's tiles, a recent card's actions and the
/// recovery banner, pressed against a saved session so a tile runs its intent
/// rather than parking behind the unsaved-changes prompt.
const TABLE_CLAIMS: &[&str] = &[
    "palette",
    "chord",
    "context menu",
    "menu's",
    "settings dialog's",
    "markers panel",
    "source monitor's",
    "sidebar's",
    "analysis panel",
    "on the timeline",
    "handles on the preview",
    "Home screen's",
    "lane header",
    "label",
    "bin",
    "rename",
    "parameter fields",
    "the mixer's",
    "Look block",
    "Grade block",
    "Styles block",
    "Audio block",
    "Animation block",
    "Speed block",
    "Template block",
    "Text block",
    "Effects block",
    "keyframe list",
];

/// Every command a shipped key chord dispatched.
///
/// A chord is driven the way a palette row is. A windowed chord takes the entity
/// context and queues a job instead of editing; `windowed_reach_tests` drives
/// those, and a job is not a command, so nothing is measured here for them.
///
/// Each chord is driven with linked selection on and off: the toggle selects
/// whether a trim or a nudge carries a clip's linked companions, which is a
/// different command.
fn chord_dispatches() -> &'static BTreeSet<&'static str> {
    static CENSUS: std::sync::LazyLock<BTreeSet<&'static str>> = std::sync::LazyLock::new(|| {
        let mut out = BTreeSet::new();
        for binding in SHIPPED_BINDINGS {
            let KeyBody::Plain(run) = binding.run else {
                continue;
            };
            for linked in [true, false] {
                let (mut state, _) = with_clip();
                state.linked_selection = linked;
                if let Some(ui) = state.selected_ui_clip() {
                    let mid = ui.timeline_start_ticks
                        + (ui.timeline_end_ticks - ui.timeline_start_ticks) / 2;
                    state.set_playhead(mid);
                }
                let watch = super::sent_census::Watch::on(&state);
                run(&mut state);
                out.extend(watch.take());
            }
        }
        out
    });
    &CENSUS
}

/// Every command a driven surface asked for: the palette, the bar, the chords,
/// and every drawn registry `drawn_census` can walk.
fn reached() -> BTreeSet<&'static str> {
    let mut out: BTreeSet<&'static str> =
        super::reach_tests::dispatched().iter().copied().collect();
    out.extend(super::menu_reach_tests::dispatched().iter().copied());
    out.extend(chord_dispatches().iter().copied());
    out.extend(super::drawn_census::dispatched().iter().copied());
    out
}

/// Every command's label with its declared reach, derived from the command
/// registry at run time.
fn declared() -> Vec<(&'static str, Reach)> {
    edit_specs()
        .iter()
        .map(|spec| (spec.label, spec.reach))
        .chain(project_specs().iter().map(|spec| (spec.label, spec.reach)))
        .collect()
}

/// One measurement, every claim held against it.
///
/// `reached()` presses every registry, which is most of this file's cost.
/// nextest runs each test in its own process, so each test reading it paid for
/// it again; one test reads it once and reports every claim's failures
/// together.
#[test]
fn the_measured_reach_agrees_with_every_declared_row() {
    let reached = reached();
    let declared = declared();
    let mut wrong = Vec::new();
    // A harness that observed nothing would pass every claim below by
    // measuring an empty set.
    for (census, sent) in [
        ("palette", super::reach_tests::dispatched().len()),
        ("menu", super::menu_reach_tests::dispatched().len()),
        ("chord", chord_dispatches().len()),
        ("drawn registry", super::drawn_census::dispatched().len()),
    ] {
        if sent == 0 {
            wrong.push(format!("the {census} census saw no command"));
        }
    }
    // The strong direction: this is measured, not described. A row stating
    // that nothing reaches it, while a table reaches it, is a false row.
    for (label, reach) in &declared {
        if !reach.is_operator() && reached.contains(label) {
            wrong.push(format!(
                "{label} declares no operator surface and is sent by a palette row, \
                 a menu row, a chord or a drawn registry"
            ));
        }
    }
    for (label, reach) in &declared {
        let Some(surface) = reach.surface() else {
            continue;
        };
        if !TABLE_CLAIMS.iter().any(|word| surface.contains(word)) {
            wrong.push(format!(
                "{label} states an undriven surface {surface:?}; add its registry to the census"
            ));
        } else if !reached.contains(label) {
            wrong.push(format!(
                "{label} states {surface:?}, but driving that surface sent nothing"
            ));
        }
    }
    for word in TABLE_CLAIMS {
        if !declared
            .iter()
            .any(|(_, reach)| reach.surface().is_some_and(|s| s.contains(word)))
        {
            wrong.push(format!(
                "TABLE_CLAIMS entry {word:?} is not claimed by any command in the registry"
            ));
        }
    }
    // Why the census watches dispatch instead of history: saving asks for a
    // command and writes no history entry, so a history-based census reports it
    // as unreached. If this fails, the harness has gone back to measuring edits
    // only.
    if !reached.contains("Save project") {
        wrong.push("a saving surface is no longer observed".into());
    }
    if super::reach_tests::measured().contains("Save project") {
        wrong.push("saving now writes history, so the dispatch comparison proves nothing".into());
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn every_operator_command_has_a_surface() {
    // The forward direction as a declaration: a command cannot be
    // operator-reachable without someone writing down the control, which is the
    // moment a missing control is noticed.
    let mut wrong = Vec::new();
    for (label, reach) in declared() {
        match reach {
            Reach::Operator(surface) => {
                if surface.len() < 21 || !surface.contains(' ') {
                    wrong.push(format!("{label} claims a surface without naming it"));
                }
            }
            Reach::Planned(missing) => {
                if !missing.contains("ledger row") {
                    wrong.push(format!("{label} is planned and names no ledger row"));
                }
            }
            Reach::Internal(reason) => {
                if !reason.contains(' ') {
                    wrong.push(format!("{label} is internal with no prose reason"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
