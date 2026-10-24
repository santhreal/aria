//! Every line a dock panel draws starts in sentence case.
//!
//! The class closed here: a readout value, a tag or a note that starts in
//! lower case among capitalised lines, as the pool's `linked` and `no`, the
//! history row's `current` and the mixer's `not in the mix` were. A line is held to [`lowercase_start`], the
//! check the status line and the dialogs are held to.
//!
//! The panels are read from the registry and the inspector's tabs from its tab
//! host when the test runs, so a panel or a tab added later is checked on the
//! run that adds it, and a tab no dressed clip kind draws fails the walk. Each
//! panel is docked into the launch layout's right frame and drawn with a media
//! clip selected, its source picked in the pool and a frame reading for the
//! Info panel. The Inspector is drawn once per tab for each of a media clip, a
//! title and a shape the tab applies to. The Media Support report is a dialog,
//! and `chrome::dialog_tests` walks it. The Assistant is drawn a second time
//! holding a row of every kind, a plan and a question, each written by the
//! path that writes it in the editor.
//!
//! A line the project supplies, such as a media name, a clip name, a lane name
//! or a path, is written as the project wrote it and is skipped. Not covered: a
//! lower case word inside a line, and a line a panel draws only after a choice
//! changes or a job reports.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use aria_ai::WireCommand;
use aria_ai::agent::{PromptStatus, describe_action};
use aria_ai::wire::{AddEffect, AddMarker};
use aria_commands::EditOutcome;
use aria_models::{ClipId, Generator, Shape};
use gpui::{Bounds, TestAppContext, VisualTestContext, point, px, size};

use crate::app::AppState;
use crate::app::assistant::{Beat, Finished, PlanStep};
use crate::app::assistant_host::{HostRequest, PendingAsk};
use crate::app::status::lowercase_start;
use crate::components::drawn;
use crate::dock::{DockTree, FrameId, PANEL_ASSISTANT, PANEL_INSPECTOR, PanelId, Zone};
use crate::panels::inspector::tab_host::host_of as tab_host;
use crate::test_support::{drawn_shell, drawn_shell_at, project_text};
use crate::types::{AgentEntryKind, UiClipKind};

/// The window the launch layout is built for.
const WINDOW_W: f32 = 1920.0;

