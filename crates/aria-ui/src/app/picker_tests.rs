//! Contracts for the one seam every file dialog goes through.
//!
//! The class closed here is "a command that prompts for a path cannot
//! be driven, and the window stops redrawing while it waits". Both came from one
//! cause: fifteen call sites opened a blocking native dialog on the thread that
//! draws, so a headless run waited for an answer that never came. Waiting for
//! ever is what a suite cannot recover from — it is a stalled run rather than a
//! failure — so the first assertion here is that an unanswered pick returns.
//!
//! What this does not catch: the dialogs themselves. `aria-dialog` opens them and
//! a native dialog needs a person. What is proved instead is that an answer
//! reaches the state through one seam, in the order the questions were asked,
//! and that cancelling is an answer rather than a second path. A new variant of
//! `Ask` is caught by the compiler, because `Ask::answer` matches exhaustively.
//!
//! The seam also holds every title to Title Case, as the menu row or palette
//! entry that opens the dialog is: `Open project` and `Relink media` sat beside
//! `Save As` and `Load LUT`. The check is at the seam, so a pick is checked by
//! whichever test first reaches it, and the panel buttons no census presses are
//! pressed here. Not caught: a pick that no test reaches.

use std::path::PathBuf;

use aria_models::ClipId;

use super::picker::{Answer, Ask, DialogError, FileSpec};
use super::premiere_tests::with_clip;

