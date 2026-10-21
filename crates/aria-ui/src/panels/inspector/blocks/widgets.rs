//! Shared inspector rows and labels, used by more than one tab.
//!
//! Every numeric control in the inspector is the same row — a label and a
//! scrubbable value (`panels::value_readout`) — so it is drawn once, here. That is
//! what makes the motion a property of "a row" rather than of the fifteen
//! places that draw one: the catalog's properties, the transform's six, and
//! every effect knob all animate because [`stepper_row`] does.
//!
//! A row drawn from the property catalog gets the whole treatment, because the
//! catalog supplies the two things the rest cannot: a range, so there is a
//! track for the value to travel along, and a [`ClipParam`], so there is
//! something for a keyframe to be written against. A row whose caller has
//! neither still fades under the pointer and reads the same.

use crate::app::AppState;
use crate::app::clip_params::{ColorRow, ParamRow};
use crate::app::drivers::DriverDraft;
use crate::app::key_strip::KeyStrip;
use crate::app::value_scrub::{ScrubTarget, ValueKind};
use crate::components::Wraps as _;
use crate::motion::{self, glide};
use crate::panels::value_readout::{self, Look};
use crate::theme::Theme;
use crate::tooltip::WithTip as _;
use crate::types::UiClipKind;
use aria_engine::EngineError;
use aria_models::ClipParam;
use gpui::motion::presets;
use gpui::prelude::FluentBuilder;
use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px, relative,
};

#[path = "widgets_catalog.rs"]
mod catalog;
#[path = "widgets_keys.rs"]
mod keys;

pub use catalog::{audio_row, color_rows, param_rows, param_rows_into, swatch_row};
use keys::{key_controls, stopwatch_dot};

/// Thickness of a row's value track.
const TRACK_H: f32 = 3.0;

/// Diameter of the thumb riding the track.
const THUMB: f32 = 7.0;

/// Diameter of the stopwatch's ring.
const DOT: f32 = 8.0;

/// The room a label holds on a row with a time area before it shrinks, in
/// pixels: the width of Anchor point X at `TEXT_UI` in Inter (77.4 px) plus a
/// little air. The label grows into any room the row has past it. In a frame
/// too narrow for every control, the strip narrows first and is left blank
/// below its minimum, and the label ends in an ellipsis only once the strip has
/// no width left.
pub const LABEL_RESERVE: f32 = 80.0;

/// The narrowest the value column is drawn on a row with a time area, in
/// pixels. A value narrower than it stands at its right edge, so the controls
/// after it start at one x down the panel whatever each row prints, and the
/// strips narrow together in a narrow frame.
const VALUE_MIN: f32 = 56.0;

/// What a row's value is doing, beyond the number it prints.
#[derive(Clone, Copy)]
pub struct Travel {
    /// The value itself. The row eases toward this, so it can tell a value
    /// that is still arriving from one that has settled.
    pub value: f32,
    /// One nudge. How far the row still has to go is measured in these, so the
    /// property's own scale decides what "nearly there" means — a constant
    /// would be wrong for degrees and right for fractions.
    pub step: f32,
    /// The bounds the model enforces, when it bounds both sides. The track is
    /// drawn against these and the fill follows the same eased value the
    /// readout does, so the bar and the number can never disagree.
    pub range: Option<(f32, f32)>,
}

/// What a row prints where the selected clips read differently.
pub const MIXED: &str = "—";

/// A row's number: what it prints, its value in the units it prints, what one
/// unit of drag moves it by, and how it is typed.
pub struct Readout {
    pub text: String,
    pub value: f32,
    pub unit: f32,
    pub kind: ValueKind,
}

impl Readout {
    /// A bare number printed with `decimals` digits.
    pub fn number(value: f32, decimals: u8, unit: f32) -> Self {
        let kind = ValueKind::Number { decimals };
        Self {
            text: kind.format(value),
            value,
            unit,
            kind,
        }
    }

