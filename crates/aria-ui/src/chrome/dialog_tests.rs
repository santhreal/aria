//! Every dialog card fits the window it is drawn in, every dialog title is in
//! Title Case, every line a dialog draws starts in sentence case, the Audio
//! Gain dialog draws each line once in every mode, and every dialog setting
//! row stands its label and its value on one grid.
//! The class closed here: a card as tall as its body. A body taller than the
//! window put the title, the header controls and the action row outside the
//! window, where no pointer reaches them and no scroll brings them back.
//!
//! For every dialog kind the overlay registry holds, drawn in a window smaller
//! than a card in both directions, the card lies inside the window, and the
//! header, the scrolling content and the action row lie inside the card.
//!
//! The kinds are read from the registry at run time, and every one of them
//! draws a card through [`super::dialog_surface`].
//!
//! Not covered: dialogs registered by a plugin outside this crate, pixels, and
//! which row the content region is scrolled to.

use std::collections::HashSet;

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, ClipParam, ParamValue};
use gpui::{Bounds, Pixels, Size, TestAppContext, VisualTestContext, point, px, size};

use super::{LABEL_WIDTH, drawn_setting_rows, record_setting_rows};
use crate::app::DialogKind;
use crate::app::clip_ops::AudioGainMode;
use crate::app::status::lowercase_start;
use crate::commands::casing_tests::check_title_case;
use crate::components::noted::{NotingControls, drawn_controls, record_controls};
use crate::interned::intern;
use crate::plugins::overlay_host::host_of;
use crate::test_support::{bounds_inside, drawn_shell, project_text, select_a_still};
use crate::theme::Theme;
/// A window narrower and shorter than a dialog card.
const SHORT: Size<Pixels> = size(px(480.), px(320.));

#[gpui::test]
fn every_dialog_card_fits_a_short_window(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "dialog-fit");
    visual.simulate_resize(SHORT);
    visual.run_until_parked();
    let window = Bounds::new(
        point(px(0.), px(0.)),
        visual.update(|window, _| window.viewport_size()),
    );
    assert_eq!(window.size, SHORT, "the window did not take the short size");

    let kinds = registered(&state, visual);
    assert!(
        kinds.len() >= 7,
        "the shipped dialogs did not register: {kinds:?}"
    );

    let mut offenders = Vec::new();
    for kind in kinds {
        open(&state, visual, kind);
        offenders.extend(misplaced(kind, window, visual));
    }
    assert!(
        offenders.is_empty(),
        "drawn in a {:?} window:\n  {}",
        window.size,
        offenders.join("\n  ")
    );
}

