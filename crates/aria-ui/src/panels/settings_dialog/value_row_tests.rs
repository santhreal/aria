//! A Settings value with actions stands on the section grid.
//!
//! The class closed here: a row that builds its own line instead of the
//! label column and the controls column. Each download timeout drew
//! "Connect timeout: 15s" as one run with its Halve and Double after it, so
//! the steppers of the three rows stood at three different x positions, and
//! the two WASM limit rows and the download quota did the same. A missing
//! model's Install, Clear analysis, and every line of section and cache
//! actions started on the label column, Install and Clear analysis filling
//! the line. In the AI section, Reveal agent files filled a line of its own
//! under the folder it reveals, and Test connection and List models started
//! on the label column.
//!
//! Opens the Transcription, Plugins, Storage, Caches and AI sections in the
//! drawn shell and reads every value and action back from the text the dialog
//! draws: each row's label starts on the section title's edge and its value
//! on the column the read-only values start on; the actions at a row's end
//! end on one edge, and a line of actions of its own starts on the values'
//! column. The rows are enumerated from `TimeoutField::ALL`,
//! `WasmLimit::ALL`, the model catalog and the cache registry, so a new
//! timeout, limit, model or cache is held to the grid on the day it is added.
//!
//! Not covered: the plugin rows' switches, which draw no text, and a cache
//! row's Use default, which a fresh session does not draw.

use gpui::{
    Bounds, CursorStyle, Modifiers, Pixels, TestAppContext, TextRunLayout, VisualTestContext, px,
};

use crate::app::models::TimeoutField;
use crate::app::plugin_install::WasmLimit;
use crate::app::settings_form::SettingsSection;
use crate::test_support::drawn_shell;

const EPS: f32 = 0.51;

/// Every run inside `card` that reads `text`.
fn runs<'a>(all: &'a [TextRunLayout], card: Bounds<Pixels>, text: &str) -> Vec<Bounds<Pixels>> {
    all.iter()
        .filter(|run| card.contains(&run.bounds.center()) && run.text.as_ref() == text)
        .map(|run| run.bounds)
        .collect()
}

/// The one run inside `card` that reads `text`, or its first wrapped line when
/// a long path wraps after a space.
fn one(all: &[TextRunLayout], card: Bounds<Pixels>, text: &str) -> Bounds<Pixels> {
    let exact = runs(all, card, text);
    if let [bounds] = exact.as_slice() {
        return *bounds;
    }
    let wrapped: Vec<Bounds<Pixels>> = all
        .iter()
        .filter(|run| {
            let line = run.text.trim_end();
            card.contains(&run.bounds.center())
                && !line.is_empty()
                && text
                    .strip_prefix(line)
                    .is_some_and(|rest| rest.starts_with(' '))
        })
        .map(|run| run.bounds)
        .collect();
    if let [bounds] = wrapped.as_slice() {
        return *bounds;
    }
    panic!(
        "the Settings dialog draws `{text}` {} times, not once",
        exact.len()
    );
}

/// The left edge of `section`'s title: its lowest run, as the filter box
/// above the cards reads the same title.
fn title_edge(all: &[TextRunLayout], card: Bounds<Pixels>, section: SettingsSection) -> Pixels {
    runs(all, card, section.title())
        .into_iter()
        .max_by(|a, b| {
            a.top()
                .partial_cmp(&b.top())
                .expect("a drawn run has a position")
        })
        .unwrap_or_else(|| panic!("the Settings dialog draws `{}`", section.title()))
        .left()
}

/// Open Settings at `section` and return the dialog card and its text runs.
fn drawn(
    state: &gpui::Entity<crate::app::AppState>,
    visual: &mut VisualTestContext,
    section: SettingsSection,
) -> (Bounds<Pixels>, Vec<TextRunLayout>) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_settings_at(section);
            cx.notify();
        });
    });
    visual.run_until_parked();
    let card = visual
        .debug_bounds("dialog-card")
        .expect("the Settings dialog draws a card");
    let all = visual.update(|window, _| window.rendered_text_runs().to_vec());
    (card, all)
}

/// Every bound in `found` starts (or ends, with `right`) on one edge, and
/// there are `count` of them.
fn one_edge(found: &[Bounds<Pixels>], count: usize, right: bool, what: &str) -> Pixels {
    assert_eq!(found.len(), count, "{what}: {found:?}");
    let edge = |b: &Bounds<Pixels>| if right { b.right() } else { b.left() };
    let first = edge(&found[0]);
    for bounds in found {
        assert!(
            (edge(bounds) - first).abs() < px(EPS),
            "{what} stand on {:?} and {first:?}, not one edge",
            edge(bounds)
        );
    }
    first
}

