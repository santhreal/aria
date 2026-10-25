//! Controls and States: every `aria_ui::components` entry, in every state it
//! can express.

use aria_ui::components;
use aria_ui::icons;
use aria_ui::motion;
use aria_ui::theme::Theme;
use gpui::{
    AnyElement, App, Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Styled, div, px,
};

use crate::gallery_kit::{caption, group, pin_hover, section, shelf, specimen};
use crate::{Gallery, GalleryState, NO_FIELD};

/// One specimen per component, at rest. The catalogue, not the state matrix.
///
/// Takes `cx` for the motion frame: the disclosure chevron below is drawn in
/// whichever motion mode the header switch is in, because "reduced" is a state
/// it has to survive rather than a state it skips, and the pinned hovers are set
/// at the frame's instant.
pub fn controls(theme: &Theme, cx: &mut Context<Gallery>) -> AnyElement {
    section(
        theme,
        "Controls",
        "every components:: entry once, at rest — hover any of these with the pointer",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_MD))
            .child(shelf(vec![
                specimen(
                    theme,
                    "button",
                    components::button("ctl-button", "Render", theme),
                ),
                specimen(
                    theme,
                    "ghost_button",
                    components::ghost_button("ctl-ghost", "Cancel", theme),
                ),
                specimen(
                    theme,
                    "danger_button",
                    components::danger_button("ctl-danger", "Delete", theme),
                ),
                specimen(
                    theme,
                    "icon_button",
                    components::icon_button("ctl-icon", icons::PLAY, theme),
                ),
                specimen(
                    theme,
                    "text_box",
                    components::text_box("ctl-field", "", "Clip name", false, theme),
                ),
            ]))
            .child(group(
                theme,
                "slider_row(theme, label, value, display)",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(Theme::SPACE_XS))
                    .child(components::slider_row(theme, "Opacity", 0.0, "0%"))
                    .child(components::slider_row(theme, "Scale", 0.65, "65%"))
                    .child(components::slider_row(theme, "Volume", 1.0, "100%")),
            ))
            .child(readout_rows(theme))
            .child(empty_state_rows(theme))
            .child(typed_rows(theme))
            .child(group(
                theme,
                "tab_bar(theme, &[(label, active)])",
                components::tab_bar(
                    theme,
                    &[("Clip", true), ("Levels", false), ("Color", false)],
                ),
            ))
            .child(chip_rows(theme, cx))
            .child(disclosure_rows(theme, cx.reduce_motion()))
            .child(opener_rows(theme, cx.reduce_motion()))
            .child(menu_lead_rows(theme))
            .child(icon_rows(theme, cx))
            .child(icon_space_rows(theme))
            .into_any_element(),
    )
    .into_any_element()
}

/// One row per [`components::ChipSize`], enumerated from `ChipSize::ALL` so a
/// third size cannot ship without a specimen.
///
/// Three states per row, because a chip is only readable as a set: the one in
/// force, one that is not, and one under the pointer — pinned, since a review
/// copy has no hand on the mouse. The compact row recedes further by design, and
/// that difference is the reason both rows are drawn.
fn chip_rows(theme: &Theme, cx: &App) -> AnyElement {
    for size in components::ChipSize::ALL {
        pin_hover("chip", &format!("ctl-chip-hover-{size:?}"), cx);
    }
    group(
        theme,
        "chip(id, label, active, size, theme) — active / inactive / hover (pinned)",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_SM))
            .children(components::ChipSize::ALL.map(|size| {
                shelf(vec![
                    chip_specimen(theme, size, "on", "All", true),
                    chip_specimen(theme, size, "off", "Video", false),
                    chip_specimen(theme, size, "hover", "Audio", false),
                ])
            })),
    )
}

fn chip_specimen(
    theme: &Theme,
    size: components::ChipSize,
    state: &str,
    label: &str,
    active: bool,
) -> AnyElement {
    let id = motion::MotionKey::new("gallery-chip", state).with(size as u8);
    specimen(
        theme,
        &format!("{size:?} {state}"),
        components::chip(id, label.to_string(), active, size, theme),
    )
}