/// Every dialog title is in Title Case, as the menu rows and palette entries
/// that open the dialogs are.
///
/// The class closed here: a title in sentence case beside Title Case ones, as
/// `Command palette`, `Unsaved changes` and `Install plugin` were. The title is
/// read from the frame each registered kind draws, so a kind registered later
/// is checked on the run that adds it. The Install Plugin dialog's other title,
/// for an update, is checked by `panels::install_dialog::tests`.
#[gpui::test]
fn every_dialog_title_is_in_title_case(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "dialog-titles");
    visual.update(|_, app| app.set_reduce_motion(true));
    let mut titles = Vec::new();
    let mut offenders = Vec::new();
    for kind in registered(&state, visual) {
        open(&state, visual, kind);
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        let Some(bounds) = visual.debug_bounds("dialog-title") else {
            offenders.push(format!("{kind:?} drew no dialog-title"));
            continue;
        };
        let title: String = visual.update(|window, _| {
            let mut runs: Vec<_> = window
                .rendered_text_runs()
                .iter()
                .filter(|run| bounds.contains(&run.bounds.center()))
                .map(|run| (f32::from(run.bounds.left()), run.text.to_string()))
                .collect();
            runs.sort_by(|a, b| a.0.total_cmp(&b.0));
            runs.into_iter().map(|(_, text)| text).collect()
        });
        if let Err(reason) = check_title_case(&title) {
            offenders.push(format!("{kind:?}: {reason}"));
        }
        titles.push(title);
    }
    assert!(titles.len() >= 7, "the walk read only {titles:?}");
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// Every line a dialog draws starts in sentence case, as the buttons and the
/// prose beside it do.
///
/// The class closed here: a value or a description that starts in lower case
/// among capitalized ones, as the Export sheet's `included`, the motion rows'
/// `every transition at its designed speed`, the Keyboard Shortcuts section's `unbound`,
/// the Project Report's `none` and the Media Support report's `available`
/// were. Every run drawn inside the card of each registered kind is read from
/// the frame, so a row added later is checked on the run that adds it. A run
/// the editor behind the card draws at the same place is the editor's, and is
/// skipped. One binding is cleared before the walk, so the Keyboard Shortcuts section draws
/// a row with no chord, and a still is placed and selected, so a dialog that
/// reads the selection draws its rows for it. A line the project supplies,
/// such as the still's name, is written as the project wrote it and is
/// skipped.
///
/// Not covered: a line a dialog draws only after a choice changes or a job
/// reports, beyond the cleared binding, a line a dialog draws only for a clip
/// that is not a still, and a lower case word inside a line.
#[gpui::test]
fn every_line_a_dialog_draws_starts_in_sentence_case(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "dialog-casing");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.update(|_, app| {
        state.update(app, |state, _| {
            let Some(first) = state.binding_rows().first().map(|row| row.binding.id) else {
                panic!("the registry holds no binding to clear");
            };
            state.clear_binding(first);
            select_a_still(state);
        })
    });
    let behind = runs_behind(visual);
    let project = visual.read(|app| project_text(state.read(app)));
    let mut read = Vec::new();
    let mut offenders = Vec::new();
    for kind in registered(&state, visual) {
        open(&state, visual, kind);
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        let Some(lines) = card_lines(visual, &behind) else {
            offenders.push(format!("{kind:?} drew no dialog-card"));
            continue;
        };
        if lines.is_empty() {
            offenders.push(format!("{kind:?} drew no line"));
        }
        for line in &lines {
            if project.iter().any(|text| line.starts_with(text.as_str())) {
                continue;
            }
            if let Some(word) = lowercase_start(line) {
                offenders.push(format!("{kind:?}: {line:?} starts with {word:?}"));
            }
        }
        read.push((kind, lines.len()));
    }
    assert!(read.len() >= 7, "the walk read only {read:?}");
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// The Audio Gain dialog draws each line once, in every mode.
///
/// The class closed here: a dialog row that repeats a row above it. The field
/// under the mode chips was labeled with the selected chip's label, so `Set
/// Gain to` was drawn twice, and a row of step buttons drew `-3 dB`, `-1 dB`
/// and `+3 dB` under presets with the same labels, where a preset set the gain
/// to the value and a step moved it by the value. Every mode in
/// [`AudioGainMode::ALL`] is selected in turn, and every run inside the card is
/// read from the frame.
///
/// Not covered: the other dialogs, where a column repeats its rows' labels, as
/// the In and Out columns of Keyframe Velocity do.
#[gpui::test]
fn the_audio_gain_dialog_draws_each_line_once_in_every_mode(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "audio-gain-lines");
    visual.update(|_, app| app.set_reduce_motion(true));
    let behind = runs_behind(visual);
    open(&state, visual, DialogKind::AudioGain);
    let mut offenders = Vec::new();
    for mode in AudioGainMode::ALL {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.set_audio_gain_mode(mode);
                cx.notify();
            })
        });
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        let Some(lines) = card_lines(visual, &behind) else {
            panic!("{mode:?}: the Audio Gain dialog drew no dialog-card");
        };
        assert!(
            lines.iter().any(|line| line == mode.label()),
            "{mode:?}: the walk read no `{}` chip in {lines:?}",
            mode.label()
        );
        let mut seen = HashSet::new();
        for line in &lines {
            if !seen.insert(line) {
                offenders.push(format!("{mode:?}: {line:?} is drawn twice"));
            }
        }
    }
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// The setting rows `kind` draws, in draw order. Matched exhaustively over
/// [`DialogKind`] so a new dialog does not compile until its rows are stated.
fn setting_rows(kind: DialogKind) -> &'static [&'static str] {
    match kind {
        DialogKind::Export => &[
            "Destination",
            "Format",
            "Encoder",
            "Scale",
            "Size",
            "Frame rate",
            "Codec",
            "Bitrate",
            "Keyframes",
            "Audio",
            "Range",
        ],
        DialogKind::SpeedDuration => &["Duration"],
        DialogKind::AudioGain => &["Gain"],
        DialogKind::MarkerEdit => &["Name", "Comment", "Duration (timecode)", "Color", "Kind"],
        DialogKind::RemoveAttributes => &["Attributes"],
        DialogKind::KeyframeVelocity => &["Speed", "Influence", "Speed", "Influence", "Continuous"],
        DialogKind::None
        | DialogKind::Settings
        | DialogKind::Palette
        | DialogKind::Jobs
        | DialogKind::ProjectReport
        | DialogKind::MediaSupport
        | DialogKind::TimelineReport
        | DialogKind::UnsavedChanges
        | DialogKind::InstallPlugin
        | DialogKind::About
        | DialogKind::Plugin(_) => &[],
    }
}

