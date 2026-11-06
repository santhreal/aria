//! The Settings pickers stand on the section grid.
//!
//! The class closed here: a list of choices drawn at its own inset and pitch.
//! The Appearance choices were 30 px rows whose labels stood 8 px right of the
//! section title and of every other settings row's label.
//!
//! The Appearance choices are read with the Playback rows below them, so a
//! picker row and a labelled control row are held to one edge. Not covered:
//! the Canvas pickers, which need a project's canvas, and the glyphs.

use aria_settings::{AiApiProtocol, AiSource, Autonomy, ReasoningSummary, ThemeChoice};
use gpui::{Modifiers, TestAppContext, TextRunLayout, VisualTestContext, px, size};

use crate::app::DialogKind;
use crate::app::settings_form::{SettingsField, SettingsSection};
use crate::appearance::AppearanceMode;
use crate::cloud::{BYOK_PROVIDERS, provider_label};
use crate::keys::keyboard_image::{self, CapKind};
use crate::motion::{ControlId, MotionPreference};
use crate::settings::MIN_WINDOW_WIDTH;
use crate::test_support::{bounds_inside, drawn_shell};
use crate::theme::Theme;
const EPS: f32 = 0.51;

/// The topmost run inside `card` that reads `text`.
fn run<'a>(
    runs: &'a [TextRunLayout],
    card: gpui::Bounds<gpui::Pixels>,
    text: &str,
) -> &'a TextRunLayout {
    runs.iter()
        .filter(|run| card.contains(&run.bounds.center()) && run.text.as_ref() == text)
        .min_by(|a, b| a.bounds.top().partial_cmp(&b.bounds.top()).unwrap())
        .unwrap_or_else(|| panic!("the Settings dialog draws `{text}`"))
}

/// The label of the picker row that reads `label`. A value row above a group
/// reads the choice in force too, so a picker row is found by its label's
/// selector rather than by its text.
fn picker(visual: &mut VisualTestContext, label: &str) -> gpui::Bounds<gpui::Pixels> {
    visual
        .debug_bounds(crate::interned::intern(&format!("picker-label-{label}")))
        .unwrap_or_else(|| panic!("the Settings dialog draws no `{label}` row"))
}

#[gpui::test]
fn settings_pickers_stand_on_the_section_grid(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "settings-rows");
    visual.simulate_resize(size(px(MIN_WINDOW_WIDTH), px(1600.0)));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(DialogKind::Settings);
            cx.notify();
        });
    });
    visual.run_until_parked();
    let card = visual
        .debug_bounds("dialog-card")
        .expect("the Settings dialog draws a card");
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let title = run(&runs, card, "Appearance");
    let choices = ["System", "Light", "Dark"].map(|text| picker(visual, text));
    let control = run(&runs, card, "Audio scrubbing");
    for (text, left) in ["System", "Light", "Dark"]
        .into_iter()
        .zip(choices.map(|bounds| bounds.left()))
        .chain([("Audio scrubbing", control.bounds.left())])
    {
        assert!(
            (left - title.bounds.left()).abs() < px(EPS),
            "`{text}` starts at {left:?}, the section title at {:?}",
            title.bounds.left()
        );
    }
    let pitch = px(Theme::ROW_HEIGHT + Theme::SPACE_XS);
    for pair in choices.windows(2) {
        let step = pair[1].top() - pair[0].top();
        assert!(
            (step - pitch).abs() < px(EPS),
            "a mode row follows the one above it by {step:?}, not one row and the section gap"
        );
    }
}