/// The label starts on the section title's edge and the value on `column`.
fn on_grid(
    all: &[TextRunLayout],
    card: Bounds<Pixels>,
    title: Pixels,
    column: Pixels,
    (label, value): (&str, &str),
) {
    let label_at = one(all, card, label);
    assert!(
        (label_at.left() - title).abs() < px(EPS),
        "`{label}` starts at {:?}, the section title at {title:?}",
        label_at.left()
    );
    let value_at = one(all, card, value);
    assert!(
        (value_at.left() - column).abs() < px(EPS),
        "`{label}`'s value `{value}` starts at {:?}, the values' column at {column:?}",
        value_at.left()
    );
}

/// The text of `first`, the first action of a line of actions, starts on
/// `column`, as a button's label does when the button starts on the controls
/// column.
fn on_column(all: &[TextRunLayout], card: Bounds<Pixels>, column: Pixels, first: &str) {
    let at = one(all, card, first).left();
    assert!(
        (at - column).abs() < px(EPS),
        "`{first}` starts at {at:?}, the values' column at {column:?}"
    );
}

#[gpui::test]
fn settings_values_with_actions_stand_on_the_section_grid(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "vrow");
    // Transcription: three timeout steppers and an Install per missing model.
    let (card, all) = drawn(&state, visual, SettingsSection::Transcription);
    let timeouts = TimeoutField::ALL.len();
    one_edge(
        &runs(&all, card, "Halve"),
        timeouts,
        false,
        "the timeouts' Halves",
    );
    let doubles = one_edge(
        &runs(&all, card, "Double"),
        timeouts,
        true,
        "the timeouts' Doubles",
    );
    let (models, settings) = visual.update(|_, app| {
        (
            state.read(app).model_rows(),
            state.read(app).model_timeouts(),
        )
    });
    let missing = models.iter().filter(|row| row.status != "Ready").count();
    assert!(
        missing > 0,
        "a fresh session has a missing model to install"
    );
    let installs = runs(&all, card, "Install");
    let ends = one_edge(&installs, missing, true, "the models' Installs");
    assert!(
        (ends - doubles).abs() < px(EPS),
        "the Installs end at {ends:?}, the Doubles at {doubles:?}"
    );
    let title = title_edge(&all, card, SettingsSection::Transcription);
    let first = &models[0];
    let column = all
        .iter()
        .find(|run| card.contains(&run.bounds.center()) && run.text.starts_with(first.description))
        .unwrap_or_else(|| panic!("the Settings dialog draws `{}`", first.description))
        .bounds
        .left();
    for (field, _, label) in TimeoutField::ALL {
        let value = format!("{} s", field.secs(&settings));
        on_grid(&all, card, title, column, (label, &value));
    }

    // Plugins: two limit steppers, their values on the plugin folder's column.
    let (card, all) = drawn(&state, visual, SettingsSection::Plugins);
    let limits = WasmLimit::ALL.len();
    one_edge(
        &runs(&all, card, "Halve"),
        limits,
        false,
        "the limits' Halves",
    );
    one_edge(
        &runs(&all, card, "Double"),
        limits,
        true,
        "the limits' Doubles",
    );
    let (held, folder) = visual.update(|_, app| {
        let state = state.read(app);
        let folder = state.plugin_installs.installed.dir().display().to_string();
        (state.wasm_limits(), folder)
    });
    let title = title_edge(&all, card, SettingsSection::Plugins);
    let column = one(&all, card, &folder).left();
    for (limit, _, label) in WasmLimit::ALL {
        let value = limit.describe(limit.value(held));
        on_grid(&all, card, title, column, (label, &value));
    }
    on_column(&all, card, column, "Install Plugin…");

    // Storage: each action at the end of the row it acts on.
    let (card, all) = drawn(&state, visual, SettingsSection::Storage);
    let rows = visual.update(|_, app| {
        let state = state.read(app);
        let bytes = crate::app::storage::format_bytes;
        let quota = state.download_quota_mib();
        [
            (
                "Storage root",
                state.download_cache_root().display().to_string(),
            ),
            (
                "Download cache",
                format!(
                    "{} of {quota} MiB",
                    bytes(state.download_cache_size_bytes())
                ),
            ),
            ("Quota", format!("{quota} MiB")),
            (
                "Analysis cache",
                format!(
                    "{} at {}",
                    bytes(state.analysis_cache_size_bytes()),
                    state.analysis_cache_root().display()
                ),
            ),
        ]
    });
    let title = title_edge(&all, card, SettingsSection::Storage);
    let column = one(&all, card, &rows[2].1).left();
    for (label, value) in &rows {
        on_grid(&all, card, title, column, (label, value.as_str()));
    }
    let ends: Vec<_> = ["Move storage…", "Clear cache", "+512 MiB", "Clear analysis"]
        .map(|action| one(&all, card, action))
        .into();
    one_edge(&ends, ends.len(), true, "the storage actions");

    // Caches: the section's actions and each cache's on lines of their own.
    let (card, all) = drawn(&state, visual, SettingsSection::Caches);
    let on_disk = visual.update(|_, app| {
        state
            .read(app)
            .cache_rows()
            .iter()
            .filter(|row| row.on_disk())
            .count()
    });
    assert!(on_disk > 0, "the cache registry holds a cache on disk");
    let column = one(
        &all,
        card,
        "Not measured; Measure walks every cache directory",
    )
    .left();
    on_column(&all, card, column, "Measure");
    // The card scrolls: the caches under its foot draw no text in view.
    let drawn_clears = runs(&all, card, "Clear");
    assert!(
        !drawn_clears.is_empty() && drawn_clears.len() <= on_disk,
        "{} Clears drawn for {on_disk} caches on disk",
        drawn_clears.len()
    );
    let clears = one_edge(
        &drawn_clears,
        drawn_clears.len(),
        false,
        "the caches' Clears",
    );
    assert!(
        (clears - column).abs() < px(EPS),
        "the caches' Clears start at {clears:?}, the values' column at {column:?}"
    );

    // AI: Reveal agent files at the end of the Agent files row, and the
    // provider probes on a line of their own on the values' column. The
    // folder wraps, and a wrapped value is one run as tall as its lines, so
    // the action is held to the row's one-line label, not to the folder.
    let (card, all) = drawn(&state, visual, SettingsSection::Ai);
    let folder = visual.update(|_, app| state.read(app).agent_dir().display().to_string());
    let path = all
        .iter()
        .find(|run| {
            card.contains(&run.bounds.center())
                && !run.text.is_empty()
                && folder.starts_with(run.text.as_ref())
        })
        .unwrap_or_else(|| panic!("the Settings dialog draws `{folder}`"))
        .bounds;
    let label = one(&all, card, "Agent files");
    let reveal = one(&all, card, "Reveal agent files");
    assert!(
        (reveal.center().y - label.center().y).abs() < px(EPS),
        "Reveal agent files stands at {:?}, its row's label at {:?}",
        reveal.center().y,
        label.center().y
    );
    on_column(&all, card, path.left(), "Test connection");
}