/// WHY: Each setting-row dialog built its own row layout, so Export stood its
/// values beside a 72 px label column while Speed/Duration, Audio Gain, Edit
/// Marker, Remove Attributes, Keyframe Velocity and `toggle_row` pushed their
/// values or switches to the far edge of the card or stacked them under
/// title-sized labels.
///
/// Walks every registered [`DialogKind`], matched exhaustively by
/// [`setting_rows`], plus every [`AudioGainMode`], and checks the drawn frame:
/// every setting row is built through [`super::setting_row`], every dialog's
/// label column has width [`LABEL_WIDTH`] starting on the dialog title's left
/// edge, and every row's value or control starts at the same x beside the
/// label column. Does not catch pixel colors or font rasterization, which the
/// test text system lays out at fixed glyph advance.
#[gpui::test]
fn every_setting_row_dialog_stands_its_labels_and_values_on_one_grid(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    aria_encoder::available_codecs();
    let (state, visual) = drawn_shell(cx, "dialog-setting-rows");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.update(|_, app| {
        state.update(app, |state, _| {
            select_a_still(state);
            let clip = ClipId::from_raw(state.store.selection[0]);
            state
                .apply_command(Command::Edit(EditCommand::SetParamConstant {
                    clip,
                    param: ClipParam::Opacity,
                    value: ParamValue::Scalar(0.5),
                }))
                .expect("opacity lands on the selected clip");
            state.sync_from_engine();
            state.select_clip(clip);
            state.open_remove_attributes();
            state.add_marker_at_playhead().expect("a marker lands");
            let marker = state.store.sequence.markers[0].id;
            assert!(state.open_marker_edit(marker), "the marker's dialog opens");
            state.open_dialog(DialogKind::None);
        })
    });
    let behind = runs_behind(visual);
    let _noting = NotingControls::start();
    let mut checked_dialogs = 0usize;
    let mut first_dialog_label_width: Option<Pixels> = None;
    let mut offenders = Vec::new();

    for kind in registered(&state, visual) {
        let expected = setting_rows(kind);
        open(&state, visual, kind);
        check_setting_rows_frame(
            &format!("{kind:?}"),
            kind,
            expected,
            &behind,
            visual,
            &mut checked_dialogs,
            &mut first_dialog_label_width,
            &mut offenders,
        );
        if kind == DialogKind::AudioGain {
            for mode in AudioGainMode::ALL {
                visual.update(|_, app| {
                    state.update(app, |state, cx| {
                        state.set_audio_gain_mode(mode);
                        cx.notify();
                    })
                });
                visual.run_until_parked();
                let mode_expected = [mode.quantity()];
                check_setting_rows_frame(
                    &format!("{kind:?}({mode:?})"),
                    kind,
                    &mode_expected,
                    &behind,
                    visual,
                    &mut checked_dialogs,
                    &mut first_dialog_label_width,
                    &mut offenders,
                );
            }
        }
    }
    for (label, fixture, tick, mode_expected) in [
        (
            "SpeedDuration(SilentMovie)",
            "../../tests/fixtures/fixture-640x360-24fps-3s.mp4",
            240,
            &["Speed", "Duration", "Reverse Speed"][..],
        ),
        (
            "SpeedDuration(SoundMovie)",
            "../../tests/fixtures/fixture-640x360-24fps-2s-av.mp4",
            480,
            &["Speed", "Duration", "Reverse Speed", "Maintain Audio Pitch"][..],
        ),
    ] {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.open_dialog(DialogKind::None);
                let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(fixture);
                let media = state.import_media(path).expect("import movie fixture");
                state
                    .drop_media_on_timeline(media, None, tick)
                    .expect("place movie fixture");
                let clip = state
                    .store
                    .sequence
                    .tracks
                    .iter()
                    .filter(|track| track.kind == crate::types::UiTrackKind::Video)
                    .flat_map(|track| track.clips.iter())
                    .last()
                    .map(|clip| ClipId::from_raw(clip.id))
                    .expect("movie fixture placed a video clip");
                state.select_clip_linked(clip, true);
                cx.notify();
            })
        });
        visual.run_until_parked();
        let behind = runs_behind(visual);
        open(&state, visual, DialogKind::SpeedDuration);
        check_setting_rows_frame(
            label,
            DialogKind::SpeedDuration,
            mode_expected,
            &behind,
            visual,
            &mut checked_dialogs,
            &mut first_dialog_label_width,
            &mut offenders,
        );
    }
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(DialogKind::None);
            let clips: Vec<ClipId> = state
                .store
                .sequence
                .tracks
                .iter()
                .filter(|track| track.kind == crate::types::UiTrackKind::Video)
                .flat_map(|track| track.clips.iter())
                .take(2)
                .map(|clip| ClipId::from_raw(clip.id))
                .collect();
            assert_eq!(clips.len(), 2, "expected still and silent movie on V1");
            state.selection = clips;
            state.refresh_projected_selection();
            cx.notify();
        })
    });
    visual.run_until_parked();
    let behind = runs_behind(visual);
    open(&state, visual, DialogKind::SpeedDuration);
    check_setting_rows_frame(
        "SpeedDuration(MixedStillAndMovie)",
        DialogKind::SpeedDuration,
        &["Speed", "Duration", "Reverse Speed"],
        &behind,
        visual,
        &mut checked_dialogs,
        &mut first_dialog_label_width,
        &mut offenders,
    );
    assert!(
        checked_dialogs >= 12,
        "expected all six setting-row dialogs, four Audio Gain modes, and four Speed/Duration selections, checked {checked_dialogs}"
    );
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

