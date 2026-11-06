//! Contracts for title text and the text-effect catalog.
//!
//! The class closed here is the recurring one: `SetGenerator` carries the whole
//! generator, so an edit that rebuilds it from scratch silently discards every
//! sibling field. Retyping a title used to send `Generator::text(content)`, which
//! dropped the font, size, color, alignment, stroke, shadow, background, and
//! preset. Both directions are asserted — a content edit keeps the preset, and a
//! preset edit keeps the content — because only asserting one lets the other rot.
//!
//! Presets are enumerated from `text_effect_catalog()` at run time, so a new entry
//! is exercised without a second list here.

use aria_models::{ClipParam, TextAlignH, TextAlignV, TextCase, TextParam, text_effect_catalog};

use super::AppState;
use super::clip_text::TextSwitch;
use super::premiere_tests::with_clip;

/// A project whose selection is a title.
pub(super) fn with_title() -> AppState {
    let (mut state, _) = with_clip();
    state
        .add_generated(aria_models::Generator::text("Hello"))
        .expect("place a title");
    let title = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| {
            clip.generator
                .as_ref()
                .is_some_and(|generator| generator.kind == "text")
        })
        .map(|clip| aria_models::ClipId::from_raw(clip.id))
        .expect("the title landed");
    state.select_clip(title);
    assert!(state.selected_is_title());
    state
}

fn content(state: &AppState) -> String {
    state
        .selected_ui_clip()
        .and_then(|clip| clip.generator)
        .map(|generator| generator.content.to_string())
        .expect("a title is selected")
}

#[test]
fn every_catalog_preset_applies_reads_back_and_clears() {
    let mut state = with_title();
    assert_eq!(state.selected_ui_clip().unwrap().text_effect, None);
    assert!(!text_effect_catalog().is_empty());

    for spec in text_effect_catalog() {
        state
            .set_selected_text_effect(&spec.id)
            .unwrap_or_else(|err| panic!("{}: {err:?}", spec.id));
        assert_eq!(
            state.selected_ui_clip().unwrap().text_effect.as_deref(),
            Some(spec.id.as_str()),
            "{} did not read back",
            spec.id
        );
    }

    state.clear_selected_text_effect().unwrap();
    assert_eq!(state.selected_ui_clip().unwrap().text_effect, None);
}

#[test]
fn retyping_a_title_keeps_its_preset() {
    let mut state = with_title();
    let first = text_effect_catalog()[0].id.as_str();
    state.set_selected_text_effect(first).unwrap();

    state.set_selected_text("Second line".into()).unwrap();
    assert_eq!(content(&state), "Second line");
    assert_eq!(
        state.selected_ui_clip().unwrap().text_effect.as_deref(),
        Some(first),
        "the content edit flattened the style"
    );
}

#[test]
fn applying_a_preset_keeps_the_text() {
    let mut state = with_title();
    state.set_selected_text("Kept".into()).unwrap();
    for spec in text_effect_catalog() {
        state.set_selected_text_effect(&spec.id).unwrap();
        assert_eq!(
            content(&state),
            "Kept",
            "{} replaced the title's content",
            spec.id
        );
    }
    state.clear_selected_text_effect().unwrap();
    assert_eq!(content(&state), "Kept");
}

#[test]
fn a_no_op_text_edit_leaves_no_undo_entry() {
    // Draining undo here would delete the title and with it the selection, so the
    // depth is probed instead: one undo after two no-ops must land on the state
    // before the single real edit.
    let mut state = with_title();
    let first = text_effect_catalog()[0].id.as_str();
    state.set_selected_text_effect(first).unwrap();

    let held = content(&state);
    state.set_selected_text(held).unwrap();
    state.set_selected_text_effect(first).unwrap();

    assert!(state.undo());
    assert_eq!(
        state.selected_ui_clip().unwrap().text_effect,
        None,
        "a no-op text edit pushed an undo entry"
    );
}

#[test]
fn text_controls_are_inert_on_a_clip_that_is_not_a_title() {
    // The media fixture is selected, not a title: every setter is a no-op rather
    // than an error in the status line.
    let (mut state, _) = with_clip();
    assert!(!state.selected_is_title());
    state.set_selected_text("nope".into()).unwrap();
    state
        .set_selected_text_effect(&text_effect_catalog()[0].id)
        .unwrap();
    state.clear_selected_text_effect().unwrap();
    assert_eq!(state.selected_ui_clip().unwrap().text_effect, None);
}