/// WHY: The Appearance card holds four independent single-choice groups
/// (appearance mode, accent family, theme file, motion preference). Without a
/// value row and a separator before each group, all twelve rows stand at one
/// pitch with four rows lit at once and read as one multi-select list.
///
/// Derives each group's labels from its registry at run time and asserts both
/// the intra-group row pitch and, across every group boundary, the step over
/// the hairline and the next group's value row. Does not catch separator
/// colour, which the test renderer does not rasterize.
#[gpui::test]
fn appearance_card_separates_its_four_choice_groups(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "settings-appearance-groups");
    visual.simulate_resize(size(px(MIN_WINDOW_WIDTH), px(1600.0)));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(DialogKind::Settings);
            cx.notify();
        });
    });
    visual.run_until_parked();
    let groups: [Vec<String>; 4] = [
        AppearanceMode::ALL
            .into_iter()
            .map(|mode| mode.label().to_string())
            .collect(),
        ThemeChoice::ALL
            .into_iter()
            .map(|choice| choice.label().to_string())
            .collect(),
        vec![super::pickers::SHIPPED_THEME.to_string()],
        MotionPreference::ALL
            .into_iter()
            .map(|pref| pref.label().to_string())
            .collect(),
    ];

    let intra_pitch = px(Theme::ROW_HEIGHT + Theme::SPACE_XS);
    let divider_step = px(1.0 + 2.0 * Theme::SPACE_XXS + Theme::SPACE_XS);
    let inter_pitch = intra_pitch + divider_step + intra_pitch;

    for group in &groups {
        let tops: Vec<gpui::Pixels> = group
            .iter()
            .map(|label| picker(visual, label).top())
            .collect();
        for (pair, names) in tops.windows(2).zip(group.windows(2)) {
            let step = pair[1] - pair[0];
            assert!(
                (step - intra_pitch).abs() < px(EPS),
                "intra-group `{}` -> `{}` is {step:?}, expected {intra_pitch:?}",
                names[0],
                names[1]
            );
        }
    }

    for pair in groups.windows(2) {
        let last = pair[0].last().expect("non-empty group");
        let first = pair[1].first().expect("non-empty group");
        let step = picker(visual, first).top() - picker(visual, last).top();
        assert!(
            (step - inter_pitch).abs() < px(EPS),
            "inter-group `{last}` -> `{first}` is {step:?}, expected {inter_pitch:?} \
             over the hairline and the value row"
        );
    }
}