    /// A number the caller prints with its own unit or suffix, typed back as a
    /// number with `decimals` digits.
    pub fn printed(text: String, value: f32, decimals: u8, unit: f32) -> Self {
        Self {
            text,
            value,
            unit,
            kind: ValueKind::Number { decimals },
        }
    }

    /// This readout with a dash printed instead of its number when `mixed`:
    /// the selected clips read differently there. A drag still moves the value
    /// it holds, the first clip's, and the others take where it lands.
    pub fn dashed(mut self, mixed: bool) -> Self {
        if mixed {
            self.text = MIXED.to_owned();
        }
        self
    }

    /// A frame count, printed and typed as timecode at `num / den`.
    pub fn frames(frames: i64, num: i32, den: i32) -> Self {
        let kind = ValueKind::Frames { num, den };
        let value = frames as f32;
        Self {
            text: kind.format(value),
            value,
            unit: 1.0,
            kind,
        }
    }
}

/// The keyframe affordances of a row, and what the property's curve looks like
/// now: the stopwatch left of the label, and the navigator, the reset button
/// and the time area right of the value.
#[derive(Clone)]
pub struct Stopwatch {
    pub param: ClipParam,
    /// Whether the property carries any keyframes at all, which is what the
    /// stopwatch turns on and off.
    pub animated: bool,
    /// Whether one of them sits under the playhead, which is what the
    /// navigator's centre diamond adds or removes.
    pub at_playhead: bool,
    /// Whether a keyframe sits before the playhead, and whether one sits after
    /// it: where the navigator's arrows go.
    pub keys_around: (bool, bool),
    /// The property holds something other than its reset value at the
    /// playhead, on the drawn clip or on another selected clip that shares it,
    /// so the reset button has something to do.
    pub off_default: bool,
    /// The property's driver, shown under the row.
    pub driver: Option<DriverLine>,
    /// A pick whip is waiting: a click on this row inserts a reference to
    /// the property instead of changing it.
    pub pick_target: bool,
    /// The property's time area, on a surface that draws one.
    pub strip: Option<KeyStrip>,
}

/// What a surface reads off the state once a frame to draw the stopwatches of
/// its catalog rows: the driver draft open on the selected clip, and whether
/// its pick whip waits.
pub struct Watching {
    draft: Option<DriverDraft>,
    picking: bool,
}

impl Watching {
    pub fn read(state: &AppState) -> Self {
        let selected = state.selected_clip();
        Self {
            draft: state
                .driver_draft()
                .filter(|draft| Some(draft.clip) == selected)
                .cloned(),
            picking: state.driver_picking(),
        }
    }

    /// The stopwatch `row` draws, with `strip` as its time area.
    pub fn stopwatch(&self, row: &ParamRow, strip: Option<KeyStrip>) -> Stopwatch {
        let param = row.param;
        // A pair shows its driver once, under its first component.
        let driver = if row.component.unwrap_or(0) != 0 {
            None
        } else if let Some(draft) = self.draft.as_ref().filter(|draft| draft.param == param) {
            Some(DriverLine {
                source: draft.text.clone().into(),
                fault: row.fault.clone(),
                editing: true,
                picking: draft.picking,
            })
        } else {
            row.driver.clone().map(|source| DriverLine {
                source,
                fault: row.fault.clone(),
                editing: false,
                picking: false,
            })
        };
        Stopwatch {
            param,
            animated: row.animated,
            at_playhead: row.at_playhead,
            keys_around: row.keys_around,
            off_default: row.off_default,
            driver,
            pick_target: self.picking,
            strip,
        }
    }
}

/// The stopwatch a colour row draws. A colour has no driver.
pub fn color_stopwatch(row: &ColorRow, strip: Option<KeyStrip>) -> Stopwatch {
    Stopwatch {
        param: row.param,
        animated: row.animated,
        at_playhead: row.at_playhead,
        keys_around: row.keys_around,
        off_default: row.off_default,
        driver: None,
        pick_target: false,
        strip,
    }
}