/// The two entries that are not controls: the disclosure chevron and the row
/// banding.
///
/// Both chevrons are drawn at once because the turn is only reviewable as a
/// difference — shut leans the way the writing goes, open points down — and the
/// bands are drawn four deep because banding one row looks like a mistake and
/// banding none is what an even row is meant to look like.
fn disclosure_rows(theme: &Theme, reduced: bool) -> AnyElement {
    group(
        theme,
        "chevron(key, open, color, reduced) / band_fill(theme, index)",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_SM))
            .child(shelf(vec![
                specimen(
                    theme,
                    "chevron shut",
                    components::chevron(
                        motion::MotionKey::new("gallery-chevron", "shut"),
                        false,
                        theme.text_faint,
                        reduced,
                    ),
                ),
                specimen(
                    theme,
                    "chevron open",
                    components::chevron(
                        motion::MotionKey::new("gallery-chevron", "open"),
                        true,
                        theme.text,
                        reduced,
                    ),
                ),
            ]))
            .children((0..4).map(|index| {
                div()
                    .h(px(20.))
                    .px(px(Theme::SPACE_SM))
                    .flex()
                    .items_center()
                    .bg(components::band_fill(theme, index))
                    .text_size(px(11.))
                    .text_color(theme.text_faint)
                    .child(format!("band_fill row {index}"))
            })),
    )
}

/// The two controls that open rows under them, shut and open: the disclosure
/// button over a group of rows, and the list chip over a list of choices.
fn opener_rows(theme: &Theme, reduced: bool) -> AnyElement {
    let disclosure = |id: &'static str, open: bool| {
        components::disclosure_button(id, "Basic Correction", open, theme, reduced)
    };
    group(
        theme,
        "disclosure_button(id, label, open, theme, reduced) / list_chip(id, label, open, theme)",
        shelf(vec![
            specimen(
                theme,
                "disclosure_button shut",
                disclosure("ctl-disclosure-shut", false),
            ),
            specimen(
                theme,
                "disclosure_button open",
                disclosure("ctl-disclosure-open", true),
            ),
            specimen(
                theme,
                "list_chip shut",
                components::list_chip("ctl-list-shut", "Name", false, theme),
            ),
            specimen(
                theme,
                "list_chip open",
                components::list_chip("ctl-list-open", "Name", true, theme),
            ),
        ]),
    )
}

/// A group of readouts under its section label, at the width of a docked
/// panel: a plain value, a value long enough to move under its label, and the
/// two tints a verdict takes, `warning` for a fact to look at and `text_faint`
/// for an empty answer.
fn readout_rows(theme: &Theme) -> AnyElement {
    group(
        theme,
        "section_label(theme, title), readout(theme, label, value), readout_tinted(theme, label, value, tint)",
        div()
            .w(px(240.))
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .child(components::section_label(theme, "Source"))
            .child(components::readout(theme, "Duration", "00:00:04:12"))
            .child(components::readout(
                theme,
                "Path",
                "/media/projects/harbour/day-two/a-camera/clip-0042.mov",
            ))
            .child(components::readout_tinted(
                theme,
                "Dropped frames",
                "3",
                theme.warning,
            ))
            .child(components::readout_tinted(
                theme,
                "Proxy",
                "None",
                theme.text_faint,
            )),
    )
}

/// The line a panel draws in place of its content, in frames the size of a
/// docked panel: alone, wrapped in a narrow frame, and with the action it
/// offers below it.
fn empty_state_rows(theme: &Theme) -> AnyElement {
    let frame = |width: f32| {
        div()
            .w(px(width))
            .h(px(140.))
            .flex()
            .flex_col()
            .border_1()
            .border_color(theme.border)
            .rounded(px(Theme::CONTROL_RADIUS))
    };
    group(
        theme,
        "empty_state(id, text, theme), and with an action as its child",
        div()
            .flex()
            .flex_row()
            .gap(px(Theme::SPACE_MD))
            .child(frame(240.).child(components::empty_state(
                "gallery-jobs",
                "No background jobs.",
                theme,
            )))
            .child(frame(160.).child(components::empty_state(
                "gallery-search",
                "Import files or drag from your file manager.",
                theme,
            )))
            .child(frame(240.).child(
                components::empty_state("gallery-markers", "No markers.", theme).child(
                    components::ghost_button("ctl-empty-add", "Add at playhead", theme),
                ),
            )),
    )
}