/// WHY: Settings controls must show user-facing labels rather than persisted
/// config identifiers (`local`, `chat_completions`, `pexels`), provider rows
/// must state an unset key once rather than `"no key (not set)"`, no drawn row
/// may use `"(s)"` pseudo-plurals, placeholders must be sentence-cased, and the
/// footer `Done` action must render through `components::button` rather than as
/// borderless ghost text.
///
/// Enumerates `AiSource::ALL`, `AiApiProtocol::ALL`, `ReasoningSummary::ALL`,
/// `Autonomy::ALL`, `BYOK_PROVIDERS`, and `SettingsField::ALL` at run time.
#[gpui::test]
fn settings_controls_draw_display_labels_sentence_placeholders_and_button_footer(
    cx: &mut TestAppContext,
) {
    let (state, visual) = drawn_shell(cx, "settings-chrome-labels");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.simulate_resize(size(px(MIN_WINDOW_WIDTH), px(3600.0)));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(DialogKind::Settings);
            cx.notify();
        });
    });
    visual.run_until_parked();
    let card = visual
        .debug_bounds("dialog-card")
        .expect("the Settings dialog draws a card");
    let header = visual
        .debug_bounds("dialog-header")
        .expect("the Settings dialog draws a header");
    let footer = visual
        .debug_bounds("dialog-footer")
        .expect("the Settings dialog draws a footer");
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let card_runs: Vec<&TextRunLayout> = runs
        .iter()
        .filter(|r| card.contains(&r.bounds.center()) && !r.text.is_empty())
        .collect();

    let has_run = |bounds: gpui::Bounds<gpui::Pixels>, text: &str| {
        runs.iter()
            .any(|r| bounds.contains(&r.bounds.center()) && r.text.as_ref() == text)
    };
    let mut faults: Vec<String> = Vec::new();

    let segmented: Vec<(&str, &str)> = AiSource::ALL
        .into_iter()
        .map(|v| (v.key(), v.label()))
        .chain(AiApiProtocol::ALL.into_iter().map(|v| (v.key(), v.label())))
        .chain(
            ReasoningSummary::ALL
                .into_iter()
                .map(|v| (v.key(), v.label())),
        )
        .chain(Autonomy::ALL.into_iter().map(|v| (v.key(), v.label())))
        .collect();
    for (persisted, display) in segmented {
        if display == persisted {
            faults.push(format!(
                "segmented option `{persisted}` uses its raw key as display label"
            ));
        }
        if !display.starts_with(|ch: char| ch.is_ascii_uppercase()) {
            faults.push(format!(
                "segmented option `{display}` does not start uppercase"
            ));
        }
        if display.contains('_') {
            faults.push(format!(
                "segmented option `{display}` exposes an underscore"
            ));
        }
        if !has_run(card, display) {
            faults.push(format!("segmented display label `{display}` was not drawn"));
        }
        if card_runs.iter().any(|r| r.text.as_ref() == persisted) {
            faults.push(format!(
                "raw persisted identifier `{persisted}` was drawn in the Settings card"
            ));
        }
    }

    for (raw_id, purpose) in BYOK_PROVIDERS {
        let label = provider_label(raw_id);
        if label == "Unknown" {
            faults.push(format!("provider `{raw_id}` has no product label"));
        }
        if !has_run(card, label) {
            faults.push(format!("provider product label `{label}` was not drawn"));
        }
        if label != raw_id && card_runs.iter().any(|r| r.text.as_ref() == raw_id) {
            faults.push(format!(
                "raw provider id `{raw_id}` was drawn instead of `{label}`"
            ));
        }
        let expected_value = format!("{purpose} · Not set");
        if !has_run(card, &expected_value) {
            faults.push(format!(
                "provider status `{expected_value}` was not drawn for `{label}`"
            ));
        }
    }
    for r in &card_runs {
        if r.text.contains("no key (not set)") {
            faults.push(format!("redundant provider status drawn: `{}`", r.text));
        }
        if r.text.contains("(s)") {
            faults.push(format!(
                "unpluralized `(s)` drawn in Settings: `{}`",
                r.text
            ));
        }
    }

    visual.read(|app| {
        let s = state.read(app);
        for field in SettingsField::ALL {
            let look = s
                .setting_look(field)
                .unwrap_or_else(|| panic!("shipped field {field:?} has no look"));
            if !look
                .placeholder
                .starts_with(|ch: char| ch.is_ascii_uppercase())
            {
                faults.push(format!(
                    "field {field:?} placeholder `{}` is not sentence-cased",
                    look.placeholder
                ));
            }
        }
    });
    if !has_run(header, "Type to narrow the sections") {
        faults.push(
            "filter placeholder `Type to narrow the sections` was not drawn in header".into(),
        );
    }

    let done = run(&runs, footer, "Done");
    let done_button_key = ControlId::Fixed("settings-done").motion("button");
    visual.simulate_mouse_move(done.bounds.center(), None, Modifiers::default());
    visual.run_until_parked();
    if !(crate::motion::hover_t(done_button_key) > 0.0) {
        faults
            .push("Done in Settings footer did not register hover on `components::button`".into());
    }
    visual.simulate_click(done.bounds.center(), Modifiers::default());
    visual.run_until_parked();
    if visual.read(|app| state.read(app).dialog) != DialogKind::None {
        faults.push("clicking Done did not close Settings".into());
    }
    assert!(
        faults.is_empty(),
        "Settings chrome faults:\n  {}",
        faults.join("\n  ")
    );
}