/// Every switch the Character and Paragraph panels draw flips its own property
/// and no other, and flips it back, read off the projected style. A switch
/// wired to the wrong field reads back a neighbor changed. Enumerated from
/// `TextSwitch::ALL`, so a new switch is covered here with no edit.
#[test]
fn every_type_switch_flips_its_own_property_and_flips_back() {
    let mut state = with_title();
    for switch in TextSwitch::ALL {
        let before = state.selected_text_face().expect("a title is selected");
        state.toggle_selected_text_switch(switch).unwrap();
        let flipped = state.selected_text_face().expect("a title is selected");
        for other in TextSwitch::ALL {
            let expected = before.is_on(other) != (other == switch);
            assert_eq!(
                flipped.is_on(other),
                expected,
                "flipping {switch:?} left {other:?} wrong"
            );
        }
        state.toggle_selected_text_switch(switch).unwrap();
        assert_eq!(
            state.selected_text_face(),
            Some(before),
            "{switch:?} did not flip back"
        );
    }
}

/// A treatment switch takes the preset mark off, because the treatments no
/// longer read as that preset; a face or paragraph switch leaves it.
#[test]
fn a_treatment_switch_clears_the_preset_mark_and_no_other_switch_does() {
    let first = text_effect_catalog()[0].id.as_str();
    for switch in TextSwitch::ALL {
        let mut state = with_title();
        state.set_selected_text_effect(first).unwrap();
        state.toggle_selected_text_switch(switch).unwrap();
        let marked = state.selected_ui_clip().unwrap().text_effect.is_some();
        assert_eq!(
            marked,
            !TextSwitch::TREATMENTS.contains(&switch),
            "{switch:?}"
        );
    }
}

/// Case and both alignments read back every choice, and choosing the one in
/// force pushes no undo entry: one undo after a repeat lands on the choice
/// before it.
#[test]
fn case_and_alignment_read_back_and_a_repeat_is_inert() {
    let mut state = with_title();
    let face = |state: &AppState| state.selected_text_face().expect("a title is selected");
    for case in TextCase::ALL {
        state.set_selected_text_case(case).unwrap();
        assert_eq!(face(&state).case, case);
    }
    for align in TextAlignH::ALL {
        state.set_selected_text_align_h(align).unwrap();
        assert_eq!(face(&state).align_h, align);
    }
    for align in TextAlignV::ALL {
        state.set_selected_text_align_v(align).unwrap();
        assert_eq!(face(&state).align_v, align);
    }
    let [.., earlier, last] = TextAlignV::ALL;
    state.set_selected_text_align_v(last).unwrap();
    assert!(state.undo());
    assert_eq!(
        face(&state).align_v,
        earlier,
        "a repeated alignment pushed an undo entry"
    );
}

/// A swatch writes the color through the catalog's property path and reads
/// back through the rows the panel draws; the color in force is inert, and the
/// stroke's color row arrives with the stroke.
#[test]
fn a_color_swatch_sets_the_color_and_a_repeat_is_inert() {
    let mut state = with_title();
    let color = |state: &AppState, param: ClipParam| {
        state
            .param_colors("text")
            .into_iter()
            .find(|row| row.param == param)
            .map(|row| row.value)
    };
    let fill = ClipParam::Text {
        param: TextParam::Fill,
    };
    let stroke = ClipParam::Text {
        param: TextParam::StrokeColor,
    };
    let held = color(&state, fill).expect("a title draws its fill");
    assert_eq!(color(&state, stroke), None, "no stroke, no stroke color");
    let picked = crate::app::clip_effects::PARAM_COLORS
        .iter()
        .map(|(_, _, rgba)| *rgba)
        .find(|rgba| *rgba != held)
        .expect("a swatch other than the fill");
    state.set_param_color(fill, picked).unwrap();
    assert_eq!(color(&state, fill), Some(picked));
    state.set_param_color(fill, picked).unwrap();
    assert!(state.undo());
    assert_eq!(
        color(&state, fill),
        Some(held),
        "a repeated swatch pushed an undo entry"
    );
    state
        .toggle_selected_text_switch(TextSwitch::Stroke)
        .unwrap();
    assert!(
        color(&state, stroke).is_some(),
        "switching the stroke on drew no stroke color"
    );
}

