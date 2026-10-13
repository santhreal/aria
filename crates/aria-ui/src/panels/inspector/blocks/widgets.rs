//! Shared inspector rows and labels, used by more than one tab.
//!
//! Every numeric control in the inspector is the same row — a label, a value,
//! and two mouse buttons that nudge it — so it is drawn once, here. That is
//! what makes the motion a property of "a row" rather than of the fifteen
//! places that draw one: the catalog's properties, the transform's six, and
//! every effect knob all animate because [`stepper_row`] does.
//!
//! A row drawn from the property catalog gets the whole treatment, because the
//! catalog supplies the two things the rest cannot: a range, so there is a
//! track for the value to travel along, and a [`ClipParam`], so there is
//! something for a keyframe to be written against. A row whose caller has
//! neither still fades under the pointer and reads the same.

use aria_engine::EngineError;
use aria_models::ClipParam;
use gpui::motion::{lerp, presets};
use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px, relative,
};

use crate::app::AppState;
use crate::motion::{self, glide};
use crate::surfaces::keyframe_rows::{self, Row};
use crate::surfaces::param_units;
use crate::theme::Theme;
use crate::types::UiClipKind;

/// Thickness of a row's value track.
const TRACK_H: f32 = 3.0;

/// Diameter of the thumb riding the track.
const THUMB: f32 = 7.0;

/// Diameter of the stopwatch's ring.
const DOT: f32 = 8.0;

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

/// The keyframe affordance, and what the property's curve looks like now.
#[derive(Clone, Copy)]
pub struct Stopwatch {
    pub param: ClipParam,
    /// Whether the property carries any keyframes at all.
    pub animated: bool,
    /// Whether one of them sits under the playhead, which is what a click
    /// toggles.
    pub at_playhead: bool,
}

/// One numeric row.
///
/// Left nudges up and right nudges down, which is the contract every stepper
/// in the inspector already had. What the row adds is a report of its own
/// state: it fades under the pointer, its fill travels to a new value instead
/// of appearing at it, and its readout leads while the value is still moving.
#[allow(clippy::too_many_arguments)]
pub fn stepper_row(
    theme: &Theme,
    key: motion::MotionKey,
    label: impl Into<SharedString>,
    display: String,
    travel: Option<Travel>,
    stopwatch: Option<Stopwatch>,
    step: f32,
    nudge: impl Fn(&mut AppState, f32) -> Result<(), EngineError> + Copy + 'static,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let label = label.into();
    let hovered = motion::hover_t(key);

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

    // A row too narrow for both, a mixer strip's, moves the readout under the
    // label rather than breaking the label mid-word.
    let mut head = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(Theme::SPACE_XS));
    if let Some(watch) = stopwatch {
        head = head.child(stopwatch_dot(theme, key, watch, cx));
    }
    let head = head
        .child(
            // An automatic basis is the label's unwrapped width, so the line
            // breaks before the label is squeezed. At zero minimum a label
            // wider than the row wraps at the width it is given.
            div()
                .flex_auto()
                .min_w_0()
                .text_size(px(13.))
                .text_color(motion::mix(theme.text_muted, theme.text, hovered))
                .child(label),
        )
        .child(
            div()
                .flex_none()
                .ml_auto()
                .font_family(theme.font_mono.clone())
                .text_size(px(13.))
                // Leading while the value is still travelling is what makes a
                // change visible without restarting an animation: a value that
                // keeps changing simply stays lit until it stops, rather than
                // strobing once per frame.
                .text_color(motion::mix(theme.text, theme.accent, lead))
                .child(display),
        );

    let mut row = div()
        .id(key.element_id("insp-row"))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.glass_hover(),
        ))
        .on_hover(motion::hover_listener(key))
        .child(head);
    if let Some(fill) = fill {
        row = row.child(track(theme, fill, hovered));
    }
    row.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            apply_nudge(this, nudge, step, cx);
        }),
    )
    .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            apply_nudge(this, nudge, -step, cx);
        }),
    )
}

