//! A save pick answered with a name that has no extension.
//!
//! Defect class: the Linux portal and zenity return a typed name as it was
//! typed, so `cut` in Save As wrote a project file called `cut`, which the
//! Open dialog's `*.aria` filter does not list. Export Frame and Export
//! Timeline each completed the name on their own; Save As, Save a Copy, Save as
//! Template, Export the Keymap and Export as… did not. The completion is now at
//! the seam every pick goes through, so it holds for each save pick, and a new
//! save pick that offers no extension fails whichever test first reaches it.
//!
//! The dialog checked the typed name for an existing file, not the completed
//! one, so a file at the completed path is replaced only on a yes.
//!
//! What this does not catch: a native dialog that completes the name itself
//! (macOS, Windows) and then reports a different one; and a typed extension
//! the format does not write (`cut.v2` is saved as typed).

use std::path::{Path, PathBuf};

use super::{Answer, Ask, FileSpec};
use crate::app::AppState;
use crate::app::premiere_tests::with_clip;

/// Record the path a continuation received on the status line, or
/// `cancelled`.
fn record(state: &mut AppState, answer: Answer) {
    state.status_message = Some(match answer.one() {
        Some(path) => path.display().to_string(),
        None => "cancelled".to_string(),
    });
}

/// Ask for a save with `filters`, answered with `typed` and then `replies` in
/// order, and return what the continuation received.
fn saved(
    state: &mut AppState,
    filters: &[(&str, &[&str])],
    typed: &Path,
    replies: &[bool],
) -> Option<String> {
    let mut spec = FileSpec::new("Save As");
    for (name, extensions) in filters {
        spec = spec.filter(*name, *extensions);
    }
    let spec = spec.file_name(format!("proposed.{}", filters[0].1[0]));
    state.status_message = None;
    state.supply_pick(Answer::file(typed));
    for reply in replies {
        state.supply_pick(Answer::Confirmed(*reply));
    }
    state.request_pick(Ask::SaveFile(spec), record);
    state.status_message.take()
}

/// A Save As dialog for a project file.
fn aria_save_spec() -> FileSpec {
    FileSpec::new("Save As")
        .filter("Aria project", &["aria"])
        .file_name("cut.aria")
}

#[test]
fn a_save_name_with_no_extension_takes_the_first_one_offered() {
    let (mut state, _) = with_clip();
    let dir = crate::scratch_pool::scratch("picker-save-complete");
    let aria: &[(&str, &[&str])] = &[("Aria project", &["aria"])];
    let images: &[(&str, &[&str])] = &[("PNG image", &["png"]), ("JPEG image", &["jpg"])];
    let cases: [(&[(&str, &[&str])], &str, &str); 8] = [
        (aria, "cut", "cut.aria"),
        (aria, "cut.", "cut.aria"),
        (aria, ".cut", ".cut.aria"),
        (aria, "cut.aria", "cut.aria"),
        (aria, "cut.ARIA", "cut.ARIA"),
        (aria, "cut.v2", "cut.v2"),
        (aria, "Wedding cut", "Wedding cut.aria"),
        (images, "still", "still.png"),
    ];
    for (filters, typed, want) in cases {
        let got = saved(&mut state, filters, &dir.join(typed), &[]);
        assert_eq!(
            got,
            Some(dir.join(want).display().to_string()),
            "a save typed as {typed:?}"
        );
    }
    let cancelled = {
        state.supply_pick(Answer::cancelled());
        let spec = aria_save_spec();
        state.request_pick(Ask::SaveFile(spec), record);
        state.status_message.take()
    };
    assert_eq!(cancelled.as_deref(), Some("cancelled"));
    assert_eq!(state.picks_unused(), 0);
}

