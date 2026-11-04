//! Contracts for the numbers a dialog takes.
//!
//! Closes the class "a dialog number a gesture writes past its dialog, or past
//! the values the dialog takes": a readout that writes the project before the
//! dialog applies, a drag that runs off the end of the draft's span into a
//! refusal, an Escape that leaves a dragged or refused value behind, a typed
//! value outside the span that closes its field, a Tab that leaves the dialog
//! for a panel readout painted behind it or stops at the last readout, an
//! Enter that commits the field without applying the dialog, and a field that
//! outlives its dialog or the Audio Gain mode it opened on, or commits on the
//! way out.
//!
//! Every registered dialog is opened over a selected still with the Inspector
//! drawing its Transform readouts behind it, and the dialog's readouts are read
//! from the frame. [`takes`] states the readouts each dialog draws, matched
//! exhaustively over [`DialogKind`], so a new dialog does not compile until it
//! states them, and a readout added to a dialog fails until it is stated. Each
//! drawn readout is driven through every case. A dialog whose readouts can be
//! refused fails the typed case until [`refusal`] reads the reason it prints.
//!
//! Not covered: a number a dialog prints outside a readout, such as a Settings
//! stepper's value, and a dialog registered by a plugin outside this crate.

use std::collections::HashSet;
use std::ops::RangeInclusive;

use gpui::{Entity, SharedString, TestAppContext, VisualTestContext};

use crate::app::clip_ops::AudioGainMode;
use crate::app::value_scrub::{PX_PER_UNIT, ScrubTarget, Write, clamp_into};
use crate::app::{AppState, DialogKind};
use crate::keys::fields;
use crate::motion::MotionKey;
use crate::panels::value_readout::{DrawnReadout, drawn_readouts, record_readouts};
use crate::plugins::overlay_host::host_of;
use crate::test_support::{drawn_shell, select_a_still};

/// Pointer travel, in pixels, past either end of every finite span drawn.
const TRAVEL: f32 = 1.0e6;

/// The readouts `kind` draws, in draw order.
fn takes(kind: DialogKind) -> Vec<MotionKey> {
    let velocity = |side: &str| {
        let key = MotionKey::new("keyframe-velocity", side);
        [key.with("speed"), key.with("influence")]
    };
    match kind {
        DialogKind::SpeedDuration => vec![
            MotionKey::new("speed-duration", "speed"),
            MotionKey::new("speed-duration", "duration"),
        ],
        DialogKind::AudioGain => vec![MotionKey::new("audio-gain", "db")],
        DialogKind::KeyframeVelocity => [velocity("in"), velocity("out")].concat(),
        DialogKind::None
        | DialogKind::Export
        | DialogKind::Settings
        | DialogKind::Palette
        | DialogKind::Jobs
        | DialogKind::ProjectReport
        | DialogKind::MediaSupport
        | DialogKind::TimelineReport
        | DialogKind::UnsavedChanges
        | DialogKind::InstallPlugin
        | DialogKind::About
        | DialogKind::RemoveAttributes
        | DialogKind::MarkerEdit
        | DialogKind::Plugin(_) => Vec::new(),
    }
}

/// The reason `kind` prints for a value it refused.
fn refusal(state: &AppState, kind: DialogKind) -> Option<SharedString> {
    match kind {
        DialogKind::SpeedDuration => state.speed_duration.error.clone(),
        DialogKind::AudioGain => state.audio_gain.error.clone(),
        DialogKind::KeyframeVelocity => state.keyframe_velocity.error.clone(),
        _ => None,
    }
}

/// The shell over a selected still with the Inspector on its Transform
/// readouts, and motion reduced so a dialog opens at rest.
fn drawn<'a>(
    cx: &'a mut TestAppContext,
    tag: &str,
) -> (Entity<AppState>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, tag);
    visual.update(|_, app| app.set_reduce_motion(true));
    act(&state, visual, |state| {
        select_a_still(state);
        state.inspector_tab = "transform".to_string();
        state.dock_tree.open(crate::dock::PANEL_INSPECTOR);
    });
    (state, visual)
}