/// WHY: Every keyboard cap label in `keyboard_image` must measure inside its
/// cap with horizontal and vertical padding at each width the dialog draws.
/// Previously `"Home"`, `"PgUp"`, `"PgDn"`, `"Num +"`, and `"Super"` overflowed
/// or touched their cap borders.
///
/// Enumerates every cap from `keyboard_image::rows()` at run time across window
/// widths down to the card's minimum clearance width.
#[gpui::test]
fn every_keyboard_cap_label_fits_inside_its_cap_across_dialog_widths(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "settings-kbd-cap-fit");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(DialogKind::Settings);
            state.settings_form.filter = SettingsSection::KeyboardShortcuts.title().into();
            cx.notify();
        });
    });
    crate::test_support::wait_out_splash(visual);

    let caps: Vec<(usize, keyboard_image::Cap)> = keyboard_image::rows()
        .flat_map(|row| row.iter().copied())
        .enumerate()
        .collect();
    assert!(!caps.is_empty(), "keyboard image has no caps");

    let h_pad = px(Theme::SPACE_XXS);
    let v_pad = px(1.0);
    for width in [1600.0_f32, 1280.0, MIN_WINDOW_WIDTH, 544.0] {
        visual.simulate_resize(size(px(width), px(1200.0)));
        visual.run_until_parked();
        let card = visual
            .debug_bounds("dialog-card")
            .expect("Settings draws a card");
        let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
        for &(index, cap) in &caps {
            if matches!(cap.kind, CapKind::Gap) {
                continue;
            }
            let cap_bounds = visual
                .debug_bounds(crate::interned::intern(&format!(
                    "settings-kbd-cap-{index}"
                )))
                .unwrap_or_else(|| {
                    panic!("{width} px: cap {index} (`{}`) was not drawn", cap.label)
                });
            assert!(
                bounds_inside(cap_bounds, card),
                "{width} px: cap {index} (`{}`) {cap_bounds:?} lies outside card {card:?}",
                cap.label
            );
            let label_run = runs
                .iter()
                .find(|r| {
                    cap_bounds.contains(&r.bounds.center()) && r.text.as_ref() == cap.label
                })
                .unwrap_or_else(|| {
                    panic!(
                        "{width} px: cap {index} (`{}`) drew no matching text run inside {cap_bounds:?}",
                        cap.label
                    )
                });
            assert!(
                label_run.bounds.left() >= cap_bounds.left() + h_pad - px(EPS)
                    && label_run.bounds.right() <= cap_bounds.right() - h_pad + px(EPS),
                "{width} px: cap `{}` text {:?} overflows or touches horizontal cap edges {:?} (min pad {h_pad:?})",
                cap.label,
                label_run.bounds,
                cap_bounds
            );
            assert!(
                label_run.bounds.top() >= cap_bounds.top() + v_pad - px(EPS)
                    && label_run.bounds.bottom() <= cap_bounds.bottom() - v_pad + px(EPS),
                "{width} px: cap `{}` text {:?} overflows or touches vertical cap edges {:?}",
                cap.label,
                label_run.bounds,
                cap_bounds
            );
        }
    }
}

/// WHY: The Keymap row drew its own label column and spaced its first item
/// `SPACE_XS` from it, where every other labelled row spaces its control
/// `SPACE_SM`, so the set chips started 4 px left of the Set name box under
/// them, and a second line of chips wrapped back under the label.
///
/// Reads the Keyboard Shortcuts section under its title as the filter: the
/// row's first item, with no saved sets the line saying so, stands at a chip's
/// inset on the column the set name box below it starts on. Not covered: the
/// row's second line of chips, which needs more saved sets than fit one line.
#[gpui::test]
fn the_keymap_row_starts_on_the_column_of_the_box_below_it(cx: &mut TestAppContext) {
    let (card, runs, shown) = drawn_under_shortcuts(cx, "settings-keymap-column");
    let first = run(&runs, card, "No saved sets");
    let set_name = run(&runs, card, &shown);
    // A text box's text stands inside its 1 px border and its padding, and a
    // chip's inside its padding.
    let column = set_name.bounds.left() - px(1.0 + Theme::SPACE_SM);
    assert!(
        (first.bounds.left() - (column + px(Theme::SPACE_SM))).abs() < px(EPS),
        "the Keymap row's text starts at {:?}, a chip's on the column of the set \
         name box below it at {:?}",
        first.bounds.left(),
        column + px(Theme::SPACE_SM)
    );
}

