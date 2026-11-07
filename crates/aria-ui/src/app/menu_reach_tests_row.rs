//! Menu row coverage and dispatch tests.
//!
//! Only `#[test]` functions are defined here. The tests reach the fixtures and
//! helpers in `menu_reach_tests.rs` through `use super::*`.

use super::*;

#[test]
fn the_table_names_every_menu_row_and_nothing_else() {
    let live: BTreeSet<String> = live_rows()
        .into_iter()
        .map(|(_, id)| id)
        .filter(|id| by_prefix(id).is_none())
        .collect();
    let declared: BTreeSet<String> = ROWS.iter().map(|(id, ..)| id.to_string()).collect();
    let missing: Vec<_> = live.difference(&declared).collect();
    let stale: Vec<_> = declared.difference(&live).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "undeclared rows {missing:?}, declared rows that no longer exist {stale:?}"
    );
}
#[test]
fn every_row_does_what_it_says() {
    // Collected rather than asserted one at a time: one run should name every
    // row that drifted, not the first.
    let mut wrong = Vec::new();
    for row in &CENSUS.rows {
        match (row.kind, row.wrote.is_empty()) {
            (Edits, true) => wrong.push(format!(
                "{} declared Edits against {:?} and sent nothing",
                row.id, row.dress
            )),
            (NoEdit, false) => wrong.push(format!(
                "{} declared NoEdit against {:?} and sent {:?}",
                row.id, row.dress, row.wrote
            )),
            _ => {}
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
/// A row its guard draws enabled never answers with a decline reason. The
/// guard and the action read one state, so an enabled row that only prints
/// why it did nothing is a guard that reads less than its action. Every row is
/// checked under its own dressing and under the bare state (one clip, nothing
/// selected, the playhead past the end), and every `Decline` reason counts. A
/// guard and action that disagree only in a state neither covers, or an action
/// that declines with wording outside `Decline`, is not caught here.
#[test]
fn an_enabled_row_never_answers_with_a_decline() {
    let reasons: BTreeSet<&str> = crate::app::decline::Decline::ALL
        .iter()
        .map(|why| why.reason())
        .collect();
    let declined = |blocked: Option<&str>, status: &Option<String>| {
        blocked.is_none() && status.as_deref().is_some_and(|line| reasons.contains(line))
    };
    let mut wrong: Vec<String> = CENSUS
        .rows
        .iter()
        .filter(|row| declined(row.blocked, &row.status))
        .map(|row| format!("{} against {:?}: {:?}", row.id, row.dress, row.status))
        .collect();
    wrong.extend(
        CENSUS
            .bare
            .iter()
            .filter(|row| declined(row.blocked, &row.status))
            .map(|row| format!("{} against the bare state: {:?}", row.id, row.status)),
    );
    assert!(
        CENSUS.bare.len() == CENSUS.rows.len(),
        "every declared row runs against the bare state"
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
/// WHY: closes, for the menu bar, the class of a structural refusal shown as
/// the value copy, "That value isn't accepted here", whose fix names a field a
/// menu row does not have: an extraction from a clip that shares its link, or
/// a 33rd mask. Every row is held to it under its own dressing and under the
/// bare state, so a new row is held to it with no edit here.
///
/// What this does not catch: a refusal reached only in a state neither covers,
/// and a form row's committed value, which may be a value the model rejects.
#[test]
fn no_menu_row_answers_with_the_value_copy() {
    let copy = value_copy();
    let mut wrong: Vec<String> = CENSUS
        .rows
        .iter()
        .filter(|row| row.error.as_deref() == Some(copy.as_str()))
        .map(|row| format!("{} against {:?}", row.id, row.dress))
        .collect();
    wrong.extend(
        CENSUS
            .bare
            .iter()
            .filter(|row| row.error.as_deref() == Some(copy.as_str()))
            .map(|row| format!("{} against the bare state", row.id)),
    );
    assert!(
        wrong.is_empty(),
        "these rows answered with the value copy:\n{}",
        wrong.join("\n")
    );
}
/// A row that records an undo step changes the project. A step over no edit
/// spends the next Undo on nothing and marks the project unsaved with nothing
/// to save, while the status line reports an edit. Every row is checked under
/// its own dressing and under the bare state; a row that edits nothing only in
/// a state neither covers, or a step whose edit the project file does not
/// record, is not caught here.
#[test]
fn a_row_that_records_a_step_changes_the_project() {
    let mut wrong: Vec<String> = CENSUS
        .rows
        .iter()
        .filter(|row| !row.wrote.is_empty() && !row.changed)
        .map(|row| {
            format!(
                "{} against {:?} recorded {:?}",
                row.id, row.dress, row.wrote
            )
        })
        .collect();
    wrong.extend(
        CENSUS
            .bare
            .iter()
            .filter(|row| !row.wrote.is_empty() && !row.changed)
            .map(|row| format!("{} against the bare state recorded {:?}", row.id, row.wrote)),
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A row run a second time over its own result changes the project again, as
/// a toggle or a paste does, or records nothing, as a row whose result is
/// already in place must. The census runs every row that recorded a step once
/// more in the same state; a repeat that records a step and changes nothing
/// spends the next Undo on nothing. Rows that open a form are not repeated.
///
/// A new row is held to this with no edit here.
#[test]
fn a_row_run_over_its_own_result_changes_the_project_or_records_nothing() {
    let repeats: Vec<String> = CENSUS
        .rows
        .iter()
        .filter(|row| !row.again.is_empty() && !row.again_changed)
        .map(|row| {
            format!(
                "{} against {:?} repeated {:?}",
                row.id, row.dress, row.again
            )
        })
        .collect();
    assert!(repeats.is_empty(), "{}", repeats.join("\n"));
}

/// WHY: closes the class of rows that record an undo step over nothing when
/// the state already holds their result: Reset Framing on a whole frame,
/// Remove Track Matte with no matte, Linear on Linear keyframes, Time-Reverse
/// on keyframes that play the same reversed, Paste Attributes onto the copy's
/// own clip or a clip that already has the copied look, From Source Monitor
/// onto the frames the clip already plays, Add Keyframe where the keyframe
/// already holds the value, and an ease row over keyframes that already have
/// that easing. Each is drawn disabled with
/// the reason its action declines with, and running it anyway records nothing.
///
/// What this does not catch: a row outside this table that repeats its own
/// result; the census in `a_row_that_records_a_step_changes_the_project` sees
/// only the dressings it drives.
#[test]
fn a_row_over_its_own_result_declines_and_records_nothing() {
    use crate::app::decline::Decline;
    let cases: [(&str, fn() -> AppState, Decline); 20] = [
        ("seq-crop-reset", || Clip.state(), Decline::FullFrame),
        ("seq-matte-clear", || Clip.state(), Decline::NoMatte),
        (
            "anim-interp-linear",
            || Keyed.state(),
            Decline::AlreadyLinear,
        ),
        (
            "anim-time-reverse",
            || Spanned.state(),
            Decline::ReversedSame,
        ),
        (
            "edit-paste-attributes",
            || {
                let (mut state, _) = with_clip();
                state.copy_selected();
                state
            },
            Decline::CopiedFromHere,
        ),
        (
            "edit-paste-attributes",
            || {
                let mut state = Copied.state();
                state.paste_attributes().expect("the look lands");
                state
            },
            Decline::SameAttributes,
        ),
        (
            "clip-replace-source",
            || {
                let mut state = Sourced.state();
                state.replace_with_source().expect("the clip is repointed");
                state
            },
            Decline::SameSource,
        ),
        ("edit-label-violet", || Labelled.state(), Decline::SameLabel),
        (
            "clip-channels-stereo",
            || Sound.state(),
            Decline::SameChannels,
        ),
        ("seq-matte-luma", || Matted.state(), Decline::SameMatte),
        (
            "seq-marker-snap",
            || {
                let mut state = Nearby.state();
                crate::commands::reach::snap_marker(&mut state);
                state
            },
            Decline::MarkerAtPlayhead,
        ),
        (
            "seq-transition-crossfade",
            || {
                let mut state = Cut.state();
                state
                    .set_selected_transition("crossfade")
                    .expect("a crossfade lands");
                state
            },
            Decline::SameTransition,
        ),
        (
            "seq-default-transitions",
            || {
                let mut state = Cut.state();
                state
                    .add_default_transition(aria_models::TransitionMedia::Picture)
                    .expect("the default lands");
                state
            },
            Decline::SameTransition,
        ),
        (
            "anim-add-keyframe",
            || {
                let mut state = Picked.state();
                state
                    .add_keyframes_on_selected_properties()
                    .expect("the keyframe lands");
                state
            },
            Decline::KeyedHere,
        ),
        ("anim-interp-bezier", || Eased.state(), Decline::SameEase),
        (
            "anim-interp-hold",
            || {
                let mut state = Keyed.state();
                state
                    .hold_selected_or_playhead()
                    .expect("the keyframe holds");
                state
            },
            Decline::SameEase,
        ),
        (
            "anim-interp-hold",
            || {
                // Two or more eased keyframes, whose curve points the model
                // re-solves after each write.
                let mut state = Spanned.state();
                state
                    .easy_ease_selected_or_playhead()
                    .expect("the keyframes ease");
                state
                    .hold_selected_or_playhead()
                    .expect("the keyframes hold");
                state
            },
            Decline::SameEase,
        ),
        ("anim-easy-ease", || Eased.state(), Decline::SameEase),
        ("anim-easy-ease-in", || Eased.state(), Decline::SameEase),
        ("anim-easy-ease-out", || Eased.state(), Decline::SameEase),
    ];
    let mut wrong = Vec::new();
    for (id, build, why) in cases {
        let state = build();
        let menu = AppMenu::ALL
            .into_iter()
            .find(|menu| items_of(*menu, &state).iter().any(|item| item.id == id))
            .unwrap_or_else(|| panic!("{id} is on no menu"));
        let probe = probe(menu, id, state, false);
        let reason = Some(why.reason());
        if probe.blocked != reason
            || probe.status.as_deref() != reason
            || !probe.wrote.is_empty()
            || probe.changed
        {
            wrong.push(format!(
                "{id} ({why:?}): drawn {:?}, said {:?}, recorded {:?}, changed {}",
                probe.blocked, probe.status, probe.wrote, probe.changed
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A project file whose eased keyframes omit their solved handles loads with
/// the default handles, which disagree with the eases. Easy Ease over such a
/// keyframe re-solves its handles, so the row is offered and records a step
/// that changes the project. The row's guard runs the model's own write and
/// compares the curve, not the easing the write stores; a guard that compared
/// only the stored easing would draw this row disabled.
#[test]
fn easy_ease_over_unsolved_handles_is_offered_and_resolves_them() {
    fn strip_points(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::Object(temporal)) = map.get_mut("temporal") {
                    temporal.remove("points");
                }
                map.values_mut().for_each(strip_points);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip_points),
            _ => {}
        }
    }
    let mut state = Spanned.state();
    state
        .easy_ease_selected_or_playhead()
        .expect("the keyframes ease");
    let clip = state.selected_clip().expect("the clip is selected");
    let param = state
        .key_params()
        .first()
        .expect("the clip can animate something")
        .param();
    let ui = state.selected_ui_clip().expect("the clip is drawn");
    let (start, end) = (ui.timeline_start_ticks, ui.timeline_end_ticks);
    let solved = project_text(&state);
    let mut file =
        serde_json::to_value(state.engine.lock().project()).expect("the project encodes");
    strip_points(&mut file);
    let loaded: aria_models::Project = serde_json::from_value(file).expect("the file loads");
    state.engine.lock().reset_project(loaded);
    state.sync_from_engine();
    state.select_clip(clip);
    state.marquee_select_keyframes(clip, param, start, end, false);
    assert_ne!(
        project_text(&state),
        solved,
        "the loaded handles are not the solved ones"
    );
    let probe = probe(AppMenu::Animation, "anim-easy-ease", state, false);
    assert_eq!(probe.blocked, None, "the row is offered");
    assert_eq!(probe.status, None, "the row writes without a decline");
    assert_eq!(
        probe.wrote,
        ["Set keyframe ease +1"],
        "one step over both keyframes is recorded"
    );
    assert!(probe.changed, "the handles are re-solved");
}

#[test]
fn every_command_the_menu_sends_is_declared_reachable() {
    // The inverse direction of `Reach`: a command the bar demonstrably sends
    // must not be declared internal.
    let labels = measured();
    for cmd in every_edit_command() {
        if labels.contains(cmd.label()) {
            assert!(
                cmd.reach().is_operator(),
                "{} is declared unreachable but the menu bar sends it",
                cmd.label()
            );
        }
    }
}
#[test]
fn every_row_that_asks_for_a_path_is_driven_with_an_answer() {
    // Derived from the live menus rather than declared twice: a row is run and
    // the state is asked whether it asked for a path. A new row that opens a
    // dialog is red here until it is declared and driven. Reaching this line at
    // all is the other half of the proof: a row that blocked on a dialog would
    // never return.
    let declared: BTreeSet<&str> = PICK_ROWS.iter().map(|(id, _)| *id).collect();
    let undeclared: Vec<&String> = CENSUS
        .asking_picks
        .iter()
        .filter(|id| !declared.contains(id.as_str()))
        .collect();
    assert!(
        undeclared.is_empty(),
        "these rows ask for a path and no test answers one: {undeclared:?}"
    );
}
#[test]
fn every_row_that_opens_a_form_is_committed() {
    // Derived from the live menus: each row runs against its own dressing, and a
    // row that leaves a dialog up whose Enter runs something must be driven with
    // Enter pressed or exempted with a reason. A new form row is red here until
    // someone decides which.
    let committed: BTreeSet<&str> = FORM_ROWS.iter().map(|(id, _)| *id).collect();
    let exempt: BTreeSet<&str> = FORM_EXEMPT.iter().map(|(id, _)| *id).collect();
    let forms: BTreeSet<String> = CENSUS
        .rows
        .iter()
        .filter(|row| row.opened_form)
        .map(|row| row.id.clone())
        .collect();
    let undeclared: Vec<&String> = forms
        .iter()
        .filter(|id| !committed.contains(id.as_str()) && !exempt.contains(id.as_str()))
        .collect();
    let stale: Vec<&&str> = committed
        .iter()
        .chain(&exempt)
        .filter(|id| !forms.contains(**id))
        .collect();
    assert!(
        undeclared.is_empty() && stale.is_empty(),
        "rows that open a form nothing commits: {undeclared:?}; declared rows that \
         open no form: {stale:?}"
    );
}
#[test]
fn opening_a_project_asks_for_one_and_loads_what_was_chosen() {
    // The dressing writes a second project of two clips and answers with it, so
    // the load is visible in the clip count. The same dressing the census drives.
    let (state, path) = Answered::Open.state();
    assert_eq!(state.store.sequence.clip_count(), 1);
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-open");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1, "the row did not ask for a project");
        assert_eq!(state.project_path.as_deref(), Some(path.as_path()));
        assert_eq!(
            state.store.sequence.clip_count(),
            2,
            "the chosen project did not load"
        );
    });
}
#[test]
fn opening_a_project_with_unsaved_work_asks_the_prompt_before_the_dialog() {
    // The guard comes first on purpose: cancelling the prompt must cost no file
    // dialog, so the pick happens after the answer and not before it.
    let (mut state, _) = with_clip();
    assert!(state.store.is_dirty, "the fixture must have unsaved edits");
    state.supply_pick(Answer::file(scratch("guard").join("never.aria")));
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-open");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.dialog, crate::app::DialogKind::UnsavedChanges);
        assert_eq!(
            state.picks_asked(),
            0,
            "the dialog opened before the prompt"
        );
    });
    cx.update(|app| entity.update(app, |state: &mut AppState, _| state.cancel_pending()));
    cx.update(|app| {
        assert_eq!(
            entity.read(app).picks_asked(),
            0,
            "a cancelled prompt still opened a dialog"
        );
    });
}