/// Change the state as a handler does, and let the frame it asks for draw.
fn act<T>(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    change: impl FnOnce(&mut AppState) -> T,
) -> T {
    let out = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let out = change(state);
            cx.notify();
            out
        })
    });
    visual.run_until_parked();
    out
}

fn read<T>(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    look: impl FnOnce(&AppState) -> T,
) -> T {
    visual.read(|app| look(state.read(app)))
}

/// Press a readout and let go without moving, which opens it as text.
fn open_field(state: &mut AppState, target: &ScrubTarget) {
    state.press_value(target.clone());
    state.release_value();
}

/// A frame drawn anew: the readouts it drew, the last draw under each key, in
/// draw order. While a field is open, the frame records its neighbours.
fn redraw(visual: &mut VisualTestContext) -> Vec<DrawnReadout> {
    record_readouts();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let mut drawn = drawn_readouts();
    let mut seen = HashSet::new();
    drawn.reverse();
    drawn.retain(|readout| seen.insert(readout.target.key));
    drawn.reverse();
    drawn
}

/// The readout drawn under `key` on a frame drawn anew.
fn drawn_under(visual: &mut VisualTestContext, key: MotionKey) -> ScrubTarget {
    let Some(readout) = redraw(visual)
        .into_iter()
        .find(|readout| readout.target.key == key)
    else {
        panic!("the frame drew no readout under {key:?}");
    };
    readout.target
}

/// The values a dialog readout's draft takes.
fn span_of(target: &ScrubTarget) -> RangeInclusive<f32> {
    match &target.write {
        Write::Draft { span, .. } => span.clone(),
        Write::Project(_) => panic!("{target:?} writes the project"),
    }
}

/// Equal to within the digits a dialog prints, relative to the value's size.
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-3 * b.abs().max(1.0)
}

/// A value inside the draft's span a unit from the one `target` shows.
fn another(target: &ScrubTarget) -> f32 {
    let span = span_of(target);
    let up = clamp_into(&span, target.value + 1.0);
    if close(up, target.value) {
        clamp_into(&span, target.value - 1.0)
    } else {
        up
    }
}

/// The dialog kinds the overlay registry holds.
fn registered(state: &Entity<AppState>, visual: &mut VisualTestContext) -> Vec<DialogKind> {
    read(state, visual, |state| {
        let Some(host) = host_of(&state.ctx) else {
            panic!("the ui plugin mounts the overlay host");
        };
        host.dialog_kinds()
    })
}

/// The registered dialogs that draw readouts, with the readouts they state.
fn dialogs_with_readouts(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
) -> Vec<(DialogKind, Vec<MotionKey>)> {
    let kinds = registered(state, visual);
    let dialogs: Vec<_> = kinds
        .into_iter()
        .map(|kind| (kind, takes(kind)))
        .filter(|(_, keys)| !keys.is_empty())
        .collect();
    assert!(!dialogs.is_empty(), "no registered dialog states a readout");
    dialogs
}