/// WHY: The filter row stood on the dialog's edge while the section cards
/// below it inset their rows by their padding and border, so the filter box
/// started 9 px left of every text box under it, on a dialog whose label
/// column exists to line those boxes up.
///
/// Reads the Keyboard Shortcuts section under its title as the filter: the
/// filter's text starts where the Set name box's text starts, so the two boxes
/// share a column. Not covered: the right edges of the boxes, which no text
/// run reaches.
#[gpui::test]
fn the_filter_box_starts_on_the_column_of_the_boxes_below_it(cx: &mut TestAppContext) {
    let (card, runs, shown) = drawn_under_shortcuts(cx, "settings-filter-column");
    // The header stands above the sections, so the topmost run of the title is
    // the filter's own text and not the section title.
    let filter = run(&runs, card, SettingsSection::KeyboardShortcuts.title());
    let set_name = run(&runs, card, &shown);
    assert!(
        (filter.bounds.left() - set_name.bounds.left()).abs() < px(EPS),
        "the filter's text starts at {:?} and the Set name box's at {:?}",
        filter.bounds.left(),
        set_name.bounds.left()
    );
}

/// WHY: A row that reads a value spread its label and value to the two ends
/// of the row, so the AI section's Status sentence ended on the card's right
/// edge while the chips and boxes above it start on the control column, and
/// every read-only value of the Storage, Caches, Plugins, Configuration and
/// Canvas sections stood the same way.
///
/// Reads the AI section alone: the Status value starts at a chip's inset on
/// the column the Source chips start on. Every read-only row is the one
/// `kv` widget, so the row read here stands for each. A value too long for one
/// line is read in `label_line_tests`.
#[gpui::test]
fn a_read_only_value_starts_on_the_column_of_the_controls(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "settings-value-column");
    let card = opened_on(&state, visual, SettingsSection::Ai.title());
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let chip = run(&runs, card, AiSource::ALL[0].label());
    let label = run(&runs, card, "Status");
    let value = runs
        .iter()
        .filter(|run| {
            card.contains(&run.bounds.center())
                && run.bounds.left() > label.bounds.right()
                && run.bounds.top() < label.bounds.bottom()
                && run.bounds.bottom() > label.bounds.top()
        })
        .min_by(|a, b| a.bounds.left().partial_cmp(&b.bounds.left()).unwrap())
        .expect("the Status row draws its value beside its label");
    assert!(
        (value.bounds.left() - chip.bounds.left()).abs() < px(EPS),
        "the Status value `{}` starts at {:?}, the Source chips' text at {:?}",
        value.text,
        value.bounds.left(),
        chip.bounds.left()
    );
}