#[allow(clippy::too_many_arguments)]
fn check_setting_rows_frame(
    scope: &str,
    kind: DialogKind,
    expected: &[&'static str],
    behind: &HashSet<(String, u32, u32)>,
    visual: &mut VisualTestContext,
    checked_dialogs: &mut usize,
    first_dialog_label_width: &mut Option<Pixels>,
    offenders: &mut Vec<String>,
) {
    const EPS: f32 = 0.51;
    record_setting_rows();
    record_controls();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let drawn = drawn_setting_rows();
    let (controls, _) = drawn_controls();

    if expected.is_empty() {
        if !drawn.is_empty() {
            offenders.push(format!(
                "{scope}: drew setting rows {drawn:?} without listing them in setting_rows"
            ));
        }
        return;
    }
    *checked_dialogs += 1;

    if kind == DialogKind::Export {
        let filtered: Vec<&'static str> = expected
            .iter()
            .copied()
            .filter(|label| drawn.contains(label))
            .collect();
        if drawn.len() < 9 || drawn != filtered {
            offenders.push(format!(
                "{scope}: drew setting rows {drawn:?}, expected at least 9 in-order rows from {expected:?}"
            ));
            return;
        }
    } else if drawn != expected {
        offenders.push(format!(
            "{scope}: drew setting rows {drawn:?}, expected {expected:?}"
        ));
        return;
    }

    let Some(card) = visual.debug_bounds("dialog-card") else {
        offenders.push(format!("{scope}: drew no dialog-card"));
        return;
    };
    let Some(title) = visual.debug_bounds("dialog-title") else {
        offenders.push(format!("{scope}: drew no dialog-title"));
        return;
    };

    let runs: Vec<gpui::TextRunLayout> = visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .filter(|run| {
                let c = run.bounds.center();
                c.x >= card.left() && c.x <= card.right() && c.y >= card.top()
            })
            .filter(|run| !behind.contains(&placed(run)))
            .cloned()
            .collect()
    });

    let mut control_boxes: Vec<Bounds<Pixels>> = controls
        .iter()
        .filter_map(|control| visual.debug_bounds(intern(&control.selector)))
        .collect();
    for extra in [
        "speed-duration-reverse",
        "speed-duration-pitch",
        "keyframe-velocity-continuous",
        "marker-edit-colors",
        "marker-edit-field-Name",
        "marker-edit-field-Comment",
        "marker-edit-field-Duration (timecode)",
    ] {
        if let Some(bounds) = visual.debug_bounds(extra) {
            control_boxes.push(bounds);
        }
    }

    let expected_value_x = title.left() + px(LABEL_WIDTH + Theme::SPACE_MD);
    let mut first_value_x: Option<Pixels> = None;
    let mut unique_labels: Vec<&'static str> = Vec::new();
    for &label in &drawn {
        if !unique_labels.contains(&label) {
            unique_labels.push(label);
        }
    }

    for label in unique_labels {
        if visual
            .debug_bounds(intern(&format!("dialog-setting-{label}")))
            .is_none()
        {
            offenders.push(format!("{scope}: missing dialog-setting-{label}"));
            continue;
        }
        let Some(label_bounds) = visual.debug_bounds(intern(&format!("dialog-label-{label}")))
        else {
            offenders.push(format!("{scope}: missing dialog-label-{label}"));
            continue;
        };
        if (label_bounds.left() - title.left()).abs() >= px(EPS) {
            offenders.push(format!(
                "{scope} `{label}`: label starts at {:?}, title at {:?}",
                label_bounds.left(),
                title.left()
            ));
        }
        if (label_bounds.size.width - px(LABEL_WIDTH)).abs() >= px(EPS) {
            offenders.push(format!(
                "{scope} `{label}`: label width is {:?}, expected {:?}",
                label_bounds.size.width,
                px(LABEL_WIDTH)
            ));
        }
        let ref_width = *first_dialog_label_width.get_or_insert(label_bounds.size.width);
        if (label_bounds.size.width - ref_width).abs() >= px(EPS) {
            offenders.push(format!(
                "{scope} `{label}`: label width {:?} differs from first dialog label width {ref_width:?}",
                label_bounds.size.width
            ));
        }

        let count = drawn.iter().filter(|&&each| each == label).count();
        let mut label_runs: Vec<&gpui::TextRunLayout> = runs
            .iter()
            .filter(|run| {
                run.text.as_ref() == label && (run.bounds.left() - title.left()).abs() < px(EPS)
            })
            .collect();
        label_runs.sort_by(|a, b| a.bounds.top().partial_cmp(&b.bounds.top()).unwrap());
        if label_runs.len() != count {
            offenders.push(format!(
                "{scope} `{label}`: expected {count} label text runs at {:?}, found {}",
                title.left(),
                label_runs.len()
            ));
            continue;
        }

        for (idx, label_run) in label_runs.into_iter().enumerate() {
            let y = label_run.bounds.center().y;
            let on_line = |b: &Bounds<Pixels>| {
                (b.center().y - y).abs() < px(Theme::CONTROL_HEIGHT * 0.5)
                    && b.left() > label_bounds.right() - px(Theme::SPACE_SM + EPS)
            };
            let box_x = (kind != DialogKind::Export)
                .then(|| {
                    control_boxes
                        .iter()
                        .filter(|b| on_line(b))
                        .map(Bounds::left)
                        .min_by(|a, b| a.partial_cmp(b).unwrap())
                })
                .flatten();
            let text_x = runs
                .iter()
                .filter(|run| on_line(&run.bounds))
                .map(|run| run.bounds.left())
                .min_by(|a, b| a.partial_cmp(b).unwrap());
            let Some(value_x) = box_x.or(text_x) else {
                offenders.push(format!(
                    "{scope} `{label}` #{idx}: found no value or control on its line"
                ));
                continue;
            };
            let first_x = *first_value_x.get_or_insert(value_x);
            if (value_x - first_x).abs() >= px(EPS) {
                offenders.push(format!(
                    "{scope} `{label}` #{idx}: value starts at {value_x:?}, first row at {first_x:?}"
                ));
            }
            if (value_x - expected_value_x).abs() >= px(EPS) {
                offenders.push(format!(
                    "{scope} `{label}` #{idx}: value starts at {value_x:?}, expected {expected_value_x:?}"
                ));
            }
        }
    }

    // Text between the first setting row and the last belongs to a row or to a
    // heading over a group of rows: it is a row's label in the label column, a
    // part of a value beside it, or a `setting_heading`, never a line hung in
    // the label column, such as a value's presets under it.
    let is_label = |run: &gpui::TextRunLayout| {
        drawn.iter().any(|label| *label == run.text.as_ref())
            && (run.bounds.left() - title.left()).abs() < px(EPS)
    };
    let label_tops = || {
        runs.iter()
            .filter(|run| is_label(run))
            .map(|run| run.bounds.top())
    };
    let (Some(first), Some(last)) = (
        label_tops().min_by(|a, b| a.partial_cmp(b).unwrap()),
        label_tops().max_by(|a, b| a.partial_cmp(b).unwrap()),
    ) else {
        return;
    };
    for run in runs.iter().filter(|run| !is_label(run)) {
        let heading = visual
            .debug_bounds(intern(&format!("dialog-heading-{}", run.text)))
            .is_some_and(|bounds| bounds.contains(&run.bounds.center()));
        if heading {
            continue;
        }
        let top = run.bounds.top();
        if top >= first && top <= last && run.bounds.left() < expected_value_x - px(EPS) {
            offenders.push(format!(
                "{scope}: `{}` starts at {:?}, in the label column left of {expected_value_x:?}",
                run.text,
                run.bounds.left()
            ));
        }
    }
}