/// The two parts of a box that takes typed text, in a box too narrow for its
/// name: the line at rest ends in an ellipsis, the line being typed leads with
/// one so its end stays beside the caret, and the caret at half strength is the
/// rename bar's blink caught midway.
fn typed_rows(theme: &Theme) -> AnyElement {
    const NAME: &str = "Interview B-roll, second take";
    let narrow = |typing: bool| {
        div()
            .w(px(120.))
            .flex()
            .items_center()
            .text_size(Theme::TEXT_UI)
            .text_color(theme.text)
            .child(components::typed_line(NAME, typing))
            .children(typing.then(|| components::caret_mark(Theme::TEXT_UI, 1.0, theme)))
    };
    group(
        theme,
        "typed_line(text, typing) / caret_mark(size, opacity, theme) — at rest / typing / blink",
        shelf(vec![
            specimen(theme, "typed_line at rest", narrow(false)),
            specimen(theme, "typed_line typing", narrow(true)),
            specimen(
                theme,
                "caret_mark at 0.5",
                div()
                    .flex()
                    .items_center()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text)
                    .child("b-roll")
                    .child(components::caret_mark(Theme::TEXT_UI, 0.5, theme)),
            ),
        ]),
    )
}

/// The lead column of a menu row in its three states, a check mark on a toggle
/// that holds, an icon, and the empty column of a plain row, each beside a
/// caption so the captions are seen to line up.
fn menu_lead_rows(theme: &Theme) -> AnyElement {
    let row = |checked: bool, icon: Option<&'static str>, caption: &'static str| {
        div()
            .px(px(Theme::SPACE_SM))
            .h(px(Theme::ROW_HEIGHT))
            .flex()
            .items_center()
            .gap(px(Theme::SPACE_SM))
            .text_size(Theme::TEXT_UI)
            .text_color(theme.text)
            .child(components::menu_lead(checked, icon, theme))
            .child(caption)
    };
    group(
        theme,
        "menu_lead(checked, icon, theme) — checked / icon / empty",
        div()
            .flex()
            .flex_col()
            .child(row(true, None, "Snapping"))
            .child(row(false, Some(icons::SCISSORS), "Split at Playhead"))
            .child(row(false, None, "Ripple Delete")),
    )
}

/// Repeated rows: the same tooltip name, one element id and one hover fade per
/// row, so a reviewer can watch a single row light rather than all six.
///
/// Row 2 is pinned open to prove the fades are keyed per index: if the key were
/// shared, every row here would be bright.
fn icon_rows(theme: &Theme, cx: &App) -> AnyElement {
    let paths = [
        icons::EYE,
        icons::LOCK,
        icons::VOLUME,
        icons::SCISSORS,
        icons::MAGNET,
        icons::TRASH,
    ];
    pin_hover("icon", "gallery-row-2", cx);
    group(
        theme,
        "icon_button((\"gallery-row\", index)) — one fade per row",
        shelf(
            paths
                .into_iter()
                .enumerate()
                .map(|(index, path)| {
                    let label = if index == 2 {
                        format!("gallery-row-{index} (pinned)")
                    } else {
                        format!("gallery-row-{index}")
                    };
                    specimen(
                        theme,
                        &label,
                        components::icon_button(("gallery-row", index), path, theme),
                    )
                })
                .collect(),
        ),
    )
}

/// Two rows of icon controls, the second with no lock: the room the lock would
/// take holds the volume control in the column of the row above.
fn icon_space_rows(theme: &Theme) -> AnyElement {
    let row = |lane: usize, lock: bool| {
        let middle = if lock {
            components::icon_button(("gallery-space-lock", lane), icons::LOCK, theme)
                .into_any_element()
        } else {
            components::icon_space().into_any_element()
        };
        div()
            .flex()
            .gap(px(Theme::SPACE_XXS))
            .child(components::icon_button(
                ("gallery-space-eye", lane),
                icons::EYE,
                theme,
            ))
            .child(middle)
            .child(components::icon_button(
                ("gallery-space-volume", lane),
                icons::VOLUME,
                theme,
            ))
    };
    group(
        theme,
        "icon_space() — the room of an icon_button with nothing in it",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XXS))
            .child(row(0, true))
            .child(row(1, false)),
    )
}