/// WHY: The Keyboard Shortcuts list drew a Clear button beside every row that
/// holds a key, a column of identical boxes down the section, and its rows and
/// the Workspaces rows stood their text 4 px right of the hint line above
/// them, where every picker row steps out to the section's edge.
///
/// Reads the shortcuts listed under one binding's name as the filter and the
/// Workspaces section: no Clear at rest, Clear on the row under the pointer
/// alone, Clear on the armed row with the pointer away, and each list's row
/// text on the edge of its hint line. Every such row is the one `row_plate`,
/// so the rows read here stand for each. Not covered: Revert, which draws at
/// rest on a row whose key differs from the shipped one, and the frames of the
/// fade between rest and lit.
#[gpui::test]
fn a_settings_list_row_stands_on_the_section_edge_and_shows_clear_for_the_pointer(
    cx: &mut TestAppContext,
) {
    let (state, visual) = drawn_shell(cx, "settings-list-rows");
    visual.update(|_, app| app.set_reduce_motion(true));
    let label = visual.read(|app| {
        let listing = state
            .read(app)
            .listed_binding_rows()
            .expect("an empty Settings filter lists every binding");
        listing
            .rows
            .into_iter()
            .find(|(row, _)| row.chord.is_some() && !row.overridden)
            .map(|(_, label)| label)
            .expect("a binding holds its shipped key")
    });
    let card = opened_on(&state, visual, &label);
    let clears = |visual: &mut VisualTestContext| -> Vec<gpui::Bounds<gpui::Pixels>> {
        visual.update(|window, _| {
            window
                .rendered_text_runs()
                .iter()
                .filter(|run| card.contains(&run.bounds.center()) && run.text.as_ref() == "Clear")
                .map(|run| run.bounds)
                .collect()
        })
    };
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let hint = run(&runs, card, "Click a row, then press the key you want").bounds;
    let row = run_below(&runs, card, hint, &label).bounds;
    assert!(
        (row.left() - hint.left()).abs() < px(EPS),
        "the `{label}` row's text starts at {:?}, the hint above it at {:?}",
        row.left(),
        hint.left()
    );
    let at_rest = clears(visual);
    assert!(at_rest.is_empty(), "at rest Clear is drawn at {at_rest:?}");

    let on_row = |clear: &[gpui::Bounds<gpui::Pixels>]| {
        clear.len() == 1 && clear[0].center().y > row.top() && clear[0].center().y < row.bottom()
    };
    visual.simulate_mouse_move(row.center(), None, Modifiers::default());
    visual.run_until_parked();
    let hovered = clears(visual);
    assert!(
        on_row(&hovered),
        "the pointer on the `{label}` row at {row:?} draws Clear at {hovered:?}"
    );

    visual.simulate_click(row.center(), Modifiers::default());
    visual.run_until_parked();
    visual.simulate_mouse_move(hint.center(), None, Modifiers::default());
    visual.run_until_parked();
    assert!(
        visual.read(|app| state.read(app).rebinding().is_some()),
        "a click on the `{label}` row arms it"
    );
    let armed = clears(visual);
    assert!(
        on_row(&armed),
        "the armed `{label}` row at {row:?}, the pointer away, draws Clear at {armed:?}"
    );

    visual.update(|_, app| state.update(app, |state, _| state.cancel_rebind()));
    let card = opened_on(&state, visual, SettingsSection::Workspaces.title());
    let name = visual.read(|app| state.read(app).workspace_rows()[0].name);
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let hint = run(
        &runs,
        card,
        "Click a workspace to arrange the panes that way",
    )
    .bounds;
    let first = run_below(&runs, card, hint, name).bounds;
    assert!(
        (first.left() - hint.left()).abs() < px(EPS),
        "the `{name}` workspace row's text starts at {:?}, the hint above it at {:?}",
        first.left(),
        hint.left()
    );
}

/// WHY: Each Keyboard Shortcuts row padded its one control-tall line by
/// `SPACE_XS` above and below, so the list stood at a 36 px pitch beside the
/// pickers' 26, where 28 holds a row and its Clear.
///
/// Reads the shortcuts listed under `Shuttle` as the filter: each row's name
/// stands one control height and the section's gap below the row before it.
/// Not covered: a row asking Move or Keep, which draws two lines, and the
/// whole list, which draws the keyboard image above the rows.
#[gpui::test]
fn the_shortcut_rows_stand_one_control_apart(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "settings-key-pitch");
    let card = opened_on(&state, visual, "Shuttle");
    let labels: Vec<String> = visual.read(|app| {
        let listing = state
            .read(app)
            .listed_binding_rows()
            .expect("`Shuttle` lists the shuttle bindings");
        listing.rows.into_iter().map(|(_, label)| label).collect()
    });
    assert!(labels.len() >= 3, "`Shuttle` lists {labels:?}");
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let mut tops: Vec<f32> = runs
        .iter()
        .filter(|run| {
            card.contains(&run.bounds.center())
                && labels
                    .iter()
                    .any(|label| label.as_str() == run.text.as_ref())
        })
        .map(|run| f32::from(run.bounds.top()))
        .collect();
    tops.sort_by(f32::total_cmp);
    assert_eq!(
        tops.len(),
        labels.len(),
        "every listed row draws its name once"
    );
    let pitch = Theme::CONTROL_HEIGHT + Theme::SPACE_XS;
    for pair in tops.windows(2) {
        assert!(
            ((pair[1] - pair[0]) - pitch).abs() < EPS,
            "shortcut rows stand {} px apart, not one control and the gap, {pitch} px: {tops:?}",
            pair[1] - pair[0]
        );
    }
}