/// A path named `name` inside a scratch dir of its own.
///
/// The dir is per name, not per test file: claiming a scratch dir empties it, so
/// two calls sharing one dir would delete each other's staged files.
fn scratch(name: &str) -> PathBuf {
    crate::scratch_pool::scratch(&format!("picker-{name}")).join(name)
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// Report the chosen path through the status line, which is the cheapest thing a
/// continuation can observably change.
fn report(state: &mut super::AppState, answer: Answer) {
    state.status_message = Some(match answer.one() {
        Some(path) => path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        None => "cancelled".to_string(),
    });
}

#[test]
fn an_unanswered_pick_returns_instead_of_waiting_for_one() {
    // The defect itself: this used to block until a person answered a native
    // dialog, so a test hung rather than failed. The pick is recorded as asked
    // and the continuation does not run.
    let (mut state, _) = with_clip();
    // The continuation writes the chosen file's name (or "cancelled") over the
    // strip, while an unanswered pick posts its waiting line until the dialog
    // answers.
    state.request_pick(Ask::OpenFile(FileSpec::new("Open Project")), report);
    assert_eq!(state.picks_asked(), 1, "the pick was not recorded");
    assert_eq!(
        state.status_message.as_deref(),
        Some("Waiting for the Open Project picker"),
        "the continuation ran without an answer or the waiting status was not posted"
    );
}

#[test]
fn a_supplied_answer_runs_the_continuation_at_once() {
    let (mut state, _) = with_clip();
    state.supply_pick(Answer::file(scratch("chosen.aria")));
    state.request_pick(Ask::OpenFile(FileSpec::new("Open Project")), report);
    assert_eq!(state.status_message.as_deref(), Some("chosen.aria"));
    assert_eq!(state.picks_unused(), 0, "the answer was not consumed");
}

#[test]
fn answers_are_used_in_the_order_they_were_supplied() {
    // A template with two slots asks twice. Answers arriving out of order would
    // fill the wrong slot, which is a wrong project rather than an error.
    let (mut state, _) = with_clip();
    state.supply_pick(Answer::file(scratch("first")));
    state.supply_pick(Answer::file(scratch("second")));
    let mut seen = Vec::new();
    for _ in 0..2 {
        state.request_pick(Ask::OpenFile(FileSpec::new("Media")), report);
        seen.push(state.status_message.take().expect("an answer per question"));
    }
    assert_eq!(seen, vec!["first".to_string(), "second".to_string()]);
}

#[test]
fn cancelling_is_an_answer_and_not_a_special_path() {
    let (mut state, _) = with_clip();
    state.supply_pick(Answer::cancelled());
    state.request_pick(
        Ask::OpenFiles(FileSpec::new("Import Media")),
        |state, answer| {
            state.status_message = Some(format!("{} chosen", answer.all().len()));
        },
    );
    assert_eq!(state.status_message.as_deref(), Some("0 chosen"));
}

#[test]
fn a_question_answered_no_is_not_permission() {
    // The one confirm in the product deletes a file off disk. "No" and "the
    // dialog was dismissed" are both false, and that is the safety property.
    assert!(!Answer::Confirmed(false).is_yes());
    assert!(Answer::Confirmed(true).is_yes());
    assert!(
        Answer::Confirmed(true).one().is_none(),
        "a yes must not read as a path"
    );
    assert!(Answer::cancelled().one().is_none());
    assert!(Answer::cancelled().all().is_empty());
    assert!(!Answer::file(scratch("x")).is_yes());
}

#[test]
fn a_lut_pick_asks_nothing_when_no_clip_is_selected() {
    // The guard belongs before the pick: asking for a file and then dropping the
    // answer is a dialog filled in for nothing.
    let (mut state, _) = with_clip();
    state.clear_selection();
    state.pick_and_load_lut();
    assert_eq!(state.picks_asked(), 0);
    assert_eq!(
        state.status_message.as_deref(),
        Some(crate::app::decline::Decline::NoSelection.reason())
    );
}

#[test]
fn a_lut_pick_loads_the_file_the_operator_chose() {
    let (mut state, _) = with_clip();
    let lut = scratch("warm.cube");
    state.supply_pick(Answer::file(lut.clone()));
    state.pick_and_load_lut();
    assert_eq!(state.picks_asked(), 1);
    assert_eq!(
        state.selected_lut().map(|applied| applied.path),
        Some(lut.display().to_string().into()),
        "the chosen LUT is not on the clip"
    );
}

/// A bundle with `slots` replaceable clips, saved to disk.
fn bundle_with_slots(slots: usize, name: &str) -> PathBuf {
    let (mut state, _) = with_clip();
    if slots > 1 {
        let ui = state.selected_ui_clip().expect("the fixture clip");
        let mid = ui.timeline_start_ticks + (ui.timeline_end_ticks - ui.timeline_start_ticks) / 2;
        state.set_playhead(mid);
        state.split_at_playhead().expect("split the fixture clip");
    }
    let ids: Vec<ClipId> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .collect();
    assert_eq!(ids.len(), slots, "the fixture does not hold {slots} clips");
    for id in ids {
        state.select_clip(id);
        state.toggle_selected_replaceable().expect("mark the slot");
    }
    assert_eq!(state.template_slot_count(), slots);
    let path = scratch(name);
    state
        .save_template(path.clone(), "Fixture".into())
        .expect("save the bundle");
    path
}

#[test]
fn a_two_slot_template_asks_once_per_slot_and_fills_both() {
    // The sequential chain: each slot is asked for only once the last has been
    // answered, so this walks the whole continuation chain rather than one link.
    let bundle = bundle_with_slots(2, "two-slots.ariat");

    let (mut state, _) = with_clip();
    state.supply_pick(Answer::file(fixture()));
    state.supply_pick(Answer::file(fixture()));
    state.fill_and_apply_template(bundle);

    assert_eq!(state.picks_asked(), 2, "one question per slot");
    assert_eq!(state.picks_unused(), 0, "an answer went unused");
    assert_eq!(state.route, crate::types::UiRoute::Editor);
    assert!(
        state.project_path.is_none(),
        "an applied template must not inherit a project path"
    );
    assert_eq!(
        state.store.sequence.clip_count(),
        2,
        "the filled template did not land"
    );
}

#[test]
fn cancelling_a_slot_applies_the_template_unfilled() {
    let bundle = bundle_with_slots(1, "one-slot.ariat");

    let (mut state, _) = with_clip();
    state.supply_pick(Answer::cancelled());
    state.fill_and_apply_template(bundle);

    assert_eq!(state.picks_asked(), 1);
    assert_eq!(
        state.route,
        crate::types::UiRoute::Editor,
        "a cancelled slot must still open the template"
    );
    assert_eq!(
        state.store.sequence.clip_count(),
        1,
        "the template's own media did not land"
    );
}

#[test]
#[should_panic(expected = "the pick titled `Open project` is not in Title Case")]
fn a_file_pick_titled_in_sentence_case_is_refused() {
    let (mut state, _) = with_clip();
    state.request_pick(Ask::OpenFile(FileSpec::new("Open project")), report);
}

#[test]
#[should_panic(expected = "the pick titled `Delete project file` is not in Title Case")]
fn a_question_titled_in_sentence_case_is_refused() {
    let (mut state, _) = with_clip();
    state.request_pick(
        Ask::Confirm {
            title: "Delete project file".into(),
            question: "Delete it?".into(),
        },
        report,
    );
}

/// What follows `: ` is the project's own data, a slot label or a plugin name,
/// and stands as it was written; the words before it are held.
#[test]
fn only_the_words_before_a_subject_are_held_to_title_case() {
    let (mut state, _) = with_clip();
    state.request_pick(
        Ask::OpenFile(FileSpec::new("Choose Media: texture.png")),
        report,
    );
    assert_eq!(state.picks_asked(), 1, "a lowercase subject was refused");
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.request_pick(Ask::OpenFile(FileSpec::new("Choose media: Logo")), report);
    }));
    assert!(
        refused.is_err(),
        "the words before the subject were not checked"
    );
}