/// The state matrix. Hover states are pinned open through the same
/// `{kind}:{id}` fade key the component builds, so the state is reviewable
/// without a pointer.
pub fn states(theme: &Theme, state: GalleryState, cx: &mut Context<Gallery>) -> AnyElement {
    section(
        theme,
        "States",
        "rest / hover (pinned) / selected / disabled / focused / error",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_MD))
            .child(button_states(theme, cx))
            .child(field_states(theme, state, cx))
            .child(tab_states(theme, cx))
            .child(caption(
                theme,
                "No components:: entry takes an enabled flag, so `disabled` is \
                 composed here from container opacity plus a blocked pointer. \
                 The control itself still reports the same rest colors.",
            ))
            .into_any_element(),
    )
    .into_any_element()
}

fn button_states(theme: &Theme, cx: &App) -> AnyElement {
    pin_hover("button", "st-button-hover", cx);
    pin_hover("ghost", "st-ghost-hover", cx);
    pin_hover("danger", "st-danger-hover", cx);
    pin_hover("icon", "st-icon-hover", cx);
    group(
        theme,
        "button / ghost_button / danger_button / icon_button",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_MD))
            .child(shelf(vec![
                specimen(
                    theme,
                    "button rest",
                    components::button("st-button-rest", "Render", theme),
                ),
                specimen(
                    theme,
                    "button hover",
                    components::button("st-button-hover", "Render", theme),
                ),
                specimen(
                    theme,
                    "button disabled",
                    disabled(components::button("st-button-off", "Render", theme)),
                ),
                specimen(
                    theme,
                    "ghost rest",
                    components::ghost_button("st-ghost-rest", "Cancel", theme),
                ),
                specimen(
                    theme,
                    "ghost hover",
                    components::ghost_button("st-ghost-hover", "Cancel", theme),
                ),
                specimen(
                    theme,
                    "ghost selected",
                    components::ghost_button("st-ghost-sel", "Cancel", theme).bg(theme.wash(0.10)),
                ),
                specimen(
                    theme,
                    "ghost disabled",
                    disabled(components::ghost_button("st-ghost-off", "Cancel", theme)),
                ),
            ]))
            .child(shelf(vec![
                specimen(
                    theme,
                    "danger rest",
                    components::danger_button("st-danger-rest", "Delete", theme),
                ),
                specimen(
                    theme,
                    "danger hover",
                    components::danger_button("st-danger-hover", "Delete", theme),
                ),
                specimen(
                    theme,
                    "danger disabled",
                    disabled(components::danger_button("st-danger-off", "Delete", theme)),
                ),
                specimen(
                    theme,
                    "icon rest",
                    components::icon_button("st-icon-rest", icons::SETTINGS, theme),
                ),
                specimen(
                    theme,
                    "icon hover",
                    components::icon_button("st-icon-hover", icons::SETTINGS, theme),
                ),
                specimen(
                    theme,
                    "icon disabled",
                    disabled(components::icon_button(
                        "st-icon-off",
                        icons::SETTINGS,
                        theme,
                    )),
                ),
            ])),
    )
}

