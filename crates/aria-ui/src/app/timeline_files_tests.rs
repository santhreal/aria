//! Timeline files through the editor: every registered exporter writes the
//! open timeline and its importer reads it back as one undo step, by menu
//! pick and by drop; the destination's extension selects the exporter; and
//! what a run could not carry opens the Timeline File dialog, after an
//! install prompt that is up, until it is closed or a clean run replaces it.
//!
//! The formats are enumerated from the registry the shipped plugin fills, so
//! a format added to it is covered here with no edit. A subtitle format writes
//! the captions on the shown text tracks, not the cut, so it round-trips a
//! captioned project. Not caught here: what each format reads and writes,
//! which `aria-interchange` covers.

use std::path::{Path, PathBuf};

use aria_commands::{Command, EditCommand};
use aria_models::{Generator, TimeRange, TrackId, TrackKind};
use aria_plugin::{DropAction, PluginContext};
use gpui::SharedString;

use super::{AppState, DialogKind};
use crate::app::picker::Answer;
use crate::plugins::mount_all;

/// Every built-in plugin mounted, a new project, and the 3 s 24 fps picture
/// placed at the start.
fn editing() -> AppState {
    let ctx = PluginContext::new();
    let dir = crate::scratch_pool::scratch("timeline-files");
    let _ = mount_all(&ctx, &dir);
    let mut state = AppState::new(ctx, dir).expect("state");
    state.new_project().expect("new project");
    let picture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s.mp4")
        .canonicalize()
        .expect("the fixture exists");
    let media = state.import_media(picture).expect("import the fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place the clip");
    state
}

/// The fixture with two captions on a text track.
fn captioned() -> AppState {
    let mut state = editing();
    let track = state
        .ensure_kind_track(TrackKind::Text)
        .expect("a text track");
    let rate = state.playhead.rate;
    for (start, frames, text) in [(0, 24, "First line"), (36, 12, "Second line")] {
        state
            .apply_command(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::text(text.to_owned()),
                timeline: TimeRange::at_rate(start, frames, rate),
            }))
            .expect("a caption");
    }
    state
}

/// Whether the exporter of `extension` writes subtitles, which hold the
/// captions on the shown text tracks rather than the cut.
fn writes_subtitles(extension: &str) -> bool {
    aria_interchange::SUBTITLE_FORMATS
        .iter()
        .any(|format| format.extension == extension)
}

/// What a file of `extension` holds of the project: the captions for a
/// subtitle format, every placed clip for any other.
fn carried(extension: &str) -> (AppState, fn(&AppState) -> Vec<(i64, i64, i64, i64)>) {
    if writes_subtitles(extension) {
        (captioned(), captions)
    } else {
        (editing(), placed)
    }
}

/// The clips on text lanes, as [`placed`] lists them.
fn captions(state: &AppState) -> Vec<(i64, i64, i64, i64)> {
    let mut clips: Vec<_> = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|track| track.kind == crate::types::UiTrackKind::Text)
        .flat_map(|track| &track.clips)
        .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks, 0, 0))
        .collect();
    clips.sort_unstable();
    clips
}

/// Where a test writes its timeline files.
fn out_dir() -> PathBuf {
    crate::scratch_pool::scratch("timeline-files-out")
}

/// Every placed clip: timeline start and end, source start and end, in ticks.
fn placed(state: &AppState) -> Vec<(i64, i64, i64, i64)> {
    let mut clips: Vec<_> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .map(|clip| {
            (
                clip.timeline_start_ticks,
                clip.timeline_end_ticks,
                clip.source_start_ticks,
                clip.source_end_ticks,
            )
        })
        .collect();
    clips.sort_unstable();
    clips
}

/// Every registered exporter's extension, in registration order.
fn extensions(state: &AppState) -> Vec<String> {
    let extensions: Vec<String> = state
        .ctx
        .exporters()
        .iter()
        .map(|exporter| exporter.extension.clone())
        .collect();
    assert!(
        !extensions.is_empty(),
        "the shipped plugin set registers no timeline exporter"
    );
    extensions
}