/// Window > Character and Window > Paragraph dock panels that draw the
/// selected title's controls, and a second press closes each.
#[gpui::test]
fn the_character_and_paragraph_panels_dock_from_the_window_menu(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "text-panels");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .add_generated(aria_models::Generator::text("Hello"))
                .expect("place a title");
            let title = state
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|track| track.clips.iter())
                .find(|clip| clip.generator.is_some())
                .map(|clip| aria_models::ClipId::from_raw(clip.id))
                .expect("the title landed");
            state.select_clip(title);
            cx.notify();
        })
    });
    for (panel, drawn) in [
        (crate::dock::PANEL_CHARACTER, "character-panel"),
        (crate::dock::PANEL_PARAGRAPH, "paragraph-panel"),
    ] {
        let row = format!("{}{panel}", crate::chrome::menu_window::PANEL_PREFIX);
        let press = |visual: &mut gpui::VisualTestContext| {
            let found = visual.update(|_, app| {
                state.update(app, |state, cx| {
                    crate::chrome::menu_items::run_item(
                        crate::types::AppMenu::Window,
                        &row,
                        state,
                        cx,
                    )
                })
            });
            assert!(found, "Window has no row for {panel}");
            visual.run_until_parked();
            visual.read(|app| state.read(app).dock_tree.is_open(panel))
        };
        if visual.read(|app| state.read(app).dock_tree.is_open(panel)) {
            assert!(!press(visual), "{panel} did not close");
        }
        assert!(press(visual), "{panel} did not open");
        assert!(
            visual.debug_bounds(drawn).is_some(),
            "{panel} is open and not drawn"
        );
        assert!(!press(visual), "{panel} did not close");
        assert!(
            visual.debug_bounds(drawn).is_none(),
            "{panel} is closed and still drawn"
        );
    }
}

/// A drawn shell with a title selected and the dock panel `panel` open.
fn title_panel<'a>(
    cx: &'a mut gpui::TestAppContext,
    tag: &str,
    panel: &'static str,
) -> (gpui::Entity<AppState>, &'a mut gpui::VisualTestContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, tag);
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .add_generated(aria_models::Generator::text("Hello"))
                .expect("place a title");
            let title = state
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|track| track.clips.iter())
                .find(|clip| clip.generator.is_some())
                .map(|clip| aria_models::ClipId::from_raw(clip.id))
                .expect("the title landed");
            state.select_clip(title);
            let row = format!("{}{panel}", crate::chrome::menu_window::PANEL_PREFIX);
            if !state.dock_tree.is_open(panel) {
                assert!(
                    crate::chrome::menu_items::run_item(
                        crate::types::AppMenu::Window,
                        &row,
                        state,
                        cx,
                    ),
                    "Window has no row for {panel}"
                );
            }
            cx.notify();
        })
    });
    visual.run_until_parked();
    (state, visual)
}

/// The Paragraph panel draws no rows that key, so its Wrap line stands in no
/// keyed columns: the label at the panel's left edge and the switch at its
/// right, as a Settings switch row stands. Drawn in the keyed rows' columns,
/// the label stood a stopwatch's width in and the switch mid-panel, before
/// blanks for a keyframe navigator and time area the panel does not draw.
#[gpui::test]
fn the_paragraph_panel_draws_wrap_from_edge_to_edge(cx: &mut gpui::TestAppContext) {
    let (_state, visual) = title_panel(cx, "paragraph-wrap", crate::dock::PANEL_PARAGRAPH);
    let panel = visual
        .debug_bounds("paragraph-panel")
        .expect("Paragraph is open and not drawn");
    let label = visual
        .debug_bounds("insp-label-free-Wrap")
        .expect("Wrap is not a free line on the Paragraph panel");
    let value = visual
        .debug_bounds("insp-value-free-Wrap")
        .expect("Wrap draws no switch on the Paragraph panel");
    // The panel's padding plus the line's own.
    let inset = gpui::px(crate::theme::Theme::SPACE_SM + crate::theme::Theme::SPACE_XS + 0.5);
    assert!(
        label.left() - panel.left() <= inset,
        "Wrap's label {label:?} stands in from the panel {panel:?}"
    );
    assert!(
        panel.right() - value.right() <= inset,
        "Wrap's switch {value:?} stops short of the panel's edge {panel:?}"
    );
}

