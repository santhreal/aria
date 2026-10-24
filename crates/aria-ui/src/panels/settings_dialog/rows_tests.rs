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
use gpui::{Modifiers, TestAppContext, TextRunLayout, px, size};

use crate::app::DialogKind;
use crate::app::settings_form::SettingsField;
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

#[gpui::test]
fn settings_pickers_stand_on_the_section_grid(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "settings-rows");
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
    let choices = ["System", "Light", "Dark"].map(|text| run(&runs, card, text));
    let control = run(&runs, card, "Audio scrubbing");
    for label in choices.iter().chain([&control]) {
        assert!(
            (label.bounds.left() - title.bounds.left()).abs() < px(EPS),
            "`{}` starts at {:?}, the section title at {:?}",
            label.text,
            label.bounds.left(),
            title.bounds.left()
        );
    }
    let pitch = px(Theme::ROW_HEIGHT + Theme::SPACE_XS);
    for pair in choices.windows(2) {
        let step = pair[1].bounds.top() - pair[0].bounds.top();
        assert!(
            (step - pitch).abs() < px(EPS),
            "`{}` follows `{}` by {step:?}, not one row and the section gap",
            pair[1].text,
            pair[0].text
        );
    }
}

/// WHY: The Appearance card holds four independent single-choice groups
/// (appearance mode, accent family, theme file, motion preference). Without a
/// separator between adjacent groups, all twelve rows stand at one pitch with
/// four rows lit at once and read as one multi-select list.
///
/// Derives each group's labels from its registry at run time and asserts both
/// the intra-group row pitch and the larger inter-group step across every group
/// boundary. Does not catch separator colour, which the test renderer does not
/// rasterize.
#[gpui::test]
fn appearance_card_separates_its_four_choice_groups(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "settings-appearance-groups");
    visual.simulate_resize(size(px(MIN_WINDOW_WIDTH), px(1200.0)));
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

    let groups: [Vec<String>; 4] = [
        AppearanceMode::ALL
            .into_iter()
            .map(|mode| mode.label().to_string())
            .collect(),
        ThemeChoice::ALL
            .into_iter()
            .map(|choice| format!("Accent: {}", choice.label()))
            .collect(),
        vec!["Theme: Shipped".to_string()],
        MotionPreference::ALL
            .into_iter()
            .map(|pref| pref.label().to_string())
            .collect(),
    ];

    let intra_pitch = px(Theme::ROW_HEIGHT + Theme::SPACE_XS);
    let divider_step = px(1.0 + 2.0 * Theme::SPACE_XXS + Theme::SPACE_XS);
    let inter_pitch = intra_pitch + divider_step;

    for group in &groups {
        let group_runs: Vec<&TextRunLayout> =
            group.iter().map(|label| run(&runs, card, label)).collect();
        for pair in group_runs.windows(2) {
            let step = pair[1].bounds.top() - pair[0].bounds.top();
            assert!(
                (step - intra_pitch).abs() < px(EPS),
                "intra-group `{}` -> `{}` is {step:?}, expected {intra_pitch:?}",
                pair[0].text,
                pair[1].text
            );
        }
    }

    for pair in groups.windows(2) {
        let last_of_prev = run(&runs, card, pair[0].last().expect("non-empty group"));
        let first_of_next = run(&runs, card, pair[1].first().expect("non-empty group"));
        let step = first_of_next.bounds.top() - last_of_prev.bounds.top();
        assert!(
            (step - inter_pitch).abs() < px(EPS),
            "inter-group `{}` -> `{}` is {step:?}, expected {inter_pitch:?} with group divider",
            last_of_prev.text,
            first_of_next.text
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
            state.settings_form.filter = "Keys".into();
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
/// Reads the Keys section under the filter `Keys`: the row's first item, with
/// no saved sets the line saying so, stands at a chip's inset on the column
/// the set name box below it starts on. Not covered: the row's
/// second line of chips, which needs more saved sets than fit one line.
#[gpui::test]
fn the_keymap_row_starts_on_the_column_of_the_box_below_it(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "settings-keymap-column");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(DialogKind::Settings);
            state.focus_setting(Some(SettingsField::Filter));
            for ch in "Keys".chars() {
                state.setting_push(ch);
            }
            cx.notify();
        });
    });
    visual.run_until_parked();
    let card = visual
        .debug_bounds("dialog-card")
        .expect("the Settings dialog draws a card");
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let first = run(&runs, card, "No saved sets");
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