#[test]
fn every_exported_format_imports_back_into_a_new_project_as_one_undo_step() {
    let dir = out_dir();
    for extension in extensions(&editing()) {
        let (mut state, read) = carried(&extension);
        let cut = read(&state);
        assert!(
            !cut.is_empty(),
            ".{extension}: the fixture holds nothing it writes"
        );
        let file = dir.join(format!("cut.{extension}"));
        assert!(
            state.export_timeline(&file),
            ".{extension}: {:?}",
            state.last_error
        );
        assert!(file.is_file(), ".{extension} was not written");

        state.new_project().expect("new project");
        assert!(
            state.import_timeline(&file),
            ".{extension}: {:?}",
            state.last_error
        );
        assert_eq!(read(&state), cut, ".{extension} read back differently");
        assert!(
            state
                .status_message
                .as_deref()
                .is_some_and(|status| status.starts_with(&format!("Imported cut.{extension}"))),
            "{:?}",
            state.status_message
        );
        assert!(state.undo(), ".{extension}: nothing to undo");
        assert!(
            placed(&state).is_empty(),
            ".{extension}: undo left a clip behind"
        );
        assert!(state.redo());
        assert_eq!(read(&state), cut);
    }
}

#[test]
fn a_picked_destination_and_a_picked_file_run_the_same_verbs() {
    let mut state = editing();
    let cut = placed(&state);
    let file = out_dir().join("picked");
    let written = file.with_extension(&extensions(&state)[0]);

    state.supply_pick(Answer::file(&file));
    state.pick_and_export_timeline();
    assert_eq!(state.picks_asked(), 1);
    assert!(written.is_file(), "{:?}", state.last_error);
    assert!(
        !file.exists(),
        "the name with no extension was written as given"
    );

    state.new_project().expect("new project");
    state.supply_pick(Answer::file(&written));
    state.pick_and_import_timeline();
    assert_eq!(state.picks_asked(), 2);
    assert_eq!(placed(&state), cut);
}

#[test]
fn a_cancelled_pick_writes_and_reads_nothing() {
    let mut state = editing();
    let cut = placed(&state);
    state.supply_pick(Answer::cancelled());
    state.pick_and_export_timeline();
    state.supply_pick(Answer::cancelled());
    state.pick_and_import_timeline();
    assert_eq!(state.picks_asked(), 2);
    assert_eq!(placed(&state), cut);
    assert_eq!(state.last_error, None);
}

#[test]
fn a_dropped_timeline_file_is_read_into_the_project() {
    let dir = out_dir();
    for extension in extensions(&editing()) {
        let (mut state, read) = carried(&extension);
        let cut = read(&state);
        let file = dir.join(format!("drop.{extension}"));
        assert!(state.export_timeline(&file), "{:?}", state.last_error);
        let shouted = dir.join(format!("DROP.{}", extension.to_ascii_uppercase()));
        std::fs::rename(&file, &shouted).expect("rename");
        assert_eq!(state.classify_drop(&shouted), DropAction::ImportTimeline);

        state.new_project().expect("new project");
        state
            .handle_dropped_paths(vec![shouted.clone()])
            .expect("drop");
        assert_eq!(
            read(&state),
            cut,
            "{} read back differently",
            shouted.display()
        );
    }
}

#[test]
fn every_extension_an_importer_reads_drops_as_a_timeline() {
    let state = editing();
    let importers = state.ctx.importers();
    assert!(!importers.is_empty(), "no timeline importer is registered");
    for importer in importers {
        for extension in &importer.extensions {
            for name in [
                format!("cut.{extension}"),
                format!("CUT.{}", extension.to_ascii_uppercase()),
            ] {
                assert_eq!(
                    state.classify_drop(Path::new(&name)),
                    DropAction::ImportTimeline,
                    "{name} is read by {}",
                    importer.id
                );
            }
        }
    }
}

