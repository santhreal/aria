//! Each single-choice group in Settings stands under the row that reads its
//! value, and no choice repeats that row's name.
//!
//! The class closed here: two single-choice groups drawn back to back, which
//! read as one list with two rows lit, and a group told apart only by a name
//! written into every one of its rows. The Canvas aspect presets stood under
//! the Size row with the canvas sizes after them, so Match footage and Derived
//! were lit in one list of a dozen rows; the Appearance accents read
//! `Accent: Blue` through `Accent: Graphite`, the theme row `Theme: Shipped`,
//! and the motion rows `Full motion` through `No motion`, while the modes above
//! them read bare. The choices are read from the registries and formats each
//! section draws, so a new aspect, baseline, rate, layout, display, mode,
//! accent, theme file or motion speed is placed without an edit here.
//!
//! Not caught: the hairline between groups, which the test renderer does not
//! raster, and a new group, whose value row needs listing in [`canvas`] or
//! [`appearance`].

use aria_models::{AudioLayout, CanvasAspect, TimeDisplay};
use aria_settings::ThemeChoice;
use gpui::{TestAppContext, px, size};

use crate::app::AppState;
use crate::app::settings_form::SettingsSection;
use crate::appearance::AppearanceMode;
use crate::motion::MotionPreference;
use crate::settings::MIN_WINDOW_WIDTH;
use crate::surfaces::settings_pick::RATES;
use crate::test_support::drawn_shell;

/// Tall enough that every row of a section is drawn without scrolling.
const TALL: f32 = 2400.0;

/// One section's groups: each value row's label and its choices, in order.
type Groups = Vec<(&'static str, Vec<String>)>;

fn canvas(state: &AppState) -> Groups {
    let sizes = std::iter::once(super::DERIVED_SIZE.to_string()).chain(
        state
            .canvas_resolution_choices()
            .into_iter()
            .map(|(baseline, pixels)| super::size_label(baseline, pixels)),
    );
    vec![
        (
            "Aspect",
            CanvasAspect::ALL
                .into_iter()
                .map(|aspect| aspect.label().to_string())
                .collect(),
        ),
        ("Size", sizes.collect()),
        (
            "Frame rate",
            RATES.into_iter().map(super::format_fps).collect(),
        ),
        (
            "Audio layout",
            AudioLayout::ALL
                .into_iter()
                .map(|layout| layout.label().to_string())
                .collect(),
        ),
        (
            "Time display",
            TimeDisplay::ALL
                .into_iter()
                .map(|display| display.label().to_string())
                .collect(),
        ),
    ]
}

fn appearance(state: &AppState) -> Groups {
    let themes = std::iter::once(super::pickers::SHIPPED_THEME.to_string())
        .chain(state.theme_files.themes.iter().map(|file| file.label()));
    vec![
        (
            "Mode",
            AppearanceMode::ALL
                .into_iter()
                .map(|mode| mode.label().to_string())
                .collect(),
        ),
        (
            "Accent",
            ThemeChoice::ALL
                .into_iter()
                .map(|choice| choice.label().to_string())
                .collect(),
        ),
        ("Theme", themes.collect()),
        (
            "Motion",
            MotionPreference::ALL
                .into_iter()
                .map(|pref| pref.label().to_string())
                .collect(),
        ),
    ]
}

#[gpui::test]
fn every_canvas_choice_stands_under_its_own_groups_value_row(cx: &mut TestAppContext) {
    assert_grouped(cx, SettingsSection::Canvas, canvas);
}

#[gpui::test]
fn every_appearance_choice_stands_under_its_own_groups_value_row(cx: &mut TestAppContext) {
    assert_grouped(cx, SettingsSection::Appearance, appearance);
}

fn assert_grouped(
    cx: &mut TestAppContext,
    section: SettingsSection,
    groups: fn(&AppState) -> Groups,
) {
    let (state, visual) = drawn_shell(cx, "settings-choice-groups");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_settings_at(section);
            cx.notify();
        });
    });
    // The boot cover's wordmark lies over the card.
    crate::test_support::wait_out_splash(visual);
    visual.simulate_resize(size(px(MIN_WINDOW_WIDTH), px(TALL)));
    visual.run_until_parked();
    let card = visual
        .debug_bounds("dialog-card")
        .expect("Settings draws a card");
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let groups = visual.read(|app| groups(state.read(app)));

    // The editor behind the scrim draws text too, so a value row is the one
    // run inside the card that reads its label.
    let headers: Vec<gpui::Pixels> = groups
        .iter()
        .map(|(header, _)| {
            let tops: Vec<gpui::Pixels> = runs
                .iter()
                .filter(|run| card.contains(&run.bounds.center()) && run.text.as_ref() == *header)
                .map(|run| run.bounds.top())
                .collect();
            assert_eq!(
                tops.len(),
                1,
                "the {section:?} card reads `{header}` {} times, not once as a value row",
                tops.len()
            );
            tops[0]
        })
        .collect();
    for pair in headers.windows(2) {
        assert!(
            pair[0] < pair[1],
            "the {section:?} value rows stand out of order: {headers:?}"
        );
    }

    for (index, (header, choices)) in groups.iter().enumerate() {
        let below = headers[index];
        let above = headers.get(index + 1).copied();
        for choice in choices {
            assert!(
                !choice.to_lowercase().contains(&header.to_lowercase()),
                "`{choice}` repeats the name of its `{header}` value row"
            );
            let row = visual
                .debug_bounds(crate::interned::intern(&format!("picker-label-{choice}")))
                .unwrap_or_else(|| panic!("the {section:?} card draws no `{choice}`"));
            let top = row.top();
            assert!(
                top > below && above.is_none_or(|next| top < next),
                "`{choice}` stands at {top:?}, outside the `{header}` group between \
                 {below:?} and {above:?}"
            );
        }
    }
}