/// Every run on a frame drawn with no dialog open, by text and place.
fn runs_behind(visual: &mut VisualTestContext) -> HashSet<(String, u32, u32)> {
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    visual.update(|window, _| window.rendered_text_runs().iter().map(placed).collect())
}

/// The text of every run inside the card on the frame just drawn, less the
/// runs in `behind`, which the editor behind the card drew at the same place,
/// or `None` when no card was drawn.
fn card_lines(
    visual: &mut VisualTestContext,
    behind: &HashSet<(String, u32, u32)>,
) -> Option<Vec<String>> {
    let card = visual.debug_bounds("dialog-card")?;
    // The content scrolls, and a run is recorded unclipped, so a line below
    // the card's edge is still the card's.
    Some(visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .filter(|run| {
                let center = run.bounds.center();
                center.x >= card.left() && center.x <= card.right() && center.y >= card.top()
            })
            .filter(|run| !behind.contains(&placed(run)))
            .map(|run| run.text.to_string())
            .collect()
    }))
}

/// A run's text and where it was drawn.
fn placed(run: &gpui::TextRunLayout) -> (String, u32, u32) {
    (
        run.text.to_string(),
        f32::from(run.bounds.left()).to_bits(),
        f32::from(run.bounds.top()).to_bits(),
    )
}