/// A property's driver as its row shows it.
#[derive(Clone)]
pub struct DriverLine {
    /// The driver's source, or the draft while it is edited.
    pub source: SharedString,
    /// Why the driver failed at the playhead.
    pub fault: Option<SharedString>,
    /// The source is open in the `aria.field.driver` box.
    pub editing: bool,
    /// The pick whip is waiting for a property.
    pub picking: bool,
}

/// One numeric row.
///
/// The value is a scrubbable readout: a drag moves it, a click opens it as
/// text. `write` moves the value by a delta in the units `readout` prints, from
/// the state the gesture started on. The row reports its own state: it fades
/// under the pointer, its fill travels to a new value instead of appearing at
/// it, and its readout leads while the value is still moving.
#[allow(clippy::too_many_arguments)]
pub fn stepper_row(
    theme: &Theme,
    state: &gpui::Entity<AppState>,
    key: motion::MotionKey,
    label: impl Into<SharedString>,
    readout: Readout,
    travel: Option<Travel>,
    stopwatch: Option<Stopwatch>,
    write: impl Fn(&mut AppState, f32) -> Result<(), EngineError> + 'static,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let label = label.into();
    let hovered = motion::hover_t(key);
    let fault = stopwatch
        .as_ref()
        .and_then(|watch| watch.driver.as_ref())
        .and_then(|line| line.fault.clone());
    let driver_line = stopwatch
        .as_ref()
        .and_then(|watch| watch.driver.clone())
        .zip(stopwatch.as_ref().map(|watch| watch.param));
    let pick = stopwatch
        .as_ref()
        .filter(|watch| watch.pick_target)
        .map(|watch| watch.param);

    // One eased value feeds both the fill and the readout, so a bar that has
    // arrived cannot be drawn beside a number that says it has not.
    let (fill, lead) = match travel {
        Some(travel) => {
            let eased = glide::glide(key, travel.value, presets::RESIZE, false);
            let remaining = if travel.step > 0.0 {
                ((eased - travel.value).abs() / travel.step).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let fill = travel.range.and_then(|(min, max)| {
                let span = max - min;
                (span > 0.0).then(|| ((eased - min) / span).clamp(0.0, 1.0))
            });
            (fill, remaining)
        }
        None => (None, 0.0),
    };

    // A row with a time area stays on one line, so its strip starts where the
    // strips of the rows above and below it start and their keyframes line up
    // in time. Every such row sizes its label from `LABEL_RESERVE` rather than
    // from its text, so the strips share one column whatever each label reads.
    // In a tight frame the strip narrows first, is left blank below its
    // minimum, and the label ends in an ellipsis, with the full name in a
    // tooltip, only once the strip has no width left. A row with no time area
    // too narrow for both, a mixer strip's, moves the readout under the label
    // rather than breaking the label mid-word.
    let column = stopwatch
        .as_ref()
        .filter(|watch| watch.strip.is_some())
        .map(|watch| watch.param);
    let timed = column.is_some();
    let mut head = div()
        .flex()
        .flex_row()
        .when(!timed, |head| head.wraps())
        .items_center()
        .gap(px(Theme::SPACE_XS));
    let mut controls = None;
    let mut strip = None;
    if let Some(mut watch) = stopwatch {
        let (navigator, time_area) = key_controls(theme, key, &mut watch, cx);
        controls = Some(navigator);
        strip = time_area;
        head = head.child(stopwatch_dot(theme, key, watch, cx));
    }
    let head = head
        .child(row_label(
            key,
            label,
            column,
            motion::mix(theme.text_muted, theme.text, hovered),
        ))
        .child(
            value_cell(column)
                .id(key.element_id("insp-value"))
                .when_some(fault.clone(), |el, fault| {
                    el.with_tip(crate::tooltip::text_builder(fault))
                })
                .child(value_readout::readout(
                    theme,
                    state,
                    ScrubTarget {
                        key,
                        value: readout.value,
                        unit: readout.unit,
                        kind: readout.kind,
                        write: std::rc::Rc::new(write),
                    },
                    readout.text,
                    Look {
                        size: Theme::TEXT_UI,
                        // Leading while the value is still travelling is what
                        // makes a change visible without restarting an
                        // animation: a value that keeps changing stays lit
                        // until it stops, rather than strobing once per frame.
                        // A failed driver is red instead.
                        ink: if fault.is_some() {
                            theme.danger
                        } else {
                            motion::mix(theme.value, theme.accent, lead)
                        },
                    },
                    cx,
                )),
        )
        .children(controls)
        .children(strip);

    let mut row = div()
        .id(key.element_id("insp-row"))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.glass_hover(),
        ))
        .on_hover(motion::hover_listener(key))
        .when(pick.is_some(), |el| {
            el.cursor(CursorStyle::PointingHand)
                .border_1()
                .border_color(theme.accent)
        })
        .child(head);
    if let Some(fill) = fill {
        row = row.child(track(theme, fill, hovered));
    }
    if let Some((line, param)) = driver_line {
        row = row.child(driver_source(theme, key, param, line, cx));
    }
    // A waiting pick whip takes the press on the row as a reference to the
    // property rather than an edit of it.
    row.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            let Some(param) = pick else {
                return;
            };
            if !this.state.read(cx).driver_picking() {
                return;
            }
            this.state.update(cx, |state, _| {
                if let Err(err) = state.pick_driver_input(param) {
                    state.report_error(&err);
                }
            });
            cx.notify();
        }),
    )
}