#[test]
fn an_extension_no_exporter_writes_is_refused_and_writes_nothing() {
    let mut state = editing();
    let file = out_dir().join("cut.txt");
    assert!(!state.export_timeline(&file));
    assert!(!file.exists());
    let listed: Vec<String> = extensions(&state)
        .iter()
        .map(|extension| format!(".{extension}"))
        .collect();
    assert_eq!(
        state.last_error.as_deref(),
        Some(
            format!(
                "No timeline exporter writes .txt files. Name the file with one of {}.",
                listed.join(", ")
            )
            .as_str()
        )
    );
}

#[test]
fn a_file_no_importer_reads_is_refused_and_changes_nothing() {
    let mut state = editing();
    let cut = placed(&state);
    let file = out_dir().join("notes.txt");
    std::fs::write(&file, "not a timeline").expect("write");
    assert!(!state.import_timeline(&file));
    assert_eq!(placed(&state), cut);
    assert_eq!(
        state.last_error,
        Some(format!("No importer reads {}", file.display()))
    );
}

#[test]
fn a_file_the_importer_cannot_read_leaves_the_project_as_it_was() {
    let mut state = editing();
    let cut = placed(&state);
    let file = out_dir().join(format!("broken.{}", extensions(&state)[0]));
    std::fs::write(&file, "\u{0}\u{1}not a timeline").expect("write");
    let before = state.next_undo_label();
    assert!(!state.import_timeline(&file));
    assert_eq!(placed(&state), cut);
    assert_eq!(
        state.next_undo_label(),
        before,
        "a failed import left an undo step"
    );
    assert!(state.last_error.is_some());
    assert_eq!(state.timeline_report, None);
}

/// A bare context mounts no interchange plugin, so neither dialog opens and
/// the error states how to get one.
#[test]
fn with_no_importer_or_exporter_no_dialog_opens() {
    let mut state = AppState::new(
        PluginContext::new(),
        crate::scratch_pool::scratch("timeline-files-bare"),
    )
    .expect("state");
    state.new_project().expect("new project");
    state.pick_and_import_timeline();
    assert_eq!(state.picks_asked(), 0);
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|error| error.starts_with("No timeline importer is mounted.")),
        "{:?}",
        state.last_error
    );
    state.pick_and_export_timeline();
    assert_eq!(state.picks_asked(), 0);
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|error| error.starts_with("No timeline exporter is mounted")),
        "{:?}",
        state.last_error
    );
}

/// A run that could not carry an item as it was opens the Timeline File
/// dialog on the report. Closing the dialog drops the report, and a clean run
/// drops an earlier one and closes its dialog.
#[test]
fn what_a_run_could_not_carry_opens_the_timeline_file_dialog_until_closed() {
    let mut state = editing();
    let (track, name) = hide_the_placed_lane(&mut state);
    let extension = &extensions(&state)[0];
    let file = out_dir().join(format!("hidden.{extension}"));

    assert!(state.export_timeline(&file), "{:?}", state.last_error);
    let report = state.timeline_report.clone().expect("a report");
    assert_eq!(&*report.title, format!("Exported hidden.{extension}"));
    assert_eq!(
        report.lines,
        vec![SharedString::from(format!(
            "{name}: a hidden lane, written as shown"
        ))]
    );
    assert_eq!(
        state.status_message.as_deref(),
        Some(
            format!("Exported hidden.{extension}; 1 item placed or written the nearest way")
                .as_str()
        )
    );
    assert_eq!(
        state.dialog,
        DialogKind::TimelineReport,
        "the report opened no dialog"
    );

    state.open_dialog(DialogKind::None);
    assert_eq!(
        state.timeline_report, None,
        "a closed dialog kept its report"
    );

    assert!(state.export_timeline(&file));
    assert_eq!(state.dialog, DialogKind::TimelineReport);
    state
        .apply_command(Command::Edit(EditCommand::SetTrackEnabled {
            track,
            enabled: true,
        }))
        .expect("show the lane");
    assert!(state.export_timeline(&file));
    assert_eq!(
        state.timeline_report, None,
        "a clean export left the earlier report up"
    );
    assert_eq!(
        state.dialog,
        DialogKind::None,
        "a clean export left the dialog up"
    );
}