/// `text_box` takes `focused` from the caller, so the caller owns the click that
/// takes focus. Both fields here are live: click one and the caret moves.
fn field_states(theme: &Theme, state: GalleryState, cx: &mut Context<Gallery>) -> AnyElement {
    pin_hover("field", "st-field-hover", cx);
    let fields: Vec<AnyElement> = (0..2usize)
        .map(|index| {
            let focused = state.focused_field == index;
            let label = if focused {
                format!("field {index} focused (click)")
            } else {
                format!("field {index} idle (click)")
            };
            let value = if index == 0 { "b-roll-04" } else { "" };
            specimen(
                theme,
                &label,
                components::text_box(
                    motion::MotionKey::new("gallery-field", index),
                    value,
                    "Clip name",
                    focused,
                    theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this: &mut Gallery, _: &MouseDownEvent, _, cx| {
                        this.state.focused_field = if this.state.focused_field == index {
                            NO_FIELD
                        } else {
                            index
                        };
                        cx.notify();
                    }),
                ),
            )
        })
        .collect();
    group(
        theme,
        "text_box(id, value, placeholder, focused, theme)",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_MD))
            .child(shelf(fields))
            .child(shelf(vec![
                specimen(
                    theme,
                    "placeholder",
                    components::text_box("st-field-empty", "", "Clip name", false, theme),
                ),
                specimen(
                    theme,
                    "value",
                    components::text_box("st-field-value", "b-roll-04", "Clip name", false, theme),
                ),
                specimen(
                    theme,
                    "hover",
                    components::text_box("st-field-hover", "b-roll-04", "Clip name", false, theme),
                ),
                specimen(
                    theme,
                    "focused (caret)",
                    components::text_box("st-field-focus", "b-roll-04", "Clip name", true, theme),
                ),
                specimen(
                    theme,
                    "error (composed)",
                    div()
                        .p(px(1.))
                        .rounded(px(Theme::CONTROL_RADIUS + 1.))
                        .border_1()
                        .border_color(theme.danger)
                        .child(components::text_box(
                            "st-field-error",
                            "b-roll-04/",
                            "Clip name",
                            false,
                            theme,
                        )),
                ),
                specimen(
                    theme,
                    "disabled",
                    disabled(components::text_box(
                        "st-field-off",
                        "b-roll-04",
                        "Clip name",
                        false,
                        theme,
                    )),
                ),
                specimen(
                    theme,
                    "frame only (timecode)",
                    components::field_frame("st-field-frame", false, theme)
                        .child(div().child("00:00:12:05")),
                ),
            ])),
    )
}

/// `tab_bar` keys each tab by `label-index`, so two tabs in one bar that share a
/// label are still two fades.
///
/// The keys are global to the process, though: nothing in them names the bar.
/// Bars one and four below carry the same labels, so their tabs share a fade —
/// that is the shipped behaviour, shown rather than hidden.
fn tab_states(theme: &Theme, cx: &App) -> AnyElement {
    pin_hover("tab", "Speed-1", cx);
    group(
        theme,
        "tab_bar — active / inactive / hover (pinned) / duplicate labels / shared keys",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_SM))
            // Bars 1 and 4 carry the same labels, so their tabs would also share
            // a gpui global element id — and shared element state is a bug, not a
            // demonstration. Each bar gets an id of its own, which scopes the
            // element state while leaving the fade key shared.
            .child(div().id("tabs-a").child(components::tab_bar(
                theme,
                &[("Transform", true), ("Look", false), ("Effects", false)],
            )))
            .child(caption(theme, "1: Transform active, nothing hovered"))
            .child(components::tab_bar(
                theme,
                &[("Motion", false), ("Speed", false), ("Blend", true)],
            ))
            .child(caption(
                theme,
                "2: Blend active, Speed pinned hover — an inactive tab's hover \
                 lands on the same wash an active tab rests at",
            ))
            .child(components::tab_bar(
                theme,
                &[("Audio", true), ("Audio", false), ("Audio", false)],
            ))
            .child(caption(
                theme,
                "3: three tabs named Audio — each fades alone because the index \
                 rides in the id",
            ))
            .child(div().id("tabs-d").child(components::tab_bar(
                theme,
                &[("Transform", true), ("Look", false), ("Effects", false)],
            )))
            .child(caption(
                theme,
                "4: same labels as bar 1 — hover a tab here and the matching tab \
                 in bar 1 lights too, because the fade key is the element id alone",
            )),
    )
}

/// A control that cannot be pressed: dimmed, and the pointer cannot reach it.
/// `components::` has no disabled variant, so this is the gallery's own
/// composition rather than a shipped state.
fn disabled(control: impl IntoElement + 'static) -> AnyElement {
    div()
        .opacity(0.38)
        .child(div().occlude().child(control))
        .into_any_element()
}