/// The picks a panel button opens with no menu row or palette entry behind it,
/// so no census presses them. Each asks one question, and a cancelled dialog
/// leaves the history, the dialog and the status line as they were: a cancel
/// that moved a cache or queued an export would be work nobody chose.
#[test]
fn every_panel_pick_asks_once_and_a_cancel_changes_nothing() {
    let presses: [(&str, fn(&mut super::AppState)); 5] = [
        ("the Export sheet's Choose", |state| {
            state.pick_export_destination()
        }),
        ("a missing item's Relink", |state| {
            let item = state.store.media.first().expect("the fixture's media");
            let media = aria_models::MediaId::from_raw(item.id);
            state.pick_and_relink(media);
        }),
        ("a cache's Move", |state| {
            state.pick_cache_move(aria_storage::CacheId::RenderFiles.as_str());
        }),
        ("a cache's Redirect", |state| {
            state.pick_cache_redirect(aria_storage::CacheId::RenderFiles.as_str());
        }),
        ("the Storage section's Move", |state| {
            state.pick_storage_root()
        }),
    ];
    for (press, run) in presses {
        let (mut state, _) = with_clip();
        let before = (
            state.history_depth(),
            state.dialog,
            state.status_message.clone(),
        );
        state.supply_pick(Answer::cancelled());
        run(&mut state);
        assert_eq!(state.picks_asked(), 1, "{press} did not ask once");
        assert_eq!(state.picks_unused(), 0, "{press} left its answer unread");
        assert_eq!(
            (
                state.history_depth(),
                state.dialog,
                state.status_message.clone(),
            ),
            before,
            "{press} acted on a cancelled dialog"
        );
    }
}