#[gpui::test]
fn every_line_a_panel_draws_starts_in_sentence_case(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "panel-casing");
    visual.update(|_, app| app.set_reduce_motion(true));
    let clips = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let clips = dress(state);
            cx.notify();
            clips
        })
    });
    visual.run_until_parked();
    let project = visual.read(|app| project_text(state.read(app)));
    let (media, _) = clips[0];

    let mut offenders = Vec::new();
    let panels = visual.read(|app| state.read(app).registered_panel_ids());
    assert!(!panels.is_empty(), "the ui plugin registers panels");
    for panel in panels {
        let lines = drawn_lines(&state, visual, panel, |state| state.select_clip(media));
        check(panel, &lines, &project, &mut offenders);
    }

    let mut drawn_tabs = BTreeSet::new();
    for (clip, kind) in clips {
        for tab in tab_host().visible(kind) {
            let id = tab.id.to_string();
            let what = format!("the Inspector's {id} tab for a {kind:?} clip");
            let lines = drawn_lines(&state, visual, PANEL_INSPECTOR, |state| {
                state.select_clip(clip);
                state.inspector_tab = id.clone();
            });
            assert!(!lines.is_empty(), "{what} drew no line");
            check(&what, &lines, &project, &mut offenders);
            drawn_tabs.insert(id);
        }
    }
    let shipped: BTreeSet<String> = tab_host()
        .tabs()
        .iter()
        .map(|tab| tab.id.to_string())
        .collect();
    assert_eq!(
        drawn_tabs, shipped,
        "a tab applies to none of the dressed clip kinds; dress a clip it applies to"
    );
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// The Assistant holding a conversation: a row of every kind, a pending plan
/// and a pending question, with a model chosen and nothing selected. Every line
/// it draws starts in sentence case.
///
/// Each kind's label, each row, the plan's step and the question are found
/// among the drawn lines first, so one drawn outside the frame fails the test
/// instead of going unchecked. The window is tall for that reason.
#[gpui::test]
fn every_line_the_assistant_draws_starts_in_sentence_case(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let at = Bounds::new(point(px(0.), px(0.)), size(px(WINDOW_W), px(2160.)));
    let (state, mut visual) = drawn_shell_at(cx, "assistant-casing", at);
    visual.update(|_, app| app.set_reduce_motion(true));
    let lines = drawn_lines(&state, &mut visual, PANEL_ASSISTANT, converse);
    let (rows, steps, asked) = visual.read(|app| {
        let state = state.read(app);
        let rows: Vec<String> = state
            .transcript
            .iter()
            .map(|row| row.content.to_string())
            .collect();
        let steps: Vec<String> = state
            .assistant
            .plan
            .iter()
            .map(|step| step.description.clone())
            .collect();
        let asked = state
            .assistant
            .pending_ask
            .as_ref()
            .map(|ask| ask.summary.clone());
        (rows, steps, asked)
    });
    let drawn = |text: &str| {
        lines
            .iter()
            .any(|line| line.to_lowercase() == text.to_lowercase())
    };
    for kind in AgentEntryKind::ALL {
        assert!(drawn(kind.label()), "no {kind:?} row was drawn: {lines:?}");
    }
    assert!(
        !steps.is_empty() && asked.is_some(),
        "the plan or the question is missing"
    );
    for text in rows.iter().chain(&steps).chain(&asked) {
        assert!(drawn(text), "{text:?} was not drawn: {lines:?}");
    }
    let project = visual.read(|app| project_text(state.read(app)));
    let mut offenders = Vec::new();
    check(PANEL_ASSISTANT, &lines, &project, &mut offenders);
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// A conversation with a row of every kind, then a dry run's plan and a
/// question waiting on an answer. The prompt, the reasoning and the answer are
/// typed here, as a person and a model write them; every other line comes from
/// the code that writes it in the editor.
fn converse(state: &mut AppState) {
    state.pick_ai_model("llama3").expect("choose a model");
    state.clear_selection();
    state.push_agent_message(AgentEntryKind::You, "Add a vignette to the first clip");
    state.fold_beat(Beat::Reasoning("The first clip is clip 1".into()));
    let effect = WireCommand::AddEffect(AddEffect {
        clip: 1,
        effect: "vignette".into(),
    });
    let updated = EditOutcome::Updated(ClipId::from_raw(1));
    state.fold_beat(Beat::Action(describe_action(&effect, Some(&updated))));
    state.fold_beat(Beat::Text("Added it".into()));
    // The reason `aria-ai` gives for a turn Stop ended, after one edit.
    state.finish_agent_turn(Some(Finished {
        text: String::new(),
        status: PromptStatus::Aborted("cancelled".into()),
        messages: Vec::new(),
        edits: 1,
        plan: Vec::new(),
    }));
    let marker = WireCommand::AddMarker(AddMarker {
        at: 1.0,
        name: None,
        color: None,
    });
    state.assistant.plan = vec![PlanStep {
        description: describe_action(&marker, None),
        command: marker,
        outcome: None,
    }]
    .into();
    let (reply, _) = crossbeam_channel::bounded(1);
    state.fold_beat(Beat::Ask(PendingAsk {
        name: "system_clear_cache".into(),
        summary: HostRequest::ClearCache {
            key: "preview".into(),
        }
        .summary(),
        reply,
    }));
}

/// Place a media clip, a title and a shape, pick the media in the pool and
/// read a frame for the Info panel. Returns every clip with its kind, the media
/// clip first.
fn dress(state: &mut AppState) -> Vec<(ClipId, UiClipKind)> {
    let still = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
    let media = state.import_media(still).expect("import the still");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place the still");
    state
        .add_generated(Generator::text("Title"))
        .expect("place a title");
    state
        .add_generated(Generator::shape(Shape::Rectangle, [255, 255, 255, 255]))
        .expect("place a shape");
    state.select_pool_item(media.raw());
    let mut clips: Vec<(ClipId, UiClipKind)> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| (ClipId::from_raw(clip.id), clip.kind))
        .collect();
    clips.sort_by_key(|(_, kind)| *kind != UiClipKind::Media);
    for kind in [UiClipKind::Media, UiClipKind::Text, UiClipKind::Shape] {
        assert!(
            clips.iter().any(|(_, placed)| *placed == kind),
            "no {kind:?} clip was placed: {clips:?}"
        );
    }
    state.select_clip(clips[0].0);
    state.frame_stats = aria_render::FrameStats {
        decode_ms: 12.5,
        raster_ms: 3.25,
        composite_ms: 1.5,
        effect_passes: 2,
        effect_pass_bind_groups: 2,
    };
    clips
}

/// Dock `panel` alone into the launch layout's right frame, apply `then`, draw
/// twice and return every line drawn in the frame.
///
/// The second draw is the one read: the first records the frame's rect, which
/// some panels read to lay themselves out.
fn drawn_lines(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    panel: PanelId,
    then: impl FnOnce(&mut AppState),
) -> Vec<String> {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.dock_tree = DockTree::editing_for_width(WINDOW_W);
            state.dock_tree.dock(panel, FrameId::RIGHT, Zone::Center);
            then(state);
            cx.notify();
        })
    });
    visual.run_until_parked();
    visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
    visual.run_until_parked();
    let rect = visual.read(|app| {
        state
            .read(app)
            .dock_frame_rects
            .lock()
            .get(&FrameId::RIGHT)
            .copied()
    });
    let Some([x, y, w, h]) = rect else {
        panic!("{panel}: the right frame was not drawn");
    };
    let frame = Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
    visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .filter(|run| !drawn::is_marker(run) && frame.contains(&run.bounds.center()))
            .map(|run| run.text.to_string())
            .collect()
    })
}

/// Record every line of `lines` that starts in lower case and is not text the
/// project supplies.
fn check(what: &str, lines: &[String], project: &HashSet<String>, offenders: &mut Vec<String>) {
    for line in lines {
        if project.iter().any(|text| line.starts_with(text.as_str())) {
            continue;
        }
        if let Some(word) = lowercase_start(line) {
            offenders.push(format!("{what}: {line:?} starts with {word:?}"));
        }
    }
}