/// The driver's source under a driven row: a click opens it for editing, and
/// while it is open the pick whip beside it waits for a property row.
fn driver_source(
    theme: &Theme,
    key: motion::MotionKey,
    param: ClipParam,
    line: DriverLine,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let colour = if line.fault.is_some() {
        theme.danger
    } else {
        theme.text_muted
    };
    let text: SharedString = if line.editing {
        format!("{}\u{258f}", line.source).into()
    } else {
        line.source.clone()
    };
    let source = div()
        .id(key.with("driver").element_id("insp-driver"))
        .debug_selector(move || format!("insp-driver-{param:?}"))
        .flex_auto()
        .min_w_0()
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .font_family(theme.font_mono.clone())
        .text_size(Theme::TEXT_TITLE)
        .text_color(colour)
        .cursor(CursorStyle::IBeam)
        .when(line.editing, |el| {
            el.bg(theme.input_bg).border_1().border_color(theme.accent)
        })
        .when_some(line.fault.clone(), |el, fault| {
            el.with_tip(crate::tooltip::text_builder(fault))
        })
        .child(text)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                // The row underneath nudges on the same button.
                cx.stop_propagation();
                this.state
                    .update(cx, |state, _| state.open_driver_draft(param));
                cx.notify();
            }),
        );
    let whip = line.editing.then(|| {
        crate::components::ghost_button(("param-whip", key.with("whip")), "@", theme)
            .when(line.picking, |el| {
                el.bg(theme.accent).text_color(theme.text)
            })
            .when_some(
                crate::tooltip::builder("inspector-driver-whip"),
                |el, tip| el.with_tip(tip),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.state.update(cx, |state, _| state.toggle_driver_pick());
                    cx.notify();
                }),
            )
    });
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(Theme::SPACE_XS))
        .child(source)
        .children(whip)
}

/// Where the value sits in its range: a rail, the part of it the value covers,
/// and the thumb where the two meet.
///
/// `hovered` brightens all three together rather than only the thumb, because
/// a lit thumb on a dim rail reads as a different control.
fn track(theme: &Theme, fill: f32, hovered: f32) -> impl IntoElement {
    let rail = motion::mix(theme.border, theme.border_strong, hovered);
    let lit = motion::mix(theme.accent, theme.accent_strong, hovered);
    let mid = (THUMB - TRACK_H) / 2.0;
    div()
        .relative()
        .w_full()
        .h(px(THUMB))
        .child(
            div()
                .absolute()
                .top(px(mid))
                .w_full()
                .h(px(TRACK_H))
                .rounded(px(TRACK_H / 2.0))
                .bg(rail),
        )
        .child(
            div()
                .absolute()
                .top(px(mid))
                .w(relative(fill))
                .h(px(TRACK_H))
                .rounded(px(TRACK_H / 2.0))
                .bg(lit),
        )
        .child(
            div()
                .absolute()
                .top(px(0.))
                .left(relative(fill))
                // Centred on the fill's edge rather than starting at it, so
                // the thumb still reads as "here" at both ends of the range.
                .ml(px(-THUMB / 2.0))
                .size(px(THUMB))
                .rounded(px(THUMB / 2.0))
                .bg(lit),
        )
}