/// WHY: saving an unsaved project used to fall back to `<data_dir>/untitled.aria`
/// without prompting, so `Ctrl+S`, `File > Save`, the command palette's Save
/// row, and the Unsaved Changes dialog's Save button wrote to a hidden internal
/// path and overwrote earlier untitled work. Closes that class by deriving every
/// save entry point from `SHIPPED_BINDINGS`, `AppMenu::File`, the live palette
/// catalog, and the unsaved prompt at run time, driving each on an unsaved
/// project first with a cancelled pick (asserting one pick was requested and no
/// `.aria` file was written) and then with a chosen path (asserting the project
/// is written at the chosen path). Does not cover the OS file dialog window
/// itself.
#[test]
fn every_save_entry_point_asks_for_a_path_on_an_unsaved_project() {
    use crate::app::palette::Pending;
    use crate::chrome::menu_items::{items_of, run_item};
    use crate::keys::KeyBody;
    use crate::keys::bindings::SHIPPED_BINDINGS;
    use crate::types::AppMenu;
    use gpui::AppContext;

    let probe_dir = crate::scratch_pool::scratch("save-entry-probe");
    let probe_ctx = aria_plugin::PluginContext::new();
    let _ = crate::plugins::mount_all(&probe_ctx, &probe_dir);
    let mut probe = super::AppState::new(probe_ctx, probe_dir).expect("probe state");
    probe.new_project().expect("probe project");

    let key_ids: Vec<&'static str> = SHIPPED_BINDINGS
        .iter()
        .filter(|b| {
            b.name.starts_with("Save") && b.name != "Save a Copy" && b.name != "Save as Template"
        })
        .map(|b| b.id)
        .collect();
    assert!(
        key_ids.contains(&"key-save") && key_ids.contains(&"key-save-as"),
        "expected key-save and key-save-as in shipped bindings, got {key_ids:?}"
    );

    let menu_ids: Vec<&'static str> = items_of(AppMenu::File, &probe)
        .into_iter()
        .filter(|item| {
            item.label.starts_with("Save")
                && item.label != "Save a Copy…"
                && item.label != "Save as Template…"
        })
        .map(|item| item.id)
        .collect();
    assert!(
        menu_ids.contains(&"file-save") && menu_ids.contains(&"file-save-as"),
        "expected file-save and file-save-as in File menu, got {menu_ids:?}"
    );

    let palette_ids: Vec<String> = probe
        .palette_catalog()
        .iter()
        .filter(|offer| {
            offer.source == Some(AppMenu::File)
                && offer.label.starts_with("Save")
                && offer.label.as_ref() != "Save a Copy…"
                && offer.label.as_ref() != "Save as Template…"
        })
        .map(|offer| offer.id.to_string())
        .collect();
    assert!(
        palette_ids.iter().any(|id| id == "file-save")
            && palette_ids.iter().any(|id| id == "file-save-as"),
        "expected file-save and file-save-as in palette catalog, got {palette_ids:?}"
    );

    type EntryRunner = Box<dyn Fn(&mut super::AppState, &gpui::TestAppContext)>;
    let mut entries: Vec<(String, EntryRunner)> = Vec::new();

    for id in key_ids {
        entries.push((
            format!("chord:{id}"),
            Box::new(move |state, _| {
                let binding = SHIPPED_BINDINGS
                    .iter()
                    .find(|b| b.id == id)
                    .expect("shipped binding");
                let KeyBody::Plain(run) = binding.run else {
                    panic!("{id} is not a plain key binding");
                };
                run(state);
            }),
        ));
    }

    for id in menu_ids {
        entries.push((
            format!("menu:{id}"),
            Box::new(move |state, cx| {
                let _ = crate::plugins::mount_all(&state.ctx, &state.data_dir);
                let placeholder = super::AppState::new(
                    aria_plugin::PluginContext::new(),
                    crate::scratch_pool::scratch("menu-placeholder"),
                )
                .expect("placeholder");
                let moved = std::mem::replace(state, placeholder);
                let entity = cx.update(|app| app.new(|_| moved));
                cx.update(|app| {
                    entity.update(app, |s, cx| {
                        assert!(run_item(AppMenu::File, id, s, cx), "{id} did not run");
                    });
                });
                let _ = cx.update(|app| {
                    entity.update(app, |s, _| {
                        let empty = super::AppState::new(
                            aria_plugin::PluginContext::new(),
                            crate::scratch_pool::scratch("menu-empty"),
                        )
                        .expect("empty");
                        *state = std::mem::replace(s, empty);
                    })
                });
            }),
        ));
    }

    for id in palette_ids {
        entries.push((
            format!("palette:{id}"),
            Box::new(move |state, cx| {
                let _ = crate::plugins::mount_all(&state.ctx, &state.data_dir);
                let index = state
                    .palette_hits()
                    .iter()
                    .position(|offer| offer.id.as_ref() == id.as_str())
                    .expect("palette offer present");
                let pending = state.run_palette_offer(index);
                if let Some(Pending::Menu(menu, row_id)) = pending {
                    let placeholder = super::AppState::new(
                        aria_plugin::PluginContext::new(),
                        crate::scratch_pool::scratch("pal-placeholder"),
                    )
                    .expect("placeholder");
                    let moved = std::mem::replace(state, placeholder);
                    let entity = cx.update(|app| app.new(|_| moved));
                    cx.update(|app| {
                        entity.update(app, |s, cx| {
                            assert!(run_item(menu, row_id, s, cx), "{row_id} did not run");
                        });
                    });
                    let _ = cx.update(|app| {
                        entity.update(app, |s, _| {
                            let empty = super::AppState::new(
                                aria_plugin::PluginContext::new(),
                                crate::scratch_pool::scratch("pal-empty"),
                            )
                            .expect("empty");
                            *state = std::mem::replace(s, empty);
                        })
                    });
                }
            }),
        ));
    }

    entries.push((
        "unsaved:save_and_continue".to_string(),
        Box::new(|state, _| {
            state.request_new_project();
            assert_eq!(state.dialog, super::DialogKind::UnsavedChanges);
            state.save_and_continue();
        }),
    ));

    let cx = gpui::TestAppContext::single();
    let out_dir = crate::scratch_pool::scratch("save-entry-targets");

    for (idx, (label, run)) in entries.iter().enumerate() {
        // Phase 1: Cancelled pick asks once and writes nothing.
        let (mut state, _) = with_clip();
        assert!(state.project_path.is_none());
        state.supply_pick(Answer::cancelled());
        run(&mut state, &cx);
        assert_eq!(
            state.picks_asked(),
            1,
            "{label} did not ask for a save path"
        );
        assert_eq!(
            state.picks_unused(),
            0,
            "{label} left its supplied pick unread"
        );
        assert!(
            state.project_path.is_none(),
            "{label} set a project path after a cancelled pick"
        );
        assert!(
            !state.data_dir.join("untitled.aria").exists(),
            "{label} wrote untitled.aria despite a cancelled pick"
        );

        // Phase 2: Supplied path writes the file at the chosen destination.
        let (mut state, _) = with_clip();
        let chosen = out_dir.join(format!("saved-{idx}.aria"));
        state.supply_pick(Answer::file(&chosen));
        run(&mut state, &cx);
        assert_eq!(
            state.picks_asked(),
            1,
            "{label} did not ask for a save path"
        );
        assert_eq!(
            state.picks_unused(),
            0,
            "{label} left its supplied pick unread"
        );
        assert!(
            chosen.is_file(),
            "{label} did not write the project to {}",
            chosen.display()
        );
        assert!(
            !state.data_dir.join("untitled.aria").exists(),
            "{label} wrote untitled.aria instead of only the chosen path"
        );
    }
}

