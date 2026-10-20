//! Driving the rename bar: every prompt it can show, typed and committed.
//!
//! The bar is one field for eight targets, so the registry here is the target
//! itself: `RenameTarget::label` is the prompt the panel draws, and the state
//! machine behind it is `begin_rename` / `rename_push` / `commit_rename`.
//! `rename_push` types one character over the selection through
//! `replace_rename_text`, the call the shell's text-input handler makes. A census drives that path one
//! character at a time rather than writing `rename_draft` and committing,
//! because the draft is bounded on push and a census that assigns the field
//! would measure a keyboard the product does not have.
//!
//! Fail by default: `typed` matches [`RenameTarget`] exhaustively, so a new
//! target cannot compile without a row, and
//! `every_prompt_the_bar_can_show_is_dressed` reads the prompt list back out of
//! `types_target.rs` and requires a dressed state for each one.

use std::collections::BTreeSet;

use aria_models::{ClipId, Generator};

use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::app::sent_census::Watch;
use crate::types::RenameTarget;

/// An editor dressed so one rename target has something to name, and the target
/// the bar would open on it.
struct Stage {
    /// What the state holds, for a failure message.
    what: &'static str,
    dress: fn() -> (AppState, RenameTarget),
}

/// One dressed state per prompt the bar can show.
///
/// A target is not a typed value: a marker rename needs a marker,
/// a slot hint needs a clip marked replaceable, and a title's text needs a title.
/// Each row exists because its target opens on nothing without it.
const RENAME_STAGES: &[Stage] = &[
    Stage {
        what: "a marker on the timeline",
        dress: a_marker,
    },
    Stage {
        what: "a marker with a note",
        dress: a_marker_note,
    },
    Stage {
        what: "a lane",
        dress: a_lane,
    },
    Stage {
        what: "a clip card",
        dress: a_clip,
    },
    Stage {
        what: "the project",
        dress: the_project,
    },
    Stage {
        what: "a clip marked as a template slot",
        dress: a_template_slot,
    },
    Stage {
        what: "a title",
        dress: a_title,
    },
    Stage {
        what: "a selection to shift",
        dress: a_selection_to_shift,
    },
    Stage {
        what: "a workspace to save",
        dress: a_workspace_to_save,
    },
    Stage {
        what: "an effect in a clip's chain",
        dress: an_effect,
    },
];

/// The text typed into `target`'s draft.
///
/// Matched exhaustively on purpose: a new [`RenameTarget`] fails to compile here
/// until someone says what a keyboard puts into it, and
/// `every_prompt_the_bar_can_show_is_dressed` then requires a stage that opens
/// it. Every value differs from what the fixture already holds, because an
/// unchanged draft cancels instead of dispatching.
fn typed(target: RenameTarget) -> &'static str {
    match target {
        RenameTarget::Marker(_) => "Chorus",
        RenameTarget::MarkerComment(_) => "second take",
        RenameTarget::Track(_) => "Dialogue",
        RenameTarget::Clip(_) => "Wide shot",
        RenameTarget::Project => "Reel",
        RenameTarget::SlotLabel => "Logo here",
        RenameTarget::ClipText => "Second title",
        RenameTarget::ShiftFrames => "5",
        RenameTarget::SaveWorkspace => "My Grading",
        RenameTarget::Effect { .. } => "Soft Blur",
    }
}

/// Every command the rename bar asked for, one target at a time.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for stage in RENAME_STAGES {
        let (mut state, target) = (stage.dress)();
        let watch = Watch::on(&state);
        commit(&mut state, target, typed(target));
        out.extend(watch.take());
    }
    out
}

/// Open the bar on `target`, type `draft`, commit: the routed path, key by key.
fn commit(state: &mut AppState, target: RenameTarget, draft: &str) {
    state.begin_rename(target);
    assert_eq!(
        state.rename,
        Some(target),
        "the bar did not open on {target:?}, so the dressed state holds nothing to name"
    );
    clear(state);
    for ch in draft.chars() {
        state.rename_push(ch);
    }
    state
        .commit_rename()
        .unwrap_or_else(|err| panic!("the {target:?} rename was refused: {err}"));
}

/// Backspace the seeded draft away, which is the only way the bar has to empty
/// itself.
fn clear(state: &mut AppState) {
    while !state.rename_draft.is_empty() {
        state.rename_pop();
    }
}