/// The Character panel's controls are taller than its frame, which scrolls
/// them. The font list draws every row it shows before it scrolls, a whole
/// row each: laid straight into the frame's column, the list was the one
/// control that could shrink and was squeezed to three rows and a part.
///
/// The rows come from the host's installed families, so on a host with fewer
/// than three the list fits either way and this cannot tell the two apart.
#[gpui::test]
fn the_character_panel_scrolls_rather_than_squeezing_the_font_list(cx: &mut gpui::TestAppContext) {
    let (state, visual) = title_panel(cx, "character-fonts", crate::dock::PANEL_CHARACTER);
    let families = visual.read(|app| state.read(app).font_families().len());
    const ROWS_SHOWN: usize = 7;
    let shown = (families + 1).min(ROWS_SHOWN);
    let mut previous: Option<gpui::Bounds<gpui::Pixels>> = None;
    for row in 0..shown {
        let drawn = visual
            .debug_bounds(format!("font-row-{row}").leak())
            .unwrap_or_else(|| panic!("font row {row} of {shown} is not drawn"));
        assert_eq!(
            drawn.size.height,
            gpui::px(crate::theme::Theme::ROW_HEIGHT),
            "font row {row} is {drawn:?}"
        );
        if let Some(previous) = previous {
            assert_eq!(
                drawn.top(),
                previous.bottom(),
                "font row {row} {drawn:?} does not follow {previous:?}"
            );
        }
        previous = Some(drawn);
    }
}