/// WHY: two untitled sessions in the same profile used to write to the same
/// `<data_dir>/untitled.aria` fallback, so saving the second silently clobbered
/// the first. Closes that class by asserting direct `save_project()` on an
/// untitled project is refused, and two untitled sessions sharing a `data_dir`
/// each prompt for their own destination and preserve both projects on disk.
#[test]
fn saving_two_untitled_projects_asks_twice_and_keeps_both_distinct() {
    let shared_dir = crate::scratch_pool::scratch("two-untitled");
    let mut first =
        super::AppState::new(aria_plugin::PluginContext::new(), shared_dir.clone()).expect("first");
    first.new_project().expect("new project A");
    first
        .add_track(aria_models::TrackKind::Video, "Project A Lane")
        .expect("lane A");

    let err = first
        .save_project()
        .expect_err("direct save_project on untitled state must refuse");
    assert!(
        err.to_string().contains("Save As"),
        "refusal must point at Save As: {err}"
    );
    assert!(!shared_dir.join("untitled.aria").exists());

    let path_a = shared_dir.join("project-a.aria");
    first.supply_pick(Answer::file(&path_a));
    first.save_or_pick();
    assert_eq!(first.picks_asked(), 1);
    assert_eq!(first.project_path.as_deref(), Some(path_a.as_path()));
    assert!(path_a.is_file());

    let mut second = super::AppState::new(aria_plugin::PluginContext::new(), shared_dir.clone())
        .expect("second");
    second.new_project().expect("new project B");
    second
        .add_track(aria_models::TrackKind::Audio, "Project B Lane")
        .expect("lane B");

    second.supply_pick(Answer::cancelled());
    second.save_or_pick();
    assert_eq!(second.picks_asked(), 1);
    assert!(second.project_path.is_none());
    assert!(!shared_dir.join("untitled.aria").exists());

    let path_b = shared_dir.join("project-b.aria");
    second.supply_pick(Answer::file(&path_b));
    second.save_or_pick();
    assert_eq!(second.picks_asked(), 2);
    assert_eq!(second.project_path.as_deref(), Some(path_b.as_path()));
    assert!(path_b.is_file());

    let loaded_a = aria_models::Project::load_from_file(&path_a).expect("load project A");
    let loaded_b = aria_models::Project::load_from_file(&path_b).expect("load project B");
    assert!(
        loaded_a
            .timeline()
            .tracks_ordered()
            .any(|t| t.name == "Project A Lane"),
        "project A was overwritten"
    );
    assert!(
        !loaded_a
            .timeline()
            .tracks_ordered()
            .any(|t| t.name == "Project B Lane"),
        "project B leaked into project A"
    );
    assert!(
        loaded_b
            .timeline()
            .tracks_ordered()
            .any(|t| t.name == "Project B Lane"),
        "project B was not saved to its chosen path"
    );
}