/// A report a run leaves while the install prompt is up waits for the prompt
/// to be answered, as a package dropped beside a timeline file leaves it, and
/// then opens.
#[test]
fn a_report_left_behind_the_install_prompt_opens_once_it_is_answered() {
    let mut state = editing();
    hide_the_placed_lane(&mut state);
    let file = out_dir().join(format!("behind-prompt.{}", extensions(&state)[0]));
    state.dialog = DialogKind::InstallPlugin;

    assert!(state.export_timeline(&file), "{:?}", state.last_error);
    assert_eq!(
        state.dialog,
        DialogKind::InstallPlugin,
        "the report replaced the install prompt"
    );
    assert!(state.timeline_report.is_some(), "the run left no report");

    state.cancel_plugin_install();
    assert_eq!(
        state.dialog,
        DialogKind::TimelineReport,
        "the answered prompt did not return to the report"
    );
    assert!(state.timeline_report.is_some(), "the report was dropped");
}

/// Hide the lane the fixture was placed on, which an export writes as shown
/// and reports. Returns the lane and its name.
fn hide_the_placed_lane(state: &mut AppState) -> (TrackId, String) {
    let lane = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|lane| !lane.clips.is_empty())
        .expect("the lane the fixture was placed on");
    let (track, name) = (TrackId::from_raw(lane.id), lane.name.to_string());
    state
        .apply_command(Command::Edit(EditCommand::SetTrackEnabled {
            track,
            enabled: false,
        }))
        .expect("hide the lane");
    (track, name)
}

/// Every function in `app/timeline_files.rs` whose name contains
/// `export_timeline`.
fn timeline_export_writers_in_source() -> std::collections::BTreeSet<String> {
    use crate::test_support::sources::{code_lines, production_sources};

    let mut writers = std::collections::BTreeSet::new();
    for (rel, code) in production_sources() {
        if rel != "app/timeline_files.rs" {
            continue;
        }
        for line in code_lines(&code) {
            let trimmed = line.trim();
            for prefix in ["pub fn ", "pub(crate) fn ", "pub(super) fn ", "fn "] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    let end = rest
                        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                        .unwrap_or(rest.len());
                    let name = &rest[..end];
                    if name.contains("export_timeline") {
                        writers.insert(name.to_owned());
                    }
                }
            }
        }
    }
    writers
}