#[test]
fn a_completed_name_over_an_existing_file_is_replaced_only_on_yes() {
    let (mut state, _) = with_clip();
    let dir = crate::scratch_pool::scratch("picker-save-replace");
    let held = dir.join("cut.aria");
    std::fs::write(&held, b"earlier").expect("write the earlier file");
    let aria: &[(&str, &[&str])] = &[("Aria project", &["aria"])];

    for (reply, want) in [
        (true, held.display().to_string()),
        (false, "cancelled".into()),
    ] {
        let asked = state.picks_asked();
        let got = saved(&mut state, aria, &dir.join("cut"), &[reply]);
        assert_eq!(got.as_deref(), Some(want.as_str()), "reply {reply}");
        assert_eq!(state.picks_asked() - asked, 2, "no Replace File question");
        assert_eq!(state.picks_unused(), 0);
    }

    // The dialog asked about the name it was given, so a typed name that
    // already has the extension is not asked about twice.
    let asked = state.picks_asked();
    let got = saved(&mut state, aria, &held, &[]);
    assert_eq!(got, Some(held.display().to_string()));
    assert_eq!(state.picks_asked() - asked, 1, "asked twice about one name");

    // Unanswered, the question waits and nothing is written.
    state.supply_pick(Answer::file(dir.join("cut")));
    let spec = aria_save_spec();
    state.request_pick(Ask::SaveFile(spec), record);
    assert_eq!(
        state.status_message.as_deref(),
        Some("Waiting for the Replace File picker")
    );
    assert_eq!(std::fs::read(&held).expect("read"), b"earlier");
}

/// The save commands write where the completed answer points.
#[test]
fn each_save_command_writes_the_completed_name() {
    type Run = fn(&mut AppState);
    let runs: [(&str, Run, &str); 3] = [
        ("Save As", AppState::save_project_as, "aria"),
        ("Save a Copy", AppState::save_project_copy, "aria"),
        (
            "Save as Template",
            AppState::pick_and_save_template,
            crate::app::templates::TEMPLATE_EXTENSION,
        ),
    ];
    for (index, (command, run, extension)) in runs.into_iter().enumerate() {
        let (mut state, _) = with_clip();
        let dir = crate::scratch_pool::scratch(&format!("picker-save-command-{index}"));
        let typed: PathBuf = dir.join("cut");
        state.supply_pick(Answer::file(&typed));
        run(&mut state);
        assert_eq!(state.last_error, None, "{command}");
        assert!(
            typed.with_extension(extension).is_file(),
            "{command} did not write cut.{extension}"
        );
        assert!(!typed.exists(), "{command} wrote a file with no extension");
    }
}

/// What each save dialog proposes, for an unsaved project whose name holds a
/// path separator and for a saved one.
///
/// Defect class: Save As and Save a Copy proposed no name, so the project's
/// name was typed again; Export Timeline proposed the project's internal
/// name (`untitled` until renamed) rather than the file it was saved as; and
/// a name with a `/` was proposed as a path.
#[test]
fn each_save_dialog_proposes_the_project_name_as_one_file() {
    type Run = fn(&mut AppState);
    let (mut state, _) = with_clip();
    state
        .ctx
        .mount(&aria_interchange::InterchangePlugin)
        .expect("mount the timeline interchange plugin");
    let timeline = state
        .ctx
        .exporters()
        .first()
        .map(|exporter| exporter.extension.clone())
        .expect("the plugin registers an exporter");
    let frame = state.playhead.value;
    let video = state.export_extension();
    let dir = crate::scratch_pool::scratch("picker-save-names");
    for (saved, stem) in [(false, "Wedding-Day"), (true, "cut")] {
        state.store_mut().project_name = "Wedding/Day".into();
        state.project_path = saved.then(|| dir.join("cut.aria"));
        let runs: [(Run, String); 7] = [
            (AppState::save_project_as, format!("{stem}.aria")),
            (AppState::save_project_copy, format!("{stem} copy.aria")),
            (AppState::pick_and_save_template, format!("{stem}.ariat")),
            (
                AppState::pick_and_export_frame,
                format!("{stem} {frame}.png"),
            ),
            (
                AppState::pick_and_export_timeline,
                format!("{stem}.{timeline}"),
            ),
            (AppState::pick_export_destination, format!("{stem}.{video}")),
            (AppState::export_keymap, "Aria keys.toml".to_string()),
        ];
        for (run, want) in runs {
            run(&mut state);
            let spec = state.last_save_spec().expect("a save dialog was asked for");
            assert_eq!(
                spec.proposed_name(),
                Some(want.as_str()),
                "{} (saved: {saved})",
                spec.title()
            );
        }
    }

    let want = FileSpec::new("Save As")
        .filter("Aria project", &["aria"])
        .file_name("cut.aria")
        .directory(&dir);
    state.save_project_as();
    assert_eq!(
        state.last_save_spec(),
        Some(&want),
        "Save As opens in the project's folder"
    );
}