/// WHY: While a native picker is open (or waiting on a broken portal), the status
/// strip used to keep whatever old receipt was there before, and a missing file
/// picker backend returned `None` which the editor treated as a silent cancel.
/// Closes that class by asserting:
/// 1. An unanswered pick posts `"Waiting for the <title> picker"` immediately and
///    `age_status` keeps it on the strip past `STATUS_TTL` until the dialog
///    answers.
/// 2. Finishing with a cancelled answer clears the waiting status without
///    posting an error.
/// 3. Finishing with `DialogError::NoBackend` clears the waiting status, skips
///    the continuation, and posts `"No file picker is available. Install
///    xdg-desktop-portal or zenity."` to `last_error`.
#[test]
fn waiting_status_persists_until_answered_and_missing_backend_posts_named_error() {
    use std::time::{Duration, Instant};

    let (mut state, _) = with_clip();
    state.save_or_pick();
    assert_eq!(state.picks_asked(), 1);
    assert_eq!(
        state.status_message.as_deref(),
        Some("Waiting for the Save As picker")
    );

    let start = Instant::now();
    assert!(!state.age_status(start));
    assert!(
        !state.age_status(start + crate::app::status::STATUS_TTL + Duration::from_secs(30)),
        "waiting status must not expire while the picker is still open"
    );
    assert_eq!(
        state.status_message.as_deref(),
        Some("Waiting for the Save As picker")
    );

    // Cancelled dialog clears the waiting status and posts no error.
    state.finish_pick(
        "Waiting for the Save As picker",
        Ok(Answer::cancelled()),
        |s, answer| {
            if let Some(path) = answer.one() {
                s.status_message = Some(format!("Saved {}", path.display()));
            }
        },
    );
    assert_eq!(state.status_message, None);
    assert_eq!(state.last_error, None);

    // Missing backend skips the continuation, clears the waiting status, and
    // posts the named error.
    let mut continuation_ran = false;
    state.request_pick(Ask::OpenFiles(FileSpec::new("Import Media")), |s, _| {
        s.status_message = Some("Imported".into());
    });
    assert_eq!(
        state.status_message.as_deref(),
        Some("Waiting for the Import Media picker")
    );
    state.finish_pick(
        "Waiting for the Import Media picker",
        Err(DialogError::NoBackend),
        |_, _| {
            continuation_ran = true;
        },
    );
    assert!(
        !continuation_ran,
        "continuation must not run when no picker backend is available"
    );
    assert_eq!(state.status_message, None);
    assert_eq!(
        state.last_error.as_deref(),
        Some("No file picker is available. Install xdg-desktop-portal or zenity.")
    );
}