/// The class closed here: an on/off property of a title drawn as a chip. Bold,
/// Italic, Underline, Kerning, Stroke, Shadow and Card were chips in a wrapping
/// row, where an off chip read as plain text and "Bold Italic Underline
/// Kerning" read as a sentence with one word lit, while Wrap, their sibling,
/// was a switch. Every switch of `TextSwitch::ALL` must draw a line of its own
/// with a switch in its value, and no chip, on the Text tab in the keyed rows'
/// columns and on one of the Character and Paragraph panels in free ones, so a
/// switch added to the model fails until both surfaces draw it.
///
/// Also closes "a Text tab control at the section heading's edge": the case,
/// alignment and preset chips and Edit text… started left of the label column
/// every other tab sets its controls on. Every chip and ghost button the tab
/// draws from Edit text… down starts its frame on or right of that column.
///
/// Not covered: a control drawn without `components::chip`, `ghost_button` or
/// `switch`, such as a font row or a cloud preset row.
#[gpui::test]
fn every_text_switch_is_a_switch_on_a_line_of_its_own(cx: &mut gpui::TestAppContext) {
    use crate::components::noted::{
        Drawn, NotingControls, control_selector, drawn_controls, record_controls,
    };
    use crate::dock::{PANEL_CHARACTER, PANEL_INSPECTOR, PANEL_PARAGRAPH};
    use crate::interned::intern;
    use crate::motion::{ControlId, MotionKey};

    let (state, visual) = title_panel(cx, "text-switch-lines", PANEL_CHARACTER);
    visual.update(|_, app| app.set_reduce_motion(true));
    let _noting = NotingControls::start();
    let paragraph = vec![TextSwitch::Wrap];
    let character: Vec<TextSwitch> = TextSwitch::ALL
        .into_iter()
        .filter(|switch| !paragraph.contains(switch))
        .collect();
    let surfaces = [
        ("insp-text", PANEL_INSPECTOR, true, TextSwitch::ALL.to_vec()),
        ("character-panel", PANEL_CHARACTER, false, character),
        ("paragraph-panel", PANEL_PARAGRAPH, false, paragraph),
    ];
    for (surface, panel, keyed, switches) in surfaces {
        record_controls();
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                if !state.dock_tree.is_open(panel) {
                    let row = format!("{}{panel}", crate::chrome::menu_window::PANEL_PREFIX);
                    let menu = crate::types::AppMenu::Window;
                    assert!(
                        crate::chrome::menu_items::run_item(menu, &row, state, cx),
                        "Window has no row for {panel}"
                    );
                }
                state.focus_dock_panel(panel);
                state.inspector_tab = "text".to_string();
                cx.notify();
            })
        });
        visual.run_until_parked();
        let (controls, _) = drawn_controls();
        for switch in switches {
            let label = switch.label();
            let cell = |cell: &str| {
                intern(&if keyed {
                    format!("insp-{cell}-{label}")
                } else {
                    format!("insp-{cell}-free-{label}")
                })
            };
            let line = visual
                .debug_bounds(cell("label"))
                .unwrap_or_else(|| panic!("{surface}: {label} draws no line of its own"));
            let value = visual
                .debug_bounds(cell("value"))
                .unwrap_or_else(|| panic!("{surface}: {label}'s line draws no value"));
            let id = ControlId::from(MotionKey::new("text-switch", label).with(surface));
            let toggle = control_selector("switch", id, &Drawn::Switch);
            assert!(
                controls.iter().any(|control| control.selector == toggle),
                "{surface}: {label} draws no switch"
            );
            let drawn = visual
                .debug_bounds(intern(&toggle))
                .unwrap_or_else(|| panic!("{surface}: {label}'s switch is not drawn"));
            assert!(
                value.contains(&drawn.center())
                    && (f32::from(drawn.center().y) - f32::from(line.center().y)).abs() < 1.0,
                "{surface}: {label}'s switch at {drawn:?} is not in its value at {value:?} on its line at {line:?}"
            );
            let chip = format!("chip-{id:?}-");
            assert!(
                !controls
                    .iter()
                    .any(|control| control.selector.starts_with(&chip)),
                "{surface}: {label} is drawn as a chip"
            );
        }
        if panel != PANEL_INSPECTOR {
            continue;
        }
        let column = visual
            .debug_bounds("insp-label-Bold")
            .expect("the Text tab draws no Bold line")
            .left();
        let body = visual
            .debug_bounds("inspector-body")
            .expect("the Inspector draws no body");
        let edit = Drawn::Labelled {
            label: "Edit text…".into(),
            inset: 0.0,
            size: gpui::px(0.0),
        };
        let edit = visual
            .debug_bounds(intern(&control_selector(
                "ghost",
                ControlId::from("text-edit"),
                &edit,
            )))
            .expect("the Text tab draws no Edit text…");
        let mut held = 0;
        for control in &controls {
            let Drawn::Labelled { label, .. } = &control.drawn else {
                continue;
            };
            let Some(drawn) = visual.debug_bounds(intern(&control.selector)) else {
                continue;
            };
            let outside = drawn.left() < body.left() || drawn.right() > body.right();
            if outside || drawn.top() < edit.top() {
                continue;
            }
            held += 1;
            assert!(
                f32::from(drawn.left()) > f32::from(column) - 0.51,
                "the Text tab's {label} starts its frame at {:?}, left of the label column at {column:?}",
                drawn.left()
            );
        }
        // Edit text…, the four cases, the alignments and the presets.
        let least = 1 + TextCase::ALL.len() + TextAlignH::ALL.len() + TextAlignV::ALL.len();
        assert!(
            held >= least,
            "the Text tab held {held} chips and buttons to the label column, fewer than {least}"
        );
    }
}

#[test]
fn every_bundled_sticker_places_a_clip_that_the_model_accepts() {
    // The Stickers tab is generated from this catalog, so a bundled asset the model
    // would refuse would be a dead tile. Each id gets its own project: placement is
    // at the playhead, so placing the whole catalog into one lane would fail on
    // overlap and prove nothing about the asset.
    let catalog = aria_models::sticker_catalog();
    assert!(!catalog.is_empty());

    for spec in catalog {
        let (mut state, _) = with_clip();
        let before = state.store.sequence.clip_count();
        state
            .add_generated(aria_models::Generator::sticker(&spec.id))
            .unwrap_or_else(|err| panic!("{}: {err:?}", spec.id));
        assert_eq!(
            state.store.sequence.clip_count(),
            before + 1,
            "{} placed no clip",
            spec.id
        );
    }
}
