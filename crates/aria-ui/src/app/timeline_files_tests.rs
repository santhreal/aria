//! Timeline files through the editor: every registered exporter writes the
//! open timeline and its importer reads it back as one undo step, by menu
//! pick and by drop; the destination's extension selects the exporter; and
//! what a run could not carry stays in the report until dismissed or replaced.
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

use super::AppState;
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
        Some(format!("no importer reads {}", file.display()))
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
            .is_some_and(|error| error.starts_with("No timeline exporter is mounted.")),
        "{:?}",
        state.last_error
    );
}

#[test]
fn what_a_run_could_not_carry_stays_reported_until_dismissed_or_replaced() {
    let mut state = editing();
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
        Some(format!("Exported hidden.{extension}: 1 item in the Timeline file panel").as_str())
    );
    assert!(shown(&state), "the Timeline file panel is hidden");

    state.dismiss_timeline_report();
    assert_eq!(state.timeline_report, None);
    assert!(!shown(&state), "a dismissed report left the panel up");

    assert!(state.export_timeline(&file));
    assert!(state.timeline_report.is_some());
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
    assert!(!shown(&state));
}

/// Whether the inspector draws the Timeline file panel, by the predicate the
/// panel registered.
fn shown(state: &AppState) -> bool {
    let host = crate::plugins::panel_host::host_of(&state.ctx).expect("panel host");
    host.cached_at(aria_plugin::PanelLocation::Inspector)
        .iter()
        .find(|entry| entry.id == "aria.panel.timeline-report")
        .is_some_and(|entry| (entry.visible)(state))
}