fn first_marker(state: &AppState) -> u64 {
    state
        .store
        .sequence
        .markers
        .first()
        .expect("the dressed state carries one marker")
        .id
}

fn a_marker() -> (AppState, RenameTarget) {
    let (mut state, _) = with_clip();
    state
        .add_marker_at_playhead()
        .expect("the marker lands at the playhead");
    let id = first_marker(&state);
    (state, RenameTarget::Marker(id))
}

fn a_marker_note() -> (AppState, RenameTarget) {
    let (state, target) = a_marker();
    let RenameTarget::Marker(id) = target else {
        unreachable!("the marker stage opens on a marker");
    };
    (state, RenameTarget::MarkerComment(id))
}

fn a_lane() -> (AppState, RenameTarget) {
    let (state, _) = with_clip();
    let id = state
        .store
        .sequence
        .tracks
        .first()
        .expect("the fixture projects one lane")
        .id;
    (state, RenameTarget::Track(id))
}

fn a_clip() -> (AppState, RenameTarget) {
    let (state, clip) = with_clip();
    (state, RenameTarget::Clip(clip.raw()))
}

fn the_project() -> (AppState, RenameTarget) {
    let (state, _) = with_clip();
    (state, RenameTarget::Project)
}

fn a_template_slot() -> (AppState, RenameTarget) {
    let (mut state, _) = with_clip();
    state
        .toggle_selected_replaceable()
        .expect("the clip takes a slot mark");
    assert!(
        state.selected_slot().is_some(),
        "the dressed clip carries no slot, so the hint would open on nothing"
    );
    (state, RenameTarget::SlotLabel)
}

fn a_title() -> (AppState, RenameTarget) {
    let (mut state, _) = with_clip();
    state
        .add_generated(Generator::text("Title"))
        .expect("the title lands on a text lane");
    // Found by content rather than by `landing_clip`, which expires: a census
    // that raced a settling animation would dress nothing under load.
    let title = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .find(|clip| clip.generator.is_some())
        .expect("the title is on the timeline")
        .id;
    state.select_clip(ClipId::from_raw(title));
    assert!(
        state.selected_is_title(),
        "the dressed selection is not a title, so its text would open on nothing"
    );
    (state, RenameTarget::ClipText)
}

fn a_selection_to_shift() -> (AppState, RenameTarget) {
    let (state, _) = with_clip();
    (state, RenameTarget::ShiftFrames)
}

fn a_workspace_to_save() -> (AppState, RenameTarget) {
    let (state, _) = with_clip();
    (state, RenameTarget::SaveWorkspace)
}

fn an_effect() -> (AppState, RenameTarget) {
    let (state, clip) = crate::app::fixtures::blurred_clip();
    (
        state,
        RenameTarget::Effect {
            clip: clip.raw(),
            index: 0,
        },
    )
}

/// Every prompt `RenameTarget::label` can draw, read back out of its own match.
///
/// Derived from source rather than listed here: the compiler already forces
/// `label` to cover a new target, so reading its arms makes the coverage test go
/// red on a target nobody dressed, which a hand-kept list would not.
fn prompts_in_source() -> BTreeSet<String> {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/types_target.rs"),
    )
    .expect("the rename target's source is readable");
    let body = source
        .split_once("pub fn label(self)")
        .expect("`RenameTarget::label` is where the prompts live")
        .1
        .split_once("\n    }")
        .expect("`RenameTarget::label` closes")
        .0;
    body.lines()
        .filter_map(|line| line.split_once("=> \""))
        .map(|(_, rest)| {
            rest.split_once('"')
                .expect("a prompt is a quoted string")
                .0
                .to_string()
        })
        .collect()
}

#[test]
fn every_prompt_the_bar_can_show_is_dressed() {
    // Fail by default on a new target: `label` must cover it to compile, so it
    // appears here whether or not anyone gave it a state to open on. A target
    // with no dressed state is a bar that opens onto nothing.
    let dressed: BTreeSet<String> = RENAME_STAGES
        .iter()
        .map(|stage| {
            let (_, target) = (stage.dress)();
            target.label().to_string()
        })
        .collect();
    assert_eq!(
        dressed.len(),
        RENAME_STAGES.len(),
        "two stages dress the same prompt, so one target is measured twice and another not at all"
    );
    assert_eq!(
        dressed,
        prompts_in_source(),
        "the dressed prompts and the prompts the bar can draw are not the same set"
    );
}