/// Every function in `app/` whose body opens `Ask::SaveFile` for an `"Export`
/// dialog. Braces are counted on lexed lines, whose string literals are blank;
/// the title is matched on the same lines with their literals kept.
fn save_file_export_writers_in_app_source() -> std::collections::BTreeSet<String> {
    use crate::test_support::sources::{code_lines, production_sources};

    let mut writers = std::collections::BTreeSet::new();
    for (rel, code) in production_sources() {
        if rel != "app.rs" && !rel.starts_with("app/") {
            continue;
        }
        let lines = code_lines(&code);
        let source: Vec<&str> = code.lines().collect();
        let mut index = 0usize;
        while index < lines.len() {
            let trimmed = lines[index].trim();
            let mut fn_name = None;
            for prefix in ["pub fn ", "pub(crate) fn ", "pub(super) fn ", "fn "] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    let end = rest
                        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                        .unwrap_or(rest.len());
                    if end > 0 {
                        fn_name = Some(rest[..end].to_owned());
                        break;
                    }
                }
            }
            let Some(name) = fn_name else {
                index += 1;
                continue;
            };
            let mut depth = 0i32;
            let mut seen_open = false;
            let mut body = String::new();
            while index < lines.len() {
                let line = &lines[index];
                for ch in line.chars() {
                    match ch {
                        '{' => {
                            depth += 1;
                            seen_open = true;
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                body.push_str(source.get(index).copied().unwrap_or_default());
                body.push('\n');
                index += 1;
                if seen_open && depth <= 0 {
                    break;
                }
            }
            if body.contains("Ask::SaveFile(") && body.contains("FileSpec::new(\"Export") {
                writers.insert(name);
            }
        }
    }
    writers
}

/// WHY: `export_timeline`, `pick_and_export_timeline`, and
/// `pick_and_export_timeline_as` passed the chosen destination straight to
/// `PluginContext::export_file` without calling
/// `aria_render::check_export_destination`, so exporting a timeline over the
/// open project file or a media clip in its pool (directly, via `..`, via a
/// symlink with the exporter's extension, or via a bare stem whose appended
/// extension named an input) replaced the project or its footage. Every
/// timeline export entry point in `app/timeline_files.rs`, expanded across
/// every registered exporter in `state.ctx.exporters()`, plus every
/// `Ask::SaveFile(FileSpec::new("Export..."))` writer in `app/`, is derived
/// from source at run time and verified to refuse both inputs without touching
/// their bytes on disk. Not caught: overwriting an unrelated file outside the
/// open project.
#[test]
fn every_timeline_and_save_file_export_writer_refuses_the_project_file_and_its_media() {
    use std::collections::BTreeSet;

    let derived_timeline = timeline_export_writers_in_source();
    let covered_timeline: BTreeSet<String> = [
        "export_timeline",
        "pick_and_export_timeline",
        "pick_and_export_timeline_as",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(
        covered_timeline, derived_timeline,
        "every *export_timeline* writer in app/timeline_files.rs must be exercised in this test"
    );

    let mut state = captioned();
    let dir = out_dir().join("refuse-inputs");
    std::fs::create_dir_all(&dir).expect("create test dir");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s.mp4")
        .canonicalize()
        .expect("fixture exists");
    let media = dir.join("clip.mp4");
    std::fs::copy(&fixture, &media).expect("copy media");
    state.import_media(media.clone()).expect("import media");
    let project_file = dir.join("cut.aria");
    state
        .save_project_to(project_file.clone())
        .expect("save project");

    let project_bytes = std::fs::read(&project_file).expect("read project");
    let media_bytes = std::fs::read(&media).expect("read media");
    let sub = dir.join("nested");
    std::fs::create_dir_all(&sub).expect("create nested dir");

    let exporters: Vec<(String, String)> = state
        .ctx
        .exporters()
        .into_iter()
        .map(|spec| (spec.id.clone(), spec.extension.clone()))
        .collect();
    assert!(!exporters.is_empty(), "shipped exporters must be mounted");

    // 1. Direct `export_timeline` and `pick_and_export_timeline` over the
    //    project file and media (canonical and `..` spellings).
    for (target, what) in [
        (project_file.clone(), "the project file"),
        (media.clone(), "media this project uses"),
        (sub.join("..").join("cut.aria"), "the project file"),
        (sub.join("..").join("clip.mp4"), "media this project uses"),
    ] {
        let expected = format!("{} is {what}; export to another file", target.display());

        state.last_error = None;
        assert!(
            !state.export_timeline(&target),
            "export_timeline accepted {what} at {}",
            target.display()
        );
        assert_eq!(state.last_error.as_deref(), Some(expected.as_str()));
        assert_eq!(state.timeline_report, None);
        assert_eq!(state.dialog, DialogKind::None);

        state.last_error = None;
        state.supply_pick(Answer::file(&target));
        state.pick_and_export_timeline();
        assert_eq!(state.last_error.as_deref(), Some(expected.as_str()));

        for (exporter_id, _) in &exporters {
            state.last_error = None;
            state.supply_pick(Answer::file(&target));
            state.pick_and_export_timeline_as(exporter_id);
            assert_eq!(
                state.last_error.as_deref(),
                Some(expected.as_str()),
                "pick_and_export_timeline_as({exporter_id}) over {what}"
            );
        }
    }

    // 2. Every registered exporter's extension: a symlink `*.<ext>` pointing to
    //    the project file or a pool media file, plus an extensionless stem
    //    whose appended `.<ext>` resolves to that symlink.
    #[cfg(unix)]
    for (idx, (exporter_id, ext)) in exporters.iter().enumerate() {
        let ext_dir = dir.join(format!("ext-{idx}-{ext}"));
        std::fs::create_dir_all(&ext_dir).expect("create exporter dir");
        let proj_stem = ext_dir.join("proj-target");
        let proj_with_ext = ext_dir.join(format!("proj-target.{ext}"));
        let media_stem = ext_dir.join("media-target");
        let media_with_ext = ext_dir.join(format!("media-target.{ext}"));
        std::os::unix::fs::symlink(&project_file, &proj_with_ext).expect("symlink project");
        std::os::unix::fs::symlink(&media, &media_with_ext).expect("symlink media");

        for (chosen, resolved, what) in [
            (&proj_with_ext, &proj_with_ext, "the project file"),
            (&media_with_ext, &media_with_ext, "media this project uses"),
            (&proj_stem, &proj_with_ext, "the project file"),
            (&media_stem, &media_with_ext, "media this project uses"),
        ] {
            let expected = format!("{} is {what}; export to another file", resolved.display());

            state.last_error = None;
            state.supply_pick(Answer::file(chosen));
            state.pick_and_export_timeline_as(exporter_id);
            assert_eq!(
                state.last_error.as_deref(),
                Some(expected.as_str()),
                "pick_and_export_timeline_as({exporter_id}) on {}",
                chosen.display()
            );

            if chosen == resolved || idx == 0 {
                state.last_error = None;
                assert!(
                    !state.export_timeline(chosen),
                    "export_timeline accepted {} -> {what}",
                    chosen.display()
                );
                assert_eq!(
                    state.last_error.as_deref(),
                    Some(expected.as_str()),
                    "export_timeline on {}",
                    chosen.display()
                );

                state.last_error = None;
                state.supply_pick(Answer::file(chosen));
                state.pick_and_export_timeline();
                assert_eq!(
                    state.last_error.as_deref(),
                    Some(expected.as_str()),
                    "pick_and_export_timeline on {}",
                    chosen.display()
                );
            }
        }
    }

    // 3. Every `Ask::SaveFile(FileSpec::new("Export..."))` writer in `app/`
    //    derived from `production_sources()`.
    let derived_save_exports = save_file_export_writers_in_app_source();
    type SaveExportRunner = (&'static str, Box<dyn Fn(&mut AppState, &Path)>);
    let save_export_runners: Vec<SaveExportRunner> = vec![
        (
            "ask_timeline_path",
            Box::new(|state, path| {
                state.supply_pick(Answer::file(path));
                state.pick_and_export_timeline();
            }),
        ),
        (
            "export_keymap",
            Box::new(|state, path| {
                state.supply_pick(Answer::file(path));
                state.export_keymap();
            }),
        ),
        (
            "pick_and_export_frame",
            Box::new(|state, path| {
                state.supply_pick(Answer::file(path));
                state.pick_and_export_frame();
            }),
        ),
        (
            "pick_export_destination",
            Box::new(|state, path| {
                state.supply_pick(Answer::file(path));
                state.pick_export_destination();
                state.export_to_destination();
            }),
        ),
    ];
    let covered_save_exports: BTreeSet<String> = save_export_runners
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect();
    assert_eq!(
        covered_save_exports, derived_save_exports,
        "every Ask::SaveFile(\"Export...\") writer in app/ must be exercised in this table"
    );

    for (writer, run) in &save_export_runners {
        for (target, what) in [
            (&project_file, "the project file"),
            (&media, "media this project uses"),
        ] {
            state.last_error = None;
            run(&mut state, target);
            assert_eq!(
                state.last_error.as_deref(),
                Some(format!("{} is {what}; export to another file", target.display()).as_str()),
                "{writer} over {what}"
            );
        }
    }

    assert_eq!(
        std::fs::read(&project_file).expect("read project"),
        project_bytes,
        "the project file was modified"
    );
    assert_eq!(
        std::fs::read(&media).expect("read media"),
        media_bytes,
        "the media file was modified"
    );
}