/// Every registered dialog draws the readouts [`takes`] states, in that
/// order, and each writes the dialog's draft rather than the project.
#[gpui::test]
fn every_dialog_draws_the_readouts_it_states(cx: &mut TestAppContext) {
    let (state, visual) = drawn(cx, "dialog-readouts");
    let behind: HashSet<MotionKey> = redraw(visual)
        .into_iter()
        .map(|readout| readout.target.key)
        .collect();
    let kinds = registered(&state, visual);
    assert!(
        kinds.len() >= 7,
        "the shipped dialogs did not register: {kinds:?}"
    );
    let mut offenders = Vec::new();
    for kind in kinds {
        act(&state, visual, |state| state.open_dialog(kind));
        assert_eq!(
            read(&state, visual, |state| state.dialog),
            kind,
            "{kind:?} did not open"
        );
        let drawn: Vec<DrawnReadout> = redraw(visual)
            .into_iter()
            .filter(|readout| !behind.contains(&readout.target.key))
            .collect();
        let keys: Vec<MotionKey> = drawn.iter().map(|readout| readout.target.key).collect();
        if keys != takes(kind) {
            offenders.push(format!(
                "{kind:?} drew {:?}, and states {} readouts",
                drawn
                    .iter()
                    .map(|readout| &readout.text)
                    .collect::<Vec<_>>(),
                takes(kind).len()
            ));
        }
        for readout in drawn.iter().filter(|readout| !readout.target.drafts()) {
            offenders.push(format!(
                "{kind:?}: {} writes the project before the dialog applies",
                readout.text
            ));
        }
    }
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// A drag on a dialog readout sets the draft to the value its travel reaches,
/// held at either end of the draft's span, draws no refusal and records no
/// undo step; Escape during the drag sets the value the press found.
#[gpui::test]
fn a_drag_holds_the_draft_inside_its_span_and_escape_restores_it(cx: &mut TestAppContext) {
    let (state, visual) = drawn(cx, "dialog-readout-drag");
    for (kind, keys) in dialogs_with_readouts(&state, visual) {
        act(&state, visual, |state| state.open_dialog(kind));
        for key in keys {
            let target = drawn_under(visual, key);
            let span = span_of(&target);
            let depth = read(&state, visual, AppState::history_depth);
            for dx in [TRAVEL, -TRAVEL] {
                let pressed = target.clone();
                act(&state, visual, |state| {
                    state.press_value(pressed);
                    state.drag_value(dx, false, false);
                });
                let reached = drawn_under(visual, key).value;
                let travelled = clamp_into(&span, target.value + dx / PX_PER_UNIT * target.unit);
                assert!(
                    close(reached, travelled),
                    "{kind:?}: {dx} px from {} reached {reached}, not {travelled}",
                    target.value
                );
                assert_eq!(
                    read(&state, visual, |state| refusal(state, kind)),
                    None,
                    "{kind:?}: a drag held inside the span drew a refusal"
                );
                act(&state, visual, AppState::cancel_value_drag);
                let restored = drawn_under(visual, key).value;
                assert!(
                    close(restored, target.value),
                    "{kind:?}: Escape left {restored}, not the {} the press found",
                    target.value
                );
                assert_eq!(
                    read(&state, visual, AppState::history_depth),
                    depth,
                    "{kind:?}: a dialog readout recorded an undo step"
                );
            }
        }
    }
}

/// A typed value outside the draft's span is refused: the field stays open
/// with the text, the dialog prints the reason, and the draft keeps its value.
/// Escape closes the field and the reason, and leaves the dialog up. A typed
/// value inside the span sets the draft and records no undo step.
#[gpui::test]
fn a_typed_value_outside_the_span_keeps_its_field_until_escape(cx: &mut TestAppContext) {
    let (state, visual) = drawn(cx, "dialog-readout-type");
    let mut refused = 0;
    for (kind, keys) in dialogs_with_readouts(&state, visual) {
        act(&state, visual, |state| state.open_dialog(kind));
        for key in keys {
            let target = drawn_under(visual, key);
            let span = span_of(&target);
            let depth = read(&state, visual, AppState::history_depth);
            if *span.end() < f32::MAX {
                let typed = target.kind.format(*span.end() + 10.0);
                let took = act(&state, visual, |state| {
                    open_field(state, &target);
                    fields::insert(state, &typed);
                    state.commit_value_entry()
                });
                assert!(!took, "{kind:?}: {typed}, past the span's end, was taken");
                let (draft, reason) = read(&state, visual, |state| {
                    (
                        state
                            .value_scrubs()
                            .entry_for(key)
                            .map(|entry| entry.draft.as_str().to_owned()),
                        refusal(state, kind),
                    )
                });
                assert_eq!(
                    draft.as_deref(),
                    Some(typed.as_str()),
                    "{kind:?}: a refused value closed its field"
                );
                assert!(
                    reason.is_some(),
                    "{kind:?}: the dialog printed no reason for {typed}"
                );
                assert!(
                    close(drawn_under(visual, key).value, target.value),
                    "{kind:?}: a refused value moved the draft"
                );

                visual.simulate_keystrokes("escape");
                visual.run_until_parked();
                let (open, reason, dialog) = read(&state, visual, |state| {
                    (state.value_entry_open(), refusal(state, kind), state.dialog)
                });
                assert!(!open, "{kind:?}: Escape left the field open");
                assert_eq!(
                    reason, None,
                    "{kind:?}: Escape left the reason for a value the draft no longer holds"
                );
                assert_eq!(
                    dialog, kind,
                    "{kind:?}: Escape in the field closed the dialog"
                );
                assert!(
                    close(drawn_under(visual, key).value, target.value),
                    "{kind:?}: Escape left the refused value in the draft"
                );
                refused += 1;
            }

            let typed = target.kind.format(clamp_into(&span, target.value + 1.0));
            let Some(expected) = target.kind.parse(&typed) else {
                panic!("{kind:?}: the printed {typed} reads as no value");
            };
            let took = act(&state, visual, |state| {
                open_field(state, &target);
                fields::insert(state, &typed);
                state.commit_value_entry()
            });
            assert!(took, "{kind:?}: {typed}, inside the span, was refused");
            assert!(
                !read(&state, visual, AppState::value_entry_open),
                "{kind:?}: a taken value kept its field"
            );
            let set = drawn_under(visual, key).value;
            assert!(
                close(set, expected),
                "{kind:?}: {typed} set the draft to {set}"
            );
            assert_eq!(
                read(&state, visual, AppState::history_depth),
                depth,
                "{kind:?}: a typed dialog value recorded an undo step"
            );
        }
    }
    assert!(refused > 0, "no dialog readout has a span to type past");
}

/// Tab walks a dialog's readouts in the order the dialog draws them and wraps
/// past the last; Shift+Tab walks them back across the wrap to the first.
/// Neither reaches a readout the Inspector paints behind the dialog. A dialog
/// with one readout closes its field on Tab. Escape closes an open field and
/// keeps the dialog.
#[gpui::test]
fn tab_walks_a_dialogs_readouts_both_ways_and_never_leaves_it(cx: &mut TestAppContext) {
    let (state, visual) = drawn(cx, "dialog-readout-tab");
    let behind: HashSet<MotionKey> = redraw(visual)
        .into_iter()
        .map(|readout| readout.target.key)
        .collect();
    assert!(
        !behind.is_empty(),
        "the Inspector drew no readout behind the dialogs"
    );
    for (kind, keys) in dialogs_with_readouts(&state, visual) {
        act(&state, visual, |state| state.open_dialog(kind));
        let frame = redraw(visual);
        assert!(
            frame
                .iter()
                .any(|readout| behind.contains(&readout.target.key)),
            "{kind:?}: no panel readout was painted behind the dialog"
        );
        let count = keys.len();
        let forward: Vec<Option<MotionKey>> = if count == 1 {
            vec![None]
        } else {
            (1..=count).map(|at| Some(keys[at % count])).collect()
        };
        let backward: Vec<Option<MotionKey>> = if count == 1 {
            vec![None]
        } else {
            (0..count).rev().map(|at| Some(keys[at])).collect()
        };
        for (stroke, walk) in [("tab", forward), ("shift-tab", backward)] {
            let first = drawn_under(visual, keys[0]);
            act(&state, visual, |state| open_field(state, &first));
            for (step, expected) in walk.into_iter().enumerate() {
                // The frame drawn with the field open records its neighbours.
                redraw(visual);
                visual.simulate_keystrokes(stroke);
                visual.run_until_parked();
                let (on, open) = read(&state, visual, |state| {
                    let scrubs = state.value_scrubs();
                    (
                        keys.iter()
                            .copied()
                            .find(|&key| scrubs.entry_for(key).is_some()),
                        scrubs.entry_open(),
                    )
                });
                match expected {
                    Some(key) => assert_eq!(
                        on,
                        Some(key),
                        "{kind:?}: {stroke} {step} opened readout {:?} of {keys:?} (a field \
                         open elsewhere: {})",
                        on.and_then(|on| keys.iter().position(|&key| key == on)),
                        open && on.is_none()
                    ),
                    None => assert!(!open, "{kind:?}: {stroke} reopened the only readout"),
                }
            }
        }
        if count == 1 {
            // Tab closed the only readout's field, and Escape with no field
            // open closes the dialog: the Escape contract is read on an open one.
            redraw(visual);
            let first = drawn_under(visual, keys[0]);
            act(&state, visual, |state| open_field(state, &first));
        }
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        let (open, dialog) = read(&state, visual, |state| {
            (state.value_entry_open(), state.dialog)
        });
        assert!(
            !open && dialog == kind,
            "{kind:?}: Escape left the field open or closed the dialog"
        );
    }
}

/// Closing a dialog, as its Cancel does, closes a field open in it and drops
/// the typed text: the dialog opened again draws the value the field opened
/// on. A change of Audio Gain mode closes a field open on the previous mode's
/// number, and the typed text sets no number.
#[gpui::test]
fn a_field_closes_uncommitted_with_its_dialog_or_mode(cx: &mut TestAppContext) {
    let (state, visual) = drawn(cx, "dialog-readout-close");
    for (kind, keys) in dialogs_with_readouts(&state, visual) {
        act(&state, visual, |state| state.open_dialog(kind));
        for key in keys {
            let target = drawn_under(visual, key);
            let typed = target.kind.format(another(&target));
            act(&state, visual, |state| {
                open_field(state, &target);
                fields::insert(state, &typed);
                state.open_dialog(DialogKind::None);
            });
            assert!(
                !read(&state, visual, AppState::value_entry_open),
                "{kind:?}: the field outlived the dialog"
            );
            act(&state, visual, |state| state.open_dialog(kind));
            let shown = drawn_under(visual, key).value;
            assert!(
                close(shown, target.value),
                "{kind:?}: closing the dialog committed {typed}: it opened again on {shown}, \
                 not {}",
                target.value
            );
        }
    }

    let key = MotionKey::new("audio-gain", "db");
    act(&state, visual, |state| {
        state.open_dialog(DialogKind::AudioGain)
    });
    for mode in AudioGainMode::ALL.into_iter().rev() {
        let target = drawn_under(visual, key);
        let form = read(&state, visual, |state| state.audio_gain.clone());
        assert_ne!(form.mode, mode, "the walk stayed on {mode:?}");
        let typed = target.kind.format(another(&target));
        act(&state, visual, |state| {
            open_field(state, &target);
            fields::insert(state, &typed);
            state.set_audio_gain_mode(mode);
        });
        let (open, after) = read(&state, visual, |state| {
            (state.value_entry_open(), state.audio_gain.clone())
        });
        assert!(!open, "{mode:?}: the field outlived the mode it opened on");
        let mut expected = form.clone();
        expected.mode = mode;
        assert_eq!(
            after, expected,
            "{:?} to {mode:?}: the typed {typed} reached the form",
            form.mode
        );
    }
}

/// Enter in a dialog's open field is the dialog's Enter: the dialog applies
/// with the typed value, as one undo step, and closes.
#[gpui::test]
fn enter_in_a_dialog_field_applies_the_dialog_with_the_typed_value(cx: &mut TestAppContext) {
    let (state, visual) = drawn(cx, "dialog-readout-enter");
    act(&state, visual, AppState::open_speed_duration);
    let target = drawn_under(visual, MotionKey::new("speed-duration", "speed"));
    let (depth, clip) = read(&state, visual, |state| {
        let Some(clip) = state.selected_clip() else {
            panic!("the still is not selected");
        };
        (state.history_depth(), clip)
    });
    act(&state, visual, |state| {
        open_field(state, &target);
        fields::insert(state, "50");
    });
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    let (dialog, open, applied, speed) = read(&state, visual, |state| {
        (
            state.dialog,
            state.value_entry_open(),
            state.history_depth(),
            state
                .ui_clip_ref(clip)
                .map(|clip| (clip.speed_num, clip.speed_den)),
        )
    });
    assert_eq!(
        dialog,
        DialogKind::None,
        "Enter committed the field and left the dialog up"
    );
    assert!(!open, "the field outlived the dialog");
    assert_eq!(
        applied,
        depth + 1,
        "the dialog did not apply as one undo step"
    );
    assert_eq!(
        speed,
        Some((1, 2)),
        "the dialog applied a speed other than 50%"
    );
}