#[test]
fn every_rename_the_bar_opens_asks_for_something() {
    // A prompt that draws, takes typing and dispatches nothing is a field that
    // discards the typed name.
    for stage in RENAME_STAGES {
        let (mut state, target) = (stage.dress)();
        let watch = Watch::on(&state);
        commit(&mut state, target, typed(target));
        let asked = if target == RenameTarget::SaveWorkspace {
            state.data_dir.join("workspaces/My Grading.json").exists()
        } else {
            !watch.take().is_empty()
        };
        assert!(asked, "renaming {} asked for nothing", stage.what);
        assert_eq!(
            state.rename, None,
            "the bar stayed open after committing {}",
            stage.what
        );
        assert!(
            state.rename_draft.is_empty(),
            "the draft survived the commit on {}",
            stage.what
        );
    }
}

#[test]
fn an_unchanged_draft_costs_no_edit() {
    // Opening the bar and pressing enter is how a rename is abandoned.
    // Dispatching there would put an empty undo entry above the last real
    // edit.
    for stage in RENAME_STAGES {
        let (mut state, target) = (stage.dress)();
        let watch = Watch::on(&state);
        state.begin_rename(target);
        state.commit_rename().expect("the unchanged commit lands");
        assert!(
            watch.take().is_empty(),
            "committing {} unchanged asked for an edit",
            stage.what
        );
        assert_eq!(
            state.rename, None,
            "the bar stayed open after cancelling {}",
            stage.what
        );
    }
}

#[test]
fn a_blank_draft_is_a_value_only_where_blank_means_something() {
    // A name cannot be blank, so an empty draft on a name is a cancel. A note
    // and a card name can be blank, and clearing one is the only way to take it
    // off, so there the empty draft has to dispatch.
    for stage in RENAME_STAGES {
        let (mut state, target) = (stage.dress)();
        // Give the target a value first: an empty draft over an already empty
        // value is unchanged, which cancels for a different reason.
        commit(&mut state, target, typed(target));
        let watch = Watch::on(&state);
        state.begin_rename(target);
        clear(&mut state);
        state.commit_rename().expect("the blank commit lands");
        assert_eq!(
            !watch.take().is_empty(),
            target.allows_empty(),
            "a blank draft on {} did not follow its own allows_empty",
            stage.what
        );
        assert_eq!(
            state.rename, None,
            "the bar stayed open after a blank commit on {}",
            stage.what
        );
    }
}

#[test]
fn typing_cannot_grow_a_draft_past_the_field() {
    // The bar draws one line. A held key that grew the draft without bound would
    // push the prompt off it, and the shift offset would parse a number nobody
    // typed.
    for stage in RENAME_STAGES {
        let (mut state, target) = (stage.dress)();
        state.begin_rename(target);
        for _ in 0..(crate::app::rename::RENAME_MAX_LEN * 2) {
            state.rename_push('x');
        }
        assert_eq!(
            state.rename_draft.chars().count(),
            crate::app::rename::RENAME_MAX_LEN,
            "the draft on {} is not bounded by the field",
            stage.what
        );
    }
}

#[test]
fn the_bar_is_drawn_from_the_rename_state() {
    // The census drives `begin_rename` / `rename_push` / `commit_rename`. That
    // measures the surface only while the drawn bar shows that draft and the key
    // routing calls those functions.
    //
    // Not covered: where the bar sits or what it looks like. The picture is the
    // panel's.
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for (file, needles) in [
        (
            "panels/rename_bar.rs",
            ["app.rename_draft.clone()", "target.label()"],
        ),
        (
            "keys/fields.rs",
            [
                "state.commit_rename()",
                "replace: AppState::replace_rename_text",
            ],
        ),
    ] {
        let drawn = std::fs::read_to_string(src.join(file))
            .unwrap_or_else(|err| panic!("{file} is readable: {err}"));
        for needle in needles {
            assert!(
                drawn.contains(needle),
                "{file} has no `{needle}`, so the census measures a path it does not run"
            );
        }
    }
}