/// Write the nudge and redraw. The redraw retargets the row's glide, and the
/// glide store requests frames until the fill has arrived.
fn apply_nudge(
    shell: &mut crate::shell::Shell,
    nudge: impl Fn(&mut AppState, f32) -> Result<(), EngineError>,
    delta: f32,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    shell.state.update(cx, |state, _| {
        if let Err(err) = nudge(state, delta) {
            state.report_error(&err);
        }
    });
    cx.notify();
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

/// The keyframe affordance.
///
/// Three states, and the travel between them is the point: constant, animated,
/// and animated with a keyframe under the playhead — which is what a click
/// adds or removes. The dot grows into each state rather than switching, so a
/// misclick is something the operator sees happen.
fn stopwatch_dot(
    theme: &Theme,
    key: motion::MotionKey,
    watch: Stopwatch,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let param = watch.param;
    let pinned = watch.at_playhead;
    // One scalar carries the whole affordance, so the two transitions cannot
    // run at different speeds: 0 constant, 1 animated, 2 pinned.
    let target = if pinned {
        2.0
    } else if watch.animated {
        1.0
    } else {
        0.0
    };
    let at = glide::glide(key.with("watch"), target, presets::CHEVRON, false);
    let on = at.min(1.0);
    let pin = (at - 1.0).clamp(0.0, 1.0);
    let ring = motion::mix(theme.text_faint, theme.accent, on);
    let core = px(lerp(0.0, DOT - 3.0, on) + lerp(0.0, 2.0, pin));
    div()
        .id(key.with("watch").element_id("insp-key"))
        .size(px(DOT + 4.0))
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .child(
            div()
                .size(px(DOT))
                .rounded(px(DOT / 2.0))
                .border_1()
                .border_color(ring)
                .flex()
                .items_center()
                .justify_center()
                .child(div().size(core).rounded(px(DOT / 2.0)).bg(ring)),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                // The row underneath nudges on the same button; a click on the
                // dot is about the curve, not the value.
                cx.stop_propagation();
                this.state.update(cx, |state, _| {
                    let wrote = keyframe_rows::apply(
                        state,
                        Row::Key {
                            param,
                            keyed: pinned,
                        },
                    );
                    if let Err(err) = wrote {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
}

/// A row whose caller owns the write and knows nothing about the property's
/// range: effect knobs, ramp points, crop insets, animation knobs.
pub fn audio_row(
    theme: &Theme,
    key: motion::MotionKey,
    label: impl Into<SharedString>,
    value: String,
    step: f32,
    apply: impl Fn(&mut AppState, f32) -> Result<(), EngineError> + Copy + 'static,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    stepper_row(theme, key, label, value, None, None, step, apply, cx)
}

/// Every stepper row of one property group, generated from the catalog.
///
/// A block states which group it draws and gets its rows; it keeps no list of
/// the properties it owns, and it does not decide how an edit is written. That
/// is what `AppState::nudge_param` is for, and it is the reason a slider here
/// no longer erases the keyframes of the block it belongs to.
///
/// It is also why the whole catalog animates at once: the range, the keyframe
/// state and the value all arrive on the row, so [`stepper_row`] draws a track
/// and a stopwatch for every property the catalog holds without any group
/// opting in, and a property added to the catalog arrives animated.
///
/// A property the model stores in units nobody reads it in carries an entry in
/// [`crate::surfaces::param_units`], and that row is drawn in the units it
/// declares, with its own step and its own write. Such a row keeps its stopwatch
/// and loses its track: the value no longer travels along the range the catalog
/// declared, and a track drawn from that range would put the thumb in the wrong
/// place.
///
/// The state entity is passed in rather than read back off the shell: a panel
/// draws inside an update of the shell entity, so reading that entity from here
/// panics with "cannot read Shell while it is already being updated".
pub fn param_rows(
    theme: &Theme,
    group: &str,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let state = gpui::Entity::clone(state);
    let rows = state.read(cx).param_rows(group);
    let mut elements = Vec::with_capacity(rows.len());
    for row in rows {
        let param = row.param;
        let component = row.component;
        let stopwatch = Stopwatch {
            param,
            animated: row.animated,
            at_playhead: row.at_playhead,
        };
        let key = motion::MotionKey::new("insp.param", &row.id);
        if let Some(units) = param_units::of_param(group, param) {
            let read = units.read;
            let write = units.write;
            elements.push(
                stepper_row(
                    theme,
                    key,
                    row.label.clone(),
                    read(row.value),
                    None,
                    Some(stopwatch),
                    units.step,
                    write,
                    cx,
                )
                .into_any_element(),
            );
            continue;
        }
        let step = row.step();
        let travel = Travel {
            value: row.value,
            step,
            range: match (row.range.min, row.range.max) {
                (Some(min), Some(max)) => Some((min, max)),
                _ => None,
            },
        };
        elements.push(
            stepper_row(
                theme,
                key,
                row.label.clone(),
                row.display(),
                Some(travel),
                Some(stopwatch),
                step,
                move |state, delta| state.nudge_param(param, component, delta),
                cx,
            )
            .into_any_element(),
        );
    }
    elements
}

pub fn section_label(theme: &Theme, title: &str) -> impl IntoElement {
    div()
        .pt(px(8.))
        .text_size(px(11.))
        .text_color(theme.text_faint)
        .child(title.to_string())
}

pub fn kv_row(theme: &Theme, label: &str, value: impl Into<String>) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .justify_between()
        .text_size(px(13.))
        .child(div().text_color(theme.text_muted).child(label.to_string()))
        .child(
            div()
                .font_family(theme.font_mono.clone())
                .text_color(theme.text)
                .child(value.into()),
        )
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