/// The dialog kinds the overlay registry holds.
fn registered(
    state: &gpui::Entity<crate::app::AppState>,
    visual: &mut VisualTestContext,
) -> Vec<DialogKind> {
    visual.read(|app| {
        let Some(host) = host_of(&state.read(app).ctx) else {
            panic!("the ui plugin mounts the overlay host");
        };
        host.dialog_kinds()
    })
}

/// Open `kind` and draw it.
fn open(
    state: &gpui::Entity<crate::app::AppState>,
    visual: &mut VisualTestContext,
    kind: DialogKind,
) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(kind);
            cx.notify();
        });
    });
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).dialog),
        kind,
        "{kind:?} did not open"
    );
}

/// What is out of place on the frame just drawn with `kind` open, one line per
/// region.
fn misplaced(
    kind: DialogKind,
    window: Bounds<Pixels>,
    visual: &mut VisualTestContext,
) -> Vec<String> {
    let Some(card) = visual.debug_bounds("dialog-card") else {
        return vec![format!(
            "{kind:?} drew no dialog card; draw it through dialog_surface"
        )];
    };
    let mut out = Vec::new();
    if !bounds_inside(card, window) {
        out.push(format!(
            "{kind:?}: the card {card:?} is not inside the window"
        ));
    }
    for region in ["dialog-header", "dialog-content"] {
        match visual.debug_bounds(region) {
            None => out.push(format!("{kind:?} drew no {region}")),
            Some(bounds) if !bounds_inside(bounds, card) => out.push(format!(
                "{kind:?}: {region} {bounds:?} is not inside the card {card:?}"
            )),
            Some(_) => {}
        }
    }
    // A body with no action draws no action row.
    if let Some(footer) = visual.debug_bounds("dialog-footer")
        && !bounds_inside(footer, card)
    {
        out.push(format!(
            "{kind:?}: the action row {footer:?} is not inside the card {card:?}"
        ));
    }
    out
}