/// WHY: Each Workspaces row drew its pane sizes twice, as the line under its
/// name and again at its right edge, so the list read every number twice.
///
/// Counts, over every row `workspace_rows` lists, the drawn runs holding a
/// row's sizes (each holds ` · Timeline `) and the runs reading "In use": one
/// sizes line per row, and one "In use" per row on screen. Not covered: a
/// plugin's name ahead of the sizes, which shares the one line.
#[gpui::test]
fn a_workspace_row_reads_its_pane_sizes_once(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "settings-workspace-sizes");
    let card = opened_on(&state, visual, SettingsSection::Workspaces.title());
    let (rows, active) = visual.read(|app| {
        let rows = state.read(app).workspace_rows();
        let active = rows.iter().filter(|row| row.active).count();
        (rows.len(), active)
    });
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let count = |matches: &dyn Fn(&str) -> bool| {
        runs.iter()
            .filter(|run| card.contains(&run.bounds.center()) && matches(run.text.as_ref()))
            .count()
    };
    assert_eq!(
        count(&|text| text.contains(" \u{b7} Timeline ")),
        rows,
        "{rows} workspace rows draw their pane sizes once each"
    );
    assert_eq!(
        count(&|text| text == "In use"),
        active,
        "{active} workspace rows are on screen"
    );
}

/// The topmost run inside `card` below `above` that reads `text`.
fn run_below<'a>(
    runs: &'a [TextRunLayout],
    card: gpui::Bounds<gpui::Pixels>,
    above: gpui::Bounds<gpui::Pixels>,
    text: &str,
) -> &'a TextRunLayout {
    runs.iter()
        .filter(|run| {
            card.contains(&run.bounds.center())
                && run.bounds.top() >= above.bottom()
                && run.text.as_ref() == text
        })
        .min_by(|a, b| a.bounds.top().partial_cmp(&b.bounds.top()).unwrap())
        .unwrap_or_else(|| panic!("the Settings dialog draws `{text}` below {above:?}"))
}

/// The Settings dialog opened with `filter`, in place of any earlier one,
/// typed into its filter box and drawn: the dialog's card.
fn opened_on(
    state: &gpui::Entity<crate::app::AppState>,
    visual: &mut VisualTestContext,
    filter: &str,
) -> gpui::Bounds<gpui::Pixels> {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(DialogKind::Settings);
            state.settings_form.filter.clear();
            state.focus_setting(Some(SettingsField::Filter));
            for ch in filter.chars() {
                state.type_key(ch);
            }
            cx.notify();
        });
    });
    visual.run_until_parked();
    visual
        .debug_bounds("dialog-card")
        .expect("the Settings dialog draws a card")
}

/// The Settings dialog drawn under the Keyboard Shortcuts title as the filter:
/// its card, its text runs, and the text the Set name box shows, its value or
/// else its placeholder.
fn drawn_under_shortcuts(
    cx: &mut TestAppContext,
    name: &str,
) -> (gpui::Bounds<gpui::Pixels>, Vec<TextRunLayout>, String) {
    let (state, visual) = drawn_shell(cx, name);
    let card = opened_on(&state, visual, SettingsSection::KeyboardShortcuts.title());
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let shown = visual.read(|app| {
        let state = state.read(app);
        let look = state
            .setting_look(SettingsField::KeymapName)
            .expect("the set name field has a look");
        let (value, _) = state.setting_draft(SettingsField::KeymapName);
        if value.is_empty() {
            look.placeholder.to_string()
        } else {
            value
        }
    });
    (card, runs, shown)
}