/// While a download holds the cache, Move storage… and Clear cache draw
/// blocked, and a press posts the reason; once it ends they draw live.
///
/// WHY: both stood live through a download, a press on Clear reported
/// `aria-cloud`'s "protected until project media inventory completes", a press
/// on Move opened a folder picker before refusing, and a Maintenance row read
/// "Available" or "Paused while the project inventory is scanned", which named
/// a scan the app does not run. Not caught: the 40 % opacity, held by
/// `components_control_tests`.
#[gpui::test]
fn the_storage_actions_draw_blocked_exactly_while_a_download_holds_the_cache(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "held");
    for held in [false, true, false] {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                if held {
                    state.hold_download_guard();
                } else {
                    state.release_download_guard();
                }
                state.last_error = None;
                cx.notify();
            })
        });
        let (card, all) = drawn(&state, visual, SettingsSection::Storage);
        assert!(
            runs(&all, card, "Maintenance").is_empty(),
            "the Storage section draws a Maintenance row"
        );
        for action in ["Move storage…", "Clear cache"] {
            let at = one(&all, card, action).center();
            visual.simulate_mouse_move(at, None, Modifiers::none());
            visual.run_until_parked();
            let cursor = visual.update(|window, _| window.rendered_cursor_style());
            let want = if held {
                CursorStyle::Arrow
            } else {
                CursorStyle::PointingHand
            };
            assert_eq!(cursor, Some(want), "`{action}` under the hand, held {held}");
        }
        if held {
            let at = one(&all, card, "Clear cache").center();
            visual.simulate_click(at, Modifiers::none());
            visual.run_until_parked();
            let (asked, error) = visual.update(|_, app| {
                let state = state.read(app);
                (state.download_cache_held(), state.last_error.clone())
            });
            assert_eq!(error.as_deref(), asked, "a blocked Clear cache posts why");
        }
    }
}