/// A row's label. On a row with a time area, `column` is the row's property:
/// the label's basis is `LABEL_RESERVE` and it takes any room the row has past
/// that, and it shrinks at the strip's yield, so the strip has no width left
/// before the label ends in an ellipsis, with the full name in a tooltip.
/// Elsewhere its basis is its unwrapped width, so the line breaks before the
/// label is squeezed, and at zero minimum a label wider than the row wraps at
/// the width it is given.
fn row_label(
    key: motion::MotionKey,
    label: SharedString,
    column: Option<ClipParam>,
    ink: gpui::Hsla,
) -> impl IntoElement {
    div()
        .id(key.element_id("insp-label"))
        .map(|el| match column {
            Some(param) => el
                .debug_selector(move || format!("insp-label-{param:?}"))
                .flex_basis(px(LABEL_RESERVE))
                .flex_grow(1.0)
                .flex_shrink(1.0)
                .min_w_0()
                .truncate()
                .with_tip(crate::tooltip::text_builder(label.clone())),
            None => el.flex_auto().min_w_0(),
        })
        .text_size(Theme::TEXT_UI)
        .text_color(ink)
        .child(label)
}

/// The cell a row's value is drawn in, right of the label. On a row with a time
/// area, `column` is the row's property and the cell is the value column: at
/// least `VALUE_MIN` wide, with the value at its right edge.
fn value_cell(column: Option<ClipParam>) -> gpui::Div {
    let cell = div().flex_none().ml_auto();
    match column {
        Some(param) => cell
            .debug_selector(move || format!("insp-value-{param:?}"))
            .flex()
            .justify_end()
            .min_w(px(VALUE_MIN)),
        None => cell,
    }
}

/// A colour's bytes as a paint colour.
pub fn swatch(rgba: [u8; 4]) -> gpui::Hsla {
    gpui::Hsla::from(gpui::Rgba {
        r: rgba[0] as f32 / 255.0,
        g: rgba[1] as f32 / 255.0,
        b: rgba[2] as f32 / 255.0,
        a: rgba[3] as f32 / 255.0,
    })
}

/// The palette name of a colour, or its hex when it is not one of the presets,
/// so a value written by the agent or an older project still reads as
/// something. An opaque colour prints six digits, `#ff0000`, and fits the value
/// column; a translucent one adds its alpha, `#ff000080`.
pub fn color_name(rgba: [u8; 4]) -> SharedString {
    crate::app::clip_effects::PARAM_COLORS
        .iter()
        .find(|(_, _, preset)| *preset == rgba)
        .map(|(_, label, _)| SharedString::new_static(label))
        .unwrap_or_else(|| {
            let [r, g, b, a] = rgba;
            SharedString::from(if a == u8::MAX {
                format!("#{r:02x}{g:02x}{b:02x}")
            } else {
                format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
            })
        })
}

pub fn section_label(theme: &Theme, title: &str) -> impl IntoElement {
    div()
        .pt(px(8.))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_faint)
        .child(title.to_string())
}

pub fn kv_row(theme: &Theme, label: &str, value: impl Into<String>) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .justify_between()
        .text_size(Theme::TEXT_UI)
        .child(div().text_color(theme.text_muted).child(label.to_string()))
        .child(div().text_color(theme.text).child(value.into()))
}

pub fn kind_label(kind: UiClipKind) -> &'static str {
    match kind {
        UiClipKind::Media => "Media",
        UiClipKind::Audio => "Audio",
        UiClipKind::Text => "Text",
        UiClipKind::Shape => "Shape",
        UiClipKind::Generator => "Generator",
    }
}
